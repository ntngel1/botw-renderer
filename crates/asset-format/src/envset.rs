//! Mirror of `botw-formats::envset`'s types and their runtime methods; the
//! parsing stays there. `bake` serializes the parsed values to RON, which
//! these deserialize.
//!
//! The renderer's base environment: `Env/env.sgenvb` in `Pack/Bootup.pack`.
//!
//! A SARC of AAMP documents that set up the game's `agl` renderer: light and
//! fog objects (`envobj/*.baglenv`), post effects (`postfx/*`), light maps
//! (`envobj/common.bagllmap`) and shadows (`postfx/*.bgsdw`). The time-of-day
//! palettes (`crate::env`) override the lights and fogs every frame; the
//! rest (colour correction, light-map curves and rim light, SSAO) stays as
//! set here. Read here, for the open field:
//!
//! - `envobj/master_field.baglenv`: the main light `dir_main`, the
//!   hemisphere light `hemi_inner`, the fogs `fog_scatter`, `fog_world`,
//!   `fog_inner` and the projectors ([`EnvObjects`]);
//! - `postfx/master_field.baglccr`: colour correction ([`ColorCorrection`]);
//! - `postfx/master_field.baglblm`: bloom ([`Bloom`]);
//! - `postfx/master_field.bksky`: the sky's scattering and its fogs ([`SkyScatter`]);
//! - `postfx/master_field.baglclwd`: the sky's clouds as drawn ([`CloudDome`]);
//! - `envobj/common.bagllmap`: light maps — the response curves (toon
//!   ramps) and rim light of each shading kind ([`LightMaps`]);
//! - `postfx/common.bgsdw`: shadows and SSAO ([`Shadows`]);
//! - `env.bgenv`: the index of the sets; the field's (`Master_Field`) names
//!   the texture of the projected cloud shadow ([`EnvSet::cloud_shadow_texture`]).
//!
//! The `agl` classes that use these are not decompiled (`lib/agl` only
//! declares them), so what a value does is read from its name. Parameter
//! names are CRC32 hashes in the files; the names used here were matched
//! against them.

/// The pack holding the environment set.
pub const ENV_PACK: &str = "Pack/Bootup.pack";
/// The environment set inside [`ENV_PACK`].
pub const ENV_SET: &str = "Env/env.sgenvb";

/// Documents read from the set.
pub const LIGHTS: &str = "envobj/master_field.baglenv";
pub const COLOR_CORRECTION: &str = "postfx/master_field.baglccr";
pub const BLOOM: &str = "postfx/master_field.baglblm";
pub const SKY: &str = "postfx/master_field.bksky";
pub const CLOUDS: &str = "postfx/master_field.baglclwd";
pub const LIGHT_MAPS: &str = "envobj/common.bagllmap";
pub const SHADOWS: &str = "postfx/common.bgsdw";
/// The index of the environment sets and the field's set in it.
pub const SET_INDEX: &str = "env.bgenv";
pub const FIELD_SET: &str = "Master_Field";
/// The environment's texture resource (`nw4f_bin` `lump` of `env.bgenv`:
/// `CloudTexture02`–`04`, `indwp_ia4_00`, `p_shadow_clouds`).
pub const TEXTURES: &str = "collect.genvres";
/// The `signature` of the projected shadow's texture among a set's
/// references (`p_shadow_clouds` in `collect.genvres` for the field).
pub const PROJECTION_TEXTURE: &str = "prjshd";

/// What `Env/env.sgenvb` sets up for the field. Parts missing from the set
/// keep hand-picked values ([`EnvSet::fallback`]).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct EnvSet {
    pub objects: EnvObjects,
    pub color: ColorCorrection,
    pub bloom: Bloom,
    pub sky: SkyScatter,
    pub clouds: CloudDome,
    pub light_maps: LightMaps,
    pub shadows: Shadows,
    /// The field set's texture of the projected shadow (the clouds'), if
    /// the index names one; [`EnvSet::load_texture`] reads it.
    pub cloud_shadow_texture: Option<TextureRef>,
    /// The documents that were found and read (the rest are fallbacks).
    #[serde(skip)]
    pub read: Vec<&'static str>,
}

