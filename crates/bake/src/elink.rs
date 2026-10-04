//! `effects/elink.ron`: the effect links (ELink, xlink2) the renderer
//! evaluates — the camera's (`Camera`: rain, snow, thunder, wind,
//! sandstorm, haze, volume masks), the chemical one's (`Chemical`:
//! lightning) and those of the map's actors that play a container by their
//! key `Always` (the distant weather: `Rain_Distance` …), with where those
//! actors stand. Reads what the `effects` step wrote (which effect files
//! are baked) and the ELink database itself.
//!
//! The database is read here from its bytes (`botw_formats::xlink` stays
//! as the original format parser has it and reads only call tables and parameters):
//! the containers (`ResContainerParam`), conditions (`ResSwitchCondition`,
//! `ResRandomCondition`), property and always triggers and the curves'
//! properties. Layout: the decomp's `lib/xlink2` (`xlink2Resource.h`,
//! `ResourceParamCreator`), big-endian on Wii U, offsets as the Wii U data
//! has them (docs/research/elink.md).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use asset_format::effects::{self as fx, EffectIndex};
use asset_format::elink::{
    Asset, AssetParams, Call, CallKind, Compare, Condition, Container, ContainerKind, Curve,
    ElinkDb, Param, PlacedUser, PropertyRef, PropertyTrigger, Spread, User, Value,
};
use asset_format::objects::{PlacedActors, all_cells};
use asset_format::paths;
use botw_formats::actor::{ActorPacks, model_list};
use botw_formats::content::ContentRoots;
use botw_formats::ptcl::Ptcl;
use botw_formats::ptcl::files::{self, Bootup};
use botw_formats::xlink::crc32;

use crate::objects::{parallel, read_cell};

/// Effect files the `effects` step bakes for the ELink users that are not
/// an actor's always-played sets: the distant weather and the map's
/// effect-only actors whose `Always` is a container (census of all cells,
/// v208: their users' own files; `Camera` and `Chemical` play the resident
/// file's sets).
pub const EFFECT_FILES: &[&str] = &[
    "Rain_Distance",
    "Snow_Distance_Gerudo",
    "Snow_Distance_Lanayru",
    "Thundercloud_Distance",
    "SandStorm_Distance",
    "SandStorm_Distance_A8",
    "SandStorm_Distance_Battle",
    "SandStorm_Distance_Ibutsu",
    "Darkness_Distance",
    "Aurora",
    "BlackSmoke",
    "DenseFogEftLocater",
    "EfLocater_GrudgeSolid_L",
    "EfLocater_GrudgeSolid_LL",
    "EfLocater_GrudgeSolid_LLL",
    "EfLocater_GrudgeSolid_M",
    "FldObj_MountainSheikerWall_A_02",
    "FldObj_RockBeachWall_A_03",
    "FlyLocater",
    "Grudge_HyruleCastle",
    "HazeEffectLocater_r50",
    "MayoiFogEftLocater",
    "SpiritForest_Effect",
    "TwnObj_AncientCivilLabo_B_01",
    "ValleyEffectLocater",
];

/// Users played without an actor: the camera's and the chemical one's.
const SYSTEM_USERS: [&str; 2] = ["Camera", "Chemical"];

