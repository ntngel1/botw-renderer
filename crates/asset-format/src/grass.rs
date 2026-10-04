//! The grass's shading data and where the game takes it away. Written by
//! `bake` from `Model/Terrain.Tex1.sbfres` (`GrassAlb`, `GrassCrossAlb`),
//! `TeraGrass` in `Pack/TitleBG.pack` → `Model/Terrain.sbfres`, and the
//! world statistics maps (`Game/Stats/archive/<quarter>.sstats`). Where the
//! grass grows, how tall and in what colour is in the terrain tiles
//! (`terrain::grass`).
//!
//! ```text
//! grass/blade.ktx2          GrassAlb as the game stores it (level 0)
//! grass/tuft.ktx2           GrassCrossAlb: RGBA8 sRGB, level 0 decoded,
//!                           mips rebuilt keeping its alpha coverage
//! grass/tera_grass.ron      TeraGrass
//! grass/hidden/<quarter>.bin  HiddenQuarter, one per 500 m quarter tile
//! ```

use serde::{Deserialize, Serialize};

use crate::{FormatError, Result};

/// The `TeraGrass` material parameters the grass shaders read; `None`
/// where the material has none (the renderer keeps its recorded values).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TeraGrass {
    /// `uking_grass_lod_color.w` of `Blade1` and `Blade2` (blade types 0
    /// and 1): how far each type turns to the far colour.
    pub far_shares: Option<[f32; 2]>,
    /// The blades' wind swell (`Blade1`).
    pub blade_swell: Option<Swell>,
    /// The tufts' wind swell (`Cross1`).
    pub tuft_swell: Option<Swell>,
}

/// A material's `uking_grass_wind_swell_*` parameters, as stored.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swell {
    pub freq_scale: f32,
    pub dispersion_scale: f32,
    pub scale: f32,
    pub world_transform_coef: f32,
}

/// Side of a quarter tile (m).
pub const QUARTER: f32 = 500.0;
/// The world the quarters cover: |x| < 5000, |z| < 4000.
pub const WORLD_HALF: [f32; 2] = [5000.0, 4000.0];
/// Metres along a quarter's side.
pub const QUARTER_METRES: usize = QUARTER as usize;

/// A quarter tile (`qx`, `qz` from the world's corner (−5000, −4000)).
pub fn quarter_of(x: f32, z: f32) -> Option<[u32; 2]> {
    if x.abs() >= WORLD_HALF[0] || z.abs() >= WORLD_HALF[1] {
        return None;
    }
    Some([
        ((x + WORLD_HALF[0]) / QUARTER).floor() as u32,
        ((z + WORLD_HALF[1]) / QUARTER).floor() as u32,
    ])
}

/// `grass/hidden/<qx>_<qz>.bin`.
pub fn hidden_path(quarter: [u32; 2]) -> String {
    format!(
        "{}/{}_{}.bin",
        crate::paths::GRASS_HIDDEN,
        quarter[0],
        quarter[1]
    )
}

/// The square metres of a quarter where the game takes the grass away for
/// good: any of the statistics maps `terrain_embedded_edge`,
/// `terrain_is_in_door`, `terrain_hidden` has its bit (the original renderer's
/// `grass::hidden`). Stored as a Zstandard frame of `500 × 500` bits, row
/// by row (+z), a row along +x, bit `i % 8` of byte `i / 8`. No file: none
/// hidden.
#[derive(Clone, Debug, PartialEq)]
pub struct HiddenQuarter {
    pub bits: Vec<u8>,
}

impl HiddenQuarter {
    const BYTES: usize = QUARTER_METRES * QUARTER_METRES / 8;

    pub fn new() -> Self {
        Self {
            bits: vec![0; Self::BYTES],
        }
    }

    /// Metre `(i, j)` from the quarter's corner.
    pub fn get(&self, i: usize, j: usize) -> bool {
        let at = j * QUARTER_METRES + i;
        self.bits[at / 8] >> (at % 8) & 1 == 1
    }

    pub fn set(&mut self, i: usize, j: usize) {
        let at = j * QUARTER_METRES + i;
        self.bits[at / 8] |= 1 << (at % 8);
    }

    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|b| *b == 0)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        zstd::encode_all(&self.bits[..], 19).map_err(|e| FormatError::Compression(e.to_string()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let bits = zstd::decode_all(bytes).map_err(|e| FormatError::Compression(e.to_string()))?;
        if bits.len() != Self::BYTES {
            return Err(FormatError::WrongSize {
                what: "hidden grass quarter",
                expected: Self::BYTES,
                actual: bits.len(),
            });
        }
        Ok(Self { bits })
    }
}

impl Default for HiddenQuarter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_round_trip() {
        let mut q = HiddenQuarter::new();
        q.set(3, 0);
        q.set(499, 499);
        let back = HiddenQuarter::from_bytes(&q.to_bytes().unwrap()).unwrap();
        assert!(back.get(3, 0) && back.get(499, 499) && !back.get(4, 0));
        assert_eq!(quarter_of(-4999.5, -3999.5), Some([0, 0]));
        assert_eq!(quarter_of(4999.0, 3999.0), Some([19, 15]));
        assert_eq!(quarter_of(5000.0, 0.0), None);
    }
}