impl EnvSet {
    /// Hand-picked values in the same shape, for when there is no dump.
    // SI-FMT-06: no-dump EnvSet fallback values are hand-picked.
    pub fn fallback() -> Self {
        Self {
            objects: EnvObjects::fallback(),
            color: ColorCorrection::fallback(),
            bloom: Bloom::fallback(),
            sky: SkyScatter::fallback(),
            clouds: CloudDome::fallback(),
            light_maps: LightMaps::fallback(),
            shadows: Shadows::fallback(),
            cloud_shadow_texture: None,
            read: Vec::new(),
        }
    }

    /// Which entry of a list of `count` cloud textures ([`Self::cloud_textures`])
    /// the texture number `number` picks: `FUN_03a59bec` (Wii U v208) takes
    /// entry 0 unless the number (unsigned) is below `count − 1`.
    pub fn cloud_texture_index(number: i32, count: usize) -> usize {
        let number = number as u32 as usize;
        if count > 0 && number < count - 1 {
            number
        } else {
            0
        }
    }
}

/// A texture a set refers to (`env.bgenv`, `set_array/N/refer`): a BFRES
/// file of the environment SARC and the texture's name in it.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct TextureRef {
    pub file: String,
    pub name: String,
}

/// The main light (`dir_main`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct DirectionalLight {
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    /// Light on faces turned away (`BacksideColor`; black in the field).
    pub backside: [f32; 3],
    pub intensity: f32,
    /// Direction the light travels (not towards the light).
    pub direction: [f32; 3],
}

/// The hemisphere light (`hemi_inner`): light from the sky above and the
/// ground below, blended by the normal's direction.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct HemisphereLight {
    pub sky: [f32; 3],
    pub ground: [f32; 3],
    pub intensity: f32,
    /// The sky's direction (+Y).
    pub direction: [f32; 3],
}

/// A distance fog (`fog_scatter`, `fog_world`, `fog_inner`): colour (alpha:
/// strength) between `start` and `end` metres.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Fog {
    pub name: String,
    pub start: f32,
    pub end: f32,
    pub color: [f32; 4],
}

/// The light and fog objects of `master_field.baglenv`.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct EnvObjects {
    pub main_light: DirectionalLight,
    pub hemisphere: HemisphereLight,
    pub fogs: Vec<Fog>,
    /// The enabled projectors (`Projector`), which projected shadows look
    /// through (`shadowTex_Projector`: the clouds' shadow).
    pub projectors: Vec<Projector>,
}

/// A projector (`agl` env object `Projector`; the Wii U v208 constructor
/// `0x03a9a6c0` gives the defaults used for missing values, except
/// `proj_type`, its argument, 0 here): a camera that
/// projects a texture onto the world. `proj_type` 0 is a perspective
/// (`near`, `far`, `fovy` in degrees, `aspect`), 1 an orthographic view of
/// height `aspect·height` (`0x03a9afd8`); the view looks from `view_pos` at
/// `view_at` with `view_up` up (`0x03a9ac08`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Projector {
    pub name: String,
    pub proj_type: i32,
    pub view_pos: [f32; 3],
    pub view_at: [f32; 3],
    pub view_up: [f32; 3],
    pub near: f32,
    pub far: f32,
    pub aspect: f32,
    pub fovy: f32,
    pub height: f32,
}

impl EnvObjects {
    fn fallback() -> Self {
        Self {
            main_light: DirectionalLight {
                diffuse: [1.0, 0.92, 0.75],
                specular: [1.0; 3],
                backside: [0.0; 3],
                intensity: 9.0,
                direction: [0.7, -0.3, 0.65],
            },
            hemisphere: HemisphereLight {
                sky: [0.85, 0.85, 0.95],
                ground: [0.45, 0.35, 0.25],
                intensity: 1.0,
                direction: [0.0, 1.0, 0.0],
            },
            fogs: Vec::new(),
            projectors: Vec::new(),
        }
    }

    /// The projector named `name`.
    pub fn projector(&self, name: &str) -> Option<&Projector> {
        self.projectors.iter().find(|p| p.name == name)
    }

    /// The fog named `name` (`fog_scatter`, `fog_world`, `fog_inner`).
    pub fn fog(&self, name: &str) -> Option<&Fog> {
        self.fogs.iter().find(|f| f.name == name)
    }
}

