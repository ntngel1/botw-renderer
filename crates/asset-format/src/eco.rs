//! Mirror of `botw-formats::eco`'s types and their runtime methods; the
//! parsing stays there. `bake` serializes the parsed values to RON, which
//! these deserialize.
//!
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

/// A run-length encoded map of the field (`.beco`): rows along Z, runs of
/// one value along X.
///
/// Layout (big-endian on Wii U, little-endian on Switch; the magic tells):
/// `u32` magic `0x00112233`, `i32` row count, `i32` divisor (metres per
/// cell), `u32` reserved; then one `i32` offset per row (in units of two
/// bytes, from the end of the offset table; the last offset ends the last
/// row); then per row runs of `(i16 value, i16 length)`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct EcoMap {
    divisor: i32,
    /// Start of each row's runs in `runs`, plus the end.
    rows: Vec<usize>,
    runs: Vec<(i16, i16)>,
}

impl EcoMap {
    pub const MAGIC: u32 = 0x0011_2233;

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
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct Ecosystem {
    pub map: EcoMap,
    pub areas: Vec<Area>,
}

impl Ecosystem {
    /// The area at world position (`x`, `z`).
    pub fn area_at(&self, x: f32, z: f32) -> Option<&Area> {
        self.map
            .at(x, z)
            .and_then(|n| usize::try_from(n).ok())
            .and_then(|n| self.areas.get(n))
    }

    /// The climate (index into [`CLIMATES`]) at world position (`x`, `z`);
    /// like the game, anything unknown is the first climate.
    pub fn climate_at(&self, x: f32, z: f32) -> usize {
        self.area_at(x, z)
            .and_then(|area| CLIMATES.iter().position(|c| *c == area.climate))
            .unwrap_or(0)
    }
}
