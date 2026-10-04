// SI-FMT-13: particle emitter field meanings from a third-party reader.
//! Emitters (`EMTR`): how particles are spawned, moved and drawn.
//!
//! An emitter's data is aligned to 0x100 bytes in the file (the header's
//! data offset varies to get there) and is 0xA88 bytes long in version 20.
//! It follows the effect library's `ResEmitter`; field names and meanings
//! follow KillzXGaming's EffectLibrary reader for the Switch successor
//! (VFXB), whose field order matches, with offsets checked against the
//! dump (archived FORMATS.md notes). Child emitters are the node's children;
//! attribute nodes (`CSDP`, `FCSF`, `CADP`, `EAA0`…) hang off the
//! attribute link and are not decoded.
//!
//! Times are in frames, lengths in world units (≈ metres).

use super::{Node, Reader};
use crate::{FormatError, Result};

/// Bytes of emitter data in version 20.
pub const DATA_SIZE: usize = 0xA88;

/// One emitter of an emitter set.
#[derive(Clone, Debug)]
pub struct Emitter {
    pub name: String,
    /// Where the emitter's data starts in the file, for digging further.
    pub data_offset: usize,
    /// Magic of each attribute node, e.g. `CSDP`.
    pub attributes: Vec<String>,
    /// Emitters spawned by this emitter's particles.
    pub children: Vec<Emitter>,

    pub info: Info,
    pub emission: Emission,
    pub shape: Shape,
    pub render: RenderState,
    pub particle: Particle,
    pub velocity: Velocity,
    /// Colour 0 and 1 (RGB) and alpha 0 and 1 over a particle's life. The
    /// shader combines them with the textures (see `combiner`).
    pub color0: Track,
    pub alpha0: Track,
    pub color1: Track,
    pub alpha1: Track,
    /// Multiplier on the colours (HDR brightness).
    pub color_scale: f32,
    /// Particle size in units and its random variation per axis. The
    /// variation looks like a percentage, not units: in `GameResident` it
    /// is a round number (10, 30, 50, 200…) and exceeds the size itself in
    /// half of the emitters.
    pub scale: [f32; 3],
    pub scale_random: [f32; 3],
    /// Scale multiplier over a particle's life (XYZ).
    pub scale_keys: Vec<Key>,
    /// A fourth animated value (shader parameter) over a particle's life.
    pub param_keys: Vec<Key>,
    pub rotation: Rotation,
    /// Alpha fades by camera distance: fully transparent nearer than
    /// `near_alpha.0`, opaque from `near_alpha.1`, and fading out again
    /// between `far_alpha.0` and `far_alpha.1`.
    pub near_alpha: (f32, f32),
    pub far_alpha: (f32, f32),
    /// Soft-particle fade against the depth buffer (distance, volume).
    pub soft_particle: (f32, f32),
    pub gravity: [f32; 3],
    pub air_resistance: f32,
    /// Up to three textures; `None` when the slot is unused.
    pub samplers: [Option<Sampler>; 3],
    /// UV animation of each sampler slot.
    pub texture_anims: [TextureAnim; 3],
    pub combiner: Combiner,
}

/// Where the emitter sits and how it is updated (`EmitterInfo`).
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// 0 none, 1 by distance, 2 by distance reversed, 3 by index (per the
    /// library; unverified).
    pub sort_type: u8,
    /// 0 CPU, 1 GPU, 2 GPU with stream-out.
    pub calc_type: u8,
    /// How particles follow the emitter: 0 fully, 1 not at all, 2 position only.
    pub follow_type: u8,
    /// Frames the emitter fades out / in over.
    pub alpha_fade_time: i32,
    pub fade_in_time: i32,
    /// Emitter transform relative to the set, with random ranges.
    pub translate: [f32; 3],
    pub translate_random: [f32; 3],
    pub rotate: [f32; 3],
    pub rotate_random: [f32; 3],
    pub scale: [f32; 3],
    /// Colours the particle colours are multiplied by.
    pub color0: [f32; 4],
    pub color1: [f32; 4],
    /// Emission thins out between these camera distances, down to
    /// `emission_ratio_far` (an integer in the files: 1–100, perhaps a
    /// percentage; unverified).
    pub emission_range: (f32, f32),
    pub emission_ratio_far: i32,
}

