#!/usr/bin/env python3
"""Particle shaders of BotW's effect files (EFTB v20, `*.sesetlist`).

Reads the `SHDA`/`SHDB` node of a PTCL file: a GFX2 (`Gfx2`, .gsh) file with
GX2 vertex and pixel shader headers and their program code. Prints each
program's reflection (uniform blocks, samplers, attributes, the semantics
passed between the stages) and which programs each emitter selects:
emitter data `+0x914` vertex shader, `+0x918` pixel shader, `+0x91C`/`+0x920`
a second pair (0 when unused); indices into the GFX2 file's VS and PS lists.

With `--cemu`, programs whose code matches a Cemu runtime dump (`.bin`)
byte for byte get Cemu's GLSL rewritten for reading: uniform blocks and
samplers by their reflection names, `uf_remapped*` resolved to (block,
byte offset) through the native constant-cache reads (`cemu_uniform_map`),
emitter-block offsets named where known (`STATIC_FIELDS`). Written to
`--out` (outside the repository); nothing is evaluated.

    eft_shaders.py manifest FILE [--emitters]
    eft_shaders.py glsl FILE... --cemu DIR... --out DIR

FILE is a PTCL file, Yaz0-compressed or not, or `PACK:ENTRY` for a file
inside a SARC (e.g. `Bootup.pack:Effect/GameResident.sesetlist`).
"""
from __future__ import annotations

import argparse
import hashlib
import re
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cemu_glsl_fold import fold  # noqa: E402
from cemu_glsl_simplify import simplify  # noqa: E402
from cemu_uniform_map import match, native_groups  # noqa: E402


# --- containers -------------------------------------------------------------

def yaz0(data: bytes) -> bytes:
    if data[:4] != b'Yaz0':
        return data
    size = struct.unpack_from('>I', data, 4)[0]
    out, src = bytearray(), 16
    while len(out) < size:
        code = data[src]
        src += 1
        for bit in range(8):
            if len(out) >= size:
                break
            if code & (0x80 >> bit):
                out.append(data[src])
                src += 1
                continue
            b1, b2 = data[src], data[src + 1]
            src += 2
            back = ((b1 & 15) << 8 | b2) + 1
            count = b1 >> 4
            if count == 0:
                count = data[src] + 0x12
                src += 1
            else:
                count += 2
            start = len(out) - back
            for k in range(count):
                out.append(out[start + k])
    return bytes(out)


def sarc_entry(data: bytes, name: str) -> bytes:
    data = yaz0(data)
    if data[:4] != b'SARC':
        raise ValueError('not a SARC archive')
    bom = '>' if data[6:8] == b'\xfe\xff' else '<'
    data_start = struct.unpack_from(bom + 'I', data, 0xC)[0]
    sfat = 0x14
    count = struct.unpack_from(bom + 'H', data, sfat + 6)[0]
    nodes = sfat + 0xC
    sfnt = nodes + 16 * count
    names = sfnt + 8
    for i in range(count):
        _, attr, begin, end = struct.unpack_from(bom + 'IIII', data, nodes + 16 * i)
        if attr >> 24:
            at = names + (attr & 0xFFFF) * 4
            entry = data[at:data.index(b'\0', at)].decode()
            if entry == name:
                return yaz0(data[data_start + begin:data_start + end])
    raise ValueError(f'{name} not in the archive')


def load(spec: str) -> bytes:
    path, _, entry = spec.partition(':')
    data = Path(path).read_bytes()
    return sarc_entry(data, entry) if entry else yaz0(data)


def be(fmt: str, data: bytes, at: int):
    return struct.unpack_from('>' + fmt, data, at)


