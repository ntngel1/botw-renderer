#!/usr/bin/env python3
"""Fold a simplified Cemu GLSL dump (output of `cemu_glsl_simplify` or
`eft_shaders`) into something closer to source code.

- The active-mask stack (`activeMaskStack*`, `predResult`) is tracked
  symbolically: every predicate gets a name (`bool P3 = …;`) and every
  masked block becomes `if (P1 && !P3) { … }`.
- Clause temporaries (`PV*`, `PS*`, `R122`–`R127`, `backupReg*`, `tempi`)
  are substituted into the expressions that read them; a temporary read
  more than once with a long value becomes `tN`. Values are materialized
  before any register they read is overwritten, so the folding keeps the
  sequential meaning of the statements.
- Attribute decoding (`attrDecoder`) collapses to `R9 = sysPosAttr;`.

A reading aid only: nothing is evaluated, bit casts were already dropped by
the simplifier, and unknown statements are passed through. Loops (`while`)
are passed through without mask tracking inside.

    cemu_glsl_fold.py GLSL
"""
from __future__ import annotations

import argparse
import re
import struct
import sys

TEMP = re.compile(r'\b(?:PV\d[xyzw]|PS\d|backupReg\d|tempResult[fi]|(?:R12[2-7]|tempi|tempf)\.[xyzw])(?![\w.])')
REG_READ = re.compile(r'\b(R\d+)(?:\.([xyzw]+))?\b')
ASSIGN = re.compile(r'^([A-Za-z_]\w*(?:\.[xyzw]+)?)\s*=\s*(.*);$')
MASK = re.compile(r'^activeMaskStack(C?)\[(\d+)\]\s*=\s*(.*);$')

class F:
    """A boolean formula over named predicates, as a truth table."""

    def __init__(self, names=(), table=1):
        self.names, self.table = tuple(names), table

    @staticmethod
    def const(value):
        return F((), 1 if value else 0)

    @staticmethod
    def lit(name):
        return F((name,), 0b10)

    def expand(self, names):
        # Truth table over `names` (a superset): bit i = value at assignment i.
        table = 0
        for i in range(1 << len(names)):
            j = 0
            for k, n in enumerate(self.names):
                if i >> names.index(n) & 1:
                    j |= 1 << k
            if self.table >> j & 1:
                table |= 1 << i
        return table

    def combine(self, other, op):
        names = tuple(sorted(set(self.names) | set(other.names), key=lambda n: int(n[1:])))
        a, b = self.expand(names), other.expand(names)
        full = (1 << (1 << len(names))) - 1
        return F(names, op(a, b) & full).reduce()

    def reduce(self):
        # Drop predicates the value does not depend on.
        for k, n in enumerate(self.names):
            keep = [m for m in self.names if m != n]
            same = True
            for i in range(1 << len(self.names)):
                if (self.table >> i & 1) != (self.table >> (i ^ (1 << k)) & 1):
                    same = False
                    break
            if same:
                table = 0
                for i in range(1 << len(keep)):
                    j = 0
                    for kk, m in enumerate(keep):
                        if i >> kk & 1:
                            j |= 1 << self.names.index(m)
                    if self.table >> j & 1:
                        table |= 1 << i
                return F(keep, table).reduce()
        return self

    def __and__(self, other):
        return self.combine(other, lambda a, b: a & b)

    def __or__(self, other):
        return self.combine(other, lambda a, b: a | b)

    def __invert__(self):
        return F(self.names, ~self.table & ((1 << (1 << len(self.names))) - 1))

    def is_false(self):
        return self.table == 0

    def is_true(self):
        return self.table == (1 << (1 << len(self.names))) - 1

    def __eq__(self, other):
        names = tuple(sorted(set(self.names) | set(other.names)))
        return self.expand(names) == other.expand(names)

    def __hash__(self):
        return 0

    def __str__(self):
        if self.is_false():
            return 'false'
        if self.is_true():
            return 'true'
        n = len(self.names)
        minterms = [i for i in range(1 << n) if self.table >> i & 1]
        # Prime implicants by merging (Quine-McCluskey), then a greedy cover.
        terms = {(i, 0) for i in minterms}
        primes = set()
        while terms:
            merged, used = set(), set()
            for a in terms:
                for b in terms:
                    if a[1] == b[1] and bin(a[0] ^ b[0]).count('1') == 1 and a[0] < b[0]:
                        merged.add((a[0] & b[0], a[1] | (a[0] ^ b[0])))
                        used |= {a, b}
            primes |= terms - used
            terms = merged
        cover, left = [], set(minterms)
        for p in sorted(primes, key=lambda p: -bin(p[1]).count('1')):
            covered = {m for m in left if m & ~p[1] == p[0]}
            if covered:
                cover.append(p)
                left -= covered
        parts = []
        for value, mask in cover:
            lits = [('' if value >> k & 1 else '!') + self.names[k] for k in range(n) if not mask >> k & 1]
            parts.append(' && '.join(lits) if lits else 'true')
        return ' || '.join(f'({p})' if ' && ' in p and len(parts) > 1 else p for p in parts)


