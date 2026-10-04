//! Day and night: game time flows like in BotW (one game minute per real
//! second, a day in 24 minutes) and moves the sun across the sky.
//!
//! The sun and the light follow the game's sky calculation
//! (`game_main_light`): on the field the sun rises in the east at 04:00,
//! stands 42° up in the north at 12:30 and sets in the west at 21:00; at
//! night a night light crosses the sky the same way from 22:00 to 03:00.
//! One main light shades the land from there, cut so that it never comes
//! from lower than some 27° (the game's `dir_main`); the sky, the haze and
//! the clouds are lit from the uncut direction. Stars, the moon (with the
//! game's phases, where the night light is) and the sun's disk are drawn on
//! a dome around the camera (`sky.wgsl`), over the sky.
//!
//! Light colours come from the game's time-of-day palettes
//! (`WorldMgr/normal.bwinfo`, see `asset_format::env`), blended between
//! the eight parts of the day like the game does; without a dump, from
//! hand-picked palettes in the same shape. The climate around the camera
//! picks the palette sets: like the game's, a clear and an overcast one,
//! blended by how overcast the sky is; with the weather it also tints the
//! light and scales the sky's scattering (see `climate.rs`).

use std::path::PathBuf;

use asset_format::env::{
    Climate as ClimateDefines, DIVISIONS, EnvPalette, EnvPaletteStatic, EnvParams, Influence,
    PALETTE_SETS, SKY_CLEAR, SKY_OVERCAST, SkyClouds, SunParams, is_field_palette_set,
    row_palette_sets, sky_division,
};
use asset_format::envset::{ENV_PACK, ENV_SET, EnvSet};
use asset_format::paths;
use asset_format::sky::CloudTextures;
use asset_format::texture::Texture;
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::atmosphere::{PhaseFunction, ScatteringMedium};
use bevy::light::{
    CascadeShadowConfig, CascadeShadowConfigBuilder, NotShadowCaster, NotShadowReceiver, SunDisk,
};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::climate::{Climate, Moisture, PaletteRows, Weather};
use crate::ready::SceneEpoch;
use crate::texture::NamedTexture;

const SKY_SHADER: Handle<Shader> = uuid_handle!("b3f0d7a2-5c19-4e64-8a3b-9d27e1c5f480");

/// Radius of the star dome; inside the camera's far plane (30 km).
const DOME_RADIUS: f32 = 25_000.0;

/// Where a session starts: a clear morning.
pub const DEFAULT_START: f32 = 10.0;

pub struct DayNightPlugin {
    /// Time of day to start at, in hours.
    pub start: f32,
    /// Whether time moves on its own (screenshots hold it still).
    pub flowing: bool,
    /// The `assets/` folder to read the palettes from.
    pub assets: PathBuf,
}

impl Plugin for DayNightPlugin {
    fn build(&self, app: &mut App) {
        let speed = if self.flowing {
            TimeOfDay::GAME_SPEED
        } else {
            0.0
        };
        load_internal_asset!(app, SKY_SHADER, "sky.wgsl", Shader::from_wgsl);
        let assets = self.assets.clone();
        // Only with the baked sky: whatever it lacks is said, not silently
        // replaced by the hand-picked fallbacks.
        let loading = assets.join(paths::ENV_PARAMS).exists().then(|| {
            AsyncComputeTaskPool::get().spawn(async move {
                let params = asset_format::read_ron::<EnvParams>(&assets.join(paths::ENV_PARAMS))
                    .inspect_err(|error| warn!("time-of-day palettes unavailable: {error}"))
                    .ok();
                let set_path = assets.join(paths::ENV_SET);
                let set = if set_path.exists() {
                    asset_format::read_ron::<EnvSet>(&set_path)
                        .inspect_err(|error| warn!("renderer environment unavailable: {error}"))
                        .ok()
                } else {
                    warn!(
                        "renderer environment: no {} (the game's {ENV_SET} in {ENV_PACK}); \
                         using the hand-picked EnvSet::fallback",
                        paths::ENV_SET
                    );
                    None
                };
                let textures =
                    asset_format::read_ron::<CloudTextures>(&assets.join(paths::CLOUD_TEXTURES))
                        .inspect_err(|error| warn!("cloud textures unavailable: {error}"))
                        .ok();
                let read = |name: &str| {
                    let path = assets.join(paths::CLOUD_TEXTURE_DIR).join(name);
                    Texture::read(&path).map(|texture| NamedTexture {
                        name: name.to_owned(),
                        texture,
                    })
                };
                let cloud_shadow = set.as_ref().and_then(|set| {
                    let Some(reference) = &set.cloud_shadow_texture else {
                        warn!("cloud shadow: the environment set names no texture; no shadow");
                        return None;
                    };
                    let Some(name) = textures.as_ref().and_then(|t| t.shadow.as_ref()) else {
                        warn!("cloud shadow: {reference:?} is not baked; no shadow");
                        return None;
                    };
                    read(name)
                        .inspect_err(|error| {
                            warn!("cloud shadow: {reference:?}: {error}; no shadow")
                        })
                        .ok()
                });
                // The textures the sky's clouds are drawn with, by number.
                let cloud_textures = textures
                    .map(|t| t.textures.iter().map(|name| read(name)).collect())
                    .transpose()
                    .inspect_err(|error| warn!("cloud textures unavailable: {error}"))
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                (params, set, cloud_shadow, cloud_textures)
            })
        });
        let time = TimeOfDay {
            hours: self.start.rem_euclid(24.0),
            day: 0,
            speed,
        };
        let environment = Environment::fallback();
        let sky = environment.sky(&time, &Climate::default(), &Weather::default());
        app.insert_resource(time)
            .insert_resource(environment)
            .insert_resource(sky)
            .insert_resource(LoadingEnvironment(loading))
            .init_resource::<SkyMedium>()
            .add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .add_systems(Startup, (spawn_lights, spawn_sky))
            .add_systems(
                Update,
                (
                    finish_loading,
                    advance_time,
                    update_sky_state,
                    move_lights,
                    scatter_like_the_game,
                )
                    .chain(),
            )
            .add_systems(PostUpdate, update_sky.before(TransformSystems::Propagate));
    }
}

/// The time of day: other systems read it (e.g. to know whether it is night).
#[derive(Resource, Clone, Debug)]
pub struct TimeOfDay {
    /// Hours since midnight, 0–24.
    pub hours: f32,
    /// Days passed since the start.
    pub day: u32,
    /// Game hours per real second.
    pub speed: f32,
}

impl TimeOfDay {
    /// BotW: a game minute every 30 frames at 30 fps (`TimeMgr`).
    pub const GAME_SPEED: f32 = 1.0 / 60.0;

    /// Night by the game's flags: before 06:00 and from 18:00.
    #[allow(dead_code)]
    pub fn is_night(&self) -> bool {
        !(6.0..18.0).contains(&self.hours)
    }

    /// The moon's phase like the game's `MoonType`: 0 full, 2 last
    /// quarter, 4 new, 6 first quarter; the next phase comes at noon.
    // SI-SKY-05: procedural stars and moon are ours.
    pub fn moon_phase(&self) -> u32 {
        (self.day + u32::from(self.hours > 12.0) + 1) % 8
    }

    /// The clock as `HH:MM`.
    pub fn clock(&self) -> String {
        let minutes = (self.hours * 60.0) as u32;
        format!("{:02}:{:02}", minutes / 60 % 24, minutes % 60)
    }

    fn advance(&mut self, game_hours: f32) {
        self.hours += game_hours;
        while self.hours >= 24.0 {
            self.hours -= 24.0;
            self.day += 1;
        }
    }
}

/// Where the sun's path begins and ends, as the game's time angle (hours ×
/// 15°): it rises at 60° (04:00) and sets at 315° (21:00) (`0x03656be0`).
const SUN_RISES: f32 = 60.0;
const SUN_SETS: f32 = 315.0;
/// Where the night light's arc begins and ends (the same code): it rises at
/// 330° (22:00) and sets at 45° (03:00).
const NIGHT_LIGHT_RISES: f32 = 330.0;
const NIGHT_LIGHT_SETS: f32 = 45.0;
/// Length of the game's light direction before `SunDirYStop` cuts it and it
/// is normalized (`0x03656be0`).
const GAME_LIGHT_LENGTH: f32 = 80_000.0;
/// Tilt of the night light's arc: it turns about the axis (0, −sin, −cos)
/// of this angle (47.5°, the constant at `0x10300e74`).
const NIGHT_ARC_TILT: f32 = 0.829_031_3;
/// How fast the game's main light fades out, and again in, when it
/// switches between the sun and the night light: its fade (`SkyMgr+0x2114`)
/// moves by `SkyMgr+0x211c` × `TimeMgr+0xb0` a 30 Hz frame, 0.0065 × 1
/// (constant `0x10300dc0`), at least and at most that (@`0x03657b1c..
/// 0x03657c74`, Wii U v208): 154 frames each way, about 5 s.
const LIGHT_SWITCH_STEP: f32 = 0.0065;

/// Towards the point `t` of the sun's path (0 rising in the east, ½ at its
/// height, 1 setting in the west): the game's day vector reversed,
/// `(cos tπ, sin tπ, slope·sin tπ)`, not normalized. The field's slope
/// (−1.1) leans it north, 42° up at its height.
fn on_sun_path(t: f32, slope: f32) -> Vec3 {
    let (sin, cos) = (t * std::f32::consts::PI).sin_cos();
    Vec3::new(cos, sin, slope * sin)
}

