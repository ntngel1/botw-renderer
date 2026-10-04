//! ELink/SLink databases (`ELink2/ELink2DB.sbelnk`, `SLink2/SLink2DB.sbslnk`
//! in `Pack/Bootup.pack`): xlink2 resources ("XLNK") that say which effects
//! or sounds an actor's ELink user plays, and with what parameters (asset
//! name, emitter set, scale, offset, colour…).
//!
//! Layout from the xlink2 library in the decompilation (`lib/xlink2`,
//! `xlink2Resource.h`, `ResourceParamCreator`), big-endian on Wii U: a
//! 0x48-byte header; per user a CRC32 name hash, then per user an offset;
//! the parameter definitions (names, types, defaults); the asset parameter
//! table (each entry a 64-bit mask of the parameters it sets, then one
//! reference per set bit); tables the references point into (direct values,
//! random ranges, curves); and at the end the condition and name tables.
//! Each user starts with a 0x30-byte header, then its local properties,
//! user parameters, sorted asset ids and asset call tables (0x20 bytes).
//! Only what finding an actor's assets needs is read.

use crate::{FormatError, Result};

/// A parsed xlink2 database.
pub struct XLink<'a> {
    data: &'a [u8],
    users: Vec<(u32, u32)>,
    num_user_params: usize,
    /// Asset parameter definitions: name and type.
    asset_params: Vec<ParamDefine>,
    asset_table: usize,
    direct_values: usize,
    random_table: usize,
    curve_table: usize,
    curve_points: usize,
    name_table: usize,
    condition_table: usize,
}

/// A parameter's name, type and default value (raw bits).
#[derive(Clone, Debug, PartialEq)]
pub struct ParamDefine {
    pub name: String,
    /// 0 u32, 1 float, 2 bool, 3 enum, 4 string, 5 arrange.
    pub kind: u32,
    pub default: u32,
}

/// A parameter value of an asset.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i32),
    Float(f32),
    Str(String),
    /// A random value between the two, drawn as the spread says.
    Random(f32, f32, Spread),
    /// A value following a property (e.g. the distance, or how hard it
    /// rains) along a curve of `(property, value)` points.
    Curve {
        property: String,
        kind: u16,
        points: Vec<(f32, f32)>,
    },
    /// Another kind of reference (curve, arrange group…): type and index.
    Other(u8, u32),
}

impl Value {
    /// The value as a number, if it is one (the middle of a random range).
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(i) => Some(*i as f32),
            // SI-AUD-18: Random values as the middle of their range.
            Value::Random(lo, hi, _) => Some((lo + hi) / 2.0),
            _ => None,
        }
    }
}

/// How a random reference draws its value: the reference types 3 and 6..=17
/// (`xlink2Types.h` `ValueReferenceType`; Wii U v208 `0x03b98990`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Spread {
    /// `Random`: uniform.
    Uniform,
    /// `Random{2,3,4,1Point5}Pow`: around the middle, `|2r - 1|` raised to
    /// the power pushes values towards it.
    Centered(f32),
    /// `Random*PowWeightMin`: `r` raised to the power, towards `min`.
    WeightMin(f32),
    /// `Random*PowWeightMax`: `1 - r` raised to the power, towards `max`.
    WeightMax(f32),
}

impl Spread {
    /// The reference type's spread, if it is a random one.
    pub fn from_ref_type(kind: u8) -> Option<Self> {
        const POWERS: [f32; 4] = [2.0, 3.0, 4.0, 1.5];
        Some(match kind {
            3 => Spread::Uniform,
            6..=9 => Spread::Centered(POWERS[kind as usize - 6]),
            10..=13 => Spread::WeightMin(POWERS[kind as usize - 10]),
            14..=17 => Spread::WeightMax(POWERS[kind as usize - 14]),
            _ => return None,
        })
    }