pub fn bake(roots: &ContentRoots, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let index: EffectIndex = asset_format::read_ron(&out.join(paths::EFFECT_INDEX))
        .map_err(|e| format!("elink needs the effects step first: {e}"))?;
    let bootup = Bootup::read(roots)
        .map_err(|e| e.to_string())?
        .ok_or("Pack/Bootup.pack not found")?;
    let bytes = bootup
        .entry(files::ELINK_DB)
        .map_err(|e| e.to_string())?
        .ok_or("ELink database not found")?;
    let db = Db::parse(&bytes)?;

    // The map's actors and their users.
    let title_bg = roots
        .find("Pack/TitleBG.pack")
        .map(|p| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())))
        .transpose()?
        .map(Arc::new);
    let packs = ActorPacks::new(roots.clone(), title_bg.clone());
    let cells: Vec<String> = all_cells().collect();
    let units = parallel(&cells, |cell| read_cell(roots, title_bg.as_deref(), cell));
    let mut map_actors = Vec::new();
    for (cell, actors) in cells.iter().zip(units) {
        map_actors.extend(actors.map_err(|e| format!("cell {cell}: {e}"))?);
    }
    let names: Vec<String> = map_actors
        .iter()
        .map(|a| a.name.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let looked_up = parallel(&names, |name| actor_user(&packs, name));
    let mut actor_users = BTreeMap::new();
    for (name, found) in names.iter().zip(looked_up) {
        let (user, has_models) = found.map_err(|e| format!("actor {name}: {e}"))?;
        if db.plays_container_always(&user)? {
            actor_users.insert(name.clone(), (user, has_models));
        }
    }
    let mut in_reach = BTreeSet::new();
    if let Ok(objects) =
        asset_format::read_ron::<asset_format::objects::ObjectIndex>(&out.join(paths::OBJECT_INDEX))
    {
        for cell in &objects.cells {
            let placed: PlacedActors = asset_format::read_ron(&out.join(paths::object_cell(cell)))
                .map_err(|e| e.to_string())?;
            in_reach.extend(placed.names);
        }
    }

    let mut out_db = ElinkDb::default();
    for actor in &map_actors {
        if let Some((user, false)) = actor_users.get(&actor.name) {
            out_db.placed.push(PlacedUser {
                actor: actor.name.clone(),
                user: user.clone(),
                hash_id: actor.hash_id,
                translate: actor.translate,
                rotate: actor.rotate,
                scale: actor.scale,
            });
        }
    }
    for (name, (user, has_models)) in &actor_users {
        if *has_models && in_reach.contains(name) {
            out_db.actor_users.insert(name.clone(), user.clone());
        }
    }
    let mut wanted: BTreeSet<String> = SYSTEM_USERS.iter().map(|s| s.to_string()).collect();
    wanted.extend(out_db.placed.iter().map(|p| p.user.clone()));
    wanted.extend(out_db.actor_users.values().cloned());

    // Which baked effect file holds each set: the user's own, else the
    // resident file, else any other baked one.
    let mut sets = SetFinder::new(roots, &bootup, &index);
    let mut missing = BTreeSet::new();
    let mut notes = BTreeSet::new();
    for name in &wanted {
        let Some(mut user) = db.user(name)? else {
            eprintln!("warning: ELink user {name} not found");
            continue;
        };
        for call in &mut user.calls {
            let CallKind::Asset(asset) = &mut call.kind else {
                continue;
            };
            asset.file = sets.find(name, &asset.set)?;
            if asset.file.is_none() && !asset.set.starts_with('@') {
                missing.insert(format!("{name}: {}", asset.set));
            }
            notes.extend(asset_notes(asset));
        }
        out_db.users.insert(name.clone(), user);
    }
    for note in &notes {
        eprintln!("note: {note}");
    }
    if !missing.is_empty() {
        eprintln!(
            "warning: {} ELink sets are in no baked effect file (they play nothing): {}",
            missing.len(),
            missing.iter().cloned().collect::<Vec<_>>().join(", ")
        );
    }
    crate::sky::write_ron::<_, ElinkDb>(&out.join(paths::ELINK), &out_db)?;
    println!(
        "elink: {} users ({} calls), {} placed locators, {} actors with models ({:.1} s)",
        out_db.users.len(),
        out_db.users.values().map(|u| u.calls.len()).sum::<usize>(),
        out_db.placed.len(),
        out_db.actor_users.len(),
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// What the renderer does not play of an asset, to report.
fn asset_notes(asset: &Asset) -> Vec<String> {
    let mut notes = Vec::new();
    let p = &asset.params;
    let all = [&p.scale, &p.emission_rate, &p.emission_scale, &p.life_scale]
        .into_iter()
        .chain(&p.position)
        .chain(&p.rotation)
        .chain(&p.color);
    for param in all {
        if let Param::Curve(c) = param
            && c.kind != 0
        {
            notes.push(format!("curve type {} on {}", c.kind, asset.set));
        }
    }
    if p.delay.is_some() {
        notes.push(format!("Delay on {}", asset.set));
    }
    notes
}

/// The ELink user an actor names in its `ActorLink` (its own name without
/// one) and whether it has models.
fn actor_user(packs: &ActorPacks, name: &str) -> Result<(String, bool), String> {
    let pack = packs.open(name).map_err(|e| e.to_string())?;
    let user = pack
        .as_ref()
        .and_then(|p| p.find("Actor/ActorLink/"))
        .map(|link| files::elink_user(&link))
        .transpose()
        .map_err(|e| e.to_string())?
        .flatten()
        .unwrap_or_else(|| name.to_owned());
    let has_models = match pack.as_ref().and_then(|p| p.find("Actor/ModelList/")) {
        Some(list) => !model_list(&list).map_err(|e| e.to_string())?.is_empty(),
        None => false,
    };
    Ok((user, has_models))
}

/// Finds the baked effect file of a set.
struct SetFinder<'a> {
    roots: &'a ContentRoots,
    bootup: &'a Bootup,
    files: Vec<String>,
    /// Set names per file, read on demand.
    sets: HashMap<String, BTreeSet<String>>,
}

impl<'a> SetFinder<'a> {
    fn new(roots: &'a ContentRoots, bootup: &'a Bootup, index: &EffectIndex) -> Self {
        Self {
            roots,
            bootup,
            files: index.files.clone(),
            sets: HashMap::new(),
        }
    }

    fn sets_of(&mut self, file: &str) -> Result<&BTreeSet<String>, String> {
        if !self.sets.contains_key(file) {
            let bytes = if file == fx::RESIDENT {
                self.bootup
                    .entry(files::RESIDENT_FILE)
                    .map_err(|e| e.to_string())?
            } else {
                files::read_effect_file(self.roots, file).map_err(|e| e.to_string())?
            };
            let names = match bytes {
                Some(bytes) => Ptcl::parse(&bytes)
                    .map_err(|e| format!("effect file {file}: {e}"))?
                    .emitter_sets
                    .into_iter()
                    .map(|s| s.name)
                    .collect(),
                None => BTreeSet::new(),
            };
            self.sets.insert(file.to_owned(), names);
        }
        Ok(&self.sets[file])
    }

    fn find(&mut self, user: &str, set: &str) -> Result<Option<String>, String> {
        let mut order: Vec<String> = Vec::new();
        if self.files.iter().any(|f| f == user) {
            order.push(user.to_owned());
        }
        order.push(fx::RESIDENT.to_owned());
        order.extend(
            self.files
                .iter()
                .filter(|f| *f != user && *f != fx::RESIDENT)
                .cloned(),
        );
        for file in order {
            if self.sets_of(&file)?.contains(set) {
                return Ok(Some(file));
            }
        }
        Ok(None)
    }
}

/// The raw database.
struct Db<'a> {
    r: Reader<'a>,
    users: HashMap<u32, usize>,
    num_user_params: usize,
    /// Asset parameter definitions: name, type, default (raw bits).
    asset_params: Vec<(String, u32, u32)>,
    asset_table: usize,
    direct_values: usize,
    random_table: usize,
    curve_table: usize,
    curve_points: usize,
    condition_table: usize,
    name_table: usize,
}

