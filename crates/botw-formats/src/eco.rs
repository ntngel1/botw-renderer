//! The ecosystem map: which area (and so which climate) every point of the
//! field belongs to.
//!
//! The game reads it in `ksys::eco::Ecosystem` (decompilation
//! `ecoSystem.cpp`) from `Pack/Bootup.pack`:
//! - `Ecosystem/FieldMapArea.sbeco` (Yaz0): the area number at each metre of
//!   the map, run-length encoded per row (see [`EcoMap`]);
//! - `Ecosystem/AreaData.sbyml` (Yaz0 BYML): one entry per area number with
//!   its name (`Area`), climate (`Climate`) and ambient sound (`EnvSound`),
//!   plus what lives there.
//!
//! `ksys::world::Manager::getClimate` maps a position to a climate through
//! both; the climate then picks the weather odds, temperatures and light
//! (`ClimateDefines_N` in `normal.bwinfo`, see [`crate::env`]).

use roead::byml::Byml;

use crate::content::ContentRoots;
use crate::{FormatError, Result};

/// A run-length encoded map of the field (`.beco`): rows along Z, runs of
/// one value along X.
///
/// Layout (big-endian on Wii U, little-endian on Switch; the magic tells):
/// `u32` magic `0x00112233`, `i32` row count, `i32` divisor (metres per
/// cell), `u32` reserved; then one `i32` offset per row (in units of two
/// bytes, from the end of the offset table; the last offset ends the last
/// row); then per row runs of `(i16 value, i16 length)`.
#[derive(serde::Serialize, Clone, Debug)]
pub struct EcoMap {
    divisor: i32,
    /// Start of each row's runs in `runs`, plus the end.
    rows: Vec<usize>,
    runs: Vec<(i16, i16)>,
}

impl EcoMap {
    pub const MAGIC: u32 = 0x0011_2233;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let header = bytes.get(..16).ok_or(FormatError::Invalid("beco: too short"))?;
        let big = match (u32::from_be_bytes(header[..4].try_into().unwrap()), u32::from_le_bytes(header[..4].try_into().unwrap())) {
            (Self::MAGIC, _) => true,
            (_, Self::MAGIC) => false,
            _ => return Err(FormatError::Invalid("beco: bad magic")),
        };
        let word = |at: usize| -> Result<i32> {
            let raw: [u8; 4] = bytes.get(at..at + 4).ok_or(FormatError::Invalid("beco: truncated"))?.try_into().unwrap();
            Ok(if big { i32::from_be_bytes(raw) } else { i32::from_le_bytes(raw) })
        };
        let half = |at: usize| -> Result<i16> {
            let raw: [u8; 2] = bytes.get(at..at + 2).ok_or(FormatError::Invalid("beco: truncated"))?.try_into().unwrap();
            Ok(if big { i16::from_be_bytes(raw) } else { i16::from_le_bytes(raw) })
        };
        let num_rows = word(4)?;
        let divisor = word(8)?;
        if num_rows < 2 || divisor < 1 {
            return Err(FormatError::Invalid("beco: bad header"));
        }
        let num_rows = num_rows as usize;
        let data = 16 + 4 * num_rows;
        let offsets = (0..num_rows).map(|i| word(16 + 4 * i).map(|o| o.max(0) as usize)).collect::<Result<Vec<_>>>()?;
        let end = *offsets.last().unwrap();
        if data + 2 * end > bytes.len() || offsets.windows(2).any(|w| w[0] > w[1]) {
            return Err(FormatError::Invalid("beco: bad row offsets"));
        }
        // Offsets count 2-byte units; a run is 4 bytes.
        let runs = (0..end / 2).map(|i| Ok((half(data + 4 * i)?, half(data + 4 * i + 2)?))).collect::<Result<Vec<_>>>()?;
        let rows = offsets.iter().map(|o| o / 2).collect();
        Ok(Self { divisor, rows, runs })
    }

    /// The value at world position (`x`, `z`), like
    /// `Ecosystem::getMapArea`: the map spans x −5000…5000 and z −4000…4000
    /// (positions outside are clamped); `None` where the row has no run
    /// there.
    pub fn at(&self, x: f32, z: f32) -> Option<i16> {
        let round = |v: f32| (v + if v >= 0.0 { 0.5 } else { -0.5 }) as i32;
        let x = x.clamp(-5000.0, 4999.0) + 5000.0;
        let z = z.clamp(-4000.0, 4000.0) + 4000.0;
        let mut column = round(x);
        let row = (round(z) / self.divisor).clamp(0, self.rows.len() as i32 - 2) as usize;
        if self.divisor == 10 {
            column /= 10;
        }
        let mut covered = 0;
        for &(value, length) in &self.runs[self.rows[row]..self.rows[row + 1]] {
            covered += i32::from(length);
            if column < covered {
                return Some(value);
            }
        }
        None
    }
}

/// One entry of `AreaData`.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Area {
    pub name: String,
    /// The climate's name, e.g. `DesertClimate` (see [`CLIMATES`]).
    pub climate: String,
    pub env_sound: String,
}

