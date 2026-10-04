//! Clouds: drifting cumulus kilometres above the map, drawn by the sky like
//! in BotW (`ksys::world::SkyMgr` draws its cloud layers in the sky shader;
//! PTCL effects only make the caps over a few mountains). For every pixel of
//! the sky `clouds.wgsl` follows the view ray up to the layer's height and
//! looks up a tileable noise texture baked at startup there, shaped into
//! puffs and lit by the sun. The layer is drawn on a dome around the camera,
//! behind everything but the stars; the pattern stays put in the world and
//! drifts with the wind. With the game's sky table the layers are laid on
//! the game's own dome meshes instead and drawn like its `cloud` shader,
//! the same pattern standing in for its textures.

use asset_format::env::{
    CloudLayer, CloudTextureNumbers, DensitySpot, PaletteCloud, SkyCloudLayer, SkyClouds, Sway,
};
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::ecs::system::SystemParam;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use bevy::shader::ShaderRef;

use crate::climate::Weather;
use crate::daynight::{Environment, Sky};
use crate::grass::buffer::SeadRandom;
use crate::look::LookTexture;
use crate::object_material::ObjectMaterial;
use crate::terrain_material::TerrainMaterial;
use crate::texture::{NamedTexture, texture_layer_image};

const SHADER: Handle<Shader> = uuid_handle!("8f1c6d2a-3b47-4e9a-b5d0-2c7e91a4f635");
/// `botw::clouds`: cloud density, shared with the surfaces clouds shade.
const COMMON: Handle<Shader> = uuid_handle!("2d94b7e1-6c05-4a3f-8e12-b7f0c4d9a816");

/// Radius of the dome the clouds are drawn on: inside the stars' dome and
/// the camera's far plane (30 km), beyond all terrain.
const DOME_RADIUS: f32 = 22_000.0;
/// Side of the baked noise texture, in texels.
const NOISE_SIZE: usize = 256;

mod reduced;

pub struct CloudsPlugin;

impl Plugin for CloudsPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, COMMON, "clouds_common.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, SHADER, "clouds.wgsl", Shader::from_wgsl);
        // Made here so every material that shows cloud shadows finds it.
        let noise = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(noise_image());
        // The look texture, for the models' haze (`LookPlugin` comes first).
        let look = app
            .world()
            .get_resource::<LookTexture>()
            .map(|look| look.0.clone())
            .unwrap_or_default();
        let shadow_map = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(no_shadow_image());
        app.insert_resource(CloudShadows {
            noise,
            shadow_map,
            look,
            params: CloudParams::default(),
            revision: 0,
        })
        .init_resource::<CloudsNow>()
        .init_resource::<GlobalRandom>()
        .add_plugins((
            MaterialPlugin::<CloudMaterial>::default(),
            reduced::ReducedBufferPlugin,
        ))
        .add_systems(Startup, spawn_clouds)
        .add_systems(
            PostUpdate,
            (
                upload_shadow_map,
                upload_cloud_textures,
                update_clouds,
                share_layers,
            )
                .chain()
                .before(TransformSystems::Propagate),
        );
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug, PartialEq)]
pub struct CloudMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub noise: Handle<Image>,
    #[uniform(2)]
    pub params: CloudParams,
    /// The shared look values (`look::LookTexture`): the sky table behind
    /// the clouds and the ad hoc fog they end in.
    #[texture(3)]
    pub look: Handle<Image>,
    #[uniform(4)]
    pub light: CloudLight,
    /// The game's textures of the upper and the lower layer: PS 449's
    /// `cBaseTexture`, `cBaseTexture_Blend`, `cNoiseTexture`,
    /// `cNoiseTexture_Blend` ([`CloudTextures`]); the noise until they are
    /// read (`CloudLight::scale.z` says whether they are).
    #[texture(5)]
    #[sampler(6)]
    pub upper_base: Handle<Image>,
    #[texture(7)]
    pub upper_base_blend: Handle<Image>,
    /// Its sampler mirrors (the game's noise samplers, unlike the bases'
    /// that repeat: [`CloudTextures`]).
    #[texture(8)]
    #[sampler(14)]
    pub upper_noise: Handle<Image>,
    #[texture(9)]
    pub upper_noise_blend: Handle<Image>,
    #[texture(10)]
    pub lower_base: Handle<Image>,
    #[texture(11)]
    pub lower_base_blend: Handle<Image>,
    #[texture(12)]
    pub lower_noise: Handle<Image>,
    #[texture(13)]
    pub lower_noise_blend: Handle<Image>,
}

/// The game's cloud light (the `cloud` shader's PS 449 in `clouds.wgsl`):
/// the palette's cloud colours and the layers' lighting. Used only with the
/// game's sky table; without it the layers keep this renderer's own light
/// (`CloudParams`).
#[derive(ShaderType, Clone, Debug, Default, PartialEq)]
pub struct CloudLight {
    /// `mCloudColorScale` (x); 1 when the layers are lit like the game (y);
    /// 1 when they are drawn with the game's textures (z).
    pub scale: Vec4,
    pub upper: CloudLayerLight,
    pub lower: CloudLayerLight,
}

/// One layer of [`CloudLight`].
#[derive(ShaderType, Clone, Debug, Default, PartialEq)]
pub struct CloudLayerLight {
    /// `sysColor0Vary`, `sysColor1Vary`, `cShadowCol` (rgb).
    pub base: Vec4,
    pub hilight: Vec4,
    pub shadow: Vec4,
    /// `cBackLightCol` (rgb), `mBacklightPower` (w).
    pub backlight: Vec4,
    /// `mSkyScale` (m), `mScatterHeight`, 1 − `mScatterAmb`, and
    /// [`PATTERN_SPAN`] (the layer's pattern scale times it is
    /// `mBaseTexScale`).
    pub dome: Vec4,
    /// `mShadowPower`, `mHilightPower`, `mHighlightRange`, `mHighlightAmbient`.
    pub relief: Vec4,
    /// `mBacklightRange`, `mBacklightParam0`, `mBacklightParam1`.
    pub back: Vec4,
    /// `mEmbossWidth`, `mEmbossDensity`, `mFarUVMul`, `mFarUVPow`.
    pub far_uv: Vec4,
    /// `mFarDensityChgStart`, `…End`, `…Power`.
    pub far_density: Vec4,
    /// `mFarAlphaChgStart`, `…End`, `…Power`.
    pub far_alpha: Vec4,
    /// The texture's offset in the dome's units (xy, [`CloudScroll`]).
    pub scroll: Vec4,
    /// `mNoiseScale1`, `mNoiseScale2`, `mNoiseDensity1`, `mNoiseDensity2`.
    pub noise: Vec4,
    /// The noises' offsets, the uniforms `mNoiseSpeed1X`, `1Y`, `2X`, `2Y`
    /// ([`CloudScroll`]).
    pub noise_offset: Vec4,
    /// `mFarDistotionChgStart`, `…End`, `…Power`.
    pub far_distortion: Vec4,
    /// `mDarkSideNoiseParam`, `mLightSideNoiseParam`.
    pub side_noise: Vec4,
    /// `mCloudTexBlendRate` (x, [`CloudBlend`]).
    pub texture_blend: Vec4,
    /// The channel PS 449 reads as x and as w from each texture (base,
    /// base blend, noise, noise blend): its GX2 component selection, 0–3 a
    /// channel, 4 zero, 5 one.
    pub channel_x: Vec4,
    pub channel_w: Vec4,
    /// The layer's spot of changed density ([`DensitySpots`]):
    /// `mPosDensityChgX`, `Y`, `Range`, `Power`.
    pub spot: Vec4,
}

/// The game's cloud light for the sky `sky` at `hours`; lit like the game
/// only when `table` (the game's sky table is there). Like `ENV_UpdateWeatherPalettes` (U-King.rpx v208): each
/// layer's row of the palette's cloud colours (the upper `CloudParam0` its
/// `Cloud0_*` or `Cloud2_*`, the lower `CloudParam2` its `Cloud2_*`, picked
/// per palette: [`EnvPalette`]) times their intensities in `F`, the
/// climate's and the weather's `FeatureColor` on the field's row of palette
/// sets ([`Sky::light_feature`]); around 22:00 and 03:00 (`SKY_CalcNightFadeByTime`) the
/// shadow turns the base colour and the highlight and the glow into the
/// light go out. The layers' relief, glow and sides' noise are what
/// `SkyMgr` writes (`FUN_0365867c`): each value of the layer's clear look
/// (`PrCloudV0_N`) blended towards its cloudy one (`PrCloudV1_N`) by the
/// world's cloudiness `cloudiness` (`SkyMgr+0x2120`); the dome's size, the
/// sky's share and the rim's `mFar*` are the renderer's
/// (`master_field.baglclwd`). `scroll` is each layer's texture and noise
/// offsets, `blend` the base textures' blend rates (upper, lower), `spots`
/// the layers' spots of changed density. The noises' scales and weights and
/// the warp towards the rim are the renderer's (`mNoiseScale*`,
/// `mNoiseDensity*`, `mFarDistotionChg*`; `SkyMgr` does not write them).
/// `mBacklightPower` is the palette's times `backlight`, `SkyMgr+0x2154`
/// ([`MoonBacklight`], 0x036499d8).
#[allow(clippy::too_many_arguments)]
fn game_light(
    environment: &Environment,
    sky: &Sky,
    hours: f32,
    table: bool,
    cloudiness: f32,
    scroll: &CloudScroll,
    blend: [f32; 2],
    spots: &DensitySpots,
    backlight: f32,
) -> CloudLight {
    let palette = &sky.palette;
    let feature = sky.light_feature;
    let night = crate::sky_lut::night_fade(hours);
    let day = 1.0 - night;
    let color = |c: [f32; 3]| Vec3::from(c) * feature;
    let dome = &environment.renderer.clouds;
    let layer = |index: usize, row: PaletteCloud| {
        let game = &blend_looks(&environment.clouds.layers[index], cloudiness);
        let drawn = &dome.layers[index];
        let shadow = color(row.shadow).lerp(color(row.base), night)
            * (row.shadow_intensity + (row.base_intensity - row.shadow_intensity) * night);
        CloudLayerLight {
            base: (color(row.base) * row.base_intensity).extend(0.0),
            hilight: (color(row.highlight) * row.highlight_intensity * day).extend(0.0),
            shadow: shadow.extend(0.0),
            backlight: (color(row.backlight) * day).extend(row.backlight_power * day * backlight),
            dome: Vec4::new(
                drawn.sky_scale.max(1000.0),
                drawn.scatter_height,
                1.0 - drawn.scatter_ambient,
                PATTERN_SPAN,
            ),
            relief: Vec4::new(
                game.shadow_power,
                game.highlight_power,
                game.highlight_range,
                game.highlight_ambient,
            ),
            back: Vec4::new(
                game.backlight_range,
                game.backlight_param0,
                game.backlight_param1,
                0.0,
            ),
            far_uv: Vec4::new(
                game.emboss_width,
                game.emboss_density,
                drawn.far_uv_mul,
                drawn.far_uv_pow,
            ),
            far_density: Vec3::from(drawn.far_density).extend(0.0),
            far_alpha: Vec3::from(drawn.far_alpha).extend(0.0),
            scroll: scroll.base[index].extend(0.0).extend(0.0),
            noise: Vec4::new(
                drawn.noise_scale[0],
                drawn.noise_scale[1],
                drawn.noise_density[0],
                drawn.noise_density[1],
            ),
            noise_offset: scroll.noise[index],
            far_distortion: Vec3::from(drawn.far_distortion).extend(0.0),
            side_noise: Vec4::new(game.dark_side_noise, game.light_side_noise, 0.0, 0.0),
            texture_blend: Vec4::new(blend[index], 0.0, 0.0, 0.0),
            channel_x: Vec4::ZERO,
            channel_w: Vec4::ZERO,
            spot: spots.uniform(&environment.clouds.layers[index], index),
        }
    };
    CloudLight {
        scale: Vec4::new(dome.color_scale, if table { 1.0 } else { 0.0 }, 0.0, 0.0),
        upper: layer(0, palette.upper_cloud()),
        lower: layer(1, palette.lower_cloud),
    }
}

