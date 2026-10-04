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

use roead::aamp::{Parameter, ParameterIO, ParameterList, ParameterObject};

use crate::bfres::{Bfres, TextureImage, assemble_whole_texture};
use crate::content::ContentRoots;
use crate::{FormatError, Result};

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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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

    /// Parses the set (the SARC, Yaz0-compressed or not).
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let bytes = crate::yaz0::decompress_if(bytes)?;
        let sarc = roead::sarc::Sarc::new(&bytes[..])?;
        let mut set = Self::fallback();
        let mut document = |name: &'static str| -> Result<Option<ParameterIO>> {
            let Some(data) = sarc.get_data(name) else {
                return Ok(None);
            };
            let data = crate::yaz0::decompress_if(data)?;
            let io = ParameterIO::from_binary(&data[..])
                .map_err(|_| FormatError::Invalid("env.sgenvb: entry is not AAMP"))?;
            set.read.push(name);
            Ok(Some(io))
        };
        let objects = document(LIGHTS)?.map(|io| EnvObjects::from_root(&io.param_root));
        let color =
            document(COLOR_CORRECTION)?.map(|io| ColorCorrection::from_root(&io.param_root));
        let bloom = document(BLOOM)?.map(|io| Bloom::from_root(&io.param_root));
        let sky = document(SKY)?.map(|io| SkyScatter::from_root(&io.param_root));
        let clouds = document(CLOUDS)?.map(|io| CloudDome::from_root(&io.param_root));
        let light_maps = document(LIGHT_MAPS)?.map(|io| LightMaps::from_root(&io.param_root));
        let shadows = document(SHADOWS)?.map(|io| Shadows::from_root(&io.param_root));
        let index = document(SET_INDEX)?;
        if let Some(objects) = objects {
            set.objects = objects;
        }
        if let Some(color) = color {
            set.color = color;
        }
        if let Some(bloom) = bloom {
            set.bloom = bloom;
        }
        if let Some(sky) = sky {
            set.sky = sky;
        }
        if let Some(clouds) = clouds {
            set.clouds = clouds;
        }
        if let Some(light_maps) = light_maps {
            set.light_maps = light_maps;
        }
        if let Some(shadows) = shadows {
            set.shadows = shadows;
        }
        set.cloud_shadow_texture =
            index.and_then(|io| TextureRef::find(&io.param_root, FIELD_SET, PROJECTION_TEXTURE));
        Ok(set)
    }

    /// Reads the set from `Pack/Bootup.pack`; `None` if the dump has no such
    /// pack or the pack has no set.
    pub fn load(roots: &ContentRoots) -> Result<Option<Self>> {
        let Some(path) = roots.find(ENV_PACK) else {
            return Ok(None);
        };
        let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
        let pack = crate::yaz0::decompress_if(&pack)?;
        let sarc = roead::sarc::Sarc::new(&pack[..])?;
        match sarc.get_data(ENV_SET) {
            Some(data) => Self::parse(data).map(Some),
            None => Ok(None),
        }
    }

    /// The texture `reference` of the set (the SARC, as for [`Self::parse`]),
    /// with all its levels; `None` if the set has no such file or texture.
    pub fn texture(bytes: &[u8], reference: &TextureRef) -> Result<Option<TextureImage>> {
        let bytes = crate::yaz0::decompress_if(bytes)?;
        let sarc = roead::sarc::Sarc::new(&bytes[..])?;
        let Some(data) = sarc.get_data(&reference.file) else {
            return Ok(None);
        };
        let data = crate::yaz0::decompress_if(data)?;
        let bfres = Bfres::parse(&data)?;
        bfres
            .texture(&reference.name)?
            .map(|texture| assemble_whole_texture(&texture))
            .transpose()
    }

    /// The environment's textures in the order the renderer lists them for
    /// the clouds (`agl::fx::Cloud+0x4bd8`, filled by `FUN_03a5b778` from
    /// `FUN_039cea0c`, Wii U v208): every texture of [`TEXTURES`] in file
    /// order but those whose name starts with `cloudTexture` (the check is
    /// case-sensitive: the field's `cloudtexture02`–`04` stay), at most 16.
    /// A layer's texture number picks from this list; a number past the
    /// second-to-last entry picks the first ([`Self::cloud_texture_index`]).
    pub fn cloud_textures(bytes: &[u8]) -> Result<Vec<TextureImage>> {
        let bytes = crate::yaz0::decompress_if(bytes)?;
        let sarc = roead::sarc::Sarc::new(&bytes[..])?;
        let Some(data) = sarc.get_data(TEXTURES) else {
            return Ok(Vec::new());
        };
        let data = crate::yaz0::decompress_if(data)?;
        let bfres = Bfres::parse(&data)?;
        let mut list = Vec::new();
        for texture in bfres.textures()? {
            if texture.name.starts_with("cloudTexture") || list.len() == 16 {
                continue;
            }
            list.push(assemble_whole_texture(&texture)?);
        }
        Ok(list)
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

    /// [`Self::cloud_textures`] from the set in `Pack/Bootup.pack`.
    pub fn load_cloud_textures(roots: &ContentRoots) -> Result<Vec<TextureImage>> {
        let Some(path) = roots.find(ENV_PACK) else {
            return Ok(Vec::new());
        };
        let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
        let pack = crate::yaz0::decompress_if(&pack)?;
        let sarc = roead::sarc::Sarc::new(&pack[..])?;
        match sarc.get_data(ENV_SET) {
            Some(data) => Self::cloud_textures(data),
            None => Ok(Vec::new()),
        }
    }

    /// [`Self::texture`] from the set in `Pack/Bootup.pack`.
    pub fn load_texture(
        roots: &ContentRoots,
        reference: &TextureRef,
    ) -> Result<Option<TextureImage>> {
        let Some(path) = roots.find(ENV_PACK) else {
            return Ok(None);
        };
        let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
        let pack = crate::yaz0::decompress_if(&pack)?;
        let sarc = roead::sarc::Sarc::new(&pack[..])?;
        match sarc.get_data(ENV_SET) {
            Some(data) => Self::texture(data, reference),
            None => Ok(None),
        }
    }
}