/// Colour correction (`baglccr`, `color_correction`): hue shift,
/// saturation, brightness and gamma, per-channel level curves, and the
/// "toy camera" stage.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ColorCorrection {
    pub enable: bool,
    pub hue: f32,
    pub saturation: f32,
    pub brightness: f32,
    pub gamma: f32,
    /// Level curves (`level`, four curves; identity in the field).
    pub levels: Vec<Curve>,
    pub toycam_enable: bool,
    pub toycam_saturation: [f32; 2],
    pub toycam_brightness: f32,
    pub toycam_contrast: f32,
    pub toycam_mul_color: [f32; 3],
}

impl ColorCorrection {
    fn fallback() -> Self {
        Self {
            enable: true,
            hue: 0.0,
            saturation: 1.15,
            brightness: 1.0,
            gamma: 1.0,
            levels: Vec::new(),
            toycam_enable: false,
            toycam_saturation: [1.0; 2],
            toycam_brightness: 1.0,
            toycam_contrast: 1.0,
            toycam_mul_color: [1.0; 3],
        }
    }
}

/// The base bloom (`baglblm`, `bloom`). The palettes scale it per time of
/// day (`crate::env::PaletteBloom`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Bloom {
    pub enable: bool,
    pub intensity: f32,
    /// Luminance where the glow starts (`threshhold`) and over how much it
    /// ramps in (`threshold_range`).
    pub threshold: f32,
    pub threshold_range: f32,
    /// The brightest luminance that blooms (`clamped_luminance`), if clamping
    /// is on (`enable_clamped_luminance`).
    pub clamped_luminance: Option<f32>,
    /// `color1`–`color4`: tints of the blur levels (alpha: weight).
    pub colors: [[f32; 4]; 4],
}

impl Bloom {
    fn fallback() -> Self {
        Self {
            enable: true,
            intensity: 0.1,
            threshold: 2.0,
            threshold_range: 0.1,
            clamped_luminance: Some(64.0),
            colors: [[1.0; 4]; 4],
        }
    }
}

/// The sky's clouds as the renderer draws them (`aglclwd`, the
/// `agl::fx::Cloud` of Wii U v208: the `cloud` program's PS 449 / VS 448,
/// `docs/research/wiiu-sky-resources.md`). `ksys::world::SkyMgr` writes
/// the layers' shape, relief and speeds over these every frame
/// (`crate::env::SkyClouds`, `FUN_0365867c`); what is read here and it does
/// not write stays as set. The document is a snapshot of a running game:
/// its speeds are what `SkyMgr` wrote at that moment.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct CloudDome {
    /// `mCloudColorScale`: how much brighter the clouds' palette colours are
    /// than the sky table they are blended with.
    pub color_scale: f32,
    /// `CloudParam0` and `CloudParam2`: the upper and the lower layer
    /// (`PrCloud_0`, `PrCloud_2` of `SkyMgr`; `CloudParam1` is off in the
    /// field).
    pub layers: [CloudDomeLayer; 2],
}

/// One layer of [`CloudDome`] (`CloudParamN`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct CloudDomeLayer {
    /// `mSkyScale`: the dome's radius across (metres; `mSkyHeight`, its
    /// height, comes from the weather).
    pub sky_scale: f32,
    /// `mScatterHeight`, `mScatterAmb`: how far the clouds take their own
    /// colour over the sky's behind them, and how much of the sky's they keep
    /// at most.
    pub scatter_height: f32,
    pub scatter_ambient: f32,
    /// `mBacklightParam0`, `mBacklightParam1`: how the glow into the light
    /// fades with the clouds' opacity.
    pub backlight_param0: f32,
    pub backlight_param1: f32,
    /// How the texture is drawn out towards the dome's rim (`mFarUVMul`,
    /// `mFarUVPow`).
    pub far_uv_mul: f32,
    pub far_uv_pow: f32,
    /// How the density and the opacity change towards the rim: from
    /// `Start` over `End` of the dome's radius, by up to `Power`
    /// (`mFarDensityChg*`, `mFarAlphaChg*`).
    pub far_density: [f32; 3],
    pub far_alpha: [f32; 3],
    /// The two noises that warp the cloud texture: how often each repeats
    /// against it (`mNoiseScale1`, `2`), how much each counts
    /// (`mNoiseDensity1`, `2`), how fast each drifts (`mNoiseSpeed1X`, `1Y`,
    /// `2X`, `2Y`, times `mNoiseSpeedMaster`).
    pub noise_scale: [f32; 2],
    pub noise_density: [f32; 2],
    pub noise_speed: [f32; 4],
    pub noise_speed_master: f32,
    /// How fast the base texture scrolls, in the dome's units a frame
    /// (`mBaseTexScrollSpdX`, `Y`).
    pub base_scroll_speed: [f32; 2],
    /// How the warp grows towards the rim (`mFarDistotionChgStart`, `End`,
    /// `Power`).
    pub far_distortion: [f32; 3],
}

