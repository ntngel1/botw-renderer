#!/usr/bin/env python3
"""Which inputs can reach each output of a Cemu-generated GLSL shader.

A navigation aid, not a semantics: every assignment is treated as executed
(active masks and predicates are ignored), so a listed input *may* reach an
output; an unlisted one cannot through the recognised statements. Inputs that
only form texture coordinates are not listed (only the fetched texture is).
"""
import argparse
import re

LANES = 'xyzw'

def parse(text):
    lines = text.split('\n')
    start = next(i for i, l in enumerate(lines) if l.startswith('void main()'))
    return lines[start:]

reg_ref = re.compile(r'\b(R\d+i|PV[01]i[xyzw]|PS[01]i|backupReg\di|tempi|R\d+f|tempResultf|tempResulti)(?:\.([xyzw]+))?')

def refs(expr):
    out = []
    for m in reg_ref.finditer(expr):
        name, sw = m.group(1), m.group(2)
        if name.startswith('PV') or name.startswith('PS') or name.startswith('backupReg') or name in ('tempResultf','tempResulti'):
            out.append(name)
        elif sw:
            out += [f'{name}.{c}' for c in sw]
        else:
            out += [f'{name}.{c}' for c in LANES]
    return out

def analyze(text):
    deps = {}  # var -> frozenset of leaf sources
    def get(v):
        return deps.get(v, frozenset([f'?{v}']))
    for line in parse(text):
        line = line.strip()
        # pass-parameter loads
        m = re.match(r'(R\d+i) = floatBitsToInt\((passParameterSem\d+)\);', line)
        if m:
            for c in LANES: deps[f'{m.group(1)}.{c}'] = frozenset([f'{m.group(2)}.{c}'])
            continue
        m = re.match(r'(R\d+i)\.([xyzw]+) = floatBitsToInt\((texture\w*)\((textureUnitPS\d+), (.*)\)\.([xyzw]+)\);', line)
        if not m:
            m2 = re.match(r'(R\d+i)\.([xyzw]+) = floatBitsToInt\((texture\w*)\((textureUnitPS\d+), (.*)\)\);', line)
            if m2:
                m = m2
        if m:
            dst, dsw, fn, unit, args = m.group(1), m.group(2), m.group(3), m.group(4), m.group(5)
            ssw = m.group(6) if m.lastindex >= 6 else 'x' * len(dsw)
            coord = frozenset().union(*[get(r) for r in refs(args)]) if refs(args) else frozenset()
            for d, s in zip(dsw, ssw):
                deps[f'{dst}.{d}'] = frozenset([f'{unit}.{s}']) | frozenset(f'coord:{x}' for x in coord if not x.startswith('coord:'))
            continue
        m = re.match(r'([A-Za-z0-9_]+(?:\.[xyzw]+)?) = (.*);$', line)
        if not m: continue
        lhs, rhs = m.group(1), m.group(2)
        if lhs.startswith('passPixelColor'):
            deps[lhs] = frozenset().union(*[get(r) for r in refs(rhs)])
            continue
        if not reg_ref.match(lhs): continue
        src = frozenset(u for u in re.findall(r'uf_\w+\[\d+\]\.[xyzw]', rhs))
        for r in refs(rhs): src |= get(r)
        name = lhs.split('.')[0]
        sw = lhs.split('.')[1] if '.' in lhs else None
        if sw and len(sw) > 1:
            for c in sw: deps[f'{name}.{c}'] = src
        elif sw:
            deps[f'{name}.{sw}'] = src
        else:
            deps[name] = src
        if name == 'tempi':
            for c in LANES: deps[f'tempi.{c}'] = src
    return deps

def summary(text):
    """Per output: texture units, varyings and uniform lanes it may depend on."""
    deps = analyze(text)
    result = {}
    for key in sorted(k for k in deps if k.startswith('passPixelColor')):
        leaves = deps[key]
        result[key] = dict(
            textures=sorted({x.split('.')[0] for x in leaves if x.startswith('textureUnit')}),
            varyings=sorted({x for x in leaves if x.startswith('passParameter')}),
            uniforms=sorted({x for x in leaves if x.startswith('uf_')}))
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('glsl')
    args = parser.parse_args()
    with open(args.glsl) as f:
        for output, info in summary(f.read()).items():
            print(output)
            for kind, values in info.items():
                print(f'  {kind}: {" ".join(values)}')