/// The game's climates in `ClimateDefines_N` order (`ksys::world::Climate`;
/// the misspelt `DarkWoodsClimat` is the game's).
pub const CLIMATES: [&str; 20] = [
    // Native game keys: keep them exact for dump and baked-asset lookup.
    "HyrulePlainClimate",
    "NorthHyrulePlainClimate",
    "HebraFrostClimate",
    "TabantaAridClimate",
    "FrostClimate",
    "GerudoDesertClimate",
    "GerudoPlateauClimate",
    "EldinClimateLv0",
    "TamourPlainClimate",
    "ZoraTemperateClimate",
    "HateruPlainClimate",
    "FiloneSubtropicalClimate",
    "SouthHateruHumidTemperateClimate",
    "EldinClimateLv1",
    "EldinClimateLv2",
    "DarkWoodsClimat",
    "LostWoodClimate",
    "GerudoFrostClimate",
    "KorogForest",
    "GerudoDesertClimateLv2",
];

/// The field's areas and the map of where they are.
#[derive(serde::Serialize, Clone, Debug)]
pub struct Ecosystem {
    pub map: EcoMap,
    pub areas: Vec<Area>,
}

impl Ecosystem {
    pub fn parse_areas(bytes: &[u8]) -> Result<Vec<Area>> {
        let doc = Byml::from_binary(bytes).map_err(|_| FormatError::Invalid("AreaData: not BYML"))?;
        let list = doc.as_array().map_err(|_| FormatError::Invalid("AreaData: not an array"))?;
        Ok(list
            .iter()
            .map(|entry| {
                let text = |key: &str| entry.as_map().ok().and_then(|m| m.get(key)).and_then(|v| v.as_string().ok()).cloned().unwrap_or_default();
                Area { name: text("Area").to_string(), climate: text("Climate").to_string(), env_sound: text("EnvSound").to_string() }
            })
            .collect())
    }

    /// Reads the map and the areas from `Pack/Bootup.pack`; `None` if the
    /// dump has no such pack.
    pub fn load(roots: &ContentRoots) -> Result<Option<Self>> {
        let Some(path) = roots.find("Pack/Bootup.pack") else { return Ok(None) };
        let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
        let pack = crate::yaz0::decompress_if(&pack)?;
        let sarc = roead::sarc::Sarc::new(&pack[..])?;
        let entry = |name: &'static str| -> Result<Vec<u8>> {
            let data = sarc.get_data(name).ok_or(FormatError::Invalid("Bootup.pack lacks an Ecosystem file"))?;
            Ok(crate::yaz0::decompress_if(data)?.into_owned())
        };
        let map = EcoMap::parse(&entry("Ecosystem/FieldMapArea.sbeco")?)?;
        let areas = Self::parse_areas(&entry("Ecosystem/AreaData.sbyml")?)?;
        Ok(Some(Self { map, areas }))
    }

    /// The area at world position (`x`, `z`).
    pub fn area_at(&self, x: f32, z: f32) -> Option<&Area> {
        self.map.at(x, z).and_then(|n| usize::try_from(n).ok()).and_then(|n| self.areas.get(n))
    }

    /// The climate (index into [`CLIMATES`]) at world position (`x`, `z`);
    /// like the game, anything unknown is the first climate.
    pub fn climate_at(&self, x: f32, z: f32) -> usize {
        self.area_at(x, z).and_then(|area| CLIMATES.iter().position(|c| *c == area.climate)).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rows (plus the end): row 0 is 3000 of area 1 then 7000 of 2,
    /// row 1 is all area 5. Big-endian like the Wii U.
    fn synthetic_map() -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [EcoMap::MAGIC, 3, 1, 0] {
            bytes.extend_from_slice(&word.to_be_bytes());
        }
        for offset in [0i32, 4, 6] {
            bytes.extend_from_slice(&offset.to_be_bytes());
        }
        for (value, length) in [(1i16, 3000i16), (2, 7000), (5, 10000)] {
            bytes.extend_from_slice(&value.to_be_bytes());
            bytes.extend_from_slice(&length.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn looks_up_runs_like_the_game() {
        let map = EcoMap::parse(&synthetic_map()).unwrap();
        // z −4000 is row 0; x −5000…−2001 is the first run.
        assert_eq!(map.at(-5000.0, -4000.0), Some(1));
        assert_eq!(map.at(-2001.0, -4000.0), Some(1));
        assert_eq!(map.at(-2000.0, -4000.0), Some(2));
        assert_eq!(map.at(4999.0, -4000.0), Some(2));
        // Everything further south clamps to the last row.
        assert_eq!(map.at(0.0, -3999.0), Some(5));
        assert_eq!(map.at(0.0, 9000.0), Some(5));
        assert!(EcoMap::parse(b"not a map at all").is_err());
    }

    #[test]
    fn reads_little_endian_maps_too() {
        let big = synthetic_map();
        let mut little = Vec::new();
        for chunk in big[..28].chunks(4) {
            little.extend(chunk.iter().rev());
        }
        for chunk in big[28..].chunks(2) {
            little.extend(chunk.iter().rev());
        }
        assert_eq!(EcoMap::parse(&little).unwrap().at(-2000.0, -4000.0), Some(2));
    }

    #[test]
    fn maps_areas_to_climates() {
        let map = EcoMap::parse(&synthetic_map()).unwrap();
        let area = |climate: &str| Area { name: String::new(), climate: climate.into(), env_sound: String::new() };
        let mut areas = vec![area("HyrulePlainClimate"); 6];
        areas[2] = area("GerudoDesertClimate");
        areas[5] = area("Unknown");
        let eco = Ecosystem { map, areas };
        assert_eq!(eco.climate_at(0.0, -4000.0), 5);
        assert_eq!(eco.climate_at(-4000.0, -4000.0), 0);
        assert_eq!(eco.climate_at(0.0, 0.0), 0);
    }
}