impl CloudDome {
    /// The constructors' values (`agl::fx::Cloud` `0x03a57340`, its
    /// `CloudParam` `0x03a55724` in U-King.rpx v208).
    fn fallback() -> Self {
        let layer = CloudDomeLayer {
            sky_scale: 12_000.0,
            scatter_height: 3.0,
            scatter_ambient: 0.1,
            backlight_param0: 0.1,
            backlight_param1: 0.15,
            far_uv_mul: 0.8,
            far_uv_pow: 8.0,
            far_density: [0.9, 0.1, -0.2],
            far_alpha: [0.8, 0.5, -0.4],
            noise_scale: [4.0, 6.0],
            noise_density: [1.0, 0.5],
            noise_speed: [-1.0, -0.6, -1.2, -1.8],
            noise_speed_master: 1.0,
            base_scroll_speed: [0.0001, 0.001],
            far_distortion: [0.4, 0.9, 3.5],
        };
        Self {
            color_scale: 1.0,
            layers: [layer.clone(), layer],
        }
    }
}

/// The sky's scattering (`bksky`, `sky`): the physical sky's inputs and its
/// two fogs over the ground — the scattering fog (`scatter_fog_*`) and an
/// "ad hoc" fog (`adhoc_fog_*`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct SkyScatter {
    /// Sun colour (alpha: brightness) and the ground's colour under the sky
    /// (`sky_postfx_sky` draws it below the horizon where alpha is 1; its
    /// `cGroundColor.a` is 1 minus this alpha).
    pub sun_color: [f32; 4],
    pub ground_color: [f32; 4],
    pub rayleigh_base_height: f32,
    pub mie_base_height: f32,
    pub mie_scattering: f32,
    pub mie_asymmetry: f32,
    /// Factors the sky is rendered with (`*_rendering`).
    pub rayleigh_amplifier: f32,
    pub mie_asymmetry_rendering: f32,
    pub mie_amplifier: f32,
    /// Scattering fog: near and far distance (metres), density, attenuation,
    /// horizontal spread.
    pub scatter_fog_near: f32,
    pub scatter_fog_far: f32,
    pub scatter_fog_density: f32,
    pub scatter_fog_attenuation: f32,
    pub scatter_fog_horizontal: f32,
    /// Ad hoc fog: near and far distance, how strongly it fades the ground
    /// and the sky, colour.
    pub adhoc_fog_near: f32,
    pub adhoc_fog_far: f32,
    pub adhoc_fog_attenuation_ground: f32,
    pub adhoc_fog_attenuation_sky: f32,
    pub adhoc_fog_color: [f32; 3],
    pub env_map_amplifier: f32,
}

impl SkyScatter {
    fn fallback() -> Self {
        Self {
            sun_color: [1.0, 0.9, 0.7, 16.0],
            ground_color: [0.5, 0.4, 0.3, 1.0],
            rayleigh_base_height: 20.0,
            mie_base_height: 2.0,
            mie_scattering: 0.002,
            mie_asymmetry: 0.8,
            rayleigh_amplifier: 1.0,
            mie_asymmetry_rendering: 0.8,
            mie_amplifier: 8.0,
            scatter_fog_near: 0.0,
            scatter_fog_far: 20_000.0,
            scatter_fog_density: 0.9,
            scatter_fog_attenuation: 10.0,
            scatter_fog_horizontal: 2.0,
            adhoc_fog_near: 0.0,
            adhoc_fog_far: 5000.0,
            adhoc_fog_attenuation_ground: 4.0,
            adhoc_fog_attenuation_sky: 0.4,
            adhoc_fog_color: [0.5, 0.4, 0.3],
            env_map_amplifier: 2.0,
        }
    }
}