/// Each layer's texture offset in the dome's units, as the game's
/// `mBaseTexScrollSpdX/Y` reach VS 448: `FUN_03a59734` (Wii U v208) adds
/// the speed times its step (`Cloud+0x4c9c`, 1 from the constructor) each
/// frame, 30 a second, and starts an offset past ±300 again at 0. The
/// noises' offsets, the uniforms `mNoiseSpeed1X`–`2Y` of PS 449, move the
/// same way by `mNoiseSpeed*`·`mNoiseSpeedMaster`·0.002 a frame.
///
/// `SkyMgr` writes the speeds every frame (`FUN_0365867c`): along the
/// layer's wind angle a (the angle of [`SkyWind`]'s direction, turned by
/// `WindVecAdd`·r per layer, r drawn in [−1, 1) at each reset
/// ([`DensitySpots`]), the lower layer's from the upper's),
/// `mBaseTexScrollSpd` = (sin a, cos a)·k₁·`ScrollSpd` and the noises' from
/// `NoiseAdd*` by k₂ (`2Y` takes cos a for both terms), with k₁, k₂ from
/// [`SkyWind::factors`]. The speeds `master_field.baglclwd` holds are what
/// `SkyMgr` wrote when it was saved (they fit with k₁ ≈ 1.05·10⁻⁴,
/// k₂ ≈ 0.105, a wind of 3.5); the renderer's own are not used.
#[derive(Default)]
pub struct CloudScroll {
    base: [Vec2; 2],
    noise: [Vec4; 2],
}

impl CloudScroll {
    /// Moves on by `frames` frames at 30 fps under `wind`, the layers turned
    /// by `turns` (r of the upper and the lower layer).
    fn advance(&mut self, environment: &Environment, frames: f32, wind: &SkyWind, turns: [f32; 2]) {
        let speeds = layer_speeds(&environment.clouds, wind, turns);
        for ((offset, drawn), (_, noise)) in self
            .noise
            .iter_mut()
            .zip(&environment.renderer.clouds.layers)
            .zip(speeds)
        {
            *offset += noise * drawn.noise_speed_master * 0.002 * frames;
            *offset = Vec4::select(offset.abs().cmpgt(Vec4::splat(300.0)), Vec4::ZERO, *offset);
        }
        for (offset, (base, _)) in self.base.iter_mut().zip(speeds) {
            *offset += base * frames;
            for value in [&mut offset.x, &mut offset.y] {
                if value.abs() > 300.0 {
                    *value = 0.0;
                }
            }
        }
    }
}

/// `SkyMgr+0x2154`, the factor on the layers' `mBacklightPower`
/// (`ENV_UpdateWeatherPalettes` 0x036499d8): each frame (`FUN_0365867c`,
/// Wii U v208) it heads for 1, or for `+0x2158` (0; only the reset writes
/// it) while `TimeMgr::getMoonType` (`FUN_0365e34c`) is the new moon, by
/// `1 − 0.9^t` of the way held to exactly 0.01·t (t the frame scale). A
/// stage's reset sets it to 1. So from noon of the new moon's day to the
/// next noon the clouds' glow into the light fades out over 100 frames.
/// While [`crate::climate::StageTimer`] runs it takes its target at once.
pub struct MoonBacklight(f32);

impl Default for MoonBacklight {
    fn default() -> Self {
        Self(1.0)
    }
}

/// `MoonType::NewMoon` (Switch `worldTimeMgr.h`).
const NEW_MOON: u32 = 4;

impl MoonBacklight {
    /// Moves on by `frames` frames at 30 fps under the moon's phase `moon`
    /// (`MoonType`, 0 full).
    fn advance(&mut self, moon: u32, frames: f32) {
        let step = 0.01 * frames;
        self.0 = crate::climate::chase(self.0, Self::target(moon), 0.9, frames, step, step);
    }

    /// Takes the target at once (under `WorldMgr+0x53c`).
    fn take(&mut self, moon: u32) {
        self.0 = Self::target(moon);
    }

    fn target(moon: u32) -> f32 {
        if moon == NEW_MOON { 0.0 } else { 1.0 }
    }
}

/// `sead::GlobalRandom` (`DAT_1046c948`, Wii U v208), the generator the
/// sky manager draws from. The game makes it once at boot (`FUN_030c47b4`)
/// and seeds it by `sead::Random::init()` with the low 32 bits of the tick
/// count at that moment (`FUN_030c4938` → `FUN_030c48dc`); nothing seeds it
/// again, and hundreds of callers draw from it, so its draws differ every
/// session and the game's values cannot be recovered. Here likewise it is
/// seeded from the clock at start ([`Self::from_clock`]), so each run
/// differs; screenshots take [`CAPTURE_SEED`] anew with every
/// scene, as if the game had just booted, so that each shot repeats on its
/// own.
/// Only the sky manager's own draws are made from it, in its order: the
/// constructor's ([`CloudSway::build`]), each stage's reset
/// ([`DensitySpots::reset`]), then every frame the spots and the blend.
/// Not made: `FUN_03655de8`'s draw every 32 ticks of the world
/// (`WorldMgr+0x538`) into `SkyMgr+0x2124`, whose use is not traced.
#[derive(Resource)]
pub struct GlobalRandom {
    pub random: SeadRandom,
    /// Seeded with [`CAPTURE_SEED`] at every new scene.
    repeats: bool,
}

impl Default for GlobalRandom {
    fn default() -> Self {
        Self::from_clock()
    }
}

impl GlobalRandom {
    /// Seeded with the low 32 bits of the clock, as `sead::Random::init()`
    /// with the tick count.
    pub fn from_clock() -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        Self {
            random: SeadRandom::new(now as u32),
            repeats: false,
        }
    }

    /// Seeded with [`CAPTURE_SEED`] now and at every new scene.
    pub fn fixed() -> Self {
        Self {
            random: SeadRandom::new(CAPTURE_SEED),
            repeats: true,
        }
    }
}

/// The seed of [`GlobalRandom`] in screenshots, so that they
/// repeat (the viewer's; the game's differs every session).
pub const CAPTURE_SEED: u32 = 0x0123_4567;

/// `mBaseTexScrollSpdX/Y` and `mNoiseSpeed1X`–`2Y` of the upper and the
/// lower layer as `SkyMgr` writes them (`FUN_0365867c`, [`CloudScroll`]).
fn layer_speeds(clouds: &SkyClouds, wind: &SkyWind, turns: [f32; 2]) -> [(Vec2, Vec4); 2] {
    let (k1, k2) = wind.factors();
    let w = wind.direction();
    let mut angle = w.x.atan2(w.y);
    std::array::from_fn(|index| {
        let layer = &clouds.layers[index];
        let a = angle + layer.wind_add * turns[index];
        let (s, c) = a.sin_cos();
        // The next layer turns from this one's angle (`atan2(sin a, cos a)`).
        angle = s.atan2(c);
        let [add1, add2, add1_side, add2_side] = layer.noise_add;
        let base = Vec2::new(s, c) * k1 * layer.scroll_speed;
        let noise = Vec4::new(
            s * add1 + c * add1_side,
            c * add1 + s * add1_side,
            s * add2 + c * add2_side,
            // Both cos, as the game has it (0x03658d74, 0x03658db8).
            c * add2 + c * add2_side,
        ) * k2;
        (base, noise)
    })
}

/// The sky manager's own wind and its speed factors (`FUN_03655de8`, Wii U
/// v208, every frame): the angle `SkyMgr+0x212c` turns towards the world's
/// wind's, `atan2(x, z)` of `world::Manager::getWindDirection`, by
/// `1 − 0.5^t` of the way, at least 0.01·t and at most 0.05·t rad (t the
/// frame scale; straight, not around the circle); the speed `+0x2130`
/// heads for `getWindSpeed` by 0.1·t exactly. The constructor starts them
/// at 0 and 0.2 (`0x0364f620`); a stage's reset leaves them. From them the
/// direction `+0x20c8/+0x20d0` = (sin, cos) of the angle and the factors
/// k₂ = `+0x2148` = min(speed/10, 1)·0.3·m and k₁ = `+0x2144` = 0.001·k₂,
/// m = `TimeMgr+0xb0`, the time step's multiple of the usual, 1 in
/// ordinary play. Under the Blood Moon's palette sets (16, 17) the game
/// pulls k₁, k₂ towards `CloudSpd`'s `CLOUDPAT_windPow`/`noisePow`; the
/// viewer has no such sets. While [`crate::climate::StageTimer`] runs both take their
/// targets at once.
pub struct SkyWind {
    angle: f32,
    speed: f32,
}

impl Default for SkyWind {
    fn default() -> Self {
        Self {
            angle: 0.0,
            speed: 0.2,
        }
    }
}

/// `TimeMgr+0xb0`: the time step over the usual one, at least 1, set only
/// while time flows normally (Switch `worldTimeMgr.cpp`, `_d0`). Always 1:
/// the time step `TimeMgr+0xa4` is written only by the constructor
/// (`FUN_0365e9fc`) and the reset (`FUN_03661870`), both with the usual
/// step `0x3c088889` (1/120), which the update (`FUN_0365f558`) divides it
/// by (Wii U v208; the Switch source has no setter either).
const TIME_STEP_MULTIPLE: f32 = 1.0;

impl SkyWind {
    /// One step of `frames` frames at 30 fps towards the world's wind, its
    /// `speed` and direction `(x, z)`.
    fn advance(&mut self, (speed, direction): (f32, Vec2), frames: f32) {
        let target = direction.x.atan2(direction.y);
        self.angle = crate::climate::chase(
            self.angle,
            target,
            0.5,
            frames,
            0.01 * frames,
            0.05 * frames,
        );
        let step = 0.1 * frames;
        self.speed = crate::climate::chase(self.speed, speed, 0.0, frames, step, step);
    }

    /// Takes the world's wind at once (under `WorldMgr+0x53c`).
    fn take(&mut self, (speed, direction): (f32, Vec2)) {
        self.angle = direction.x.atan2(direction.y);
        self.speed = speed;
    }

    /// The direction (x, z), `SkyMgr+0x20c8`, `+0x20d0`.
    fn direction(&self) -> Vec2 {
        let (s, c) = self.angle.sin_cos();
        Vec2::new(s, c)
    }

    /// k₁ and k₂ (`SkyMgr+0x2144`, `+0x2148`).
    fn factors(&self) -> (f32, f32) {
        let k2 = (self.speed / 10.0).min(1.0) * 0.3 * TIME_STEP_MULTIPLE;
        (k2 * 0.001, k2)
    }
}

