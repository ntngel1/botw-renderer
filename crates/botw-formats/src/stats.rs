//! World statistics maps (`Game/Stats/archive/*.sstats`): per quarter of a
//! map tile (500 m square, e.g. `E-4.01`), a Yaz0 SARC of named grids such
//! as `terrain_hidden`, `forest_density` or `route_distance`, each entry
//! `<map>/<quarter>.<map>.agstats`. The game's map manager (`0x1046d688`)
//! finds a grid by name (`0x032965b8`) and samples it at a world point
//! (`0x032980c8`); see archived FORMATS.md notes, "World statistics maps".
//!
//! An `.agstats` grid, big-endian:
//!
//! | Offset | Type | Value |
//! |---|---|---|
//! | 0x00 | `STAT` | magic |
//! | 0x04 | u32 | version (0x00010000) |
//! | 0x08 | u32 | grids (1) |
//! | 0x0c | u32 | 0x28, the grid header's size |
//! | 0x10 | f32 × 4 | x₀, z₀, width, depth in metres |
//! | 0x20 | u32 × 2 | columns, rows |
//! | 0x28 | u32 | value type (table below) |
//! | 0x2c | u32 | values a sample |
//! | 0x30 | u32 | offset of the values from here (8) |
//! | 0x34 | u32 | their size |
//!
//! Values run row by row (+z), a row along +x. Value types as stored
//! (strides `0x102a2a9c`): 0/1/2 unsigned 8/16/32-bit, 3/4/5 signed
//! 8/16/32-bit, 6 half, 7 float, 8/9 unsigned 8/16-bit, 10 raw 32-bit,
//! 11/12 signed 8/16-bit, 13 float. The only sampler found, `0x032980c8`,
//! puts out u32: types 0–5 integer-divided by their largest value, half
//! and float truncated, the rest as they are (archived FORMATS.md notes).

use std::path::Path;

use crate::content::ContentRoots;
use crate::{FormatError, Result};

/// World extent the game samples the grids in (`0x032980c8`): |x| < 5000,
/// |z| < 4000.
pub const WORLD_HALF: (f32, f32) = (5000.0, 4000.0);
/// Side of a quarter tile in metres.
pub const QUARTER: f32 = 500.0;

/// One grid of a statistics archive.
#[derive(Clone, Debug, PartialEq)]
pub struct StatGrid {
    pub x0: f32,
    pub z0: f32,
    pub width: f32,
    pub depth: f32,
    pub columns: u32,
    pub rows: u32,
    pub kind: u32,
    pub channels: u32,
    /// The values, as stored (big-endian).
    pub data: Vec<u8>,
}

/// Bytes a value of each type takes (`0x102a2a9c`).
fn stride(kind: u32) -> Option<usize> {
    Some(match kind {
        0 | 3 | 8 | 11 => 1,
        1 | 4 | 6 | 9 | 12 => 2,
        2 | 5 | 7 | 10 | 13 => 4,
        _ => return None,
    })
}

impl StatGrid {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let u32_at = |at: usize| {
            bytes
                .get(at..at + 4)
                .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
                .ok_or(FormatError::Invalid("agstats: truncated header"))
        };
        let f32_at = |at: usize| u32_at(at).map(f32::from_bits);
        if bytes.get(..4) != Some(b"STAT") {
            return Err(FormatError::Invalid("agstats: no STAT magic"));
        }
        let (columns, rows, kind, channels) =
            (u32_at(0x20)?, u32_at(0x24)?, u32_at(0x28)?, u32_at(0x2c)?);
        let stride = stride(kind).ok_or(FormatError::Invalid("agstats: unknown value type"))?;
        let start = 0x30 + u32_at(0x30)? as usize;
        let len = columns as usize * rows as usize * channels as usize * stride;
        let data = bytes
            .get(start..start + len)
            .ok_or(FormatError::Invalid("agstats: values past the end"))?
            .to_vec();
        Ok(Self {
            x0: f32_at(0x10)?,
            z0: f32_at(0x14)?,
            width: f32_at(0x18)?,
            depth: f32_at(0x1c)?,
            columns,
            rows,
            kind,
            channels,
            data,
        })
    }

    /// The sample covering a world point (column, row), clamped to the grid
    /// as the game does; `None` outside it.
    pub fn cell(&self, x: f32, z: f32) -> Option<(u32, u32)> {
        let (u, v) = ((x - self.x0) / self.width, (z - self.z0) / self.depth);
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return None;
        }
        let column = ((u * self.columns as f32) as u32).min(self.columns - 1);
        let row = ((v * self.rows as f32) as u32).min(self.rows - 1);
        Some((column, row))
    }

    /// A raw 32-bit value (type 10) of a sample.
    pub fn raw32(&self, column: u32, row: u32) -> Option<u32> {
        if self.kind != 10 {
            return None;
        }
        let at = ((row * self.columns + column) * self.channels) as usize * 4;
        self.data
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
    }

    /// For the bit masks (`terrain_hidden`, `terrain_embedded_edge`,
    /// `terrain_is_in_door`: type 10, 5 m samples): whether the square
    /// metre at world `(x, z)` (its corner) has its bit. A sample holds
    /// 5 × 5 bits, bit `5·(z − z₅) + (x − x₅)` from its corner `(x₅, z₅)`
    /// on the world's 5 m grid (`0x035bd074`).
    pub fn metre_bit(&self, x: i32, z: i32) -> bool {
        let (x5, z5) = (x.div_euclid(5) * 5, z.div_euclid(5) * 5);
        let Some((column, row)) = self.cell(x5 as f32 + 0.5, z5 as f32 + 0.5) else {
            return false;
        };
        let bit = 5 * (z - z5) + (x - x5);
        self.raw32(column, row).is_some_and(|v| v >> bit & 1 == 1)
    }
}