/// Towards the point `t` of the night light's arc (0 rising in the east, 1
/// setting in the west): the game's night vector reversed, a unit circle
/// tilted north by `NIGHT_ARC_TILT`, 42.5° up at its height.
fn on_night_arc(t: f32) -> Vec3 {
    let (sin, cos) = (t * std::f32::consts::PI).sin_cos();
    Vec3::new(cos, NIGHT_ARC_TILT.cos() * sin, -NIGHT_ARC_TILT.sin() * sin)
}

/// How far along a path that is up between the time angles `rises` and
/// `sets` it is at `hours`: 0–1 while it is up, 1–2 while it is down (it
/// carries on round the same circle below the horizon, at the pace that
/// brings it back up in time).
fn along(hours: f32, rises: f32, sets: f32) -> f32 {
    let since_rise = (hours * 15.0 - rises).rem_euclid(360.0);
    let up = (sets - rises).rem_euclid(360.0);
    if since_rise <= up {
        since_rise / up
    } else {
        1.0 + (since_rise - up) / (360.0 - up)
    }
}

/// Towards the sun at `hours`. By day (04:00–21:00) the game's sun path
/// (`0x03656be0` with the palettes' `SunSlope`: on the field it rises in the
/// east, stands 42° up in the north at 12:30 and sets in the west). The game
/// has no sun at night; the viewer carries it on round the same circle below
/// the horizon, from 21:00 to 04:00, for what it measures by the sun's
/// height (dusk, night, the glow round a low sun).
pub fn sun_direction(hours: f32, sun: &SunParams) -> Vec3 {
    on_sun_path(along(hours, SUN_RISES, SUN_SETS), sun.slope).normalize()
}

/// Towards the moon at `hours`: the game's night light, whose arc
/// (`0x03656be0`) rises in the east at 22:00, stands 42.5° up in the north
/// at 00:30 and sets in the west at 03:00 (below the horizon the rest of the
/// day). Where the game draws its moon sprite is not traced; this puts it
/// where the night light shades from.
pub fn moon_direction(hours: f32) -> Vec3 {
    on_night_arc(along(hours, NIGHT_LIGHT_RISES, NIGHT_LIGHT_SETS))
}

/// The game's main light at a time of day ([`game_main_light`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GameLight {
    /// Where `dir_main` shades from (towards the light, cut at
    /// `SunDirYStop`).
    pub towards: Vec3,
    /// Where the sky's light is (towards it, uncut: the sky object's
    /// `+0x72c`, the sky table's `cSunDir`/`cSunZenithAngle`).
    pub sky: Vec3,
    /// `base_light_change_ratio`: 1, less while the light switches.
    pub change: f32,
}

/// The game's main light (`dir_main`) and the sky's light at `hours`, from
/// the field's sky calculation (Wii U v208 `0x03656be0`, time angle =
/// hours × 15°):
///
/// - by day (04:00–21:00) the sun's path, from the east to the west through
///   the sky `slope` tilts it to (the field's −1.1: the north, 42° up at
///   12:30); before 04:00 it waits in the east, after 21:00 in the west;
/// - by night (22:00–03:00) the night light's arc, from the east to the
///   west, as high at 00:30 as the sun at noon;
/// - `dir_main`, `GAME_LIGHT_LENGTH` long, keeps its Y at or below
///   `SunDirYStop` (the field's −42 000: the light never shades from lower
///   than about 27°), then is normalized. The sky's light keeps the uncut
///   direction (the game also eases it towards a new direction by at most
///   about 0.05 a frame, which only shows as a quick swing when the light
///   switches; not repeated).
///
/// When the light switches it fades out and back in ([`LightSwitch`]);
/// here `change` is 1.
pub fn game_main_light(hours: f32, sun: &SunParams) -> GameLight {
    let at = hours.rem_euclid(24.0);
    let angle = at * 15.0;
    let towards = if is_night_light(at) {
        on_night_arc(along(at, NIGHT_LIGHT_RISES, NIGHT_LIGHT_SETS))
    } else {
        let t = ((angle - SUN_RISES) / (SUN_SETS - SUN_RISES)).clamp(0.0, 1.0);
        on_sun_path(t, sun.slope)
    };
    let mut cut = -towards * GAME_LIGHT_LENGTH;
    cut.y = cut.y.min(sun.dir_y_stop);
    GameLight {
        towards: -cut.normalize_or(Vec3::NEG_Y),
        sky: towards.normalize_or(Vec3::Y),
        change: 1.0,
    }
}

/// Whether the game's light is the night light at `hours` (the night branch
/// of `0x03656be0`, 22:00–03:00).
fn is_night_light(hours: f32) -> bool {
    !(NIGHT_LIGHT_SETS..=NIGHT_LIGHT_RISES).contains(&(hours.rem_euclid(24.0) * 15.0))
}

/// The game's fade of the main light as it switches between the sun and the
/// night light (`0x03656be0`, Wii U v208): entering or leaving the night
/// branch (`SkyMgr+0x2190`) sets the target `+0x2118` to 1; the fade
/// `+0x2114` follows it by [`LIGHT_SWITCH_STEP`] a frame; while the target
/// is up the directions are not updated; once the fade is 1 the target
/// returns to 0 and the fade follows it back. `base_light_change_ratio` is
/// `1 − fade`: `0x03656be0` hands min(1, fade + the lightning's `+0x2f0`
/// term, added in `update_sky_state`) to `0x03408770`, which sets 1 − that.
/// A scene starts with the light settled.
// SI-WTH-05: the DifUse conditions are left out.
#[derive(Clone, Debug, Default)]
pub struct LightSwitch {
    night: Option<bool>,
    fading: bool,
    fade: f32,
    held: Option<GameLight>,
}

impl LightSwitch {
    /// One frame: the light `light` at `hours` after `frames` of the game's
    /// 30 a second; what the game shades with.
    pub fn step(&mut self, light: GameLight, hours: f32, frames: f32) -> GameLight {
        let night = is_night_light(hours);
        if self.night.is_some_and(|was| was != night) {
            self.fading = true;
        }
        self.night = Some(night);
        if !self.fading || self.held.is_none() {
            self.held = Some(light);
        }
        let target = if self.fading { 1.0 } else { 0.0 };
        let step = LIGHT_SWITCH_STEP * frames;
        let gap = target - self.fade;
        self.fade = if gap.abs() <= step {
            target
        } else {
            self.fade + step.copysign(gap)
        };
        if self.fade >= 1.0 {
            self.fading = false;
        }
        GameLight {
            change: 1.0 - self.fade.min(1.0),
            ..self.held.unwrap_or(light)
        }
    }
}

/// The palettes of the eight parts of the day, from the game or hand-picked.
#[derive(Resource, Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Environment {
    /// Palette set 0: the ordinary field in clear weather.
    pub palettes: [EnvPalette; DIVISIONS],
    /// Every palette set (`EnvAttribute_N`; climates pick one), set 0 first.
    pub sets: Vec<[EnvPalette; DIVISIONS]>,
    pub sun: SunParams,
    /// The sky's cloud layers.
    pub clouds: SkyClouds,
    /// The climates (`ClimateDefines_N`); none without a dump.
    pub climates: Vec<ClimateDefines>,
    /// How each weather tints the light (`WeatherInfluence_N`).
    pub weather_influences: Vec<Influence>,
    /// What the palettes share (`EnvPaletteStatic`).
    #[allow(dead_code)] // Read by the wave-3 look work (docs/STYLE.md).
    pub palette_static: EnvPaletteStatic,
    /// The renderer's base environment (`Env/env.sgenvb`): hemisphere light,
    /// fogs, colour correction, bloom, sky scattering, light-map curves and
    /// rim light; hand-picked without a dump.
    #[allow(dead_code)] // Read by the wave-3 look work (docs/STYLE.md).
    pub renderer: EnvSet,
}

impl Environment {
    /// From the dump's parameters; `renderer` is `None` when the dump's
    /// environment set could not be read (the loader has said why).
    fn from_game(params: &EnvParams, renderer: Option<EnvSet>) -> Self {
        let set =
            |set: usize| std::array::from_fn(|division| params.palette(set, division).clone());
        // Every set the game has, as its palette lookup resolves them.
        let sets: Vec<[EnvPalette; DIVISIONS]> = (0..PALETTE_SETS).map(set).collect();
        let clouds = params.clouds.clone().unwrap_or_else(|| {
            warn!(
                "sky clouds: the time-of-day parameters have no cloud layers; \
                 using the hand-picked fallback_sky_clouds"
            );
            crate::clouds::fallback_sky_clouds()
        });
        Self {
            palettes: sets[0].clone(),
            sets,
            sun: params.sun.clone(),
            clouds,
            climates: params.climates.clone(),
            weather_influences: params.weather_influences.clone(),
            palette_static: params.palette_static.clone(),
            renderer: renderer.unwrap_or_else(EnvSet::fallback),
        }
    }

