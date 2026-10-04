//! `.hght`: 256×256 little-endian `u16` heights, row-major with X varying
//! fastest. Raw values scale linearly to `0..=WORLD_HEIGHT`. Both the Wii U
//! and Switch releases store these little-endian (see archived FORMATS.md notes).

use super::{TILE_SAMPLES, WORLD_HEIGHT};
use crate::{FormatError, Result};

const SAMPLE_COUNT: usize = TILE_SAMPLES * TILE_SAMPLES;
pub const FILE_SIZE: usize = SAMPLE_COUNT * 2;

#[derive(Clone, PartialEq, Eq)]
pub struct HeightTile {
    raw: Box<[u16]>,
}

impl std::fmt::Debug for HeightTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (min, max) = self.raw_range();
        f.debug_struct("HeightTile")
            .field("raw_min", &min)
            .field("raw_max", &max)
            .finish()
    }
}

impl HeightTile {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != FILE_SIZE {
            return Err(FormatError::WrongSize {
                what: "hght tile",
                expected: FILE_SIZE,
                actual: bytes.len(),
            });
        }
        let raw = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_le_bytes(pair))
            .collect();
        Ok(Self { raw })
    }

    /// Builds a tile from raw samples in file order. Panics on a wrong length.
    pub fn from_raw(raw: Vec<u16>) -> Self {
        assert_eq!(raw.len(), SAMPLE_COUNT, "a height tile has 256×256 samples");
        Self { raw: raw.into() }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.raw.iter().flat_map(|h| h.to_le_bytes()).collect()
    }

    /// Raw sample at grid position `(x, z)`, each in `0..256`.
    pub fn raw(&self, x: usize, z: usize) -> u16 {
        self.raw[z * TILE_SAMPLES + x]
    }

    /// World-space height at grid position `(x, z)`.
    pub fn height(&self, x: usize, z: usize) -> f32 {
        raw_to_world(self.raw(x, z))
    }

    /// Bilinearly interpolated world height at fractional grid position
    /// `(u, v)`, each in `0.0..=255.0`.
    // SI-FMT-11: terrain blend byte and height scale are our reading.
    pub fn sample(&self, u: f32, v: f32) -> f32 {
        let max = (TILE_SAMPLES - 1) as f32;
        let (u, v) = (u.clamp(0.0, max), v.clamp(0.0, max));
        let (x0, z0) = (u.floor() as usize, v.floor() as usize);
        let (x1, z1) = (
            (x0 + 1).min(TILE_SAMPLES - 1),
            (z0 + 1).min(TILE_SAMPLES - 1),
        );
        let (fx, fz) = (u - x0 as f32, v - z0 as f32);
        let top = lerp(self.height(x0, z0), self.height(x1, z0), fx);
        let bottom = lerp(self.height(x0, z1), self.height(x1, z1), fx);
        lerp(top, bottom, fz)
    }

    pub fn raw_range(&self) -> (u16, u16) {
        self.raw
            .iter()
            .fold((u16::MAX, u16::MIN), |(lo, hi), &h| (lo.min(h), hi.max(h)))
    }
}

pub fn raw_to_world(raw: u16) -> f32 {
    f32::from(raw) / f32::from(u16::MAX) * WORLD_HEIGHT
}

pub fn world_to_raw(height: f32) -> u16 {
    (height / WORLD_HEIGHT * f32::from(u16::MAX))
        .round()
        .clamp(0.0, f32::from(u16::MAX)) as u16
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_little_endian_row_major() {
        let mut raw = vec![0u16; SAMPLE_COUNT];
        raw[1] = 0x0102; // x = 1, z = 0
        raw[TILE_SAMPLES] = u16::MAX; // x = 0, z = 1
        let bytes: Vec<u8> = raw.iter().flat_map(|h| h.to_le_bytes()).collect();
        assert_eq!(&bytes[2..4], &[0x02, 0x01]);

        let tile = HeightTile::parse(&bytes).unwrap();
        assert_eq!(tile.raw(1, 0), 0x0102);
        assert_eq!(tile.height(0, 1), WORLD_HEIGHT);
        assert_eq!(tile.to_bytes(), bytes);
    }

    #[test]
    fn rejects_wrong_size() {
        assert!(matches!(
            HeightTile::parse(&[0; 10]),
            Err(FormatError::WrongSize {
                expected: FILE_SIZE,
                actual: 10,
                ..
            })
        ));
    }

    #[test]
    fn samples_bilinearly() {
        let raw = (0..SAMPLE_COUNT)
            .map(|i| world_to_raw((i % TILE_SAMPLES) as f32))
            .collect();
        let tile = HeightTile::from_raw(raw);
        assert!((tile.sample(10.5, 3.0) - 10.5).abs() < 0.02);
        assert!(
            (tile.sample(300.0, 0.0) - 255.0).abs() < 0.02,
            "clamps to the edge"
        );
    }
}
