#!/usr/bin/env python3
"""Map a Cemu shader's `uf_remapped{PS,VS}[i]` to native constant-cache banks.

Walks the native R600/R700 control-flow program, collects every ALU source
that reads a locked constant cache (selectors 128–191) in ALU-group order,
and pairs it with the `uf_remapped*[i].c` references of the same group in
the exactly matching Cemu GLSL. The pairing must be channel-exact and
bijective or the tool refuses. It reads operands only: no instruction is
evaluated and no frame uniform value is claimed.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import struct
from pathlib import Path

ALU_CF = range(8, 16)


def alu_clauses(code: bytes) -> list:
    """(offset, end, banks, bases) of each ALU clause, in program order."""
    clauses, pc = [], 0
    while pc + 8 <= len(code):
        w0, w1 = struct.unpack_from('<II', code, pc)
        if w1 >> 29 & 1 and (w1 >> 26 & 15) in ALU_CF:
            offset = (w0 & 0x3fffff) * 8
            count = (w1 >> 18 & 127) + 1
            banks = [w0 >> 22 & 15, w0 >> 26 & 15]
            bases = [(w1 >> 2 & 255) * 16, (w1 >> 10 & 255) * 16]
            clauses.append((offset, offset + count * 8, banks, bases))
        elif w1 >> 21 & 1:
            break
        pc += 8
    else:
        raise ValueError('control flow without END_OF_PROGRAM')
    return clauses


def native_groups(code: bytes) -> list:
    groups = []
    for clause, (offset, end, banks, bases) in enumerate(alu_clauses(code)):
        index = 0
        while offset < end:
            refs, max_literal = [], -1
            while True:
                x, y = struct.unpack_from('<II', code, offset)
                op3 = (y >> 13 & 31) >= 8
                sources = [(x & 511, x >> 10 & 3), (x >> 13 & 511, x >> 23 & 3)]
                if op3:
                    sources.append((y & 511, y >> 10 & 3))
                for selector, channel in sources:
                    if selector == 253:
                        max_literal = max(max_literal, channel)
                    if 128 <= selector < 192:
                        cache = (selector - 128) // 32
                        refs.append(dict(bank=banks[cache], index=bases[cache] + (selector - 128) % 32, channel=channel))
                offset += 8
                if x >> 31:
                    break
            if max_literal >= 0:
                offset += 8 * (1 + max_literal // 2)
            groups.append(dict(clause=clause, group=index, uniforms=refs))
            index += 1
        if offset != end:
            raise ValueError(f'ALU clause {clause} alignment mismatch')
    return groups


def match(groups: list, glsl: str, stage: str) -> dict:
    translated = re.split(r'^// \d+\n', glsl, flags=re.M)[1:]
    if len(groups) != len(translated):
        raise ValueError(f'ALU group count mismatch: native {len(groups)}, GLSL {len(translated)}')
    mapping, reverse, occurrences = {}, {}, 0
    pattern = rf'uf_remapped{stage}\[(\d+)\]\.([xyzw])'
    for native, text in zip(groups, translated):
        refs = re.findall(pattern, text)
        if len(refs) != len(native['uniforms']):
            raise ValueError(f"reference count mismatch in clause {native['clause']} group {native['group']}")
        for (remapped, channel), source in zip(refs, native['uniforms']):
            if 'xyzw'.index(channel) != source['channel']:
                raise ValueError(f"channel mismatch in clause {native['clause']} group {native['group']}")
            key, remapped = (source['bank'], source['index']), int(remapped)
            if mapping.get(remapped, key) != key or reverse.get(key, remapped) != remapped:
                raise ValueError(f'non-bijective mapping at uf_remapped{stage}[{remapped}]')
            mapping[remapped], reverse[key] = key, remapped
            occurrences += 1
    return dict(mapping={str(k): dict(bank=v[0], vec4=v[1]) for k, v in sorted(mapping.items())},
                references=occurrences, alu_groups=len(groups))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('code', type=Path, help='native shader code (.code)')
    parser.add_argument('glsl', type=Path, help='matching Cemu GLSL dump (.txt)')
    parser.add_argument('--stage', choices=['PS', 'VS'], default='PS')
    args = parser.parse_args()
    code, glsl = args.code.read_bytes(), args.glsl.read_bytes()
    try:
        result = match(native_groups(code), glsl.decode().replace('\r\n', '\n'), args.stage)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    print(json.dumps(dict(code_sha256=hashlib.sha256(code).hexdigest(),
                          glsl_sha256=hashlib.sha256(glsl).hexdigest(), **result), indent=2))


if __name__ == '__main__':
    main()