/// A texture a set refers to (`env.bgenv`, `set_array/N/refer`): a BFRES
/// file of the environment SARC and the texture's name in it.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct TextureRef {
    pub file: String,
    pub name: String,
}

impl TextureRef {
    /// The reference with `signature` of the set named `set` in the index.
    fn find(root: &ParameterList, set: &str, signature: &str) -> Option<Self> {
        let sets = root.lists.get("set_array")?;
        let set =
            sets.lists.iter().map(|(_, list)| list).find(|list| {
                list.objects.get("param").and_then(|o| text(o, "name")) == Some(set)
            })?;
        set.lists
            .get("refer")?
            .objects
            .iter()
            .map(|(_, o)| o)
            .find(|o| text(o, "signature") == Some(signature))
            .and_then(|o| {
                Some(Self {
                    file: text(o, "file")?.to_owned(),
                    name: text(o, "name")?.to_owned(),
                })
            })
    }
}

/// The main light (`dir_main`).
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct HemisphereLight {
    pub sky: [f32; 3],
    pub ground: [f32; 3],
    pub intensity: f32,
    /// The sky's direction (+Y).
    pub direction: [f32; 3],
}

/// A distance fog (`fog_scatter`, `fog_world`, `fog_inner`): colour (alpha:
/// strength) between `start` and `end` metres.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Fog {
    pub name: String,
    pub start: f32,
    pub end: f32,
    pub color: [f32; 4],
}

/// The light and fog objects of `master_field.baglenv`.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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

impl Projector {
    fn from_object(o: &ParameterObject) -> Self {
        Self {
            name: text(o, "name").unwrap_or_default().to_owned(),
            proj_type: o
                .get("proj_type")
                .and_then(|p| p.as_int::<i32>().ok())
                .unwrap_or(0),
            view_pos: vec3(o, "view_pos", [0.0, 1000.0, 0.0]),
            view_at: vec3(o, "view_at", [0.0, 0.0, 0.1]),
            view_up: vec3(o, "view_up", [0.0, 0.0, 1.0]),
            near: float(o, "near", 1.0),
            far: float(o, "far", 10_000.0),
            aspect: float(o, "aspect", 1.0),
            fovy: float(o, "fovy", 45.0),
            height: float(o, "height", 1000.0),
        }
    }
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

