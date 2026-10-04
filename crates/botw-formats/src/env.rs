//! Time-of-day environment parameters: the colours and light the game uses
//! at each part of the day.
//!
//! They live in `WorldMgr/normal.bwinfo` inside `Pack/TitleBG.pack`, an AAMP
//! document (`winfo`) the game's world manager loads at boot. The parts read
//! here:
//! - `EnvPalette_N`: one set of colours each — main light (`BgDifColor` ×
//!   `BgDifIntencity`), fog, the sky's sun colour and scattering, the cloud
//!   layer's colours.
//! - `EnvAttribute_N`: a palette set; `PaletteSel00`–`07` pick a palette for
//!   each of the game's eight sky divisions of the day (see [`sky_division`]).
//!   Set 0 is the ordinary field in clear weather.
//! - `SunParam`: sizes of the sun and moon sprites and their distance.
//! - `PrCloud_N`, `PrCloudV0_N`/`PrCloudV1_N`: the sky's cloud layers N = 0
//!   and 2 under a clear and a cloudy sky (see [`SkyClouds`]).
//! - `EnvPaletteStatic`: what the palettes share — the sky's scattering
//!   heights, the scattering fog's far end, the bloom layers
//!   ([`EnvPaletteStatic`]).
//! - `ClimateDefines_N`: each climate's weather odds, temperatures, wind, tint
//!   and row of palette sets ([`Climate`], [`PALETTE_SET_ROWS`]);
//!   `WeatherInfluence_N`: each weather's tint,
//!   fog colour and bloom ([`Influence`]).
//!
//! How the game blends between divisions follows `ksys::world::EnvMgr::
//! updateTimeDivision` in the decompilation. `Env/env.sgenvb` in
//! `Pack/Bootup.pack` holds the renderer's base objects (`dir_main`,
//! `hemi_inner`, fogs, colour correction, light maps), which these palettes
//! override per frame; see [`crate::envset`].

use roead::aamp::{Parameter, ParameterIO, ParameterObject};

use crate::content::ContentRoots;
use crate::{FormatError, Result};

/// Where the parameters live (inside `Pack/TitleBG.pack`).
pub const WORLD_INFO: &str = "WorldMgr/normal.bwinfo";

/// Number of sky divisions the day is split into.
pub const DIVISIONS: usize = 8;

/// How many palette sets (`EnvAttribute_N`) and palettes (`EnvPalette_N`)
/// the game has (`EnvMgr`, Wii U v208: the checks `0x3a <` in
/// `0x0364228c` and `cmplwi 207` in `0x036425b8`).
pub const PALETTE_SETS: usize = 59;
pub const PALETTES: usize = 207;

/// One palette: the environment at one part of the day.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct EnvPalette {
    /// Main (sun or moon) light colour and intensity (`BgDifColor`,
    /// `BgDifIntencity`; 9 at noon).
    pub light_color: [f32; 3],
    pub light_intensity: f32,
    /// Distance fog colour (alpha: its strength) and range in metres.
    pub fog_color: [f32; 4],
    pub fog_start: f32,
    pub fog_end: f32,
    /// Colour of the sun (or moon) in the sky and its brightness
    /// (`SkySunColor`; its colour is `BgDifColor`'s while `SkySunColorNoUse`
    /// is set, see [`Self::from_object`]).
    pub sky_sun_color: [f32; 3],
    pub sky_sun_intensity: f32,
    /// The upper cloud layer's colours (the renderer's `CloudParam0`): base,
    /// highlight and shadow colours with their intensities, and the rim seen
    /// against the light; the palette's row `Cloud0_*`, or `Cloud2_*` while
    /// `Cloud2NoUse` is set ([`Self::from_object`]).
    pub cloud_base: [f32; 3],
    pub cloud_base_intensity: f32,
    pub cloud_highlight: [f32; 3],
    pub cloud_highlight_intensity: f32,
    pub cloud_shadow: [f32; 3],
    pub cloud_shadow_intensity: f32,
    pub cloud_backlight: [f32; 3],
    pub cloud_backlight_power: f32,
    /// The lower cloud layer's colours (`CloudParam2`): always the row
    /// `Cloud2_*`.
    pub lower_cloud: PaletteCloud,
    /// Whether clouds cast shadows on the ground (`CloudShadowOnOff`), as
    /// the game blends it: 1 or 0 per palette, blended like the other
    /// fields (`ENV_UpdateWeatherPalettes` @`0x03647c9c..0x03647e44`, Wii U
    /// v208), the strength of the cloud shadow (`1 − proj_shadow_off`,
    /// docs/research/wiiu-render-cpu.md).
    pub cloud_shadow_on: f32,
    /// The sky's scattering (`SkyRParam_*`): how strongly the air (Rayleigh)
    /// and the haze (Mie) scatter light, and how much the haze scatters
    /// forward (the glow around the sun).
    pub rayleigh_amplifier: f32,
    pub mie_amplifier: f32,
    pub mie_asymmetry: f32,
    pub ambient_intensity: f32,
    pub exposure: f32,
    /// Height fog (`YFogColor`, alpha: its strength; `YFogStart`, metres).
    /// What it does is not decompiled; it ends at
    /// [`EnvPaletteStatic::yfog_end`].
    pub yfog_color: [f32; 4],
    pub yfog_start: f32,
    /// `SkyIsotropicfade`: fades the sky towards an even colour (0 in the field).
    pub sky_isotropic_fade: f32,
    /// The scattering fog over distance (`SfParam_near`, `SfParam_attenuation`,
    /// `SfParam_horizontal`); its far end and density are static
    /// ([`EnvPaletteStatic`]).
    pub scatter_near: f32,
    pub scatter_attenuation: f32,
    pub scatter_horizontal: f32,
    /// How strongly the air perspective fades the ground and the sky
    /// (`afParam_attenuationForGrd`, `afParam_attenuationForSky`).
    pub attenuation_ground: f32,
    pub attenuation_sky: f32,
    /// Brightness of the environment map (`AmplifierForEnvMap`).
    pub env_map_amplifier: f32,
    pub bloom: PaletteBloom,
    /// Light shafts (`VolumeMaskColor`, `VolumeMaskIntencity`).
    pub volume_mask_color: [f32; 4],
    pub volume_mask_intensity: f32,
}

/// One row of a palette's cloud colours (`CloudN_*`): base, highlight and
/// shadow colours with their intensities, the rim's colour and power.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq)]
pub struct PaletteCloud {
    pub base: [f32; 3],
    pub base_intensity: f32,
    pub highlight: [f32; 3],
    pub highlight_intensity: f32,
    pub shadow: [f32; 3],
    pub shadow_intensity: f32,
    pub backlight: [f32; 3],
    pub backlight_power: f32,
}

impl PaletteCloud {
    /// The row `Cloud2_*` as `EnvMgr::initEnvPalette` sets it before the
    /// document is read (Wii U v208 `0x0363d76c`, as on Switch), for the
    /// palettes other than 8–15 ([`EnvPalette::default_at`]).
    pub const CLOUD2_DEFAULT: Self = Self {
        base: [1.0, 0.91, 0.72],
        base_intensity: 1.0,
        highlight: [1.0, 0.91, 0.72],
        highlight_intensity: 48.0,
        shadow: [0.0, 0.541, 0.437],
        shadow_intensity: 2.0,
        backlight: [0.0, 0.0, 0.0],
        backlight_power: 1.6,
    };

    fn lerp(&self, other: &Self, t: f32) -> Self {
        let f = |a: f32, b: f32| a + (b - a) * t;
        let v3 = |a: [f32; 3], b: [f32; 3]| [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])];
        Self {
            base: v3(self.base, other.base),
            base_intensity: f(self.base_intensity, other.base_intensity),
            highlight: v3(self.highlight, other.highlight),
            highlight_intensity: f(self.highlight_intensity, other.highlight_intensity),
            shadow: v3(self.shadow, other.shadow),
            shadow_intensity: f(self.shadow_intensity, other.shadow_intensity),
            backlight: v3(self.backlight, other.backlight),
            backlight_power: f(self.backlight_power, other.backlight_power),
        }
    }
}

/// A palette's bloom (`Bloom*`): the luminance above which it glows
/// (`BloomThreshhold`), how strongly, the luminance it clamps at and a
/// distance-dependent offset (`BloomOffset` between `BloomOffsetStart` and
/// `BloomOffsetEnd` metres).
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq)]
pub struct PaletteBloom {
    pub threshold: f32,
    pub intensity: f32,
    pub clamped_luminance: f32,
    pub offset: f32,
    pub offset_start: f32,
    pub offset_end: f32,
}

impl Default for PaletteBloom {
    /// `EnvMgr::initEnvPalette`.
    fn default() -> Self {
        Self {
            threshold: 2.0,
            intensity: 0.1,
            clamped_luminance: 64.0,
            offset: 2.0,
            offset_start: 19_999.0,
            offset_end: 20_000.0,
        }
    }
}

impl PaletteBloom {
    fn lerp(&self, other: &Self, t: f32) -> Self {
        let f = |a: f32, b: f32| a + (b - a) * t;
        Self {
            threshold: f(self.threshold, other.threshold),
            intensity: f(self.intensity, other.intensity),
            clamped_luminance: f(self.clamped_luminance, other.clamped_luminance),
            offset: f(self.offset, other.offset),
            offset_start: f(self.offset_start, other.offset_start),
            offset_end: f(self.offset_end, other.offset_end),
        }
    }
}

