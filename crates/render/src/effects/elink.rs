//! The game's effect links (ELink, xlink2) evaluated as the game does
//! (docs/research/elink.md, docs/research/weather.md §2–§4): the global
//! properties ([`GlobalProperties`], filled from the renderer's weather,
//! time, wind and climate each frame) drive the property triggers of the
//! user `Camera`, whose trees (switch, blend, random, sequence) pick the
//! rain, snow, sky flashes, strong wind, sandstorm and volume masks; its
//! key `FieldEnvEffect` plays the field haze; the distant weather actors
//! (`Rain_Distance` …) play their key `Always` with their local property
//! `カメラとの距離`. Each chosen asset is an [`EffectSet`] entity whose
//! transform is the asset's matrix (`Matrix` mode relative to its user's
//! source, every frame) and whose live parameters follow their curves.
//! A trigger whose condition fails, or a switch that picks another child,
//! fades what it started (`EffectSet::fading`).
//!
//! One-off plays: [`PlayElinkKey`] (a user's key once at a place); the
//! lightning's requests become the `Chemical` user's keys (§5.1).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use asset_format::elink::{
    Asset, CallKind, Compare, Condition, ContainerKind, ElinkDb, Param, PlacedUser, PropertyRef,
    User, Value,
};
use asset_format::env::WEATHERS;
use asset_format::paths;
use bevy::math::Affine3A;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use super::{EffectLibrary, EffectOwner, EffectSet};
use crate::camera::MainView;
use crate::grass::buffer::SeadRandom;
use crate::lightning::LightningRequest;
use crate::objects::PlacedObject;

/// Debug switches for camera effects with incomplete activation or rendering.
#[derive(Resource, Clone, Copy, Default)]
pub struct CameraEffectSettings {
    /// SI-WTH-15: opt-in until the FieldEnvEffect caller is recovered.
    pub field_haze: bool,
    /// SI-WTH-21: camera volume particles use incomplete custom shaders.
    pub volume_dust: bool,
    pub volume_fog: bool,
    pub volume_add: bool,
}

pub struct ElinkPlugin {
    /// The `assets/` folder.
    pub assets: PathBuf,
}

impl Plugin for ElinkPlugin {
    fn build(&self, app: &mut App) {
        let path = self.assets.join(paths::ELINK);
        let task = path.exists().then(|| {
            AsyncComputeTaskPool::get().spawn(async move {
                asset_format::read_ron::<ElinkDb>(&path)
                    .inspect_err(|e| warn!("elink: {e}"))
                    .ok()
            })
        });
        if task.is_none() {
            warn!("no effect links; run `cargo bake --only effects,elink`");
        }
        app.insert_resource(Elink { task, ..default() })
            .init_resource::<GlobalProperties>()
            .init_resource::<CameraEffectSettings>()
            .add_message::<PlayElinkKey>()
            .add_systems(
                PostUpdate,
                (
                    finish_reading,
                    update_global_properties,
                    play_lightning,
                    evaluate,
                )
                    .chain()
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// Plays `key` of ELink user `user` once, its source at `at` (the key's
/// assets are placed by their `Matrix` relative to it).
#[derive(Message, Clone, Debug)]
pub struct PlayElinkKey {
    pub user: String,
    pub key: String,
    pub at: Transform,
}

// ---------------------------------------------------------------------
// Properties

/// A property's value: an enum entry's index, an integer or a float
/// (`xlink2::PropertyType`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropertyValue {
    Enum(i32),
    Int(i32),
    Float(f32),
}

impl PropertyValue {
    pub fn as_f32(self) -> f32 {
        match self {
            PropertyValue::Enum(v) | PropertyValue::Int(v) => v as f32,
            PropertyValue::Float(v) => v,
        }
    }
}

/// The global properties' enum entries the renderer sets, by name, as
/// `EFFECT_CreateELinkGlobalPropertyDefs` (`0x03954fb0`, Wii U v208)
/// defines them.
fn enum_entries(property: &str) -> Option<&'static [&'static str]> {
    Some(match property {
        // `0x036722bc(i)`: the weathers' names.
        "天候" => &WEATHERS,
        // Climates in `ClimateDefines` order (`0x10341fa0` …).
        "地方" => &REGIONS,
        "シーンタイプ" => &SCENE_TYPES,
        "カメラ位置が屋内" => &["False", "True"],
        _ => return None,
    })
}

/// `地方`'s entries: the climates, in `eco::CLIMATES` order.
const REGIONS: [&str; 20] = [
    "ハイラル平原",
    "北ハイラル平原",
    "ヘブラ氷雪",
    "タバンタ乾燥",
    "ラネール山氷雪",
    "ゲルド砂漠 Lv1",
    "ゲルド高原乾燥",
    "オルディン気候 Lv0",
    "タムール平原",
    "ゾーラ温帯",
    "ハテール平原",
    "フィローネ亜熱帯",
    "南ハテール温湿",
    "オルディン気候 Lv1",
    "オルディン気候 Lv2",
    "オルディン気候 Lv3",
    "まよいの森",
    "ゲルド高地氷雪",
    "コログの森",
    "ゲルド砂漠 Lv2",
];

/// `シーンタイプ`'s entries (`0x10341f30`, 11 slots, 6 named).
const SCENE_TYPES: [&str; 6] = [
    "なし",
    "オープンワールド",
    "Cダンジョン",
    "GameTset",
    "四大遺物",
    "ビューワー",
];
const OPEN_WORLD: i32 = 1;

/// The xlink2 system's global properties (`EFFECT_UpdateXLinkGlobalProperties`
/// `0x0383b398` writes them each frame), by name.
#[derive(Resource, Clone, Debug, Default)]
pub struct GlobalProperties {
    values: HashMap<&'static str, PropertyValue>,
}

impl GlobalProperties {
    pub fn set(&mut self, name: &'static str, value: PropertyValue) {
        self.values.insert(name, value);
    }

    /// The value; a property nothing sets reads 0 of its kind.
    pub fn get(&self, name: &str) -> Option<PropertyValue> {
        self.values.get(name).copied()
    }
}

/// What the evaluator reads properties from: the globals and a user
/// instance's locals.
struct Properties<'a> {
    globals: &'a GlobalProperties,
    locals: &'a HashMap<String, PropertyValue>,
}