TRUE = F.const(True)
FALSE = F.const(False)


def conj(*parts):
    out = TRUE
    for p in parts:
        out = out & p
    return out


def negate(f):
    return ~f


def show(f) -> str:
    return str(f)


def atomic(expr: str) -> bool:
    if re.fullmatch(r'-?[\w.]+', expr):
        return True
    if re.fullmatch(r'[\w.]+\(.*\)', expr) and balanced_call(expr):
        return True
    return False


def balanced_call(expr: str) -> bool:
    depth = 0
    for i, c in enumerate(expr):
        depth += {'(': 1, ')': -1}.get(c, 0)
        if depth == 0 and c == ')' and i != len(expr) - 1:
            return False
    return depth == 0


def strip_parens(expr: str) -> str:
    while expr.startswith('(') and expr.endswith(')') and balanced_call('x' + expr):
        expr = expr[1:-1].strip()
    return expr


def _float_literal(m):
    value = int(m.group(0), 16)
    if 0x30000000 <= value & 0x7FFFFFFF <= 0x4F800000:
        f = struct.unpack('<f', struct.pack('<I', value))[0]
        text = f'{f:.7g}'
        return text if re.search(r'[.e]', text) else text + '.0'
    return m.group(0)


def pretty(line: str) -> str:
    """Cosmetic rewrites of patterns the R600 code generator produces."""
    for _ in range(4):
        # SETGT/SETGT pair: sign(x).
        line = re.sub(r'\(\(([^?]+?) > 0\.0\)\?1\.0:0\.0\) \+ -\(\(\(0\.0 > \1\)\?1\.0:0\.0\)\)', r'sign(\1)', line)
        line = re.sub(r'\(\(\(([^?]+?) > 0\.0\)\?1\.0:0\.0\) \+ -\(\(\(0\.0 > \1\)\?1\.0:0\.0\)\)\)', r'sign(\1)', line)
        # -(a) + b → b - a for simple operands.
        line = re.sub(r'-\(([\w.]+)\) \+ ([\w.]+)\b(?![(.\[])', r'\2 - \1', line)
        line = re.sub(r'\+ -\(([\w.]+)\)', r'- \1', line)
        line = re.sub(r'\+ -([\d.]+)\b', r'- \1', line)
    # Hex words that are float operands (not bit masks or integer compares).
    line = re.sub(r'(?<![&|^] )(?<![=!]= )(?<![&|^])\b0x[0-9a-fA-F]{8}\b(?! *[&|^]| *[=!]=)', _float_literal, line)
    return line


