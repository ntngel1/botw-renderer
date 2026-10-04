#!/usr/bin/env python3
"""Make a Cemu GLSL shader dump readable: literal floats, plain products,
no bit-cast wrappers, and names from the shader archive's manifest.

`uf_remapped{PS,VS}[i].c` become the manifest's uniform names (through
`cemu_uniform_map`'s constant-cache pairing with the native program), or
`<block>[vec4].c` where no uniform covers the slot; `textureUnit{PS,VS}n`
become sampler names by the program's `sampler_locations`. The result is
for reading only: bit-casts are dropped, so integer tricks (`& 0xe0`,
`>> 3`) read as if on floats, and nothing is evaluated.

    cemu_glsl_simplify.py MANIFEST MODEL PROGRAM GLSL [--code CODE] [--stage PS]

MODEL and PROGRAM are the manifest's indices (e.g. `uking_sys` model 1
program 28); without `--code` the uniforms keep their Cemu names.
"""
from __future__ import annotations

import argparse
import re
import struct
from pathlib import Path

from cemu_uniform_map import match, native_groups

# Floats per slot count for the manifest's uniform types (the descriptor's
# byte 6): float, vec2, vec3, vec4.
TYPE_FLOATS = {0x04: 1, 0x09: 2, 0x0a: 3, 0x0b: 4}


def read_manifest(text: str, model: int, program: int) -> dict:
    """Samplers, blocks with their uniforms, and the program's locations."""
    current, blocks, samplers, block = None, [], [], None
    locations = {}
    in_program = False
    for line in text.splitlines():
        m = re.match(r'model (\d+) ', line)
        if m:
            current = int(m.group(1))
            in_program = False
            continue
        if current != model:
            continue
        m = re.match(r'  sampler (\S+) @', line)
        if m:
            samplers.append(m.group(1))
            continue
        m = re.match(r'  block (\S+) @', line)
        if m:
            block = dict(name=m.group(1), uniforms=[])
            blocks.append(block)
            continue
        m = re.match(r'    uniform (\S+) @\S+ \[(.*)\]', line)
        if m and block is not None:
            b = [int(x, 16) for x in m.group(2).split(', ')]
            offset = (b[8] << 8 | b[9]) - 1
            if offset >= 0:
                floats = TYPE_FLOATS.get(b[6], 4) * max(b[5], 1)
                block['uniforms'].append((m.group(1), offset, floats))
            continue
        m = re.match(r'  program (\d+) ', line)
        if m:
            in_program = int(m.group(1)) == program
            continue
        m = re.match(r'    (sampler|block)_locations=\[(.*)\]', line)
        if m and in_program:
            locations[m.group(1)] = [int(x, 16) for x in m.group(2).split(', ')]
    if 'sampler' not in locations or 'block' not in locations:
        raise ValueError(f'model {model} program {program}: no locations in the manifest')
    return dict(samplers=samplers, blocks=blocks, locations=locations)