    /// Hand-picked palettes for when there is no dump: warm days, orange
    /// dawn and dusk, a blue night.
    pub fn fallback() -> Self {
        let palette =
            |light: [f32; 3], intensity: f32, lit: [f32; 3], shade: [f32; 3], sky_sun: [f32; 3]| {
                EnvPalette {
                    light_color: light,
                    light_intensity: intensity,
                    cloud_base: lit,
                    cloud_base_intensity: 0.5,
                    cloud_highlight: lit,
                    cloud_highlight_intensity: 0.5,
                    cloud_shadow: shade,
                    cloud_shadow_intensity: 1.0,
                    cloud_backlight: lit,
                    sky_sun_color: sky_sun,
                    ..EnvPalette::default()
                }
            };
        let palettes = [
            // 03:00 before dawn, 04:00 dawn, 06:00 morning, 09:00 day.
            palette(
                [0.6, 0.7, 1.0],
                1.5,
                [0.3, 0.36, 0.5],
                [0.12, 0.15, 0.22],
                [0.6, 0.8, 1.0],
            ),
            palette(
                [1.0, 0.8, 0.65],
                3.5,
                [1.0, 0.72, 0.55],
                [0.35, 0.33, 0.4],
                [1.0, 0.8, 0.7],
            ),
            palette(
                [1.0, 0.92, 0.72],
                7.0,
                [1.0, 0.95, 0.85],
                [0.55, 0.6, 0.66],
                [1.0, 0.95, 0.85],
            ),
            palette(
                [1.0, 0.95, 0.85],
                9.0,
                [1.0, 0.98, 0.95],
                [0.55, 0.62, 0.74],
                [1.0, 0.95, 0.9],
            ),
            // 16:00 afternoon, 18:00 sunset, 20:00 dusk, 21:00 night.
            palette(
                [1.0, 0.72, 0.5],
                7.0,
                [1.0, 0.8, 0.55],
                [0.45, 0.45, 0.5],
                [1.0, 0.7, 0.45],
            ),
            palette(
                [1.0, 0.55, 0.45],
                4.0,
                [1.0, 0.55, 0.35],
                [0.2, 0.17, 0.2],
                [1.0, 0.55, 0.45],
            ),
            palette(
                [0.8, 0.6, 0.75],
                1.2,
                [0.35, 0.28, 0.32],
                [0.15, 0.13, 0.16],
                [0.7, 0.6, 1.0],
            ),
            palette(
                [0.5, 0.75, 1.0],
                1.8,
                [0.2, 0.28, 0.38],
                [0.06, 0.1, 0.14],
                [0.6, 0.9, 1.0],
            ),
        ];
        // Thin air at night, thick haze glowing round the low sun.
        let scattering = [
            (0.5, 1.0),
            (1.0, 6.0),
            (1.0, 3.0),
            (1.0, 2.0),
            (1.0, 5.0),
            (1.0, 12.0),
            (0.8, 9.0),
            (0.35, 0.5),
        ];
        let palettes = std::array::from_fn(|i| EnvPalette {
            rayleigh_amplifier: scattering[i].0,
            mie_amplifier: scattering[i].1,
            ..palettes[i].clone()
        });
        // Set 1, under an overcast sky: half the light, like the game's field.
        let overcast = palettes.clone().map(|p| EnvPalette {
            light_intensity: p.light_intensity * 0.5,
            ..p
        });
        Self {
            sets: vec![palettes.clone(), overcast],
            palettes,
            sun: SunParams::default(),
            clouds: crate::clouds::fallback_sky_clouds(),
            climates: Vec::new(),
            // Clear, cloudy, rain, heavy rain: ever greyer and flatter.
            weather_influences: vec![
                Influence::default(),
                Influence {
                    feature_color: [0.65, 0.65, 0.68],
                    mie: 1.8,
                    ..Influence::default()
                },
                Influence {
                    feature_color: [0.5, 0.58, 0.7],
                    rayleigh: 0.6,
                    mie: 0.8,
                    ..Influence::default()
                },
                Influence {
                    feature_color: [0.35, 0.42, 0.52],
                    rayleigh: 0.6,
                    mie: 0.8,
                    ..Influence::default()
                },
            ],
            palette_static: EnvPaletteStatic::default(),
            renderer: EnvSet::fallback(),
        }
    }

    /// The clear palette of set 0 at `hours`, blended between parts of the
    /// day.
    #[cfg(test)]
    pub fn at(&self, hours: f32) -> EnvPalette {
        self.at_in_set(0, hours)
    }

    /// `set`, or the ordinary set 0 if `set`'s palettes have no light at all
    /// (set 10, the Dark Woods': the game lights them by other means).
    // SI-LGT-04: falling back to palette set 0 when a set is all dark is our heuristic.
    fn lit_set(&self, set: usize) -> usize {
        let lit = self
            .sets
            .get(set)
            .is_some_and(|palettes| palettes.iter().any(|p| p.light_intensity > 0.01));
        if lit { set } else { 0 }
    }

    /// The palette of palette set `set` at `hours`.
    fn at_in_set(&self, set: usize, hours: f32) -> EnvPalette {
        let (previous, current, t) = sky_division(hours);
        let palettes = self.sets.get(set).unwrap_or(&self.palettes);
        palettes[previous].lerp(&palettes[current], t)
    }

    /// The palette of row of palette sets `row` (a climate's
    /// `PaletteSetSelect`) at `hours` like the game's weather update
    /// (`0x036425b8`): the row gives a clear and an overcast set, blended by
    /// the weight of the overcast sky `overcast` (0–1): `lerp(lerp(a, b,
    /// t), lerp(c, d, t), w)` over the sets' palettes of the two divisions,
    /// as `ENV_BilerpPaletteSetValues` (`0x03642418`) blends every field.
    fn at_row(&self, row: usize, hours: f32, overcast: f32) -> EnvPalette {
        let sets = row_palette_sets(row);
        let (clear, cloudy) = (
            self.lit_set(sets[SKY_CLEAR]),
            self.lit_set(sets[SKY_OVERCAST]),
        );
        let palette = self.at_in_set(clear, hours);
        if clear == cloudy || overcast <= 0.0 {
            return palette;
        }
        palette.lerp(&self.at_in_set(cloudy, hours), overcast.min(1.0))
    }

    /// The palette at `hours` under a sky `overcast` (0–1): the former and
    /// the active row of palette sets, each blended as [`Self::at_row`],
    /// then lerped by the rows' transition (`EnvMgr+0x198`).
    fn at_rows(&self, hours: f32, rows: &PaletteRows, overcast: f32) -> EnvPalette {
        let [(previous, _), (active, share)] = rows.shares();
        let palette = self.at_row(active, hours, overcast);
        if share >= 1.0 || previous == active {
            return palette;
        }
        self.at_row(previous, hours, overcast)
            .lerp(&palette, share.max(0.0))
    }

    /// What the strength of the game's `fog_scatter` (the height and the ad
    /// hoc fog) and the ad hoc fog's `atten_sky` are made of at `hours` for
    /// the rows of palette sets `rows` under a sky `overcast` (0–1): the
    /// weather update (`ENV_UpdateWeatherPalettes` `0x036425b8`, PPC
    /// @`0x036448ec..0x03644994` and @`0x03647420..0x036474c4`, Wii U v208)
    /// replaces, in each palette before the blend, the `FogColor` alpha by
    /// the moisture where the palette's set is one of the field's
    /// ([`is_field_palette_set`]) and `afParam_attenuationForSky` by the
    /// moisture × 4.8 in sets 0 and 1; the blend is linear, so the parts are
    /// the weights of those palettes and the blended values of the others.
    // SI-SKY-02: no EnvMgr+0x3d58 values, special branches or lightning.
    fn moist_fog(&self, hours: f32, rows: &PaletteRows, overcast: f32) -> MoistFog {
        let mut fog = MoistFog::default();
        let mut add = |set: usize, weight: f32| {
            let palette = self.at_in_set(self.lit_set(set), hours);
            if is_field_palette_set(set) {
                fog.moist += weight;
            } else {
                fog.alpha += weight * palette.fog_color[3];
            }
            if set == 0 || set == 1 {
                fog.moist_sky += weight;
            } else {
                fog.attenuation_sky += weight * palette.attenuation_sky;
            }
        };
        let overcast = overcast.clamp(0.0, 1.0);
        for (row, share) in rows.shares() {
            let sets = row_palette_sets(row);
            add(sets[SKY_CLEAR], share * (1.0 - overcast));
            add(sets[SKY_OVERCAST], share * overcast);
        }
        fog
    }

    /// The climates' factors (`FeatureColor`, `CalcRayleigh`, `CalcMie`,
    /// `CalcMieSymmetrical`, `CalcSfParam*`, `CalcVolumeMaskIntencity`),
    /// lerped from the former climate's to the current one's by the
    /// climate's transition (`ENV_UpdateWeatherPalettes`
    /// @`0x03642e04…0x036431b4`, Wii U v208).
    fn climate_influence(&self, climate: &Climate) -> Influence {
        Influence::weighted_mean(climate.shares().map(|(index, share)| {
            let influence = self
                .climates
                .get(index)
                .map_or_else(Influence::default, |c| c.influence);
            (influence, share)
        }))
    }

    /// Illuminance (lux) per unit of palette light intensity, in the game's
    /// units: its field lights a white surface facing the light to `env5 =
    /// BgDifColor × BgDifIntencity` (PS 32, no division by π; the CPU chain
    /// in docs/research/wiiu-render-cpu.md), Bevy to `lux / π · exposure`.
    /// At the camera's fixed exposure (`DAY_EV100`) one unit of intensity
    /// gives one unit in the frame, the input of the game's tone curve.
    fn lux_per_intensity(&self) -> f32 {
        std::f32::consts::PI / bevy::camera::Exposure { ev100: DAY_EV100 }.exposure()
    }