/// How far each layer has blended its base texture into the other
/// (`mCloudTexBlendRate` of `CloudParam0` and `CloudParam2`), as `SkyMgr`
/// drives it (`FUN_03659fa8`, Wii U v208, called every frame through
/// `FUN_0365b0b4` → `FUN_0365acb0` outside stage types 0, 2 and 5): after
/// a reset (the stage's load, `FUN_0364f324` from `FUN_03673f6c`) both are
/// 0; then the upper layer goes to 1, the lower to 1, the upper back to 0,
/// the lower back to 0 and so on, each leg at a speed drawn from
/// `sead::Random`, 0.0002 + 0.0008·r per frame (the draw is kept for the
/// leg after the lower layer's return), a chase with base 0.99 whose steps
/// are exactly the speed times the frame scale (`VFR::lerp`). The game runs
/// this twice a frame (its loop over `CloudParam0` and `2` takes the whole
/// step each time). Its other two branches stay off in the field and here:
/// the palettes' `TexBlendRatio` only while `SkyMgr+0x2188` is set, which
/// nothing but resets does, and the pull towards `CloudPat*` only in palette
/// sets 16 and 17 or under the Blood Moon (`FUN_03655de8`), which the viewer
/// does not have. The draws come from [`GlobalRandom`].
// SI-SKY-08: the CloudPat branch (sets 16/17, Blood Moon) is not run.
pub struct CloudBlend {
    state: u32,
    speed: f32,
    rates: [f32; 2],
}

impl Default for CloudBlend {
    fn default() -> Self {
        Self {
            state: 0,
            // `SkyMgr+0x2138` after a reset (`FUN_0364f324`).
            speed: 0.001,
            rates: [0.0; 2],
        }
    }
}

impl CloudBlend {
    /// Moves on by `frames` frames at 30 fps and returns the rates (upper,
    /// lower).
    fn advance(&mut self, frames: f32, random: &mut SeadRandom) -> [f32; 2] {
        for _ in 0..2 {
            self.pass(frames, random);
        }
        self.rates
    }

    fn draw_speed(&mut self, random: &mut SeadRandom) {
        self.speed = 0.0002 + 0.0008 * random.unit();
    }

    /// One pass of `FUN_03659fa8`'s state machine (`SkyMgr+0x217c`).
    fn pass(&mut self, frames: f32, random: &mut SeadRandom) {
        let (layer, target, next, draw) = match self.state {
            0 => {
                // The numbers are copied (`CloudTextureNumbers`), the
                // blend turned on at 0.
                self.rates = [0.0; 2];
                self.draw_speed(random);
                self.state = 1;
                return;
            }
            1 => (0, 1.0, 2, true),
            2 => (1, 1.0, 3, true),
            3 => (0, 0.0, 4, true),
            _ => (1, 0.0, 1, false),
        };
        let step = self.speed * frames;
        let rate = &mut self.rates[layer];
        *rate = crate::climate::chase(*rate, target, 0.99, frames, step, step);
        let arrived = if target > 0.5 {
            *rate >= target
        } else {
            *rate <= target
        };
        if arrived {
            self.state = next;
            if draw {
                self.draw_speed(random);
            }
        }
    }
}

/// The cloud shadow's texture offset (`bias_trans`, `SkyMgr+0x2104`,
/// `+0x2108`): drawn at the reset ([`DensitySpots`]), moved every frame
/// along the sky manager's wind by k₁·1.75·t (`FUN_03655de8`, Wii U v208;
/// [`SkyWind`]) and started over by 1 past ±1 (`FUN_03657fac`), so the
/// shadows sail with the clouds. (With the counter `+0x2184` > 0 the game
/// moves it by `+0x210c/+0x2110` instead; neither has a writer but the
/// reset's 0.) The shaders move it themselves between updates: they get
/// the offset at their clock's 0 and its velocity a second
/// ([`CloudParams::shadow`]), taken afresh when the velocity changes or the
/// guess drifts off (and so when their clock wraps).
pub struct ShadowDrift {
    offset: Vec2,
    /// The offset at shader time 0 and the velocity the shaders have.
    base: Vec2,
    velocity: Vec2,
}

/// The shadow's step over the upper layer's (k₁) along the wind
/// (`0x103013b0`, 1.75).
const SHADOW_DRIFT: f32 = 1.75;

impl ShadowDrift {
    fn new(offset: Vec2) -> Self {
        Self {
            offset,
            base: offset,
            velocity: Vec2::ZERO,
        }
    }

    /// Moves on by `frames` frames at 30 fps under `wind`; `time` is the
    /// shaders' clock (`globals.time`).
    fn advance(&mut self, wind: &SkyWind, frames: f32, time: f32) {
        let step = wind.direction() * wind.factors().0 * SHADOW_DRIFT;
        self.offset += step * frames;
        for value in [&mut self.offset.x, &mut self.offset.y] {
            if *value > 1.0 {
                *value -= 1.0;
            }
            if *value < -1.0 {
                *value += 1.0;
            }
        }
        // Whole repeats of the texture do not show.
        let off = self.offset - (self.base + self.velocity * time);
        let off = off - off.round();
        let velocity = step * 30.0;
        let changed = (velocity - self.velocity).length() > 0.01 * velocity.length();
        if changed || off.abs().max_element() > 0.001 {
            self.velocity = velocity;
            self.base = self.offset - velocity * time;
        }
    }
}

// SI-SKY-09: CloudPat powers and the spot's pull are not run.
/// Each layer's spot where its density changes (`mPosDensityChgX`, `Y`,
/// `Range`, `Power`), as `SkyMgr` moves it every frame (`FUN_0365867c`, Wii U
/// v208; state `PrCloud+0x84`, strength `+0x80`): it draws the spot's place
/// at random in [−1, 1)² on the dome's mesh (two `sead::Random` draws, x
/// then y) with strength 0, grows the strength to 1 by `PosDensityChgSpeed`
/// times the frame scale a frame (a `VFR::lerp` with base 0, so exactly that
/// step), shrinks it back to 0 the same way, and draws a new place; the
/// power is `PosDensityChgPower` times the strength, the range
/// `PosDensityChgRange`. Its pull towards fixed values (`SkyMgr+0x2134` < 1,
/// or palette set 2 by `FUN_0364bdac`) stays off in the field and here.
///
/// The same generator makes the reset's draws (`FUN_0364f324`, a stage's
/// load), in its order: each of the three layers' wind turn r = 2u − 1
/// (`PrCloud+0x98`, [`CloudScroll`]; the unused middle layer's too), then
/// the cloud shadow's offset u, v in [0, 1) (`SkyMgr+0x2104`, `+0x2108`).
/// The draws come from [`GlobalRandom`].
pub struct DensitySpots {
    layers: [DensitySpotState; 2],
    /// r of the upper and the lower layer.
    turns: [f32; 2],
    shadow: ShadowDrift,
}

#[derive(Clone, Copy, Default)]
struct DensitySpotState {
    state: u8,
    strength: f32,
    at: Vec2,
}

impl DensitySpots {
    /// A stage's reset (`FUN_0364f324`).
    fn reset(random: &mut SeadRandom) -> Self {
        let [upper, _, lower] = std::array::from_fn(|_| 2.0 * random.unit() - 1.0);
        let shadow = ShadowDrift::new(Vec2::new(random.unit(), random.unit()));
        Self {
            layers: [DensitySpotState::default(); 2],
            turns: [upper, lower],
            shadow,
        }
    }

    /// Moves on by `frames` frames at 30 fps.
    fn advance(&mut self, clouds: &SkyClouds, frames: f32, random: &mut SeadRandom) {
        for (spot, layer) in self.layers.iter_mut().zip(&clouds.layers) {
            let step = layer.density_spot.speed * frames;
            if spot.state == 0 {
                let x = 2.0 * random.unit() - 1.0;
                let y = 2.0 * random.unit() - 1.0;
                (spot.at, spot.strength, spot.state) = (Vec2::new(x, y), 0.0, 1);
            }
            if spot.state == 1 {
                spot.strength = crate::climate::chase(spot.strength, 1.0, 0.0, frames, step, step);
                if spot.strength >= 1.0 {
                    spot.state = 2;
                }
            } else {
                spot.strength = crate::climate::chase(spot.strength, 0.0, 0.0, frames, step, step);
                if spot.strength <= 0.0 {
                    spot.state = 0;
                }
            }
        }
    }

    /// The shader's `spot` of layer `index` (0 upper, 1 lower).
    fn uniform(&self, layer: &SkyCloudLayer, index: usize) -> Vec4 {
        let spot: &DensitySpot = &layer.density_spot;
        let state = &self.layers[index];
        state
            .at
            .extend(spot.range)
            .extend(spot.power * state.strength)
    }
}

/// The layer's look at cloudiness `cloudiness` (`SkyMgr+0x2120`): each value
/// of the clear look blended towards the cloudy one, like `SkyMgr` writes
/// them (`FUN_0365867c`). The swaying values keep the clear look's own
/// ([`layers`] sways them).
fn blend_looks(layer: &SkyCloudLayer, cloudiness: f32) -> CloudLayer {
    let [clear, cloudy] = &layer.looks;
    let f = |a: f32, b: f32| a + (b - a) * cloudiness;
    CloudLayer {
        emboss_width: f(clear.emboss_width, cloudy.emboss_width),
        emboss_density: f(clear.emboss_density, cloudy.emboss_density),
        shadow_power: f(clear.shadow_power, cloudy.shadow_power),
        highlight_power: f(clear.highlight_power, cloudy.highlight_power),
        highlight_range: f(clear.highlight_range, cloudy.highlight_range),
        highlight_ambient: f(clear.highlight_ambient, cloudy.highlight_ambient),
        backlight_power: f(clear.backlight_power, cloudy.backlight_power),
        backlight_range: f(clear.backlight_range, cloudy.backlight_range),
        backlight_param0: f(clear.backlight_param0, cloudy.backlight_param0),
        backlight_param1: f(clear.backlight_param1, cloudy.backlight_param1),
        dark_side_noise: f(clear.dark_side_noise, cloudy.dark_side_noise),
        light_side_noise: f(clear.light_side_noise, cloudy.light_side_noise),
        ..clear.clone()
    }
}

/// See `clouds_common.wgsl`.
#[derive(ShaderType, Clone, Debug, PartialEq, Reflect)]
pub struct CloudParams {
    /// Direction towards the sun (xyz).
    pub sun: Vec4,
    /// Sunlit colour (rgb) and brightness (w).
    pub lit: Vec4,
    /// Shaded colour (rgb).
    pub shade: Vec4,
    /// Colour seen looking into the light (rgb) and its strength (w).
    pub backlight: Vec4,
    /// Colour of the sky the layers fade into at the horizon (rgb) and how
    /// brightly it glows towards the light there (w, in the backlight's
    /// colour).
    pub haze: Vec4,
    pub upper: CloudLayerParams,
    pub lower: CloudLayerParams,
    /// The cloud shadow's texture coordinates of a world point and their
    /// divisor, the game's `gsys_context[35..37]` ([`shadow_rows`]).
    pub shadow_u: Vec4,
    pub shadow_v: Vec4,
    pub shadow_w: Vec4,
    /// The shadow's strength (x, `1 − proj_shadow_off`), 1 where the
    /// surfaces take it (y) and the texture's velocity a second of the
    /// shaders' clock (zw, [`ShadowDrift`]).
    pub shadow: Vec4,
}