    /// The value for a uniform `r` in `[0, 1)` (`getRandomValue`,
    /// `getRandomValueWeightMin/Max`; Wii U `0x03b986c4`, `0x03b98744`,
    /// `0x03b98838`, `0x03b988d8`).
    pub fn sample(self, min: f32, max: f32, r: f32) -> f32 {
        match self {
            Spread::Uniform => min + (max - min) * r,
            Spread::Centered(power) => {
                let half = (max - min).abs() / 2.0;
                let signed = 2.0 * r - 1.0;
                let offset = signed.abs().powf(power) * half;
                if signed < 0.0 {
                    min + half - offset
                } else {
                    min + half + offset
                }
            }
            Spread::WeightMin(power) => min + r.powf(power) * (max - min).abs(),
            Spread::WeightMax(power) => min + (1.0 - r.powf(power)) * (max - min).abs(),
        }
    }
}

/// An entry of a user's asset call table: an asset (an effect or sound
/// with parameters) or a container choosing between its children.
#[derive(Clone, Debug, PartialEq)]
pub struct AssetCall {
    /// The key the game triggers it by (e.g. an action or property name).
    pub key: String,
    pub asset_id: i16,
    pub is_container: bool,
    pub duration: i32,
    pub parent_index: i32,
    /// The parameters it sets (assets only), by name.
    pub params: Vec<(String, Value)>,
    /// Relative selection weight in a random container, if present.
    pub random_weight: Option<f32>,
}

impl AssetCall {
    pub fn param(&self, name: &str) -> Option<&Value> {
        self.params.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

impl<'a> XLink<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let r = Reader(data);
        if r.slice(0, 4)? != b"XLNK" {
            return Err(FormatError::Invalid("xlink: not an XLNK resource"));
        }
        let h = |i: usize| r.u32(4 + 4 * i).map(|v| v as usize);
        let local_prop_pos = h(6)?;
        let (num_local_names, num_local_enums, num_direct) = (h(7)?, h(8)?, h(9)?);
        let (num_random, num_curves, num_user, name_table) = (h(10)?, h(11)?, h(14)?, h(16)?);
        let users = (0..num_user)
            .map(|i| Ok((r.u32(0x48 + 4 * i)?, r.u32(0x48 + 4 * (num_user + i))?)))
            .collect::<Result<_>>()?;

        // Parameter definitions: size, user, asset, custom asset and trigger
        // parameter counts, then 12-byte definitions and their strings.
        let defines = 0x48 + 8 * num_user;
        let define_size = r.u32(defines)? as usize;
        let num_user_params = r.u32(defines + 4)? as usize;
        let num_asset_params = r.u32(defines + 8)? as usize;
        let num_trigger_params = r.u32(defines + 16)? as usize;
        let first = defines + 20;
        let strings = first + 12 * (num_user_params + num_asset_params + num_trigger_params);
        let asset_params = (0..num_asset_params)
            .map(|i| {
                let at = first + 12 * (num_user_params + i);
                Ok(ParamDefine {
                    name: r.cstr(strings + r.u32(at)? as usize)?,
                    kind: r.u32(at + 4)?,
                    default: r.u32(at + 8)?,
                })
            })
            .collect::<Result<_>>()?;

        let direct_values = local_prop_pos + 4 * (num_local_names + num_local_enums);
        let random_table = direct_values + 4 * num_direct;
        let curve_table = random_table + 8 * num_random;
        Ok(Self {
            data,
            users,
            num_user_params,
            asset_params,
            asset_table: defines + define_size,
            direct_values,
            random_table,
            curve_table,
            curve_points: curve_table + 0x14 * num_curves,
            name_table,
            condition_table: h(15)?,
        })
    }

    /// The asset parameters' definitions.
    pub fn asset_params(&self) -> &[ParamDefine] {
        &self.asset_params
    }