class Ptcl:
    """The node tree of an EFTB file (layout: botw-formats `ptcl`)."""

    def __init__(self, data: bytes):
        if data[:4] != b'EFTB' or be('I', data, 4)[0] != 20:
            raise ValueError('not an EFTB v20 file')
        self.data = data

    def node(self, at: int) -> dict:
        size, child, nxt, attr, data = be('IIIII', self.data, at + 4)
        link = lambda v: None if v == 0xFFFFFFFF else at + v  # noqa: E731
        return dict(magic=self.data[at:at + 4], at=at, size=size, child=link(child),
                    next=link(nxt), attr=link(attr), data=at + data, children=be('H', self.data, at + 28)[0])

    def siblings(self, at):
        while at is not None:
            n = self.node(at)
            yield n
            at = n['next']

    def emitters(self) -> list:
        """(set name, emitter name, data offset, depth) depth first."""
        out = []

        def walk(at, set_name, depth):
            for e in self.siblings(at):
                if e['magic'] == b'EMTR':
                    name = self.data[e['data'] + 0x10:e['data'] + 0x50].split(b'\0')[0].decode()
                    out.append((set_name, name, e['data'], depth))
                    walk(e['child'], set_name, depth + 1)

        for top in self.siblings(0x30):
            if top['magic'] == b'ESTA':
                for s in self.siblings(top['child']):
                    name = self.data[s['data'] + 0x10:s['data'] + 0x50].split(b'\0')[0].decode()
                    walk(s['child'], name, 0)
        return out

    def gfx2(self):
        """(start, end) of the GFX2 file in `SHDA`→`SHDB`, or None."""
        for top in self.siblings(0x30):
            if top['magic'] == b'SHDA':
                for c in self.siblings(top['child']):
                    if c['magic'] == b'SHDB':
                        return c['data'], c['data'] + c['size']
        return None

    def shader_indices(self, emitter_at: int) -> tuple:
        return be('IIII', self.data, emitter_at + 0x914)


# --- GFX2 -------------------------------------------------------------------

GFX2_VS_HEADER, GFX2_VS_PROGRAM, GFX2_PS_HEADER, GFX2_PS_PROGRAM, GFX2_END = 3, 5, 6, 7, 1


def gfx2_blocks(data: bytes, start: int, end: int) -> list:
    if data[start:start + 4] != b'Gfx2':
        raise ValueError('SHDB is not a GFX2 file')
    at, out = start + be('I', data, start + 4)[0], []
    while at < end:
        magic, header, _, _, kind, size = be('4sIIIII', data, at)
        if magic != b'BLK{':
            raise ValueError('bad GFX2 block')
        out.append((kind, at + header, size))
        at += header + size
        if kind == GFX2_END:
            break
    return out


def _pointer(value: int) -> int:
    # Unrelocated GFX2 pointers: 0xD06xxxxx into the header block, 0xCA7xxxxx
    # into the program block; the low 20 bits are the offset.
    if value >> 20 not in (0xD06, 0xCA7):
        raise ValueError(f'unexpected GFX2 pointer {value:#x}')
    return value & 0xFFFFF


def _cstr(h: bytes, value: int) -> str:
    at = _pointer(value)
    return h[at:h.index(b'\0', at)].decode()


def _table(h: bytes, count: int, pointer: int, stride: int, read) -> list:
    if not count:
        return []
    at = _pointer(pointer)
    return [read(at + stride * i) for i in range(count)]


def _reflection(h: bytes, at: int) -> dict:
    """Blocks, uniforms, initial values, loops and samplers from `at`
    (GX2 `numUniformBlocks` onwards)."""
    c = lambda k: be('II', h, at + 8 * k)  # noqa: E731
    return dict(
        blocks=_table(h, *c(0), 12, lambda o: (_cstr(h, be('I', h, o)[0]),) + be('II', h, o + 4)),
        uniforms=_table(h, *c(1), 20, lambda o: (_cstr(h, be('I', h, o)[0]),) + be('IIIi', h, o + 4)),
        initial_values=_table(h, *c(2), 20, lambda o: be('4fI', h, o)),
        loops=_table(h, *c(3), 8, lambda o: be('II', h, o)),
        samplers=_table(h, *c(4), 12, lambda o: (_cstr(h, be('I', h, o)[0]),) + be('II', h, o + 4)),
    )


def _semantics(words: list) -> list:
    return [w >> (8 * k) & 0xFF for w in words for k in range(4) if w >> (8 * k) & 0xFF != 0xFF]