class Folder:
    def __init__(self):
        self.out = []
        self.masks = {}          # ('S'|'C', i) -> formula
        self.pred = TRUE         # formula of the last predResult
        self.npred = 0
        self.ntemp = 0
        self.env = {}            # temp -> expression
        self.uses = {}           # temp -> remaining expected uses
        self.cond = TRUE
        self.indent = 0

    # -- temporaries --------------------------------------------------------
    def substitute(self, expr: str) -> str:
        def repl(m):
            name = m.group(0)
            if name not in self.env:
                return name
            value = self.env[name]
            return value if atomic(value) else f'({value})'
        return TEMP.sub(repl, expr)

    def materialize_readers(self, register: str, components: str):
        """Before `register.components` changes, fix the temps that read it."""
        for temp, value in list(self.env.items()):
            for m in REG_READ.finditer(value):
                if m.group(1) == register and (not m.group(2) or not components or set(m.group(2)) & set(components)):
                    self.ntemp += 1
                    name = f't{self.ntemp}'
                    self.emit(f'float {name} = {strip_parens(value)};')
                    self.env[temp] = name
                    break

    def emit(self, line: str):
        self.out.append('  ' * self.indent + pretty(line))

    def flush_block(self):
        self.env = {}

    # -- statements ---------------------------------------------------------
    def statement(self, line: str, uses_after):
        m = MASK.match(line)
        if m:
            self.mask(m.group(1), int(m.group(2)), m.group(3))
            return
        if line.startswith('predResult'):
            m = ASSIGN.match(line)
            self.npred += 1
            name = f'P{self.npred}'
            self.emit(f'bool {name} = {strip_parens(self.substitute(m.group(2)))};')
            self.pred = F.lit(name)
            return
        m = re.match(r'^([\w.]+) ([*/+-])= (.*);$', line)
        if m:
            line = f'{m.group(1)} = {m.group(1)} {m.group(2)} ({m.group(3)});'
        if re.match(r'^if\( isinf\(\w+\) == true \) \w+ = -3\.40282347E\+38F;$', line):
            return  # log2(0) clamped to -FLT_MAX instead of -inf
        m = ASSIGN.match(line)
        if not m:
            self.emit(self.substitute(line))
            return
        lhs, rhs = m.group(1), self.substitute(m.group(2))
        base, _, comps = lhs.partition('.')
        if TEMP.fullmatch(lhs):
            count = uses_after(lhs)
            rhs = strip_parens(rhs)
            if count == 0:
                return
            if count > 1 and len(rhs) > 40:
                self.ntemp += 1
                name = f't{self.ntemp}'
                self.emit(f'float {name} = {rhs};  // {lhs}')
                rhs = name
            # A temp redefined: readers of its old value were substituted already.
            self.env[lhs] = rhs
            return
        if re.fullmatch(r'R\d+', base):
            self.materialize_readers(base, comps)
        self.emit(f'{lhs} = {strip_parens(rhs)};')

    def mask(self, kind: str, index: int, value: str):
        value = value.strip()
        key = ('C' if kind else 'S', index)
        if value == 'true':
            f = TRUE
        elif value == 'false':
            f = FALSE
        elif value == 'predResult':
            f = self.pred
        else:
            m = re.fullmatch(r'activeMaskStack(C?)\[(\d+)\]', value)
            if m:
                f = self.masks.get(('C' if m.group(1) else 'S', int(m.group(2))), TRUE)
            else:
                m = re.fullmatch(r'activeMaskStack\[(\d+)\] == false', value)
                if m:
                    f = negate(self.masks.get(('S', int(m.group(1))), TRUE))
                else:
                    m = re.fullmatch(r'(predResult|activeMaskStack\[(\d+)\]) == true && activeMaskStackC\[(\d+)\] == true', value)
                    if not m:
                        raise ValueError(f'unknown mask statement: {value}')
                    left = self.pred if m.group(1) == 'predResult' else self.masks.get(('S', int(m.group(2))), TRUE)
                    f = conj(left, self.masks.get(('C', int(m.group(3))), TRUE))
        self.masks[key] = f

    def run(self, text: str) -> str:
        lines = [l.strip() for l in text.split('\n')]
        start = next(i for i, l in enumerate(lines) if l.startswith('void main'))
        head = lines[:start]
        body = lines[start + 2:]
        # Drop declarations.
        decl = re.compile(r'^(?:const )?(?:ivec4|vec4|uvec4|int|float|bool|vec3)\s+[\w\[\], =.()0-9-]*;$')
        body = [l for l in body if l and not decl.match(l) and not l.startswith('//')]
        if body and body[-1] == '}':
            body = body[:-1]
        body = self.collapse_attributes(body)
        # Masked blocks: `if( activeMaskStackC[n] == true ) {` … `}`
        # optionally followed by `else {` … `}` with mask statements only.
        i = 0
        while i < len(body):
            line = body[i]
            m = re.match(r'^if\( activeMaskStackC\[(\d+)\] == true \) \{$', line)
            if m:
                guard = self.masks.get(('C', int(m.group(1))), TRUE)
                end = self.block_end(body, i)
                inner = body[i + 1:end]
                saved = dict(self.masks)
                self.flush_block()
                if guard.is_true():
                    self.lines(inner)
                elif not guard.is_false():
                    self.emit(f'if ({show(guard)}) {{')
                    self.indent += 1
                    self.lines(inner)
                    self.indent -= 1
                    self.emit('}')
                self.flush_block()
                if_masks = self.masks
                i = end + 1
                else_masks = dict(saved)
                if i < len(body) and body[i] == 'else {':
                    end2 = self.block_end(body, i)
                    self.masks = else_masks
                    for l in body[i + 1:end2]:
                        mm = MASK.match(l)
                        if not mm:
                            raise ValueError(f'unexpected statement in a mask else: {l}')
                        self.mask(mm.group(1), int(mm.group(2)), mm.group(3))
                    i = end2 + 1
                merged = {}
                for k in set(if_masks) | set(else_masks):
                    a, b = if_masks.get(k, saved.get(k, TRUE)), else_masks.get(k, saved.get(k, TRUE))
                    merged[k] = (guard & a) | (~guard & b)
                self.masks = merged
                continue
            if line.startswith('while('):
                end = self.block_end(body, i)
                self.emit('// loop (mask tracking stops here)')
                for l in body[i:end + 1]:
                    self.emit(l)
                i = end + 1
                continue
            # A straight run of statements up to the next block.
            j = i
            while j < len(body) and not body[j].startswith('if( activeMaskStackC') and not body[j].startswith('while('):
                j += 1
            self.lines(body[i:j])
            i = j
        return '\n'.join(self.out)

    def lines(self, lines):
        for k, line in enumerate(lines):
            def uses_after(var, k=k):
                n = 0
                pattern = re.compile(rf'(?<![\w.]){re.escape(var)}(?![\w])')
                for later in lines[k + 1:]:
                    m = ASSIGN.match(later)
                    rhs = m.group(2) if m else later
                    n += len(pattern.findall(rhs))
                    if m and m.group(1) == var:
                        break
                return n
            if line.startswith('if(') and 'activeMaskStackC' in line:
                raise ValueError('nested masked block')
            self.statement(line, uses_after)

    @staticmethod
    def block_end(body, i):
        depth = 0
        for j in range(i, len(body)):
            depth += body[j].count('{') - body[j].count('}')
            if depth == 0:
                return j
        raise ValueError('unbalanced block')

    @staticmethod
    def collapse_attributes(body):
        out, source = [], None
        for line in body:
            m = re.match(r'^attrDecoder(\.xyz)? = (\w+)(\.xyz)?;$', line)
            if m and not m.group(2).startswith('attrDecoder'):
                source = m.group(2)
                continue
            if line.startswith('attrDecoder'):
                continue
            m = re.match(r'^(R\d+) = ivec4\(int\(attrDecoder\.x\).*\);$', line)
            if m and source:
                tail = ' (w = 1.0)' if '0x3f800000' in line else ''
                out.append(f'{m.group(1)} = {source};' + (f'  // xyz{tail}' if tail else ''))
                continue
            out.append(line)
        return out


def drop_dead(text: str) -> str:
    """Remove `float tN = …;` lines whose `tN` nothing reads."""
    lines = text.split('\n')
    while True:
        defs = {}
        for i, l in enumerate(lines):
            m = re.match(r'^\s*float (t\d+) = ', l)
            if m:
                defs[m.group(1)] = i
        used = set(re.findall(r'\bt\d+\b', '\n'.join(re.sub(r'^\s*float t\d+ = ', '', l) for l in lines)))
        dead = {i for name, i in defs.items() if name not in used}
        if not dead:
            return '\n'.join(lines)
        lines = [l for i, l in enumerate(lines) if i not in dead]


def fold(text: str) -> str:
    return drop_dead(Folder().run(text))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('glsl')
    args = parser.parse_args()
    text = open(args.glsl).read()
    sys.stdout.write(fold(text) + '\n')


if __name__ == '__main__':
    main()
