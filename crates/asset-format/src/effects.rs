//! Particle effects: the game's effect files (PTCL, `Effect/*.sesetlist`)
//! that something in the baked world plays, and what plays them. Written
//! by `bake` (step `effects`).
//!
//! ```text
//! effects/index.ron              EffectIndex: files, who plays what
//! effects/files/<file>.ron       EffectFile: sets, emitters, primitives
//! effects/files/<file>.res       the emitters' resource blocks, as stored
//! effects/files/<file>/<id>.ktx2 its textures (id as 8 hex digits)
//! ```
//!
//! An emitter is kept twice: decoded into named fields (`Emitter`, the
//! meanings as far as they are known) and as the game's own resource
//! block, byte for byte (`Emitter::resource`, a range of the `.res` file),
//! which the effect library hands to the GPU as the emitter's static
//! uniform block. Attribute blocks (`CSDP`, `FCSF`, …) are kept the same
//! way. Big-endian, as the game stores them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The shared effect file holding resident textures and primitives
/// (`Effect/GameResident.sesetlist` in `Pack/Bootup.pack`).
pub const RESIDENT: &str = "GameResident";

/// `effects/index.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectIndex {
    /// Every file in `effects/files/`.
    pub files: Vec<String>,
    /// Per actor name: the emitter sets its ELink user plays all the time.
    pub actors: BTreeMap<String, Vec<PlayedSet>>,
    /// Actors anywhere on the map that are nothing but an effect (no
    /// model; e.g. `MountainCloud`), placed: kept for the whole map, as
    /// some are seen from far away.
    pub effect_actors: Vec<EffectActor>,
}

/// An emitter set an ELink asset plays, with the asset's parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlayedSet {
    /// The effect file (its name without extension, the ELink user).
    pub file: String,
    pub set: String,
    pub scale: f32,
    pub offset: [f32; 3],
    pub color: [f32; 4],
}

/// A placed actor that is only an effect.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectActor {
    pub name: String,
    pub hash_id: u32,
    pub translate: [f32; 3],
    /// Radians, applied X, Y, Z (as `objects::PlacedActor`).
    pub rotate: [f32; 3],
    pub scale: [f32; 3],
}

/// `effects/files/<file>.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectFile {
    pub name: String,
    pub sets: Vec<EmitterSet>,
    /// Ids of the textures written next to it.
    pub textures: Vec<u32>,
    pub primitives: Vec<Primitive>,
}

impl EffectFile {
    pub fn set(&self, name: &str) -> Option<&EmitterSet> {
        self.sets.iter().find(|s| s.name == name)
    }

    pub fn primitive(&self, id: u64) -> Option<&Primitive> {
        self.primitives.iter().find(|p| p.id == id)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmitterSet {
    pub name: String,
    pub emitters: Vec<Emitter>,
}

/// A byte range of the file's `.res` blob.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub offset: u32,
    pub size: u32,
}

impl Block {
    pub fn get<'a>(&self, res: &'a [u8]) -> Option<&'a [u8]> {
        res.get(self.offset as usize..(self.offset + self.size) as usize)
    }
}

/// An emitter. Meanings and offsets: docs/research/eft-runtime.md.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Emitter {
    pub name: String,
    /// The resource block as stored (0xA88 bytes, big-endian).
    pub resource: Block,
    /// `sysEmitterStaticUniformBlock`: the resource's bytes `0x000..0x750`
    /// after the library's load-time patch, words from 0x50 on
    /// little-endian, ready for the GPU (0x750 bytes).
    pub static_block: Block,
    /// `sysEmitterFieldUniformBlock` (0x120 bytes, little-endian), when
    /// the emitter has fields.
    pub field_block: Option<Block>,
    /// Attribute blocks by magic, in the file's order (big-endian).
    pub attributes: Vec<(String, Block)>,
    /// Emitter animations (`EA**` nodes) by magic.
    pub animations: Vec<(String, EmitterAnim)>,
    /// Emitters spawned by this one's particles.
    pub children: Vec<Emitter>,
    pub params: EmitterParams,
    /// Up to three textures; `None` when the slot is unused.
    pub samplers: [Option<Sampler>; 3],
}