def gfx2_shaders(data: bytes, start: int, end: int):
    """Vertex and pixel shaders with reflection and code, in file order.

    Struct layouts: WUT `gx2/shaders.h` (GX2VertexShader 0x134 bytes, size
    and program at +0xD0/+0xD4; GX2PixelShader 0xE8, +0xA4/+0xA8)."""
    blocks = gfx2_blocks(data, start, end)
    vs, ps = [], []
    for kind, at, size in blocks:
        h = data[at:at + size]
        if kind == GFX2_VS_HEADER:
            s = dict(stage='VS', size=be('I', h, 0xD0)[0], mode=be('I', h, 0xD8)[0], **_reflection(h, 0xDC))
            s['attributes'] = _table(h, *be('II', h, 0x104), 16,
                                     lambda o: (_cstr(h, be('I', h, o)[0]),) + be('III', h, o + 4))
            s['outputs'] = _semantics(list(be('10I', h, 0x10))[:be('I', h, 0x0C)[0]])
            s['vertex_semantics'] = list(be('32I', h, 0x44))[:be('I', h, 0x40)[0]]
            s['regs'] = h[:0xD0]
            vs.append(s)
        elif kind == GFX2_PS_HEADER:
            s = dict(stage='PS', size=be('I', h, 0xA4)[0], mode=be('I', h, 0xAC)[0], **_reflection(h, 0xB0))
            s['inputs'] = [w & 0xFF for w in be('32I', h, 0x14)[:be('I', h, 0x10)[0]]]
            s['regs'] = h[:0xA4]
            ps.append(s)
    codes = {GFX2_VS_PROGRAM: [], GFX2_PS_PROGRAM: []}
    for kind, at, size in blocks:
        if kind in codes:
            codes[kind].append(data[at:at + size])
    for shaders, kind in ((vs, GFX2_VS_PROGRAM), (ps, GFX2_PS_PROGRAM)):
        if len(codes[kind]) != len(shaders):
            raise ValueError('GFX2 header and program counts differ')
        for s, code in zip(shaders, codes[kind]):
            s['code'] = code[:s['size']]
            s['sha1'] = hashlib.sha1(s['code']).hexdigest()
    return vs, ps


def file_shaders(ptcl: Ptcl):
    span = ptcl.gfx2()
    return gfx2_shaders(ptcl.data, *span) if span else ([], [])


# --- naming -----------------------------------------------------------------

BLOCK_SHORT = {
    'sysViewUniformBlock': 'view', 'sysEmitterStaticUniformBlock': 'emt',
    'sysEmitterDynamicUniformBlock': 'dyn', 'sysEmitterFieldUniformBlock': 'field',
    'sysEmitterPluginUniformBlock': 'plugin', 'sysCustomShaderReservedUniformBlockParam': 'reserved',
    'sysCustomShaderUniformBlock0': 'cus0', 'sysCustomShaderUniformBlock1': 'cus1',
    'sysCustomShaderUniformBlock2': 'cus2',
}

# Byte offsets of sysEmitterStaticUniformBlock (= emitter data 0..0x750)
# whose meaning is established; see docs/research/eft-shaders.md.
STATIC_FIELDS: dict = {}
# Same for the other blocks: {block short name: {byte offset: name}}.
BLOCK_FIELDS: dict = {}


def slot_name(block: str, byte: int) -> str:
    short = BLOCK_SHORT.get(block, block)
    fields = STATIC_FIELDS if short == 'emt' else BLOCK_FIELDS.get(short, {})
    if byte in fields:
        return fields[byte]
    return f'{short}_{byte:03X}'


def match_by_channel(code: bytes, glsl: str, stage: str) -> dict:
    """`cemu_uniform_map.match`, but pairing the references of an ALU group
    per channel in order. Cemu writes DOT4's operands as two vectors (all
    of src0, then all of src1) while the native group interleaves them per
    slot; within one channel the order agrees. Still refuses anything that
    is not channel-exact and bijective."""
    try:
        return match(native_groups(code), glsl, stage)['mapping']
    except ValueError:
        pass
    groups = native_groups(code)
    translated = re.split(r'^// \d+\n', glsl, flags=re.M)[1:]
    if len(groups) != len(translated):
        raise ValueError('ALU group count mismatch')
    mapping, reverse = {}, {}
    for native, text in zip(groups, translated):
        refs = re.findall(rf'uf_remapped{stage}\[(\d+)\]\.([xyzw])', text)
        if len(refs) != len(native['uniforms']):
            raise ValueError(f"reference count mismatch in clause {native['clause']} group {native['group']}")
        for channel in range(4):
            mine = [int(i) for i, c in refs if 'xyzw'.index(c) == channel]
            theirs = [(u['bank'], u['index']) for u in native['uniforms'] if u['channel'] == channel]
            if len(mine) != len(theirs):
                raise ValueError(f"channel mismatch in clause {native['clause']} group {native['group']}")
            for remapped, key in zip(mine, theirs):
                if mapping.get(remapped, key) != key or reverse.get(key, remapped) != remapped:
                    raise ValueError(f'non-bijective mapping at uf_remapped{stage}[{remapped}]')
                mapping[remapped], reverse[key] = key, remapped
    return {str(k): dict(bank=v[0], vec4=v[1]) for k, v in mapping.items()}