impl Default for EnvPalette {
    /// The game's built-in defaults (`EnvMgr::initEnvPalette`) of the
    /// palettes other than 8–15 ([`EnvPalette::default_at`]).
    fn default() -> Self {
        Self {
            light_color: [1.0, 0.894, 0.68],
            light_intensity: 8.0,
            fog_color: [0.535, 0.763, 0.568, 0.4],
            fog_start: 0.0,
            fog_end: 600.0,
            sky_sun_color: [1.0, 0.894, 0.68],
            sky_sun_intensity: 10.0,
            cloud_base: [0.956, 0.643, 0.0],
            cloud_base_intensity: 0.376,
            cloud_highlight: [0.8, 0.92, 0.557],
            cloud_highlight_intensity: 11.76,
            cloud_shadow: [0.44, 0.577, 0.61],
            cloud_shadow_intensity: 1.0,
            cloud_backlight: [1.0, 0.95, 0.645],
            cloud_backlight_power: 1.6,
            lower_cloud: PaletteCloud::CLOUD2_DEFAULT,
            cloud_shadow_on: 1.0,
            rayleigh_amplifier: 1.0,
            mie_amplifier: 2.0,
            mie_asymmetry: 0.8,
            ambient_intensity: 1.0,
            exposure: 0.0,
            yfog_color: [1.288, 0.992, 0.178, 0.8],
            yfog_start: 0.0,
            sky_isotropic_fade: 0.0,
            scatter_near: 0.0,
            scatter_attenuation: 15.0,
            scatter_horizontal: 2.4,
            attenuation_ground: 2.5,
            attenuation_sky: 0.25,
            env_map_amplifier: 5.0,
            bloom: PaletteBloom::default(),
            volume_mask_color: [1.0; 4],
            volume_mask_intensity: 1.0,
        }
    }
}

impl EnvPalette {
    /// Palette `index`'s built-in defaults: [`Default`]'s, but the rows'
    /// `Cloud{0,1,2}_BacklightPower` is 1.2 for the palettes 8–15 (1.6 for
    /// the rest; `EnvMgr::initEnvPalette`, Wii U v208 `0x0363d76c`).
    pub fn default_at(index: usize) -> Self {
        let mut palette = Self::default();
        if (8..16).contains(&index) {
            palette.cloud_backlight_power = 1.2;
            palette.lower_cloud.backlight_power = 1.2;
        }
        palette
    }

    /// The palette as the game's weather update takes it
    /// (`ENV_UpdateWeatherPalettes` `0x036425b8`, Wii U v208): the sky's sun
    /// colour is the palette's `BgDifColor` while `SkySunColorNoUse` is not 0
    /// (its intensity stays `SkySunColor`'s alpha; PPC @`0x036455c8..
    /// 0x03645a70`); the upper cloud layer (`CloudParam0`) takes the row
    /// `Cloud0_*` and the lower (`CloudParam2`) the row `Cloud2_*`, and both
    /// take `Cloud2_*` while `Cloud2NoUse` (`+0x34c`) is not 0
    /// (@`0x03647f24..0x03648110`; `CloudParam1` takes none). Both flags are
    /// per palette, before the palettes are blended. `index` is the
    /// palette's number (`EnvPalette_N`): it picks the defaults.
    fn from_object(object: &ParameterObject, index: usize) -> Self {
        let d = Self::default_at(index);
        let rgba = |name: &str, fallback: [f32; 4]| match object.get(name) {
            Some(Parameter::Color(c)) => [c.r, c.g, c.b, c.a],
            _ => fallback,
        };
        let rgb = |name: &str, fallback: [f32; 3]| {
            let [r, g, b, _] = rgba(name, [fallback[0], fallback[1], fallback[2], 1.0]);
            [r, g, b]
        };
        let float = |name: &str, fallback: f32| {
            object
                .get(name)
                .and_then(|p| p.as_f32().ok())
                .unwrap_or(fallback)
        };
        // `SkySunColorNoUse`, `Cloud2NoUse`: u32 flags, 1 by default
        // (`initEnvPalette`).
        let flag = |name: &str| match object.get(name) {
            Some(Parameter::U32(v)) => *v != 0,
            Some(Parameter::I32(v)) => *v != 0,
            Some(Parameter::Bool(v)) => *v,
            _ => true,
        };
        let sky_sun = rgba(
            "SkySunColor",
            [
                d.sky_sun_color[0],
                d.sky_sun_color[1],
                d.sky_sun_color[2],
                d.sky_sun_intensity,
            ],
        );
        let light_color = rgb("BgDifColor", d.light_color);
        let sky_sun_color = if flag("SkySunColorNoUse") {
            light_color
        } else {
            [sky_sun[0], sky_sun[1], sky_sun[2]]
        };
        let row = |i: usize, d: PaletteCloud| PaletteCloud {
            base: rgb(&format!("Cloud{i}_ColorBase"), d.base),
            base_intensity: float(&format!("Cloud{i}_IntencityBase"), d.base_intensity),
            highlight: rgb(&format!("Cloud{i}_ColorHilight"), d.highlight),
            highlight_intensity: float(
                &format!("Cloud{i}_IntencityHilight"),
                d.highlight_intensity,
            ),
            shadow: rgb(&format!("Cloud{i}_ColorShadow"), d.shadow),
            shadow_intensity: float(&format!("Cloud{i}_IntencityShadow"), d.shadow_intensity),
            backlight: rgb(&format!("Cloud{i}_ColorBackLight"), d.backlight),
            backlight_power: float(&format!("Cloud{i}_BacklightPower"), d.backlight_power),
        };
        let lower = row(
            2,
            PaletteCloud {
                backlight_power: d.cloud_backlight_power,
                ..PaletteCloud::CLOUD2_DEFAULT
            },
        );
        let upper = if flag("Cloud2NoUse") {
            lower
        } else {
            row(0, d.upper_cloud())
        };
        Self {
            light_color,
            light_intensity: float("BgDifIntencity", d.light_intensity),
            fog_color: rgba("FogColor", d.fog_color),
            fog_start: float("FogStart", d.fog_start),
            fog_end: float("FogEnd", d.fog_end),
            sky_sun_color,
            sky_sun_intensity: sky_sun[3],
            cloud_base: upper.base,
            cloud_base_intensity: upper.base_intensity,
            cloud_highlight: upper.highlight,
            cloud_highlight_intensity: upper.highlight_intensity,
            cloud_shadow: upper.shadow,
            cloud_shadow_intensity: upper.shadow_intensity,
            cloud_backlight: upper.backlight,
            cloud_backlight_power: upper.backlight_power,
            lower_cloud: lower,
            cloud_shadow_on: object
                .get("CloudShadowOnOff")
                .and_then(|p| p.as_bool().ok())
                .map_or(d.cloud_shadow_on, |on| if on { 1.0 } else { 0.0 }),
            rayleigh_amplifier: float("SkyRParam_rayleigh_amplifier", d.rayleigh_amplifier),
            mie_amplifier: float("SkyRParam_mie_amplifier", d.mie_amplifier),
            mie_asymmetry: float("SkyRParam_mie_symmetricalProperty", d.mie_asymmetry),
            ambient_intensity: float("AmbientIntencity", d.ambient_intensity),
            exposure: float("Exposure", d.exposure),
            yfog_color: rgba("YFogColor", d.yfog_color),
            yfog_start: float("YFogStart", d.yfog_start),
            sky_isotropic_fade: float("SkyIsotropicfade", d.sky_isotropic_fade),
            scatter_near: float("SfParam_near", d.scatter_near),
            scatter_attenuation: float("SfParam_attenuation", d.scatter_attenuation),
            scatter_horizontal: float("SfParam_horizontal", d.scatter_horizontal),
            attenuation_ground: float("afParam_attenuationForGrd", d.attenuation_ground),
            attenuation_sky: float("afParam_attenuationForSky", d.attenuation_sky),
            env_map_amplifier: float("AmplifierForEnvMap", d.env_map_amplifier),
            bloom: PaletteBloom {
                threshold: float("BloomThreshhold", d.bloom.threshold),
                intensity: float("BloomIntencity", d.bloom.intensity),
                clamped_luminance: float("BloomClampedLuminance", d.bloom.clamped_luminance),
                offset: float("BloomOffset", d.bloom.offset),
                offset_start: float("BloomOffsetStart", d.bloom.offset_start),
                offset_end: float("BloomOffsetEnd", d.bloom.offset_end),
            },
            volume_mask_color: rgba("VolumeMaskColor", d.volume_mask_color),
            volume_mask_intensity: float("VolumeMaskIntencity", d.volume_mask_intensity),
        }
    }

    /// The upper cloud layer's colours as one row.
    pub fn upper_cloud(&self) -> PaletteCloud {
        PaletteCloud {
            base: self.cloud_base,
            base_intensity: self.cloud_base_intensity,
            highlight: self.cloud_highlight,
            highlight_intensity: self.cloud_highlight_intensity,
            shadow: self.cloud_shadow,
            shadow_intensity: self.cloud_shadow_intensity,
            backlight: self.cloud_backlight,
            backlight_power: self.cloud_backlight_power,
        }
    }

    /// Blends towards `other` (`t` = 0 is `self`, 1 is `other`).
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let f = |a: f32, b: f32| a + (b - a) * t;
        let v3 = |a: [f32; 3], b: [f32; 3]| [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])];
        let v4 =
            |a: [f32; 4], b: [f32; 4]| [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2]), f(a[3], b[3])];
        Self {
            light_color: v3(self.light_color, other.light_color),
            light_intensity: f(self.light_intensity, other.light_intensity),
            fog_color: v4(self.fog_color, other.fog_color),
            fog_start: f(self.fog_start, other.fog_start),
            fog_end: f(self.fog_end, other.fog_end),
            sky_sun_color: v3(self.sky_sun_color, other.sky_sun_color),
            sky_sun_intensity: f(self.sky_sun_intensity, other.sky_sun_intensity),
            cloud_base: v3(self.cloud_base, other.cloud_base),
            cloud_base_intensity: f(self.cloud_base_intensity, other.cloud_base_intensity),
            cloud_highlight: v3(self.cloud_highlight, other.cloud_highlight),
            cloud_highlight_intensity: f(
                self.cloud_highlight_intensity,
                other.cloud_highlight_intensity,
            ),
            cloud_shadow: v3(self.cloud_shadow, other.cloud_shadow),
            cloud_shadow_intensity: f(self.cloud_shadow_intensity, other.cloud_shadow_intensity),
            cloud_backlight: v3(self.cloud_backlight, other.cloud_backlight),
            cloud_backlight_power: f(self.cloud_backlight_power, other.cloud_backlight_power),
            lower_cloud: self.lower_cloud.lerp(&other.lower_cloud, t),
            cloud_shadow_on: f(self.cloud_shadow_on, other.cloud_shadow_on),
            rayleigh_amplifier: f(self.rayleigh_amplifier, other.rayleigh_amplifier),
            mie_amplifier: f(self.mie_amplifier, other.mie_amplifier),
            mie_asymmetry: f(self.mie_asymmetry, other.mie_asymmetry),
            ambient_intensity: f(self.ambient_intensity, other.ambient_intensity),
            exposure: f(self.exposure, other.exposure),
            yfog_color: v4(self.yfog_color, other.yfog_color),
            yfog_start: f(self.yfog_start, other.yfog_start),
            sky_isotropic_fade: f(self.sky_isotropic_fade, other.sky_isotropic_fade),
            scatter_near: f(self.scatter_near, other.scatter_near),
            scatter_attenuation: f(self.scatter_attenuation, other.scatter_attenuation),
            scatter_horizontal: f(self.scatter_horizontal, other.scatter_horizontal),
            attenuation_ground: f(self.attenuation_ground, other.attenuation_ground),
            attenuation_sky: f(self.attenuation_sky, other.attenuation_sky),
            env_map_amplifier: f(self.env_map_amplifier, other.env_map_amplifier),
            bloom: self.bloom.lerp(&other.bloom, t),
            volume_mask_color: v4(self.volume_mask_color, other.volume_mask_color),
            volume_mask_intensity: f(self.volume_mask_intensity, other.volume_mask_intensity),
        }
    }
}

