//! The game's air perspective: the distance dissolves in a haze of the
//! palette's fog colour (`FogColor`: turquoise by day, peach at dawn, olive at
//! sunset, teal at night), tinted by the climate (`FeatureColor`) and the
//! weather (`FeatureFogColor`), thickening with distance by the palette's
//! scattering fog (`SfParam_*`).
//!
//! How much of the distance the haze takes is the game's scattering fog
//! (its pre-shading pass, docs/research/wiiu-deferred-shading.md): over the
//! view depth z, `density · (1 − (1 − t)^attenuation)` with
//! `t = (z − near)/(far − near)`, from the palette's `SfParam_near` and
//! `SfParam_attenuation` (the climate adds its offsets) and the static
//! `SfParam_far` and `SfParam_density`; the fog's own colour comes in only
//! past a few hundred metres (`apply_haze`). Bad weather thickens it only
//! through its palettes: like the game, an overcast sky blends in the
//! climate's overcast palette set (`daynight.rs`).
//!
//! Its colour is the game's where the dump has the sky's table: like the
//! game, the fog reads it from the sky table baked from the atmosphere
//! (`sky_lut.rs`, `apply_haze`), in the game's units like the frame
//! (`daynight.rs`, `lux_per_intensity`). The sky itself is then the game's too: the
//! dome (`sky_haze.wgsl`) draws the same table like the game's
//! `sky_postfx_sky` with its ad hoc fog ([`sky_dome`]), so the sky and the
//! distance share one source, and the same ad hoc fog, as strong as the air
//! is moist, veils the surfaces past the palette's `FogStart`
//! ([`SkyDome::surface_texels`]): the game's light veil in the rain. Without the table, the palette's fog colour
//! in the light, and distant land fades into a darker, bluer colour than
//! the sky at the horizon ([`DISTANT_LAND`]), fits to the game's
//! screenshots (`docs/STYLE.md`, track 2), and the dome lays a fitted haze
//! over the atmosphere's sky.
//!
//! Two ways to lay it over the surfaces, picked by [`HAZE_IN_SHADERS`]:
//! Bevy's `DistanceFog` on the camera, which every material that calls
//! `main_pass_post_lighting_processing` applies (an exponential stand-in);
//! or `apply_haze` of `botw::look`, the game's shape, in the materials
//! that call it (the values go through the [`Look`] texture). While the camera has the `DistanceFog`, `apply_haze`
//! leaves colours alone, so nothing is hazed twice.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::Exposure;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver, light_consts};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{DistanceFog, FogFalloff, MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

use asset_format::light::GreyImage as GreyTexture;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::climate::{Climate, Weather};
use crate::daynight::{DAY_EV100, Environment, Sky, TimeOfDay};
use crate::look::{LOOK_NOISE, Look, LookNoise, LookSkyLut, LookSystems, LookTexture};

/// Whether the surfaces' own shaders lay the haze (`apply_haze`, the
/// game's shape) instead of Bevy's `DistanceFog`. Every surface shader
/// calls `apply_haze` (terrain, objects, grass, far trees, water, lava,
/// characters, shaded clouds); only the placeholder models and particles
/// go without.
const HAZE_IN_SHADERS: bool = true;

const SKY_HAZE_SHADER: Handle<Shader> = uuid_handle!("c4a8e2d1-7b3f-4e95-a061-5d2f9b8c3e47");

/// Radius of the sky haze's dome: inside the stars' (25 km) and the camera's
/// far plane (30 km).
const DOME_RADIUS: f32 = 24_000.0;

pub struct FogPlugin {
    /// The `assets/` folder to read the fog's height noise from
    /// (`cloud_noise`).
    pub assets: std::path::PathBuf,
}

impl Plugin for FogPlugin {
    fn build(&self, app: &mut App) {
        let assets = self.assets.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { load_noise(&assets) });
        app.insert_resource(NoiseLoading(Some(task)))
            .add_systems(Update, finish_noise);
        load_internal_asset!(app, SKY_HAZE_SHADER, "sky_haze.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<SkyHazeMaterial>::default())
            .init_resource::<Moisture>()
            .add_systems(Startup, spawn_sky_haze)
            .add_systems(Update, update_moisture)
            .add_systems(
                PostUpdate,
                update_haze
                    .before(TransformSystems::Propagate)
                    .before(LookSystems::Upload),
            );
    }
}