def rename(glsl: str, shader: dict) -> str:
    stage = shader['stage']
    by_location = {loc: name for name, loc, _ in shader['blocks']}
    text = glsl
    if f'uf_remapped{stage}' in text:
        mapping = match_by_channel(shader['code'], text, stage)

        def remapped(m):
            source = mapping[m.group(1)]
            block = by_location.get(source['bank'], f'bank{source["bank"]}')
            return slot_name(block, 16 * source['vec4'] + 4 * 'xyzw'.index(m.group(2)))
        text = re.sub(rf'intBitsToFloat\(uf_remapped{stage}\[(\d+)\]\.([xyzw])\)', remapped, text)
        text = re.sub(rf'uf_remapped{stage}\[(\d+)\]\.([xyzw])', lambda m: 'floatBitsToInt(' + remapped(m) + ')', text)

    def block(m):
        location, index, channel = int(m.group(1)), m.group(2), m.group(3)
        name = by_location.get(location, f'block{location}')
        if index.isdigit():
            return slot_name(name, 16 * int(index) + 4 * 'xyzw'.index(channel or 'x')) + ('' if channel else '_vec4')
        return f'{BLOCK_SHORT.get(name, name)}[{index}]' + (f'.{channel}' if channel else '')
    text = re.sub(rf'uf_block{stage}(\d+)\[([^\]]+)\](?:\.([xyzw]))?', block, text)
    samplers = {loc: name for name, _, loc in shader['samplers']}
    text = re.sub(rf'textureUnit{stage}(\d+)\b', lambda m: samplers.get(int(m.group(1)), m.group(0)), text)
    if stage == 'VS':
        attributes = {loc: name for name, _, _, loc in shader['attributes']}
        text = re.sub(r'attrDataSem(\d+)\b', lambda m: attributes.get(int(m.group(1)), m.group(0)), text)
    return simplify(text, None, None, stage)


# --- commands ---------------------------------------------------------------

def describe(s: dict) -> list:
    lines = [f"  {s['stage']} size={s['size']:#x} mode={s['mode']} sha1={s['sha1']}"]
    for name, loc, size in s['blocks']:
        lines.append(f'    block {name} @{loc} {size} bytes')
    for name, kind, loc in s['samplers']:
        lines.append(f'    sampler {name} @{loc} type {kind}')
    for name, kind, count, loc in s.get('attributes', []):
        lines.append(f'    attribute {name} @{loc} type {kind}')
    for u in s['uniforms']:
        lines.append(f'    uniform {u}')
    if s['stage'] == 'VS':
        lines.append(f"    exports semantics {s['outputs']}")
    else:
        lines.append(f"    reads semantics {s['inputs']}")
    return lines


def cmd_manifest(args):
    ptcl = Ptcl(load(args.file))
    vs, ps = file_shaders(ptcl)
    out = [f'{len(vs)} vertex shaders, {len(ps)} pixel shaders']
    for i, s in enumerate(vs):
        out += [f'VS {i}'] + describe(s)
    for i, s in enumerate(ps):
        out += [f'PS {i}'] + describe(s)
    if args.emitters:
        for set_name, name, at, depth in ptcl.emitters():
            v, p, v2, p2 = ptcl.shader_indices(at)
            ok = set(ps[p]['inputs']) <= set(vs[v]['outputs'])
            out.append(f"{'  ' * depth}{set_name}/{name} @{at:#x}: VS {v} PS {p} second {v2}/{p2}"
                       + ('' if ok else ' (stage semantics disagree!)'))
    print('\n'.join(out))