/// What does not change with the time of day (`EnvPaletteStatic`): the sky's
/// scattering heights and haze, the far end and density of the scattering
/// fog, where the height fog ends and the bloom layers.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct EnvPaletteStatic {
    /// Scale heights of the air (`rayleigh_baseHeigh`) and the haze
    /// (`mie_baseHeight`); units are not decompiled (probably km).
    // SI-FMT-05: the unit of this height is our reading.
    pub rayleigh_base_height: f32,
    pub mie_base_height: f32,
    /// `mie_scatteringCoeff`, `mie_symmetricalPropert`.
    pub mie_scattering: f32,
    pub mie_asymmetry: f32,
    /// Where the height fog ends (`YFogEnd`).
    pub yfog_end: f32,
    /// Far end (metres) and density of the scattering fog (`SfParam_far`,
    /// `SfParam_density`).
    pub scatter_far: f32,
    pub scatter_density: f32,
    /// Colours of the bloom's blur levels at 1/8, 1/16, 1/32 and 1/64 size
    /// (`BloomLayerColor_8_8` …; alpha: weight) and whether each is off
    /// (`BloomLayerColorNoUse_*`), and the colour the bloom is added with.
    pub bloom_layers: [([f32; 4], bool); 4],
    pub bloom_compose_color: [f32; 4],
}

impl Default for EnvPaletteStatic {
    /// `EnvMgr::initEnvPaletteStatic`.
    fn default() -> Self {
        Self {
            rayleigh_base_height: 20.0,
            mie_base_height: 2.0,
            mie_scattering: 0.0018,
            mie_asymmetry: 0.8,
            yfog_end: 160.0,
            scatter_far: 24_000.0,
            scatter_density: 0.85,
            bloom_layers: [
                ([1.0, 1.0, 1.0, 0.5], false),
                ([1.0; 4], false),
                ([1.0; 4], true),
                ([1.0, 1.0, 1.0, 0.0], true),
            ],
            bloom_compose_color: [1.0; 4],
        }
    }
}

impl EnvPaletteStatic {
    fn from_object(object: &ParameterObject) -> Self {
        let d = Self::default();
        let float = |name: &str, fallback: f32| param_f32(object, name, fallback);
        let rgba = |name: &str, fallback: [f32; 4]| match object.get(name) {
            Some(Parameter::Color(c)) => [c.r, c.g, c.b, c.a],
            _ => fallback,
        };
        // `NoUse` is an integer flag in the data.
        let unused = |name: &str, fallback: bool| {
            object
                .get(name)
                .and_then(|p| p.as_int::<i64>().ok())
                .map_or(fallback, |v| v != 0)
        };
        let layer = |i: usize, size: u32| {
            let (color, off) = d.bloom_layers[i];
            // The first layer's flag is spelt `BloomComposeColorNoUse_8_8`.
            let flag = if size == 8 {
                "BloomComposeColorNoUse_8_8".to_owned()
            } else {
                format!("BloomLayerColorNoUse_{size}_{size}")
            };
            (
                rgba(&format!("BloomLayerColor_{size}_{size}"), color),
                unused(&flag, off),
            )
        };
        Self {
            rayleigh_base_height: float("rayleigh_baseHeigh", d.rayleigh_base_height),
            mie_base_height: float("mie_baseHeight", d.mie_base_height),
            mie_scattering: float("mie_scatteringCoeff", d.mie_scattering),
            mie_asymmetry: float("mie_symmetricalPropert", d.mie_asymmetry),
            yfog_end: float("YFogEnd", d.yfog_end),
            scatter_far: float("SfParam_far", d.scatter_far),
            scatter_density: float("SfParam_density", d.scatter_density),
            bloom_layers: [layer(0, 8), layer(1, 16), layer(2, 32), layer(3, 64)],
            bloom_compose_color: rgba("BloomComposeColor", d.bloom_compose_color),
        }
    }
}

/// Sun and moon sprites (`SunParam`).
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct SunParams {
    /// Size of the sun and moon sprites and their distance from the camera,
    /// in the same (arbitrary) units (`SunMoonDispDist` is negative in the
    /// game's data and default; only its size is used).
    pub sun_scale: f32,
    pub moon_scale: f32,
    pub distance: f32,
    /// Tilt of the sun's path: the sun's direction at noon leans by this
    /// much along Z per unit down (Wii U v208 `0x03656be0`; the field's
    /// -1.1 puts the noon sun in the north, 42 degrees up).
    pub slope: f32,
    /// Where the main light's direction stops going down (`SunDirYStop`):
    /// the light's direction, 80 000 units long before this cut, keeps its
    /// Y at or below this (same code).
    pub dir_y_stop: f32,
}

impl Default for SunParams {
    /// The game's built-in defaults (`SkyMgr::SkyMgr`, Wii U v208
    /// `0x0364f620`).
    fn default() -> Self {
        Self {
            sun_scale: 2400.0,
            moon_scale: 2400.0,
            distance: -24000.0,
            slope: 1.1,
            dir_y_stop: -55000.0,
        }
    }
}

impl SunParams {
    /// Apparent diameters of the sun and the moon in radians.
    // SI-FMT-05: sun size from SunScale over distance is our reading.
    pub fn angular_sizes(&self) -> (f32, f32) {
        let distance = self.distance.abs().max(1.0);
        (self.sun_scale / distance, self.moon_scale / distance)
    }
}

/// A value `SkyMgr` sways between `min` and `max` (`<Name>Min`, `<Name>Max`,
/// `<Name>SinSeedAdd` next to `<Name>` in a `PrCloudV` object):
/// `min + (max − min)·(sin φ + 1)/2`, the phase φ advancing by `rate` times
/// a factor of the sky manager (`SkyMgr+0x2148`) each frame (`FUN_0365867c`,
/// Wii U v208). `value` itself is drawn only while a flag next to the phase
/// is set, which the constructor clears and nothing in play sets.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Sway {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub rate: f32,
}

impl Sway {
    /// A value that holds still at `value`.
    const fn fixed(value: f32) -> Self {
        Self {
            value,
            min: value,
            max: value,
            rate: 0.0,
        }
    }

    /// Each of the four read on its own, keeping `fallback`'s where the
    /// document has none (the constructor sets them apart).
    fn read(object: &ParameterObject, name: &str, fallback: Self) -> Self {
        let float =
            |suffix: &str, fallback: f32| param_f32(object, &format!("{name}{suffix}"), fallback);
        Self {
            value: float("", fallback.value),
            min: float("Min", fallback.min),
            max: float("Max", fallback.max),
            rate: float("SinSeedAdd", fallback.rate),
        }
    }

    /// The middle of the swing, `(min + max)/2`.
    pub fn centre(&self) -> f32 {
        (self.min + self.max) / 2.0
    }

    /// Half the swing with its sign, `(max − min)/2`: the value is
    /// `centre + half_swing·sin φ`.
    pub fn half_swing(&self) -> f32 {
        (self.max - self.min) / 2.0
    }

    /// The value at phase `phase`.
    pub fn at(&self, phase: f32) -> f32 {
        self.min + (self.max - self.min) * (phase.sin() + 1.0) / 2.0
    }
}

/// One of the two looks of a cloud layer (`PrCloudV0_N` under a clear sky,
/// `PrCloudV1_N` under a cloudy one; `ksys::world::SkyMgr`): the
/// `agl::fx::Cloud` layer parameters of the same names (`CloudParamN`; the
/// shader that uses them: `docs/research/wiiu-sky-resources.md`). `SkyMgr`
/// blends the two by the world's cloudiness (`SkyMgr+0x2120`) every frame.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct CloudLayer {
    /// How much a noise texture warps the pattern.
    pub distortion: Sway,
    /// How much of the sky the layer covers.
    pub density: Sway,
    /// Opacity scale and the density at which the clouds turn opaque.
    pub alpha_mul: Sway,
    pub alpha_threshold: Sway,
    /// How often the cloud texture repeats (bigger is smaller clouds).
    pub tex_scale: Sway,
    /// Height of the layer above the ground, in metres.
    pub height: Sway,
    /// The relief shading: how far towards the light the shader looks for
    /// more cloud (`EmbossWidth`) and how much that darkens (`EmbossDensity`).
    pub emboss_width: f32,
    pub emboss_density: f32,
    pub shadow_power: f32,
    pub highlight_power: f32,
    pub highlight_range: f32,
    pub highlight_ambient: f32,
    /// The glow looking into the light (`BacklightPowe`, `BacklightRange`).
    pub backlight_power: f32,
    pub backlight_range: f32,
    /// How the glow into the light fades with the clouds' opacity
    /// (`BacklightParam0`, `BacklightParam1`).
    pub backlight_param0: f32,
    pub backlight_param1: f32,
    /// How the noise darkens the shaded side and lightens the lit side
    /// (`DarkSideNoiseParam`, `LightSideNoiseParam`).
    pub dark_side_noise: f32,
    pub light_side_noise: f32,
}