impl Properties<'_> {
    fn value(&self, property: &PropertyRef) -> Option<PropertyValue> {
        if property.global {
            self.globals.get(&property.name)
        } else {
            self.locals.get(&property.name).copied()
        }
    }

    fn f32(&self, property: &PropertyRef) -> f32 {
        self.value(property).map_or(0.0, PropertyValue::as_f32)
    }

    /// Whether `condition` holds for `property` (the switch's watched
    /// property or the trigger's): the property definition's type picks an
    /// integer or a float compare (Wii U `0x03b9a3b0`, `0x03ba4250`); an
    /// enum condition's entry is looked up by name, −1 when the property
    /// lacks it. A property nothing sets reads 0 (of the condition's
    /// kind).
    fn holds(&self, property: &PropertyRef, compare: Compare, value: &Value) -> bool {
        let current = self.value(property);
        match (current, value) {
            (Some(PropertyValue::Float(p)), Value::Float(v)) => compare.holds(p, *v),
            (Some(PropertyValue::Float(p)), Value::Int(v)) => compare.holds(p, *v as f32),
            (None, Value::Float(v)) => compare.holds(0.0, *v),
            (p, Value::Enum(name)) => {
                let p = match p {
                    Some(PropertyValue::Enum(i) | PropertyValue::Int(i)) => i,
                    Some(PropertyValue::Float(f)) => f as i32,
                    None => 0,
                };
                let entry = enum_entries(&property.name)
                    .and_then(|entries| entries.iter().position(|e| e == name))
                    .map_or(-1, |i| i as i32);
                compare.holds(p, entry)
            }
            (p, Value::Int(v)) => {
                let p = match p {
                    Some(PropertyValue::Enum(i) | PropertyValue::Int(i)) => i,
                    _ => 0,
                };
                compare.holds(p, *v)
            }
            (Some(PropertyValue::Enum(p) | PropertyValue::Int(p)), Value::Float(v)) => {
                compare.holds(p as f32, *v)
            }
        }
    }
}

/// The game's property values from the renderer's state, each frame
/// (`EFFECT_UpdateXLinkGlobalProperties` `0x0383b398`, weather.md §2.1).
#[allow(clippy::too_many_arguments)]
fn update_global_properties(
    mut globals: ResMut<GlobalProperties>,
    mut elink: ResMut<Elink>,
    real: Res<Time>,
    weather: Option<Res<crate::climate::Weather>>,
    climate: Option<Res<crate::climate::Climate>>,
    time: Option<Res<crate::daynight::TimeOfDay>>,
    precipitation: Option<Res<crate::precipitation::Precipitation>>,
    wind: Option<Res<crate::grass::GrassWind>>,
) {
    let g = &mut *globals;
    if let Some(weather) = &weather {
        g.set("天候", PropertyValue::Enum(weather.current as i32));
        // SI-WTH-12: 気温 is the climate's air temperature at the camera,
        // not TempMgr+0x2c (the effect temperature) itself.
        g.set(
            "気温",
            PropertyValue::Float(weather.temperature.unwrap_or(0.0)),
        );
    }
    if let Some(p) = &precipitation {
        g.set("濃度:雨", PropertyValue::Float(p.rain));
        g.set("濃度:雷", PropertyValue::Float(p.thunder));
    }
    if let Some(time) = &time {
        g.set("時刻", PropertyValue::Float(time.hours));
    }
    if let Some(climate) = &climate {
        g.set("地方", PropertyValue::Enum(climate.current as i32));
    }
    // `風の強さ`: the mean of the last four samples of the wind speed at
    // the camera (`0x0366d388`); the world's wind (`getWindSpeed`).
    let speed = wind.as_ref().map_or(0.0, |w| w.world().0);
    let frames = real.delta_secs() * 30.0;
    if frames > 0.0 {
        elink.wind.rotate_left(1);
        elink.wind[3] = speed;
    }
    g.set(
        "風の強さ",
        PropertyValue::Float(elink.wind.iter().sum::<f32>() * 0.25),
    );
    // SI-WTH-13: what the renderer does not model reads as the open
    // field: scene type open world, the camera outside, no sandstorm,
    // spores, sparks, blood moon or volume fog, ecosystem area 0, no
    // nearby material.
    g.set("シーンタイプ", PropertyValue::Enum(OPEN_WORLD));
    g.set("カメラ位置が屋内", PropertyValue::Enum(0));
    for name in [
        "濃度:砂嵐",
        "濃度:胞子",
        "濃度:火粉",
        "濃度:ＢＭ",
        "濃度:VFog",
        "室内率",
    ] {
        g.values.entry(name).or_insert(PropertyValue::Float(0.0));
    }
    g.values
        .entry("生態系エリア")
        .or_insert(PropertyValue::Int(0));
    g.values
        .entry("近傍マテリアル")
        .or_insert(PropertyValue::Enum(-1));
}

// ---------------------------------------------------------------------
// Placement

/// A user's source: its root matrix (rotation and translation) and its
/// scale (the user's `+0x28`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Source {
    pub rotation: Quat,
    pub translation: Vec3,
    pub scale: Vec3,
}

impl Source {
    pub fn at(transform: &Transform) -> Self {
        Self {
            rotation: transform.rotation,
            translation: transform.translation,
            scale: transform.scale,
        }
    }
}

