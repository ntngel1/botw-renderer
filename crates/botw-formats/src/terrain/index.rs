//! Which terrain archives exist, and loading tiles out of them.

use std::collections::HashMap;
use std::path::PathBuf;

use super::{GrassTile, HeightTile, MAINFIELD_DIR, MaterialTile, TerrainKind, TileId, WaterTile};
use crate::content::ContentRoots;
use crate::{FormatError, Result, stera};

/// Index of the MainField terrain archives found in the content roots.
#[derive(Clone, Debug, Default)]
pub struct TerrainIndex {
    /// Keyed by the first tile of each archive.
    archives: HashMap<(TileId, TerrainKind), PathBuf>,
}

impl TerrainIndex {
    pub fn scan(roots: &ContentRoots) -> Self {
        let archives = roots
            .list_dir(MAINFIELD_DIR)
            .into_iter()
            .filter(|(name, _)| name.ends_with(".sstera"))
            .filter_map(|(name, path)| {
                let (tile, kind) = TileId::parse_name(&name)?;
                Some(((tile.archive_base(), kind), path))
            })
            .collect();
        Self { archives }
    }

    pub fn is_empty(&self) -> bool {
        self.archives.is_empty()
    }

    /// Number of archives of `kind` per level of detail.
    pub fn archive_counts(&self, kind: TerrainKind) -> [usize; super::MAX_LOD as usize + 1] {
        let mut counts = [0; super::MAX_LOD as usize + 1];
        for (tile, _) in self.archives.keys().filter(|(_, k)| *k == kind) {
            counts[tile.lod() as usize] += 1;
        }
        counts
    }

    /// Whether the archive that would hold `tile` exists. Archives may hold
    /// fewer than four tiles, so a load can still find the tile missing.
    pub fn may_have(&self, tile: TileId, kind: TerrainKind) -> bool {
        self.archives.contains_key(&(tile.archive_base(), kind))
    }

    /// Reads one entry of `kind` for `tile`; `Ok(None)` if absent.
    pub fn load_raw(&self, tile: TileId, kind: TerrainKind) -> Result<Option<Vec<u8>>> {
        let Some(path) = self.archives.get(&(tile.archive_base(), kind)) else {
            return Ok(None);
        };
        let bytes = std::fs::read(path).map_err(|source| FormatError::Io {
            path: path.clone(),
            source,
        })?;
        let wanted = tile.entry_name(kind);
        Ok(stera::read(&bytes)?
            .into_iter()
            .find(|entry| entry.name == wanted)
            .map(|entry| entry.data))
    }

    pub fn load_height(&self, tile: TileId) -> Result<Option<HeightTile>> {
        self.load_raw(tile, TerrainKind::Height)?
            .map(|bytes| HeightTile::parse(&bytes))
            .transpose()
    }

    pub fn load_material(&self, tile: TileId) -> Result<Option<MaterialTile>> {
        self.load_raw(tile, TerrainKind::Material)?
            .map(|bytes| MaterialTile::parse(&bytes))
            .transpose()
    }

    pub fn load_water(&self, tile: TileId) -> Result<Option<WaterTile>> {
        self.load_raw(tile, TerrainKind::Water)?.map(|bytes| WaterTile::parse(&bytes)).transpose()
    }

    pub fn load_grass(&self, tile: TileId) -> Result<Option<GrassTile>> {
        self.load_raw(tile, TerrainKind::Grass)?.map(|bytes| GrassTile::parse(&bytes)).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stera::Entry;

    #[test]
    fn scans_and_loads_tiles_from_archives() {
        let root = std::env::temp_dir().join(format!("botw-index-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("content").join(MAINFIELD_DIR);
        std::fs::create_dir_all(&dir).unwrap();

        // An archive at lod 1 holding only three of its four tiles.
        let heights = HeightTile::from_raw(vec![1234; 256 * 256]);
        let entries: Vec<Entry> = [0u16, 1, 3]
            .into_iter()
            .map(|i| Entry {
                name: TileId::new(1, i).unwrap().entry_name(TerrainKind::Height),
                data: heights.to_bytes(),
            })
            .collect();
        let base = TileId::new(1, 0).unwrap();
        std::fs::write(
            dir.join(base.archive_name(TerrainKind::Height)),
            stera::write(&entries, roead::Endian::Big),
        )
        .unwrap();
        std::fs::write(dir.join("unrelated.txt"), "x").unwrap();

        let (roots, _) = ContentRoots::resolve([&root]);
        let index = TerrainIndex::scan(&roots);
        assert_eq!(index.archive_counts(TerrainKind::Height)[1], 1);

        let tile3 = TileId::new(1, 3).unwrap();
        assert!(index.may_have(tile3, TerrainKind::Height));
        assert!(!index.may_have(tile3, TerrainKind::Material));
        assert_eq!(index.load_height(tile3).unwrap(), Some(heights));
        assert_eq!(index.load_height(TileId::new(1, 2).unwrap()).unwrap(), None);
        assert_eq!(index.load_material(tile3).unwrap(), None);
    }
}