impl CloudLayer {
    /// Look `look` (0 clear, 1 cloudy) of layer `layer` (`mPrCloudV[layer]
    /// [look]`, the object `PrCloudV{look}_{layer}`: the names are
    /// `PrCloudV0_%d`, `PrCloudV1_%d` by layer) before the document is read:
    /// `SkyMgr::SkyMgr` (Wii U v208 `0x0364f620`, the same values as the
    /// Switch `worldSkyMgr.cpp`).
    pub fn game_default(layer: usize, look: usize) -> Self {
        // Distortion, density, alpha mul and threshold, emboss width and
        // density, shadow power, highlight power, range and ambient, height.
        #[rustfmt::skip]
        let (distortion, density, alpha_mul, alpha_threshold, emboss_width, emboss_density,
            shadow_power, highlight_power, highlight_range, highlight_ambient, height) =
            match (layer, look) {
                (0, 0) => (0.6, Sway::fixed(0.3), 0.4, 0.7, 1.0, 1.0, 0.249, 1.0, 0.8, 0.1, 15_000.0),
                (0, _) => (0.6, Sway::fixed(0.65), 0.65, 0.95, 0.06, 0.039, 0.223, 1.0, 0.8, 0.1, 13_000.0),
                (1, 0) => (
                    0.6,
                    Sway { value: 0.2, min: 0.05, max: 0.05, rate: 0.01 },
                    0.4, 0.7, 1.0, 1.0, 0.249, 1.0, 0.8, 0.1, 15_000.0,
                ),
                (1, _) => (1.0, Sway::fixed(0.5), 1.0, 1.0, 1.0, 1.0, 0.249, 1.0, 0.8, 0.1, 15_000.0),
                (_, 0) => (0.6, Sway::fixed(0.3), 1.0, 0.7, 0.08904, 0.5, 0.75, 20.0, 1.7, 0.5, 14_500.0),
                _ => (0.6, Sway::fixed(0.65), 1.0, 0.8, 0.08904, 0.5, 0.75, 20.0, 1.7, 0.5, 10_000.0),
            };
        Self {
            distortion: Sway::fixed(distortion),
            density,
            alpha_mul: Sway::fixed(alpha_mul),
            alpha_threshold: Sway::fixed(alpha_threshold),
            tex_scale: Sway::fixed(1.0),
            height: Sway::fixed(height),
            emboss_width,
            emboss_density,
            shadow_power,
            highlight_power,
            highlight_range,
            highlight_ambient,
            backlight_power: 1.5,
            backlight_range: 0.65,
            backlight_param0: 0.2,
            backlight_param1: 0.45,
            dark_side_noise: if look == 0 { 1.0 } else { 0.5 },
            light_side_noise: if look == 0 { 0.25 } else { 1.0 },
        }
    }

    fn from_object(object: &ParameterObject, d: &Self) -> Self {
        let sway = |name: &str, fallback: Sway| Sway::read(object, name, fallback);
        let float = |name: &str, fallback: f32| param_f32(object, name, fallback);
        Self {
            distortion: sway("Distotion", d.distortion),
            density: sway("Density", d.density),
            alpha_mul: sway("AlphaMul", d.alpha_mul),
            alpha_threshold: sway("AlphaThreshold", d.alpha_threshold),
            tex_scale: sway("BaseTexScale", d.tex_scale),
            height: sway("SkyHeight", d.height),
            emboss_width: float("EmbossWidth", d.emboss_width),
            emboss_density: float("EmbossDensity", d.emboss_density),
            shadow_power: float("ShadowPower", d.shadow_power),
            highlight_power: float("HighlightPower", d.highlight_power),
            highlight_range: float("HighlightRange", d.highlight_range),
            highlight_ambient: float("HighlightAmbient", d.highlight_ambient),
            backlight_power: float("BacklightPowe", d.backlight_power),
            backlight_range: float("BacklightRange", d.backlight_range),
            backlight_param0: float("BacklightParam0", d.backlight_param0),
            backlight_param1: float("BacklightParam1", d.backlight_param1),
            dark_side_noise: float("DarkSideNoiseParam", d.dark_side_noise),
            light_side_noise: float("LightSideNoiseParam", d.light_side_noise),
        }
    }
}

/// Which of the renderer's cloud textures a layer draws with: numbers in
/// the list of the environment's textures ([`crate::envset::EnvSet::cloud_textures`]).
/// `SkyMgr` copies them from `PrCloud_0` into the upper layer (`CloudParam0`)
/// and from `PrCloud_2` into the lower one (`CloudParam2`), and turns the
/// blending of the two base textures on (`FUN_03659fa8`, Wii U v208).
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CloudTextureNumbers {
    /// `BaseTextureNo`, `BaseTextureNo_Blend`: the two cloud textures the
    /// layer blends by `mCloudTexBlendRate`.
    pub base: i32,
    pub base_blend: i32,
    /// `NoiseTextureNo`, `NoiseTextureNo_Blend`: the two noise textures that
    /// warp them, blended the same way.
    pub noise: i32,
    pub noise_blend: i32,
}

impl CloudTextureNumbers {
    /// `PrCloud_{index}` before the document is read: `SkyMgr::SkyMgr`
    /// (Switch `worldSkyMgr.cpp`, the same on Wii U v208 `0x0364f620`),
    /// which sets `NoiseTextureNo_Blend` of the first and the last to 3.
    pub fn game_default(index: usize) -> Self {
        Self {
            base: 2,
            base_blend: 4,
            noise: 3,
            noise_blend: if index == 1 { 4 } else { 3 },
        }
    }

    fn from_object(object: &ParameterObject, d: Self) -> Self {
        Self {
            base: param_i32(object, "BaseTextureNo", d.base),
            base_blend: param_i32(object, "BaseTextureNo_Blend", d.base_blend),
            noise: param_i32(object, "NoiseTextureNo", d.noise),
            noise_blend: param_i32(object, "NoiseTextureNo_Blend", d.noise_blend),
        }
    }
}

/// A patch where a layer thins or thickens (`PosDensityChgRange`,
/// `PosDensityChgPower`, `PosDensityChgSpeed` of `PrCloud_N`): `SkyMgr`
/// puts it at a random spot of the dome, grows its strength from 0 to
/// `power` at `speed` a frame, fades it back and moves it (`FUN_0365867c`).
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq)]
pub struct DensitySpot {
    /// Radius on the dome's mesh (1 is the rim).
    pub range: f32,
    /// Density factor at the centre minus 1, at full strength.
    pub power: f32,
    /// Strength gained or lost per frame (30 a second).
    pub speed: f32,
}

/// One of the sky's two cloud layers as `SkyMgr` drives it (`PrCloud_N`,
/// `PrCloudV0_N`, `PrCloudV1_N`; N = 0 for the upper layer `CloudParam0`,
/// 2 for the lower `CloudParam2`; layer 1 is off in the field).
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct SkyCloudLayer {
    /// `ScrollSpd`: the base texture's speed along the layer's wind
    /// (`mBaseTexScrollSpdX/Y` = (sin a, cos a)·`SkyMgr+0x2144`·`ScrollSpd`).
    pub scroll_speed: f32,
    /// `NoiseAdd1`, `NoiseAdd2`, `NoiseAdd1_side`, `NoiseAdd2_side`: the
    /// noises' speeds along and across the wind (`mNoiseSpeed*`).
    pub noise_add: [f32; 4],
    /// `WindVecAdd`: how far the layer's wind may turn from the one below
    /// (times a draw in [−1, 1) at each reset).
    pub wind_add: f32,
    pub density_spot: DensitySpot,
    pub textures: CloudTextureNumbers,
    /// Under a clear sky (`PrCloudV0_N`) and a cloudy one (`PrCloudV1_N`).
    pub looks: [CloudLayer; 2],
}

impl SkyCloudLayer {
    /// Layer `index` (`PrCloud_{index}` and its looks) before the document
    /// is read: `SkyMgr::SkyMgr` (Switch `worldSkyMgr.cpp`, Wii U v208
    /// `0x0364f620`).
    pub fn game_default(index: usize) -> Self {
        let (scroll_speed, noise_add, wind_add) = if index == 0 {
            (-0.35, [-0.75, -0.2, -0.2, -0.3], 0.262)
        } else {
            (-0.5, [-0.25, -0.5, -0.2, -0.3], 0.0)
        };
        Self {
            scroll_speed,
            noise_add,
            wind_add,
            density_spot: DensitySpot {
                range: 0.0,
                power: 0.0,
                speed: 0.0,
            },
            textures: CloudTextureNumbers::game_default(index),
            looks: [0, 1].map(|look| CloudLayer::game_default(index, look)),
        }
    }

    fn from_objects(objects: &roead::aamp::ParameterObjectMap, index: usize) -> Self {
        let d = Self::game_default(index);
        let mut layer = d.clone();
        if let Some(o) = objects.get(format!("PrCloud_{index}").as_str()) {
            let float = |name: &str, fallback: f32| param_f32(o, name, fallback);
            let names = ["NoiseAdd1", "NoiseAdd2", "NoiseAdd1_side", "NoiseAdd2_side"];
            layer.scroll_speed = float("ScrollSpd", d.scroll_speed);
            layer.noise_add = [0, 1, 2, 3].map(|i| float(names[i], d.noise_add[i]));
            layer.wind_add = float("WindVecAdd", d.wind_add);
            layer.density_spot = DensitySpot {
                range: float("PosDensityChgRange", d.density_spot.range),
                power: float("PosDensityChgPower", d.density_spot.power),
                speed: float("PosDensityChgSpeed", d.density_spot.speed),
            };
            layer.textures = CloudTextureNumbers::from_object(o, d.textures);
        }
        for (n, (look, default)) in layer.looks.iter_mut().zip(&d.looks).enumerate() {
            if let Some(o) = objects.get(format!("PrCloudV{n}_{index}").as_str()) {
                *look = CloudLayer::from_object(o, default);
            }
        }
        layer
    }
}

/// The sky's clouds as `SkyMgr` writes them into the renderer's layers
/// every frame (`FUN_0365867c`, Wii U v208, from `FUN_0365acb0`): the upper
/// layer from `PrCloud_0`/`PrCloudV*_0`, the lower from `PrCloud_2`/
/// `PrCloudV*_2`. The per-weather `SkyPalette0_N`/`SkyPalette2_N` take over
/// only while `SkyMgr+0x2188` is set, which nothing but resets writes (0) in
/// v208, and `CloudPat*` only in palette sets 16/17 and under the Blood Moon
/// (`FUN_03655de8`): neither is read here.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct SkyClouds {
    pub layers: [SkyCloudLayer; 2],
}