    /// The weathers' tint, blended by their shares.
    fn weather_influence(&self, weather: &Weather) -> Influence {
        Influence::weighted_mean(weather.weights.iter().enumerate().map(|(index, share)| {
            (
                self.weather_influences
                    .get(Weather::influence_index(index))
                    .copied()
                    .unwrap_or_default(),
                *share,
            )
        }))
    }

    /// The factor on the palette's colours, like the game's weather update
    /// (`ENV_UpdateWeatherPalettes` `0x036425b8`, Wii U v208): the climate's
    /// `FeatureColor` ([`Self::climate_influence`]) times the weather's
    /// colour `weather` per channel, on the field's row of palette sets, 0;
    /// no tint on the others (the woods). The game multiplies each row's
    /// colours before it lerps the two rows; here the two rows' factors are
    /// lerped by the same share and multiply the lerped colours (the same
    /// once a change of rows is done; archived GAPS.md notes RENDER-002). Its fade
    /// factor (`EnvMgr+0x3ce64`, 1 in play) is left out
    /// (docs/research/wiiu-render-cpu.md, "Where `F` and the fog product go").
    // SI-LGT-06: row factors blended apart from colours; fade factor k left out.
    fn feature(&self, climate: &Climate, weather: Vec3) -> Vec3 {
        let tint = Vec3::from(self.climate_influence(climate).feature_color) * weather;
        climate
            .rows
            .shares()
            .into_iter()
            .map(|(row, share)| if row == 0 { tint } else { Vec3::ONE } * share)
            .sum()
    }

    /// The factors and offsets on the palette's scalars, like
    /// [`Self::feature`] on its colours: the climate's `CalcRayleigh`,
    /// `CalcMie`, `CalcMieSymmetrical` and `CalcSfParam*` followed by the
    /// weather's (`weather`) on the field's row of palette sets, 0; none on
    /// the others (the woods). The bloom's factors are `TempMgr`'s
    /// (`moisture`), not the weathers' blend; the extra Mie step
    /// (`SkyMgr+0x2178`) is 1 in play (docs/research/wiiu-render-cpu.md,
    /// "The scalar factors").
    fn scalars(&self, climate: &Climate, weather: &Influence, moisture: &Moisture) -> Influence {
        let weather = &Influence {
            bloom_threshold: moisture.bloom_threshold,
            bloom_intensity: moisture.bloom_intensity,
            ..*weather
        };
        let field = self.climate_influence(climate).then(weather);
        Influence::weighted_mean(climate.rows.shares().map(|(row, share)| {
            let here = if row == 0 {
                field
            } else {
                Influence::default()
            };
            (here, share)
        }))
    }

    /// The sky with the bloom factors `weather` settles at (see
    /// [`Self::sky_with`]).
    pub(crate) fn sky(&self, time: &TimeOfDay, climate: &Climate, weather: &Weather) -> Sky {
        let moisture = Moisture::settled(weather, &self.weather_influences);
        self.sky_with(time, climate, weather, &moisture)
    }

    /// The sky for `time`, the climate, the weather and
    /// `TempMgr`'s bloom factors (`moisture`).
    pub(crate) fn sky_with(
        &self,
        time: &TimeOfDay,
        climate: &Climate,
        weather: &Weather,
        moisture: &Moisture,
    ) -> Sky {
        let towards_sun = sun_direction(time.hours, &self.sun);
        let towards_moon = moon_direction(time.hours);
        let overcast_sky = weather.overcast_sky();
        let palette = self.at_rows(time.hours, &climate.rows, overcast_sky);
        // The weathers' influence, blended by their shares.
        let weather = self.weather_influence(weather);
        // `F` (on the light, the sky's sun and the clouds) and the fogs'
        // factor, the weather's fog colour in place of its light colour.
        let light_feature = self.feature(climate, Vec3::from(weather.feature_color));
        let fog_feature = self.feature(climate, Vec3::from(weather.feature_fog_color));
        let scalars = self.scalars(climate, &weather, moisture);
        let moist_fog = self.moist_fog(time.hours, &climate.rows, overcast_sky);
        let horizon = horizon_color(&palette, towards_sun.y);
        let night = night_fraction(towards_sun.y);
        // SI-WTH-06: night fraction and sun_up from the viewer's sun height.
        let sun_up = smoothstep(-0.03, 0.03, towards_sun.y);
        let light = game_main_light(time.hours, &self.sun);
        Sky {
            palette,
            weather,
            light_feature,
            fog_feature,
            scalars,
            towards_sun,
            towards_moon,
            towards_light: light.sky,
            main_light: light.towards,
            light_change: light.change,
            sun_up,
            night,
            horizon,
            moist_fog,
            flash: 0.0,
            flash_peak: false,
        }
    }
}

/// The sky right now: where the sun and moon are and the current palette.
#[derive(Resource, Clone, Debug)]
pub struct Sky {
    pub palette: EnvPalette,
    /// The weather's part of it (overcast greys the clouds too).
    pub weather: Influence,
    /// What the game multiplies the palette's light colour (`BgDifColor`),
    /// the sky's sun (`SkySunColor`) and the cloud colours with: the
    /// climate's and the weather's `FeatureColor` where the climate uses the
    /// field's row of palette sets, 1 elsewhere ([`Environment::feature`]).
    pub light_feature: Vec3,
    /// The same for the fog colours (`FogColor`, `YFogColor`): the
    /// climate's `FeatureColor` times the weather's `FeatureFogColor`, on
    /// the field's row only.
    pub fog_feature: Vec3,
    /// The factors on the palette's sky scattering (`CalcRayleigh`,
    /// `CalcMie`, `CalcMieSymmetrical`) and bloom, and the offsets on its
    /// scattering fog (`CalcSfParam*`): the climate's and the weather's on
    /// the field's row, neutral elsewhere ([`Environment::scalars`]).
    pub scalars: Influence,
    /// Towards the sun ([`sun_direction`]: the game's path by day, below the
    /// horizon at night) and the moon ([`moon_direction`]).
    pub towards_sun: Vec3,
    pub towards_moon: Vec3,
    /// Where the sky's light is ([`GameLight::sky`]: the sun by day, the
    /// night light at night): what lights the sky table, the haze and the
    /// clouds.
    pub towards_light: Vec3,
    /// Where the game's main light shades from and how much of it is left
    /// while it switches between the sun and the night light
    /// ([`game_main_light`]): what lights the land, day and night.
    pub main_light: Vec3,
    pub light_change: f32,
    /// 1 while the sun is above the horizon, 0 once it has set.
    pub sun_up: f32,
    /// 0 by day and at sunset, 1 once the sun is well below the horizon.
    pub night: f32,
    /// Roughly the colour of the sky low over the horizon (for what fades
    /// into it, like distant clouds).
    pub horizon: Vec3,
    /// What the fogs' strength is made of ([`Environment::moist_fog`]).
    pub moist_fog: MoistFog,
    /// The lightning's flash (`WeatherMgr+0x2f0`, `crate::lightning`), 0–1,
    /// and whether it is at its height (its state 2), when the sky table
    /// evens out fully.
    pub flash: f32,
    pub flash_peak: bool,
}

/// The parts of the strength of the game's `fog_scatter` and of the ad hoc
/// fog's `atten_sky` ([`Environment::moist_fog`]): the weight of the
/// palettes that take the moisture and the blended value of the others.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MoistFog {
    /// Weight of the palettes whose strength is the moisture (0–1).
    pub moist: f32,
    /// The others' `FogColor` alpha, times their weight.
    pub alpha: f32,
    /// Weight of the palettes whose `atten_sky` is the moisture × 4.8.
    pub moist_sky: f32,
    /// The others' `afParam_attenuationForSky`, times their weight.
    pub attenuation_sky: f32,
}

/// How far into the night it is (0–1) by the sun's height: dusk lasts until
/// the sun is some 20° below the horizon (on the field's path, from about
/// 21:05 to 21:50, and back from 03:10 to 03:55).
// SI-WTH-06: night fraction and sun_up from the viewer's sun height.
fn night_fraction(sun_height: f32) -> f32 {
    1.0 - smoothstep(-0.35, -0.05, sun_height)
}

/// Pale blue by day, tinted like the sky's sun at dawn and dusk, deep blue
/// at night; `sun_height` is the sine of the sun's elevation.
// SI-LGT-02: horizon colour is fitted; only the far particle haze reads it.
fn horizon_color(palette: &EnvPalette, sun_height: f32) -> Vec3 {
    let day = Vec3::new(0.78, 0.84, 0.92);
    let night = Vec3::new(0.02, 0.035, 0.06);
    let tint = Vec3::from(palette.sky_sun_color);
    let twilight = tint / tint.max_element().max(1e-3) * 0.7;
    let lit = twilight.lerp(day, smoothstep(0.05, 0.3, sun_height));
    night.lerp(lit, smoothstep(-0.2, 0.05, sun_height))
}

/// The game's palettes and renderer environment, read in the background.
#[derive(Resource)]
pub struct LoadingEnvironment(Option<Task<LoadedEnvironment>>);

/// The palettes, the renderer's set, the cloud shadow texture it names and
/// the list of the environment's textures the clouds pick theirs from.
type LoadedEnvironment = (
    Option<EnvParams>,
    Option<EnvSet>,
    Option<NamedTexture>,
    Vec<NamedTexture>,
);

impl LoadingEnvironment {
    pub fn is_loading(&self) -> bool {
        self.0.is_some()
    }
}