    /// The asset call table of the user named `name`, if there is one.
    pub fn user(&self, name: &str) -> Result<Option<Vec<AssetCall>>> {
        let hash = crc32(name.as_bytes());
        let Some(&(_, offset)) = self.users.iter().find(|(h, _)| *h == hash) else {
            return Ok(None);
        };
        let r = Reader(self.data);
        let user = offset as usize;
        let num_local = r.u32(user + 4)? as usize;
        let num_calls = r.u32(user + 8)? as usize;
        let sorted_ids = user + 0x30 + 4 * num_local + 4 * self.num_user_params;
        let calls = sorted_ids + 2 * (num_calls + (num_calls & 1));
        (0..num_calls)
            .map(|i| self.call(calls + 0x20 * i))
            .collect::<Result<_>>()
            .map(Some)
    }

    fn call(&self, at: usize) -> Result<AssetCall> {
        let r = Reader(self.data);
        let flag = r.u16(at + 6)?;
        let is_container = flag & 1 != 0;
        let params = if is_container {
            Vec::new()
        } else {
            self.asset_values(self.asset_table + r.u32(at + 0x18)? as usize)?
        };
        let condition = r.u32(at + 0x1c)?;
        let random_weight = if condition != u32::MAX {
            let condition = self.condition_table + condition as usize;
            match r.u32(condition)? {
                // ResRandomCondition, also used by Random2 containers.
                1 | 2 => Some(r.f32(condition + 4)?),
                _ => None,
            }
        } else {
            None
        };
        Ok(AssetCall {
            key: r.cstr(self.name_table + r.u32(at)? as usize)?,
            asset_id: r.u16(at + 4)? as i16,
            is_container,
            duration: r.u32(at + 8)? as i32,
            parent_index: r.u32(at + 12)? as i32,
            params,
            random_weight,
        })
    }

    /// An asset parameter entry: which parameters it sets, and their values.
    fn asset_values(&self, at: usize) -> Result<Vec<(String, Value)>> {
        let r = Reader(self.data);
        let mask = (r.u32(at)? as u64) << 32 | r.u32(at + 4)? as u64;
        let mut out = Vec::new();
        let mut k = 0;
        for (idx, define) in self.asset_params.iter().enumerate().take(64) {
            if mask >> idx & 1 == 0 {
                continue;
            }
            let raw = r.u32(at + 8 + 4 * k)?;
            k += 1;
            let (kind, index) = ((raw >> 24) as u8, (raw & 0xFF_FFFF) as usize);
            let value = match kind {
                0 => {
                    let bits = r.u32(self.direct_values + 4 * index)?;
                    if define.kind == 1 {
                        Value::Float(f32::from_bits(bits))
                    } else {
                        Value::Int(bits as i32)
                    }
                }
                1 => Value::Str(r.cstr(self.name_table + index)?),
                2 => self.curve(index)?,
                kind @ (3 | 6..=17) => Value::Random(
                    r.f32(self.random_table + 8 * index)?,
                    r.f32(self.random_table + 8 * index + 4)?,
                    Spread::from_ref_type(kind).unwrap(),
                ),
                other => Value::Other(other, index as u32),
            };
            out.push((define.name.clone(), value));
        }
        Ok(out)
    }
}

impl XLink<'_> {
    /// Curve `index`: `u16` first point, point count, curve type, whether
    /// the property is global; `u32` property name; `s32` property index;
    /// `s16` local property. Points are `(x, y)` floats.
    // SI-AUD-19: only linear curve kind 0; property units are ours.
    fn curve(&self, index: usize) -> Result<Value> {
        let r = Reader(self.data);
        let at = self.curve_table + 0x14 * index;
        let (first, count, kind) = (r.u16(at)? as usize, r.u16(at + 2)? as usize, r.u16(at + 4)?);
        let property = r.cstr(self.name_table + r.u32(at + 8)? as usize)?;
        let points = (first..first + count)
            .map(|p| {
                Ok((
                    r.f32(self.curve_points + 8 * p)?,
                    r.f32(self.curve_points + 8 * p + 4)?,
                ))
            })
            .collect::<Result<_>>()?;
        Ok(Value::Curve {
            property,
            kind,
            points,
        })
    }
}