/// The haze right now, in the renderer's units.
#[derive(Clone, Debug, PartialEq)]
pub struct Haze {
    /// Colour the sky fades into towards the horizon, after exposure (linear).
    pub color: Vec3,
    /// Colour distant land fades into, after exposure (linear).
    pub land: Vec3,
    /// How much of `color` the far distance shows (0–1).
    pub max: f32,
    /// View depth where the fog starts and where it is whole, in metres
    /// (negative near: already thick at the camera), and the exponent it
    /// thickens by between them.
    pub near: f32,
    pub far: f32,
    pub attenuation: f32,
    /// Glow of the haze towards the sun: colour per unit of the light
    /// reaching the eye (Bevy multiplies it by the light's colour and
    /// exposure), and how tightly it gathers round the sun.
    pub glow: Vec3,
    pub glow_exponent: f32,
    /// The main light as it reaches the eye after exposure (linear).
    pub light: Vec3,
    /// The height fog (the game's `fog_scatter`): the palette's `FogColor`
    /// after exposure, and the depth below the camera where it starts and
    /// is whole (`FogStart`, `FogEnd`, metres).
    pub height_color: Vec3,
    pub height_start: f32,
    pub height_end: f32,
    /// How fast the haze in the sky thins upwards, per unit of the sine of
    /// the elevation.
    pub sky_fade: f32,
    /// Towards the light the haze glows round.
    pub towards_light: Vec3,
    /// The sun's glow in the sky at its centre, after exposure (`sky_haze.wgsl`).
    pub halo: Vec3,
    /// The band of glow along the horizon under a low sun, below the sun,
    /// after exposure (`sky_haze.wgsl`).
    pub band: Vec3,
    /// Brightness of the game's sky table after exposure (`sky_lut.rs`), and
    /// how fast the fog's colour sinks from the zenith to the horizon with
    /// depth (`SfParam_horizontal`, `horz` in `apply_haze`).
    pub sky_table: f32,
    pub horizontal: f32,
}

impl Haze {
    /// How fast the fog thickens near the camera, per metre: `(1 − t)^a`
    /// falls like `e^(−a·t)` (for Bevy's exponential `DistanceFog`).
    pub fn density(&self) -> f32 {
        self.attenuation / (self.far - self.near).max(1.0)
    }

    /// The air section of the look texture (`Look::haze`, read by
    /// `apply_haze` in `look.wgsl`): distances in km, so they keep their
    /// digits in half floats.
    pub fn texels(&self) -> [Vec4; 4] {
        [
            self.land.extend(self.max),
            Vec4::new(
                1000.0 / (self.far - self.near).max(1.0),
                self.near / 1000.0,
                self.attenuation,
                0.0,
            ),
            (self.glow * self.light).extend(self.glow_exponent),
            self.towards_light.extend(0.0),
        ]
    }
}

/// How fast the sky's haze thins upwards (per unit of the sine of the
/// elevation) under a clear sky with the sun high, and otherwise: by day the
/// blue air shows above a pale band (half gone 10 degrees up); at dawn, dusk
/// and night and under clouds the game's sky is the fog's colour nearly all
/// the way up (fit to the game's screenshots).
const SKY_HAZE_FADE: (f32, f32) = (4.0, 1.0);

/// Brightness of the fog colour after exposure per lux of the main light
/// and unit of exposure, without the sky table (a fallback, not the game):
/// fitted so that the midday haze matches the game's horizon and far
/// mountains (`#a6bfc4`, R/443) under Bevy's tone mapping; brighter, the
/// distance turned a white sheet.
const FOG_PER_LUX: f32 = 0.08;

/// How much brighter the haze is at night: the game's night distance glows
/// teal over dark land, its sky a clear teal (≈ #2c4848, R/507). A fit.
const NIGHT_HAZE: f32 = 2.2;

/// What a clear day's air adds to the palette's fog colour: the game's
/// midday `FogColor` is mint, its distance and horizon light cyan (fit).
const CLEAR_AIR: Vec3 = Vec3::new(0.7, 0.95, 1.4);

/// How much of a clear day's air (`CLEAR_AIR`) a climate keeps, as a power
/// of its Rayleigh over its Mie scattering (`CalcRayleigh` / `CalcMie`, at
/// most 1): the game's desert (0.75 / 2) keeps a mint haze (`#a9d4b7`,
/// R/230: its palette's `FogColor` in the climate's tint), the central plain
/// (1 / 1) a cyan one. A fit.
const AIR_FOLLOWS_SCATTERING: f32 = 2.0;

/// The sun's glow in the sky next to its disk, per unit of the light
/// reaching the eye, at midday's haze (fit to the game's glare).
// SI-SKY-03: sun halo and disk drawn over the game's sky are ours.
const HALO: f32 = 0.1;

/// Glow towards the sun at midday's Mie scattering (fit).
const GLOW: f32 = 0.025;
/// How the glow grows with the palette's Mie scattering relative to
/// midday's (`SkyRParam_mie_amplifier` 12 at noon, 128 at sunset), as a
/// power: by its square root the dusk sky round the sun went a pale salmon
/// beige, where the game's is a dim olive over an orange band (R/719). A fit.
const GLOW_FOLLOWS_MIE: f32 = 0.1;

/// The colour distant land fades into, relative to the haze low in the sky,
/// in clear weather: the game's far mountains and Castle are a slate
/// blue (`#7da7c5`, L* 66) under a sky that is nearly white at the horizon
/// (`#cce4e2`, L* 89; R/443, R/304), where ours faded into the pale horizon
/// colour and went milky. Under clouds and rain the veil matches its sky.
/// A fit.
const DISTANT_LAND: Vec3 = Vec3::new(0.3, 0.42, 0.75);

/// How bright the band along the horizon is while the sun is low, times the
/// brightness of the glow at the sun, in the clouds' backlight colour: the
/// game's sunsets and dawns have a strong orange band across the horizon
/// under a dim sky (R/719, R/041, R/718: L* ≈ 63, C ≈ 62 under an olive sky
/// at L* ≈ 50; a fit).
const SUNSET_BAND: f32 = 30.0;