/// A `sead` curve (`sead::hostio::Curve`): points interpolated by kind.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Curve {
    pub kind: CurveKind,
    /// The used floats (`numUse` of the 30 stored).
    pub values: Vec<f32>,
}

/// `sead::hostio::CurveType`; the light maps and colour levels use the 2D
/// kinds, whose values are points along x.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveKind {
    /// `(x, y)` pairs, straight lines between them.
    Linear2D,
    /// `(x, y, slope)` triples, Hermite segments between them.
    Hermit2D,
    /// `(x, y)` pairs, holding each y until the next x.
    Step2D,
    /// Another `CurveType` (the number), not evaluated.
    Other(u32),
}

impl Curve {
    /// The curve at `t`, like `sead::hostio::curveLinear2D_`,
    /// `curveHermit2D_` and `curveStep2D_` (held at the end points); `t`
    /// itself for kinds that are not evaluated or curves without points.
    pub fn eval(&self, t: f32) -> f32 {
        let f = &self.values;
        let stride = match self.kind {
            CurveKind::Linear2D | CurveKind::Step2D => 2,
            CurveKind::Hermit2D => 3,
            // SI-FMT-07: bloom field names and curve kinds are our reading.
            CurveKind::Other(_) => return t,
        };
        let n = f.len() / stride;
        if n == 0 {
            return t;
        }
        if t <= f[0] {
            return f[1];
        }
        let last = stride * (n - 1);
        if t >= f[last] {
            return f[last + 1];
        }
        for i in 0..n - 1 {
            let j = stride * i;
            if f[j + stride] > t {
                let x = (t - f[j]) / (f[j + stride] - f[j]);
                return match self.kind {
                    CurveKind::Linear2D => f[j + 1] + x * (f[j + 3] - f[j + 1]),
                    CurveKind::Step2D => f[j + 1],
                    // The game's Hermite does not scale the slopes by the
                    // segment's width.
                    _ => {
                        let (x2, x3) = (x * x, x * x * x);
                        (2.0 * x3 - 3.0 * x2 + 1.0) * f[j + 1]
                            + (-2.0 * x3 + 3.0 * x2) * f[j + 4]
                            + (x3 - x2) * f[j + 5]
                            + (x3 - 2.0 * x2 + x) * f[j + 2]
                    }
                };
            }
        }
        f[last + 1]
    }

    /// `size` samples of the curve over 0–1 (a lookup table for a shader).
    pub fn bake(&self, size: usize) -> Vec<f32> {
        (0..size)
            .map(|i| self.eval(i as f32 / (size.max(2) - 1) as f32))
            .collect()
    }
}

/// The rim light of a light map (`rim_*`): a glow along silhouettes lit by
/// `light` (`rim_light_ref`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Rim {
    pub enable: bool,
    pub light: String,
    /// Strength (`rim_effect`), width, angle and exponent (`rim_pow`).
    pub strength: f32,
    pub width: f32,
    pub angle: f32,
    pub power: f32,
}

/// One light a light map takes in (`env_obj_ref_array`): which light, how
/// its angle is turned into light (`lut`, a curve of [`LightMaps::curves`];
/// `lut_mip1` for the blurred level), how strongly (`effect`) and the
/// specular exponent (`pow`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct LightMapInput {
    /// `AmbientLight`, `DirectionalLight` or `HemisphereLight`.
    pub kind: String,
    /// The light's name (`dir_main`), empty for none.
    pub light: String,
    pub calc_type: i32,
    pub lut: String,
    pub lut_mip1: String,
    pub effect: f32,
    pub power: f32,
    pub power_mip_max: f32,
    pub mip0: bool,
    pub mip1: bool,
}

/// A light map: how one kind of surface is lit (`Effect_Actor` for
/// characters, `Effect_Leaf`, `Effect_Diffuse`…).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct LightMap {
    pub name: String,
    pub rim: Rim,
    pub inputs: Vec<LightMapInput>,
}

impl LightMap {
    /// The main light's input on the sharp level (`dir_main`, mip 0).
    pub fn main_light(&self) -> Option<&LightMapInput> {
        self.inputs
            .iter()
            .find(|i| i.kind == "DirectionalLight" && i.light == "dir_main" && i.mip0)
    }
}