/// The quarter tile holding a world point, e.g. `E-4.01`: tile columns A–J
/// from x = −5000, rows 1–8 from z = −4000, then the quarter's x and z
/// halves.
pub fn quarter_name(x: f32, z: f32) -> Option<String> {
    if x.abs() >= WORLD_HALF.0 || z.abs() >= WORLD_HALF.1 {
        return None;
    }
    let qx = ((x + WORLD_HALF.0) / QUARTER).floor() as u32;
    let qz = ((z + WORLD_HALF.1) / QUARTER).floor() as u32;
    let column = char::from(b'A' + (qx / 2) as u8);
    Some(format!("{column}-{}.{}{}", qz / 2 + 1, qx % 2, qz % 2))
}

/// Reads the named grids of a quarter's archive (missing names are left
/// out).
pub fn read_quarter(
    bytes: &[u8],
    quarter: &str,
    names: &[&str],
) -> Result<Vec<(String, StatGrid)>> {
    let sarc_bytes = crate::yaz0::decompress_if(bytes)?;
    let sarc = roead::sarc::Sarc::new(sarc_bytes.as_ref())?;
    let mut grids = Vec::new();
    for name in names {
        if let Some(data) = sarc.get_data(&format!("{name}/{quarter}.{name}.agstats")) {
            grids.push((name.to_string(), StatGrid::parse(data)?));
        }
    }
    Ok(grids)
}

/// Loads the named grids of the quarter at a world point from the dump.
/// `Ok(None)` when the dump has no archive for it.
pub fn load_quarter(
    roots: &ContentRoots,
    x: f32,
    z: f32,
    names: &[&str],
) -> Result<Option<Vec<(String, StatGrid)>>> {
    let Some(quarter) = quarter_name(x, z) else {
        return Ok(None);
    };
    let Some(path) = roots.find(Path::new("Game/Stats/archive").join(format!("{quarter}.sstats")))
    else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
    read_quarter(&bytes, &quarter, names).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(x0: f32, z0: f32, values: &[u32]) -> Vec<u8> {
        let mut bytes = b"STAT".to_vec();
        for v in [0x0001_0000u32, 1, 0x28] {
            bytes.extend(v.to_be_bytes());
        }
        for v in [x0, z0, 10.0, 10.0] {
            bytes.extend(v.to_bits().to_be_bytes());
        }
        for v in [2u32, 2, 10, 1, 8, values.len() as u32 * 4] {
            bytes.extend(v.to_be_bytes());
        }
        for v in values {
            bytes.extend(v.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn names_the_quarter_of_a_point() {
        assert_eq!(quarter_name(-4999.0, -3999.0).as_deref(), Some("A-1.00"));
        assert_eq!(quarter_name(-800.0, -300.0).as_deref(), Some("E-4.01"));
        assert_eq!(quarter_name(-300.0, -800.0).as_deref(), Some("E-4.10"));
        assert_eq!(quarter_name(4999.0, 3999.0).as_deref(), Some("J-8.11"));
        assert_eq!(quarter_name(5000.0, 0.0), None);
    }

    #[test]
    fn reads_the_metre_bits_of_a_mask() {
        // 2 × 2 samples of 5 m from (−10, 20): the second sample of the
        // first row has the bit of its metre (1, 2) set.
        let bytes = grid(-10.0, 20.0, &[0, 1 << (5 * 2 + 1), 0, 0]);
        let mask = StatGrid::parse(&bytes).unwrap();
        assert_eq!((mask.columns, mask.rows, mask.kind), (2, 2, 10));
        assert!(mask.metre_bit(-4, 22));
        assert!(!mask.metre_bit(-4, 21));
        assert!(!mask.metre_bit(-9, 22));
        assert!(!mask.metre_bit(100, 22));
    }

    #[test]
    fn rejects_a_short_grid() {
        let mut bytes = grid(0.0, 0.0, &[0, 0, 0, 0]);
        bytes.truncate(0x40);
        assert!(StatGrid::parse(&bytes).is_err());
        assert!(StatGrid::parse(b"NOPE").is_err());
    }

    #[test]
    fn reads_named_grids_of_an_archive() {
        let mut writer = roead::sarc::SarcWriter::new(roead::Endian::Big);
        writer.add_file(
            "terrain_hidden/E-4.01.terrain_hidden.agstats",
            grid(0.0, 0.0, &[7, 0, 0, 0]),
        );
        let archive = crate::yaz0::compress_stored(&writer.to_binary());
        let grids = read_quarter(
            &archive,
            "E-4.01",
            &["terrain_hidden", "terrain_is_in_door"],
        )
        .unwrap();
        assert_eq!(grids.len(), 1);
        assert_eq!(grids[0].0, "terrain_hidden");
        assert!(grids[0].1.metre_bit(2, 0));
    }
}
