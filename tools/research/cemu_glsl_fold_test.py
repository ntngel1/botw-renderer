"""Synthetic Cemu-style GLSL for the folder."""
import unittest

from cemu_glsl_fold import F, fold

GLSL = """header
void main()
{
vec4 R0 = vec4(0.0);
R0 = passParameterSem0;
activeMaskStack[0] = true;
activeMaskStackC[0] = true;
activeMaskStackC[1] = true;
if( activeMaskStackC[1] == true ) {
activeMaskStack[1] = activeMaskStack[0];
activeMaskStackC[2] = activeMaskStackC[1];
// 0
PV0x = R0.x * 2.0;
// 1
R1.x = PV0x + 1.0;
predResult = (R1.x > 0.5);
activeMaskStack[1] = predResult;
activeMaskStackC[2] = predResult == true && activeMaskStackC[1] == true;
}
else {
activeMaskStack[1] = false;
activeMaskStackC[2] = false;
}
if( activeMaskStackC[2] == true ) {
R2.x = R1.x;
}
activeMaskStack[1] = activeMaskStack[1] == false;
activeMaskStackC[2] = activeMaskStack[1] == true && activeMaskStackC[1] == true;
if( activeMaskStackC[2] == true ) {
R2.x = 0.0;
}
activeMaskStackC[1] = activeMaskStack[0] == true && activeMaskStackC[0] == true;
passParameterSem0 = vec4(R2.x, R2.x, R2.x, R2.x);
}
"""


class FoldTests(unittest.TestCase):
    def test_masks_become_conditions(self):
        out = fold(GLSL)
        self.assertIn('R1.x = (R0.x * 2.0) + 1.0;', out)
        self.assertIn('bool P1 = R1.x > 0.5;', out)
        self.assertIn('if (P1) {', out)
        self.assertIn('if (!P1) {', out)
        self.assertNotIn('PV0x', out)

    def test_formula_simplifies(self):
        p, q = F.lit('P1'), F.lit('P2')
        self.assertEqual(str((p & q) | (p & ~q)), 'P1')
        self.assertTrue(((p & ~p)).is_false())


if __name__ == '__main__':
    unittest.main()
