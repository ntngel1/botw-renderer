//! Baked terrain: the game's quadtree of 256×256 tiles, one file per tile.
//!
//! Level of detail `L` splits the 16 000 × 16 000 terrain grid into
//! `2^L × 2^L` tiles addressed by a Z-order index (the game's addressing,
//! see [`TileId`]). A tile file ([`TileFile`]) holds the tile's samples in
//! the game's own record layouts, so nothing is resampled: heights
//! ([`HeightTile`]), materials ([`MaterialTile`]) and, where the game has
//! them, water ([`WaterTile`]) and grass ([`GrassTile`]).
//!
//! [`TerrainIndex`] lists the baked tiles with their height ranges;
//! [`MaterialTable`] the terrain materials and their texture layers.

pub mod grass;
pub mod hght;
pub mod mate;
pub mod tile;
pub mod water;
pub mod zorder;

use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use grass::{GrassSample, GrassTile};
pub use hght::HeightTile;
pub use mate::{MaterialSample, MaterialTile};
pub use tile::TileId;
pub use water::{WaterSample, WaterTile};

use crate::{FormatError, Result};

/// Side length of the square terrain grid in world units; the grid spans
/// `-WORLD_SIZE / 2 ..= WORLD_SIZE / 2` on X and Z (`MainField.tscb`'s root
/// area, 32 × `world_scale` 500).
pub const WORLD_SIZE: f32 = 16_000.0;

/// World height of the maximum raw height sample (`u16::MAX`).
pub const WORLD_HEIGHT: f32 = 800.0;

/// Finest level of detail.
pub const MAX_LOD: u8 = 8;

/// Samples per tile edge. Neighbouring tiles share their edge samples, so a
/// tile spans 255 sample intervals.
pub const TILE_SAMPLES: usize = 256;

/// One baked tile: its samples by kind. Heights are always there.
#[derive(Clone, Debug, PartialEq)]
pub struct TileFile {
    pub height: HeightTile,
    pub material: Option<MaterialTile>,
    pub water: Option<WaterTile>,
    pub grass: Option<GrassTile>,
}

/// Tile files are a zstd frame of: `HTIL`, `u32` version, `u32` section
/// count, then per section a four-byte tag, a `u32` length and the bytes.
/// All integers little-endian. Unknown sections are skipped.
const TILE_MAGIC: &[u8; 4] = b"HTIL";
const TILE_VERSION: u32 = 1;
const SECTION_HEIGHT: &[u8; 4] = b"HGHT";
const SECTION_MATERIAL: &[u8; 4] = b"MATE";
const SECTION_WATER: &[u8; 4] = b"WATR";
const SECTION_GRASS: &[u8; 4] = b"GRAS";
const ZSTD_LEVEL: i32 = 9;

