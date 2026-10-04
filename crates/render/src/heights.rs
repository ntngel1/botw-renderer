// SI-WLD-08: finest tile as the game's height has no source.
//! Terrain and water heights on the CPU, for collision, swimming and placing
//! things on the ground. Always answers from the finest tile that has the
//! data at a position, which is what the game itself uses.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use asset_format::terrain::grass::GRASS_SAMPLES;
use asset_format::terrain::water::WATER_SAMPLES;
use asset_format::terrain::{
    GrassTile, HeightTile, MAX_LOD, MaterialTile, TILE_SAMPLES, TileId, WaterTile,
};

use crate::source::TerrainSource;

/// Tiles of each kind kept in memory; a collision cell touches one tile, the
/// player's surroundings a handful.
const CACHE_TILES: usize = 64;

/// The water surface at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterAt {
    pub height: f32,
    /// See `asset_format::terrain::water::kind`.
    pub kind: u8,
}

/// The grass at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GrassAt {
    /// 0 (none) to 255 (the tallest).
    pub height: f32,
    /// Raw colour, see `asset_format::terrain::grass`.
    pub color: [f32; 3],
}

/// The terrain's materials at a point, as the game's grass reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialAt {
    pub material0: u8,
    pub material1: u8,
    /// 0 (only `material0`) to 255 (only `material1`), filtered.
    pub blend: u8,
}

/// Thread-safe height lookups with a small tile cache. Cheap to clone.
#[derive(Clone)]
pub struct HeightSampler {
    source: Arc<TerrainSource>,
    cache: Arc<Mutex<Cache>>,
}

/// Loaded tiles (`None`: known not to exist) with the clock of their last use.
type TileCache<T> = HashMap<TileId, (Option<Arc<T>>, u64)>;

#[derive(Default)]
struct Cache {
    heights: TileCache<HeightTile>,
    water: TileCache<WaterTile>,
    grass: TileCache<GrassTile>,
    materials: TileCache<MaterialTile>,
    clock: u64,
}

impl HeightSampler {
    pub fn new(source: Arc<TerrainSource>) -> Self {
        Self {
            source,
            cache: Arc::default(),
        }
    }

    /// The finest existing tile containing `tile`'s area (the tile itself or
    /// its deepest ancestor with data), with its heights.
    pub fn finest_covering(&self, tile: TileId) -> Option<(TileId, Arc<HeightTile>)> {
        let max_lod = self.source.max_lod();
        let mut candidate = Some(tile);
        while let Some(t) = candidate {
            if t.lod() <= max_lod
                && self.source.may_have(t)
                && let Some(heights) =
                    self.cached(|c| &mut c.heights, t, || self.source.load_height(t))
            {
                return Some((t, heights));
            }
            candidate = t.parent();
        }
        None
    }

    /// The finest tile at or above `tile` with water data, with the data.
    /// Water often stops at a coarser level than the terrain.
    pub fn water_covering(&self, tile: TileId) -> Option<(TileId, Arc<WaterTile>)> {
        let mut candidate = Some(tile);
        while let Some(t) = candidate {
            if self.source.may_have_water(t)
                && let Some(water) = self.cached(|c| &mut c.water, t, || self.source.load_water(t))
            {
                return Some((t, water));
            }
            candidate = t.parent();
        }
        None
    }

    /// The finest tile at or above `tile` with grass data, with the data.
    pub fn grass_covering(&self, tile: TileId) -> Option<(TileId, Arc<GrassTile>)> {
        let mut candidate = Some(tile);
        while let Some(t) = candidate {
            if self.source.may_have_grass(t)
                && let Some(grass) = self.cached(|c| &mut c.grass, t, || self.source.load_grass(t))
            {
                return Some((t, grass));
            }
            candidate = t.parent();
        }
        None
    }

    /// The grass at `(x, z)`, if the map has grass data there.
    pub fn grass_at(&self, x: f32, z: f32) -> Option<GrassAt> {
        let finest = TileId::containing(MAX_LOD, x, z)?;
        let (tile, grass) = self.grass_covering(finest)?;
        let (min_x, min_z) = tile.world_min();
        let scale = (GRASS_SAMPLES - 1) as f32 / tile.world_size();
        let (height, color) = grass.sample((x - min_x) * scale, (z - min_z) * scale);
        Some(GrassAt { height, color })
    }