/// CRC32 (as `sead::HashCRC32`), which the user table is keyed by.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn slice(&self, at: usize, len: usize) -> Result<&[u8]> {
        at.checked_add(len)
            .and_then(|end| self.0.get(at..end))
            .ok_or(FormatError::Invalid("xlink: read past the end"))
    }

    fn u16(&self, at: usize) -> Result<u16> {
        Ok(u16::from_be_bytes(self.slice(at, 2)?.try_into().unwrap()))
    }

    fn u32(&self, at: usize) -> Result<u32> {
        Ok(u32::from_be_bytes(self.slice(at, 4)?.try_into().unwrap()))
    }

    fn f32(&self, at: usize) -> Result<f32> {
        self.u32(at).map(f32::from_bits)
    }

    fn cstr(&self, at: usize) -> Result<String> {
        let rest = self
            .0
            .get(at..)
            .ok_or(FormatError::Invalid("xlink: string past the end"))?;
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn be(v: u32) -> [u8; 4] {
        v.to_be_bytes()
    }

    #[test]
    fn random_spreads_lean_as_their_names_say() {
        let draws = |spread: Spread| -> Vec<f32> {
            (0..1000)
                .map(|i| spread.sample(2.0, 6.0, (i as f32 + 0.5) / 1000.0))
                .collect()
        };
        let share_below =
            |v: &[f32], x: f32| v.iter().filter(|&&n| n < x).count() as f32 / v.len() as f32;
        let middle = |v: &[f32]| {
            v.iter().filter(|&&n| (3.0..5.0).contains(&n)).count() as f32 / v.len() as f32
        };
        for kind in [3, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17] {
            let spread = Spread::from_ref_type(kind).unwrap();
            assert!(
                draws(spread).iter().all(|n| (2.0..=6.0).contains(n)),
                "{kind}"
            );
        }
        assert_eq!(Spread::from_ref_type(4), None);
        let uniform = draws(Spread::Uniform);
        assert!((middle(&uniform) - 0.5).abs() < 0.01);
        // Squaring |2r - 1| keeps 1/sqrt(2) of the draws in the middle half.
        assert!((middle(&draws(Spread::Centered(2.0))) - 0.5f32.sqrt()).abs() < 0.01);
        assert!(share_below(&draws(Spread::WeightMin(2.0)), 4.0) > 0.7);
        assert!(share_below(&draws(Spread::WeightMax(2.0)), 4.0) < 0.3);
        assert_eq!(Spread::WeightMax(3.0).sample(2.0, 6.0, 0.0), 6.0);
    }

    /// An asset call table entry.
    fn call(key: u32, id: i16, flag: u16, param: i32) -> Vec<u8> {
        let mut c = be(key).to_vec();
        c.extend((id as u16).to_be_bytes());
        c.extend(flag.to_be_bytes());
        for v in [1u32, u32::MAX, 0, 0, param as u32, u32::MAX] {
            c.extend(be(v));
        }
        c
    }

    /// A database with one user, `Test`, whose `Always` asset plays
    /// `Test_Set` at scale 2.5 with a random Y offset, and a container.
    pub fn sample() -> Vec<u8> {
        let names = b"Always\0Test_Set\0Loop\0";
        let (always, set, looping) = (0u32, 7u32, 16u32);
        let strings = b"AssetName\0RuntimeAssetName\0Scale\0PositionY\0\0\0\0";
        // Definitions: header, four 12-byte entries, strings.
        let mut defines = Vec::new();
        for v in [20 + 4 * 12 + strings.len() as u32, 0, 4, 0, 0] {
            defines.extend(be(v));
        }
        for (name, kind, default) in [
            (0u32, 4u32, 0u32),
            (10, 4, 0),
            (27, 1, 1.0f32.to_bits()),
            (33, 1, 0),
        ] {
            defines.extend([be(name), be(kind), be(default)].concat());
        }
        defines.extend(strings);
        // Asset parameters: RuntimeAssetName (a string), Scale (direct
        // value 0) and PositionY (random range 0).
        let mut assets = 0b1110u64.to_be_bytes().to_vec();
        assets.extend([be(1 << 24 | set), be(0), be(3 << 24)].concat());
        // Direct values, then random ranges.
        let tables = [
            be(2.5f32.to_bits()),
            be(1.0f32.to_bits()),
            be(3.0f32.to_bits()),
        ]
        .concat();
        // The user: header, sorted asset ids (padded to 4 bytes), calls.
        let mut user = Vec::new();
        for v in [0u32, 0, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0] {
            user.extend(be(v));
        }
        user.extend([0, 0, 0, 1]);
        user.extend(call(always, 0, 0, 0));
        user.extend(call(looping, -1, 1, -1));

        let defines_at = 0x48 + 8;
        let tables_at = defines_at + defines.len() + assets.len();
        let user_at = tables_at + tables.len();
        let names_at = user_at + user.len();
        let mut out = vec![0u8; 0x48];
        out[..4].copy_from_slice(b"XLNK");
        // Asset parameter entries, local property table (where the value
        // tables start), direct values, random ranges, users, conditions
        // and names.
        for (field, v) in [
            (3, 1),
            (6, tables_at),
            (9, 1),
            (10, 1),
            (14, 1),
            (15, names_at),
            (16, names_at),
        ] {
            out[4 + 4 * field..8 + 4 * field].copy_from_slice(&be(v as u32));
        }
        out.extend(be(crc32(b"Test")));
        out.extend(be(user_at as u32));
        out.extend(defines);
        out.extend(assets);
        out.extend(tables);
        out.extend(user);
        out.extend(names);
        out
    }

    #[test]
    fn reads_a_users_assets() {
        let bytes = sample();
        let db = XLink::parse(&bytes).unwrap();
        let names: Vec<_> = db.asset_params().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            ["AssetName", "RuntimeAssetName", "Scale", "PositionY"]
        );
        assert!(db.user("Other").unwrap().is_none());
        let calls = db.user("Test").unwrap().unwrap();
        assert_eq!(calls.len(), 2);
        let (always, looping) = (&calls[0], &calls[1]);
        assert_eq!(
            (
                always.key.as_str(),
                always.asset_id,
                always.is_container,
                always.parent_index
            ),
            ("Always", 0, false, -1)
        );
        assert_eq!(
            always.param("RuntimeAssetName"),
            Some(&Value::Str("Test_Set".into()))
        );
        assert_eq!(always.param("Scale"), Some(&Value::Float(2.5)));
        assert_eq!(
            always.param("PositionY"),
            Some(&Value::Random(1.0, 3.0, Spread::Uniform))
        );
        assert_eq!(always.param("AssetName"), None);
        assert!(looping.is_container && looping.params.is_empty());
        assert_eq!((looping.key.as_str(), looping.asset_id), ("Loop", -1));
    }

    #[test]
    fn rejects_other_files() {
        assert!(XLink::parse(b"SARC").is_err());
        assert!(XLink::parse(b"XLNK").is_err());
    }

    #[test]
    fn reads_random_container_weights_and_checks_their_bounds() {
        let mut bytes = sample();
        let user = u32::from_be_bytes(bytes[0x4c..0x50].try_into().unwrap()) as usize;
        let condition = bytes.len();
        bytes[0x40..0x44].copy_from_slice(&be(condition as u32));
        let reference = user + 0x30 + 4 + 0x1c;
        bytes[reference..reference + 4].copy_from_slice(&be(0));
        bytes.extend([be(1), be(0.3f32.to_bits())].concat());
        let calls = XLink::parse(&bytes).unwrap().user("Test").unwrap().unwrap();
        assert_eq!(calls[0].random_weight, Some(0.3));
        assert_eq!(calls[1].random_weight, None);
        bytes.pop();
        assert!(XLink::parse(&bytes).unwrap().user("Test").is_err());
    }

    #[test]
    fn hashes_names_like_sead() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