impl TileFile {
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut sections: Vec<(&[u8; 4], Vec<u8>)> = vec![(SECTION_HEIGHT, self.height.to_bytes())];
        if let Some(material) = &self.material {
            sections.push((SECTION_MATERIAL, material.to_bytes()));
        }
        if let Some(water) = &self.water {
            sections.push((SECTION_WATER, water.to_bytes()));
        }
        if let Some(grass) = &self.grass {
            sections.push((SECTION_GRASS, grass.to_bytes()));
        }
        let mut raw = Vec::new();
        raw.extend_from_slice(TILE_MAGIC);
        raw.extend_from_slice(&TILE_VERSION.to_le_bytes());
        raw.extend_from_slice(&(sections.len() as u32).to_le_bytes());
        for (tag, bytes) in sections {
            raw.extend_from_slice(tag);
            raw.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            raw.extend_from_slice(&bytes);
        }
        zstd::encode_all(&raw[..], ZSTD_LEVEL).map_err(|e| FormatError::Compression(e.to_string()))
    }

    pub fn parse(compressed: &[u8]) -> Result<Self> {
        let mut raw = Vec::new();
        zstd::Decoder::new(compressed)
            .and_then(|mut d| d.read_to_end(&mut raw))
            .map_err(|e| FormatError::Compression(e.to_string()))?;
        let mut reader = Reader(&raw);
        if reader.take(4)? != TILE_MAGIC {
            return Err(FormatError::Invalid("tile: missing HTIL magic"));
        }
        if reader.u32()? != TILE_VERSION {
            return Err(FormatError::Invalid("tile: unsupported version"));
        }
        let (mut height, mut material, mut water, mut grass) = (None, None, None, None);
        for _ in 0..reader.u32()? {
            let tag: [u8; 4] = reader.take(4)?.try_into().expect("four bytes");
            let len = reader.u32()? as usize;
            let bytes = reader.take(len)?;
            match &tag {
                SECTION_HEIGHT => height = Some(HeightTile::parse(bytes)?),
                SECTION_MATERIAL => material = Some(MaterialTile::parse(bytes)?),
                SECTION_WATER => water = Some(WaterTile::parse(bytes)?),
                SECTION_GRASS => grass = Some(GrassTile::parse(bytes)?),
                _ => {}
            }
        }
        Ok(Self {
            height: height.ok_or(FormatError::Invalid("tile: no heights"))?,
            material,
            water,
            grass,
        })
    }

    pub fn read(path: &Path) -> Result<Self> {
        Self::parse(&crate::read(path)?)
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        if self.0.len() < len {
            return Err(FormatError::Invalid("tile: truncated"));
        }
        let (head, rest) = self.0.split_at(len);
        self.0 = rest;
        Ok(head)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }
}

/// The baked tiles (`terrain/index.ron`): every tile that has a file, and
/// what the game's terrain scene (`MainField.tscb`) says about it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TerrainIndex {
    /// The region the tiles were baked for (see `places.ron`).
    pub region: String,
    pub tiles: Vec<TileInfo>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TileInfo {
    pub tile: TileId,
    pub min_height: f32,
    pub max_height: f32,
    /// Height range of the tile's water surface, if it has water.
    pub water_range: Option<[f32; 2]>,
    /// Whether the file has a water section (the game's `.water.extm`);
    /// tiles without one use their nearest ancestor's.
    pub water: bool,
    /// Whether the file has a grass section (`.grass.extm`).
    pub grass: bool,
}

/// The terrain materials (`terrain/materials.ron`): `.mate` samples index
/// this list.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MaterialTable {
    /// Edge of the albedo layers (`terrain/albedo.ktx2`), texels.
    pub albedo_size: u32,
    /// Edge of the normal layers (`terrain/normals.ktx2`), texels; 0 if the
    /// game has none.
    pub normals_size: u32,
    pub materials: Vec<TerrainMaterial>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerrainMaterial {
    /// The game's texture name (`MaterialAlb`'s `file` user data).
    pub name: String,
    /// Layer in the albedo and normal arrays (`array_index`).
    pub layer: u32,
    /// Texture repeats per metre (`MainField.tscb`).
    pub uv_scale: [f32; 2],
    /// `MainField.tscb`'s two further values per material; unread.
    pub unknown: [f32; 2],
    /// Mean albedo of the layer, linear RGB (the grass reads it).
    pub mean_albedo: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_files_round_trip() {
        let n = TILE_SAMPLES * TILE_SAMPLES;
        let file = TileFile {
            height: HeightTile::from_raw((0..n).map(|i| i as u16).collect()),
            material: Some(MaterialTile::from_samples(vec![
                MaterialSample {
                    material0: 3,
                    material1: 7,
                    blend: 128,
                    unknown: 1,
                };
                n
            ])),
            water: None,
            grass: Some(GrassTile::from_samples(vec![
                GrassSample {
                    height: 200,
                    color: grass::DEFAULT_COLOR,
                };
                grass::GRASS_SAMPLES
                    * grass::GRASS_SAMPLES
            ])),
        };
        let bytes = file.to_bytes().unwrap();
        assert_eq!(TileFile::parse(&bytes).unwrap(), file);
        assert!(TileFile::parse(&bytes[..bytes.len() / 2]).is_err());
    }
}
