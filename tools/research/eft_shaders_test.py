"""Synthetic GFX2 and PTCL bytes for the effect-shader reader."""
import struct
import unittest

import eft_shaders as E


def block(kind: int, data: bytes) -> bytes:
    return struct.pack('>4sIIIIIII', b'BLK{', 0x20, 1, 0, kind, len(data), 0, 0) + data


def vs_header() -> bytes:
    h = bytearray(0x134 + 0x40)
    struct.pack_into('>I', h, 0x0C, 1)                      # one spi_vs_out_id register
    struct.pack_into('>I', h, 0x10, 0xFFFF0100)             # semantics 0, 1
    struct.pack_into('>I', h, 0xD0, 8)                      # program size
    struct.pack_into('>II', h, 0xDC, 1, 0xD0600000 | 0x134)  # one uniform block
    struct.pack_into('>III', h, 0x134, 0xD0600000 | 0x140, 7, 0x750)
    h[0x140:0x140 + 29] = b'sysEmitterStaticUniformBlock\0'
    return bytes(h)


def ps_header() -> bytes:
    h = bytearray(0xE8 + 0x20)
    struct.pack_into('>I', h, 0x10, 1)                      # one input
    struct.pack_into('>I', h, 0x14, 0x100)                  # semantic 0
    struct.pack_into('>I', h, 0xA4, 8)
    struct.pack_into('>II', h, 0xD0, 1, 0xD0600000 | 0xE8)  # one sampler
    struct.pack_into('>III', h, 0xE8, 0xD0600000 | 0xF4, 1, 0)
    h[0xF4:0xF4 + 19] = b'sysTextureSampler0\0'
    return bytes(h)


def gfx2() -> bytes:
    head = struct.pack('>4sIIIIIII', b'Gfx2', 0x20, 7, 1, 2, 0, 0, 0)
    return head + block(3, vs_header()) + block(5, b'\x11' * 8) + block(6, ps_header()) \
        + block(7, b'\x22' * 8) + block(1, b'')


class Gfx2Tests(unittest.TestCase):
    def test_reflection_and_code(self):
        data = gfx2()
        vs, ps = E.gfx2_shaders(data, 0, len(data))
        self.assertEqual(vs[0]['blocks'], [('sysEmitterStaticUniformBlock', 7, 0x750)])
        self.assertEqual(vs[0]['outputs'], [0, 1])
        self.assertEqual(vs[0]['code'], b'\x11' * 8)
        self.assertEqual(ps[0]['samplers'], [('sysTextureSampler0', 1, 0)])
        self.assertEqual(ps[0]['inputs'], [0])
        self.assertEqual(ps[0]['code'], b'\x22' * 8)

    def test_rejects_foreign_pointers(self):
        with self.assertRaises(ValueError):
            E._pointer(0x12345678)

    def test_yaz0(self):
        # One literal group: 8 literal bytes.
        packed = b'Yaz0' + struct.pack('>I', 8) + b'\0' * 8 + b'\xff' + b'ABCDEFGH'
        self.assertEqual(E.yaz0(packed), b'ABCDEFGH')


if __name__ == '__main__':
    unittest.main()