/// When and how many particles are emitted.
#[derive(Clone, Debug, PartialEq)]
pub struct Emission {
    /// Emit for `duration` frames only instead of forever.
    pub one_time: bool,
    pub world_gravity: bool,
    pub start: u32,
    pub duration: u32,
    /// Particles per emission, plus up to `rate_random` more.
    pub rate: f32,
    pub rate_random: i32,
    /// Frames between emissions (0 = every frame).
    pub interval: i32,
    pub interval_random: i32,
    pub position_random: f32,
}

/// The volume particles are born in.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// 0 point, 1 circle, 2 circle (even division), 3 filled circle, 4
    /// sphere, 5–6 sphere (even division), 7 filled sphere, 8 cylinder, 9
    /// filled cylinder, 10 box, 11 filled box, 12–13 line, 14 rectangle,
    /// 15 primitive (per the library; 0 and 3 are confirmed by the clouds).
    pub volume_type: u8,
    /// Arc swept around Y and from the pole (radians).
    pub sweep_longitude: f32,
    pub sweep_latitude: f32,
    pub sweep_start: f32,
    /// Inner radius as a fraction of the outer one (hollow shapes).
    pub caliber_ratio: f32,
    pub radius: [f32; 3],
    pub form_scale: [f32; 3],
}

/// Blending and depth (`EmitterRenderState`).
#[derive(Clone, Debug, PartialEq)]
pub struct RenderState {
    pub blend: bool,
    pub depth_test: bool,
    pub depth_write: bool,
    pub alpha_test: bool,
    /// 0 normal (alpha), 1 additive, 2 subtractive, 3 screen, 4 multiply.
    pub blend_type: u8,
    /// Faces drawn: 0 both, 1 front, 2 back.
    pub display_side: u8,
    pub alpha_threshold: f32,
}

/// What each particle is.
#[derive(Clone, Debug, PartialEq)]
pub struct Particle {
    pub infinite_life: bool,
    /// Orientation: 0 billboard, 1 complex billboard, 2 Y-axis billboard,
    /// 3 polygon in XY, 4 polygon in XZ, 5 velocity-aligned, 6
    /// velocity-aligned polygon (per the library; 2 is confirmed by the
    /// mountain clouds, which stand upright).
    pub billboard: u8,
    /// Which axes rotate: 0 none, 1 X, 2 Y, 3 Z, 4 YZX, 5 XYZ, 6 ZXY.
    pub rotation_type: u8,
    /// Frames a particle lives, with a random reduction that looks like a
    /// percentage (never above 100 in `GameResident`, often above `life`).
    pub life: i32,
    pub life_random: i32,
    /// Mesh drawn instead of a quad, by `Primitive::id`.
    pub primitive: Option<u64>,
}

/// Initial velocity.
#[derive(Clone, Debug, PartialEq)]
pub struct Velocity {
    /// Speed outwards from the emitter centre.
    pub all_direction: f32,
    /// Speed along `direction`.
    pub directional: f32,
    pub direction: [f32; 3],
    /// Spread of `direction` (radians) and per-axis diffusion.
    pub diffusion_angle: f32,
    pub diffusion: [f32; 3],
    pub random: f32,
}

/// Particle rotation (radians, per frame for `add`).
#[derive(Clone, Debug, PartialEq)]
pub struct Rotation {
    pub initial: [f32; 3],
    pub initial_random: [f32; 3],
    pub add: [f32; 3],
    pub add_random: [f32; 3],
    /// Damping of the rotation speed.
    pub resistance: f32,
}

/// A texture slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Sampler {
    /// `Texture::id`, in this file or (when `resident`) in `GameResident`.
    pub texture: u32,
    /// Wrapping per axis: 0 mirror, 1 repeat, 2 clamp (per the library).
    pub wrap: [u8; 2],
    pub filter: u8,
    pub max_lod: f32,
    pub lod_bias: f32,
    /// The texture lives in another file (the resident one).
    pub resident: bool,
}