/// How much darker the haze is while the sun is low in clear weather: the
/// game's dusk sky above the band is a dim olive (L* ≈ 48–51, R/719) and
/// the distance under it a dark teal (≈ 41), ours a pale beige veil
/// (≈ 70). A fit.
const DUSK_HAZE_DIM: f32 = 0.5;
/// Power on the band's colour: deeper than the clouds' backlight, or the
/// tone curve bleaches it to cream (a fit: at 3 it still came out beige,
/// C ≈ 24 against the game's ≈ 62).
const BAND_SATURATION: f32 = 5.0;

/// The haze for the sky and palette `sky` seen with a camera at exposure `ev100`.
pub fn haze(environment: &Environment, sky: &Sky, ev100: f32) -> Haze {
    let palette = &sky.palette;
    let influence = &sky.scalars;
    let static_ = &environment.palette_static;
    // The main light's illuminance, like `daynight::move_lights` sets it.
    let brightest = environment
        .palettes
        .iter()
        .map(|p| p.light_intensity)
        .fold(1e-3, f32::max);
    let lux = palette.light_intensity / brightest * light_consts::lux::RAW_SUNLIGHT;
    let exposure = Exposure { ev100 }.exposure();
    // The game tints its fogs by the climate's `FeatureColor` and the
    // weather's `FeatureFogColor` on the field's row (`Sky::fog_feature`);
    // the haze also dims by how grey the weather makes the scene (a fit).
    let weather = Vec3::from(sky.weather.feature_color)
        .dot(Vec3::splat(1.0 / 3.0))
        .clamp(0.05, 1.0);
    let tint = sky.fog_feature * weather;
    // Blue air only under a clear sky with the sun well up.
    let clear = smoothstep(0.6, 1.0, weather) * smoothstep(0.05, 0.35, sky.towards_sun.y);
    let blue_air = (influence.rayleigh / influence.mie.max(0.05))
        .min(1.0)
        .powf(AIR_FOLLOWS_SCATTERING);
    let air = Vec3::ONE.lerp(CLEAR_AIR, clear * blue_air);
    let dusk = smoothstep(0.6, 1.0, weather)
        * (1.0 - smoothstep(0.05, 0.35, sky.towards_sun.y))
        * (1.0 - sky.night);
    // The height fog is `fog_scatter`: the palette's `FogColor` in the
    // game's fog factor; without the sky table its brightness in lux is a
    // fit (with the table `update_haze` takes the game's, like B's).
    let height_color =
        Vec3::from_slice(&palette.fog_color) * sky.fog_feature * lux * exposure * FOG_PER_LUX;
    let color = Vec3::from_slice(&palette.fog_color)
        * tint
        * air
        * lux
        * exposure
        * FOG_PER_LUX
        * (1.0 + (NIGHT_HAZE - 1.0) * sky.night)
        * (1.0 - DUSK_HAZE_DIM * dusk);
    // The glow round the sun follows the palette's haze (Mie) and its forward
    // scattering, in the colour of the sun in the sky.
    let noon_mie = environment
        .palettes
        .iter()
        .max_by(|a, b| a.light_intensity.total_cmp(&b.light_intensity))
        .map_or(12.0, |p| p.mie_amplifier);
    let mie = (palette.mie_amplifier * influence.mie / noon_mie.max(1e-3)).max(0.0);
    let sun = Vec3::from(palette.sky_sun_color);
    let glow = sun / sun.max_element().max(1e-3) * GLOW * mie.powf(GLOW_FOLLOWS_MIE);
    let asymmetry = (palette.mie_asymmetry * influence.mie_symmetrical).clamp(0.0, 0.95);
    let light = Vec3::from(palette.light_color) * sky.light_feature * lux * exposure;
    // The band along a low sun's horizon glows in the colour of the clouds'
    // backlight (`Cloud0_ColorBackLight`: orange at sunset).
    let backlight = Vec3::from(palette.cloud_backlight).max(Vec3::splat(1e-3));
    // The climate offsets the palette's scattering fog (`CalcSfParam*`).
    let near = palette.scatter_near + influence.scatter_near;
    let far = static_.scatter_far.max(near + 1.0);
    let attenuation = (palette.scatter_attenuation + influence.scatter_attenuation).max(0.01);
    Haze {
        color,
        land: color * Vec3::ONE.lerp(DISTANT_LAND, smoothstep(0.6, 1.0, weather)),
        max: static_.scatter_density.clamp(0.0, 1.0),
        near,
        far,
        attenuation,
        glow,
        glow_exponent: 2.0 / (1.0 - asymmetry),
        light,
        height_color,
        height_start: palette.fog_start,
        height_end: palette.fog_end.max(palette.fog_start + 1.0),
        // `SkyIsotropicfade` (1 before dawn and at dusk) evens the sky out
        // into the haze all the way up.
        sky_fade: (SKY_HAZE_FADE.1 + (SKY_HAZE_FADE.0 - SKY_HAZE_FADE.1) * clear)
            * (1.0 - palette.sky_isotropic_fade.clamp(0.0, 1.0)),
        towards_light: sky.towards_light,
        // Larger in thicker haze, only while the sun is up.
        // SI-SKY-03: sun halo and disk drawn over the game's sky are ours.
        halo: sun / sun.max_element().max(1e-3)
            * light.dot(Vec3::new(0.2126, 0.7152, 0.0722))
            * HALO
            * mie.sqrt().min(3.0)
            * sky.sun_up,
        // While the sun is low, until a while after it has set; only in
        // clear weather.
        // The table is in the game's units, like the frame at the day's
        // exposure.
        sky_table: exposure / Exposure { ev100: DAY_EV100 }.exposure(),
        horizontal: palette.scatter_horizontal.max(0.0),
        band: (backlight / backlight.max_element().max(1e-3)).powf(BAND_SATURATION)
            * (glow * light).dot(Vec3::new(0.2126, 0.7152, 0.0722))
            * SUNSET_BAND
            * smoothstep(0.4, 0.05, sky.towards_sun.y)
            * smoothstep(-0.25, -0.02, sky.towards_sun.y)
            * smoothstep(0.6, 1.0, weather),
    }
}