def cmd_glsl(args):
    cemu = {}
    for d in args.cemu:
        for b in sorted(Path(d).glob('*.bin')):
            cemu.setdefault(hashlib.sha1(b.read_bytes()).hexdigest(), []).append(b)
    programs = {}
    for spec in args.file:
        ptcl = Ptcl(load(spec))
        vs, ps = file_shaders(ptcl)
        users = {}
        for set_name, name, at, _ in ptcl.emitters():
            v, p, v2, p2 = ptcl.shader_indices(at)
            for stage, i in (('VS', v), ('PS', p)) + ((('VS', v2), ('PS', p2)) if v2 or p2 else ()):
                users.setdefault((stage, i), []).append(f'{set_name}/{name}')
        label = Path(spec.split(':')[-1]).name.split('.')[0]
        for stage, shaders in (('VS', vs), ('PS', ps)):
            for i, s in enumerate(shaders):
                entry = programs.setdefault(s['sha1'], dict(shader=s, aliases=[]))
                entry['aliases'].append(f"{label} {stage} {i}: {', '.join(users.get((stage, i), ['(no emitter)'])[:6])}")
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    written = 0
    for sha, entry in sorted(programs.items()):
        s = entry['shader']
        texts = {}
        for b in cemu.get(sha, []):
            t = b.with_suffix('.txt')
            if t.exists():
                texts.setdefault(t.read_text().replace('\r\n', '\n'), []).append(b.stem)
        for k, (glsl, names) in enumerate(texts.items()):
            try:
                body = rename(glsl, s)
            except ValueError as error:
                body = f'// renaming failed: {error}\n' + glsl
            head = [f'// {s["stage"]} program sha1 {sha}; Cemu {", ".join(sorted(set(names)))}']
            head += [f'// used by {a}' for a in entry['aliases'][:args.aliases]]
            if len(entry['aliases']) > args.aliases:
                head.append(f'// … and {len(entry["aliases"]) - args.aliases} more')
            head += ['//' + line for line in describe(s)]
            name = f'{s["stage"].lower()}_{sha[:12]}' + (f'_{k}' if k else '')
            (out / f'{name}.glsl').write_text('\n'.join(head) + '\n' + body)
            if args.fold:
                try:
                    folded = fold(body)
                except Exception as error:  # a reading aid: keep going
                    folded = f'// fold failed: {error}'
                (out / f'{name}.fold.glsl').write_text('\n'.join(head) + '\n' + folded + '\n')
            written += 1
    print(f'{len(programs)} programs, {written} GLSL files written')


def cmd_coverage(args):
    """Per emitter: its programs and whether Cemu has dumped them."""
    dumped = set()
    for d in args.cemu:
        for b in Path(d).glob('*.bin'):
            dumped.add(hashlib.sha1(b.read_bytes()).hexdigest())
    total = both = 0
    for spec in args.file:
        ptcl = Ptcl(load(spec))
        vs, ps = file_shaders(ptcl)
        label = Path(spec.split(':')[-1]).name.split('.')[0]
        for set_name, name, at, _ in ptcl.emitters():
            v, p, _, _ = ptcl.shader_indices(at)
            ok = (vs[v]['sha1'] in dumped, ps[p]['sha1'] in dumped)
            total += 1
            both += all(ok)
            print(f"{label}\t{set_name}/{name}\tVS {v} {vs[v]['sha1'][:12]} {'dumped' if ok[0] else 'MISSING'}"
                  f"\tPS {p} {ps[p]['sha1'][:12]} {'dumped' if ok[1] else 'MISSING'}")
    print(f'# {both}/{total} emitters have both programs dumped', file=sys.stderr)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest='command', required=True)
    m = sub.add_parser('manifest')
    m.add_argument('file')
    m.add_argument('--emitters', action='store_true')
    g = sub.add_parser('glsl')
    g.add_argument('file', nargs='+')
    g.add_argument('--cemu', nargs='+', required=True, help='Cemu dump/shaders directories')
    g.add_argument('--out', required=True)
    g.add_argument('--aliases', type=int, default=12)
    g.add_argument('--fold', action='store_true', help='also write cemu_glsl_fold output (*.fold.glsl)')
    c = sub.add_parser('coverage')
    c.add_argument('file', nargs='+')
    c.add_argument('--cemu', nargs='+', required=True, help='Cemu dump/shaders directories')
    args = parser.parse_args()
    {'manifest': cmd_manifest, 'glsl': cmd_glsl, 'coverage': cmd_coverage}[args.command](args)


if __name__ == '__main__':
    main()
