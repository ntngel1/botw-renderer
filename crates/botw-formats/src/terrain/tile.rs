//! Tile addressing and file naming.
//!
//! A tile is named `5{lod}{index:08X}` (e.g. `580000C0A0`). Tiles are stored
//! four siblings per archive, in `{first sibling}.{kind}.sstera`.

use super::{MAX_LOD, WORLD_SIZE, zorder};

/// A terrain tile: level of detail plus Z-order index within that level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    lod: u8,
    index: u16,
}

impl TileId {
    /// The single tile covering the whole world.
    pub const ROOT: TileId = TileId { lod: 0, index: 0 };

    /// Returns `None` if `lod` exceeds [`MAX_LOD`] or `index` is outside the level.
    pub fn new(lod: u8, index: u16) -> Option<Self> {
        (lod <= MAX_LOD && u32::from(index) < tiles_per_axis(lod) * tiles_per_axis(lod))
            .then_some(Self { lod, index })
    }

    /// Tile at grid position `(x, z)` of level `lod`.
    pub fn from_grid(lod: u8, x: u8, z: u8) -> Option<Self> {
        let per_axis = tiles_per_axis(lod);
        (lod <= MAX_LOD && u32::from(x) < per_axis && u32::from(z) < per_axis)
            .then(|| Self { lod, index: zorder::interleave(x, z) })
    }

    /// Tile of level `lod` containing the world position `(x, z)`, if inside the world.
    pub fn containing(lod: u8, x: f32, z: f32) -> Option<Self> {
        let size = tile_world_size(lod);
        let gx = ((x + WORLD_SIZE / 2.0) / size).floor();
        let gz = ((z + WORLD_SIZE / 2.0) / size).floor();
        if gx < 0.0 || gz < 0.0 || gx > 255.0 || gz > 255.0 {
            return None;
        }
        Self::from_grid(lod, gx as u8, gz as u8)
    }

    pub fn lod(self) -> u8 {
        self.lod
    }

    pub fn index(self) -> u16 {
        self.index
    }

    /// Grid position `(x, z)` within this tile's level.
    pub fn grid(self) -> (u8, u8) {
        zorder::deinterleave(self.index)
    }

    /// Edge length of this tile in world units.
    pub fn world_size(self) -> f32 {
        tile_world_size(self.lod)
    }

    /// World-space `(x, z)` of the tile corner with the smallest coordinates.
    pub fn world_min(self) -> (f32, f32) {
        let (gx, gz) = self.grid();
        let size = self.world_size();
        (
            -WORLD_SIZE / 2.0 + f32::from(gx) * size,
            -WORLD_SIZE / 2.0 + f32::from(gz) * size,
        )
    }

    pub fn parent(self) -> Option<TileId> {
        (self.lod > 0).then(|| TileId {
            lod: self.lod - 1,
            index: self.index >> 2,
        })
    }

    /// The four tiles one level down, in Z-order (−x−z, +x−z, −x+z, +x+z).
    pub fn children(self) -> Option<[TileId; 4]> {
        (self.lod < MAX_LOD).then(|| {
            std::array::from_fn(|i| TileId {
                lod: self.lod + 1,
                index: (self.index << 2) | i as u16,
            })
        })
    }

    /// First of the four siblings that share this tile's archive.
    pub fn archive_base(self) -> TileId {
        TileId {
            lod: self.lod,
            index: self.index & !0b11,
        }
    }

    /// File name stem, e.g. `580000C0A0`.
    pub fn file_stem(self) -> String {
        format!("5{}{:08X}", self.lod, self.index)
    }

    /// Name of the tile inside its archive, e.g. `580000C0A0.hght`.
    pub fn entry_name(self, kind: TerrainKind) -> String {
        format!("{}.{}", self.file_stem(), kind.extension())
    }

    /// Name of the archive holding this tile, e.g. `580000C0A0.hght.sstera`.
    pub fn archive_name(self, kind: TerrainKind) -> String {
        format!("{}.{}.sstera", self.archive_base().file_stem(), kind.extension())
    }