/// One cloud layer on the GPU (see `CloudLayer` in `clouds_common.wgsl`).
#[derive(ShaderType, Clone, Debug, PartialEq, Reflect)]
pub struct CloudLayerParams {
    /// Height in metres, density, opacity scale, the coverage at which the
    /// clouds turn opaque.
    pub shape: Vec4,
    /// Pattern repeats per metre, blend from puffs to overcast, warp
    /// strength, unused.
    pub pattern: Vec4,
    /// Drift in repeats per second (xy), relief sample distance in repeats
    /// (z), how far the pattern is drawn out along the drift (w).
    pub drift: Vec4,
    /// How far each value of `shape` sways either way under the clear
    /// sky's look and its phase ([`CloudSway`]; the shaders sway the layer).
    pub shape_swing: Vec4,
    pub shape_phase: Vec4,
    /// The same for the pattern scale (x) and the warp strength (z).
    pub pattern_swing: Vec4,
    pub pattern_phase: Vec4,
    /// The cloudy sky's look: its swings and phases.
    pub shape_swing_cloudy: Vec4,
    pub shape_phase_cloudy: Vec4,
    pub pattern_swing_cloudy: Vec4,
    pub pattern_phase_cloudy: Vec4,
}

impl Default for CloudParams {
    fn default() -> Self {
        let environment = Environment::fallback();
        let (upper, lower) = layers(
            &environment,
            Weather::default().cloudiness,
            &CloudSway::default(),
        );
        Self {
            sun: Vec3::new(0.4, 0.7, 0.35).normalize().extend(0.0),
            lit: Vec4::new(1.0, 0.98, 0.95, 1.35),
            shade: Vec4::new(0.55, 0.62, 0.74, 0.0),
            backlight: Vec4::new(1.0, 0.98, 0.95, 0.8),
            haze: Vec4::new(0.78, 0.84, 0.92, 0.0),
            upper,
            lower,
            shadow_u: Vec4::new(0.0, 0.0, 0.0, 0.5),
            shadow_v: Vec4::new(0.0, 0.0, 0.0, 0.5),
            shadow_w: Vec4::W,
            shadow: Vec4::new(0.0, 1.0, 0.0, 0.0),
        }
    }
}

impl CloudParams {
    /// Parameters that cast no shadows (for when there are no clouds).
    pub fn without_shadows() -> Self {
        let mut params = Self::default();
        params.shadow.y = 0.0;
        params
    }

    /// Whether the cloud shadow differs from `other`'s enough to show.
    fn shadow_differs(&self, other: &Self) -> bool {
        (self.shadow.x - other.shadow.x).abs() > 0.005
            || self.shadow.zw() != other.shadow.zw()
            || [
                (self.shadow_u, other.shadow_u),
                (self.shadow_v, other.shadow_v),
                (self.shadow_w, other.shadow_w),
            ]
            .iter()
            .any(|(a, b)| *a != *b)
    }

    /// Whether the layers differ from `other`'s enough to show.
    fn layers_differ(&self, other: &Self) -> bool {
        let pairs = [
            (self.upper.shape, other.upper.shape),
            (self.upper.pattern, other.upper.pattern),
            (self.upper.drift, other.upper.drift),
            (self.lower.shape, other.lower.shape),
            (self.lower.pattern, other.lower.pattern),
            (self.lower.drift, other.lower.drift),
            (self.upper.shape_swing, other.upper.shape_swing),
            (self.upper.pattern_swing, other.upper.pattern_swing),
            (self.lower.shape_swing, other.lower.shape_swing),
            (self.lower.pattern_swing, other.lower.pattern_swing),
            (
                self.upper.shape_swing_cloudy,
                other.upper.shape_swing_cloudy,
            ),
            (
                self.upper.pattern_swing_cloudy,
                other.upper.pattern_swing_cloudy,
            ),
            (
                self.lower.shape_swing_cloudy,
                other.lower.shape_swing_cloudy,
            ),
            (
                self.lower.pattern_swing_cloudy,
                other.lower.pattern_swing_cloudy,
            ),
        ];
        pairs.iter().any(|(a, b)| {
            ((*a - *b).abs() / a.abs().max(b.abs()).max(Vec4::splat(1e-6))).max_element() > 0.01
        })
    }
}

/// How far one repeat of the cloud pattern spans at `BaseTexScale` 1, in
/// metres (the game's cloud textures are unknown; picked by eye).
// SI-SKY-08: the no-dump cloud pattern is our own.
const PATTERN_SPAN: f32 = 150_000.0;
/// How far each layer's pattern is drawn out downwind (upper, lower): the
/// game's high clouds are long wisps, its low ones broad soft heaps (by eye).
const STRETCH: (f32, f32) = (3.0, 1.6);
/// Drift speed in m/s per unit of the game's `ScrollSpd`, for this
/// renderer's own pattern (the game's textures move by
/// [`CloudScroll`]).
const DRIFT_PER_SCROLL: f32 = 30.0;
/// The wind this renderer's own pattern drifts with (towards the
/// east-south-east).
const WIND: Vec2 = Vec2::new(0.8, 0.6);
/// The phases φ of the swaying values of each layer (upper, lower) and look
/// (clear, cloudy), in the game's order (height, distortion, density, alpha
/// mul, alpha threshold, texture scale), as `SkyMgr` keeps them (`PrCloudV`,
/// `FUN_0365867c`, Wii U v208): each frame the value is taken at φ, then φ
/// advances by `SinSeedAdd`·k₂·t (k₂ = `SkyMgr+0x2148`,
/// [`SkyWind::factors`]; t the frame scale) and starts over past 2π. A
/// stage's reset leaves them. The constructor draws them ([`Self::build`]);
/// the default, 0, is only for parameters before the first frame.
#[derive(Default)]
pub struct CloudSway {
    phases: [[[f32; 6]; 2]; 2],
}

impl CloudSway {
    /// The phases as `SkyMgr`'s constructor draws them (`FUN_0364f620`,
    /// Wii U v208): after the cloud shadow's offset u, v (`+0x2104`,
    /// `+0x2108`, which every stage's reset draws anew), 2π·u for each
    /// value in order (@`0x036543ec…0x036544c0`, 2π at `0x10300e78`), the
    /// same for every layer and look.
    fn build(random: &mut SeadRandom) -> Self {
        let _shadow = (random.unit(), random.unit());
        let phases = std::array::from_fn(|_| std::f32::consts::TAU * random.unit());
        Self::starting_at(phases)
    }

    fn starting_at(phases: [f32; 6]) -> Self {
        Self {
            phases: [[phases; 2]; 2],
        }
    }

    /// Moves on by `frames` frames at 30 fps with k₂ `k2`.
    fn advance(&mut self, clouds: &SkyClouds, k2: f32, frames: f32) {
        for (layer, game) in self.phases.iter_mut().zip(&clouds.layers) {
            for (phases, look) in layer.iter_mut().zip(&game.looks) {
                for (phase, sway) in phases.iter_mut().zip(sways(look)) {
                    *phase += sway.rate * k2 * frames;
                    if *phase > std::f32::consts::TAU {
                        *phase -= std::f32::consts::TAU;
                    }
                }
            }
        }
    }
}

/// A look's swaying values in [`CloudSway`]'s order.
fn sways(look: &CloudLayer) -> [Sway; 6] {
    [
        look.height,
        look.distortion,
        look.density,
        look.alpha_mul,
        look.alpha_threshold,
        look.tex_scale,
    ]
}

/// Hand-picked cloud layers for when there is no dump: fair-weather
/// cumulus, a thin higher layer drifting at an angle over a lower, denser
/// one, fuller, lower and greyer under a cloudy sky.
pub fn fallback_sky_clouds() -> SkyClouds {
    let sway = |value: f32, swing: f32| Sway {
        value,
        min: value - swing,
        max: value + swing,
        rate: 0.01,
    };
    let look =
        |height: f32, density: f32, alpha_mul: f32, threshold: f32, scale: f32, emboss: f32| {
            CloudLayer {
                distortion: sway(0.6, 0.1),
                density: sway(density, 0.06),
                alpha_mul: sway(alpha_mul, 0.1),
                alpha_threshold: sway(threshold, 0.08),
                tex_scale: sway(scale, 0.3),
                height: sway(height, 500.0),
                emboss_width: emboss,
                emboss_density: 0.0,
                shadow_power: 0.1,
                highlight_power: 0.0,
                highlight_range: 1.5,
                highlight_ambient: 0.1,
                backlight_power: 1.5,
                backlight_range: 0.6,
                backlight_param0: 0.25,
                backlight_param1: 0.7,
                dark_side_noise: 1.0,
                light_side_noise: 0.25,
            }
        };
    let layer = |index: usize, looks: [CloudLayer; 2]| SkyCloudLayer {
        scroll_speed: -0.9,
        wind_add: if index == 0 { 0.5 } else { 0.0 },
        looks,
        ..SkyCloudLayer::game_default(index)
    };
    SkyClouds {
        layers: [
            layer(
                0,
                [
                    look(8000.0, 0.3, 0.6, 0.65, 1.75, 0.05),
                    look(7000.0, 0.45, 0.8, 0.65, 1.75, 0.05),
                ],
            ),
            layer(
                2,
                [
                    look(6500.0, 0.35, 0.9, 0.8, 2.0, 0.075),
                    look(5500.0, 0.45, 0.8, 0.8, 2.0, 0.075),
                ],
            ),
        ],
    }
}