/// UV animation of one texture slot: pattern (flipbook) and scroll,
/// scale and rotation.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureAnim {
    /// 0 none, other values select how the pattern table plays.
    pub pattern_type: u8,
    pub scroll: bool,
    pub rotate: bool,
    pub scale: bool,
    /// Flipbook: number of cells used, frames per cell, cells to pick
    /// from at random, table of cells.
    pub pattern_count: f32,
    pub pattern_frequency: f32,
    pub pattern_random: f32,
    pub pattern_table: Vec<i32>,
    pub scroll_add: [f32; 2],
    pub scroll_initial: [f32; 2],
    pub scroll_random: [f32; 2],
    pub scale_add: [f32; 2],
    pub scale_initial: [f32; 2],
    pub scale_random: [f32; 2],
    pub rotate_add: f32,
    pub rotate_initial: f32,
    pub rotate_random: f32,
    /// Flipbook grid: cells across and down.
    pub uv_divisions: [f32; 2],
}

/// How the fixed-function combiner mixes colours, alphas and textures;
/// the raw codes of `EmitterCombiner`.
#[derive(Clone, Debug, PartialEq)]
pub struct Combiner {
    pub color_process: u8,
    pub alpha_process: u8,
    pub texture1_color_blend: u8,
    pub texture2_color_blend: u8,
    pub primitive_color_blend: u8,
    pub texture1_alpha_blend: u8,
    pub texture2_alpha_blend: u8,
    pub primitive_alpha_blend: u8,
    pub shader_type: u8,
}

/// A key of an 8-key animation: a value (RGB, XYZ, or an alpha repeated
/// three times) at a point of the particle's life (0–1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Key {
    pub value: [f32; 3],
    pub time: f32,
}

/// A colour or alpha over a particle's life.
#[derive(Clone, Debug, PartialEq)]
pub enum Track {
    Constant([f32; 3]),
    /// One of the keys' values, picked at random per particle.
    Random(Vec<Key>),
    Animated(Vec<Key>),
}

impl Track {
    /// The value at `t` (0–1 of the particle's life); for `Random`, key 0.
    pub fn at(&self, t: f32) -> [f32; 3] {
        match self {
            Track::Constant(v) => *v,
            // SI-FMT-13: particle emitter field meanings from a third-party reader.
            Track::Random(keys) => keys.first().map_or([1.0; 3], |k| k.value),
            Track::Animated(keys) => sample_keys(keys, t),
        }
    }
}

/// Linear interpolation between keys, holding the ends.
pub fn sample_keys(keys: &[Key], t: f32) -> [f32; 3] {
    let Some(first) = keys.first() else {
        return [1.0; 3];
    };
    if t <= first.time {
        return first.value;
    }
    for pair in keys.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.time {
            let f = if b.time > a.time {
                (t - a.time) / (b.time - a.time)
            } else {
                1.0
            };
            return [0, 1, 2].map(|i| a.value[i] + (b.value[i] - a.value[i]) * f);
        }
    }
    keys.last().unwrap().value
}