    fn from_root(root: &ParameterList) -> Self {
        let d = Self::fallback();
        let named = |list: &str, name: &str| -> Option<&ParameterObject> {
            root.lists
                .get(list)?
                .objects
                .iter()
                .map(|(_, o)| o)
                .find(|o| text(o, "name") == Some(name))
        };
        let main_light = named("DirectionalLight", "dir_main").map_or(d.main_light.clone(), |o| {
            DirectionalLight {
                diffuse: rgb(o, "DiffuseColor", d.main_light.diffuse),
                specular: rgb(o, "SpecularColor", d.main_light.specular),
                backside: rgb(o, "BacksideColor", d.main_light.backside),
                intensity: float(o, "Intensity", d.main_light.intensity),
                direction: vec3(o, "Direction", d.main_light.direction),
            }
        });
        let hemisphere = named("HemisphereLight", "hemi_inner").map_or(d.hemisphere.clone(), |o| {
            HemisphereLight {
                sky: rgb(o, "SkyColor", d.hemisphere.sky),
                ground: rgb(o, "GroundColor", d.hemisphere.ground),
                intensity: float(o, "Intensity", d.hemisphere.intensity),
                direction: vec3(o, "Direction", d.hemisphere.direction),
            }
        });
        let fogs = root.lists.get("Fog").map_or_else(Vec::new, |list| {
            list.objects
                .iter()
                .map(|(_, o)| o)
                .filter(|o| flag(o, "enable", true))
                .map(|o| Fog {
                    name: text(o, "name").unwrap_or_default().to_owned(),
                    start: float(o, "Start", 0.0),
                    end: float(o, "End", 0.0),
                    color: rgba(o, "Color", [1.0; 4]),
                })
                .collect()
        });
        let projectors = root.lists.get("Projector").map_or_else(Vec::new, |list| {
            list.objects
                .iter()
                .map(|(_, o)| o)
                .filter(|o| flag(o, "enable", true))
                .map(Projector::from_object)
                .collect()
        });
        Self {
            main_light,
            hemisphere,
            fogs,
            projectors,
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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

    fn from_root(root: &ParameterList) -> Self {
        let d = Self::fallback();
        let Some(o) = root.objects.get("color_correction") else {
            return d;
        };
        Self {
            enable: flag(o, "enable", d.enable),
            hue: float(o, "hue", d.hue),
            saturation: float(o, "saturation", d.saturation),
            brightness: float(o, "brightness", d.brightness),
            gamma: float(o, "gamma", d.gamma),
            levels: curves(o, "level"),
            toycam_enable: flag(o, "toycam_enable", d.toycam_enable),
            toycam_saturation: [
                float(o, "toycam_saturation1", 1.0),
                float(o, "toycam_saturation2", 1.0),
            ],
            toycam_brightness: float(o, "toycam_brightness", d.toycam_brightness),
            toycam_contrast: float(o, "toycam_contrast", d.toycam_contrast),
            toycam_mul_color: rgb(o, "toycam_mul_color", d.toycam_mul_color),
        }
    }
}

/// The base bloom (`baglblm`, `bloom`). The palettes scale it per time of
/// day (`crate::env::PaletteBloom`).
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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

    fn from_root(root: &ParameterList) -> Self {
        let d = Self::fallback();
        let Some(o) = root.objects.get("bloom") else {
            return d;
        };
        let clamped = float(o, "clamped_luminance", 64.0);
        Self {
            enable: flag(o, "enable", d.enable),
            intensity: float(o, "intensity", d.intensity),
            // SI-FMT-07: bloom field names and curve kinds are our reading.
            threshold: float(o, "threshhold", d.threshold),
            threshold_range: float(o, "threshold_range", d.threshold_range),
            clamped_luminance: flag(o, "enable_clamped_luminance", true).then_some(clamped),
            colors: std::array::from_fn(|i| rgba(o, &format!("color{}", i + 1), d.colors[i])),
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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

    fn from_root(root: &ParameterList) -> Self {
        let d = Self::fallback();
        let change = |o: &ParameterObject, prefix: &str, d: [f32; 3]| {
            [0, 1, 2].map(|i| {
                let suffix = ["Start", "End", "Power"][i];
                float(o, &format!("{prefix}{suffix}"), d[i])
            })
        };
        let layer = |name: &str, d: &CloudDomeLayer| {
            let Some(o) = root.objects.get(name) else {
                return d.clone();
            };
            CloudDomeLayer {
                sky_scale: float(o, "mSkyScale", d.sky_scale),
                scatter_height: float(o, "mScatterHeight", d.scatter_height),
                scatter_ambient: float(o, "mScatterAmb", d.scatter_ambient),
                backlight_param0: float(o, "mBacklightParam0", d.backlight_param0),
                backlight_param1: float(o, "mBacklightParam1", d.backlight_param1),
                far_uv_mul: float(o, "mFarUVMul", d.far_uv_mul),
                far_uv_pow: float(o, "mFarUVPow", d.far_uv_pow),
                far_density: change(o, "mFarDensityChg", d.far_density),
                far_alpha: change(o, "mFarAlphaChg", d.far_alpha),
                noise_scale: [1, 2]
                    .map(|i| float(o, &format!("mNoiseScale{i}"), d.noise_scale[i - 1])),
                noise_density: [1, 2]
                    .map(|i| float(o, &format!("mNoiseDensity{i}"), d.noise_density[i - 1])),
                noise_speed: [0, 1, 2, 3].map(|i| {
                    let name = [
                        "mNoiseSpeed1X",
                        "mNoiseSpeed1Y",
                        "mNoiseSpeed2X",
                        "mNoiseSpeed2Y",
                    ];
                    float(o, name[i], d.noise_speed[i])
                }),
                noise_speed_master: float(o, "mNoiseSpeedMaster", d.noise_speed_master),
                base_scroll_speed: [
                    float(o, "mBaseTexScrollSpdX", d.base_scroll_speed[0]),
                    float(o, "mBaseTexScrollSpdY", d.base_scroll_speed[1]),
                ],
                far_distortion: change(o, "mFarDistotionChg", d.far_distortion),
            }
        };
        Self {
            color_scale: root.objects.get("Cloud").map_or(d.color_scale, |o| {
                float(o, "mCloudColorScale", d.color_scale)
            }),
            layers: [
                layer("CloudParam0", &d.layers[0]),
                layer("CloudParam2", &d.layers[1]),
            ],
        }
    }
}

/// The sky's scattering (`bksky`, `sky`): the physical sky's inputs and its
/// two fogs over the ground — the scattering fog (`scatter_fog_*`) and an
/// "ad hoc" fog (`adhoc_fog_*`).
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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

    fn from_root(root: &ParameterList) -> Self {
        let d = Self::fallback();
        let Some(o) = root.objects.get("sky") else {
            return d;
        };
        Self {
            sun_color: rgba(o, "sun_color", d.sun_color),
            ground_color: rgba(o, "ground_color", d.ground_color),
            rayleigh_base_height: float(o, "rayleigh_base_height", d.rayleigh_base_height),
            mie_base_height: float(o, "mie_base_height", d.mie_base_height),
            mie_scattering: float(o, "mie_scattering_coeff", d.mie_scattering),
            mie_asymmetry: float(o, "mie_symmetrical_prop", d.mie_asymmetry),
            rayleigh_amplifier: float(o, "rayleigh_amplifier_rendering", d.rayleigh_amplifier),
            mie_asymmetry_rendering: float(
                o,
                "mie_symmetrical_prop_rendering",
                d.mie_asymmetry_rendering,
            ),
            mie_amplifier: float(o, "mie_amplifier_rendering", d.mie_amplifier),
            scatter_fog_near: float(o, "scatter_fog_near", d.scatter_fog_near),
            scatter_fog_far: float(o, "scatter_fog_far", d.scatter_fog_far),
            scatter_fog_density: float(o, "scatter_fog_density", d.scatter_fog_density),
            scatter_fog_attenuation: float(o, "scatter_fog_atten", d.scatter_fog_attenuation),
            scatter_fog_horizontal: float(o, "scatter_fog_horz", d.scatter_fog_horizontal),
            adhoc_fog_near: float(o, "adhoc_fog_near", d.adhoc_fog_near),
            adhoc_fog_far: float(o, "adhoc_fog_far", d.adhoc_fog_far),
            adhoc_fog_attenuation_ground: float(
                o,
                "adhoc_fog_atten_grd",
                d.adhoc_fog_attenuation_ground,
            ),
            adhoc_fog_attenuation_sky: float(o, "adhoc_fog_atten_sky", d.adhoc_fog_attenuation_sky),
            adhoc_fog_color: rgb(o, "adhoc_fog_color", d.adhoc_fog_color),
            env_map_amplifier: float(o, "amplifier_for_envmap", d.env_map_amplifier),
        }
    }
}

/// A `sead` curve (`sead::hostio::Curve`): points interpolated by kind.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Curve {
    pub kind: CurveKind,
    /// The used floats (`numUse` of the 30 stored).
    pub values: Vec<f32>,
}

/// `sead::hostio::CurveType`; the light maps and colour levels use the 2D
/// kinds, whose values are points along x.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq, Eq)]
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
    fn from_roead(curve: &roead::types::Curve) -> Self {
        let kind = match curve.b {
            6 => CurveKind::Linear2D,
            7 => CurveKind::Hermit2D,
            8 => CurveKind::Step2D,
            other => CurveKind::Other(other),
        };
        let used = (curve.a as usize).min(curve.floats.len());
        Self {
            kind,
            values: curve.floats[..used].to_vec(),
        }
    }

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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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