// SI-FMT-07: bloom field names and curve kinds are our reading.
/// The light maps (`common.bagllmap`): named response curves (`lut_param`:
/// `Lambert`, `Half-Lambert`, `toon_1114`, `toon_skin_1114`…) and the maps
/// that use them. What x of a curve is (probably N·L remapped to 0–1) is not
/// decompiled; the curves named Lambert, Half-Lambert and Hemisphere are
/// empty in the data — the renderer seems to compute those itself.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct LightMaps {
    pub curves: Vec<(String, Curve)>,
    pub maps: Vec<LightMap>,
}

/// Name of the characters' shading curve and of the skin's.
pub const TOON: &str = "toon_1114";
pub const TOON_SKIN: &str = "toon_skin_1114";
/// Name of the light map characters use.
pub const ACTOR_MAP: &str = "Effect_Actor";

impl LightMaps {
    fn fallback() -> Self {
        // Hand-picked hard steps and a rim like the characters' reference.
        let step = |at: f32| Curve {
            kind: CurveKind::Linear2D,
            values: vec![at - 0.01, 0.0, at, 1.0],
        };
        let rim = |enable: bool| Rim {
            enable,
            light: "main_dir".into(),
            strength: 1.0,
            width: 1.5,
            angle: 0.4,
            power: 3.0,
        };
        let main = |lut: &str| LightMapInput {
            kind: "DirectionalLight".into(),
            light: "dir_main".into(),
            calc_type: 1,
            lut: lut.into(),
            lut_mip1: lut.into(),
            effect: 1.0,
            power: 1.0,
            power_mip_max: 1.0,
            mip0: true,
            mip1: false,
        };
        Self {
            curves: vec![(TOON.into(), step(0.55)), (TOON_SKIN.into(), step(0.6))],
            maps: vec![
                LightMap {
                    name: "Effect_Diffuse".into(),
                    rim: rim(true),
                    inputs: vec![main("Lambert")],
                },
                LightMap {
                    name: ACTOR_MAP.into(),
                    rim: rim(false),
                    inputs: vec![main(TOON)],
                },
            ],
        }
    }

    /// The curve named `name`.
    pub fn curve(&self, name: &str) -> Option<&Curve> {
        self.curves.iter().find(|(n, _)| n == name).map(|(_, c)| c)
    }

    /// The light map named `name`.
    pub fn map(&self, name: &str) -> Option<&LightMap> {
        self.maps.iter().find(|m| m.name == name)
    }
}

/// Shadows (`common.bgsdw`): the light casting depth shadows, the cloud
/// shadow projection and screen-space ambient occlusion.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Shadows {
    /// The light the depth shadows come from (`dir_main`).
    pub light: String,
    /// Strength of the projected shadow texture (`projection_shadow_0`,
    /// `density`) and the projector it uses (`shadowTex_Projector`, the
    /// clouds' shadows seen from 8 km up).
    pub projection_density: f32,
    pub projector: String,
    /// How the texture lies in the projector's view (`bias_scale`,
    /// `bias_rotate` in degrees) and whether it repeats (`repeat`). Its
    /// offset (`bias_trans`) and drift (`anim_trans_vel`) are the
    /// environment manager's every frame (`0x03657fac`), not these.
    pub projection_bias_scale: [f32; 2],
    pub projection_bias_rotate: f32,
    pub projection_repeat: bool,
    pub ssao: Ssao,
}

/// SSAO (`ssao_parameter`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct Ssao {
    pub enable: bool,
    pub radius: f32,
    /// Distance it fades out at (`ao_far`) and whether it does (`enable_dist_attn`, `dist_attn`).
    pub far: f32,
    pub distance_attenuation: Option<f32>,
    pub density: f32,
    pub mix_rate: f32,
}

impl Shadows {
    fn fallback() -> Self {
        Self {
            light: "dir_main".into(),
            projection_density: 1.0,
            projector: String::new(),
            projection_bias_scale: [1.0, 1.0],
            projection_bias_rotate: 0.0,
            projection_repeat: false,
            ssao: Ssao {
                enable: true,
                radius: 0.05,
                far: 200.0,
                distance_attenuation: Some(1.0),
                density: 3.0,
                mix_rate: 0.8,
            },
        }
    }
}