impl Emitter {
    pub(super) fn read(r: &Reader, node: &Node) -> Result<Self> {
        let d = node.data;
        r.slice(d, DATA_SIZE)
            .map_err(|_| FormatError::Invalid("ptcl: emitter data too short"))?;
        let attributes = Node::siblings(r, node.attribute)?
            .iter()
            .map(Node::magic_str)
            .collect();
        let children = node
            .children(r)?
            .iter()
            .map(|n| Emitter::read(r, n))
            .collect::<Result<_>>()?;

        let b = |o: usize| r.u8(d + o);
        let flag = |o: usize| r.u8(d + o).map(|v| v != 0);
        let f = |o: usize| r.f32(d + o);
        let v2 = |o: usize| r.f32s::<2>(d + o);
        let v3 = |o: usize| r.f32s::<3>(d + o);
        let i = |o: usize| r.i32(d + o);
        // Key counts, then tables of 8 keys × (XYZ, time).
        let key_count = |n: usize| r.u32(d + 0x60 + 4 * n).map(|c| c.min(8) as usize);
        let keys = |n: usize, table: usize| -> Result<Vec<Key>> {
            (0..key_count(n)?)
                .map(|k| {
                    let [x, y, z, time] = r.f32s::<4>(d + table + 16 * k)?;
                    Ok(Key {
                        value: [x, y, z],
                        time,
                    })
                })
                .collect()
        };
        // Colour sources, then constant RGB+A for colour 0 and 1.
        let track =
            |kind: usize, count: usize, table: usize, constant: [f32; 3]| -> Result<Track> {
                Ok(match b(0x9A4 + kind)? {
                    0 => Track::Constant(constant),
                    1 => Track::Random(keys(count, table)?),
                    _ => Track::Animated(keys(count, table)?),
                })
            };
        let [c0r, c0g, c0b, a0, c1r, c1g, c1b, a1] = r.f32s::<8>(d + 0x9A8)?;

        let samplers = [0, 1, 2].map(|s| {
            let at = 0x9F8 + 0x20 * s;
            let id = r.u64(d + at).ok()?;
            (id != u64::MAX).then(|| Sampler {
                texture: id as u32,
                wrap: [b(at + 8).unwrap_or(0), b(at + 9).unwrap_or(0)],
                filter: b(at + 10).unwrap_or(0),
                max_lod: f(at + 12).unwrap_or(0.0),
                lod_bias: f(at + 16).unwrap_or(0.0),
                resident: flag(at + 0x18).unwrap_or(false),
            })
        });
        let texture_anims = [0, 1, 2].map(|s| {
            let (flags, pattern, scroll) = (0xA58 + 0x10 * s, 0x110 + 0x90 * s, 0x2C0 + 0x50 * s);
            TextureAnim {
                pattern_type: b(flags).unwrap_or(0),
                scroll: flag(flags + 1).unwrap_or(false),
                rotate: flag(flags + 2).unwrap_or(false),
                scale: flag(flags + 3).unwrap_or(false),
                pattern_count: f(pattern).unwrap_or(0.0),
                pattern_frequency: f(pattern + 4).unwrap_or(0.0),
                pattern_random: f(pattern + 8).unwrap_or(0.0),
                pattern_table: (0..32)
                    .map(|k| i(pattern + 16 + 4 * k).unwrap_or(0))
                    .collect(),
                scroll_add: v2(scroll).unwrap_or_default(),
                scroll_initial: v2(scroll + 8).unwrap_or_default(),
                scroll_random: v2(scroll + 0x10).unwrap_or_default(),
                scale_add: v2(scroll + 0x18).unwrap_or_default(),
                scale_initial: v2(scroll + 0x20).unwrap_or_default(),
                scale_random: v2(scroll + 0x28).unwrap_or_default(),
                rotate_add: f(scroll + 0x30).unwrap_or(0.0),
                rotate_initial: f(scroll + 0x34).unwrap_or(0.0),
                rotate_random: f(scroll + 0x38).unwrap_or(0.0),
                uv_divisions: v2(scroll + 0x48).unwrap_or_default(),
            }
        });
        let primitive = r.u64(d + 0x8C8)?;

        Ok(Emitter {
            name: r.string(d + 0x10, 0x40)?,
            data_offset: d,
            attributes,
            children,
            info: Info {
                sort_type: b(0x751)?,
                calc_type: b(0x752)?,
                follow_type: b(0x753)?,
                alpha_fade_time: i(0x768)?,
                fade_in_time: i(0x76C)?,
                translate: v3(0x770)?,
                translate_random: v3(0x77C)?,
                rotate: v3(0x788)?,
                rotate_random: v3(0x794)?,
                scale: v3(0x7A0)?,
                color0: r.f32s(d + 0x7AC)?,
                color1: r.f32s(d + 0x7BC)?,
                emission_range: (f(0x7CC)?, f(0x7D0)?),
                emission_ratio_far: i(0x7D4)?,
            },
            emission: Emission {
                one_time: flag(0x7F0)?,
                world_gravity: flag(0x7F1)?,
                start: r.u32(d + 0x7F4)?,
                duration: r.u32(d + 0x7FC)?,
                rate: f(0x800)?,
                rate_random: i(0x804)?,
                interval: i(0x808)?,
                interval_random: i(0x80C)?,
                position_random: f(0x810)?,
            },
            shape: Shape {
                volume_type: b(0x838)?,
                sweep_longitude: f(0x840)?,
                sweep_latitude: f(0x844)?,
                sweep_start: f(0x848)?,
                caliber_ratio: f(0x850)?,
                radius: v3(0x85C)?,
                form_scale: v3(0x868)?,
            },
            render: RenderState {
                blend: flag(0x898)?,
                depth_test: flag(0x899)?,
                depth_write: flag(0x89B)?,
                alpha_test: flag(0x89C)?,
                blend_type: b(0x89E)?,
                display_side: b(0x89F)?,
                alpha_threshold: f(0x8A0)?,
            },
            particle: Particle {
                infinite_life: flag(0x8A8)?,
                billboard: b(0x8AA)?,
                rotation_type: b(0x8AB)?,
                life: i(0x8B8)?,
                life_random: i(0x8BC)?,
                primitive: (primitive != u64::MAX).then_some(primitive),
            },
            velocity: Velocity {
                all_direction: f(0x96C)?,
                directional: f(0x970)?,
                direction: v3(0x974)?,
                diffusion_angle: f(0x980)?,
                diffusion: v3(0x988)?,
                random: f(0x994)?,
            },
            color0: track(0, 0, 0x3C0, [c0r, c0g, c0b])?,
            color1: track(1, 2, 0x4C0, [c1r, c1g, c1b])?,
            alpha0: track(2, 1, 0x440, [a0; 3])?,
            alpha1: track(3, 3, 0x540, [a1; 3])?,
            color_scale: f(0x3B0)?,
            scale: v3(0x9C8)?,
            scale_random: v3(0x9D4)?,
            scale_keys: keys(4, 0x600)?,
            param_keys: keys(5, 0x680)?,
            rotation: Rotation {
                initial: v3(0x700)?,
                initial_random: v3(0x710)?,
                add: v3(0x720)?,
                add_random: v3(0x730)?,
                resistance: f(0x72C)?,
            },
            near_alpha: (f(0x5D0)?, f(0x5D4)?),
            far_alpha: (f(0x5D8)?, f(0x5DC)?),
            soft_particle: (f(0x5F4)?, f(0x5F8)?),
            // SI-FMT-13: particle emitter field meanings from a third-party reader.
            gravity: v3(0xB0)?.map(|g| g * f(0xBC).unwrap_or(0.0) + 0.0),
            air_resistance: f(0xC0)?,
            samplers,
            texture_anims,
            combiner: Combiner {
                color_process: b(0x8F8)?,
                alpha_process: b(0x8F9)?,
                texture1_color_blend: b(0x8FA)?,
                texture2_color_blend: b(0x8FB)?,
                primitive_color_blend: b(0x8FC)?,
                texture1_alpha_blend: b(0x8FD)?,
                texture2_alpha_blend: b(0x8FE)?,
                primitive_alpha_blend: b(0x8FF)?,
                shader_type: b(0x900)?,
            },
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn put(data: &mut [u8], at: usize, bytes: &[u8]) {
        data[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn put_f32s(data: &mut [u8], at: usize, values: &[f32]) {
        for (k, v) in values.iter().enumerate() {
            put(data, at + 4 * k, &v.to_be_bytes());
        }
    }

    /// Emitter data shaped like the mountain clouds: a Y billboard on a
    /// primitive, a fixed colour, alpha fading in and out, one resident
    /// texture.
    pub fn sample(name: &str) -> Vec<u8> {
        let mut d = vec![0u8; DATA_SIZE];
        put(&mut d, 0x10, name.as_bytes());
        // Key counts: colour 0, alpha 0, colour 1, alpha 1, scale, param.
        for (k, n) in [0u32, 3, 0, 0, 2, 0].iter().enumerate() {
            put(&mut d, 0x60 + 4 * k, &n.to_be_bytes());
        }
        put_f32s(
            &mut d,
            0x440,
            &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.5, 0.0, 0.0, 0.0, 1.0],
        );
        put_f32s(&mut d, 0x600, &[1.0, 1.0, 1.0, 0.0, 2.0, 1.5, 2.0, 1.0]);
        put_f32s(&mut d, 0x5D0, &[500.0, 600.0, 1400.0, 1600.0]);
        put_f32s(&mut d, 0x770, &[0.0, -50.0, 0.0]);
        put(&mut d, 0x808, &500i32.to_be_bytes());
        put(&mut d, 0x80C, &30i32.to_be_bytes());
        put_f32s(&mut d, 0x800, &[2.0]);
        put(&mut d, 0x838, &[3]);
        put_f32s(&mut d, 0x85C, &[110.0, 110.0, 110.0]);
        put(&mut d, 0x898, &[1, 1, 3, 0, 1, 4, 0, 0]);
        put(&mut d, 0x8AA, &[2]);
        put(&mut d, 0x8B8, &1600i32.to_be_bytes());
        put(&mut d, 0x8C8, &0x9CC7_0E4Fu64.to_be_bytes());
        put(&mut d, 0x9A4, &[0, 0, 2, 0]);
        put_f32s(&mut d, 0x9A8, &[0.3, 0.3, 0.4, 1.0, 0.1, 0.1, 0.1, 0.8]);
        put_f32s(&mut d, 0x9C8, &[200.0, 180.0, 200.0, 55.0, 55.0, 55.0]);
        put(&mut d, 0x9F8, &0x3000_1F20u64.to_be_bytes());
        put(&mut d, 0xA00, &[1, 2]);
        put(&mut d, 0xA10, &[1]);
        put(&mut d, 0xA18, &u64::MAX.to_be_bytes());
        put(&mut d, 0xA38, &u64::MAX.to_be_bytes());
        put(&mut d, 0xA58, &[0, 1, 0, 1]);
        put_f32s(&mut d, 0x2C0, &[0.0, -0.001]);
        put_f32s(&mut d, 0x2E0, &[2.5, 2.0]);
        d
    }

    #[test]
    fn reads_emitter_parameters() {
        let bytes = super::super::tests::sample_file(sample, &[]);
        let ptcl = super::super::Ptcl::parse(&bytes).unwrap();
        let e = &ptcl.emitter_sets[0].emitters[0];
        assert_eq!(e.name, "Parent");
        assert_eq!(e.color0, Track::Constant([0.3, 0.3, 0.4]));
        assert_eq!(e.alpha1, Track::Constant([0.8; 3]));
        assert_eq!(e.alpha0.at(0.25), [0.5; 3]);
        assert_eq!(e.alpha0.at(2.0), [0.0; 3]);
        assert_eq!(sample_keys(&e.scale_keys, 0.5), [1.5, 1.25, 1.5]);
        assert_eq!(
            (e.near_alpha, e.far_alpha),
            ((500.0, 600.0), (1400.0, 1600.0))
        );
        assert_eq!(e.info.translate, [0.0, -50.0, 0.0]);
        assert_eq!(
            (
                e.emission.rate,
                e.emission.interval,
                e.emission.interval_random
            ),
            (2.0, 500, 30)
        );
        assert_eq!((e.shape.volume_type, e.shape.radius), (3, [110.0; 3]));
        assert!(
            e.render.blend && e.render.depth_test && !e.render.depth_write && e.render.alpha_test
        );
        assert_eq!(
            (e.particle.billboard, e.particle.life, e.particle.primitive),
            (2, 1600, Some(0x9CC7_0E4F))
        );
        assert_eq!(
            (e.scale, e.scale_random),
            ([200.0, 180.0, 200.0], [55.0; 3])
        );
        let sampler = e.samplers[0].as_ref().unwrap();
        assert_eq!(
            (sampler.texture, sampler.wrap, sampler.resident),
            (0x3000_1F20, [1, 2], true)
        );
        assert!(e.samplers[1].is_none() && e.samplers[2].is_none());
        let anim = &e.texture_anims[0];
        assert!(anim.scroll && anim.scale && !anim.rotate);
        assert_eq!(
            (anim.scroll_add, anim.scale_initial),
            ([0.0, -0.001], [2.5, 2.0])
        );
    }

    #[test]
    fn short_emitter_data_is_an_error() {
        let bytes = super::super::tests::sample_file(
            |name| super::super::tests::named(0x10, name, 0x60),
            &[],
        );
        assert!(super::super::Ptcl::parse(&bytes).is_err());
    }
}