/// The ad hoc fog's share at the zenith (`adhoc_fog_atten_minscale_sky`):
/// the game's weather update writes 0.3 every frame
/// (`ENV_UpdateWeatherPalettes` `0x036425b8`, Wii U v208).
const ADHOC_SKY_MIN_SCALE: f32 = 0.3;

/// How fast the ad hoc fog thins upwards in the sky per unit of moisture
/// (0–1): in palette sets 0 and 1 the weather update replaces the
/// palettes' `afParam_attenuationForSky` by moisture/100 · 4.8 (constant
/// `0x1030023c`); the sky keeps at least 0.5 (`SKY_SetAdhocFogParams`
/// `0x033f7bd8`).
const ADHOC_SKY_ATTENUATION_PER_MOISTURE: f32 = 4.8;
const ADHOC_SKY_MIN_ATTENUATION: f32 = 0.5;

/// The strength of the game's `fog_scatter` (its colour's alpha: the height
/// fog's and the ad hoc fog's) with the air `moisture` moist (0–1,
/// [`Moisture::fog`]): the moisture in the field's palette sets, the
/// palettes' `FogColor` alpha in the others, blended
/// ([`crate::daynight::MoistFog`]).
fn fog_strength(sky: &Sky, moisture: f32) -> f32 {
    let fog = &sky.moist_fog;
    (moisture * fog.moist + fog.alpha).max(0.0)
}

/// The ad hoc fog's `atten_sky`: the moisture × 4.8 in palette sets 0 and 1,
/// the palettes' `afParam_attenuationForSky` in the others, blended; at
/// least 0.5.
fn adhoc_sky_attenuation(sky: &Sky, moisture: f32) -> f32 {
    let fog = &sky.moist_fog;
    (moisture * ADHOC_SKY_ATTENUATION_PER_MOISTURE * fog.moist_sky + fog.attenuation_sky)
        .max(ADHOC_SKY_MIN_ATTENUATION)
}

/// The game's sky dome (`sky_postfx_sky` of `uking_pass_shader`, variants
/// 8 and 12, PS 435 and 443; drawn by `SKY_DrawPostfxSkyDome` `0x033f6a08`
/// with `cAmplifierAdhoc` 1 in the main view; docs/research/wiiu-sky-resources.md):
/// the sky table in the view's direction, the ground's colour below the
/// horizon, and an "ad hoc" fog of the palette's fog colour over it all,
/// as strong as the air is moist at the horizon and fading towards
/// [`ADHOC_SKY_MIN_SCALE`] at the zenith. Colours in the game's units (the
/// sky table's; `Haze::sky_table` brings them to the screen's).
#[derive(Clone, Debug, PartialEq)]
pub struct SkyDome {
    /// `cGroundColor`: the ground's colour (bksky `ground_color`, rgb) and
    /// 1 − its alpha, how much of it shows above the horizon too (a).
    pub ground: Vec4,
    /// `cNormalFogColor`: `fog_scatter`'s colour, the palette's `FogColor`
    /// in the game's fog factor ([`Sky::fog_feature`]).
    pub fog_color: Vec3,
    /// `cNormalFogCoeff`: `adhoc_fog_atten_grd` (not used by the sky), how
    /// fast the fog thins upwards, its share at the zenith and its strength
    /// at the horizon (0: no fog, the game's variant 8).
    pub fog: Vec4,
}

/// The sky dome for the sky `sky` with the air `moisture` moist (0–1,
/// [`Moisture::fog`]). Like the game: the fog's strength is the
/// `fog_scatter` alpha ([`fog_strength`]: the moisture in the field's
/// palette sets), its thinning upwards `atten_sky`
/// ([`adhoc_sky_attenuation`]: growing with the moisture in sets 0 and 1).
pub fn sky_dome(environment: &Environment, sky: &Sky, moisture: f32) -> SkyDome {
    let ground = environment.renderer.sky.ground_color;
    let strength = fog_strength(sky, moisture);
    let fog = if strength > 0.0 {
        Vec4::new(
            sky.palette.attenuation_ground.max(0.0),
            adhoc_sky_attenuation(sky, moisture),
            ADHOC_SKY_MIN_SCALE,
            strength,
        )
    } else {
        Vec4::new(0.0, 0.0, 1.0, 0.0)
    };
    SkyDome {
        ground: Vec4::new(
            ground[0],
            ground[1],
            ground[2],
            1.0 - ground[3].clamp(0.0, 1.0),
        ),
        fog_color: Vec3::from_slice(&sky.palette.fog_color[..3]) * sky.fog_feature,
        fog,
    }
}