fn finish_loading(
    mut commands: Commands,
    mut loading: ResMut<LoadingEnvironment>,
    mut environment: ResMut<Environment>,
) {
    let Some(task) = &mut loading.0 else { return };
    let Some((params, set, cloud_shadow, cloud_textures)) = block_on(poll_once(task)) else {
        return;
    };
    loading.0 = None;
    if let Some(texture) = cloud_shadow {
        commands.insert_resource(crate::clouds::CloudShadowTexture(texture));
    }
    if !cloud_textures.is_empty() {
        commands.insert_resource(crate::clouds::CloudTextureList(cloud_textures));
    }
    if set.is_some() {
        info!("renderer environment: the game's, from {}", paths::ENV_SET);
    }
    match params {
        Some(params) => {
            info!(
                "time-of-day palettes: {} from the game",
                params.palettes.len()
            );
            *environment = Environment::from_game(&params, set);
        }
        None => {
            if let Some(set) = set {
                environment.renderer = set;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_sky_state(
    real: Res<Time>,
    epoch: Option<Res<SceneEpoch>>,
    time: Res<TimeOfDay>,
    environment: Res<Environment>,
    climate: Option<Res<Climate>>,
    weather: Option<Res<Weather>>,
    moisture: Option<Res<Moisture>>,
    lightning: Option<Res<crate::lightning::Lightning>>,
    mut switch: Local<LightSwitch>,
    mut sky: ResMut<Sky>,
) {
    let (climate, weather, moisture) = (
        climate.as_deref().cloned().unwrap_or_default(),
        weather.as_deref().cloned().unwrap_or_default(),
        moisture.as_deref().cloned().unwrap_or_default(),
    );
    // A new scene (the next camera of a batch) starts with the light settled.
    if epoch.is_some_and(|epoch| epoch.is_changed()) {
        *switch = LightSwitch::default();
    }
    let mut now = environment.sky_with(&time, &climate, &weather, &moisture);
    let light = switch.step(
        GameLight {
            towards: now.main_light,
            sky: now.towards_light,
            change: now.light_change,
        },
        time.hours,
        real.delta_secs() * 30.0,
    );
    // `0x03656be0` hands min(1, switch fade + flash) to `0x03408770`.
    // SI-WTH-09: the flash does not fade the depth shadows.
    let (flash, flash_peak) = lightning.map_or((0.0, false), |l| (l.flash, l.at_peak()));
    (now.main_light, now.towards_light, now.light_change) =
        (light.towards, light.sky, (light.change - flash).max(0.0));
    (now.flash, now.flash_peak) = (flash, flash_peak);
    *sky = now;
}

/// The one main light, like the game's `dir_main`: the sun by day, the
/// night light at night.
#[derive(Component)]
pub struct MainLight;

/// Shadows near the player sharp, far terrain still shadowed.
// SI-LGT-05: Bevy shadow cascades, not the game's cascades and baked world shadow.
fn cascades() -> CascadeShadowConfig {
    CascadeShadowConfigBuilder {
        num_cascades: 4,
        minimum_distance: 0.3,
        first_cascade_far_bound: 20.0,
        maximum_distance: 600.0,
        overlap_proportion: 0.2,
    }
    .build()
}

fn spawn_lights(mut commands: Commands) {
    // No disk (a light without one gets the sun's): it shades from where the
    // game's light does, cut above the horizon, while the sun sinks;
    // `sky.wgsl` draws the sun and the moon where they are.
    commands.spawn((
        Name::new("main light"),
        MainLight,
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        SunDisk::OFF,
        cascades(),
    ));
}

/// Game hours per real second while T is held.
pub const FAST_FORWARD: f32 = 1.0;

fn advance_time(
    input: Option<Res<crate::viewer::ViewerInput>>,
    real: Res<Time>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut time: ResMut<TimeOfDay>,
) {
    let fast = !input.is_some_and(|input| input.keyboard)
        && keys.is_some_and(|keys| keys.pressed(KeyCode::KeyT));
    let speed = if fast { FAST_FORWARD } else { time.speed };
    time.advance(speed * real.delta_secs());
}

/// The palette's main light like the game's `env5`: its illuminance (lux,
/// from `BgDifIntencity`) and its colour, `BgDifColor` times the climate's
/// and the weather's `FeatureColor` per channel (`Sky::light_feature`): the
/// desert's light is yellow and brighter (its sand a warm yellow in the
/// game, C ≈ 40 on R/230, from a grey beige texture), rain's grey and
/// dimmer.
fn main_light(sky: &Sky, environment: &Environment) -> (f32, Vec3) {
    let palette = &sky.palette;
    (
        palette.light_intensity * environment.lux_per_intensity(),
        Vec3::from(palette.light_color) * sky.light_feature,
    )
}

/// Rec. 709 luminance weights (linear RGB).
const LUMINANCE: Vec3 = Vec3::new(0.2126, 0.7152, 0.0722);

/// One main light like the game's: the palette's light from where the
/// game's `dir_main` shades, by day and by night, fading while it switches
/// between the sun and the night light. Its colour is the palette's as it
/// is (the game's `env5`), not lifted against Bevy's atmosphere: the field,
/// the characters and the water read it straight (`deferred_light.wgsl`,
/// `water_material.wgsl`); only what Bevy lights (`apply_pbr_lighting`:
/// lava) gets its atmosphere's reddening on top.
fn move_lights(
    sky: Res<Sky>,
    environment: Res<Environment>,
    mut lights: Query<(&mut Transform, &mut DirectionalLight), With<MainLight>>,
) {
    let Ok((mut transform, mut light)) = lights.single_mut() else {
        return;
    };
    *transform = Transform::default().looking_to(-sky.main_light, Vec3::Y);
    let (lux, color) = main_light(&sky, &environment);
    light.illuminance = lux * sky.light_change;
    light.color = Color::linear_rgb(color.x, color.y, color.z);
}

/// The camera's exposure, fixed like the game's (its palettes' `Exposure`
/// does not reach the light or the frame: docs/research/wiiu-render-cpu.md);
/// the lights' units follow it (`Environment::lux_per_intensity`).
pub const DAY_EV100: f32 = 13.0;

/// Strength of the environment map's light (the atmosphere's,
/// `AtmosphereEnvironmentMapLight`, or the scene's cube map, `cubemap.rs`)
/// on the surfaces Bevy lights; the game's own shading reads the cube map at
/// full strength and divides this out (`cube_at` in `deferred_light.wgsl`).
/// A fit (half, once the clear day's share of a fill the game does not have).
// SI-LGT-01: strength of Bevy's environment light is fitted.
pub const SKY_LIGHT: f32 = 0.5;

/// The palette's sky scattering, relative to the midday palette's (the
/// brightest): Rayleigh and Mie scale and the haze's forward scattering.
// SI-LGT-03: Bevy atmosphere tuning (scattering, sky tint, CYAN_RAYLEIGH) is ours.
fn scattering(environment: &Environment, palette: &EnvPalette) -> Vec3 {
    let noon = environment
        .palettes
        .iter()
        .max_by(|a, b| a.light_intensity.total_cmp(&b.light_intensity))
        .unwrap_or(palette);
    let relative = |value: f32, reference: f32| {
        if reference > 1e-3 {
            value / reference
        } else {
            1.0
        }
    };
    Vec3::new(
        relative(palette.rayleigh_amplifier, noon.rayleigh_amplifier),
        relative(palette.mie_amplifier, noon.mie_amplifier),
        palette.mie_asymmetry.clamp(0.0, 0.95),
    )
}

/// Least real time between rebuilds of the atmosphere's medium, in seconds.
const MEDIUM_INTERVAL: f32 = 0.5;

/// Whether to rebuild the medium for scattering `target` at real time `now`,
/// having last built it for `last` (scattering, time): only for a real
/// change, and not more often than every `MEDIUM_INTERVAL` (fast-forwarding
/// time or crossing climates would otherwise rebuild it every frame).
fn medium_due(last: Option<(Vec3, f32)>, target: Vec3, now: f32) -> bool {
    let Some((scattering, at)) = last else {
        return true;
    };
    let change = ((target - scattering) / scattering.max(Vec3::splat(0.05)))
        .abs()
        .max_element();
    change >= 0.03 && now - at >= MEDIUM_INTERVAL
}

/// The scattering and tint the atmosphere's medium was last built for, and
/// when (real seconds).
#[derive(Resource, Default)]
pub struct SkyMedium {
    applied: Option<(Vec3, f32)>,
    tinted: Option<(Vec3, f32)>,
    /// A rebuild is wanted but waits for `MEDIUM_INTERVAL`.
    pending: bool,
}

impl SkyMedium {
    /// The medium lags behind the sky, waiting to be rebuilt.
    pub fn is_pending(&self) -> bool {
        self.pending
    }
}

/// Thins or thickens Bevy's Earth atmosphere like the game's palettes
/// scale its sky: a dim, clear night sky, a hazy glow round the setting sun;
/// the climate and the weather scale it further. Rebuilding the medium
/// redoes its lookup tables on the CPU and the GPU, so only now and then.
#[allow(clippy::too_many_arguments)]
fn scatter_like_the_game(
    real: Res<Time>,
    sky: Res<Sky>,
    environment: Res<Environment>,
    atmospheres: Query<&bevy::light::Atmosphere>,
    epoch: Option<Res<SceneEpoch>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    mut state: ResMut<SkyMedium>,
) {
    let state = state.bypass_change_detection();
    // A new scene builds its medium right away.
    if epoch.is_some_and(|epoch| epoch.is_changed()) {
        (state.applied, state.tinted) = (None, None);
    }
    let influence = &sky.scalars;
    let target = scattering(&environment, &sky.palette)
        * Vec3::new(influence.rayleigh, influence.mie, influence.mie_symmetrical);
    let target = target.with_z(target.z.clamp(0.0, 0.95));
    let tint = sky_tint(&sky.palette);
    let now = real.elapsed_secs();
    if !medium_due(state.applied, target, now) && !medium_due(state.tinted, tint, now) {
        // Due but for the interval: the medium lags behind.
        let later = f32::INFINITY;
        state.pending =
            medium_due(state.applied, target, later) || medium_due(state.tinted, tint, later);
        return;
    }
    let Ok(atmosphere) = atmospheres.single() else {
        return;
    };
    let Some(mut medium) = media.get_mut(&atmosphere.medium) else {
        return;
    };
    *medium = earth_scaled(target, tint);
    state.applied = Some((target, now));
    state.tinted = Some((tint, now));
    state.pending = false;
}

/// How the game lights its sky against how it lights the ground: the sky's
/// sun colour over the main light's (`SkySunColor` / `BgDifColor`), at the
/// same brightness: teal at night, cyan-green in the morning, bluish before
/// dawn. The atmosphere is lit by the main light, so its air scatters in
/// this tint instead.
fn sky_tint(palette: &EnvPalette) -> Vec3 {
    let ratio =
        Vec3::from(palette.sky_sun_color) / Vec3::from(palette.light_color).max(Vec3::splat(0.05));
    let brightness = ratio.dot(LUMINANCE).max(1e-3);
    (ratio / brightness).clamp(Vec3::splat(0.5), Vec3::splat(2.0))
}

/// How the game's sky differs from Bevy's Earth air per colour channel: its
/// Rayleigh scattering is cyan rather than deep blue (a fit to the game's
/// midday sky, hue ~220° against Earth's ~240°).
const CYAN_RAYLEIGH: Vec3 = Vec3::new(1.0, 1.12, 0.8);

/// Bevy's Earth medium with its Rayleigh scattering scaled by `x` (turned
/// cyan like the game's and tinted by `tint` per channel), its Mie
/// scattering by `y` and the Mie asymmetry set to `z`.
fn earth_scaled(scattering: Vec3, tint: Vec3) -> ScatteringMedium {
    let mut medium = ScatteringMedium::earth(256, 256);
    for term in medium.terms.iter_mut() {
        match term.phase {
            PhaseFunction::Rayleigh => term.scattering *= scattering.x * CYAN_RAYLEIGH * tint,
            PhaseFunction::Mie { .. } => {
                term.scattering *= scattering.y;
                term.phase = PhaseFunction::Mie {
                    asymmetry: scattering.z,
                };
            }
            _ => {}
        }
    }
    medium
}

/// How much the sun's disk may be lifted against the atmosphere's reddening
/// (the sun near the horizon still turns orange).
// SI-SKY-03: sun halo and disk drawn over the game's sky are ours.
const MAX_TRANSMITTANCE_BOOST: f32 = 3.0;

/// Roughly how much of the light from a direction with height `sin_elevation`
/// gets through Bevy's Earth atmosphere to the ground (per channel):
/// Rayleigh, Mie and ozone optical depths at the zenith times the air mass
/// (Kasten and Young).
fn air_transmittance(sin_elevation: f32) -> Vec3 {
    const ZENITH_DEPTH: Vec3 = Vec3::new(0.0615, 0.142, 0.271);
    let elevation = sin_elevation.clamp(0.0, 1.0).asin().to_degrees();
    let air_mass =
        1.0 / (elevation.to_radians().sin() + 0.50572 * (elevation + 6.07995).powf(-1.6364));
    (-ZENITH_DEPTH * air_mass).exp()
}

/// The part of the game's sun sprite that is the bright disk (the rest is
/// glow, which bloom draws).
const SUN_CORE: f32 = 0.3;

/// The sun's disk in the sky: its angular radius and its radiance in the
/// frame's units, like the atmosphere's disk that stood in for it
/// (the palette's light over the disk's solid angle, reddened by the air it
/// comes through, lifted by up to `MAX_TRANSMITTANCE_BOOST`). The game draws
/// a sprite (`SunParam`), whose look is not reproduced.
fn sun_disk(sky: &Sky, environment: &Environment, exposure: f32) -> (f32, Vec3) {
    let radius = environment.sun.angular_sizes().0 * SUN_CORE * 0.5;
    let solid_angle = std::f32::consts::PI * radius * radius;
    let (lux, color) = main_light(sky, environment);
    let air = (air_transmittance(sky.towards_sun.y) * MAX_TRANSMITTANCE_BOOST).min(Vec3::ONE);
    (
        radius,
        color * air * (lux * sky.sun_up * exposure / solid_angle),
    )
}

/// Stars, the moon and the sun, drawn additively on a dome around the
/// camera; the clouds, drawn over it, hide them by their own alpha, like the
/// game's (PS 449 blends over the sky).
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct SkyMaterial {
    #[uniform(0)]
    pub params: SkyParams,
}

/// See `sky.wgsl`.
#[derive(ShaderType, Clone, Debug, Default, PartialEq)]
pub struct SkyParams {
    /// World direction to star-map direction (the sky turns with the time).
    pub stars: Mat4,
    /// Direction towards the moon (xyz), its angular radius (w).
    pub moon: Vec4,
    /// Moon colour (rgb), phase as a fraction of the cycle, 0 full (w).
    pub moon_color: Vec4,
    /// How visible the night sky is, 0 by day to 1 at night (x), and how
    /// clear the sky is, 0 under overcast (y).
    pub night: Vec4,
    /// Direction towards the sun (xyz), its disk's angular radius (w).
    pub sun: Vec4,
    /// The disk's radiance in the frame's units (rgb; w is unused).
    pub sun_color: Vec4,
}

impl Material for SkyMaterial {
    fn fragment_shader() -> ShaderRef {
        SKY_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    /// Behind every other transparent thing (clouds).
    fn depth_bias(&self) -> f32 {
        -1.0e9
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Seen from inside.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Component)]
struct SkyDome;

fn spawn_sky(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let material = SkyMaterial {
        params: SkyParams::default(),
    };
    commands.spawn((
        Name::new("stars"),
        SkyDome,
        Mesh3d(meshes.add(Sphere::new(DOME_RADIUS).mesh().uv(48, 24))),
        MeshMaterial3d(materials.add(material)),
        Transform::default(),
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

/// How clear the sky is for the weather's tint, 1 in clear weather to 0
/// under the grey of cloud and rain (the game's `FeatureColor` 0.6 and less).
// SI-SKY-05: procedural stars and moon are ours.
fn clear_sky(weather: &Influence) -> f32 {
    let grey = Vec3::from(weather.feature_color).dot(Vec3::splat(1.0 / 3.0));
    smoothstep(0.6, 0.95, grey)
}

/// Keeps the dome on the camera and turns the sky with the time.
fn update_sky(
    time: Res<TimeOfDay>,
    sky: Res<Sky>,
    environment: Res<Environment>,
    cameras: Query<(&GlobalTransform, Option<&bevy::camera::Exposure>), crate::camera::MainCamera>,
    mut domes: Query<(&mut Transform, &MeshMaterial3d<SkyMaterial>), With<SkyDome>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    let (Ok((camera, exposure)), Ok((mut transform, material))) =
        (cameras.single(), domes.single_mut())
    else {
        return;
    };
    if transform.translation != camera.translation() {
        transform.translation = camera.translation();
    }
    // The stars wheel about the axis of the sun's circle, once a day.
    let axis = Vec3::X
        .cross(sun_direction(12.5, &environment.sun))
        .normalize();
    let turn = Quat::from_axis_angle(axis, time.hours / 24.0 * std::f32::consts::TAU);
    // The palette's sky "sun" colour is the moon's at night; keep it pale.
    let tint = Vec3::from(sky.palette.sky_sun_color);
    let moon_color = Vec3::ONE.lerp(tint / tint.max_element().max(1e-3), 0.35);
    let moon_size = environment.sun.angular_sizes().1;
    let exposure = exposure
        .copied()
        .unwrap_or(bevy::camera::Exposure { ev100: DAY_EV100 });
    let (sun_radius, sun_radiance) = sun_disk(&sky, &environment, exposure.exposure());
    let params = SkyParams {
        stars: Mat4::from_quat(turn.inverse()),
        moon: sky.towards_moon.extend(moon_size / 2.0),
        moon_color: moon_color.extend(time.moon_phase() as f32 / 8.0),
        night: Vec4::new(sky.night, clear_sky(&sky.weather), 0.0, 0.0),
        sun: sky.towards_sun.extend(sun_radius),
        sun_color: sun_radiance.extend(0.0),
    };
    // Only a changed material is re-extracted and re-bound.
    if materials
        .get(&material.0)
        .is_some_and(|m| m.params != params)
        && let Some(mut material) = materials.get_mut(&material.0)
    {
        material.params = params;
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::climate::SkyState;

    #[test]
    fn the_sun_follows_the_games_path() {
        let sun = field_sun();
        let rise = sun_direction(4.0, &sun);
        assert!(rise.x > 0.99 && rise.y.abs() < 1e-4, "{rise}");
        // In the north at 12:30, 42° up.
        let noon = sun_direction(12.5, &sun);
        let expected = Vec3::new(0.0, 1.0, -1.1).normalize();
        assert!(noon.distance(expected) < 1e-4, "{noon}");
        let set = sun_direction(21.0, &sun);
        assert!(set.x < -0.99 && set.y.abs() < 1e-4, "{set}");
        // Below the horizon at night, back up in time.
        assert!(sun_direction(0.5, &sun).y < -0.6);
        assert!(sun_direction(3.9, &sun).y < 0.0 && sun_direction(4.1, &sun).y > 0.0);
        // The game's built-in slope (no dump) leans it south instead.
        assert!(sun_direction(12.5, &SunParams::default()).z > 0.7);
    }

    #[test]
    fn the_moon_follows_the_night_light() {
        let midnight = moon_direction(0.5);
        assert!(midnight.x.abs() < 1e-4 && midnight.z < 0.0 && midnight.y > 0.6);
        assert!(moon_direction(22.0).x > 0.99 && moon_direction(3.0).x < -0.99);
        assert!(moon_direction(12.0).y < -0.5);
    }

    /// The field's sun parameters (`WorldMgr/normal.bwinfo` `SunParam`).
    fn field_sun() -> SunParams {
        SunParams {
            slope: -1.1,
            dir_y_stop: -42_000.0,
            ..SunParams::default()
        }
    }

    #[test]
    fn the_games_main_light_culminates_in_the_north() {
        let light = game_main_light(12.5, &field_sun());
        assert_eq!(light.change, 1.0);
        let expected = Vec3::new(0.0, 1.0, -1.1).normalize();
        assert!(light.towards.distance(expected) < 1e-4, "{}", light.towards);
        assert!(light.sky.distance(expected) < 1e-4, "{}", light.sky);
        // The night light's arc peaks about as high at 00:30.
        let midnight = game_main_light(0.5, &field_sun()).towards;
        assert!(midnight.x.abs() < 1e-4 && midnight.z < 0.0);
        assert!((midnight.y - expected.y).abs() < 0.01, "{midnight}");
        // Rising in the east, setting in the west, by day and by night.
        let towards = |hours: f32| game_main_light(hours, &field_sun()).towards;
        assert!(towards(5.0).x > 0.8);
        assert!(towards(20.5).x < -0.8);
        assert!(towards(22.5).x > 0.8);
        assert!(towards(2.5).x < -0.8);
    }

    #[test]
    fn the_games_main_light_never_shades_from_low() {
        // Cut at `SunDirYStop` (0.525 of the length): about 27° at the
        // lowest, where the sun's path leans north as it cuts.
        let lowest = (0..24 * 60)
            .map(|minute| {
                game_main_light(minute as f32 / 60.0, &field_sun())
                    .towards
                    .y
            })
            .fold(f32::MAX, f32::min);
        assert!(lowest > 0.45 && lowest < 0.47, "{lowest}");
        // The sky's light is not cut: it sinks to the horizon at sunset and
        // waits there until the night light rises.
        let sky = game_main_light(21.5, &field_sun()).sky;
        assert!(sky.x < -0.99 && sky.y.abs() < 1e-4, "{sky}");
        assert!(game_main_light(20.5, &field_sun()).sky.y < 0.1);
    }

    #[test]
    fn the_games_main_light_fades_while_it_switches() {
        let sun = field_sun();
        let mut switch = LightSwitch::default();
        // A frame at 30 fps, the clock at a game minute a second.
        let mut hours = 21.99;
        let mut frame = |switch: &mut LightSwitch| {
            hours += 1.0 / 60.0 / 30.0;
            switch.step(game_main_light(hours, &sun), hours, 1.0)
        };
        // Some 18 frames still before 22:00.
        for _ in 0..17 {
            assert_eq!(frame(&mut switch).change, 1.0);
        }
        let mut light = frame(&mut switch);
        while light.change == 1.0 {
            light = frame(&mut switch);
        }
        // Fading out where it was (the sun, in the west), 0.0065 a frame…
        for _ in 0..76 {
            light = frame(&mut switch);
        }
        assert!((light.change - 0.5).abs() < 0.01, "{}", light.change);
        assert!(light.towards.x < -0.8);
        // …then back in from the night light, in the east, 154 frames later.
        for _ in 0..154 {
            light = frame(&mut switch);
        }
        assert!((light.change - 0.5).abs() < 0.02, "{}", light.change);
        assert!(light.towards.x > 0.8);
        for _ in 0..80 {
            light = frame(&mut switch);
        }
        assert_eq!(light.change, 1.0);
        // A scene that starts at night starts lit.
        let mut fresh = LightSwitch::default();
        assert_eq!(
            fresh.step(game_main_light(22.02, &sun), 22.02, 1.0).change,
            1.0
        );
    }

    #[test]
    fn low_light_is_redder() {
        let zenith = air_transmittance(1.0);
        assert!(zenith.x > 0.9 && zenith.z > 0.7 && zenith.z < zenith.x);
        let horizon = air_transmittance(0.0);
        assert!(horizon.x < 0.2 && horizon.z < 1e-3);
    }

    #[test]
    fn the_land_is_lit_by_the_games_light_the_sky_by_its_uncut_light() {
        let environment = Environment::fallback();
        let sky = |hours: f32| {
            environment.sky(
                &TimeOfDay {
                    hours,
                    day: 0,
                    speed: 0.0,
                },
                &Climate::default(),
                &Weather::default(),
            )
        };
        let noon = sky(12.5);
        assert_eq!(
            (noon.sun_up, noon.night, noon.light_change),
            (1.0, 0.0, 1.0)
        );
        assert!(noon.towards_light.distance(noon.towards_sun) < 1e-5);
        assert!(noon.main_light.distance(noon.towards_sun) < 1e-5);
        // Just after sunset the sky's light waits on the horizon in the
        // west, the land is still lit from higher up there.
        let dusk = sky(21.5);
        assert!(dusk.sun_up < 0.01 && dusk.night > 0.5);
        assert!(dusk.towards_light.x < -0.99 && dusk.towards_light.y.abs() < 1e-4);
        assert!(dusk.main_light.x < -0.7 && dusk.main_light.y > 0.5);
        // At night both follow the night light, where the moon is.
        let midnight = sky(0.5);
        assert!(midnight.night > 0.99);
        assert!(midnight.towards_light.distance(midnight.towards_moon) < 1e-5);
        assert!(midnight.main_light.z < 0.0 && midnight.main_light.y > 0.6);
    }

    #[test]
    fn scattering_is_relative_to_midday() {
        let environment = Environment::fallback();
        let at = |hours: f32| scattering(&environment, &environment.at(hours));
        assert!(at(12.0).distance(Vec3::new(1.0, 1.0, 0.8)) < 1e-5);
        assert!(at(18.9).y > 3.0 && at(0.0).x < 0.5);
        let medium = earth_scaled(Vec3::new(0.5, 2.0, 0.7), Vec3::ONE);
        let earth = ScatteringMedium::earth(256, 256);
        assert_eq!(
            medium.terms[0].scattering,
            earth.terms[0].scattering * 0.5 * CYAN_RAYLEIGH
        );
        assert_eq!(medium.terms[1].scattering, earth.terms[1].scattering * 2.0);
        // The night sky scatters teal: its sky sun is cyan against a blue moonlight.
        let night = sky_tint(&EnvPalette {
            sky_sun_color: [0.35, 1.0, 1.0],
            light_color: [0.425, 0.8, 1.0],
            ..EnvPalette::default()
        });
        assert!(night.y > night.z && night.z > night.x);
        assert!((night.dot(LUMINANCE) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn rows_of_palette_sets_blend_their_palettes() {
        let mut environment = Environment::fallback();
        // Palette set 3 (row 1 of the game's table): everything twice as bright.
        let brighter = environment.palettes.clone().map(|p| EnvPalette {
            light_intensity: p.light_intensity * 2.0,
            ..p
        });
        environment
            .sets
            .extend([environment.palettes.clone(), brighter]);
        let desert = ClimateDefines {
            palette_set: 1,
            ..ClimateDefines::default()
        };
        environment.climates = vec![ClimateDefines::default(), desert];
        // Halfway from the field's row to row 1.
        let rows = PaletteRows::between(0, 1, 0.5);
        let palette = environment.at_rows(12.0, &rows, 0.0);
        let plain = environment.at(12.0);
        assert!((palette.light_intensity - plain.light_intensity * 1.5).abs() < 1e-4);
        // A set without light falls back to the ordinary one.
        environment.sets[3] = environment.palettes.clone().map(|p| EnvPalette {
            light_intensity: 0.0,
            ..p
        });
        assert_eq!(environment.at_rows(12.0, &rows, 0.0), plain);
        // Without the sets everything is the plain palette.
        assert_eq!(Environment::fallback().at_rows(12.0, &rows, 0.0), plain);
    }

    #[test]
    fn an_overcast_sky_takes_the_climates_overcast_palettes() {
        let mut environment = Environment::fallback();
        // The field's overcast set 1: half the light, greener.
        environment.sets[1] = environment.palettes.clone().map(|p| EnvPalette {
            light_intensity: p.light_intensity * 0.5,
            light_color: [0.85, 1.0, 0.78],
            ..p
        });
        environment.climates = vec![ClimateDefines::default()];
        let noon = |weather: &Weather| {
            environment.sky(
                &TimeOfDay {
                    hours: 12.0,
                    day: 0,
                    speed: 0.0,
                },
                &Climate::default(),
                weather,
            )
        };
        let clear = noon(&Weather::default());
        assert_eq!(clear.palette, environment.at(12.0));
        // Rain: the overcast set; the sky halfway to overcast: halfway.
        let mut weather = Weather::settled(2);
        let rain = noon(&weather);
        assert!((rain.palette.light_intensity - clear.palette.light_intensity * 0.5).abs() < 1e-4);
        assert_eq!(rain.palette.light_color, [0.85, 1.0, 0.78]);
        weather.sky = SkyState {
            from: SKY_CLEAR,
            to: SKY_OVERCAST,
            transition: 0.5,
        };
        let showers = noon(&weather);
        assert!(
            (showers.palette.light_intensity - clear.palette.light_intensity * 0.75).abs() < 1e-4
        );
        // Rain in sunshine keeps the clear sky, like the game's.
        assert_eq!(noon(&Weather::settled(8)).palette, clear.palette);
        // A row with one set for every sky keeps it (row 1: set 3).
        let overcast = environment.sets[1].clone();
        environment.sets.extend([overcast.clone(), overcast]);
        let woods = PaletteRows::settled(1);
        let before = environment.at_rows(12.0, &woods, 0.0);
        assert_eq!(environment.at_rows(12.0, &woods, 1.0), before);
    }

    #[test]
    fn the_fog_takes_the_moisture_in_the_fields_palette_sets_only() {
        let mut environment = Environment::fallback();
        let woods_set = environment.sets[0].clone().map(|p| EnvPalette {
            fog_color: [0.2, 0.3, 0.2, 0.4],
            attenuation_sky: 2.0,
            ..p
        });
        environment.sets.resize(11, woods_set);
        // The field (row 0) and the Dark Woods (row 7: set 10).
        environment.climates = vec![
            ClimateDefines::default(),
            ClimateDefines {
                palette_set: 7,
                ..ClimateDefines::default()
            },
        ];
        let field = environment.moist_fog(12.0, &PaletteRows::default(), 0.5);
        assert_eq!(
            field,
            MoistFog {
                moist: 1.0,
                alpha: 0.0,
                moist_sky: 1.0,
                attenuation_sky: 0.0,
            }
        );
        // Halfway from the field's row to the Dark Woods'.
        let edge = environment.moist_fog(12.0, &PaletteRows::between(0, 7, 0.5), 0.0);
        assert!((edge.moist - 0.5).abs() < 1e-6 && (edge.alpha - 0.2).abs() < 1e-6);
        assert!((edge.attenuation_sky - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_medium_is_rebuilt_only_now_and_then() {
        let base = Vec3::new(1.0, 2.0, 0.8);
        assert!(medium_due(None, base, 0.0));
        // Small changes never, real changes at most twice a second.
        assert!(!medium_due(Some((base, 0.0)), base * 1.01, 10.0));
        assert!(!medium_due(Some((base, 0.0)), base * 1.5, 0.2));
        assert!(medium_due(Some((base, 0.0)), base * 1.5, 0.6));
    }

    #[test]
    fn overcast_flattens_the_light() {
        // The hand-picked overcast set has half the light, like the field's.
        let environment = Environment::fallback();
        let mut weather = Weather::default();
        let at_noon = |weather: &Weather| {
            environment.sky(
                &TimeOfDay {
                    hours: 12.0,
                    day: 0,
                    speed: 0.0,
                },
                &Climate::default(),
                weather,
            )
        };
        let clear = at_noon(&weather);
        assert_eq!(weather.overcast_sky(), 0.0);
        weather = Weather::settled(2);
        let rain = at_noon(&weather);
        assert_eq!(weather.overcast_sky(), 1.0);
        // The overcast palette halves the sun, the rain's grey `FeatureColor`
        // dims its colour further.
        let sun = |sky: &Sky| {
            let (lux, color) = main_light(sky, &environment);
            lux * color.dot(LUMINANCE)
        };
        let (clear_sun, rain_sun) = (sun(&clear), sun(&rain));
        let grey = Vec3::from(environment.weather_influences[2].feature_color);
        let ratio = (Vec3::from(rain.palette.light_color) * grey).dot(LUMINANCE)
            / Vec3::from(clear.palette.light_color).dot(LUMINANCE);
        assert!((rain_sun / clear_sun - 0.5 * ratio).abs() < 1e-3);
    }

    #[test]
    fn the_climate_and_the_weather_tint_the_light_per_channel() {
        let mut environment = Environment::fallback();
        let tinted = |feature_color: [f32; 3], palette_set: usize| ClimateDefines {
            influence: Influence {
                feature_color,
                ..Influence::default()
            },
            palette_set,
            ..ClimateDefines::default()
        };
        // A field climate (row 0) and one of the woods (another row).
        environment.climates = vec![tinted([1.25, 1.31, 1.09], 0), tinted([0.5; 3], 7)];
        let noon = TimeOfDay {
            hours: 12.0,
            day: 0,
            speed: 0.0,
        };
        let mut weather = Weather::default();
        let desert = environment.sky(&noon, &Climate::default(), &weather);
        let light = Vec3::from(desert.palette.light_color);
        let (lux, color) = main_light(&desert, &environment);
        assert_eq!(
            lux,
            desert.palette.light_intensity * environment.lux_per_intensity()
        );
        assert!((color - light * Vec3::new(1.25, 1.31, 1.09)).length() < 1e-5);
        // Rain's `FeatureColor` on top of the climate's.
        weather = Weather::settled(2);
        let rain = environment.sky(&noon, &Climate::default(), &weather);
        let grey = Vec3::from(environment.weather_influences[2].feature_color);
        let (_, color) = main_light(&rain, &environment);
        let expected = Vec3::from(rain.palette.light_color) * Vec3::new(1.25, 1.31, 1.09) * grey;
        assert!((color - expected).length() < 1e-5);
        // The fogs take the weather's fog colour in place of its light colour.
        let fog = Vec3::from(environment.weather_influences[2].feature_fog_color);
        assert!((rain.fog_feature - Vec3::new(1.25, 1.31, 1.09) * fog).length() < 1e-5);
        // The woods' row takes no tint, not even the weather's.
        let woods = environment.sky(&noon, &Climate::settled(1, 7), &weather);
        assert_eq!(woods.light_feature, Vec3::ONE);
        assert_eq!(woods.fog_feature, Vec3::ONE);
        // A change of climate eases the climate's colour over; halfway from
        // the woods' row, half the field's tint is in.
        let mut changing = Climate::settled(0, 0);
        (changing.previous, changing.transition) = (1, 0.25);
        let (_, color) = main_light(&environment.sky(&noon, &changing, &weather), &environment);
        let climate = Vec3::splat(0.5).lerp(Vec3::new(1.25, 1.31, 1.09), 0.25);
        let expected = Vec3::from(rain.palette.light_color) * climate * grey;
        assert!((color - expected).length() < 1e-5);
        changing = Climate {
            rows: PaletteRows::between(7, 0, 0.5),
            ..Climate::default()
        };
        let edge = environment.sky(&noon, &changing, &weather);
        let tint = Vec3::new(1.25, 1.31, 1.09) * grey;
        assert!((edge.light_feature - (tint + Vec3::ONE) / 2.0).length() < 1e-5);
    }

    #[test]
    fn the_scattering_and_bloom_factors_apply_on_the_fields_row_only() {
        let mut environment = Environment::fallback();
        let hazy = |palette_set: usize| ClimateDefines {
            influence: Influence {
                rayleigh: 0.5,
                mie: 3.0,
                scatter_near: -50.0,
                ..Influence::default()
            },
            palette_set,
            ..ClimateDefines::default()
        };
        environment.climates = vec![hazy(0), hazy(7)];
        let rain = environment.weather_influences[2];
        let noon = TimeOfDay {
            hours: 12.0,
            day: 0,
            speed: 0.0,
        };
        // Rain has blown in: `TempMgr` has reached its bloom entry.
        let weather = Weather::settled(2);
        // The field's row: the climate's factors, then the weather's.
        let field = environment.sky(&noon, &Climate::default(), &weather);
        assert_eq!(field.scalars.mie, 3.0 * rain.mie);
        assert_eq!(field.scalars.rayleigh, 0.5 * rain.rayleigh);
        assert_eq!(field.scalars.scatter_near, -50.0 + rain.scatter_near);
        assert_eq!(field.scalars.bloom_threshold, rain.bloom_threshold);
        // The woods' row: the palette as it is, whatever the weather.
        let woods = environment.sky(&noon, &Climate::settled(1, 7), &weather);
        assert_eq!(woods.scalars, Influence::default());
        // Halfway between the rows: the mean of the two.
        let edge = Climate {
            rows: PaletteRows::between(0, 7, 0.5),
            ..Climate::settled(1, 7)
        };
        let edge = environment.sky(&noon, &edge, &weather);
        assert!((edge.scalars.mie - (3.0 * rain.mie + 1.0) / 2.0).abs() < 1e-6);
    }

    #[test]
    fn days_roll_over() {
        let mut time = TimeOfDay {
            hours: 23.5,
            day: 0,
            speed: TimeOfDay::GAME_SPEED,
        };
        time.advance(1.0);
        assert_eq!(time.day, 1);
        assert!((time.hours - 0.5).abs() < 1e-5);
        assert!(time.is_night());
        assert_eq!(time.clock(), "00:30");
        assert_eq!(
            TimeOfDay {
                hours: 18.999,
                ..time
            }
            .clock(),
            "18:59"
        );
    }
}