/// The emitter set's matrix by the asset's `Matrix` mode
/// (`XLINK_AssetExecutorELinkCalcMtx` `0x03b824dc`, weather.md §3.3):
/// a base matrix from the source times the asset's SRT (`Scale` × the
/// user's scale, `Rotation` X then Y then Z, `Position`):
///
/// | mode | base | SRT scale | translation |
/// |---|---|---|---|
/// | 0 | source | user × S | P |
/// | 1 | source | user × S | P × user |
/// | 2 | source, axes normalised | S | P |
/// | 3 | source position, world axes | user × S | P × user |
/// | 4 | source position, world axes | S | P |
/// | 5 | source position, world axes | user × S | (R·P) × user |
/// | 6 | source position, world axes | S | R·P |
///
/// (R: the source's rotation; `0x03c6fe08` rotates without translating.)
/// The source's root is unscaled here, so normalising changes nothing and
/// `RotateSource` (rotation from the user's matrix source rather than the
/// bone) picks the same rotation.
pub fn set_matrix(
    mode: i32,
    source: &Source,
    scale: f32,
    position: Vec3,
    rotation: Vec3,
) -> Affine3A {
    let rotated = Affine3A::from_rotation_translation(source.rotation, source.translation);
    let placed = Affine3A::from_translation(source.translation);
    let user = source.scale;
    let (base, s, t) = match mode {
        0 => (rotated, user * scale, position),
        1 => (rotated, user * scale, position * user),
        2 => (rotated, Vec3::splat(scale), position),
        3 => (placed, user * scale, position * user),
        4 => (placed, Vec3::splat(scale), position),
        5 => (placed, user * scale, (source.rotation * position) * user),
        6 => (placed, Vec3::splat(scale), source.rotation * position),
        // Not a mode the resource uses.
        _ => (placed, Vec3::splat(scale), position),
    };
    let r = Quat::from_euler(EulerRot::ZYX, rotation.z, rotation.y, rotation.x);
    base * Affine3A::from_scale_rotation_translation(s, r, t)
}

// ---------------------------------------------------------------------
// The evaluator

/// The asset parameters an asset draws or follows, in this order.
const FIELDS: usize = 16;
const SCALE: usize = 0;
const POSITION: usize = 1;
const ROTATION: usize = 4;
const COLOR: usize = 7;
const EMISSION_RATE: usize = 11;
const EMISSION_SCALE: usize = 12;
const EMISSION_INTERVAL: usize = 13;
const LIFE_SCALE: usize = 14;
const DURATION: usize = 15;

fn fields(asset: &Asset) -> [Option<&Param>; FIELDS] {
    let p = &asset.params;
    [
        Some(&p.scale),
        Some(&p.position[0]),
        Some(&p.position[1]),
        Some(&p.position[2]),
        Some(&p.rotation[0]),
        Some(&p.rotation[1]),
        Some(&p.rotation[2]),
        Some(&p.color[0]),
        Some(&p.color[1]),
        Some(&p.color[2]),
        Some(&p.color[3]),
        Some(&p.emission_rate),
        Some(&p.emission_scale),
        Some(&p.emission_interval),
        Some(&p.life_scale),
        p.duration.as_ref(),
    ]
}

/// An asset playing.
#[derive(Debug)]
struct LiveAsset {
    call: usize,
    /// `None` for a set no baked file has (a blank): it plays nothing
    /// and never ends.
    entity: Option<Entity>,
    /// Random parameters, drawn when it started.
    drawn: [f32; FIELDS],
    /// Frames played, and how many it plays for (`Duration`).
    age: f32,
    duration: Option<f32>,
    /// Spawned this frame (not yet visible to queries).
    fresh: bool,
}

/// A started entry of the call table (`xlink2::ContainerBase`).
#[derive(Debug)]
enum Node {
    Asset(LiveAsset),
    /// `SwitchContainer`: the child of the first matching entry, kept
    /// while it matches; re-made when it ended and still matches.
    Switch {
        call: usize,
        chosen: Option<usize>,
        child: Option<Box<Node>>,
    },
    /// `BlendContainer`: every child; ends when all have.
    Blend {
        children: Vec<Node>,
    },
    /// `RandomContainer`: one child, drawn when it starts.
    Single {
        child: Box<Node>,
    },
    /// `SequenceContainer`: the children one after the other.
    Sequence {
        call: usize,
        index: usize,
        child: Option<Box<Node>>,
    },
}

/// What one frame of evaluation needs besides the tree.
struct Io<'a, 'w, 's> {
    user: &'a User,
    user_name: &'a str,
    properties: Properties<'a>,
    source: Source,
    owner: Option<Entity>,
    commands: &'a mut Commands<'w, 's>,
    /// Effect sets that exist.
    existing: &'a HashSet<Entity>,
    library: &'a EffectLibrary,
    random: &'a mut SeadRandom,
    frames: f32,
    /// Live parameters to write this frame.
    updates: &'a mut Vec<(Entity, Update)>,
    /// Sets to fade.
    fades: &'a mut Vec<Entity>,
}

/// An asset's placement and live parameters this frame.
#[derive(Clone, Copy, Debug)]
struct Update {
    matrix: Affine3A,
    color: Vec4,
    alpha: f32,
    emission_rate: f32,
    emission_scale: f32,
    emission_interval: f32,
    life_scale: f32,
}