    /// Parses an archive entry or archive file name back into a tile and kind.
    pub fn parse_name(name: &str) -> Option<(TileId, TerrainKind)> {
        let name = name.strip_suffix(".sstera").unwrap_or(name);
        let (stem, ext) = name.split_once('.')?;
        let kind = TerrainKind::from_extension(ext)?;
        Some((Self::parse_stem(stem)?, kind))
    }

    /// Parses a bare tile name such as `580000C0A0`.
    pub fn parse_stem(stem: &str) -> Option<TileId> {
        let digits = stem.strip_prefix('5')?;
        if digits.len() != 9 || !digits.is_ascii() {
            return None;
        }
        let lod = digits[..1].parse::<u8>().ok()?;
        let index = u32::from_str_radix(&digits[1..], 16).ok()?;
        TileId::new(lod, u16::try_from(index).ok()?)
    }
}

/// Kinds of per-tile terrain data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TerrainKind {
    /// 256×256 heights, see [`super::hght`].
    Height,
    /// 256×256 material samples, see [`super::mate`].
    Material,
    /// Optional water surface (`.water.extm`), see [`super::water`].
    Water,
    /// Optional grass coverage (`.grass.extm`), see [`super::grass`].
    Grass,
}

impl TerrainKind {
    pub const ALL: [TerrainKind; 4] = [Self::Height, Self::Material, Self::Water, Self::Grass];

    pub fn extension(self) -> &'static str {
        match self {
            Self::Height => "hght",
            Self::Material => "mate",
            Self::Water => "water.extm",
            Self::Grass => "grass.extm",
        }
    }

    pub fn from_extension(ext: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.extension() == ext)
    }
}

fn tiles_per_axis(lod: u8) -> u32 {
    1 << lod.min(MAX_LOD)
}

fn tile_world_size(lod: u8) -> f32 {
    WORLD_SIZE / tiles_per_axis(lod) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_game_scheme() {
        let tile = TileId::new(8, 0xC0A3).unwrap();
        assert_eq!(tile.file_stem(), "580000C0A3");
        assert_eq!(tile.entry_name(TerrainKind::Height), "580000C0A3.hght");
        assert_eq!(tile.archive_name(TerrainKind::Height), "580000C0A0.hght.sstera");
        assert_eq!(TileId::ROOT.archive_name(TerrainKind::Water), "5000000000.water.extm.sstera");
    }

    #[test]
    fn parses_entry_and_archive_names() {
        let tile = TileId::new(5, 0x3FF).unwrap();
        for kind in TerrainKind::ALL {
            assert_eq!(TileId::parse_name(&tile.entry_name(kind)), Some((tile, kind)));
        }
        assert_eq!(
            TileId::parse_name("5500000000.grass.extm.sstera"),
            Some((TileId::new(5, 0).unwrap(), TerrainKind::Grass))
        );
        assert_eq!(TileId::parse_name("MainField.tscb"), None);
        assert_eq!(TileId::parse_name("5900000000.hght"), None, "lod 9 does not exist");
        assert_eq!(TileId::parse_name("5100000004.hght"), None, "lod 1 has 4 tiles");
    }

    #[test]
    fn quadtree_relations() {
        let root = TileId::ROOT;
        let children = root.children().unwrap();
        assert_eq!(children.map(|c| c.grid()), [(0, 0), (1, 0), (0, 1), (1, 1)]);
        for child in children {
            assert_eq!(child.parent(), Some(root));
        }
        assert_eq!(TileId::new(MAX_LOD, 0).unwrap().children(), None);
    }

    #[test]
    fn world_placement() {
        assert_eq!(TileId::ROOT.world_min(), (-8000.0, -8000.0));
        assert_eq!(TileId::ROOT.world_size(), 16_000.0);
        let tile = TileId::from_grid(2, 3, 1).unwrap();
        assert_eq!(tile.world_min(), (4000.0, -4000.0));
        assert_eq!(TileId::containing(2, 4100.0, -3900.0), Some(tile));
        assert_eq!(TileId::containing(0, 8001.0, 0.0), None);
    }
}