/// What the CPU side of the library reads from an emitter's (patched)
/// resource. Times are in frames (30 a second).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EmitterParams {
    pub visible: bool,
    /// 0 none, 1 newest first, 2 back to front, 3 oldest first.
    pub sort: u8,
    /// 0 CPU, 1 GPU, 2 GPU with stream-out.
    pub calc: u8,
    /// 0 particles follow the emitter, 1 not at all, 2 its position only.
    pub follow: u8,
    pub fade_stops_emission: bool,
    pub fade_out_alpha: bool,
    pub fade_out_scale: bool,
    /// 0 game random, 1 the set's random, 2 fixed (`seed`).
    pub seed_mode: u8,
    pub rerandomize_matrix: bool,
    pub lod_every_frame: bool,
    pub lod_emission: bool,
    pub fade_in_alpha: bool,
    pub fade_in_scale: bool,
    pub seed: u32,
    pub draw_path: u32,
    pub fade_out_frames: i32,
    pub fade_in_frames: i32,
    pub translate: [f32; 3],
    pub translate_random: [f32; 3],
    /// Radians.
    pub rotate: [f32; 3],
    pub rotate_random: [f32; 3],
    pub scale: [f32; 3],
    /// Emitter colours, rgb + alpha.
    pub color0: [f32; 4],
    pub color1: [f32; 4],
    /// Camera distances: hidden (or faded) nearer, hidden farther (−1:
    /// never), and the percentage of emission kept at `far`.
    pub near: f32,
    pub far: f32,
    pub far_percent: i32,
    /// Child inheritance flags `0x7d8..0x7df`: velocity, scale, rotation,
    /// —, colour0, colour1, alpha0, alpha1.
    pub inherit: [bool; 8],
    pub draw_before_parent: bool,
    pub alpha0_with_parent: bool,
    pub alpha1_with_parent: bool,
    pub inherit_velocity_scale: f32,
    pub inherit_scale_scale: f32,
    pub one_time: bool,
    pub world_gravity: bool,
    pub by_distance: bool,
    pub world_direction: bool,
    pub start: i32,
    pub child_start_percent: i32,
    pub duration: i32,
    /// Particles per emission (patched for equal divisions).
    pub rate: f32,
    /// Percentage reduction.
    pub rate_random: i32,
    /// Frames skipped between emissions, and a random number more.
    pub interval: i32,
    pub interval_random: i32,
    pub position_random: f32,
    pub gravity_scale: f32,
    pub gravity: [f32; 3],
    pub distance_unit: f32,
    pub distance_min: f32,
    pub distance_max: f32,
    pub distance_threshold: f32,
    pub volume: u8,
    pub random_start_angle: bool,
    pub latitude_mode: bool,
    pub sphere_table: u8,
    pub sphere64_count: u8,
    pub latitude_axis: u8,
    pub sweep: f32,
    pub latitude: f32,
    pub sweep_start: f32,
    pub division_angle_random: f32,
    pub caliber: f32,
    pub line_center: f32,
    pub line_length: f32,
    pub volume_radius: [f32; 3],
    pub form_scale: [f32; 3],
    /// Equal divisions: 0 every point at once, 1 a random one, 2 the next
    /// one; −1 other volumes.
    pub division_mode: i32,
    pub shape_primitive: Option<u64>,
    pub circle_divisions: i32,
    pub circle_division_random: i32,
    pub line_divisions: i32,
    pub line_division_random: i32,
    pub blend: bool,
    pub depth_test: bool,
    /// GX2 compare functions (3 less-or-equal, 4 greater …).
    pub depth_func: u8,
    pub depth_write: bool,
    pub alpha_test: bool,
    pub alpha_func: u8,
    /// 0 alpha, 1 add, 2 subtract, 3 multiply, 4 screen.
    pub blend_type: u8,
    /// 0 none, 1 cull back, 2 cull front.
    pub cull: u8,
    pub alpha_ref: f32,
    pub infinite_life: bool,
    pub billboard: u8,
    /// Euler order of the particle rotation: 4 Rx·Rz·Ry, 5 Rz·Ry·Rx,
    /// 6 Ry·Rx·Rz.
    pub rotation_order: u8,
    pub life: i32,
    /// Integer percentage reduction.
    pub life_random: i32,
    pub speed_random: f32,
    pub primitive: Option<u64>,
    pub omni_velocity: f32,
    pub directional_velocity: f32,
    pub direction: [f32; 3],
    /// Degrees.
    pub diffusion_angle: f32,
    pub xz_diffusion: f32,
    pub velocity_random_axes: [f32; 3],
    /// Percentage reduction.
    pub velocity_random: f32,
    pub emitter_velocity_inherit: f32,
    pub particle_scale: [f32; 3],
    /// Percentage reductions.
    pub scale_random: [f32; 3],
    pub air_resistance: f32,
    /// Colour0, colour1, alpha0, alpha1: 0 constant, 2 animated, 3 random
    /// key.
    pub color_sources: [u8; 4],
    /// The pixel shader's combiner bytes `0x8f8..0x90a`
    /// (docs/research/eft-shaders.md §8).
    pub combiner: [u8; 18],
    /// Texture slots whose colour the shader squares (sampler byte +0x17).
    pub texture_squared: [bool; 3],
    /// BotW's custom shader (`0x92c`: 0 none, 3 weather/clouds/haze, 4 lit
    /// meshes …) and its switch words (`0x930`, `0x934`).
    pub custom_shader: u32,
    pub custom_switches: [u32; 2],
    /// Emitter plugin 4 (`EP04`, area loop), if any
    /// (docs/research/eft-custom-blocks.md §2).
    #[serde(default)]
    pub area_loop: Option<AreaLoop>,
    /// The `CSDP` node: BotW's custom shader parameters, the
    /// `sysCustomShaderReservedUniformBlockParam` (0x80 bytes, as 32
    /// little-endian words; zero past the node's payload).
    #[serde(default)]
    pub custom_params: Vec<f32>,
    /// SHA-1 of the emitter's vertex and pixel shader programs (`0x914`,
    /// `0x918` into its file's GFX2 lists): which of the game's programs
    /// draws it (docs/research/eft-shaders.md names programs by these).
    #[serde(default)]
    pub vertex_program: String,
    #[serde(default)]
    pub pixel_program: String,
}