/// The two layers at the world's cloudiness `cloudiness` (`SkyMgr+0x2120`,
/// [`Weather::cloudiness`]) as `SkyMgr` writes them (`FUN_0365867c`, Wii U
/// v208): each swaying value of a look is `Min + (Max − Min)·(sin φ + 1)/2`
/// at its phase φ in `sway` (the value itself only behind a flag nothing
/// sets), the clear look (`PrCloudV0_N`) blended towards the cloudy one
/// (`PrCloudV1_N`) by the cloudiness. The shaders take the sines (`swayed`
/// in `clouds_common.wgsl`); the many shadowed materials that get the layers
/// do not sway them and need no updates for the phases. The texture blend
/// of this renderer's own pattern (`pattern.y`, from puffs to overcast)
/// follows the cloudiness; the game's textures blend by [`CloudBlend`].
fn layers(
    environment: &crate::daynight::Environment,
    cloudiness: f32,
    sway: &CloudSway,
) -> (CloudLayerParams, CloudLayerParams) {
    let c = cloudiness.clamp(0.0, 1.0);
    // Keep a whole number of repeats of drift per hour: shader time wraps
    // at an hour, and so the pattern does not jump then.
    let drift = |speed: f32, angle: f32, repeats_per_metre: f32| {
        let per_hour = (speed * 3600.0 * repeats_per_metre).round().max(1.0);
        Vec2::from_angle(angle).rotate(WIND.normalize()) * per_hour / 3600.0
    };
    let mix = |a: f32, b: f32| a + (b - a) * c;
    let layer = |index: usize| {
        let game = &environment.clouds.layers[index];
        let [clear, cloudy] = &game.looks;
        let shape = |l: &CloudLayer| [l.height, l.density, l.alpha_mul, l.alpha_threshold];
        let pattern = |l: &CloudLayer| [l.tex_scale, l.distortion];
        let (shape_clear, shape_cloudy) = (shape(clear), shape(cloudy));
        let (pattern_clear, pattern_cloudy) = (pattern(clear), pattern(cloudy));
        let centre = |i: usize| mix(shape_clear[i].centre(), shape_cloudy[i].centre());
        let scale = mix(clear.tex_scale.centre(), cloudy.tex_scale.centre()) / PATTERN_SPAN;
        let emboss = mix(clear.emboss_width, cloudy.emboss_width);
        // The upper layer drifts at an angle to the lower one.
        let angle = if index == 0 { game.wind_add } else { 0.0 };
        let halves = |sways: [Sway; 4], k: f32| Vec4::from_array(sways.map(|s| s.half_swing() * k));
        let pattern_swing = |sways: [Sway; 2], k: f32| {
            Vec4::new(
                sways[0].half_swing() * k / PATTERN_SPAN,
                0.0,
                sways[1].half_swing() * k,
                0.0,
            )
        };
        let phases = |look: usize| {
            let [height, distortion, density, alpha_mul, threshold, tex_scale] =
                sway.phases[index][look];
            (
                Vec4::new(height, density, alpha_mul, threshold),
                Vec4::new(tex_scale, 0.0, distortion, 0.0),
            )
        };
        let (shape_phase, pattern_phase) = phases(0);
        let (shape_phase_cloudy, pattern_phase_cloudy) = phases(1);
        CloudLayerParams {
            shape: Vec4::new(centre(0), centre(1), centre(2), centre(3)),
            pattern: Vec4::new(
                scale,
                c,
                mix(clear.distortion.centre(), cloudy.distortion.centre()),
                0.0,
            ),
            drift: drift(game.scroll_speed.abs() * DRIFT_PER_SCROLL, angle, scale)
                .extend(emboss.abs())
                .extend(if index == 0 { STRETCH.0 } else { STRETCH.1 }),
            shape_swing: halves(shape_clear, 1.0 - c),
            shape_phase,
            pattern_swing: pattern_swing(pattern_clear, 1.0 - c),
            pattern_phase,
            shape_swing_cloudy: halves(shape_cloudy, c),
            shape_phase_cloudy,
            pattern_swing_cloudy: pattern_swing(pattern_cloudy, c),
            pattern_phase_cloudy,
        }
    };
    (layer(0), layer(1))
}

/// What materials need under the clouds: the noise the sky's clouds are
/// made of (for the water that mirrors them), the game's cloud shadow
/// texture and their parameters. `params.sun` follows the main light for
/// the shaders that cannot see the scene's lights (the grass's vertex
/// shader).
#[derive(Resource, Clone)]
pub struct CloudShadows {
    pub noise: Handle<Image>,
    /// The cloud shadow's texture (`p_shadow_clouds`, [`upload_shadow_map`]);
    /// plain white, no shadow, until the game's is read.
    pub shadow_map: Handle<Image>,
    /// The look texture the cloud dome binds (`look::LookTexture`).
    pub look: Handle<Image>,
    pub params: CloudParams,
    /// Bumped whenever the layers or the shadow in `params` change (not
    /// for the sun).
    pub revision: u32,
}

/// The clouds in the sky this frame (`CloudShadows` follows them in steps).
#[derive(Resource, Clone, Default)]
pub struct CloudsNow(pub CloudParams);

impl CloudShadows {
    /// The shadows have caught up with the clouds in the sky.
    pub fn follows(&self, now: &CloudsNow) -> bool {
        !now.0.layers_differ(&self.params)
    }
}

impl Material for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    /// In front of the stars and the moon (see `SkyMaterial`), behind every
    /// other transparent thing.
    fn depth_bias(&self) -> f32 {
        -1.0e8
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

/// The dome the clouds are drawn on.
#[derive(Component)]
pub struct CloudDome;

fn spawn_clouds(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    shadows: Res<CloudShadows>,
    mut materials: ResMut<Assets<CloudMaterial>>,
) {
    let material = materials.add(CloudMaterial {
        noise: shadows.noise.clone(),
        params: shadows.params.clone(),
        look: shadows.look.clone(),
        light: CloudLight::default(),
        upper_base: shadows.noise.clone(),
        upper_base_blend: shadows.noise.clone(),
        upper_noise: shadows.noise.clone(),
        upper_noise_blend: shadows.noise.clone(),
        lower_base: shadows.noise.clone(),
        lower_base_blend: shadows.noise.clone(),
        lower_noise: shadows.noise.clone(),
        lower_noise_blend: shadows.noise.clone(),
    });
    commands.spawn((
        Name::new("clouds"),
        CloudDome,
        Mesh3d(meshes.add(Sphere::new(DOME_RADIUS).mesh().uv(48, 24))),
        MeshMaterial3d(material),
        Transform::default(),
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
        // The game draws its sky and clouds into the cube map (the
        // callback `FUN_0340b6cc`, docs/research/wiiu-deferred-shading.md);
        // the main camera's go through the reduced buffer (`reduced.rs`).
        bevy::camera::visibility::RenderLayers::layer(crate::cubemap::CUBE_LAYER),
    ));
}

/// What the clouds follow: the sky and the weather.
#[derive(SystemParam)]
pub struct SkyState<'w, 's> {
    sky: Option<Res<'w, Sky>>,
    environment: Option<Res<'w, Environment>>,
    weather: Option<Res<'w, Weather>>,
    time: Option<Res<'w, crate::daynight::TimeOfDay>>,
    sky_lut: Option<Res<'w, crate::look::LookSkyLut>>,
    textures: Option<Res<'w, CloudTextures>>,
    stage: Option<Res<'w, crate::climate::StageTimer>>,
    _marker: std::marker::PhantomData<&'s ()>,
}

/// Keeps the dome on the camera, shapes the layers like the game's sky and
/// lights them like the time of day: by the sun, or the moon at night, in the
/// game's cloud colours.
#[allow(clippy::too_many_arguments)]
pub fn update_clouds(
    real: Res<Time>,
    mut last_share: Local<f32>,
    (mut wind, mut scroll, mut sway): (
        Local<SkyWind>,
        Local<CloudScroll>,
        Local<Option<CloudSway>>,
    ),
    mut blend: Local<CloudBlend>,
    mut spots: Local<Option<DensitySpots>>,
    mut backlight: Local<MoonBacklight>,
    mut random: ResMut<GlobalRandom>,
    world_wind: Option<Res<crate::grass::GrassWind>>,
    epoch: Option<Res<crate::ready::SceneEpoch>>,
    cameras: Query<(&GlobalTransform, Option<&bevy::camera::Exposure>), crate::camera::MainCamera>,
    state: SkyState,
    (mut shadows, mut now): (ResMut<CloudShadows>, ResMut<CloudsNow>),
    mut domes: Query<(&mut Transform, &MeshMaterial3d<CloudMaterial>), With<CloudDome>>,
    mut materials: ResMut<Assets<CloudMaterial>>,
) {
    let new_scene = epoch.is_some_and(|epoch| epoch.is_changed());
    if new_scene && random.repeats {
        // Captures: every scene as if the game had just booted.
        *random = GlobalRandom::fixed();
        *sway = None;
    }
    // The sky manager is built once, like the game's at boot; its draws
    // come first.
    let random = &mut random.random;
    let sway = sway.get_or_insert_with(|| CloudSway::build(random));
    // A new scene shares its layers right away and starts the blend of
    // the cloud textures afresh, like the game's stage load.
    if new_scene {
        *last_share = f32::NEG_INFINITY;
        *blend = CloudBlend::default();
        *spots = Some(DensitySpots::reset(random));
        *backlight = MoonBacklight::default();
    }
    let spots = spots.get_or_insert_with(|| DensitySpots::reset(random));
    let (Ok((camera, exposure)), Ok((mut transform, material))) =
        (cameras.single(), domes.single_mut())
    else {
        return;
    };
    if transform.translation != camera.translation() {
        transform.translation = camera.translation();
    }
    let SkyState {
        sky: Some(sky),
        environment: Some(environment),
        weather,
        time,
        sky_lut,
        textures,
        stage,
        ..
    } = state
    else {
        return;
    };
    let cloudiness = weather.map_or_else(|| Weather::default().cloudiness, |w| w.cloudiness);
    // Worked on a copy: only a changed material is re-extracted and
    // re-bound (see the end).
    let handle = material.0.clone();
    let Some(mut material) = materials.get(&handle).cloned() else {
        return;
    };
    let hours = time.as_ref().map_or(12.0, |t| t.hours);
    let table = sky_lut.is_some_and(|lut| lut.0.is_some());
    let frames = real.delta_secs() * 30.0;
    // The world's wind; without its manager none (the game's outside the
    // field).
    let world = world_wind.map_or((0.0, Vec2::Y), |w| w.world());
    // The world manager's timer as the world's last frame left it.
    let snap = stage.is_some_and(|stage| stage.running());
    if snap {
        wind.take(world);
    } else {
        wind.advance(world, frames);
    }
    spots
        .shadow
        .advance(&wind, frames, real.elapsed_secs_wrapped());
    // `SkyMgr`'s order in a frame: the glow's factor, the layers' speeds,
    // spots and values
    // (`FUN_0365867c`; the renderer moves the textures by the speeds), then
    // the texture blend (`FUN_03659fa8`).
    let moon = time.as_ref().map_or(0, |t| t.moon_phase());
    if snap {
        backlight.take(moon);
    } else {
        backlight.advance(moon, frames);
    }
    scroll.advance(&environment, frames, &wind, spots.turns);
    spots.advance(&environment.clouds, frames, random);
    let rates = blend.advance(frames, random);
    material.light = game_light(
        &environment,
        &sky,
        hours,
        table,
        cloudiness,
        &scroll,
        rates,
        &spots,
        backlight.0,
    );
    if let Some(textures) = textures {
        let numbers = environment.clouds.layers.each_ref().map(|l| l.textures);
        textures.bind(&mut material, &numbers);
    }
    let params = &mut material.params;
    (params.upper, params.lower) = layers(&environment, cloudiness, sway);
    sway.advance(&environment.clouds, wind.factors().1, frames);
    light_clouds(params, &sky);
    shade_like_the_game(params, &environment, &sky, &spots.shadow);
    let ev100 = exposure.map_or(crate::daynight::DAY_EV100, |e| e.ev100);
    // Far away, low in the sky, they fade into the haze like the far land.
    let haze = crate::fog::haze(&environment, &sky, ev100);
    let glow = (haze.glow * haze.light).max_element();
    params.haze = haze.color.extend(glow);
    now.0 = params.clone();
    // Surfaces under the clouds get the same layers once they change enough
    // (not too often: every material that shows shadows is updated), and the
    // sun in steps for those that cannot see the lights.
    let now = real.elapsed_secs();
    let differs = params.layers_differ(&shadows.params) || params.shadow_differs(&shadows.params);
    if differs && (now - *last_share >= SHARE_INTERVAL) {
        *last_share = now;
        shadows.params.upper = params.upper.clone();
        shadows.params.lower = params.lower.clone();
        share_shadow(&mut shadows.params, params);
        shadows.revision = shadows.revision.wrapping_add(1);
    }
    // Their shadows fall along the main light.
    let towards = sky.main_light;
    if shadows.params.sun.truncate().angle_between(towards) > 0.1_f32.to_radians() {
        shadows.params.sun = towards.extend(0.0);
    }
    if materials.get(&handle) != Some(&material)
        && let Some(mut stored) = materials.get_mut(&handle)
    {
        *stored = material;
    }
}

// SI-SKY-11: InDoor and CloudShadowOff factors stay 1.
/// The game's cloud shadow (docs/research/wiiu-render-cpu.md, "Cloud
/// shadows"): its texture coordinates from the environment set's projector
/// ([`shadow_rows`]) and its strength, `1 − proj_shadow_off`. In the field
/// the game's strength is the palettes' blended `CloudShadowOnOff`
/// ([`EnvPalette::cloud_shadow_on`]) times factors the viewer cannot drive,
/// so they stay 1: a fade to 0 while the player touches an `InDoor` sensor
/// (`0x03677224`), one while a `ChangeWeatherTag` with `CloudShadowOff`
/// runs (`+0x2134`). The lightning flash fades it too. The
/// counters `+0x2180` (no shadow) and `+0x2184` (another drift) only
/// count down from 0 in v208. Without a projector the rows keep casting
/// none. The texture sails with the wind ([`ShadowDrift`] `drift`).
///
/// [`EnvPalette::cloud_shadow_on`]: asset_format::env::EnvPalette::cloud_shadow_on
fn shade_like_the_game(
    params: &mut CloudParams,
    environment: &Environment,
    sky: &Sky,
    drift: &ShadowDrift,
) {
    if let Some([u, v, w]) = shadow_rows(&environment.renderer, drift.base) {
        (params.shadow_u, params.shadow_v, params.shadow_w) = (u, v, w);
        (params.shadow.z, params.shadow.w) = drift.velocity.into();
    }
    // The lightning's flash fades it (`0x03658284`: `F·s·q·(1 − L)`).
    params.shadow.x = sky.palette.cloud_shadow_on.clamp(0.0, 1.0) * (1.0 - sky.flash);
}

/// Copies the cloud shadow of `from` into `to`, keeping whether `to` takes it.
pub fn share_shadow(to: &mut CloudParams, from: &CloudParams) {
    (to.shadow_u, to.shadow_v, to.shadow_w) = (from.shadow_u, from.shadow_v, from.shadow_w);
    to.shadow.x = from.shadow.x;
    (to.shadow.z, to.shadow.w) = (from.shadow.z, from.shadow.w);
}

/// The cloud shadow's texture coordinates of a world point, like the
/// game's `gsys_context[35..37]` (Wii U v208; `0x039ddb98`, `0x039dee9c`):
/// the projection shadow's texture matrix (`0x03b112b0`: scale
/// `0.5/bias_scale` with v flipped, moved by `0.5 + offset`) times the
/// projector's view-projection (`0x03a9b0c8`). Rows u and v, and w to
/// divide them by. `None` without the set's projector, or for what is not
/// read: an orthographic projector or a turned texture (`bias_rotate`).
fn shadow_rows(set: &asset_format::envset::EnvSet, offset: Vec2) -> Option<[Vec4; 3]> {
    let shadows = &set.shadows;
    let projector = set.objects.projector(&shadows.projector)?;
    if projector.proj_type != 0 || shadows.projection_bias_rotate != 0.0 {
        return None;
    }
    // sead's look-at (`0x03a9ac08`) and perspective (`0x03a9afd8`) are
    // OpenGL's; only x, y and w are used, which agree between depth ranges.
    let view = Mat4::look_at_rh(
        Vec3::from(projector.view_pos),
        Vec3::from(projector.view_at),
        Vec3::from(projector.view_up),
    );
    let projection = Mat4::perspective_rh_gl(
        projector.fovy.to_radians(),
        projector.aspect,
        projector.near,
        projector.far,
    );
    let clip = projection * view;
    let [scale_u, scale_v] = shadows.projection_bias_scale;
    let u = clip.row(0) * (0.5 / scale_u) + clip.row(3) * (0.5 + offset.x);
    let v = clip.row(1) * (-0.5 / scale_v) + clip.row(3) * (0.5 + offset.y);
    Some([u, v, clip.row(3)])
}

/// A 1×1 white shadow texture: no cloud shadow (no dump yet, or none).
fn no_shadow_image() -> Image {
    Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[255],
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// The game's cloud shadow texture once it is read (`daynight.rs` loads it
/// with the environment set).
#[derive(Resource)]
pub struct CloudShadowTexture(pub NamedTexture);

/// Puts the game's cloud shadow texture in [`CloudShadows::shadow_map`],
/// with all its levels, sampled like the projection shadow's
/// `agl::TextureSampler` (`aglprojsdw` constructor `0x03b114b4`, again at
/// `0x03b11414`, Wii U v208): repeating as the set says (`repeat`: wrap,
/// else clamp), linear within a level and the nearest level (mip filter
/// point: `+0x418` is 0 from the constructor on).
fn upload_shadow_map(
    mut commands: Commands,
    texture: Option<Res<CloudShadowTexture>>,
    shadows: Res<CloudShadows>,
    environment: Option<Res<Environment>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(texture) = texture else { return };
    commands.remove_resource::<CloudShadowTexture>();
    let Some(mut image) = texture_layer_image(&texture.0, 0) else {
        warn!(
            "cloud shadow: {:?} textures are not supported; no shadow",
            texture.0.format
        );
        return;
    };
    let repeat = environment.is_some_and(|e| e.renderer.shadows.projection_repeat);
    let address = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });
    info!(
        "cloud shadow: the game's {}, {}x{}, {} levels",
        texture.0.name, texture.0.width, texture.0.height, texture.0.mip_levels
    );
    let _ = images.insert(&shadows.shadow_map, image);
}

