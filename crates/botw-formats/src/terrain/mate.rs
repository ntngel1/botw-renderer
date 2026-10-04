//! `.mate`: 256×256 material samples, 4 bytes each, same layout as `.hght`.
//!
//! Each sample blends two of the ~88 terrain materials. Material indices refer
//! to the texture array in `Terrain.Tex1.sbfres` (its `array_index` user data
//! gives the order), not to a fixed table in this crate.

use super::TILE_SAMPLES;
use crate::{FormatError, Result};

const SAMPLE_COUNT: usize = TILE_SAMPLES * TILE_SAMPLES;
pub const FILE_SIZE: usize = SAMPLE_COUNT * 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MaterialSample {
    pub material0: u8,
    // SI-FMT-11: terrain blend byte and height scale are our reading.
    pub material1: u8,
    /// 0 = only `material0`, 255 = only `material1`.
    pub blend: u8,
    /// Purpose unknown.
    pub unknown: u8,
}

#[derive(Clone, PartialEq, Eq)]
pub struct MaterialTile {
    samples: Box<[MaterialSample]>,
}

impl std::fmt::Debug for MaterialTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaterialTile").finish_non_exhaustive()
    }
}

impl MaterialTile {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != FILE_SIZE {
            return Err(FormatError::WrongSize {
                what: "mate tile",
                expected: FILE_SIZE,
                actual: bytes.len(),
            });
        }
        let samples = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[material0, material1, blend, unknown]| MaterialSample {
                material0,
                material1,
                blend,
                unknown,
            })
            .collect();
        Ok(Self { samples })
    }

    /// Builds a tile from samples in file order. Panics on a wrong length.
    pub fn from_samples(samples: Vec<MaterialSample>) -> Self {
        assert_eq!(
            samples.len(),
            SAMPLE_COUNT,
            "a material tile has 256×256 samples"
        );
        Self {
            samples: samples.into(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.samples
            .iter()
            .flat_map(|s| [s.material0, s.material1, s.blend, s.unknown])
            .collect()
    }

    /// Sample at grid position `(x, z)`, each in `0..256`.
    pub fn get(&self, x: usize, z: usize) -> MaterialSample {
        self.samples[z * TILE_SAMPLES + x]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_samples_in_order() {
        let mut bytes = vec![0u8; FILE_SIZE];
        let second_row = TILE_SAMPLES * 4;
        bytes[second_row..second_row + 4].copy_from_slice(&[7, 9, 128, 1]);
        let tile = MaterialTile::parse(&bytes).unwrap();
        assert_eq!(
            tile.get(0, 1),
            MaterialSample {
                material0: 7,
                material1: 9,
                blend: 128,
                unknown: 1
            }
        );
        assert_eq!(tile.to_bytes(), bytes);
    }
}