impl SkyClouds {
    fn parse(objects: &roead::aamp::ParameterObjectMap) -> Option<Self> {
        let present = [0, 2].iter().any(|i| {
            ["PrCloud_", "PrCloudV0_", "PrCloudV1_"]
                .iter()
                .any(|prefix| objects.get(format!("{prefix}{i}").as_str()).is_some())
        });
        present.then(|| Self {
            layers: [0, 2].map(|i| SkyCloudLayer::from_objects(objects, i)),
        })
    }
}

fn param_f32(object: &ParameterObject, name: &str, fallback: f32) -> f32 {
    object
        .get(name)
        .and_then(|p| p.as_f32().ok())
        .unwrap_or(fallback)
}

fn param_i32(object: &ParameterObject, name: &str, fallback: i32) -> i32 {
    object
        .get(name)
        .and_then(|p| p.as_i32().ok())
        .unwrap_or(fallback)
}

fn param_bool(object: &ParameterObject, name: &str, fallback: bool) -> bool {
    object
        .get(name)
        .and_then(|p| p.as_bool().ok())
        .unwrap_or(fallback)
}

fn param_rgb(object: &ParameterObject, name: &str, fallback: [f32; 3]) -> [f32; 3] {
    match object.get(name) {
        Some(Parameter::Color(c)) => [c.r, c.g, c.b],
        _ => fallback,
    }
}

/// How a climate or a weather changes the light and the sky
/// (`ClimateDefines_N`, `WeatherInfluence_N`; the game's `Remains_N` have the
/// same fields): a colour the scene is tinted with (`FeatureColor`) and one
/// for the fog (`FeatureFogColor`, weathers only), factors on the palette's
/// sky scattering (`CalcRayleigh`, `CalcMie`, `CalcMieSymmetrical`), light
/// shafts (`CalcVolumeMaskIntencity`) and bloom (`BloomThreshhold`,
/// `BloomIntencity`, weathers only), and offsets to the scattering fog
/// (`CalcSfParamNear`, `CalcSfParamAttenuation`, climates only). How exactly
/// the renderer applies them is in code that is not decompiled.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Influence {
    pub feature_color: [f32; 3],
    pub feature_fog_color: [f32; 3],
    pub rayleigh: f32,
    pub mie: f32,
    pub mie_symmetrical: f32,
    pub volume_mask: f32,
    pub bloom_threshold: f32,
    pub bloom_intensity: f32,
    /// Added to the palette's `SfParam_near` and `SfParam_attenuation`
    /// (the data's values, e.g. −50, look like offsets rather than factors).
    pub scatter_near: f32,
    pub scatter_attenuation: f32,
    /// Moisture the weather adds (`AddMoisture`, percent).
    pub add_moisture: f32,
}

impl Default for Influence {
    /// `EnvMgr::initWeatherInfluence`, `Manager::onStageInit`.
    fn default() -> Self {
        Self {
            feature_color: [1.0; 3],
            feature_fog_color: [1.0; 3],
            rayleigh: 1.0,
            mie: 1.0,
            mie_symmetrical: 1.0,
            volume_mask: 1.0,
            bloom_threshold: 1.0,
            bloom_intensity: 1.0,
            scatter_near: 0.0,
            scatter_attenuation: 0.0,
            add_moisture: 0.0,
        }
    }
}

impl Influence {
    fn from_object(object: &ParameterObject) -> Self {
        let d = Self::default();
        Self {
            feature_color: param_rgb(object, "FeatureColor", d.feature_color),
            feature_fog_color: param_rgb(object, "FeatureFogColor", d.feature_fog_color),
            rayleigh: param_f32(object, "CalcRayleigh", d.rayleigh),
            mie: param_f32(object, "CalcMie", d.mie),
            mie_symmetrical: param_f32(object, "CalcMieSymmetrical", d.mie_symmetrical),
            volume_mask: param_f32(object, "CalcVolumeMaskIntencity", d.volume_mask),
            bloom_threshold: param_f32(object, "BloomThreshhold", d.bloom_threshold),
            bloom_intensity: param_f32(object, "BloomIntencity", d.bloom_intensity),
            scatter_near: param_f32(object, "CalcSfParamNear", d.scatter_near),
            scatter_attenuation: param_f32(object, "CalcSfParamAttenuation", d.scatter_attenuation),
            add_moisture: param_f32(object, "AddMoisture", d.add_moisture),
        }
    }

    /// The mean of `influences` weighted by their weights (all fields; the
    /// default if the weights add up to nothing).
    // SI-FMT-04: weighted mean of climates and weathers is ours.
    pub fn weighted_mean(influences: impl IntoIterator<Item = (Influence, f32)>) -> Self {
        let mut sum = [0.0f32; 15];
        let mut total = 0.0;
        for (influence, weight) in influences {
            for (s, v) in sum.iter_mut().zip(influence.to_array()) {
                *s += v * weight;
            }
            total += weight;
        }
        if total <= 0.0 {
            return Self::default();
        }
        Self::from_array(sum.map(|s| s / total.max(1e-6)))
    }

    /// `self` followed by `other` (a climate's influence, then the
    /// weather's): colours and factors multiply, offsets add up.
    // SI-FMT-04: weighted mean of climates and weathers is ours.
    pub fn then(&self, other: &Self) -> Self {
        let mul = |a: [f32; 3], b: [f32; 3]| [a[0] * b[0], a[1] * b[1], a[2] * b[2]];
        Self {
            feature_color: mul(self.feature_color, other.feature_color),
            feature_fog_color: mul(self.feature_fog_color, other.feature_fog_color),
            rayleigh: self.rayleigh * other.rayleigh,
            mie: self.mie * other.mie,
            mie_symmetrical: self.mie_symmetrical * other.mie_symmetrical,
            volume_mask: self.volume_mask * other.volume_mask,
            bloom_threshold: self.bloom_threshold * other.bloom_threshold,
            bloom_intensity: self.bloom_intensity * other.bloom_intensity,
            scatter_near: self.scatter_near + other.scatter_near,
            scatter_attenuation: self.scatter_attenuation + other.scatter_attenuation,
            add_moisture: self.add_moisture + other.add_moisture,
        }
    }

    fn to_array(self) -> [f32; 15] {
        let [r, g, b] = self.feature_color;
        let [fr, fg, fb] = self.feature_fog_color;
        [
            r,
            g,
            b,
            fr,
            fg,
            fb,
            self.rayleigh,
            self.mie,
            self.mie_symmetrical,
            self.volume_mask,
            self.bloom_threshold,
            self.bloom_intensity,
            self.scatter_near,
            self.scatter_attenuation,
            self.add_moisture,
        ]
    }

    fn from_array(a: [f32; 15]) -> Self {
        Self {
            feature_color: [a[0], a[1], a[2]],
            feature_fog_color: [a[3], a[4], a[5]],
            rayleigh: a[6],
            mie: a[7],
            mie_symmetrical: a[8],
            volume_mask: a[9],
            bloom_threshold: a[10],
            bloom_intensity: a[11],
            scatter_near: a[12],
            scatter_attenuation: a[13],
            add_moisture: a[14],
        }
    }
}

/// The weathers the game knows (`ksys::world::WeatherType`).
pub const WEATHERS: [&str; 9] = [
    "Bluesky",
    "Cloudy",
    "Rain",
    "HeavyRain",
    "Snow",
    "HeavySnow",
    "ThunderStorm",
    "ThunderRain",
    "BlueskyRain",
];

/// One climate (`ClimateDefines_N`, N in [`crate::eco::CLIMATES`] order;
/// `ksys::world::ClimateInfo`).
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Climate {
    /// Chances in percent of clear sky, cloudy, rain, heavy rain and storm
    /// (`Weather*Rate`; `WeatherMgr::rollNewWeather` rolls them).
    pub weather_rates: [i32; 5],
    /// Clear sky always by day or by night (`DayLockBlueSky`, `NightLockBlueSky`).
    pub day_lock_blue_sky: bool,
    pub night_lock_blue_sky: bool,
    pub influence: Influence,
    // SI-FMT-04: weighted mean of climates and weathers is ours.
    /// Factor on the palette's ambient light (`CalcAmbientIntencity`).
    pub ambient: f32,
    /// Which palette sets to use (`PaletteSetSelect`): a row of
    /// [`PALETTE_SET_ROWS`], see [`Climate::palette_sets`].
    pub palette_set: usize,
    /// Air temperature by day and by night at 1000, 900, … 0 m (°C;
    /// `ClimateTemperatureDay_1000` … `_0000`).
    pub temperature_day: [f32; 11],
    pub temperature_night: [f32; 11],
    /// Wind strength (`WindPower`; the game rerolls a multiplier for it).
    pub wind_power: f32,
    /// Air moisture range in percent (`MoistureMin`, `MoistureMax`).
    pub moisture: (f32, f32),
    /// `FogType` (0 or 1; 1 in the snowy climates; meaning not decompiled).
    pub fog_type: i32,
}

impl Default for Climate {
    /// The game's built-in defaults (`Manager::onStageInit`).
    fn default() -> Self {
        Self {
            weather_rates: [60, 20, 15, 5, 0],
            day_lock_blue_sky: false,
            night_lock_blue_sky: false,
            influence: Influence::default(),
            ambient: 1.0,
            palette_set: 0,
            temperature_day: [
                -60.0, -60.0, -60.0, -45.0, -30.0, -10.0, 0.0, 10.0, 20.0, 23.0, 25.0,
            ],
            temperature_night: [
                -63.0, -63.0, -63.0, -48.0, -33.0, -13.0, -3.0, 7.0, 17.0, 20.0, 23.0,
            ],
            wind_power: 5.0,
            moisture: (0.0, 0.0),
            fog_type: 0,
        }
    }
}

