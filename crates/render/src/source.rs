//! Where terrain tiles come from: the baked tiles in `assets/terrain/`.

use std::collections::HashMap;
use std::path::PathBuf;

use asset_format::paths;
use asset_format::terrain::{
    GrassTile, HeightTile, MAX_LOD, MaterialTile, TerrainIndex, TileFile, TileId, TileInfo,
    WaterTile,
};

/// Height and material data for one tile.
pub struct TileData {
    pub height: HeightTile,
    pub material: Option<MaterialTile>,
    pub water: Option<WaterTile>,
    pub grass: Option<GrassTile>,
}

/// The baked terrain: which tiles exist (`terrain/index.ron`) and their files.
pub struct TerrainSource {
    root: PathBuf,
    region: String,
    tiles: HashMap<TileId, TileInfo>,
    max_lod: u8,
}

impl TerrainSource {
    pub fn open(assets: PathBuf) -> Result<Self, String> {
        let index: TerrainIndex = asset_format::read_ron(&assets.join(paths::TERRAIN_INDEX))
            .map_err(|e| e.to_string())?;
        let max_lod = index.tiles.iter().map(|t| t.tile.lod()).max().unwrap_or(0);
        Ok(Self {
            root: assets,
            region: index.region,
            tiles: index.tiles.into_iter().map(|t| (t.tile, t)).collect(),
            max_lod,
        })
    }

    /// Short description for logs.
    pub fn describe(&self) -> String {
        format!(
            "baked terrain of {} ({} tiles)",
            self.region,
            self.tiles.len()
        )
    }

    /// The region the tiles were baked for.
    pub fn region(&self) -> &str {
        &self.region
    }

    /// Finest level of detail baked.
    pub fn max_lod(&self) -> u8 {
        self.max_lod.min(MAX_LOD)
    }

    /// Whether `tile` was baked.
    pub fn may_have(&self, tile: TileId) -> bool {
        self.tiles.contains_key(&tile)
    }

    /// Height range of `tile` without loading it.
    pub fn bounds(&self, tile: TileId) -> Option<(f32, f32)> {
        self.tiles.get(&tile).map(|t| (t.min_height, t.max_height))
    }

    /// Loads a tile; `Ok(None)` if it was not baked.
    pub fn load(&self, tile: TileId) -> Result<Option<TileData>, String> {
        if !self.may_have(tile) {
            return Ok(None);
        }
        let file = TileFile::read(&self.root.join(paths::terrain_tile(tile)))
            .map_err(|e| format!("{}: {e}", tile.file_stem()))?;
        Ok(Some(TileData {
            height: file.height,
            material: file.material,
            water: file.water,
            grass: file.grass,
        }))
    }

    /// Loads only a tile's heights.
    pub fn load_height(&self, tile: TileId) -> Result<Option<HeightTile>, String> {
        Ok(self.load(tile)?.map(|data| data.height))
    }

    /// Loads only a tile's materials (`.mate`).
    pub fn load_material(&self, tile: TileId) -> Result<Option<MaterialTile>, String> {
        Ok(self.load(tile)?.and_then(|data| data.material))
    }

    /// Whether `tile` has its own water data. Tiles without it use their
    /// nearest ancestor's water.
    pub fn may_have_water(&self, tile: TileId) -> bool {
        self.tiles.get(&tile).is_some_and(|t| t.water)
    }

    pub fn load_water(&self, tile: TileId) -> Result<Option<WaterTile>, String> {
        if !self.may_have_water(tile) {
            return Ok(None);
        }
        Ok(self.load(tile)?.and_then(|data| data.water))
    }

    /// Whether `tile` has grass data. The finest tile with it wins.
    pub fn may_have_grass(&self, tile: TileId) -> bool {
        self.tiles.get(&tile).is_some_and(|t| t.grass)
    }

    pub fn load_grass(&self, tile: TileId) -> Result<Option<GrassTile>, String> {
        if !self.may_have_grass(tile) {
            return Ok(None);
        }
        Ok(self.load(tile)?.and_then(|data| data.grass))
    }
}
