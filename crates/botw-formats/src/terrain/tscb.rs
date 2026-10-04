//! `MainField.tscb`: the terrain scene. Lists every tile that exists (the
//! game calls them areas) with its height range and whether it has water or
//! grass data, plus the terrain material table.
//!
//! Layout verified against the Wii U 1.5.0 file (big-endian; the Switch file
//! is little-endian). Every offset in the file is relative to the position it
//! is stored at.
//!
//! ```text
//! 0x00 "TSCB"              0x04 version 0x0A000000   0x08 u32 1
//! 0x0C u32 string table    0x10 f32 world_scale      0x14 f32 max height
//! 0x18 u32 material count  0x1C u32 area count       0x20 u32 0, u32 0
//! 0x28 f32 tile size       0x2C u32 8
//! 0x30 offset to the area offset table
//! 0x34 material offset table, then materials (20 bytes each):
//!      u32 index, f32 u scale, f32 v scale, f32 unknown × 2
//! area: f32 x, z, size (tile units; × world_scale for world units),
//!       f32 min, max terrain height, f32 min, max water height (× max height;
//!       the empty range 1..0 when there is no water),
//!       u32 extra count, name offset, 0, 0,
//!       [u32 4, u32 4 × extra count, (extra count − 1) offsets to extras 2..,
//!        extras: u32 3, u32 kind (0 grass, 1 water), u32 1, u32 0]
//! ```

use super::TileId;
use crate::{FormatError, Result};

/// Parsed terrain scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Tscb {
    /// World units per tile-size unit (500 in MainField).
    pub world_scale: f32,
    /// World height of a normalised height of 1 (800 in MainField).
    pub max_height: f32,
    /// Edge length of the root area in tile-size units (32 in MainField).
    pub tile_size: f32,
    pub materials: Vec<MaterialInfo>,
    pub areas: Vec<Area>,
}

/// One entry of the terrain material table. `.mate` samples index into it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialInfo {
    pub index: u32,
    pub u_scale: f32,
    pub v_scale: f32,
    pub unknown: [f32; 2],
}

/// A tile that exists, in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Area {
    pub tile: TileId,
    pub center: [f32; 2],
    pub size: f32,
    pub min_height: f32,
    pub max_height: f32,
    /// Height range of the tile's water surface, if it has water.
    pub water: Option<[f32; 2]>,
    pub has_water_data: bool,
    pub has_grass_data: bool,
}

const EXTRA_GRASS: u32 = 0;
const EXTRA_WATER: u32 = 1;

impl Tscb {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.get(..4) != Some(b"TSCB") {
            return Err(FormatError::Invalid("tscb: missing TSCB magic"));
        }
        let r = Reader::new(bytes);
        let world_scale = r.f32(0x10)?;
        let max_height = r.f32(0x14)?;
        let material_count = r.u32(0x18)? as usize;
        let area_count = r.u32(0x1C)? as usize;
        let tile_size = r.f32(0x28)?;

        let materials = (0..material_count)
            .map(|i| {
                let at = r.follow(0x34 + 4 * i)?;
                Ok(MaterialInfo {
                    index: r.u32(at)?,
                    u_scale: r.f32(at + 4)?,
                    v_scale: r.f32(at + 8)?,
                    unknown: [r.f32(at + 12)?, r.f32(at + 16)?],
                })
            })
            .collect::<Result<_>>()?;

        let table = r.follow(0x30)?;
        let areas = (0..area_count)
            .map(|i| r.area(r.follow(table + 4 * i)?, world_scale, max_height))
            .collect::<Result<_>>()?;

        Ok(Self { world_scale, max_height, tile_size, materials, areas })
    }
}

/// Bounds-checked reads in the file's byte order.
struct Reader<'a> {
    bytes: &'a [u8],
    big_endian: bool,
}

