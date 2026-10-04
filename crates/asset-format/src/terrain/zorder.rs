//! Z-order (Morton) indices used to name terrain tiles: the X coordinate
//! occupies the even bits and Z the odd bits, so `index >> 2` is the parent
//! tile one level up and the low two bits pick the quadrant.

/// Packs 8-bit grid coordinates into a 16-bit Z-order index.
pub fn interleave(x: u8, z: u8) -> u16 {
    spread(x) | (spread(z) << 1)
}

/// Unpacks a Z-order index into `(x, z)` grid coordinates.
pub fn deinterleave(index: u16) -> (u8, u8) {
    (compact(index), compact(index >> 1))
}

/// Moves bit `i` of `v` to bit `2i`.
fn spread(v: u8) -> u16 {
    let mut v = u16::from(v);
    v = (v | (v << 4)) & 0x0F0F;
    v = (v | (v << 2)) & 0x3333;
    v = (v | (v << 1)) & 0x5555;
    v
}

/// Inverse of [`spread`]: gathers the even bits of `v`.
fn compact(v: u16) -> u8 {
    let mut v = v & 0x5555;
    v = (v | (v >> 1)) & 0x3333;
    v = (v | (v >> 2)) & 0x0F0F;
    v = (v | (v >> 4)) & 0x00FF;
    v as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_coordinate() {
        for x in 0..=u8::MAX {
            for z in 0..=u8::MAX {
                assert_eq!(deinterleave(interleave(x, z)), (x, z));
            }
        }
    }

    #[test]
    fn x_uses_even_bits() {
        assert_eq!(interleave(1, 0), 0b01);
        assert_eq!(interleave(0, 1), 0b10);
        assert_eq!(interleave(0xFF, 0), 0x5555);
        assert_eq!(interleave(0, 0xFF), 0xAAAA);
        // Bits 15, 14, 7 and 5 set: x gets bit 14, z gets bits 15, 7 and 5.
        assert_eq!(deinterleave(0xC0A0), (0x80, 0x8C));
    }
}
