//! `.grass.extm`: where grass grows on a tile, and its colour. 64×64
//! records of 4 bytes, row-major (X fastest), like `.water.extm`:
//!
//! ```text
//! u8 height   0 = no grass, 255 = the tallest
//! u8 r, g, b  colour; (24, 52, 8) is the common green, Akkala's is red
//! ```
//!
//! Verified against the Wii U dump: every entry is 16 384 bytes, zero
//! heights line up with paths and cliffs, and the colour varies by region
//! (darker in the forests, reddish in Akkala). The archives exist only in
//! the update and only for levels 5–8; tiles flagged in `MainField.tscb`
//! have them. The height byte / 255 is the game's `grass_summary0.w`, a
//! factor in the blade shader's length (docs/research/wiiu-field-shading.md,
//! "Grass blade").

use crate::{FormatError, Result};

/// Samples per grass tile edge.
pub const GRASS_SAMPLES: usize = 64;
const SAMPLE_COUNT: usize = GRASS_SAMPLES * GRASS_SAMPLES;
pub const FILE_SIZE: usize = SAMPLE_COUNT * 4;

/// The colour most grass has.
pub const DEFAULT_COLOR: [u8; 3] = [24, 52, 8];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GrassSample {
    /// Grass height, 0 (none) to 255.
    pub height: u8,
    pub color: [u8; 3],
}

#[derive(Clone, PartialEq, Eq)]
pub struct GrassTile {
    samples: Box<[GrassSample]>,
}

impl std::fmt::Debug for GrassTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let covered = self.samples.iter().filter(|s| s.height > 0).count();
        f.debug_struct("GrassTile")
            .field("covered", &covered)
            .finish()
    }
}

impl GrassTile {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != FILE_SIZE {
            return Err(FormatError::WrongSize {
                what: "grass tile",
                expected: FILE_SIZE,
                actual: bytes.len(),
            });
        }
        let samples = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|r| GrassSample {
                height: r[0],
                color: [r[1], r[2], r[3]],
            })
            .collect();
        Ok(Self { samples })
    }

    /// Sample at grid position `(x, z)`, each in `0..64`.
    pub fn get(&self, x: usize, z: usize) -> GrassSample {
        self.samples[z * GRASS_SAMPLES + x]
    }

    /// Bilinearly interpolated height (0–255) and colour at fractional grid
    /// position `(u, v)`, each in `0.0..=63.0`.
    // SI-GRS-03: grass heights and normals from our own 1 m grid.
    pub fn sample(&self, u: f32, v: f32) -> (f32, [f32; 3]) {
        let max = (GRASS_SAMPLES - 1) as f32;
        let (u, v) = (u.clamp(0.0, max), v.clamp(0.0, max));
        let (x0, z0) = (u.floor() as usize, v.floor() as usize);
        let (x1, z1) = (
            (x0 + 1).min(GRASS_SAMPLES - 1),
            (z0 + 1).min(GRASS_SAMPLES - 1),
        );
        let (fx, fz) = (u - x0 as f32, v - z0 as f32);
        let weights = [
            (x0, z0, (1.0 - fx) * (1.0 - fz)),
            (x1, z0, fx * (1.0 - fz)),
            (x0, z1, (1.0 - fx) * fz),
            (x1, z1, fx * fz),
        ];
        let (mut height, mut color) = (0.0, [0.0; 3]);
        for (x, z, w) in weights {
            let s = self.get(x, z);
            height += f32::from(s.height) * w;
            for (c, &value) in color.iter_mut().zip(&s.color) {
                *c += f32::from(value) * w;
            }
        }
        (height, color)
    }

    /// Builds a tile from samples in file order. Panics on a wrong length.
    pub fn from_samples(samples: Vec<GrassSample>) -> Self {
        assert_eq!(
            samples.len(),
            SAMPLE_COUNT,
            "a grass tile has 64×64 samples"
        );
        Self {
            samples: samples.into(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.samples
            .iter()
            .flat_map(|s| [s.height, s.color[0], s.color[1], s.color[2]])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_interpolates() {
        let samples = (0..SAMPLE_COUNT)
            .map(|i| GrassSample {
                height: if i % GRASS_SAMPLES < 32 { 0 } else { 200 },
                color: DEFAULT_COLOR,
            })
            .collect();
        let tile = GrassTile::from_samples(samples);
        let bytes = tile.to_bytes();
        assert_eq!(bytes.len(), FILE_SIZE);
        assert_eq!(&bytes[32 * 4..32 * 4 + 4], &[200, 24, 52, 8]);
        assert_eq!(GrassTile::parse(&bytes).unwrap(), tile);

        assert_eq!(tile.get(31, 5).height, 0);
        let (height, color) = tile.sample(31.5, 5.0);
        assert!((height - 100.0).abs() < 1e-3);
        assert!((color[1] - 52.0).abs() < 1e-3);
        assert!(GrassTile::parse(&bytes[1..]).is_err());
    }
}