impl Io<'_, '_, '_> {
    /// The switch `call`'s first matching child.
    fn switch_choice(&self, call: usize) -> Option<usize> {
        switch_choice(self.user, &self.properties, call)
    }

    fn value(&self, live: &LiveAsset, asset: &Asset, field: usize) -> f32 {
        match fields(asset)[field] {
            Some(Param::Const(v)) => *v,
            Some(Param::Random { .. }) => live.drawn[field],
            Some(Param::Curve(c)) => c.eval(self.properties.f32(&c.property)),
            None => 0.0,
        }
    }

    fn update(&self, live: &LiveAsset, asset: &Asset) -> Update {
        let v = |field| self.value(live, asset, field);
        let position = Vec3::new(v(POSITION), v(POSITION + 1), v(POSITION + 2));
        let rotation = Vec3::new(v(ROTATION), v(ROTATION + 1), v(ROTATION + 2));
        Update {
            matrix: set_matrix(asset.matrix, &self.source, v(SCALE), position, rotation),
            color: Vec4::new(v(COLOR), v(COLOR + 1), v(COLOR + 2), 1.0),
            alpha: v(COLOR + 3),
            emission_rate: v(EMISSION_RATE),
            emission_scale: v(EMISSION_SCALE),
            emission_interval: v(EMISSION_INTERVAL),
            life_scale: v(LIFE_SCALE),
        }
    }

    /// Starts entry `call` (`ContainerBase::createChildContainer_`);
    /// `None` when it plays nothing.
    fn start(&mut self, call: usize) -> Option<Node> {
        let user = self.user;
        match &user.calls[call].kind {
            CallKind::Asset(asset) => Some(Node::Asset(self.start_asset(call, asset))),
            CallKind::Empty => None,
            CallKind::Container(c) => match c.kind {
                ContainerKind::Switch => {
                    // Watched: it starts even with no child to pick.
                    let chosen = self.switch_choice(call);
                    let child = chosen.and_then(|i| self.start(i)).map(Box::new);
                    Some(Node::Switch {
                        call,
                        chosen,
                        child,
                    })
                }
                ContainerKind::Blend => {
                    let children: Vec<Node> =
                        c.children.iter().filter_map(|&i| self.start(i)).collect();
                    (!children.is_empty()).then_some(Node::Blend { children })
                }
                // SI-WTH-14: Random2 (no repeat of the last pick in the
                // game) draws like Random.
                ContainerKind::Random | ContainerKind::Random2 => {
                    let pick = pick_by_weight(user, &c.children, self.random.unit())?;
                    self.start(pick).map(|child| Node::Single {
                        child: Box::new(child),
                    })
                }
                ContainerKind::Sequence => {
                    let mut node = Node::Sequence {
                        call,
                        index: 0,
                        child: None,
                    };
                    for (index, &i) in c.children.iter().enumerate() {
                        if let Some(child) = self.start(i) {
                            node = Node::Sequence {
                                call,
                                index,
                                child: Some(Box::new(child)),
                            };
                            break;
                        }
                    }
                    matches!(node, Node::Sequence { child: Some(_), .. }).then_some(node)
                }
            },
        }
    }

    fn start_asset(&mut self, call: usize, asset: &Asset) -> LiveAsset {
        let mut drawn = [0.0; FIELDS];
        for (slot, param) in drawn.iter_mut().zip(fields(asset)) {
            if let Some(Param::Random { min, max, spread }) = param {
                *slot = spread.sample(*min, *max, self.random.unit());
            }
        }
        let mut live = LiveAsset {
            call,
            entity: None,
            drawn,
            age: 0.0,
            duration: None,
            fresh: true,
        };
        live.duration = asset
            .params
            .duration
            .as_ref()
            .map(|_| self.value(&live, asset, DURATION));
        let file = asset.file.as_deref().filter(|f| {
            !asset.set.is_empty()
                && self
                    .library
                    .index()
                    .is_some_and(|i| i.files.iter().any(|n| n == f))
        });
        if let Some(file) = file {
            let u = self.update(&live, asset);
            let set = EffectSet {
                color: u.color,
                alpha: u.alpha,
                emission_rate: u.emission_rate,
                emission_scale: u.emission_scale,
                emission_interval: u.emission_interval,
                life_scale: u.life_scale,
                ..EffectSet::new(file, &asset.set)
            };
            let mut entity = self.commands.spawn((
                Name::new(format!(
                    "effect {} (elink {})",
                    asset.set, self.user.calls[call].key
                )),
                set,
                Transform::from_matrix(Mat4::from(u.matrix)),
                Visibility::default(),
            ));
            if let Some(owner) = self.owner {
                entity.insert(EffectOwner(owner));
            }
            live.entity = Some(entity.id());
            debug!(
                "elink: {} plays {}/{} ({})",
                self.user_name, file, asset.set, self.user.calls[call].key
            );
        }
        live
    }

    /// One frame of a started entry; `false` once it has ended.
    fn calc(&mut self, node: &mut Node) -> bool {
        match node {
            Node::Asset(live) => self.calc_asset(live),
            Node::Switch {
                call,
                chosen,
                child,
            } => {
                let now = self.switch_choice(*call);
                if now != *chosen {
                    if let Some(old) = child.take() {
                        self.fade(*old);
                    }
                    *chosen = now;
                }
                if let Some(c) = child
                    && !self.calc(c)
                {
                    *child = None;
                }
                // A child that ended is made again while it still matches.
                if child.is_none()
                    && let Some(i) = *chosen
                {
                    *child = self.start(i).map(Box::new);
                }
                true
            }
            Node::Blend { children } => {
                children.retain_mut(|c| self.calc(c));
                !children.is_empty()
            }
            Node::Single { child } => self.calc(child),
            Node::Sequence { call, index, child } => {
                if let Some(c) = child
                    && self.calc(c)
                {
                    return true;
                }
                *child = None;
                let CallKind::Container(c) = &self.user.calls[*call].kind else {
                    return false;
                };
                let children = c.children.clone();
                while *index + 1 < children.len() {
                    *index += 1;
                    if let Some(next) = self.start(children[*index]) {
                        *child = Some(Box::new(next));
                        return true;
                    }
                }
                false
            }
        }
    }

    fn calc_asset(&mut self, live: &mut LiveAsset) -> bool {
        let CallKind::Asset(asset) = &self.user.calls[live.call].kind else {
            return false;
        };
        live.age += self.frames;
        if let Some(entity) = live.entity {
            if !live.fresh && !self.existing.contains(&entity) {
                return false;
            }
            live.fresh = false;
            let update = self.update(live, asset);
            self.updates.push((entity, update));
        }
        if live.duration.is_some_and(|d| live.age >= d) {
            if let Some(entity) = live.entity {
                self.fades.push(entity);
            }
            return false;
        }
        true
    }

    /// Fades what `node` started (`fadeBySystem`).
    fn fade(&mut self, node: Node) {
        match node {
            Node::Asset(live) => {
                if let Some(entity) = live.entity {
                    self.fades.push(entity);
                }
            }
            Node::Switch { child, .. } | Node::Sequence { child, .. } => {
                if let Some(child) = child {
                    self.fade(*child);
                }
            }
            Node::Blend { children } => {
                for child in children {
                    self.fade(child);
                }
            }
            Node::Single { child } => self.fade(*child),
        }
    }
}

/// The switch `call`'s first child whose condition holds under the
/// watched property; a child without a condition always does
/// (`SwitchContainer`, Wii U `0x03b9a3b0`).
fn switch_choice(user: &User, properties: &Properties, call: usize) -> Option<usize> {
    let CallKind::Container(c) = &user.calls[call].kind else {
        return None;
    };
    c.children
        .iter()
        .copied()
        .find(|&i| match (&user.calls[i].condition, c.watch.as_ref()) {
            (None, _) => true,
            (Some(Condition::Switch { compare, value }), Some(watch)) => {
                properties.holds(watch, *compare, value)
            }
            (Some(Condition::Switch { .. }), None) => false,
            (Some(Condition::Random { .. }), _) => true,
        })
}

