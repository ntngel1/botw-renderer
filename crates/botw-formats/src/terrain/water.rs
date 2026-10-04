//! `.water.extm`: the water surface of a tile, a coarser grid than the
//! terrain. 64×64 records of 8 bytes, row-major (X fastest), little-endian
//! (like `.hght`, also on Wii U):
//!
//! ```text
//! u16 height   same scale as .hght: raw / 65535 × 800
//! u16 flow x   32768 = still
//! u16 flow z
//! u8  unknown  kind + 3 in the root tile, small values (0–10) elsewhere
//! u8  kind     see `kind`
//! ```
//!
//! Verified against the Wii U dump: heights match the water ranges in
//! `MainField.tscb` exactly; kinds checked at known places. The surface
//! extends under dry land; the terrain hides it there. Water data often
//! stops at a coarser level than the terrain (Lake Hylia: level 5, terrain
//! to 7), so finer tiles use their nearest ancestor's water.

use super::hght::raw_to_world;
use crate::{FormatError, Result};

/// Samples per water tile edge.
pub const WATER_SAMPLES: usize = 64;
const SAMPLE_COUNT: usize = WATER_SAMPLES * WATER_SAMPLES;
pub const FILE_SIZE: usize = SAMPLE_COUNT * 8;

/// Water kinds seen in the dump (others exist but are not identified yet).
pub mod kind {
    /// Lakes, rivers, ponds (Lake Hylia, the castle moat).
    pub const FRESH: u8 = 0;
    /// Hot springs (the spas in Hebra and Tabantha).
    pub const HOT: u8 = 1;
    /// Death Mountain's lava.
    pub const LAVA: u8 = 3;
    /// The sea, at 105.9 m.
    pub const SEA: u8 = 7;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterSample {
    /// World height of the surface.
    pub height: f32,
    /// Raw flow, 32768 = still; direction and scale not yet verified.
    pub flow: [u16; 2],
    /// What the water is, see [`kind`].
    pub kind: u8,
    /// The byte before `kind`; meaning unknown.
    // SI-WAT-08: the meaning of this byte is a guess.
    pub unknown: u8,
}

#[derive(Clone, PartialEq)]
pub struct WaterTile {
    samples: Box<[WaterSample]>,
}

impl std::fmt::Debug for WaterTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (min, max) = self.height_range();
        f.debug_struct("WaterTile")
            .field("min", &min)
            .field("max", &max)
            .finish()
    }
}