impl Climate {
    fn from_object(object: &ParameterObject) -> Self {
        let d = Self::default();
        let rate = |name: &str, i: usize| {
            param_i32(object, &format!("Weather{name}Rate"), d.weather_rates[i])
        };
        let temps = |kind: &str, fallback: [f32; 11]| {
            std::array::from_fn(|i| {
                param_f32(
                    object,
                    &format!("ClimateTemperature{kind}_{:04}", (10 - i) * 100),
                    fallback[i],
                )
            })
        };
        Self {
            weather_rates: [
                rate("Bluesky", 0),
                rate("Cloudy", 1),
                rate("Rain", 2),
                rate("HeavyRain", 3),
                rate("Storm", 4),
            ],
            day_lock_blue_sky: param_bool(object, "DayLockBlueSky", false),
            night_lock_blue_sky: param_bool(object, "NightLockBlueSky", false),
            influence: Influence::from_object(object),
            ambient: param_f32(object, "CalcAmbientIntencity", 1.0),
            palette_set: param_i32(object, "PaletteSetSelect", 0).max(0) as usize,
            temperature_day: temps("Day", d.temperature_day),
            temperature_night: temps("Night", d.temperature_night),
            wind_power: param_f32(object, "WindPower", d.wind_power),
            moisture: (
                param_f32(object, "MoistureMin", d.moisture.0),
                param_f32(object, "MoistureMax", d.moisture.1),
            ),
            fog_type: param_i32(object, "FogType", d.fog_type),
        }
    }

    /// The palette sets (`EnvAttribute_N`) of the sky states clear, overcast
    /// and change of day ([`SKY_CLEAR`]…), from the game's table.
    pub fn palette_sets(&self) -> [usize; 3] {
        row_palette_sets(self.palette_set)
    }

    /// The temperature at `height` metres like `Manager::calcTempDay`:
    /// interpolated between the 100 m steps, held beyond 0 and 1000 m.
    pub fn temperature(&self, height: f32, night: bool) -> f32 {
        let table = if night {
            &self.temperature_night
        } else {
            &self.temperature_day
        };
        let steps = (height / 100.0).clamp(0.0, 10.0);
        let below = steps.floor() as usize;
        let above = (below + 1).min(10);
        let t = steps - below as f32;
        // The table runs from 1000 m down to 0 m.
        table[10 - below] + (table[10 - above] - table[10 - below]) * t
    }
}

/// The environment parameters from `normal.bwinfo`.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct EnvParams {
    pub palettes: Vec<EnvPalette>,
    /// What the palettes share (`EnvPaletteStatic`).
    pub palette_static: EnvPaletteStatic,
    /// Palette sets: a palette index for each sky division.
    pub palette_sets: Vec<[usize; DIVISIONS]>,
    pub sun: SunParams,
    /// The sky's cloud layers, if the file has them.
    pub clouds: Option<SkyClouds>,
    /// The climates (`ClimateDefines_N`), in [`crate::eco::CLIMATES`] order.
    pub climates: Vec<Climate>,
    /// How each weather changes the light (`WeatherInfluence_N`): clear,
    /// cloudy, rain, heavy rain or storm.
    pub weather_influences: Vec<Influence>,
}

impl EnvParams {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let io = ParameterIO::from_binary(bytes)
            .map_err(|_| FormatError::Invalid("bwinfo: not AAMP"))?;
        let objects = &io.param_root.objects;
        let palettes: Vec<EnvPalette> = (0..)
            .map_while(|i| objects.get(format!("EnvPalette_{i}").as_str()))
            .enumerate()
            .map(|(i, object)| EnvPalette::from_object(object, i))
            .collect();
        if palettes.is_empty() {
            return Err(FormatError::Invalid("bwinfo: no EnvPalette objects"));
        }
        let palette_sets = (0..)
            .map_while(|i| objects.get(format!("EnvAttribute_{i}").as_str()))
            .enumerate()
            .map(|(set, attribute)| {
                std::array::from_fn(|division| {
                    let selected = attribute
                        .get(format!("PaletteSel{division:02}").as_str())
                        .and_then(|p| p.as_i32().ok());
                    // The game's default selection is 8 × set + division
                    // (`initEnvAttribute`); a negative one reads as past
                    // the palettes ([`Self::palette`]).
                    let index = selected.unwrap_or((set * DIVISIONS + division) as i32);
                    usize::try_from(index).unwrap_or(usize::MAX)
                })
            })
            .collect();
        let sun = objects
            .get("SunParam")
            .map_or_else(SunParams::default, |object| {
                let d = SunParams::default();
                let float = |name: &str, fallback: f32| {
                    object
                        .get(name)
                        .and_then(|p| p.as_f32().ok())
                        .unwrap_or(fallback)
                };
                SunParams {
                    sun_scale: float("SunScale", d.sun_scale),
                    moon_scale: float("MoonScale", d.moon_scale),
                    distance: float("SunMoonDispDist", d.distance),
                    slope: float("SunSlope", d.slope),
                    dir_y_stop: float("SunDirYStop", d.dir_y_stop),
                }
            });
        let clouds = SkyClouds::parse(objects);
        let climates = (0..)
            .map_while(|i| objects.get(format!("ClimateDefines_{i}").as_str()))
            .map(Climate::from_object)
            .collect();
        let weather_influences = (0..)
            .map_while(|i| objects.get(format!("WeatherInfluence_{i}").as_str()))
            .map(Influence::from_object)
            .collect();
        let palette_static = objects
            .get("EnvPaletteStatic")
            .map_or_else(EnvPaletteStatic::default, EnvPaletteStatic::from_object);
        Ok(Self {
            palettes,
            palette_static,
            palette_sets,
            sun,
            clouds,
            climates,
            weather_influences,
        })
    }

    /// Reads the parameters from `Pack/TitleBG.pack`; `None` if the dump has
    /// no such pack.
    pub fn load(roots: &ContentRoots) -> Result<Option<Self>> {
        let Some(path) = roots.find("Pack/TitleBG.pack") else {
            return Ok(None);
        };
        let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
        let pack = crate::yaz0::decompress_if(&pack)?;
        let sarc = roead::sarc::Sarc::new(&pack[..])?;
        let data = sarc.get_data(WORLD_INFO).ok_or(FormatError::Invalid(
            "TitleBG.pack has no WorldMgr/normal.bwinfo",
        ))?;
        Self::parse(&crate::yaz0::decompress_if(data)?).map(Some)
    }

    /// The palette index of palette set `set` at sky division `division`,
    /// like `ENV_GetPaletteIndexOfSetDivision` (`0x0364228c`, Wii U v208):
    /// the set's `PaletteSel{division}`, set 0's past the game's 59 sets; a
    /// set the document lacks keeps the default 8 × set + division.
    pub fn palette_index(&self, set: usize, division: usize) -> usize {
        let set = if set < PALETTE_SETS { set } else { 0 };
        let division = division % DIVISIONS;
        self.palette_sets
            .get(set)
            .map_or(set * DIVISIONS + division, |sel| sel[division])
    }

    /// The palette of palette set `set` at sky division `division`
    /// ([`Self::palette_index`]). Past the game's 207 palettes the weather
    /// update reads palette 0 (`0x036425b8`); one the document lacks keeps
    /// the built-in defaults.
    pub fn palette(&self, set: usize, division: usize) -> &EnvPalette {
        static DEFAULT: std::sync::LazyLock<EnvPalette> =
            std::sync::LazyLock::new(EnvPalette::default);
        match self.palette_index(set, division) {
            index if index >= PALETTES => &self.palettes[0],
            index => self.palettes.get(index).unwrap_or(&DEFAULT),
        }
    }

    /// The environment at `hours` (0–24) in palette set `set`, blended
    /// between divisions like the game.
    pub fn at(&self, set: usize, hours: f32) -> EnvPalette {
        let (previous, current, t) = sky_division(hours);
        self.palette(set, previous)
            .lerp(self.palette(set, current), t)
    }
}

/// The sky division at `hours` (0–24): the previous division, the current
/// one, and how far the blend from the previous one has got (0–1).
///
/// Divisions: 0 03:00 before dawn, 1 04:00 dawn, 2 06:00 morning, 3 09:00
/// day, 4 16:00 afternoon, 5 18:00 sunset, 6 20:00 dusk, 7 21:00 night. Each
/// blends in over its first hour (40 minutes for 0 and 6).
pub fn sky_division(hours: f32) -> (usize, usize, f32) {
    let h = hours.rem_euclid(24.0);
    let ramp = |start: f32, length: f32| ((h - start) / length).clamp(0.0, 1.0);
    let (division, t) = match h {
        h if (3.0..4.0).contains(&h) => (0, ramp(3.0, 40.0 / 60.0)),
        h if (4.0..6.0).contains(&h) => (1, ramp(4.0, 1.0)),
        h if (6.0..9.0).contains(&h) => (2, ramp(6.0, 1.0)),
        h if (9.0..16.0).contains(&h) => (3, ramp(9.0, 1.0)),
        h if (16.0..18.0).contains(&h) => (4, ramp(16.0, 1.0)),
        h if (18.0..20.0).contains(&h) => (5, ramp(18.0, 1.0)),
        h if (20.0..21.0).contains(&h) => (6, ramp(20.0, 40.0 / 60.0)),
        h if (21.0..22.0).contains(&h) => (7, ramp(21.0, 1.0)),
        _ => (7, 1.0),
    };
    ((division + DIVISIONS - 1) % DIVISIONS, division, t)
}

/// The sky states, the columns of [`PALETTE_SET_ROWS`]: clear, overcast and
/// the change of day (around midnight).
pub const SKY_CLEAR: usize = 0;
pub const SKY_OVERCAST: usize = 1;
pub const SKY_DAY_CHANGE: usize = 2;

/// The palette set (`EnvAttribute_N`) for each sky state, one row per
/// climate's `PaletteSetSelect`. Not in the data: the game's static table at
/// `0x1030024c` (Wii U v208 `U-King.rpx`), copied by the weather update
/// `0x036425b8`, which falls back to set 0 past its 57 rows. Most climates
/// use row 0, the field: set 0 when clear, 1 overcast, 2 at the change of
/// day.
/// The palette sets of row `row` of [`PALETTE_SET_ROWS`] (set 0 past them,
/// as the game falls back).
pub fn row_palette_sets(row: usize) -> [usize; 3] {
    PALETTE_SET_ROWS.get(row).copied().unwrap_or([0; 3])
}