/// A parameter reference (`ResParam`): type and index.
#[derive(Clone, Copy)]
struct RawParam {
    kind: u8,
    index: usize,
}

impl<'a> Db<'a> {
    fn parse(data: &'a [u8]) -> Result<Self, String> {
        let r = Reader(data);
        if r.slice(0, 4)? != b"XLNK" {
            return Err("ELink database: not an XLNK resource".into());
        }
        // `ResourceHeader` words after the magic.
        let h = |i: usize| r.u32(4 + 4 * i).map(|v| v as usize);
        let local_names_at = h(6)?;
        let (num_local_names, num_local_enums, num_direct) = (h(7)?, h(8)?, h(9)?);
        let (num_random, num_curves) = (h(10)?, h(11)?);
        let (num_user, condition_table, name_table) = (h(14)?, h(15)?, h(16)?);
        let mut users = HashMap::new();
        for i in 0..num_user {
            users.insert(
                r.u32(0x48 + 4 * i)?,
                r.u32(0x48 + 4 * (num_user + i))? as usize,
            );
        }
        // `ResParamDefineTableHeader`, then 12-byte `ResParamDefine`s
        // (user, asset, trigger parameters) and their strings.
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
                Ok((
                    r.cstr(strings + r.u32(at)? as usize)?,
                    r.u32(at + 4)?,
                    r.u32(at + 8)?,
                ))
            })
            .collect::<Result<_, String>>()?;
        let direct_values = local_names_at + 4 * (num_local_names + num_local_enums);
        let random_table = direct_values + 4 * num_direct;
        let curve_table = random_table + 8 * num_random;
        Ok(Self {
            r,
            users,
            num_user_params,
            asset_params,
            asset_table: defines + define_size,
            direct_values,
            random_table,
            curve_table,
            curve_points: curve_table + 0x14 * num_curves,
            condition_table,
            name_table,
        })
    }

    fn name(&self, offset: u32) -> Result<String, String> {
        self.r.cstr(self.name_table + offset as usize)
    }

    /// Where user `name`'s tables are: header, call table, container
    /// table.
    fn layout(&self, name: &str) -> Result<Option<UserLayout>, String> {
        let Some(&at) = self.users.get(&crc32(name.as_bytes())) else {
            return Ok(None);
        };
        let r = &self.r;
        let num_local = r.u32(at + 4)? as usize;
        let num_calls = r.u32(at + 8)? as usize;
        let sorted_ids = at + 0x30 + 4 * num_local + 4 * self.num_user_params;
        let calls = sorted_ids + 2 * (num_calls + (num_calls & 1));
        Ok(Some(UserLayout {
            at,
            num_local,
            num_calls,
            calls,
            containers: calls + 0x20 * num_calls,
        }))
    }

    /// Whether `user`'s top-level key `Always` is a container.
    fn plays_container_always(&self, user: &str) -> Result<bool, String> {
        let Some(l) = self.layout(user)? else {
            return Ok(false);
        };
        for i in 0..l.num_calls {
            let at = l.calls + 0x20 * i;
            let parent = self.r.u32(at + 12)? as i32;
            if parent < 0 && self.r.u16(at + 6)? & 1 != 0 && self.name(self.r.u32(at)?)? == "Always"
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn user(&self, name: &str) -> Result<Option<User>, String> {
        let Some(l) = self.layout(name)? else {
            return Ok(None);
        };
        let r = &self.r;
        let local_properties = (0..l.num_local)
            .map(|i| self.name(r.u32(l.at + 0x30 + 4 * i)?))
            .collect::<Result<_, _>>()?;
        let calls = (0..l.num_calls)
            .map(|i| self.call(&l, l.calls + 0x20 * i))
            .collect::<Result<Vec<_>, _>>()?;

        // `ResUserHeader` counts, then the trigger tables at
        // `triggerTablePos`: action slots, actions, action triggers,
        // properties, property triggers, always triggers.
        let count = |i: usize| r.u32(l.at + 4 * i).map(|v| v as usize);
        let (slots, actions, action_triggers) = (count(5)?, count(6)?, count(7)?);
        let (properties, property_triggers, always) = (count(8)?, count(9)?, count(10)?);
        let tables = l.at + count(11)?;
        let properties_at = tables + 8 * slots + 12 * actions + 0x18 * action_triggers;
        let triggers_at = properties_at + 0x10 * properties;
        let always_at = triggers_at + 0x14 * property_triggers;
        let call_index = |pos: u32| -> Result<usize, String> {
            let i = pos as usize / 0x20;
            if pos as usize % 0x20 != 0 || i >= l.num_calls {
                return Err(format!("ELink user {name}: bad call position {pos:#x}"));
            }
            Ok(i)
        };
        let mut out_triggers = Vec::new();
        for p in 0..properties {
            let at = properties_at + 0x10 * p;
            let property = PropertyRef {
                name: self.name(r.u32(at)?)?,
                global: r.u32(at + 4)? != 0,
            };
            let (start, end) = (r.u32(at + 8)? as i32, r.u32(at + 12)? as i32);
            for t in start.max(0)..=end {
                let t = t as usize;
                if t >= property_triggers {
                    return Err(format!("ELink user {name}: bad trigger {t}"));
                }
                let at = triggers_at + 0x14 * t;
                out_triggers.push(PropertyTrigger {
                    property: property.clone(),
                    call: call_index(r.u32(at + 4)?)?,
                    condition: self.condition(r.u32(at + 8)?)?,
                });
            }
        }
        let always_triggers = (0..always)
            .map(|i| call_index(r.u32(always_at + 0x10 * i + 4)?))
            .collect::<Result<_, _>>()?;
        Ok(Some(User {
            local_properties,
            calls,
            property_triggers: out_triggers,
            always_triggers,
        }))
    }

    /// `ResAssetCallTable`.
    fn call(&self, l: &UserLayout, at: usize) -> Result<Call, String> {
        let r = &self.r;
        let key = self.name(r.u32(at)?)?;
        let is_container = r.u16(at + 6)? & 1 != 0;
        let duration = r.u32(at + 8)? as i32;
        let parent = r.u32(at + 12)? as i32;
        let param = r.u32(at + 0x18)?;
        let kind = if !is_container {
            CallKind::Asset(Box::new(self.asset(self.asset_table + param as usize)?))
        } else if param == u32::MAX {
            CallKind::Empty
        } else {
            // `ResContainerParam` (+ `ResSwitchContainerParam`).
            let c = l.containers + param as usize;
            let kind = match r.u32(c)? {
                0 => ContainerKind::Switch,
                1 => ContainerKind::Random,
                2 => ContainerKind::Random2,
                3 => ContainerKind::Blend,
                4 => ContainerKind::Sequence,
                other => return Err(format!("ELink {key}: container type {other}")),
            };
            let (start, end) = (r.u32(c + 4)? as i32, r.u32(c + 8)? as i32);
            let children = if start < 0 || end < start {
                Vec::new()
            } else {
                (start as usize..=end as usize).collect()
            };
            if children.iter().any(|&i| i >= l.num_calls) {
                return Err(format!("ELink {key}: children past the table"));
            }
            let watch = if kind == ContainerKind::Switch {
                Some(PropertyRef {
                    name: self.name(r.u32(c + 12)?)?,
                    global: r.u8(c + 0x16)? != 0,
                })
            } else {
                None
            };
            CallKind::Container(Container {
                kind,
                children,
                watch,
            })
        };
        Ok(Call {
            key,
            parent: (parent >= 0).then_some(parent as usize),
            condition: self.condition(r.u32(at + 0x1c)?)?,
            duration,
            kind,
        })
    }

    /// `ResSwitchCondition` or `ResRandomCondition` at `pos` in the
    /// condition table (−1: none).
    fn condition(&self, pos: u32) -> Result<Option<Condition>, String> {
        if pos == u32::MAX {
            return Ok(None);
        }
        let r = &self.r;
        let at = self.condition_table + pos as usize;
        Ok(Some(match r.u32(at)? {
            0 => {
                let compare = Compare::from_raw(r.u32(at + 8)?)
                    .ok_or_else(|| format!("ELink condition {pos:#x}: compare type"))?;
                let raw = r.u32(at + 12)?;
                let value = match r.u32(at + 4)? {
                    0 => Value::Enum(self.name(raw)?),
                    1 => Value::Int(raw as i32),
                    2 => Value::Float(f32::from_bits(raw)),
                    other => return Err(format!("ELink condition: property type {other}")),
                };
                Condition::Switch { compare, value }
            }
            1 | 2 => Condition::Random {
                weight: r.f32(at + 4)?,
            },
            other => return Err(format!("ELink condition: container type {other}")),
        }))
    }

    /// The parameter references of an asset (`ResAssetParam`): a 64-bit
    /// mask of the parameters it sets, one reference per set bit.
    fn raw_params(&self, at: usize) -> Result<HashMap<&str, RawParam>, String> {
        let r = &self.r;
        let mask = (r.u32(at)? as u64) << 32 | r.u32(at + 4)? as u64;
        let mut out = HashMap::new();
        let mut k = 0;
        for (i, (name, _, _)) in self.asset_params.iter().enumerate().take(64) {
            if mask >> i & 1 == 0 {
                continue;
            }
            let raw = r.u32(at + 8 + 4 * k)?;
            k += 1;
            out.insert(
                name.as_str(),
                RawParam {
                    kind: (raw >> 24) as u8,
                    index: (raw & 0xFF_FFFF) as usize,
                },
            );
        }
        Ok(out)
    }

    fn asset(&self, at: usize) -> Result<Asset, String> {
        let raw = self.raw_params(at)?;
        let set = match raw.get("RuntimeAssetName") {
            Some(p) if p.kind == 1 => self.r.cstr(self.name_table + p.index)?,
            _ => String::new(),
        };
        let param = |name: &str| self.param(name, raw.get(name).copied());
        let set_param = |name: &str| -> Result<Option<Param>, String> {
            raw.get(name).map(|_| param(name)).transpose()
        };
        let int = |name: &str| -> Result<i32, String> {
            Ok(match param(name)? {
                Param::Const(v) => v as i32,
                _ => 0,
            })
        };
        Ok(Asset {
            set,
            file: None,
            matrix: int("Matrix")?,
            rotate_source: int("RotateSource")?,
            params: AssetParams {
                scale: param("Scale")?,
                position: [
                    param("PositionX")?,
                    param("PositionY")?,
                    param("PositionZ")?,
                ],
                rotation: [
                    param("RotationX")?,
                    param("RotationY")?,
                    param("RotationZ")?,
                ],
                color: [
                    param("Red")?,
                    param("Green")?,
                    param("Blue")?,
                    param("Alpha")?,
                ],
                emission_rate: param("EmissionRate")?,
                emission_scale: param("EmissionScale")?,
                emission_interval: param("EmissionInterval")?,
                life_scale: param("LifeScale")?,
                directional_velocity: param("DirectionalVel")?,
                duration: set_param("Duration")?,
                delay: set_param("Delay")?,
            },
        })
    }

    /// A numeric parameter: its reference, or the table's default.
    fn param(&self, name: &str, raw: Option<RawParam>) -> Result<Param, String> {
        let r = &self.r;
        let Some((_, kind, default)) = self.asset_params.iter().find(|(n, _, _)| n == name) else {
            return Err(format!("ELink: no asset parameter {name}"));
        };
        let number = |bits: u32| {
            if *kind == 1 {
                f32::from_bits(bits)
            } else {
                bits as i32 as f32
            }
        };
        let Some(raw) = raw else {
            return Ok(Param::Const(number(*default)));
        };
        Ok(match raw.kind {
            0 => Param::Const(number(r.u32(self.direct_values + 4 * raw.index)?)),
            2 => Param::Curve(self.curve(raw.index)?),
            kind @ (3 | 6..=17) => {
                const POWERS: [f32; 4] = [2.0, 3.0, 4.0, 1.5];
                let spread = match kind {
                    3 => Spread::Uniform,
                    6..=9 => Spread::Centered(POWERS[kind as usize - 6]),
                    10..=13 => Spread::WeightMin(POWERS[kind as usize - 10]),
                    _ => Spread::WeightMax(POWERS[kind as usize - 14]),
                };
                let at = self.random_table + 8 * raw.index;
                Param::Random {
                    min: r.f32(at)?,
                    max: r.f32(at + 4)?,
                    spread,
                }
            }
            other => {
                return Err(format!("ELink: parameter {name} of reference type {other}"));
            }
        })
    }

    /// `ResCurveCallTable` `index`: `u16` first point, point count, curve
    /// type, whether the property is global; `u32` property name; `s32`
    /// property index; `s16` local property. Points are `(x, y)` floats.
    fn curve(&self, index: usize) -> Result<Curve, String> {
        let r = &self.r;
        let at = self.curve_table + 0x14 * index;
        let (first, count) = (r.u16(at)? as usize, r.u16(at + 2)? as usize);
        let points = (first..first + count)
            .map(|p| {
                let p = self.curve_points + 8 * p;
                Ok([r.f32(p)?, r.f32(p + 4)?])
            })
            .collect::<Result<_, String>>()?;
        Ok(Curve {
            property: PropertyRef {
                name: self.name(r.u32(at + 8)?)?,
                global: r.u16(at + 6)? != 0,
            },
            kind: r.u16(at + 4)?,
            points,
        })
    }
}

struct UserLayout {
    at: usize,
    num_local: usize,
    num_calls: usize,
    /// The asset call table.
    calls: usize,
    /// The container parameters (`containerTablePos`), after the calls.
    containers: usize,
}

#[derive(Clone, Copy)]
struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn slice(&self, at: usize, len: usize) -> Result<&[u8], String> {
        at.checked_add(len)
            .and_then(|end| self.0.get(at..end))
            .ok_or_else(|| format!("ELink database: read past the end at {at:#x}"))
    }

    fn u8(&self, at: usize) -> Result<u8, String> {
        Ok(self.slice(at, 1)?[0])
    }

    fn u16(&self, at: usize) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.slice(at, 2)?.try_into().unwrap()))
    }

    fn u32(&self, at: usize) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.slice(at, 4)?.try_into().unwrap()))
    }

    fn f32(&self, at: usize) -> Result<f32, String> {
        self.u32(at).map(f32::from_bits)
    }

    fn cstr(&self, at: usize) -> Result<String, String> {
        let rest = self
            .0
            .get(at..)
            .ok_or("ELink database: string past the end")?;
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
    }
}