    fn from_root(root: &ParameterList) -> Self {
        let curves = root.objects.get("lut_param").map_or_else(Vec::new, |o| {
            (0..)
                .map_while(|i| {
                    let name = text(o, &format!("name{i}"))?.to_owned();
                    let curve = curves(o, &format!("intensity{i}")).into_iter().next()?;
                    Some((name, curve))
                })
                .collect()
        });
        let maps = (0..)
            .map_while(|i| root.lists.get(format!("{i}").as_str()))
            .map(|list| {
                let setting = list.objects.get("setting");
                let s = |name: &str, fallback: f32| {
                    setting.map_or(fallback, |o| float(o, name, fallback))
                };
                let rim = Rim {
                    enable: setting.is_some_and(|o| flag(o, "rim_enable", false)),
                    light: setting
                        .and_then(|o| text(o, "rim_light_ref"))
                        .unwrap_or_default()
                        .to_owned(),
                    strength: s("rim_effect", 1.0),
                    width: s("rim_width", 1.0),
                    angle: s("rim_angle", 1.0),
                    power: s("rim_pow", 2.0),
                };
                let inputs = list
                    .lists
                    .get("env_obj_ref_array")
                    .map_or_else(Vec::new, |refs| {
                        (0..)
                            .map_while(|j| refs.objects.get(format!("{j}").as_str()))
                            .map(|o| LightMapInput {
                                kind: text(o, "type").unwrap_or_default().to_owned(),
                                light: text(o, "name").unwrap_or_default().to_owned(),
                                calc_type: o
                                    .get("calc_type")
                                    .and_then(|p| p.as_int::<i32>().ok())
                                    .unwrap_or(0),
                                lut: text(o, "lut_name").unwrap_or_default().to_owned(),
                                lut_mip1: text(o, "lut_name_mip1").unwrap_or_default().to_owned(),
                                effect: float(o, "effect", 1.0),
                                power: float(o, "pow", 1.0),
                                power_mip_max: float(o, "pow_mip_max", 1.0),
                                mip0: flag(o, "enable_mip0", true),
                                mip1: flag(o, "enable_mip1", false),
                            })
                            .collect()
                    });
                LightMap {
                    name: setting
                        .and_then(|o| text(o, "name"))
                        .unwrap_or_default()
                        .to_owned(),
                    rim,
                    inputs,
                }
            })
            .collect();
        Self { curves, maps }
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
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

    fn from_root(root: &ParameterList) -> Self {
        let d = Self::fallback();
        let light = root
            .objects
            .get("depth_shadow_parameter_0")
            .and_then(|o| text(o, "light_name"))
            .map_or(d.light, str::to_owned);
        let projection = root.objects.get("projection_shadow_0");
        let ssao = root
            .objects
            .get("ssao_parameter")
            .map_or(d.ssao.clone(), |o| Ssao {
                enable: flag(o, "is_enable", d.ssao.enable),
                radius: float(o, "radius", d.ssao.radius),
                far: float(o, "ao_far", d.ssao.far),
                distance_attenuation: flag(o, "enable_dist_attn", true)
                    .then(|| float(o, "dist_attn", 1.0)),
                density: float(o, "density", d.ssao.density),
                mix_rate: float(o, "mix_rate", d.ssao.mix_rate),
            });
        Self {
            light,
            projection_density: projection.map_or(d.projection_density, |o| {
                float(o, "density", d.projection_density)
            }),
            projector: projection
                .and_then(|o| text(o, "proj_name"))
                .map_or(d.projector, str::to_owned),
            projection_bias_scale: projection.map_or(d.projection_bias_scale, |o| {
                vec2(o, "bias_scale", d.projection_bias_scale)
            }),
            projection_bias_rotate: projection.map_or(d.projection_bias_rotate, |o| {
                float(o, "bias_rotate", d.projection_bias_rotate)
            }),
            projection_repeat: projection.map_or(d.projection_repeat, |o| {
                flag(o, "repeat", d.projection_repeat)
            }),
            ssao,
        }
    }
}

fn float(object: &ParameterObject, name: &str, fallback: f32) -> f32 {
    object
        .get(name)
        .and_then(|p| p.as_f32().ok())
        .unwrap_or(fallback)
}

fn flag(object: &ParameterObject, name: &str, fallback: bool) -> bool {
    object
        .get(name)
        .and_then(|p| p.as_bool().ok())
        .unwrap_or(fallback)
}

fn text<'a>(object: &'a ParameterObject, name: &str) -> Option<&'a str> {
    object.get(name)?.as_str().ok()
}