impl<'a> Reader<'a> {
    /// The world scale is a small positive float in the right byte order and
    /// garbage in the wrong one.
    fn new(bytes: &'a [u8]) -> Self {
        let scale_be = bytes.get(0x10..0x14).map(|b| f32::from_be_bytes(b.try_into().unwrap()));
        let big_endian = scale_be.is_some_and(|s| (1.0..1.0e5).contains(&s));
        Self { bytes, big_endian }
    }

    fn u32(&self, at: usize) -> Result<u32> {
        let word: [u8; 4] = self
            .bytes
            .get(at..at + 4)
            .and_then(|b| b.try_into().ok())
            .ok_or(FormatError::Invalid("tscb: offset past the end of the file"))?;
        Ok(if self.big_endian { u32::from_be_bytes(word) } else { u32::from_le_bytes(word) })
    }

    fn f32(&self, at: usize) -> Result<f32> {
        self.u32(at).map(f32::from_bits)
    }

    /// Resolves the self-relative offset stored at `at`.
    fn follow(&self, at: usize) -> Result<usize> {
        Ok(at + self.u32(at)? as usize)
    }

    fn string(&self, at: usize) -> Result<&'a str> {
        let tail = self.bytes.get(at..).ok_or(FormatError::Invalid("tscb: string past the end"))?;
        let end = tail.iter().position(|&b| b == 0).ok_or(FormatError::Invalid("tscb: unterminated string"))?;
        std::str::from_utf8(&tail[..end]).map_err(|_| FormatError::Invalid("tscb: name is not UTF-8"))
    }

    fn area(&self, at: usize, world_scale: f32, max_height: f32) -> Result<Area> {
        let f = |i: usize| self.f32(at + 4 * i);
        let name = self.string(self.follow(at + 0x20)?)?;
        let tile = TileId::parse_stem(name).ok_or(FormatError::Invalid("tscb: area name is not a tile name"))?;

        let extra_count = self.u32(at + 0x1C)? as usize;
        let (mut has_water_data, mut has_grass_data) = (false, false);
        if extra_count > 0 {
            let offsets = at + 0x34;
            let first = offsets + 4 * (extra_count - 1);
            for i in 0..extra_count {
                let extra = if i == 0 { first } else { self.follow(offsets + 4 * (i - 1))? };
                if self.u32(extra)? != 3 {
                    return Err(FormatError::Invalid("tscb: unexpected extra info layout"));
                }
                match self.u32(extra + 4)? {
                    EXTRA_GRASS => has_grass_data = true,
                    EXTRA_WATER => has_water_data = true,
                    _ => {}
                }
            }
        }

        let (water_min, water_max) = (f(5)?, f(6)?);
        Ok(Area {
            tile,
            center: [f(0)? * world_scale, f(1)? * world_scale],
            size: f(2)? * world_scale,
            min_height: f(3)? * max_height,
            max_height: f(4)? * max_height,
            water: (water_min <= water_max).then_some([water_min * max_height, water_max * max_height]),
            has_water_data,
            has_grass_data,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds a big-endian TSCB with the same layout as the game's.
    pub(crate) fn build(areas: &[(TileId, [f32; 4], Vec<u32>)]) -> Vec<u8> {
        let mut out = Vec::new();
        let put = |out: &mut Vec<u8>, at: usize, word: u32| out[at..at + 4].copy_from_slice(&word.to_be_bytes());
        out.extend_from_slice(b"TSCB");
        for word in [0x0A00_0000u32, 1, 0, 500f32.to_bits(), 800f32.to_bits(), 1, areas.len() as u32, 0, 0] {
            out.extend_from_slice(&word.to_be_bytes());
        }
        out.extend_from_slice(&32f32.to_bits().to_be_bytes());
        out.extend_from_slice(&8u32.to_be_bytes());
        out.extend_from_slice(&[0; 8]); // 0x30 area table offset, 0x34 material offset
        put(&mut out, 0x34, 4);
        for word in [7u32, 0.1f32.to_bits(), 0.2f32.to_bits(), 0, 1f32.to_bits()] {
            out.extend_from_slice(&word.to_be_bytes());
        }
        let table = out.len();
        put(&mut out, 0x30, (table - 0x30) as u32);
        out.resize(table + 4 * areas.len(), 0);

        let mut names = Vec::new();
        for (i, (tile, [min, max, water_min, water_max], extras)) in areas.iter().enumerate() {
            let at = out.len();
            put(&mut out, table + 4 * i, (at - table - 4 * i) as u32);
            let size = 32.0 / f32::from(1u16 << tile.lod());
            let (x0, z0) = tile.world_min();
            let center = [(x0 / 500.0) + size / 2.0, (z0 / 500.0) + size / 2.0];
            for value in [center[0], center[1], size, *min, *max, *water_min, *water_max] {
                out.extend_from_slice(&value.to_bits().to_be_bytes());
            }
            for word in [extras.len() as u32, 0, 0, 0] {
                out.extend_from_slice(&word.to_be_bytes());
            }
            names.push((at + 0x20, tile.file_stem()));
            if !extras.is_empty() {
                out.extend_from_slice(&4u32.to_be_bytes());
                out.extend_from_slice(&(4 * extras.len() as u32).to_be_bytes());
                let offsets = out.len();
                out.resize(offsets + 4 * (extras.len() - 1), 0);
                for (k, kind) in extras.iter().enumerate() {
                    if k > 0 {
                        let slot = offsets + 4 * (k - 1);
                        let offset = (out.len() - slot) as u32;
                        put(&mut out, slot, offset);
                    }
                    for word in [3, *kind, 1, 0] {
                        out.extend_from_slice(&word.to_be_bytes());
                    }
                }
            }
        }
        let strings = out.len() as u32;
        put(&mut out, 0x0C, strings);
        for (slot, name) in names {
            let offset = (out.len() - slot) as u32;
            put(&mut out, slot, offset);
            out.extend_from_slice(name.as_bytes());
            out.push(0);
        }
        out
    }

    #[test]
    fn parses_header_materials_and_areas() {
        let child = TileId::new(1, 2).unwrap();
        let fine = TileId::new(8, 0xDADF).unwrap();
        let bytes = build(&[
            (TileId::ROOT, [0.0, 1.0, 1.0, 0.0], vec![]),
            (child, [0.25, 0.5, 0.0, 0.125], vec![EXTRA_WATER]),
            (fine, [0.1, 0.2, 0.1, 0.1], vec![EXTRA_GRASS, EXTRA_WATER]),
        ]);
        let tscb = Tscb::parse(&bytes).unwrap();
        assert_eq!((tscb.world_scale, tscb.max_height, tscb.tile_size), (500.0, 800.0, 32.0));
        assert_eq!(tscb.materials, [MaterialInfo { index: 7, u_scale: 0.1, v_scale: 0.2, unknown: [0.0, 1.0] }]);

        let root = tscb.areas[0];
        assert_eq!((root.tile, root.center, root.size), (TileId::ROOT, [0.0, 0.0], 16_000.0));
        assert_eq!((root.min_height, root.max_height, root.water), (0.0, 800.0, None));
        assert!(!root.has_water_data && !root.has_grass_data);

        let area = tscb.areas[1];
        assert_eq!((area.tile, area.center, area.size), (child, [-4000.0, 4000.0], 8000.0));
        assert_eq!(area.water, Some([0.0, 100.0]));
        assert!(area.has_water_data && !area.has_grass_data);

        let area = tscb.areas[2];
        assert_eq!(area.tile, fine);
        assert_eq!(area.center, [4968.75, 3718.75]);
        assert!(area.has_water_data && area.has_grass_data);
    }

    #[test]
    fn rejects_other_files() {
        assert!(Tscb::parse(b"SARC....").is_err());
        assert!(Tscb::parse(b"TSCB").is_err(), "truncated");
    }
}