/// The child a random container draws (`RandomContainer::start`): the
/// first whose running weight exceeds `r` × the total.
fn pick_by_weight(user: &User, children: &[usize], r: f32) -> Option<usize> {
    let weight = |i: usize| match user.calls[i].condition {
        Some(Condition::Random { weight }) => Some(weight),
        _ => None,
    };
    let total: f32 = children.iter().filter_map(|&i| weight(i)).sum();
    if total <= 0.0 {
        return None;
    }
    let target = r * total;
    let mut sum = 0.0;
    for &i in children {
        if let Some(w) = weight(i) {
            sum += w;
            if target < sum {
                return Some(i);
            }
        }
    }
    None
}

/// What starts a tree of a user instance.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RootKind {
    /// Property trigger `i`: plays while its condition holds
    /// (`PropertyTriggerCtrl`, Wii U `0x03ba4250`).
    Property(usize),
    /// An always trigger or a key played all the time: made again when
    /// it ends (`AlwaysTriggerCtrl`).
    Loop(usize),
    /// FieldEnvEffect is explicitly gated; it is not an always trigger.
    FieldHaze(usize),
    VolumeDust(usize),
    VolumeFog(usize),
    VolumeAdd(usize),
    /// A key played once.
    Once(usize),
}

#[derive(Debug)]
struct Root {
    kind: RootKind,
    node: Option<Node>,
    /// A `Once` root has started.
    started: bool,
}

/// A user playing: its source, local properties and trees.
#[derive(Debug)]
struct Instance {
    user: String,
    source: Source,
    locals: HashMap<String, PropertyValue>,
    owner: Option<Entity>,
    roots: Vec<Root>,
    /// Everything it plays fades, then it is dropped.
    stopped: bool,
}

impl Instance {
    fn new(user: &str, source: Source, roots: Vec<RootKind>) -> Self {
        Self {
            user: user.to_owned(),
            source,
            locals: HashMap::new(),
            owner: None,
            roots: roots
                .into_iter()
                .map(|kind| Root {
                    kind,
                    node: None,
                    started: false,
                })
                .collect(),
            stopped: false,
        }
    }

    /// The camera's property/always triggers, with incomplete volume
    /// overlays and the haze key gated by their debug overrides.
    fn camera(user: &User, source: Source) -> Self {
        let mut roots: Vec<RootKind> = (0..user.property_triggers.len())
            .map(RootKind::Property)
            .collect();
        // SI-WTH-21: these volume emitters currently pass through the generic
        // particle renderer, producing visible camera-relative clouds even
        // at zero VFog. Keep them opt-in until their programs are ported.
        roots.extend(
            user.always_triggers
                .iter()
                .map(|&i| match user.calls[i].key.as_str() {
                    "VolumeSwitch_Dust" => RootKind::VolumeDust(i),
                    "VolumeSwitch_Fog" => RootKind::VolumeFog(i),
                    "VolumeSwitch_Add" => RootKind::VolumeAdd(i),
                    _ => RootKind::Loop(i),
                }),
        );
        // SI-WTH-15: the game's caller is not traced. Do not assume this
        // camera-relative haze is always active in the open world.
        roots.extend(user.key("FieldEnvEffect").map(RootKind::FieldHaze));
        Self::new("Camera", source, roots)
    }

    /// Whether anything is still playing (a `Once` instance ends when its
    /// trees have).
    fn playing(&self) -> bool {
        !self.stopped
            && self
                .roots
                .iter()
                .any(|r| r.node.is_some() || (matches!(r.kind, RootKind::Once(_)) && !r.started))
    }

    /// One frame: triggers, trees, live parameters.
    #[allow(clippy::too_many_arguments)]
    fn step(
        &mut self,
        db: &ElinkDb,
        globals: &GlobalProperties,
        camera_effects: &CameraEffectSettings,
        commands: &mut Commands,
        existing: &HashSet<Entity>,
        library: &EffectLibrary,
        random: &mut SeadRandom,
        frames: f32,
        updates: &mut Vec<(Entity, Update)>,
        fades: &mut Vec<Entity>,
    ) {
        if self.stopped {
            self.fade_all(fades);
            return;
        }
        let Some(user) = db.users.get(&self.user) else {
            return;
        };
        let mut io = Io {
            user,
            user_name: &self.user,
            properties: Properties {
                globals,
                locals: &self.locals,
            },
            source: self.source,
            owner: self.owner,
            commands,
            existing,
            library,
            random,
            frames,
            updates,
            fades,
        };
        for root in &mut self.roots {
            let wanted = match root.kind {
                RootKind::Property(i) => {
                    let trigger = &user.property_triggers[i];
                    match &trigger.condition {
                        Some(Condition::Switch { compare, value }) => {
                            io.properties.holds(&trigger.property, *compare, value)
                        }
                        // No condition: the controller only fades.
                        _ => false,
                    }
                }
                RootKind::Loop(_) => true,
                RootKind::FieldHaze(_) => camera_effects.field_haze,
                RootKind::VolumeDust(_) => camera_effects.volume_dust,
                RootKind::VolumeFog(_) => camera_effects.volume_fog,
                RootKind::VolumeAdd(_) => camera_effects.volume_add,
                RootKind::Once(_) => !root.started,
            };
            let call = match root.kind {
                RootKind::Property(i) => user.property_triggers[i].call,
                RootKind::Loop(c)
                | RootKind::FieldHaze(c)
                | RootKind::VolumeDust(c)
                | RootKind::VolumeFog(c)
                | RootKind::VolumeAdd(c)
                | RootKind::Once(c) => c,
            };
            if !wanted && !matches!(root.kind, RootKind::Once(_)) {
                if let Some(node) = root.node.take() {
                    io.fade(node);
                }
                continue;
            }
            if let Some(node) = &mut root.node
                && !io.calc(node)
            {
                root.node = None;
            }
            if root.node.is_none() && wanted {
                root.node = io.start(call);
                root.started = true;
            }
        }
    }