/// The environment's textures once read (`daynight.rs` loads them with the
/// environment set), in the renderer's order
/// ([`EnvSet::cloud_textures`](asset_format::envset::EnvSet::cloud_textures)).
#[derive(Resource)]
pub struct CloudTextureList(pub Vec<NamedTexture>);

/// The environment's textures on the GPU, in the renderer's order, with
/// their component selections: the textures the sky's cloud layers pick by
/// number (`CloudTextureNumbers`). Each is there twice, as the layer's two
/// kinds of sampler read it: `images` repeating for the base and its blend,
/// `mirrored` for the noise and its blend.
#[derive(Resource)]
pub struct CloudTextures {
    images: Vec<Handle<Image>>,
    mirrored: Vec<Handle<Image>>,
    channels: Vec<[u8; 4]>,
}

impl CloudTextures {
    /// Gives `material` each layer's four textures by the layers' numbers
    /// `numbers` (upper, lower) and says it is drawn with them.
    fn bind(&self, material: &mut CloudMaterial, numbers: &[CloudTextureNumbers; 2]) {
        let pick = |n: i32| asset_format::envset::EnvSet::cloud_texture_index(n, self.images.len());
        let mut layers = [[0usize; 4]; 2];
        for (layer, numbers) in layers.iter_mut().zip(numbers) {
            *layer = [
                numbers.base,
                numbers.base_blend,
                numbers.noise,
                numbers.noise_blend,
            ]
            .map(pick);
        }
        let image = |[base, base_blend, noise, noise_blend]: [usize; 4]| {
            [
                self.images[base].clone(),
                self.images[base_blend].clone(),
                self.mirrored[noise].clone(),
                self.mirrored[noise_blend].clone(),
            ]
        };
        [
            material.upper_base,
            material.upper_base_blend,
            material.upper_noise,
            material.upper_noise_blend,
        ] = image(layers[0]);
        [
            material.lower_base,
            material.lower_base_blend,
            material.lower_noise,
            material.lower_noise_blend,
        ] = image(layers[1]);
        // PS 449 reads x of the base textures and x and w of the noises.
        let channel = |component: usize| move |i: usize| f32::from(self.channels[i][component]);
        for (light, layer) in [&mut material.light.upper, &mut material.light.lower]
            .into_iter()
            .zip(layers)
        {
            light.channel_x = Vec4::from_array(layer.map(channel(0)));
            light.channel_w = Vec4::from_array(layer.map(channel(3)));
        }
        material.light.scale.z = 1.0;
    }
}

/// A cloud layer sampler as the game sets it (Wii U v208): the
/// `agl::TextureSampler` constructor (`0x03a82f34`, defaults `0x03b4ad34`)
/// filters linearly within a level with no anisotropy, and every step
/// `FUN_03a59734` sets the mip filter to point (`Cloud+0x4bd4`, 0 from
/// `AGL_ConstructCloudObject` on; linear only when it is set) and the
/// wrap: `address`.
fn cloud_sampler(address: ImageAddressMode) -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    })
}

/// Puts the environment's textures on the GPU once they are read, with all
/// their levels, sampled as `FUN_03a59734` sets the layers' samplers each
/// step: the base and its blend (`CloudParam+0x788`, `+0xbc0`) repeat, the
/// noise and its blend (`+0x904`, `+0xd3c`) mirror ([`cloud_sampler`]).
fn upload_cloud_textures(
    mut commands: Commands,
    list: Option<Res<CloudTextureList>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Some(list) = list else { return };
    commands.remove_resource::<CloudTextureList>();
    let mut textures = CloudTextures {
        images: Vec::new(),
        mirrored: Vec::new(),
        channels: Vec::new(),
    };
    for texture in &list.0 {
        let Some(mut image) = texture_layer_image(texture, 0) else {
            warn!(
                "cloud textures: {} is {:?}, not supported; the clouds keep this renderer's pattern",
                texture.name, texture.format
            );
            return;
        };
        image.sampler = cloud_sampler(ImageAddressMode::Repeat);
        let mut mirrored = image.clone();
        mirrored.sampler = cloud_sampler(ImageAddressMode::MirrorRepeat);
        textures.images.push(images.add(image));
        textures.mirrored.push(images.add(mirrored));
        textures.channels.push(texture.swizzle);
    }
    let names: Vec<&str> = list.0.iter().map(|t| t.name.as_str()).collect();
    info!("cloud textures: the game's {}", names.join(", "));
    commands.insert_resource(textures);
}

/// Least time between sharing changed layers with the surfaces under the
/// clouds, in real seconds.
const SHARE_INTERVAL: f32 = 1.0;

/// Gives the terrain and the models (and the tree models' dissolving
/// twins) the clouds' current layers, so that their shadows keep matching
/// the sky (grass and far trees follow `CloudShadows` themselves).
fn share_layers(
    shadows: Res<CloudShadows>,
    mut shared: Local<u32>,
    terrain: Option<ResMut<Assets<TerrainMaterial>>>,
    objects: Option<ResMut<Assets<ObjectMaterial>>>,
) {
    if *shared == shadows.revision {
        return;
    }
    *shared = shadows.revision;
    let share = |params: &mut CloudParams| {
        // Materials made without clouds keep taking none.
        if params.shadow.y > 0.0 {
            params.upper = shadows.params.upper.clone();
            params.lower = shadows.params.lower.clone();
            share_shadow(params, &shadows.params);
        }
    };
    // Only the materials whose layers differ are touched: a changed
    // material is re-extracted and re-bound with every mesh that uses it.
    if let Some(mut terrain) = terrain {
        let stale: Vec<_> = terrain
            .iter()
            .filter_map(|(id, material)| {
                let mut clouds = material.extension.clouds.clone();
                share(&mut clouds);
                (clouds != material.extension.clouds).then_some((id, clouds))
            })
            .collect();
        for (id, clouds) in stale {
            if let Some(mut material) = terrain.get_mut(id) {
                material.extension.clouds = clouds;
            }
        }
    }
    if let Some(mut objects) = objects {
        let stale: Vec<_> = objects
            .iter()
            .filter_map(|(id, material)| {
                let mut clouds = material.extension.clouds.clone();
                share(&mut clouds);
                (clouds != material.extension.clouds).then_some((id, clouds))
            })
            .collect();
        for (id, clouds) in stale {
            if let Some(mut material) = objects.get_mut(id) {
                material.extension.clouds = clouds;
            }
        }
    }
}

