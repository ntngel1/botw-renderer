//! Mirror of `botw-formats::env`'s types and their runtime methods; the
//! parsing stays there. `bake` serializes the parsed values to RON, which
//! these deserialize.
//!
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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

/// Sun and moon sprites (`SunParam`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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
}

/// Which of the renderer's cloud textures a layer draws with: numbers in
/// the list of the environment's textures ([`crate::envset::EnvSet::cloud_textures`]).
/// `SkyMgr` copies them from `PrCloud_0` into the upper layer (`CloudParam0`)
/// and from `PrCloud_2` into the lower one (`CloudParam2`), and turns the
/// blending of the two base textures on (`FUN_03659fa8`, Wii U v208).
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
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
}

/// A patch where a layer thins or thickens (`PosDensityChgRange`,
/// `PosDensityChgPower`, `PosDensityChgSpeed` of `PrCloud_N`): `SkyMgr`
/// puts it at a random spot of the dome, grows its strength from 0 to
/// `power` at `speed` a frame, fades it back and moves it (`FUN_0365867c`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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
}

/// The sky's clouds as `SkyMgr` writes them into the renderer's layers
/// every frame (`FUN_0365867c`, Wii U v208, from `FUN_0365acb0`): the upper
/// layer from `PrCloud_0`/`PrCloudV*_0`, the lower from `PrCloud_2`/
/// `PrCloudV*_2`. The per-weather `SkyPalette0_N`/`SkyPalette2_N` take over
/// only while `SkyMgr+0x2188` is set, which nothing but resets writes (0) in
/// v208, and `CloudPat*` only in palette sets 16/17 and under the Blood Moon
/// (`FUN_03655de8`): neither is read here.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
pub struct SkyClouds {
    pub layers: [SkyCloudLayer; 2],
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq)]
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