fn rgba(object: &ParameterObject, name: &str, fallback: [f32; 4]) -> [f32; 4] {
    match object.get(name) {
        Some(Parameter::Color(c)) => [c.r, c.g, c.b, c.a],
        _ => fallback,
    }
}

fn rgb(object: &ParameterObject, name: &str, fallback: [f32; 3]) -> [f32; 3] {
    let [r, g, b, _] = rgba(object, name, [fallback[0], fallback[1], fallback[2], 1.0]);
    [r, g, b]
}

fn vec2(object: &ParameterObject, name: &str, fallback: [f32; 2]) -> [f32; 2] {
    match object.get(name) {
        Some(Parameter::Vec2(v)) => [v.x, v.y],
        _ => fallback,
    }
}

fn vec3(object: &ParameterObject, name: &str, fallback: [f32; 3]) -> [f32; 3] {
    match object.get(name) {
        Some(Parameter::Vec3(v)) => [v.x, v.y, v.z],
        _ => fallback,
    }
}

fn curves(object: &ParameterObject, name: &str) -> Vec<Curve> {
    match object.get(name) {
        Some(Parameter::Curve1(c)) => c.iter().map(Curve::from_roead).collect(),
        Some(Parameter::Curve2(c)) => c.iter().map(Curve::from_roead).collect(),
        Some(Parameter::Curve3(c)) => c.iter().map(Curve::from_roead).collect(),
        Some(Parameter::Curve4(c)) => c.iter().map(Curve::from_roead).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use roead::aamp::ParameterList;
    use roead::types::{Color, Vector3f};

    use super::*;

    fn color(r: f32, g: f32, b: f32, a: f32) -> Parameter {
        Parameter::Color(Color { r, g, b, a })
    }

    fn string(s: &str) -> Parameter {
        Parameter::StringRef(s.into())
    }

    fn curve(kind: u32, values: &[f32]) -> roead::types::Curve {
        let mut floats = [0.0; 30];
        floats[..values.len()].copy_from_slice(values);
        roead::types::Curve {
            a: values.len() as u32,
            b: kind,
            floats,
        }
    }

    fn document(kind: &str, root: ParameterList) -> Vec<u8> {
        ParameterIO {
            version: 0,
            data_type: kind.into(),
            param_root: root,
        }
        .to_binary()
    }

    fn lights() -> Vec<u8> {
        let mut root = ParameterList::default();
        let mut directional = ParameterList::default();
        directional.objects.insert(
            "DirectionalLight0",
            ParameterObject::new()
                .with_parameter("name", string("dir_main"))
                .with_parameter("DiffuseColor", color(1.0, 0.9, 0.7, 0.0))
                .with_parameter("Intensity", Parameter::F32(8.0))
                .with_parameter(
                    "Direction",
                    Parameter::Vec3(Vector3f {
                        x: 0.6,
                        y: -0.8,
                        z: 0.0,
                    }),
                ),
        );
        let mut hemisphere = ParameterList::default();
        // The game's key is an unnamed hash; lights are found by `name`.
        hemisphere.objects.insert(
            "HemisphereLight9",
            ParameterObject::new()
                .with_parameter("name", string("hemi_inner"))
                .with_parameter("SkyColor", color(0.8, 0.8, 0.9, 1.0))
                .with_parameter("GroundColor", color(0.4, 0.3, 0.2, 1.0)),
        );
        let mut fogs = ParameterList::default();
        let fog = |name: &str, end: f32, enable: bool| {
            ParameterObject::new()
                .with_parameter("enable", Parameter::Bool(enable))
                .with_parameter("name", string(name))
                .with_parameter("Start", Parameter::F32(-5.0))
                .with_parameter("End", Parameter::F32(end))
                .with_parameter("Color", color(0.5, 1.0, 0.8, 0.1))
        };
        fogs.objects.insert("Fog0", fog("fog_scatter", 300.0, true));
        fogs.objects.insert("Fog1", fog("fog_off", 10.0, false));
        root.lists.insert("DirectionalLight", directional);
        root.lists.insert("HemisphereLight", hemisphere);
        root.lists.insert("Fog", fogs);
        let mut projectors = ParameterList::default();
        projectors.objects.insert(
            "Projector0",
            ParameterObject::new()
                .with_parameter("enable", Parameter::Bool(true))
                .with_parameter("name", string("shadowTex_Projector"))
                .with_parameter("proj_type", Parameter::I32(0))
                .with_parameter(
                    "view_pos",
                    Parameter::Vec3(Vector3f {
                        x: 0.0,
                        y: 8000.0,
                        z: 0.0,
                    }),
                )
                .with_parameter("fovy", Parameter::F32(5.0)),
        );
        root.lists.insert("Projector", projectors);
        document("aglenv", root)
    }

    fn index() -> Vec<u8> {
        let refer = |file: &str, name: &str, signature: &str| {
            ParameterObject::new()
                .with_parameter("file", string(file))
                .with_parameter("name", string(name))
                .with_parameter("signature", string(signature))
        };
        let set = |name: &str, texture: &str| {
            let mut set = ParameterList::default();
            set.objects.insert(
                "param",
                ParameterObject::new().with_parameter("name", string(name)),
            );
            let mut references = ParameterList::default();
            references
                .objects
                .insert("0", refer("collect.genvres", "indwp_ia4_00", "doftex"));
            references
                .objects
                .insert("1", refer("collect.genvres", texture, "prjshd"));
            set.lists.insert("refer", references);
            set
        };
        let mut sets = ParameterList::default();
        sets.lists.insert("0", set("Master_Cdungeon", "p_other"));
        sets.lists
            .insert("1", set("Master_Field", "p_shadow_clouds"));
        let mut root = ParameterList::default();
        root.lists.insert("set_array", sets);
        document("genv", root)
    }

    fn light_maps() -> Vec<u8> {
        let mut root = ParameterList::default();
        root.objects.insert(
            "lut_param",
            ParameterObject::new()
                .with_parameter("name0", string("Lambert"))
                .with_parameter(
                    "intensity0",
                    Parameter::Curve1(Box::new([curve(7, &[0.0; 9])])),
                )
                .with_parameter("name1", string("toon_1114"))
                .with_parameter(
                    "intensity1",
                    Parameter::Curve1(Box::new([curve(
                        7,
                        &[0.0, 0.0, 0.0, 0.55, 0.0, 0.0, 0.55, 1.0, 0.0],
                    )])),
                ),
        );
        let mut map = ParameterList::default();
        map.objects.insert(
            "setting",
            ParameterObject::new()
                .with_parameter("name", string("Effect_Actor"))
                .with_parameter("rim_enable", Parameter::Bool(true))
                .with_parameter("rim_width", Parameter::F32(1.5))
                .with_parameter("rim_pow", Parameter::F32(3.0)),
        );
        let mut refs = ParameterList::default();
        refs.objects.insert(
            "0",
            ParameterObject::new().with_parameter("type", string("AmbientLight")),
        );
        refs.objects.insert(
            "1",
            ParameterObject::new()
                .with_parameter("type", string("DirectionalLight"))
                .with_parameter("name", string("dir_main"))
                .with_parameter("calc_type", Parameter::I32(1))
                .with_parameter("lut_name", string("toon_1114"))
                .with_parameter("pow", Parameter::F32(0.82))
                .with_parameter("enable_mip0", Parameter::Bool(true)),
        );
        map.lists.insert("env_obj_ref_array", refs);
        root.lists.insert("0", map);
        document("agllmap", root)
    }

    fn synthetic_set() -> Vec<u8> {
        let mut root = ParameterList::default();
        root.objects.insert(
            "color_correction",
            ParameterObject::new()
                .with_parameter("saturation", Parameter::F32(1.175))
                .with_parameter(
                    "level",
                    Parameter::Curve4(Box::new(
                        [0, 1, 2, 3].map(|_| curve(7, &[0.0, 0.0, 1.0, 1.0, 1.0, 1.0])),
                    )),
                ),
        );
        let correction = document("aglccr", root);
        let mut root = ParameterList::default();
        root.objects.insert(
            "sky",
            ParameterObject::new()
                .with_parameter("scatter_fog_far", Parameter::F32(20_000.0))
                .with_parameter("adhoc_fog_atten_grd", Parameter::F32(4.0))
                .with_parameter("adhoc_fog_color", color(0.5, 0.4, 0.3, 0.0))
                .with_parameter("ground_color", color(0.25, 0.2, 0.15, 1.0)),
        );
        let sky = document("ksky", root);
        let mut root = ParameterList::default();
        root.objects.insert(
            "bloom",
            ParameterObject::new()
                .with_parameter("threshhold", Parameter::F32(2.5))
                .with_parameter("enable_clamped_luminance", Parameter::Bool(false)),
        );
        let bloom = document("aglblm", root);
        let mut root = ParameterList::default();
        root.objects.insert(
            "ssao_parameter",
            ParameterObject::new().with_parameter("radius", Parameter::F32(0.1)),
        );
        root.objects.insert(
            "projection_shadow_0",
            ParameterObject::new()
                .with_parameter("proj_name", string("shadowTex_Projector"))
                .with_parameter("repeat", Parameter::Bool(true))
                .with_parameter(
                    "bias_scale",
                    Parameter::Vec2(roead::types::Vector2f { x: 2.0, y: 0.5 }),
                ),
        );
        let shadows = document("gsdw", root);
        let mut root = ParameterList::default();
        root.objects.insert(
            "Cloud",
            ParameterObject::new().with_parameter("mCloudColorScale", Parameter::F32(2.75)),
        );
        root.objects.insert(
            "CloudParam2",
            ParameterObject::new()
                .with_parameter("mSkyScale", Parameter::F32(26_500.0))
                .with_parameter("mScatterAmb", Parameter::F32(0.2))
                .with_parameter("mFarAlphaChgEnd", Parameter::F32(0.9))
                .with_parameter("mNoiseScale2", Parameter::F32(8.0))
                .with_parameter("mNoiseSpeed2Y", Parameter::F32(0.29))
                .with_parameter("mBaseTexScrollSpdY", Parameter::F32(0.000178))
                .with_parameter("mFarDistotionChgPower", Parameter::F32(14.0)),
        );
        let clouds = document("aglclwd", root);

        let mut writer = roead::sarc::SarcWriter::new(roead::Endian::Big);
        writer.add_file(LIGHTS, lights());
        writer.add_file(COLOR_CORRECTION, correction);
        writer.add_file(SKY, sky);
        writer.add_file(BLOOM, bloom);
        writer.add_file(LIGHT_MAPS, light_maps());
        writer.add_file(SHADOWS, shadows);
        writer.add_file(CLOUDS, clouds);
        writer.add_file(SET_INDEX, index());
        writer.add_file("envobj/other.baglenv", b"not read".to_vec());
        crate::yaz0::compress_stored(&writer.to_binary())
    }

    #[test]
    fn reads_lights_and_fogs() {
        let set = EnvSet::parse(&synthetic_set()).unwrap();
        assert_eq!(set.read.len(), 8);
        let o = &set.objects;
        assert_eq!(o.main_light.diffuse, [1.0, 0.9, 0.7]);
        assert_eq!(
            (o.main_light.intensity, o.main_light.direction),
            (8.0, [0.6, -0.8, 0.0])
        );
        assert_eq!(
            (o.hemisphere.sky, o.hemisphere.ground),
            ([0.8, 0.8, 0.9], [0.4, 0.3, 0.2])
        );
        assert_eq!(o.fogs.len(), 1);
        let fog = o.fog("fog_scatter").unwrap();
        assert_eq!(
            (fog.start, fog.end, fog.color),
            (-5.0, 300.0, [0.5, 1.0, 0.8, 0.1])
        );
    }

    #[test]
    fn reads_the_clouds_as_drawn() {
        let set = EnvSet::parse(&synthetic_set()).unwrap();
        let clouds = &set.clouds;
        assert_eq!(clouds.color_scale, 2.75);
        // CloudParam0 is missing: the constructor's values.
        assert_eq!(clouds.layers[0], CloudDome::fallback().layers[0]);
        let lower = &clouds.layers[1];
        assert_eq!((lower.sky_scale, lower.scatter_ambient), (26_500.0, 0.2));
        assert_eq!(lower.scatter_height, 3.0);
        // Each of Start, End and Power on its own.
        assert_eq!(lower.far_alpha, [0.8, 0.9, -0.4]);
        assert_eq!((lower.far_uv_mul, lower.far_density[2]), (0.8, -0.2));
        assert_eq!(lower.noise_scale, [4.0, 8.0]);
        assert_eq!(lower.noise_speed, [-1.0, -0.6, -1.2, 0.29]);
        assert_eq!(lower.base_scroll_speed, [0.0001, 0.000178]);
        assert_eq!(lower.far_distortion, [0.4, 0.9, 14.0]);
    }

    #[test]
    fn picks_cloud_textures_like_the_renderer() {
        // The field's five textures: numbers 0–3 pick themselves, 4 (the
        // last) and anything past it or negative the first.
        let picks: Vec<usize> = [0, 1, 2, 3, 4, 7, -1]
            .map(|n| EnvSet::cloud_texture_index(n, 5))
            .to_vec();
        assert_eq!(picks, [0, 1, 2, 3, 0, 0, 0]);
        assert_eq!(EnvSet::cloud_texture_index(2, 0), 0);
        // A set without the texture resource lists none.
        assert!(EnvSet::cloud_textures(&synthetic_set()).unwrap().is_empty());
    }

    #[test]
    fn reads_post_effects() {
        let set = EnvSet::parse(&synthetic_set()).unwrap();
        assert_eq!(set.color.saturation, 1.175);
        assert_eq!(set.color.levels.len(), 4);
        assert_eq!(set.color.levels[2].eval(0.3), 0.3);
        // Missing values keep the fallback.
        assert_eq!(set.color.gamma, 1.0);
        assert_eq!(
            (set.bloom.threshold, set.bloom.clamped_luminance),
            (2.5, None)
        );
        assert_eq!(
            (
                set.sky.scatter_fog_far,
                set.sky.adhoc_fog_attenuation_ground
            ),
            (20_000.0, 4.0)
        );
        assert_eq!(set.sky.adhoc_fog_color, [0.5, 0.4, 0.3]);
        // The ground colour keeps its alpha (the sky's `cGroundColor.a`).
        assert_eq!(set.sky.ground_color, [0.25, 0.2, 0.15, 1.0]);
        assert_eq!(set.shadows.ssao.radius, 0.1);
        assert!(set.shadows.ssao.enable);
    }

    #[test]
    fn reads_the_projected_cloud_shadow() {
        let set = EnvSet::parse(&synthetic_set()).unwrap();
        let shadows = &set.shadows;
        assert_eq!(shadows.projector, "shadowTex_Projector");
        assert!(shadows.projection_repeat);
        assert_eq!(shadows.projection_bias_scale, [2.0, 0.5]);
        let projector = set.objects.projector(&shadows.projector).unwrap();
        assert_eq!(
            (projector.view_pos, projector.fovy),
            ([0.0, 8000.0, 0.0], 5.0)
        );
        // Missing values: the constructor's.
        assert_eq!(
            (projector.view_up, projector.far),
            ([0.0, 0.0, 1.0], 10_000.0)
        );
        // The field set's texture, not another set's.
        assert_eq!(
            set.cloud_shadow_texture,
            Some(TextureRef {
                file: "collect.genvres".into(),
                name: "p_shadow_clouds".into(),
            })
        );
    }

    #[test]
    fn reads_light_maps() {
        let set = EnvSet::parse(&synthetic_set()).unwrap();
        let maps = &set.light_maps;
        assert_eq!(maps.curves.len(), 2);
        let actor = maps.map(ACTOR_MAP).unwrap();
        assert!(actor.rim.enable);
        assert_eq!(
            (actor.rim.width, actor.rim.power, actor.rim.angle),
            (1.5, 3.0, 1.0)
        );
        assert_eq!(actor.inputs.len(), 2);
        let main = actor.main_light().unwrap();
        assert_eq!(
            (main.lut.as_str(), main.calc_type, main.power),
            (TOON, 1, 0.82)
        );
        // The toon curve is a hard step at 0.55.
        let toon = maps.curve(TOON).unwrap();
        assert_eq!(
            (
                toon.eval(0.3),
                toon.eval(0.549),
                toon.eval(0.55),
                toon.eval(0.9)
            ),
            (0.0, 0.0, 1.0, 1.0)
        );
        assert_eq!(maps.curve("Lambert").unwrap().eval(0.7), 0.0);
    }

    #[test]
    fn evaluates_curves_like_sead() {
        let linear = Curve {
            kind: CurveKind::Linear2D,
            values: vec![0.0, 0.0, 0.5, 1.0, 1.0, 0.0],
        };
        assert_eq!(
            (
                linear.eval(-1.0),
                linear.eval(0.25),
                linear.eval(0.75),
                linear.eval(2.0)
            ),
            (0.0, 0.5, 0.5, 0.0)
        );
        let step = Curve {
            kind: CurveKind::Step2D,
            values: vec![0.0, 0.2, 0.5, 0.8],
        };
        assert_eq!((step.eval(0.4), step.eval(0.6)), (0.2, 0.8));
        // Hermite with zero slopes: smoothstep between the points.
        let hermite = Curve {
            kind: CurveKind::Hermit2D,
            values: vec![0.0, 0.0, 0.0, 1.0, 1.0, 0.0],
        };
        assert!((hermite.eval(0.5) - 0.5).abs() < 1e-6);
        assert!((hermite.eval(0.25) - 0.15625).abs() < 1e-6);
        assert_eq!(hermite.bake(3), vec![0.0, 0.5, 1.0]);
        assert_eq!(
            Curve {
                kind: CurveKind::Other(0),
                values: vec![1.0]
            }
            .eval(0.3),
            0.3
        );
    }

    #[test]
    fn missing_documents_keep_the_fallback() {
        let mut writer = roead::sarc::SarcWriter::new(roead::Endian::Little);
        writer.add_file(LIGHTS, lights());
        let set = EnvSet::parse(&writer.to_binary()).unwrap();
        assert_eq!(set.read, vec![LIGHTS]);
        assert_eq!(set.color, EnvSet::fallback().color);
        assert!(set.light_maps.curve(TOON_SKIN).is_some());
        assert!(EnvSet::parse(b"not a sarc").is_err());
    }
}