impl SkyDome {
    /// The same ad hoc fog over the surfaces (B of the game's PS 140, the
    /// look's texels 16–19, `apply_haze`): its colour at the sky table's
    /// brightness `sky_table` (its strength 0 without the table: the game
    /// turns it off then too), between `start` and `end` metres of view
    /// depth (the palette's `FogStart`, `FogEnd`: the game's
    /// `adhoc_fog_near`/`_far` are `fog_scatter`'s), thickening by
    /// `adhoc_fog_atten_grd` and thinner looking up.
    pub fn surface_texels(&self, sky_table: Option<f32>, start: f32, end: f32) -> [Vec4; 4] {
        let Some(brightness) = sky_table else {
            return [Vec4::ZERO; 4];
        };
        [
            (self.fog_color * brightness).extend(self.fog.w),
            Vec4::new(
                1000.0 / (end - start).max(1.0),
                start / 1000.0,
                self.fog.x,
                0.0,
            ),
            Vec4::new(self.fog.y, 1.0 - self.fog.z.clamp(0.0, 1.0), 0.0, 0.0),
            Vec4::ZERO,
        ]
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The sky dome (`sky_haze.wgsl`): the game's sky from its table, or
/// without it the fitted haze low in the atmosphere's sky.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct SkyHazeMaterial {
    #[uniform(0)]
    pub params: SkyHazeParams,
    /// The shared look values with the sky table (`look::LookTexture`).
    #[texture(1)]
    pub look: Handle<Image>,
}

/// See `SkyHaze` in `sky_haze.wgsl`.
#[derive(ShaderType, Clone, Debug, Default, PartialEq)]
pub struct SkyHazeParams {
    pub color: Vec4,
    pub glow: Vec4,
    pub light: Vec4,
    pub sun: Vec4,
    pub halo: Vec4,
    pub band: Vec4,
    pub ground: Vec4,
    pub fog_color: Vec4,
    pub fog: Vec4,
}

impl Material for SkyHazeMaterial {
    fn fragment_shader() -> ShaderRef {
        SKY_HAZE_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    /// Over the atmosphere's sky, behind the stars (`SkyMaterial`, -1e9) and
    /// the clouds.
    fn depth_bias(&self) -> f32 {
        -2.0e9
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
struct SkyHazeDome;

fn spawn_sky_haze(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<SkyHazeMaterial>>,
    look: Option<Res<LookTexture>>,
) {
    commands.spawn((
        Name::new("sky haze"),
        SkyHazeDome,
        Mesh3d(meshes.add(Sphere::new(DOME_RADIUS).mesh().uv(48, 24))),
        MeshMaterial3d(materials.add(SkyHazeMaterial {
            params: SkyHazeParams::default(),
            look: look.map(|look| look.0.clone()).unwrap_or_default(),
        })),
        Transform::default(),
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
        // The game draws its sky and clouds into the cube map (the
        // callback `FUN_0340b6cc`, docs/research/wiiu-deferred-shading.md).
        crate::cubemap::in_cube_map(),
    ));
}

/// The cameras and the fog they have, if any.
type FogCameras<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static GlobalTransform,
        Option<&'static Exposure>,
        Option<&'static mut DistanceFog>,
    ),
    crate::camera::MainCamera,
>;

/// Lays the haze over the camera's view and low in its sky.
fn update_haze(
    mut commands: Commands,
    sky: Option<Res<Sky>>,
    environment: Option<Res<Environment>>,
    mut cameras: FogCameras,
    mut domes: Query<(&mut Transform, &MeshMaterial3d<SkyHazeMaterial>), With<SkyHazeDome>>,
    mut materials: ResMut<Assets<SkyHazeMaterial>>,
    moisture: Res<Moisture>,
    sky_lut: Res<LookSkyLut>,
    mut look: ResMut<Look>,
) {
    let (Some(sky), Some(environment)) = (sky, environment) else {
        return;
    };
    for (entity, camera, exposure, fog) in &mut cameras {
        let ev100 = exposure.map_or(DAY_EV100, |e| e.ev100);
        let haze = haze(&environment, &sky, ev100);
        let color = Color::linear_rgba(haze.land.x, haze.land.y, haze.land.z, haze.max);
        let fog_now = DistanceFog {
            color,
            directional_light_color: Color::linear_rgb(haze.glow.x, haze.glow.y, haze.glow.z),
            directional_light_exponent: haze.glow_exponent,
            falloff: FogFalloff::Exponential {
                density: haze.density(),
            },
        };
        match (fog, HAZE_IN_SHADERS) {
            (Some(mut fog), false) => *fog = fog_now,
            (None, false) => {
                commands.entity(entity).insert(fog_now);
            }
            (Some(_), true) => {
                commands.entity(entity).remove::<DistanceFog>();
            }
            (None, true) => {}
        }
        let texels = haze.texels();
        if look.haze != texels {
            look.haze = texels;
        }
        let dome = sky_dome(&environment, &sky, moisture.fog());
        // The height fog is `fog_scatter` like the ad hoc fog B: with the
        // sky table its colour in the game's units at the table's
        // brightness, like B's; without it the fitted brightness.
        let height_color = if sky_lut.0.is_some() {
            dome.fog_color * haze.sky_table
        } else {
            haze.height_color
        };
        let height = [
            height_color.extend(dome.fog.w),
            Vec4::new(
                1.0 / (haze.height_end - haze.height_start),
                haze.height_start,
                0.0,
                0.0,
            ),
            Vec4::ZERO,
            if sky_lut.0.is_some() {
                Vec4::new(
                    haze.sky_table,
                    haze.horizontal,
                    horizon_cosine(camera.translation().y),
                    0.0,
                )
            } else {
                Vec4::ZERO
            },
        ];
        if look.spare != height {
            look.spare = height;
        }
        let adhoc = dome.surface_texels(
            sky_lut.0.is_some().then_some(haze.sky_table),
            haze.height_start,
            haze.height_end,
        );
        if look.adhoc != adhoc {
            look.adhoc = adhoc;
        }
        let Ok((mut transform, material)) = domes.single_mut() else {
            continue;
        };
        if transform.translation != camera.translation() {
            transform.translation = camera.translation();
        }
        let params = SkyHazeParams {
            color: haze.color.extend(haze.max),
            glow: (haze.glow * haze.light).extend(haze.glow_exponent),
            light: sky.towards_light.extend(haze.sky_fade),
            sun: sky.towards_sun.extend(0.0),
            halo: haze.halo.extend(0.0),
            band: haze.band.extend(0.0),
            ground: dome.ground,
            fog_color: dome.fog_color.extend(0.0),
            fog: dome.fog,
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
}

/// Cosine of the horizon's angle from the zenith seen from `height` metres
/// up, −√(1 − (6360/r)²) with r = 6360 km + height (the game's
/// `SKY_BakeInscatterLut` writes it for the fog: `e27.w` of PS 140).
fn horizon_cosine(height: f32) -> f32 {
    let r = 6360.0 + height.max(0.0) / 1000.0;
    -(1.0 - (6360.0 / r).powi(2)).max(0.0).sqrt()
}

/// The fog's height noise being read from the game's data.
#[derive(Resource)]
struct NoiseLoading(Option<Task<Option<LookNoise>>>);

/// The game's `cloud_noise` (64×64), or `None` (reported) if it cannot be
/// read or has another size.
fn load_noise(assets: &std::path::Path) -> Option<LookNoise> {
    let path = assets.join(asset_format::paths::CLOUD_NOISE);
    let loaded = if path.exists() {
        asset_format::read_ron::<GreyTexture>(&path).map(Some)
    } else {
        Ok(None)
    };
    match loaded {
        Ok(Some(noise))
            if noise.width as usize == LOOK_NOISE && noise.height as usize == LOOK_NOISE =>
        {
            Some(LookNoise(noise.texels))
        }
        Ok(Some(noise)) => {
            warn!(
                "fog noise: {}x{} instead of {LOOK_NOISE}x{LOOK_NOISE}; no height noise",
                noise.width, noise.height
            );
            None
        }
        Ok(None) => None,
        Err(error) => {
            warn!("fog noise: {error}; no height noise");
            None
        }
    }
}

fn finish_noise(mut loading: ResMut<NoiseLoading>, mut noise: ResMut<LookNoise>) {
    let Some(task) = loading.0.as_mut() else {
        return;
    };
    let Some(loaded) = block_on(poll_once(task)) else {
        return;
    };
    loading.0 = None;
    if let Some(loaded) = loaded {
        let mean = loaded.0.iter().map(|&v| f32::from(v)).sum::<f32>() / loaded.0.len() as f32;
        info!("fog noise: the game's cloud_noise, mean {:.0}/255", mean);
        *noise = loaded;
    }
}

/// The air's moisture in percent, like the game's `TempMgr` (field +0x28,
/// `0x0365d3c4`, docs/research/wiiu-deferred-shading.md): each game hour
/// and on a change of climate a new target between the climate's
/// `MoistureMin` and `MoistureMax` (in the upper half from 5 to 9 o'clock),
/// plus the weather's `AddMoisture`; the value follows it by a tenth per
/// 30 Hz frame, at most 0.1 per frame. In the field its hundredth is the
/// strength of the height fog.
#[derive(Resource, Clone, Debug, Default)]
pub struct Moisture {
    pub percent: f32,
    /// The target's base and what it was rolled for (day, hour, climate).
    base: f32,
    rolled: Option<(u32, u32, usize)>,
}

impl Moisture {
    /// The height fog's strength (the game's `fog_scatter` alpha in field
    /// palette sets).
    pub fn fog(&self) -> f32 {
        (self.percent / 100.0).clamp(0.0, 1.0)
    }
}

/// The game's roll `r` in [0, 1) for an hour (`TempMgr` draws one from
/// `sead::Random`; here a hash of the day, the hour and the climate, so a
/// scene looks the same every time).
// SI-SKY-12: moisture roll hash and arrival test are ours, not sead::Random.
fn moisture_roll(day: u32, hour: u32, climate: usize) -> f32 {
    let mut x =
        (u64::from(day) << 32) ^ (u64::from(hour) << 8) ^ climate as u64 ^ 0x9e37_79b9_7f4a_7c15;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

/// The weather's added moisture (`WeatherInfluence_N.AddMoisture`) as the
/// game's `TempMgr` picks it: N = 0 for cloudy and thunderstorm, 1 for rain
/// in sunshine, 2 for rain and snow, 3 for heavy rain, heavy snow and
/// thunder rain; nothing in clear weather.
fn weather_moisture(environment: &Environment, weather: usize) -> f32 {
    let index = match weather {
        1 | 6 => 0,
        8 => 1,
        2 | 4 => 2,
        3 | 5 | 7 => 3,
        _ => return 0.0,
    };
    environment
        .weather_influences
        .get(index)
        .map_or(0.0, |i| i.add_moisture)
}

/// The target the moisture heads for.
// SI-SKY-12: moisture roll hash and arrival test are ours, not sead::Random.
fn moisture_target(base: f32, environment: &Environment, weather: &Weather) -> f32 {
    // The game adds the weather's once its change is nearly done (the
    // weather transition at 0.9, `0x0365d3c4`).
    let added = if weather.transition >= 0.9 {
        weather_moisture(environment, weather.arriving)
    } else {
        0.0
    };
    (base + added).min(100.0)
}

fn update_moisture(
    time_of_day: Option<Res<TimeOfDay>>,
    climate: Option<Res<Climate>>,
    weather: Option<Res<Weather>>,
    environment: Option<Res<Environment>>,
    time: Res<Time>,
    mut moisture: ResMut<Moisture>,
) {
    let (Some(time_of_day), Some(climate), Some(weather), Some(environment)) =
        (time_of_day, climate, weather, environment)
    else {
        return;
    };
    let Some(defines) = environment.climates.get(climate.current) else {
        return;
    };
    let hour = (time_of_day.hours.max(0.0) as u32) % 24;
    let key = (time_of_day.day, hour, climate.current);
    let first = moisture.rolled.is_none();
    if moisture.rolled != Some(key) {
        let mut r = moisture_roll(key.0, key.1, key.2);
        if (5..10).contains(&hour) {
            r = r * 0.5 + 0.5;
        }
        let (min, max) = defines.moisture;
        moisture.base = min + (max - min) * r;
        moisture.rolled = Some(key);
    }
    let target = moisture_target(moisture.base, &environment, &weather);
    if first {
        moisture.percent = target;
        return;
    }
    // `m += (t − m)(1 − 0.9^Δ)`, at least 0.01Δ and at most 0.1Δ per step,
    // Δ in 30 Hz frames.
    let frames = time.delta_secs() * 30.0;
    let gap = target - moisture.percent;
    if gap.abs() <= 0.01 * frames {
        moisture.percent = target;
        return;
    }
    let step = (gap.abs() * (1.0 - 0.9f32.powf(frames))).clamp(0.01 * frames, 0.1 * frames);
    moisture.percent += step.copysign(gap);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use asset_format::env::Influence;

    /// The sky at `hours` in the plain climate and clear weather.
    pub(crate) fn sky_at(environment: &Environment, hours: f32) -> Sky {
        let towards_sun = crate::daynight::sun_direction(hours, &environment.sun);
        let palette = environment.at(hours);
        Sky {
            palette,
            weather: Influence::default(),
            light_feature: Vec3::ONE,
            fog_feature: Vec3::ONE,
            scalars: Influence::default(),
            towards_sun,
            towards_moon: crate::daynight::moon_direction(hours),
            towards_light: towards_sun,
            main_light: towards_sun,
            light_change: 1.0,
            sun_up: 1.0,
            night: 0.0,
            horizon: Vec3::ONE,
            // The field's clear set 0: the fog takes the moisture.
            moist_fog: crate::daynight::MoistFog {
                moist: 1.0,
                moist_sky: 1.0,
                ..default()
            },
            flash: 0.0,
            flash_peak: false,
        }
    }

    #[test]
    fn midday_haze_is_the_palettes_fog_scaled_by_the_light() {
        let environment = Environment::fallback();
        let noon = haze(&environment, &sky_at(&environment, 12.0), 13.5);
        let fog = Vec3::from_slice(&environment.at(12.0).fog_color) * CLEAR_AIR;
        // The palette's fog colour in a clear day's air.
        assert!(
            (noon.color / noon.color.max_element() - fog / fog.max_element())
                .abs()
                .max_element()
                < 1e-4
        );
        // An exposure a stop more open brightens the haze a stop, like the frame.
        let brighter = haze(&environment, &sky_at(&environment, 12.0), 12.5);
        assert!((brighter.color.y / noon.color.y - 2.0).abs() < 1e-3);
        // Distant land fades into a darker, bluer colour than the sky.
        assert_eq!(noon.land, noon.color * DISTANT_LAND);
        // The game's scattering fog from the palette: up to 85 % between the
        // palette's near and the static far end, thickening by its exponent.
        let palette = environment.at(12.0);
        assert_eq!(noon.near, palette.scatter_near);
        assert_eq!(noon.far, environment.palette_static.scatter_far);
        assert_eq!(noon.attenuation, palette.scatter_attenuation);
        assert!((noon.max - 0.85).abs() < 1e-6);
        // The look texture holds the range in kilometres.
        let texels = noon.texels();
        assert_eq!(texels[0].w, noon.max);
        assert!((texels[1].x - 1000.0 / (noon.far - noon.near)).abs() < 1e-6);
        assert!((texels[1].y - noon.near / 1000.0).abs() < 1e-6 && texels[1].z == noon.attenuation);
        // The glow gathers round the sun, tighter for more forward scattering.
        assert!(noon.glow.max_element() > 0.0 && noon.glow_exponent > 2.0);
    }

    #[test]
    fn a_hazy_climate_keeps_its_fog_colour() {
        let environment = Environment::fallback();
        let plain = haze(&environment, &sky_at(&environment, 12.0), 13.5);
        let mut sky = sky_at(&environment, 12.0);
        sky.scalars = Influence {
            rayleigh: 0.75,
            mie: 2.0,
            ..Influence::default()
        };
        let desert = haze(&environment, &sky, 13.5);
        let fog = Vec3::from_slice(&environment.at(12.0).fog_color);
        let blue = |c: Vec3| c.z / c.y;
        assert!(blue(desert.color) < blue(plain.color));
        let kept = (0.75f32 / 2.0).powf(AIR_FOLLOWS_SCATTERING);
        let expected = fog * Vec3::ONE.lerp(CLEAR_AIR, kept);
        assert!((blue(desert.color) - blue(expected)).abs() < 1e-4);
    }

    #[test]
    fn the_sky_dome_fogs_as_the_air_is_moist() {
        let environment = Environment::fallback();
        let mut sky = sky_at(&environment, 12.0);
        // Dry air: no ad hoc fog (the game's variant without it).
        assert_eq!(sky_dome(&environment, &sky, 0.0).fog.w, 0.0);
        // Moist air: as strong as the moisture at the horizon, thinning
        // upwards faster the moister it is, at least by 0.5.
        let wet = sky_dome(&environment, &sky, 0.6);
        assert_eq!((wet.fog.w, wet.fog.z), (0.6, ADHOC_SKY_MIN_SCALE));
        assert!((wet.fog.y - 0.6 * ADHOC_SKY_ATTENUATION_PER_MOISTURE).abs() < 1e-6);
        assert_eq!(
            sky_dome(&environment, &sky, 0.05).fog.y,
            ADHOC_SKY_MIN_ATTENUATION
        );
        // Outside the field's palette sets the palettes' own strength and
        // `atten_sky`, whatever the moisture.
        let woods = Sky {
            moist_fog: crate::daynight::MoistFog {
                alpha: 0.4,
                attenuation_sky: 2.0,
                ..default()
            },
            ..sky.clone()
        };
        for moisture in [0.0, 0.6] {
            let dome = sky_dome(&environment, &woods, moisture);
            assert_eq!((dome.fog.w, dome.fog.y), (0.4, 2.0));
        }
        // In the palette's fog colour, tinted by the fog factor.
        sky.fog_feature = Vec3::new(0.5, 1.0, 2.0);
        let tinted = sky_dome(&environment, &sky, 0.6);
        let fog = Vec3::from_slice(&sky.palette.fog_color[..3]);
        assert_eq!(tinted.fog_color, fog * Vec3::new(0.5, 1.0, 2.0));
        // The ground's colour; its bksky alpha 1 shows it only below the horizon.
        let ground = environment.renderer.sky.ground_color;
        assert_eq!(tinted.ground.truncate(), Vec3::from_slice(&ground[..3]));
        assert_eq!(tinted.ground.w, 1.0 - ground[3]);
        // Over the surfaces the same fog, only with the sky table.
        assert_eq!(tinted.surface_texels(None, -20.0, 400.0), [Vec4::ZERO; 4]);
        let surface = tinted.surface_texels(Some(2.0), -20.0, 400.0);
        assert_eq!(surface[0], (tinted.fog_color * 2.0).extend(0.6));
        assert!((surface[1].x - 1000.0 / 420.0).abs() < 1e-4 && surface[1].y == -0.02);
        assert_eq!(surface[1].z, sky.palette.attenuation_ground);
        assert!((surface[2].y - (1.0 - ADHOC_SKY_MIN_SCALE)).abs() < 1e-6);
    }

    #[test]
    fn moisture_rolls_between_the_climates_bounds_and_follows_slowly() {
        // Rolls land in [0, 1) and differ from hour to hour.
        let rolls: Vec<f32> = (0..24).map(|hour| moisture_roll(3, hour, 0)).collect();
        assert!(rolls.iter().all(|r| (0.0..1.0).contains(r)));
        assert!(rolls.windows(2).any(|w| w[0] != w[1]));
        let mut environment = Environment::fallback();
        let weather = Weather::default();
        // Clear weather adds nothing, cloudy its influence's `AddMoisture`.
        assert_eq!(moisture_target(12.0, &environment, &weather), 12.0);
        environment.weather_influences = (0..4)
            .map(|i| Influence {
                add_moisture: 10.0 * (i + 1) as f32,
                ..Influence::default()
            })
            .collect();
        let mut weather = Weather::settled(1);
        assert_eq!(moisture_target(12.0, &environment, &weather), 22.0);
        // Not while it is still blowing in.
        weather.transition = 0.5;
        assert_eq!(moisture_target(12.0, &environment, &weather), 12.0);
        let weather = Weather::settled(3);
        assert_eq!(moisture_target(90.0, &environment, &weather), 100.0);
        // Its hundredth is the height fog's strength.
        let moisture = Moisture {
            percent: 20.0,
            ..Moisture::default()
        };
        assert!((moisture.fog() - 0.2).abs() < 1e-6);
    }
}