    /// The water surface above or below `(x, z)`, if the map has water data
    /// there. The surface extends under dry land: compare with the terrain.
    pub fn water_at(&self, x: f32, z: f32) -> Option<WaterAt> {
        let finest = TileId::containing(MAX_LOD, x, z)?;
        let (tile, water) = self.water_covering(finest)?;
        let (min_x, min_z) = tile.world_min();
        let scale = (WATER_SAMPLES - 1) as f32 / tile.world_size();
        let (u, v) = ((x - min_x) * scale, (z - min_z) * scale);
        Some(WaterAt {
            height: water.height(u, v),
            kind: water.kind(u, v),
        })
    }

    /// The materials at `(x, z)` from the finest tile with them, sampled as
    /// the game's grass does (`0x036982a4`, Wii U): both materials from the
    /// nearest sample, the blend filtered between the four around.
    pub fn material_at(&self, x: f32, z: f32) -> Option<MaterialAt> {
        let finest = TileId::containing(MAX_LOD, x, z)?;
        let max_lod = self.source.max_lod();
        let mut candidate = Some(finest);
        let (tile, materials) = loop {
            let t = candidate?;
            if t.lod() <= max_lod
                && self.source.may_have(t)
                && let Some(materials) =
                    self.cached(|c| &mut c.materials, t, || self.source.load_material(t))
            {
                break (t, materials);
            }
            candidate = t.parent();
        };
        let (min_x, min_z) = tile.world_min();
        let scale = (TILE_SAMPLES - 1) as f32 / tile.world_size();
        let last = (TILE_SAMPLES - 1) as f32;
        let (u, v) = (
            ((x - min_x) * scale).clamp(0.0, last),
            ((z - min_z) * scale).clamp(0.0, last),
        );
        let nearest = materials.get(u.round() as usize, v.round() as usize);
        let (u0, v0) = (
            (u.floor() as usize).min(TILE_SAMPLES - 2),
            (v.floor() as usize).min(TILE_SAMPLES - 2),
        );
        let (fu, fv) = (u - u0 as f32, v - v0 as f32);
        let blend = |i: usize, j: usize| (materials.get(i, j).blend as f32 + 0.5) / 255.0;
        let top = blend(u0, v0) + (blend(u0 + 1, v0) - blend(u0, v0)) * fu;
        let bottom = blend(u0, v0 + 1) + (blend(u0 + 1, v0 + 1) - blend(u0, v0 + 1)) * fu;
        let blend = (top + (bottom - top) * fv).clamp(0.0, 1.0);
        Some(MaterialAt {
            material0: nearest.material0,
            material1: nearest.material1,
            blend: (blend * 255.0 + 0.5) as u8,
        })
    }

    /// World height at `(x, z)`, or `None` outside the terrain grid.
    pub fn height_at(&self, x: f32, z: f32) -> Option<f32> {
        let finest = TileId::containing(MAX_LOD, x, z)?;
        let (tile, heights) = self.finest_covering(finest)?;
        let (min_x, min_z) = tile.world_min();
        let scale = (TILE_SAMPLES - 1) as f32 / tile.world_size();
        Some(heights.sample((x - min_x) * scale, (z - min_z) * scale))
    }

    /// A tile from `pick`'s cache, loading it with `load` on a miss.
    fn cached<T>(
        &self,
        pick: fn(&mut Cache) -> &mut TileCache<T>,
        tile: TileId,
        load: impl FnOnce() -> Result<Option<T>, String>,
    ) -> Option<Arc<T>> {
        {
            let mut cache = self.cache.lock().unwrap();
            cache.clock += 1;
            let clock = cache.clock;
            if let Some((data, used)) = pick(&mut cache).get_mut(&tile) {
                *used = clock;
                return data.clone();
            }
        }
        // Load outside the lock so other threads are not held up by file I/O.
        let data = match load() {
            Ok(data) => data.map(Arc::new),
            Err(error) => {
                bevy::log::warn!("tile {}: {error}", tile.file_stem());
                None
            }
        };
        let mut cache = self.cache.lock().unwrap();
        let clock = cache.clock;
        let tiles = pick(&mut cache);
        tiles.insert(tile, (data.clone(), clock));
        if tiles.len() > CACHE_TILES
            && let Some(oldest) = tiles
                .iter()
                .min_by_key(|(_, (_, used))| *used)
                .map(|(t, _)| *t)
        {
            tiles.remove(&oldest);
        }
        data
    }
}