    fn fade_all(&mut self, fades: &mut Vec<Entity>) {
        fn collect(node: Node, fades: &mut Vec<Entity>) {
            match node {
                Node::Asset(live) => fades.extend(live.entity),
                Node::Switch { child, .. } | Node::Sequence { child, .. } => {
                    if let Some(child) = child {
                        collect(*child, fades);
                    }
                }
                Node::Blend { children } => {
                    for child in children {
                        collect(child, fades);
                    }
                }
                Node::Single { child } => collect(*child, fades),
            }
        }
        for root in &mut self.roots {
            if let Some(node) = root.node.take() {
                collect(node, fades);
            }
        }
    }
}

/// How near the camera a placed locator plays (m, horizontal).
// SI-WTH-16: locators play within a fixed radius of the camera (their
// actors' `Landmark05km` life is not modelled).
const LOCATOR_RADIUS: f32 = 5000.0;
/// Re-check after the camera moved this far (m).
const LOCATOR_RECHECK: f32 = 100.0;

/// The evaluator's state.
#[derive(Resource)]
pub(crate) struct Elink {
    task: Option<Task<Option<ElinkDb>>>,
    db: Option<Arc<ElinkDb>>,
    camera: Option<Instance>,
    /// Placed locators playing, by index into `ElinkDb::placed`.
    placed: HashMap<usize, Instance>,
    checked_at: Option<Vec3>,
    /// Objects with models whose user plays a container by `Always`.
    objects: HashMap<Entity, Instance>,
    /// Keys played once, until their trees end.
    shots: Vec<Instance>,
    /// The lightning warning, held while it is requested.
    warning: Option<(Vec3, Instance)>,
    random: SeadRandom,
    /// The last four wind speeds (`風の強さ`).
    wind: [f32; 4],
}

impl Default for Elink {
    fn default() -> Self {
        Self {
            task: None,
            db: None,
            camera: None,
            placed: HashMap::new(),
            checked_at: None,
            objects: HashMap::new(),
            shots: Vec::new(),
            warning: None,
            random: SeadRandom::new(0x454c_4e4b),
            wind: [0.0; 4],
        }
    }
}

impl Elink {
    pub(crate) fn is_loading(&self) -> bool {
        self.task.is_some()
    }
}

fn finish_reading(mut elink: ResMut<Elink>) {
    if let Some(task) = &mut elink.task
        && let Some(db) = block_on(poll_once(task))
    {
        elink.task = None;
        if let Some(db) = db {
            info!(
                "elink: {} users, {} placed locators",
                db.users.len(),
                db.placed.len()
            );
            elink.db = Some(Arc::new(db));
        }
    }
}

/// The lightning's requests as the `Chemical` user's keys (weather.md
/// §5.1): the warning `Chemical_LightningSign_OT` at the strike point
/// while it is announced, the bolt `Chemical_Lightning`, the far bolt
/// `Far_Lightning`.
fn play_lightning(
    mut requests: MessageReader<LightningRequest>,
    mut elink: ResMut<Elink>,
    mut plays: MessageWriter<PlayElinkKey>,
) {
    let Some(db) = elink.db.clone() else {
        requests.clear();
        return;
    };
    let Some(chemical) = db.users.get("Chemical") else {
        requests.clear();
        return;
    };
    let mut warned = None;
    for request in requests.read() {
        match *request {
            // SI-WTH-17: the ChemicalMgr's handling of the requests is not
            // traced: the warning plays while it is requested, the bolts
            // once; the far warning and the rumble play nothing.
            LightningRequest::Warning { at, .. } => warned = Some(at),
            LightningRequest::Strike { at } => {
                plays.write(PlayElinkKey {
                    user: "Chemical".into(),
                    key: "Chemical_Lightning".into(),
                    at: Transform::from_translation(at),
                });
            }
            LightningRequest::FarStrike { at } => {
                plays.write(PlayElinkKey {
                    user: "Chemical".into(),
                    key: "Far_Lightning".into(),
                    at: Transform::from_translation(at),
                });
            }
            LightningRequest::FarWarning { .. } | LightningRequest::Rumble => {}
        }
    }
    match (warned, &elink.warning) {
        (Some(at), Some((held, _))) if *held == at => {}
        (Some(at), _) => {
            if let Some(call) = chemical.key("Chemical_LightningSign_OT") {
                let source = Source::at(&Transform::from_translation(at));
                let instance = Instance::new("Chemical", source, vec![RootKind::Loop(call)]);
                // The previous one is faded by `evaluate` when replaced.
                let old = elink.warning.replace((at, instance));
                if let Some((_, old)) = old {
                    elink.shots.push(into_fading(old));
                }
            }
        }
        (None, Some(_)) => {
            let old = elink.warning.take();
            if let Some((_, old)) = old {
                elink.shots.push(into_fading(old));
            }
        }
        (None, None) => {}
    }
}

/// An instance whose trees are to fade (`evaluate` fades and drops it).
fn into_fading(mut instance: Instance) -> Instance {
    instance.stopped = true;
    instance
}