impl WaterTile {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != FILE_SIZE {
            return Err(FormatError::WrongSize {
                what: "water tile",
                expected: FILE_SIZE,
                actual: bytes.len(),
            });
        }
        let samples = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|r| WaterSample {
                height: raw_to_world(u16::from_le_bytes([r[0], r[1]])),
                flow: [
                    u16::from_le_bytes([r[2], r[3]]),
                    u16::from_le_bytes([r[4], r[5]]),
                ],
                unknown: r[6],
                kind: r[7],
            })
            .collect();
        Ok(Self { samples })
    }

    /// Sample at grid position `(x, z)`, each in `0..64`.
    pub fn get(&self, x: usize, z: usize) -> WaterSample {
        self.samples[z * WATER_SAMPLES + x]
    }

    /// Bilinearly interpolated surface height at fractional grid position
    /// `(u, v)`, each in `0.0..=63.0`.
    pub fn height(&self, u: f32, v: f32) -> f32 {
        let max = (WATER_SAMPLES - 1) as f32;
        let (u, v) = (u.clamp(0.0, max), v.clamp(0.0, max));
        let (x0, z0) = (u.floor() as usize, v.floor() as usize);
        let (x1, z1) = (
            (x0 + 1).min(WATER_SAMPLES - 1),
            (z0 + 1).min(WATER_SAMPLES - 1),
        );
        let (fx, fz) = (u - x0 as f32, v - z0 as f32);
        let h = |x, z| self.get(x, z).height;
        let top = h(x0, z0) + (h(x1, z0) - h(x0, z0)) * fx;
        let bottom = h(x0, z1) + (h(x1, z1) - h(x0, z1)) * fx;
        top + (bottom - top) * fz
    }

    /// Bilinearly interpolated flow at `(u, v)` (like [`Self::height`]),
    /// raw units relative to still water: 0 = still, one unit = 1/32768 of
    /// the raw range.
    pub fn flow(&self, u: f32, v: f32) -> [f32; 2] {
        let max = (WATER_SAMPLES - 1) as f32;
        let (u, v) = (u.clamp(0.0, max), v.clamp(0.0, max));
        let (x0, z0) = (u.floor() as usize, v.floor() as usize);
        let (x1, z1) = (
            (x0 + 1).min(WATER_SAMPLES - 1),
            (z0 + 1).min(WATER_SAMPLES - 1),
        );
        let (fx, fz) = (u - x0 as f32, v - z0 as f32);
        let f = |x, z, axis: usize| f32::from(self.get(x, z).flow[axis]) - 32768.0;
        std::array::from_fn(|axis| {
            let top = f(x0, z0, axis) + (f(x1, z0, axis) - f(x0, z0, axis)) * fx;
            let bottom = f(x0, z1, axis) + (f(x1, z1, axis) - f(x0, z1, axis)) * fx;
            top + (bottom - top) * fz
        })
    }

    /// The kind at the nearest sample to `(u, v)`.
    // SI-WAT-08: nearest sample for water and lava; finer tiles use the ancestor.
    pub fn kind(&self, u: f32, v: f32) -> u8 {
        let max = (WATER_SAMPLES - 1) as f32;
        self.get(
            u.clamp(0.0, max).round() as usize,
            v.clamp(0.0, max).round() as usize,
        )
        .kind
    }

    /// The kind the game's water vertex shader reads at `(u, v)` (like
    /// [`Self::height`]): the tile is a 64×64 R16G16B16A16 UNORM texture of
    /// the file's bytes, sampled bilinearly with clamping (Wii U v208:
    /// format `0x036a00c0`/`0x036f226c`, sampler @`0x036a0204..0x036a0258`),
    /// so alpha is `unknown + 256·kind` filtered between the four samples
    /// around; the shader takes `floor(a·65535/256 + 0.5/65535)`
    /// (docs/research/wiiu-water-variants.md). Between samples of different
    /// kinds this passes through the kinds in between.
    pub fn packed_kind(&self, u: f32, v: f32) -> u8 {
        let max = (WATER_SAMPLES - 1) as f32;
        let (u, v) = (u.clamp(0.0, max), v.clamp(0.0, max));
        let (x0, z0) = (u.floor() as usize, v.floor() as usize);
        let (x1, z1) = (
            (x0 + 1).min(WATER_SAMPLES - 1),
            (z0 + 1).min(WATER_SAMPLES - 1),
        );
        let (fx, fz) = (u - x0 as f32, v - z0 as f32);
        let a = |x, z| {
            let s = self.get(x, z);
            f32::from(u16::from(s.kind) << 8 | u16::from(s.unknown)) / 65535.0
        };
        let top = a(x0, z0) + (a(x1, z0) - a(x0, z0)) * fx;
        let bottom = a(x0, z1) + (a(x1, z1) - a(x0, z1)) * fx;
        let alpha = top + (bottom - top) * fz;
        let q = alpha * (65535.0 / 256.0) + 0.5 / 65535.0;
        q.floor().clamp(0.0, 255.0) as u8
    }

    pub fn height_range(&self) -> (f32, f32) {
        self.samples
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), s| {
                (lo.min(s.height), hi.max(s.height))
            })
    }

    /// Builds a tile from samples in file order. Panics on a wrong length.
    pub fn from_samples(samples: Vec<WaterSample>) -> Self {
        assert_eq!(
            samples.len(),
            SAMPLE_COUNT,
            "a water tile has 64×64 samples"
        );
        Self {
            samples: samples.into(),
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.samples
            .iter()
            .flat_map(|s| {
                let raw = super::hght::world_to_raw(s.height).to_le_bytes();
                let [fx, fz] = s.flow.map(u16::to_le_bytes);
                [
                    raw[0], raw[1], fx[0], fx[1], fz[0], fz[1], s.unknown, s.kind,
                ]
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_little_endian_records() {
        let mut bytes = vec![0u8; FILE_SIZE];
        // Sample (1, 0): 400 m of still lava.
        let raw = super::super::hght::world_to_raw(400.0).to_le_bytes();
        bytes[8..16].copy_from_slice(&[raw[0], raw[1], 0x00, 0x80, 0x00, 0x80, 6, 3]);
        let tile = WaterTile::parse(&bytes).unwrap();
        let sample = tile.get(1, 0);
        assert!((sample.height - 400.0).abs() < 0.01);
        assert_eq!(
            (sample.flow, sample.kind, sample.unknown),
            ([32768, 32768], kind::LAVA, 6)
        );
        assert!((tile.height(0.5, 0.0) - 200.0).abs() < 0.01);
        assert_eq!(tile.flow(1.0, 0.0), [0.0, 0.0]);
        // Sample (0, 0) is all zeros: flow -32768 on both axes.
        assert_eq!(tile.flow(0.5, 0.0), [-16384.0, -16384.0]);
        assert_eq!(tile.to_bytes(), bytes);
        assert!(WaterTile::parse(&[0; 8]).is_err());
    }

    #[test]
    fn the_shader_kind_is_the_floor_of_the_filtered_packed_alpha() {
        let sample = |kind, unknown| WaterSample {
            height: 100.0,
            flow: [32768; 2],
            kind,
            unknown,
        };
        let mut samples = vec![sample(kind::FRESH, 0); SAMPLE_COUNT];
        // Sea at (1, 0), with the largest low byte; fresh water elsewhere.
        samples[1] = sample(kind::SEA, 255);
        let tile = WaterTile::from_samples(samples);
        // On a sample: its own kind, whatever the low byte.
        assert_eq!(tile.packed_kind(1.0, 0.0), kind::SEA);
        assert_eq!(tile.packed_kind(0.0, 0.0), kind::FRESH);
        // Halfway: (7·256 + 255)/2 / 256 = 3.998 — the lava's row, where
        // the nearest sample says fresh water or sea.
        assert_eq!(tile.packed_kind(0.5, 0.0), 3);
        assert_eq!(tile.packed_kind(0.8, 0.0), 6);
        // A quarter of the way down towards fresh water: 0.75·2047/256 = 5.997.
        assert_eq!(tile.packed_kind(1.0, 0.25), 5);
        // Clamped outside the tile.
        assert_eq!(tile.packed_kind(-3.0, 70.0), kind::FRESH);
    }
}