def slot_names(block: dict) -> dict:
    """(vec4, component) → uniform name, for every float a uniform covers."""
    names = {}
    for name, offset, floats in block['uniforms']:
        for k in range(floats):
            at = offset // 4 + k
            label = name if floats == 1 else f'{name}.{"xyzw"[k]}' if floats <= 4 else f'{name}[{k}]'
            names.setdefault((at // 4, at % 4), label)
    return names


def stage_index(stage: str) -> int:
    # Each location entry is four bytes: vertex, geometry, pixel, compute.
    return {'VS': 0, 'PS': 2}[stage]


def uniform_names(manifest: dict, mapping: dict, stage: str) -> dict:
    """`uf_remapped` index → (block name, vec4, slot names) of its bank."""
    at = stage_index(stage)
    banks = {}
    for i, block in enumerate(manifest['blocks']):
        entry = manifest['locations']['block'][4 * i: 4 * i + 4]
        if len(entry) == 4 and entry[at] != 0xff:
            banks[entry[at]] = block
    result = {}
    for index, source in mapping.items():
        block = banks.get(source['bank'])
        if block is None:
            continue
        result[int(index)] = (block['name'], source['vec4'], slot_names(block))
    return result


def sampler_names(manifest: dict, stage: str) -> dict:
    at = stage_index(stage)
    names = {}
    for i, name in enumerate(manifest['samplers']):
        entry = manifest['locations']['sampler'][4 * i: 4 * i + 4]
        if len(entry) == 4 and entry[at] != 0xff:
            names[entry[at]] = name
    return names


def call_spans(text: str, name: str):
    """(start, args start, end) of each `name(...)` call, innermost last."""
    spans = []
    for m in re.finditer(rf'\b{name}\(', text):
        depth, i = 1, m.end()
        while depth and i < len(text):
            depth += {'(': 1, ')': -1}.get(text[i], 0)
            i += 1
        spans.append((m.start(), m.end(), i))
    return spans


def split_args(args: str) -> list:
    parts, depth, start = [], 0, 0
    for i, c in enumerate(args):
        depth += {'(': 1, ')': -1}.get(c, 0)
        if c == ',' and depth == 0:
            parts.append(args[start:i].strip())
            start = i + 1
    parts.append(args[start:].strip())
    return parts


def rewrite_calls(text: str, name: str, rewrite) -> str:
    # One call at a time, from the last opening: inner calls go first.
    while True:
        spans = call_spans(text, name)
        if not spans:
            return text
        start, args, end = spans[-1]
        text = text[:start] + rewrite(split_args(text[args:end - 1])) + text[end:]


def literal(match_: re.Match) -> str:
    value = struct.unpack('<f', struct.pack('<I', int(match_.group(1), 16)))[0]
    text = f'{value:.7g}'
    return text if re.search(r'[.e]', text) else text + '.0'


def simplify(glsl: str, uniforms: dict | None = None, samplers: dict | None = None, stage: str = 'PS') -> str:
    text = re.sub(r'intBitsToFloat\((0x[0-9a-fA-F]{8})\)', literal, glsl)
    for cast in ('floatBitsToInt', 'intBitsToFloat', 'floatBitsToUint', 'uintBitsToFloat'):
        text = rewrite_calls(text, cast, lambda a: a[0] if len(a) == 1 else ', '.join(a))
    text = rewrite_calls(text, 'mul_nonIEEE', lambda a: f'({a[0]} * {a[1]})')
    text = rewrite_calls(text, 'clampFI32', lambda a: f'sat({a[0]})')
    text = re.sub(r'clamp\(([^(),]+), 0\.0, 1\.0\)', r'sat(\1)', text)
    text = re.sub(r'\b(R\d+|PV\d|PS\d|backupReg\d)[if]\b', r'\1', text)
    text = re.sub(r'\b(PV\d)[if]([xyzw])\b', r'\1\2', text)
    if uniforms:
        def name(m):
            index, channel = int(m.group(1)), 'xyzw'.index(m.group(2))
            if index not in uniforms:
                return m.group(0)
            block, vec4, slots = uniforms[index]
            return slots.get((vec4, channel), f'{block}[{vec4}].{m.group(2)}')
        text = re.sub(rf'uf_remapped{stage}\[(\d+)\]\.([xyzw])', name, text)
    if samplers:
        text = re.sub(rf'textureUnit{stage}(\d+)\b', lambda m: samplers.get(int(m.group(1)), m.group(0)), text)
    return text


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('manifest', type=Path)
    parser.add_argument('model', type=int)
    parser.add_argument('program', type=int)
    parser.add_argument('glsl', type=Path)
    parser.add_argument('--code', type=Path, help='native shader code, to name the uniforms')
    parser.add_argument('--stage', choices=['PS', 'VS'], default='PS')
    args = parser.parse_args()
    try:
        glsl = args.glsl.read_text().replace('\r\n', '\n')
        manifest = read_manifest(args.manifest.read_text(), args.model, args.program)
        uniforms = None
        if args.code:
            mapping = match(native_groups(args.code.read_bytes()), glsl, args.stage)['mapping']
            uniforms = uniform_names(manifest, mapping, args.stage)
        print(simplify(glsl, uniforms, sampler_names(manifest, args.stage), args.stage))
    except (OSError, ValueError) as error:
        parser.error(str(error))


if __name__ == '__main__':
    main()