#[allow(clippy::too_many_arguments)]
fn evaluate(
    mut commands: Commands,
    real: Res<Time>,
    globals: Res<GlobalProperties>,
    camera_effects: Res<CameraEffectSettings>,
    library: Res<EffectLibrary>,
    mut elink: ResMut<Elink>,
    mut plays: MessageReader<PlayElinkKey>,
    camera: Query<&Transform, With<MainView>>,
    objects: Query<(Entity, &PlacedObject, &Transform), Without<EffectSet>>,
    mut sets: Query<(Entity, &mut EffectSet, &mut Transform), Without<MainView>>,
) {
    let Some(db) = elink.db.clone() else {
        plays.clear();
        return;
    };
    let Ok(eye) = camera.single() else {
        return;
    };
    let frames = real.delta_secs() * 30.0;
    let existing: HashSet<Entity> = sets.iter().map(|(e, _, _)| e).collect();
    let mut updates = Vec::new();
    let mut fades = Vec::new();
    let state = &mut *elink;

    // The camera's user follows the camera.
    // SI-WTH-20: its source is the main camera's transform (the user's
    // creation was not traced); its locals (`受ける風の強さ`, `シーン状態`)
    // are never set and read 0.
    let camera_source = Source {
        scale: Vec3::ONE,
        ..Source::at(eye)
    };
    if state.camera.is_none()
        && let Some(user) = db.users.get("Camera")
    {
        state.camera = Some(Instance::camera(user, camera_source));
    }

    // Placed locators near the camera.
    let near = |p: &PlacedUser, radius: f32| {
        Vec2::new(p.translate[0], p.translate[2]).distance(eye.translation.xz()) < radius
    };
    if state
        .checked_at
        .is_none_or(|at| at.distance(eye.translation) >= LOCATOR_RECHECK)
    {
        state.checked_at = Some(eye.translation);
        state.placed.retain(|&i, instance| {
            let keep = near(&db.placed[i], LOCATOR_RADIUS + LOCATOR_RECHECK);
            if !keep {
                instance.fade_all(&mut fades);
            }
            keep
        });
        for (i, p) in db.placed.iter().enumerate() {
            if state.placed.contains_key(&i) || !near(p, LOCATOR_RADIUS) {
                continue;
            }
            let Some(always) = db.users.get(&p.user).and_then(|u| u.key("Always")) else {
                continue;
            };
            let [x, y, z] = p.rotate;
            let source = Source {
                rotation: Quat::from_euler(EulerRot::ZYX, z, y, x),
                translation: Vec3::from(p.translate),
                scale: Vec3::from(p.scale),
            };
            // SI-WTH-18: the locators play their key `Always` (the
            // EffectLocater class's play was not traced).
            state.placed.insert(
                i,
                Instance::new(&p.user, source, vec![RootKind::Loop(always)]),
            );
        }
    }

    // Objects with models that play a container by `Always`.
    let mut seen = HashSet::new();
    for (entity, object, transform) in &objects {
        let Some(user) = db.actor_users.get(&*object.0) else {
            continue;
        };
        seen.insert(entity);
        if let Some(instance) = state.objects.get_mut(&entity) {
            instance.source = Source::at(transform);
            continue;
        }
        let Some(always) = db.users.get(user).and_then(|u| u.key("Always")) else {
            continue;
        };
        let mut instance = Instance::new(user, Source::at(transform), vec![RootKind::Loop(always)]);
        instance.owner = Some(entity);
        state.objects.insert(entity, instance);
    }
    state.objects.retain(|entity, _| seen.contains(entity));

    // Keys played once.
    for play in plays.read() {
        let Some(call) = db.users.get(&play.user).and_then(|u| u.key(&play.key)) else {
            warn_once!("elink: no key {} in user {}", play.key, play.user);
            continue;
        };
        state.shots.push(Instance::new(
            &play.user,
            Source::at(&play.at),
            vec![RootKind::Once(call)],
        ));
    }

    let Elink {
        camera,
        placed,
        objects: object_instances,
        shots,
        warning,
        random,
        ..
    } = state;
    if let Some(camera) = camera {
        camera.source = camera_source;
    }
    let instances = camera
        .iter_mut()
        .chain(placed.values_mut())
        .chain(object_instances.values_mut())
        .chain(shots.iter_mut())
        .chain(warning.iter_mut().map(|(_, i)| i));
    for instance in instances {
        // `カメラとの距離`: the locator's distance from the camera.
        // SI-WTH-19: set as the straight-line distance from the camera.
        let distance = instance.source.translation.distance(eye.translation);
        instance
            .locals
            .insert("カメラとの距離".into(), PropertyValue::Float(distance));
        instance.step(
            &db,
            &globals,
            &camera_effects,
            &mut commands,
            &existing,
            &library,
            random,
            frames,
            &mut updates,
            &mut fades,
        );
    }
    shots.retain(|s| s.playing());

    for (entity, u) in updates {
        if let Ok((_, mut set, mut transform)) = sets.get_mut(entity) {
            set.color = u.color;
            set.alpha = u.alpha;
            set.emission_rate = u.emission_rate;
            set.emission_scale = u.emission_scale;
            set.emission_interval = u.emission_interval;
            set.life_scale = u.life_scale;
            *transform = Transform::from_matrix(Mat4::from(u.matrix));
        }
    }
    for entity in fades {
        if let Ok((_, mut set, _)) = sets.get_mut(entity) {
            set.fading = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_format::elink::{Call, Container, Curve};

    fn global(name: &str) -> PropertyRef {
        PropertyRef {
            name: name.into(),
            global: true,
        }
    }

    fn call(
        key: &str,
        parent: Option<usize>,
        condition: Option<Condition>,
        kind: CallKind,
    ) -> Call {
        Call {
            key: key.into(),
            parent,
            condition,
            duration: 1,
            kind,
        }
    }

    fn when(compare: Compare, value: Value) -> Option<Condition> {
        Some(Condition::Switch { compare, value })
    }

    /// `RainSnow`: a switch on 気温 between snow and rain, rain a switch
    /// on 濃度:雨 between heavy (≥ 2) and normal (> 0).
    fn rain_snow() -> User {
        let switch = |children: Vec<usize>, watch: &str| {
            CallKind::Container(Container {
                kind: ContainerKind::Switch,
                children,
                watch: Some(global(watch)),
            })
        };
        User {
            calls: vec![
                call("RainSnow", None, None, switch(vec![1, 2], "気温")),
                call(
                    "Snow",
                    Some(0),
                    when(Compare::LessOrEqual, Value::Float(-2.0)),
                    CallKind::Empty,
                ),
                call(
                    "Rain",
                    Some(0),
                    when(Compare::Greater, Value::Float(-2.0)),
                    switch(vec![3, 4], "濃度:雨"),
                ),
                call(
                    "HeavyRain",
                    Some(2),
                    when(Compare::GreaterOrEqual, Value::Float(2.0)),
                    CallKind::Empty,
                ),
                call(
                    "NormalRain",
                    Some(2),
                    when(Compare::Greater, Value::Float(0.0)),
                    CallKind::Empty,
                ),
            ],
            ..Default::default()
        }
    }

    fn choice(user: &User, globals: &GlobalProperties, call: usize) -> Option<usize> {
        let locals = HashMap::new();
        let properties = Properties {
            globals,
            locals: &locals,
        };
        switch_choice(user, &properties, call)
    }

    #[test]
    fn a_switch_takes_its_first_matching_child() {
        let user = rain_snow();
        let mut g = GlobalProperties::default();
        g.set("気温", PropertyValue::Float(-2.0));
        assert_eq!(choice(&user, &g, 0), Some(1));
        g.set("気温", PropertyValue::Float(5.0));
        assert_eq!(choice(&user, &g, 0), Some(2));
        // 濃度:雨 climbing to 2: normal rain, heavy only at 2 exactly.
        g.set("濃度:雨", PropertyValue::Float(1.5));
        assert_eq!(choice(&user, &g, 2), Some(4));
        g.set("濃度:雨", PropertyValue::Float(2.0));
        assert_eq!(choice(&user, &g, 2), Some(3));
        g.set("濃度:雨", PropertyValue::Float(0.0));
        assert_eq!(choice(&user, &g, 2), None);
    }

    #[test]
    fn enum_conditions_compare_entries_by_index() {
        let mut g = GlobalProperties::default();
        g.set("天候", PropertyValue::Enum(8));
        let locals = HashMap::new();
        let p = Properties {
            globals: &g,
            locals: &locals,
        };
        let weather = global("天候");
        let named = |n: &str| Value::Enum(n.into());
        assert!(p.holds(&weather, Compare::Equal, &named("BlueskyRain")));
        assert!(!p.holds(&weather, Compare::Equal, &named("Rain")));
        assert!(p.holds(&weather, Compare::NotEqual, &named("Unknown")));
        g.set("地方", PropertyValue::Enum(6));
        let p = Properties {
            globals: &g,
            locals: &locals,
        };
        assert!(p.holds(&global("地方"), Compare::Equal, &named("ゲルド高原乾燥")));
        // An unset float reads 0.
        assert!(!p.holds(
            &global("濃度:砂嵐"),
            Compare::GreaterOrEqual,
            &Value::Float(0.1)
        ));
    }

    #[test]
    fn random_containers_pick_by_running_weight() {
        let mut user = rain_snow();
        for (i, w) in [(1, 1.0), (2, 3.0)] {
            user.calls[i].condition = Some(Condition::Random { weight: w });
        }
        assert_eq!(pick_by_weight(&user, &[1, 2], 0.0), Some(1));
        assert_eq!(pick_by_weight(&user, &[1, 2], 0.24), Some(1));
        assert_eq!(pick_by_weight(&user, &[1, 2], 0.26), Some(2));
        assert_eq!(pick_by_weight(&user, &[3, 4], 0.5), None);
    }

    #[test]
    fn matrix_modes_place_like_the_game() {
        let source = Source {
            rotation: Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            translation: Vec3::new(10.0, 20.0, 30.0),
            scale: Vec3::splat(2.0),
        };
        let p = Vec3::new(0.0, 0.0, -30.0);
        let at = |mode| set_matrix(mode, &source, 1.0, p, Vec3::ZERO);
        // 0: in the source's frame, the user's scale on the asset.
        let m0 = at(0);
        assert!(
            m0.translation
                .abs_diff_eq(Vec3::new(-20.0, 20.0, 30.0).into(), 1e-4)
        );
        assert!(m0.matrix3.x_axis.length() > 1.99);
        // 2: turned with the source, unit scale.
        let m2 = at(2);
        assert!((m2.matrix3.x_axis.length() - 1.0).abs() < 1e-5);
        // 4: at the source, world axes, offset not turned.
        let m4 = at(4);
        assert!(
            m4.translation
                .abs_diff_eq(Vec3::new(10.0, 20.0, 0.0).into(), 1e-4)
        );
        assert!(m4.matrix3.x_axis.abs_diff_eq(Vec3::X.into(), 1e-6));
        // 6: world axes, offset turned by the source.
        let m6 = at(6);
        assert!(
            m6.translation
                .abs_diff_eq(Vec3::new(-20.0, 20.0, 30.0).into(), 1e-4)
        );
        assert!(m6.matrix3.x_axis.abs_diff_eq(Vec3::X.into(), 1e-6));
    }

    #[test]
    fn curves_follow_their_property() {
        let curve = Curve {
            property: global("濃度:雷"),
            kind: 0,
            points: vec![[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]],
        };
        assert!((curve.eval(0.75) - 0.625).abs() < 1e-6);
        assert_eq!(curve.eval(3.0), 1.0);
    }

    #[test]
    fn camera_overlays_are_independently_opt_in_and_leave_weather_running() {
        let switch = || {
            CallKind::Container(Container {
                kind: ContainerKind::Switch,
                children: Vec::new(),
                watch: Some(global("scene")),
            })
        };
        let user = User {
            calls: vec![
                call("Weather", None, None, switch()),
                call("VolumeSwitch_Dust", None, None, switch()),
                call("VolumeSwitch_Fog", None, None, switch()),
                call("VolumeSwitch_Add", None, None, switch()),
                call("FieldEnvEffect", None, None, switch()),
            ],
            always_triggers: vec![0, 1, 2, 3],
            ..default()
        };
        let mut instance = Instance::camera(&user, Source::at(&Transform::IDENTITY));
        let db = ElinkDb {
            users: [("Camera".into(), user)].into(),
            ..default()
        };
        let library = EffectLibrary::new(PathBuf::new());
        let mut world = World::new();
        let mut commands = world.commands();
        let globals = GlobalProperties::default();
        let existing = HashSet::new();
        let mut random = SeadRandom::new(1);
        let mut updates = Vec::new();
        let mut fades = Vec::new();
        for enabled in [
            [false, false, false, false],
            [true, false, false, false],
            [false, true, false, false],
            [false, false, true, false],
            [false, false, false, true],
            [true, true, true, true],
            [false, false, false, false],
        ] {
            instance.step(
                &db,
                &globals,
                &CameraEffectSettings {
                    volume_dust: enabled[0],
                    volume_fog: enabled[1],
                    volume_add: enabled[2],
                    field_haze: enabled[3],
                },
                &mut commands,
                &existing,
                &library,
                &mut random,
                1.0,
                &mut updates,
                &mut fades,
            );
            assert!(
                instance.roots[0].node.is_some(),
                "always-trigger effects must continue"
            );
            for (root, wanted) in instance.roots[1..].iter().zip(enabled) {
                assert_eq!(root.node.is_some(), wanted, "{:?}", root.kind);
            }
        }
        let defaults = CameraEffectSettings::default();
        assert!(!defaults.field_haze);
        assert!(!defaults.volume_dust);
        assert!(!defaults.volume_fog);
        assert!(!defaults.volume_add);
    }
}
