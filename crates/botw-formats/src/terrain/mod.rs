//! MainField terrain: a quadtree of 256×256 height/material tiles.
//!
//! Level of detail `L` splits the 16 000 × 16 000 terrain grid into
//! `2^L × 2^L` tiles addressed by a Z-order index, so every tile at every
//! level has the same 256×256 resolution and the finest level (8) is ~0.25
//! units per sample. Higher levels only exist where the game needs the detail.
//! See `archived FORMATS.md notes` for sources.

pub mod grass;
pub mod hght;
pub mod index;
pub mod mate;
pub mod textures;
pub mod tile;
pub mod tscb;
pub mod water;
pub mod zorder;

pub use grass::{GrassSample, GrassTile};
pub use hght::HeightTile;
pub use index::TerrainIndex;
pub use mate::{MaterialSample, MaterialTile};
pub use tile::{TerrainKind, TileId};
pub use tscb::Tscb;
pub use water::{WaterSample, WaterTile};

/// Side length of the square terrain grid in world units; the grid spans
/// `-WORLD_SIZE / 2 ..= WORLD_SIZE / 2` on X and Z. This is `MainField.tscb`'s
/// root area (32 × `world_scale` 500). The playable map is only the middle
/// 10 000 × 8 000 of it; the rest is sea and backdrop mountains.
pub const WORLD_SIZE: f32 = 16_000.0;

/// World height of the maximum raw height sample (`u16::MAX`).
pub const WORLD_HEIGHT: f32 = 800.0;

/// Finest level of detail.
pub const MAX_LOD: u8 = 8;

/// Samples per tile edge. Neighbouring tiles share their edge samples, so a
/// tile spans 255 sample intervals.
pub const TILE_SAMPLES: usize = 256;

/// Relative path of the MainField terrain tiles inside a content root.
pub const MAINFIELD_DIR: &str = "Terrain/A/MainField";

/// Relative path of the MainField terrain scene (list of existing tiles).
pub const MAINFIELD_TSCB: &str = "Terrain/A/MainField.tscb";