/// The area loop plugin (`EP04`): the emitter is drawn `extra_draws + 1`
/// times, copy k shifted by k·`step`, every particle wrapped into a box of
/// half-size `half_size` (centred `centre` in camera space when
/// `follows_camera`, else in the emitter's space).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AreaLoop {
    pub step: [f32; 3],
    pub extra_draws: f32,
    pub half_size: [f32; 3],
    pub cut_height: f32,
    pub centre: [f32; 3],
    /// 0 none, 1 cut above `cut_height`, 2 below.
    pub cut_mode: i32,
    pub fade: [f32; 3],
    pub follows_camera: bool,
    /// Euler rotation of the box (radians).
    pub rotation: [f32; 3],
}

/// An emitter animation over the emitter's frames: linear keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EmitterAnim {
    pub enabled: bool,
    pub looping: bool,
    pub keys: Vec<Key>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    pub value: [f32; 3],
    pub time: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sampler {
    pub texture: u32,
    /// The texture is in the resident file, not the emitter's.
    pub resident: bool,
    /// U, V: 0 mirror, 1 repeat, 2 clamp, 3 mirror once.
    pub wrap: [u8; 2],
    /// 0 linear, else point.
    pub filter: u8,
    pub max_lod: f32,
    pub lod_bias: f32,
}

/// A mesh particles are drawn with. Attributes it lacks are empty.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Primitive {
    pub id: u64,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 4]>,
    pub colors: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// `effects/tables.ron`: constant tables the effect library's CPU code
/// reads, extracted from the game's executable (`U-King.rpx` v208) by
/// `bake` (docs/research/eft-runtime.md).
pub const TABLES: &str = "effects/tables.ron";

/// Tables from the effect library's code and data.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EffectTables {
    /// sead's sine table (`0x103d3794`): per 1/256 turn `sin, Δsin, cos,
    /// Δcos` (`sead::Mathf::sinIdx`/`cosIdx`).
    pub sin_cos: Vec<[f32; 4]>,
    /// Unit vectors of the equally divided sphere (volume 5), by the
    /// emitter's table index (`0x1047eea8`: 2, 3, 4, 6, 8, 12, 20, 32
    /// points).
    pub sphere: Vec<Vec<[f32; 3]>>,
    /// Unit vectors of the equally divided sphere of up to 64 points
    /// (volume 6), at `points − 4` (`0x1047eed0`).
    pub sphere64: Vec<Vec<[f32; 3]>>,
    /// The threshold below which a shortest-arc rotation from +Y is taken
    /// as the half turn about X (`0x103d3368`, copied to `0x1059837c`).
    pub arc_epsilon: f32,
    /// The table the library builds its curl-noise texture from
    /// (`sysCurlNoiseTextureArray`, `0x03b69644`): 32×32×32 texels of
    /// three signed bytes, x fastest, then y, then z (`0x103a6920`,
    /// 0x18000 bytes). Empty in tables baked before it was added.
    #[serde(default)]
    pub curl_noise: Vec<u8>,
}

/// Texels per side of the curl-noise texture.
pub const CURL_NOISE_SIZE: usize = 32;

pub fn file_path(file: &str) -> String {
    format!("{}/{file}.ron", crate::paths::EFFECT_FILES)
}

pub fn resource_path(file: &str) -> String {
    format!("{}/{file}.res", crate::paths::EFFECT_FILES)
}

pub fn texture_path(file: &str, id: u32) -> String {
    format!("{}/{file}/{id:08x}.ktx2", crate::paths::EFFECT_FILES)
}