#[rustfmt::skip]
pub const PALETTE_SET_ROWS: [[usize; 3]; 57] = [
    [0, 1, 2], [3, 3, 3], [4, 4, 2], [6, 6, 6], [5, 5, 5], [8, 8, 8],
    [9, 9, 9], [10, 10, 10], [11, 12, 11], [0, 0, 2], [13, 13, 13], [0, 1, 2],
    [14, 14, 14], [15, 15, 2], [16, 16, 16], [17, 17, 2], [18, 18, 18], [19, 19, 19],
    [20, 20, 20], [21, 21, 21], [22, 22, 22], [23, 23, 23], [24, 24, 24], [25, 25, 25],
    [26, 26, 26], [27, 27, 27], [28, 28, 28], [29, 29, 29], [30, 30, 30], [31, 31, 31],
    [32, 32, 32], [33, 33, 33], [34, 34, 34], [35, 35, 35], [36, 36, 36], [37, 37, 37],
    [38, 38, 38], [39, 39, 39], [40, 40, 40], [41, 41, 41], [42, 42, 42], [43, 43, 43],
    [44, 44, 44], [45, 45, 45], [46, 46, 46], [47, 47, 47], [48, 48, 48], [49, 49, 49],
    [50, 50, 50], [51, 51, 51], [52, 52, 52], [53, 53, 53], [54, 54, 54], [55, 55, 55],
    [56, 56, 56], [57, 57, 57], [58, 58, 58],
];

/// Whether palette set `set` is one of the field's, where the game gives
/// the height and ad hoc fog (`fog_scatter`) the air's moisture as their
/// strength in place of the palette's `FogColor` alpha: sets 0 and 1 (the
/// field clear and overcast), 21–37 and 48–55 (`WEATHER_IsFieldPaletteSet`
/// `0x03642434`, Wii U v208 `U-King.rpx`).
pub fn is_field_palette_set(set: usize) -> bool {
    matches!(set, 0 | 1 | 21..=37 | 48..=55)
}

#[cfg(test)]
mod tests {
    use roead::aamp::ParameterList;
    use roead::types::Color;

    use super::*;

    fn palette_object(intensity: f32, shadows: bool) -> ParameterObject {
        ParameterObject::new()
            .with_parameter(
                "BgDifColor",
                Parameter::Color(Color {
                    r: 1.0,
                    g: 0.5,
                    b: 0.25,
                    a: 1.0,
                }),
            )
            .with_parameter("BgDifIntencity", Parameter::F32(intensity))
            .with_parameter(
                "SkySunColor",
                Parameter::Color(Color {
                    r: 0.3,
                    g: 0.6,
                    b: 1.0,
                    a: 12.0,
                }),
            )
            .with_parameter("CloudShadowOnOff", Parameter::Bool(shadows))
            .with_parameter("SkyRParam_mie_amplifier", Parameter::F32(intensity * 4.0))
            .with_parameter("SfParam_attenuation", Parameter::F32(intensity * 10.0))
            .with_parameter("afParam_attenuationForGrd", Parameter::F32(4.0))
            .with_parameter(
                "YFogColor",
                Parameter::Color(Color {
                    r: 1.4,
                    g: 1.0,
                    b: 0.3,
                    a: 0.4,
                }),
            )
            .with_parameter("BloomThreshhold", Parameter::F32(intensity / 10.0))
            .with_parameter("VolumeMaskIntencity", Parameter::F32(2.0))
    }

    fn synthetic_info() -> Vec<u8> {
        let mut root = ParameterList::default();
        for i in 0..16 {
            root.objects.insert(
                format!("EnvPalette_{i}").as_str(),
                palette_object(i as f32, i % 2 == 0),
            );
        }
        // Set 0 uses palettes 8-15 in reverse; set 1 has no selection (defaults).
        let mut set0 = ParameterObject::new();
        for division in 0..8 {
            set0.insert(
                format!("PaletteSel{division:02}").as_str(),
                Parameter::I32(15 - division),
            );
        }
        root.objects.insert("EnvAttribute_0", set0);
        root.objects
            .insert("EnvAttribute_1", ParameterObject::new());
        root.objects.insert(
            "SunParam",
            ParameterObject::new().with_parameter("MoonScale", Parameter::F32(2300.0)),
        );
        root.objects.insert(
            "EnvPaletteStatic",
            ParameterObject::new()
                .with_parameter("SfParam_far", Parameter::F32(30_000.0))
                .with_parameter("mie_baseHeight", Parameter::F32(1.0))
                .with_parameter("BloomLayerColorNoUse_16_16", Parameter::I32(1))
                .with_parameter(
                    "BloomLayerColor_64_64",
                    Parameter::Color(Color {
                        r: 0.5,
                        g: 0.5,
                        b: 0.5,
                        a: 0.25,
                    }),
                ),
        );
        root.objects.insert(
            "PrCloud_0",
            ParameterObject::new()
                .with_parameter("ScrollSpd", Parameter::F32(-0.9))
                .with_parameter("NoiseAdd1", Parameter::F32(-1.0))
                .with_parameter("NoiseAdd2_side", Parameter::F32(3.0))
                .with_parameter("PosDensityChgRange", Parameter::F32(0.7))
                .with_parameter("PosDensityChgPower", Parameter::F32(-0.425))
                .with_parameter("PosDensityChgSpeed", Parameter::F32(0.001))
                .with_parameter("BaseTextureNo", Parameter::I32(0))
                .with_parameter("NoiseTextureNo", Parameter::I32(1)),
        );
        let layer = |height: f32| {
            ParameterObject::new()
                .with_parameter("SkyHeight", Parameter::F32(height))
                .with_parameter("SkyHeightMin", Parameter::F32(height - 500.0))
                .with_parameter("SkyHeightMax", Parameter::F32(height + 500.0))
                .with_parameter("SkyHeightSinSeedAdd", Parameter::F32(0.01))
        };
        root.objects.insert("PrCloudV0_0", layer(8000.0));
        root.objects.insert("PrCloudV1_0", layer(7000.0));
        root.objects
            .insert("ClimateDefines_0", ParameterObject::new());
        root.objects.insert(
            "ClimateDefines_1",
            ParameterObject::new()
                .with_parameter("WeatherBlueskyRate", Parameter::I32(100))
                .with_parameter("DayLockBlueSky", Parameter::Bool(true))
                .with_parameter(
                    "FeatureColor",
                    Parameter::Color(Color {
                        r: 1.25,
                        g: 1.3,
                        b: 1.1,
                        a: 1.0,
                    }),
                )
                .with_parameter("CalcMie", Parameter::F32(2.0))
                .with_parameter("PaletteSetSelect", Parameter::I32(7))
                .with_parameter("ClimateTemperatureDay_0000", Parameter::F32(40.0))
                .with_parameter("ClimateTemperatureDay_0100", Parameter::F32(30.0))
                .with_parameter("CalcSfParamNear", Parameter::F32(-50.0))
                .with_parameter("WindPower", Parameter::F32(10.0))
                .with_parameter("FogType", Parameter::I32(1)),
        );
        root.objects
            .insert("WeatherInfluence_0", ParameterObject::new());
        root.objects.insert(
            "WeatherInfluence_1",
            ParameterObject::new()
                .with_parameter("CalcRayleigh", Parameter::F32(0.5))
                .with_parameter(
                    "FeatureFogColor",
                    Parameter::Color(Color {
                        r: 0.75,
                        g: 0.875,
                        b: 1.0,
                        a: 1.0,
                    }),
                )
                .with_parameter("BloomIntencity", Parameter::F32(1.25)),
        );
        ParameterIO {
            version: 0,
            data_type: "winfo".into(),
            param_root: root,
        }
        .to_binary()
    }

    #[test]
    fn divisions_follow_the_game() {
        assert_eq!(sky_division(12.0), (2, 3, 1.0));
        assert_eq!(sky_division(9.5), (2, 3, 0.5));
        assert_eq!(sky_division(0.0), (6, 7, 1.0));
        assert_eq!(sky_division(24.0 + 3.0 + 20.0 / 60.0).1, 0);
        let (previous, current, t) = sky_division(3.0 + 20.0 / 60.0);
        assert_eq!((previous, current), (7, 0));
        assert!((t - 0.5).abs() < 1e-4);
        assert_eq!(sky_division(20.9), (5, 6, 1.0));
        assert_eq!(sky_division(21.25), (6, 7, 0.25));
    }

    #[test]
    fn cloud_shadow_switch_blends_as_a_number() {
        let params = EnvParams::parse(&synthetic_info()).unwrap();
        let (off, on) = (&params.palettes[3], &params.palettes[2]);
        assert_eq!((off.cloud_shadow_on, on.cloud_shadow_on), (0.0, 1.0));
        assert_eq!(on.lerp(off, 0.25).cloud_shadow_on, 0.75);
    }

    #[test]
    fn parses_palettes_and_sets() {
        let params = EnvParams::parse(&synthetic_info()).unwrap();
        assert_eq!(params.palettes.len(), 16);
        let p = &params.palettes[3];
        assert_eq!(p.light_color, [1.0, 0.5, 0.25]);
        assert_eq!(p.light_intensity, 3.0);
        assert_eq!(p.sky_sun_intensity, 12.0);
        assert_eq!(p.cloud_shadow_on, 0.0);
        assert_eq!(p.mie_amplifier, 12.0);
        assert_eq!(p.rayleigh_amplifier, 1.0);
        // Missing parameters keep the game's defaults.
        assert_eq!(p.fog_end, 600.0);
        assert_eq!(params.palette_sets[0], [15, 14, 13, 12, 11, 10, 9, 8]);
        assert_eq!(params.palette_sets[1], [8, 9, 10, 11, 12, 13, 14, 15]);
        assert_eq!(params.palette(0, 3).light_intensity, 12.0);
        // Past the game's 59 sets: set 0's selection.
        assert_eq!(params.palette_index(70, 3), 12);
        // A set the document lacks keeps its default selection; a palette the
        // document lacks the defaults, one past the game's 207 palette 0.
        assert_eq!(params.palette_index(5, 2), 42);
        assert_eq!(params.palette(5, 2), &EnvPalette::default());
        let mut far = params.clone();
        far.palette_sets[1][0] = 300;
        assert_eq!(far.palette(1, 0), &params.palettes[0]);
        assert_eq!(params.sun.moon_scale, 2300.0);
        assert_eq!(params.sun.sun_scale, 2400.0);
    }