/// The layers' light from the sky: its direction and the palette's cloud
/// colours, read as layers of light: the base and the shadow colour
/// everywhere (the shadow colour is the sky's bluish fill), the highlight
/// added where the sun reaches, the backlight around the light. So a noon
/// cloud is white on top and blue-grey below, a sunset one orange on top and
/// dark below (a reading of the names; the sky shader is not decompiled).
fn light_clouds(params: &mut CloudParams, sky: &Sky) {
    let palette = &sky.palette;
    let rgb = |c: [f32; 3], k: f32| Vec3::from(c) * k;
    let shade = rgb(palette.cloud_base, palette.cloud_base_intensity)
        + rgb(palette.cloud_shadow, palette.cloud_shadow_intensity);
    let lit = shade + rgb(palette.cloud_highlight, palette.cloud_highlight_intensity);
    params.sun = sky.towards_light.extend(0.0);
    // The climate and bad weather tint the clouds like the light (`F`).
    let feature = sky.light_feature;
    params.lit = (lit * feature * CLOUD_BRIGHTNESS).extend(1.0);
    params.shade = (shade * feature * CLOUD_BRIGHTNESS).extend(0.0);
    // The rim of thin cloud against the light: bright (above the bloom's
    // threshold at sunset), in the backlight colour.
    params.backlight =
        Vec3::from(palette.cloud_backlight).extend(palette.cloud_backlight_power * RIM);
}

/// Brightness of the clouds' rims against the light per unit of the
/// palette's `BacklightPower` (fit to the game's sunset rims).
// SI-SKY-08: the no-dump cloud look is our own.
const RIM: f32 = 1.5;

/// Scales the palette's cloud colours to this renderer's brightness.
// SI-SKY-08: the no-dump cloud look is our own.
const CLOUD_BRIGHTNESS: f32 = 1.0;

/// A tileable noise texture with mips: R broad value noise (cloud cover),
/// G and B inverted cellular noise at two scales (puffs, billows), A fine
/// value noise (ragged edges).
fn noise_image() -> Image {
    let n = NOISE_SIZE;
    let mut rgba = vec![0u8; n * n * 4];
    let broad = |x: f32, y: f32| fbm_value(x, y, 4, 4);
    let puffs = |x: f32, y: f32| 0.65 * (1.0 - worley(x, y, 6)) + 0.35 * (1.0 - worley(x, y, 12));
    let billows = |x: f32, y: f32| 1.0 - worley(x, y, 18);
    let fine = |x: f32, y: f32| fbm_value(x, y, 24, 3);
    for y in 0..n {
        for x in 0..n {
            let (u, v) = (x as f32 / n as f32, y as f32 / n as f32);
            let texel = [broad(u, v), puffs(u, v), billows(u, v), fine(u, v)];
            for (c, value) in texel.iter().enumerate() {
                rgba[(y * n + x) * 4 + c] = (value.clamp(0.0, 1.0) * 255.0) as u8;
            }
        }
    }
    // A full mip chain, so the far layer does not shimmer.
    let (mut data, mut level, mut size, mut levels) = (Vec::new(), rgba, n as u32, 0);
    loop {
        data.extend_from_slice(&level);
        levels += 1;
        if size == 1 {
            break;
        }
        let (next, w, _) = crate::texture::downsample(&level, size, size);
        level = next;
        size = w;
    }
    Image {
        data: Some(data),
        data_order: TextureDataOrder::MipMajor,
        texture_descriptor: TextureDescriptor {
            label: Some("cloud noise"),
            size: Extent3d {
                width: n as u32,
                height: n as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 16,
            ..default()
        }),
        texture_view_descriptor: None,
        asset_usage: RenderAssetUsages::RENDER_WORLD,
        copy_on_resize: false,
    }
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
        ^ (y as u32).wrapping_mul(0xD816_3841)
        ^ seed.wrapping_mul(0xCB1A_B31F);
    h = (h ^ (h >> 13)).wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32
}

/// Value noise on a `cells × cells` lattice that wraps at the texture edge.
fn value_noise(u: f32, v: f32, cells: i32, seed: u32) -> f32 {
    let (x, y) = (u * cells as f32, v * cells as f32);
    let (x0, y0) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let at = |i: i32, j: i32| hash(i.rem_euclid(cells), j.rem_euclid(cells), seed);
    let top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * sx;
    let bottom = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * sx;
    top + (bottom - top) * sy
}

fn fbm_value(u: f32, v: f32, cells: i32, octaves: u32) -> f32 {
    let (mut sum, mut amplitude, mut norm) = (0.0, 0.5, 0.0);
    for octave in 0..octaves {
        sum += value_noise(u, v, cells << octave, octave + 1) * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
    }
    sum / norm
}

