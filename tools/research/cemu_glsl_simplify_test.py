"""Synthetic manifest and GLSL for the Cemu GLSL simplifier."""
import unittest

from cemu_glsl_simplify import read_manifest, sampler_names, simplify, uniform_names

MANIFEST = '''model 0 "other" offset=0x0 programs=1 static_options=0 dynamic_options=0
  sampler wrong @0x0 [00, 00, 00, 00, 00, 00, 00, 00]
model 1 "shading" offset=0x70 programs=2 static_options=0 dynamic_options=0
  sampler albedo @0x10 [00, 01, 01, 00, 00, 00, 00, 00]
  sampler depth @0x18 [01, 01, 01, 00, 00, 00, 00, 00]
  block context @0x20 [00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00]
  block material @0x30 [01, 01, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00, 00]
    uniform tint @0x40 [00, 00, 00, 00, 00, 01, 0b, 00, 00, 11, 0f, 00, 00, 00, 00, 00]
    uniform power @0x50 [00, 00, 00, 01, 00, 01, 04, 00, 00, 21, 0c, 00, 00, 00, 00, 00]
  program 0 @0x100 flags=0x0 keys=[00000000]
    sampler_locations=[ff, ff, 03, ff, ff, ff, ff, ff]
    block_locations=[ff, ff, ff, ff, ff, ff, ff, ff]
  program 1 @0x200 flags=0x0 keys=[00000000]
    sampler_locations=[ff, ff, 05, ff, 02, ff, 07, ff]
    block_locations=[ff, ff, 01, ff, ff, ff, 08, ff]
'''


class SimplifyTests(unittest.TestCase):
    def test_reads_the_programs_locations_and_uniform_slots(self):
        manifest = read_manifest(MANIFEST, 1, 1)
        self.assertEqual(sampler_names(manifest, 'PS'), {5: 'albedo', 7: 'depth'})
        # Bank 8 is `material` for the pixel shader: `tint` fills vec4 1,
        # `power` the first float of vec4 2 (offsets are the descriptor's
        # bytes 8–9 minus one).
        uniforms = uniform_names(manifest, {'3': dict(bank=8, vec4=1), '4': dict(bank=8, vec4=2), '5': dict(bank=1, vec4=16)}, 'PS')
        self.assertEqual(uniforms[3][2][(1, 2)], 'tint.z')
        self.assertEqual(uniforms[4][2][(2, 0)], 'power')
        self.assertEqual(uniforms[5][:2], ('context', 16))

    def test_rewrites_literals_products_casts_and_names(self):
        manifest = read_manifest(MANIFEST, 1, 1)
        uniforms = uniform_names(manifest, {'3': dict(bank=8, vec4=1), '5': dict(bank=1, vec4=16)}, 'PS')
        glsl = ('R1i.x = floatBitsToInt(mul_nonIEEE(intBitsToFloat(R0i.x), intBitsToFloat(0x3f800000)) '
                '* intBitsToFloat(uf_remappedPS[3].y) + intBitsToFloat(uf_remappedPS[5].w));\n'
                'R2f.x = clamp(R1f.x, 0.0, 1.0);\n'
                'R3f.xyz = texture(textureUnitPS5, vec2(R0f.x, R0f.y)).xyz;\n')
        self.assertEqual(
            simplify(glsl, uniforms, sampler_names(manifest, 'PS')),
            'R1.x = (R0.x * 1.0) * tint.y + context[16].w;\n'
            'R2.x = sat(R1.x);\n'
            'R3.xyz = texture(albedo, vec2(R0.x, R0.y)).xyz;\n')

    def test_refuses_a_program_without_locations(self):
        with self.assertRaises(ValueError):
            read_manifest(MANIFEST, 1, 9)


if __name__ == '__main__':
    unittest.main()
