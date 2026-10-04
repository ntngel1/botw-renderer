"""Synthetic Cemu-style GLSL: fetches, uniforms, moves and unrelated inputs."""
import unittest

from cemu_glsl_deps import summary

SHADER = '''
void main()
{
R0i = floatBitsToInt(passParameterSem0);
R1i.xy = floatBitsToInt(texture(textureUnitPS3, vec2(intBitsToFloat(R0i.x),intBitsToFloat(R0i.y))).xy);
R2i.z = floatBitsToInt(texture(textureUnitPS5, vec2(intBitsToFloat(R0i.z),intBitsToFloat(R0i.w))).x);
PV0ix = floatBitsToInt(mul_nonIEEE(intBitsToFloat(R1i.y), uf_blockPS6[13].x));
R3i.x = PV0ix;
R3i.x = clampFI32(R3i.x);
passPixelColor0 = vec4(intBitsToFloat(R3i.x), 0.0, 0.0, 1.0);
passPixelColor1 = vec4(intBitsToFloat(R2i.z), 0.0, 0.0, 1.0);
}
'''


class GlslDepsTests(unittest.TestCase):
    def test_outputs_keep_their_own_sources(self):
        result = summary(SHADER)
        first, second = result['passPixelColor0'], result['passPixelColor1']
        self.assertEqual(first['textures'], ['textureUnitPS3'])
        self.assertEqual(first['uniforms'], ['uf_blockPS6[13].x'])
        # Fetch coordinates count: R1 was sampled at Sem0.xy.
        self.assertEqual(first['varyings'], [])
        self.assertEqual(second['textures'], ['textureUnitPS5'])
        self.assertEqual(second['uniforms'], [])


if __name__ == '__main__':
    unittest.main()