/// Distance to the nearest of one random point per cell, wrapping, scaled
/// to roughly 0–1.
fn worley(u: f32, v: f32, cells: i32) -> f32 {
    let (x, y) = (u * cells as f32, v * cells as f32);
    let (cx, cy) = (x.floor() as i32, y.floor() as i32);
    let mut nearest = f32::MAX;
    for j in -1..=1 {
        for i in -1..=1 {
            let (gx, gy) = (cx + i, cy + j);
            let (wx, wy) = (gx.rem_euclid(cells), gy.rem_euclid(cells));
            let px = gx as f32 + hash(wx, wy, 101);
            let py = gy as f32 + hash(wx, wy, 202);
            nearest = nearest.min(((px - x).powi(2) + (py - y).powi(2)).sqrt());
        }
    }
    (nearest / 1.1).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_scroll_along_the_sky_managers_wind_and_start_over_past_300() {
        let mut environment = Environment::fallback();
        let layer = &mut environment.clouds.layers[0];
        (layer.scroll_speed, layer.wind_add) = (-0.9, 0.0);
        layer.noise_add = [0.25, -0.5, 0.1, 0.3];
        environment.renderer.clouds.layers[0].noise_speed_master = 4.0;
        // Settled on the world's wind of 5 towards +X (east).
        let mut wind = SkyWind::default();
        for _ in 0..200 {
            wind.advance((5.0, Vec2::X), 1.0);
        }
        let (k1, k2) = wind.factors();
        assert!((k2 - 0.15).abs() < 1e-6 && (k1 - 0.000_15).abs() < 1e-9);
        let mut scroll = CloudScroll::default();
        // 30 frames: along +X, sin a = 1 and cos a = 0.
        scroll.advance(&environment, 30.0, &wind, [0.0, 0.0]);
        let base = Vec2::new(k1 * -0.9, 0.0) * 30.0;
        assert!((scroll.base[0] - base).abs().max_element() < 1e-7);
        let noise = Vec4::new(0.25, 0.1, -0.5, 0.0) * k2 * 4.0 * 0.002 * 30.0;
        assert!((scroll.noise[0] - noise).abs().max_element() < 1e-6);
        scroll.noise[0].x = 299.995;
        scroll.base[0].x = -299.999_99;
        scroll.advance(&environment, 30.0, &wind, [0.0, 0.0]);
        assert_eq!((scroll.noise[0].x, scroll.base[0].x), (0.0, 0.0));
    }

    #[test]
    fn the_sky_managers_wind_turns_and_speeds_up_by_its_steps() {
        let mut wind = SkyWind::default();
        assert_eq!(wind.factors().1, 0.2 / 10.0 * 0.3);
        // Towards −Z (north, π): at most 0.05 rad a frame; the speed from
        // 0.2 by exactly 0.1.
        wind.advance((5.0, Vec2::NEG_Y), 1.0);
        assert!((wind.angle - 0.05).abs() < 1e-6);
        assert!((wind.speed - 0.3).abs() < 1e-6);
        for _ in 0..200 {
            wind.advance((5.0, Vec2::NEG_Y), 1.0);
        }
        assert!((wind.direction() - Vec2::NEG_Y).length() < 1e-5);
        // Stronger than 10, k₂ stays at 0.3.
        for _ in 0..200 {
            wind.advance((20.0, Vec2::NEG_Y), 1.0);
        }
        assert_eq!(wind.factors(), (0.3 * 0.001, 0.3));
    }

    #[test]
    fn the_clouds_glow_fades_out_at_the_new_moon() {
        let mut backlight = MoonBacklight::default();
        // Exactly 0.01 a frame: some 100 frames to 0 (the last step snaps
        // where the float steps fall short), and back.
        backlight.advance(NEW_MOON, 1.0);
        assert!((backlight.0 - 0.99).abs() < 1e-6);
        let mut frames = 1;
        while backlight.0 > 0.0 {
            backlight.advance(NEW_MOON, 1.0);
            frames += 1;
        }
        assert!((100..=101).contains(&frames), "{frames}");
        backlight.advance(0, 1.0);
        assert!((backlight.0 - 0.01).abs() < 1e-6);
    }

    /// A world frame at `camera` with no player, in climate `climate`
    /// everywhere; climate 1 has the darkness's palette set.
    #[test]
    fn the_sky_wind_and_the_glow_can_take_their_targets_at_once() {
        let mut wind = SkyWind::default();
        wind.take((7.0, Vec2::NEG_X));
        assert_eq!((wind.speed, wind.direction().round()), (7.0, Vec2::NEG_X));
        let mut backlight = MoonBacklight::default();
        backlight.take(NEW_MOON);
        assert_eq!(backlight.0, 0.0);
    }

    #[test]
    fn the_cloud_shadow_sails_with_the_wind() {
        let mut wind = SkyWind::default();
        for _ in 0..200 {
            wind.advance((5.0, Vec2::X), 1.0);
        }
        let k1 = wind.factors().0;
        let mut drift = ShadowDrift::new(Vec2::new(0.25, 0.5));
        // A frame moves it by k₁·1.75 along the wind (+X).
        drift.advance(&wind, 1.0, 10.0);
        assert!((drift.offset - Vec2::new(0.25 + k1 * 1.75, 0.5)).length() < 1e-6);
        // The shaders' guess keeps up with it, up to whole repeats, over
        // many frames and past the wrap at 1.
        let mut time = 10.0;
        for _ in 0..10_000 {
            time += 1.0 / 30.0;
            drift.advance(&wind, 1.0, time);
            let guess = drift.base + drift.velocity * time - drift.offset;
            assert!((guess - guess.round()).abs().max_element() <= 0.001);
            assert!(drift.offset.abs().max_element() <= 1.0);
        }
        // The shaders' clock wraps: taken afresh.
        drift.advance(&wind, 1.0, 0.0);
        assert!((drift.base - drift.offset).length() < 1e-6);
    }

    #[test]
    fn the_lower_layer_turns_from_the_upper() {
        let mut clouds = fallback_sky_clouds();
        clouds.layers[0].wind_add = 0.5;
        clouds.layers[1].wind_add = 0.3;
        let wind = SkyWind::default();
        let [(upper, _), (lower, _)] = layer_speeds(&clouds, &wind, [1.0, -1.0]);
        let angle = |v: Vec2| v.x.atan2(v.y);
        // `ScrollSpd` < 0: the textures move against the angle's direction.
        assert!((angle(-upper) - 0.5).abs() < 1e-5);
        assert!((angle(-lower) - 0.2).abs() < 1e-5);
    }

    #[test]
    fn base_textures_blend_back_and_forth_like_the_game() {
        let mut blend = CloudBlend::default();
        let random = &mut GlobalRandom::fixed().random;
        // A frame: the reset's pass and one step of the upper layer.
        let [upper, lower] = blend.advance(1.0, random);
        assert!(upper > 0.0 && upper <= 0.001 && lower == 0.0);
        // The upper layer reaches 1, then the lower, then both return, the
        // legs at two passes a frame of 0.0002–0.001: 500–2500 frames each.
        let mut legs = Vec::new();
        let mut frames = 1;
        let mut previous = blend.state;
        while legs.len() < 5 {
            blend.advance(1.0, random);
            frames += 1;
            if blend.state != previous {
                legs.push((previous, blend.rates, frames));
                previous = blend.state;
                frames = 0;
            }
        }
        let states: Vec<u32> = legs.iter().map(|l| l.0).collect();
        assert_eq!(states, [1, 2, 3, 4, 1]);
        // The frame's second pass may already step the next leg.
        let near =
            |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() <= 0.001 && (a[1] - b[1]).abs() <= 0.001;
        assert!(near(legs[0].1, [1.0, 0.0]), "{:?}", legs[0]);
        assert!(near(legs[1].1, [1.0, 1.0]), "{:?}", legs[1]);
        assert!(near(legs[2].1, [0.0, 1.0]), "{:?}", legs[2]);
        assert!(near(legs[3].1, [0.0, 0.0]), "{:?}", legs[3]);
        assert!(legs.iter().all(|l| (500..=2501).contains(&l.2)));
    }

    #[test]
    fn layers_take_their_textures_by_number() {
        let mut images = Assets::<Image>::default();
        let textures = CloudTextures {
            images: (0..5).map(|_| images.add(no_shadow_image())).collect(),
            mirrored: (0..5).map(|_| images.add(no_shadow_image())).collect(),
            channels: vec![
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 0, 0, 0],
                [0, 1, 1, 1],
                [0, 0, 0, 5],
            ],
        };
        let noise = images.add(no_shadow_image());
        let mut material = CloudMaterial {
            noise: noise.clone(),
            params: CloudParams::default(),
            look: noise.clone(),
            light: CloudLight::default(),
            upper_base: noise.clone(),
            upper_base_blend: noise.clone(),
            upper_noise: noise.clone(),
            upper_noise_blend: noise.clone(),
            lower_base: noise.clone(),
            lower_base_blend: noise.clone(),
            lower_noise: noise.clone(),
            lower_noise_blend: noise,
        };
        // The field's numbers, and the constructor's for the lower layer:
        // 4 is the last of five and picks the first.
        let numbers = [
            CloudTextureNumbers {
                base: 0,
                base_blend: 2,
                noise: 1,
                noise_blend: 1,
            },
            CloudTextureNumbers::game_default(2),
        ];
        textures.bind(&mut material, &numbers);
        assert_eq!(material.upper_base, textures.images[0]);
        assert_eq!(material.upper_base_blend, textures.images[2]);
        // The noises through the mirroring samplers, like the game's.
        assert_eq!(material.upper_noise, textures.mirrored[1]);
        assert_eq!(material.lower_base, textures.images[2]);
        assert_eq!(material.lower_base_blend, textures.images[0]);
        assert_eq!(material.lower_noise, textures.mirrored[3]);
        // x and w of the two-channel noise are its red and green.
        assert_eq!(material.light.lower.channel_x, Vec4::ZERO);
        assert_eq!(
            material.light.lower.channel_w,
            Vec4::new(0.0, 0.0, 1.0, 1.0)
        );
        assert_eq!(material.light.scale.z, 1.0);
    }

    #[test]
    fn noise_tiles_seamlessly() {
        for i in 0..20 {
            let v = i as f32 / 20.0;
            assert!((value_noise(0.0, v, 8, 1) - value_noise(1.0, v, 8, 1)).abs() < 1e-5);
            assert!((worley(0.0, v, 6) - worley(1.0, v, 6)).abs() < 1e-4);
        }
    }

    #[test]
    fn layers_blend_their_looks_by_the_cloudiness_and_sway_by_the_sky_managers_k2() {
        let environment = Environment::fallback();
        let clouds = &environment.clouds;
        let start = [5.2, 4.4, 0.4, 2.1, 1.3, 3.3];
        let mut sway = CloudSway::starting_at(start);
        for cloudiness in [0.0, 0.25, 1.0] {
            let (upper, lower) = layers(&environment, cloudiness, &sway);
            for (layer, game) in [(&upper, &clouds.layers[0]), (&lower, &clouds.layers[1])] {
                let [clear, cloudy] = &game.looks;
                let mix = |a: f32, b: f32| a + (b - a) * cloudiness;
                // Centred between the looks' swings, each look swaying by
                // its share of the cloudiness.
                assert!(
                    (layer.shape.x - mix(clear.height.centre(), cloudy.height.centre())).abs()
                        < 1e-3
                );
                assert!(
                    (layer.shape.y - mix(clear.density.centre(), cloudy.density.centre())).abs()
                        < 1e-6
                );
                assert!(
                    (layer.shape_swing.x - clear.height.half_swing() * (1.0 - cloudiness)).abs()
                        < 1e-3
                );
                assert!(
                    (layer.shape_swing_cloudy.y - cloudy.density.half_swing() * cloudiness).abs()
                        < 1e-6
                );
                let per_hour = layer.drift.truncate().truncate().length() * 3600.0;
                assert!((per_hour - per_hour.round()).abs() < 1e-3 && per_hour >= 1.0);
            }
        }
        // A frame moves each phase by `SinSeedAdd`·k₂·t; the swaying value at
        // phase φ is the game's Min + (Max − Min)·(sin φ + 1)/2.
        let height = clouds.layers[0].looks[0].height;
        sway.advance(clouds, 0.15, 2.0);
        let phase = start[0] + height.rate * 0.15 * 2.0;
        let (upper, _) = layers(&environment, 0.0, &sway);
        assert!((upper.shape_phase.x - phase).abs() < 1e-6);
        let game = height.at(phase);
        assert!((upper.shape.x + upper.shape_swing.x * phase.sin() - game).abs() < 1e-2);
        // Past 2π it starts over.
        sway.phases[0][0][0] = std::f32::consts::TAU - 1e-4;
        sway.advance(clouds, 0.3, 1.0);
        assert!(sway.phases[0][0][0] < 0.01);
        assert_eq!(CloudParams::without_shadows().shadow.y, 0.0);
    }

    #[test]
    fn spots_grow_fade_and_move_like_the_game() {
        let mut clouds = fallback_sky_clouds();
        clouds.layers[1].density_spot = DensitySpot {
            range: 0.7,
            power: -0.45,
            speed: 0.001,
        };
        let random = &mut GlobalRandom::fixed().random;
        let mut spots = DensitySpots::reset(random);
        spots.advance(&clouds, 1.0, random);
        // Drawn in [−1, 1)², a first step of 0.001 at once.
        let first = spots.layers[1];
        assert!(first.at.abs().max_element() <= 1.0);
        assert!((first.strength - 0.001).abs() < 1e-6);
        let spot = spots.uniform(&clouds.layers[1], 1);
        assert!((spot.z - 0.7).abs() < 1e-6 && (spot.w + 0.45 * 0.001).abs() < 1e-7);
        // Up to 1 in 1000 frames, back to 0 in 1000 more (one more where the
        // sum of the float steps falls short), then elsewhere.
        let leg = |spots: &mut DensitySpots, random: &mut SeadRandom, from: u8| {
            let mut frames = 0;
            while spots.layers[1].state == from {
                spots.advance(&clouds, 1.0, random);
                frames += 1;
            }
            frames
        };
        assert!((999..=1000).contains(&leg(&mut spots, random, 1)));
        assert_eq!((spots.layers[1].state, spots.layers[1].strength), (2, 1.0));
        assert!((1000..=1001).contains(&leg(&mut spots, random, 2)));
        assert_eq!((spots.layers[1].state, spots.layers[1].strength), (0, 0.0));
        spots.advance(&clouds, 1.0, random);
        assert_ne!(spots.layers[1].at, first.at);
        // No power, no change: the upper layer keeps m = 1.
        assert_eq!(spots.uniform(&clouds.layers[0], 0).w, 0.0);
    }

    #[test]
    fn cloud_shadow_is_projected_from_high_above() {
        use asset_format::envset::{EnvSet, Projector};
        // The field's projector and projection shadow (master_field.baglenv,
        // common.bgsdw).
        let mut set = EnvSet::fallback();
        set.objects.projectors.push(Projector {
            name: "shadowTex_Projector".into(),
            proj_type: 0,
            view_pos: [0.0, 8000.0, 0.0],
            view_at: [0.0, 0.0, 0.1],
            view_up: [0.0, 0.0, 1.0],
            near: 1.0,
            far: 10_000.0,
            aspect: 1.0,
            fovy: 5.0,
            height: 1000.0,
        });
        set.shadows.projector = "shadowTex_Projector".into();
        set.shadows.projection_bias_scale = [1.0, 1.0];
        let [u, v, w] = shadow_rows(&set, Vec2::new(0.25, 0.1)).unwrap();
        let uv = |p: Vec3| {
            let p = p.extend(1.0);
            Vec2::new(u.dot(p), v.dot(p)) / w.dot(p)
        };
        // Right below the projector: the texture's centre, moved by the offset.
        let centre = uv(Vec3::ZERO);
        assert!(centre.distance(Vec2::new(0.75, 0.6)) < 1e-3, "{centre}");
        // The 5° view spans 2·8000·tan 2.5° ≈ 698.6 m at sea level: one
        // repeat. East runs to smaller u, south (+Z) to smaller v.
        let half = 8000.0 * 2.5_f32.to_radians().tan();
        assert!(uv(Vec3::new(half, 0.0, 0.0)).distance(Vec2::new(0.25, 0.6)) < 1e-3);
        assert!(uv(Vec3::new(0.0, 0.0, half)).distance(Vec2::new(0.75, 0.1)) < 1e-3);
        // Higher up the view is narrower: halfway up, the same step is twice
        // as far across the texture.
        assert!(uv(Vec3::new(half, 4000.0, 0.0)).distance(Vec2::new(-0.25, 0.6)) < 1e-3);
        // Not read: an orthographic projector.
        set.objects.projectors[0].proj_type = 1;
        assert!(shadow_rows(&set, Vec2::ZERO).is_none());
    }

    #[test]
    fn noise_image_has_a_full_mip_chain() {
        let image = noise_image();
        assert_eq!(image.texture_descriptor.mip_level_count, 9);
        let expected: usize = (0..9).map(|l| (NOISE_SIZE >> l).pow(2) * 4).sum();
        assert_eq!(image.data.as_ref().unwrap().len(), expected);
    }
}
