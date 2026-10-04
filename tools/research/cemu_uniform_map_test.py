"""Synthetic R700 control flow and GLSL for the constant-cache mapper."""
import struct
import unittest

from cemu_uniform_map import match, native_groups


def program() -> bytes:
    # CF0: ALU clause at 64-bit slot 2, one slot, kcache0 = bank 6 from vec4
    # 32, kcache1 = bank 1 from vec4 16. CF1: END_OF_PROGRAM.
    w0 = 2 | 6 << 22 | 1 << 26
    w1 = (32 // 16) << 2 | (16 // 16) << 10 | (1 - 1) << 18 | 8 << 26
    cf = struct.pack('<II', w0, w1) + struct.pack('<II', 0, 1 << 21)
    # One OP2 instruction (last in group): src0 = kcache0[3].y, src1 =
    # kcache1[1].w, no literals.
    x = (128 + 3) | 1 << 10 | (160 + 1) << 13 | 3 << 23 | 1 << 31
    return cf + struct.pack('<II', x, 0)


class UniformMapTests(unittest.TestCase):
    def test_pairs_group_references_with_native_banks(self):
        groups = native_groups(program())
        self.assertEqual(groups[0]['uniforms'], [dict(bank=6, index=35, channel=1), dict(bank=1, index=17, channel=3)])
        glsl = '// 0\nR0i.x = uf_remappedPS[4].y * uf_remappedPS[9].w;\n// 1\n'
        result = match(groups + [dict(clause=0, group=1, uniforms=[])], glsl, 'PS')
        self.assertEqual(result['mapping'], {'4': dict(bank=6, vec4=35), '9': dict(bank=1, vec4=17)})

    def test_refuses_channel_mismatch(self):
        groups = native_groups(program()) + [dict(clause=0, group=1, uniforms=[])]
        with self.assertRaises(ValueError):
            match(groups, '// 0\nR0i.x = uf_remappedPS[4].x * uf_remappedPS[9].w;\n// 1\n', 'PS')


if __name__ == '__main__':
    unittest.main()