    #[test]
    fn cloud_defaults_follow_the_layer_look_and_palette() {
        let far = CloudLayer::game_default(2, 1);
        assert_eq!(
            (far.height.value, far.emboss_width, far.highlight_power),
            (10_000.0, 0.08904, 20.0)
        );
        // Swaying bounds of their own, not the value's.
        let thin = CloudLayer::game_default(1, 0).density;
        assert_eq!((thin.value, thin.min, thin.max), (0.2, 0.05, 0.05));
        // The rows' backlight power by the palette's number.
        let empty = ParameterObject::new();
        for (index, power) in [(7, 1.6), (8, 1.2), (15, 1.2), (16, 1.6)] {
            let palette = EnvPalette::from_object(&empty, index);
            assert_eq!(palette.cloud_backlight_power, power, "palette {index}");
            assert_eq!(
                palette.lower_cloud.backlight_power, power,
                "palette {index}"
            );
        }
    }

    #[test]
    fn the_sky_sun_and_the_cloud_rows_follow_their_flags() {
        let color = |r: f32, a: f32| {
            Parameter::Color(Color {
                r,
                g: 0.5,
                b: 0.5,
                a,
            })
        };
        let palette = |sun_no_use: u32, cloud2_no_use: u32| {
            EnvPalette::from_object(
                &ParameterObject::new()
                    .with_parameter("BgDifColor", color(0.9, 1.0))
                    .with_parameter("SkySunColorNoUse", Parameter::U32(sun_no_use))
                    .with_parameter("SkySunColor", color(0.3, 12.0))
                    .with_parameter("Cloud2NoUse", Parameter::U32(cloud2_no_use))
                    .with_parameter("Cloud0_ColorBase", color(0.1, 1.0))
                    .with_parameter("Cloud0_IntencityBase", Parameter::F32(0.15))
                    .with_parameter("Cloud2_ColorBase", color(0.2, 1.0))
                    .with_parameter("Cloud2_IntencityBase", Parameter::F32(0.1)),
                0,
            )
        };
        let used = palette(0, 0);
        assert_eq!((used.sky_sun_color[0], used.sky_sun_intensity), (0.3, 12.0));
        // The upper layer takes row 0, the lower row 2.
        assert_eq!((used.cloud_base[0], used.cloud_base_intensity), (0.1, 0.15));
        assert_eq!(
            (used.lower_cloud.base[0], used.lower_cloud.base_intensity),
            (0.2, 0.1)
        );
        // Flags set: the main light's colour at the sun's intensity; row 2
        // on both layers.
        let unused = palette(1, 1);
        assert_eq!(
            (unused.sky_sun_color[0], unused.sky_sun_intensity),
            (0.9, 12.0)
        );
        assert_eq!(unused.upper_cloud(), unused.lower_cloud);
        assert_eq!(unused.cloud_base[0], 0.2);
    }

    #[test]
    fn parses_fog_bloom_and_static_fields() {
        let params = EnvParams::parse(&synthetic_info()).unwrap();
        let p = &params.palettes[4];
        assert_eq!(p.scatter_attenuation, 40.0);
        assert_eq!(p.attenuation_ground, 4.0);
        assert_eq!(p.yfog_color, [1.4, 1.0, 0.3, 0.4]);
        assert_eq!(p.bloom.threshold, 0.4);
        assert_eq!(p.volume_mask_intensity, 2.0);
        // Missing parameters keep the game's defaults.
        assert_eq!(p.scatter_horizontal, 2.4);
        assert_eq!(p.bloom.clamped_luminance, 64.0);
        let blend = params.palettes[4].lerp(&params.palettes[6], 0.5);
        assert!((blend.scatter_attenuation - 50.0).abs() < 1e-4);
        assert!((blend.bloom.threshold - 0.5).abs() < 1e-6);
        let s = &params.palette_static;
        assert_eq!(
            (s.scatter_far, s.mie_base_height, s.scatter_density),
            (30_000.0, 1.0, 0.85)
        );
        assert_eq!(s.bloom_layers[1], ([1.0; 4], true));
        assert_eq!(s.bloom_layers[3], ([0.5, 0.5, 0.5, 0.25], true));
        assert_eq!(s.bloom_layers[0], ([1.0, 1.0, 1.0, 0.5], false));
    }

    #[test]
    fn parses_cloud_layers() {
        let clouds = EnvParams::parse(&synthetic_info()).unwrap().clouds.unwrap();
        let [upper, lower] = &clouds.layers;
        assert_eq!(upper.scroll_speed, -0.9);
        assert_eq!(upper.noise_add, [-1.0, -0.2, -0.2, 3.0]);
        assert_eq!(
            upper.density_spot,
            DensitySpot {
                range: 0.7,
                power: -0.425,
                speed: 0.001
            }
        );
        // `PrCloudV1_0` is the upper layer under a cloudy sky.
        assert_eq!(
            upper.looks[1].height,
            Sway {
                value: 7000.0,
                min: 6500.0,
                max: 7500.0,
                rate: 0.01
            }
        );
        assert_eq!(upper.looks[0].height.centre(), 8000.0);
        assert_eq!(upper.looks[0].height.half_swing(), 500.0);
        assert_eq!(
            upper.looks[0].height.at(std::f32::consts::FRAC_PI_2),
            8500.0
        );
        // Missing parameters keep the game's defaults: the upper layer's
        // own, and the lower layer's (no `PrCloud_2`, no `PrCloudV*_2`).
        assert_eq!(upper.looks[0].density, Sway::fixed(0.3));
        assert_eq!(upper.wind_add, 0.262);
        assert_eq!(upper.looks[1].emboss_width, 0.06);
        assert_eq!(upper.looks[1].alpha_threshold, Sway::fixed(0.95));
        assert_eq!(
            (
                upper.looks[1].dark_side_noise,
                upper.looks[1].light_side_noise
            ),
            (0.5, 1.0)
        );
        assert_eq!(*lower, SkyCloudLayer::game_default(2));
        assert_eq!(lower.looks[1].height, Sway::fixed(10_000.0));
        assert_eq!(
            upper.textures,
            CloudTextureNumbers {
                base: 0,
                base_blend: 4,
                noise: 1,
                noise_blend: 3
            }
        );
        assert_eq!(lower.textures, CloudTextureNumbers::game_default(2));
    }

    #[test]
    fn parses_climates_and_weather() {
        let params = EnvParams::parse(&synthetic_info()).unwrap();
        assert_eq!(params.climates.len(), 2);
        assert_eq!(params.climates[0], Climate::default());
        let desert = &params.climates[1];
        assert_eq!(desert.weather_rates, [100, 20, 15, 5, 0]);
        assert!(desert.day_lock_blue_sky && !desert.night_lock_blue_sky);
        assert_eq!(desert.influence.feature_color, [1.25, 1.3, 1.1]);
        assert_eq!(
            (desert.influence.mie, desert.influence.rayleigh),
            (2.0, 1.0)
        );
        assert_eq!(desert.palette_set, 7);
        // 0 m is the last entry; halfway to 100 m is halfway between.
        assert_eq!(desert.temperature(-5.0, false), 40.0);
        assert_eq!(desert.temperature(50.0, false), 35.0);
        assert_eq!(desert.temperature(5000.0, true), -63.0);
        assert_eq!(
            (
                desert.influence.scatter_near,
                desert.wind_power,
                desert.fog_type
            ),
            (-50.0, 10.0, 1)
        );
        assert_eq!(params.climates[0].wind_power, 5.0);
        assert_eq!(params.weather_influences.len(), 2);
        let cloudy = params.weather_influences[1];
        assert_eq!(cloudy.rayleigh, 0.5);
        assert_eq!(cloudy.feature_fog_color, [0.75, 0.875, 1.0]);
        assert_eq!(
            (cloudy.bloom_intensity, cloudy.bloom_threshold),
            (1.25, 1.0)
        );
    }

    #[test]
    fn mixes_and_stacks_influences() {
        let params = EnvParams::parse(&synthetic_info()).unwrap();
        let (clear, cloudy) = (params.weather_influences[0], params.weather_influences[1]);
        let mean = Influence::weighted_mean([(clear, 3.0), (cloudy, 1.0)]);
        assert!((mean.rayleigh - 0.875).abs() < 1e-6);
        assert!((mean.feature_fog_color[0] - 0.9375).abs() < 1e-6);
        assert_eq!(Influence::weighted_mean([]), Influence::default());
        let desert = params.climates[1].influence;
        let stacked = desert.then(&cloudy);
        assert_eq!(stacked.feature_color, [1.25, 1.3, 1.1]);
        assert_eq!(
            (stacked.mie, stacked.rayleigh, stacked.scatter_near),
            (2.0, 0.5, -50.0)
        );
        assert_eq!(stacked.feature_fog_color, [0.75, 0.875, 1.0]);
    }

    #[test]
    fn blends_between_divisions() {
        let params = EnvParams::parse(&synthetic_info()).unwrap();
        // 09:30: halfway from division 2 (palette 13) to 3 (palette 12).
        let p = params.at(0, 9.5);
        assert!((p.light_intensity - 12.5).abs() < 1e-5);
        assert_eq!(params.at(0, 12.0).light_intensity, 12.0);
        assert!(EnvParams::parse(b"not aamp").is_err());
    }

    #[test]
    fn climates_pick_palette_sets_by_the_sky() {
        // The field: clear, overcast and change-of-day sets 0, 1, 2.
        assert_eq!(Climate::default().palette_sets(), [0, 1, 2]);
        let row = |palette_set| {
            Climate {
                palette_set,
                ..Climate::default()
            }
            .palette_sets()
        };
        assert_eq!(row(7)[SKY_OVERCAST], 10);
        // Past the table the game falls back to set 0.
        assert_eq!(row(PALETTE_SET_ROWS.len()), [0; 3]);
        // The field's sets take the moisture as the fog's strength; the
        // change of day, the woods and the villages do not.
        let field: Vec<usize> = (0..60).filter(|&set| is_field_palette_set(set)).collect();
        assert_eq!(field.len(), 2 + 17 + 8);
        assert!(!is_field_palette_set(2) && !is_field_palette_set(10));
        assert!(is_field_palette_set(21) && is_field_palette_set(55));
    }
}
