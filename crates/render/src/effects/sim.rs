//! The CPU side of the effect library (NintendoWare eft2 in `U-King.rpx`
//! v208) for one emitter set: emitter lifetimes, emission, particle
//! initialisation, the CPU particle update and child emitters, in the
//! order `eft_EmitterSet_Calc` `0x03b5a348` runs them, plus BotW's LOD
//! callback (`botw_EmitterCalcLodCallback` `0x0387b8d0`). What it hands the
//! GPU: per emitter the dynamic uniform block and the live particles'
//! attributes. Every rule: docs/research/eft-runtime.md, by address.
//!
//! Frames are game frames (30 a second); `step` is the frame-rate scale
//! the game passes (1 at 30 fps).

use std::sync::{Arc, OnceLock};

use asset_format::effects::{self as fx, EffectTables, EmitterAnim, EmitterParams, Primitive};
use bevy::math::{Affine3A, Vec3, Vec4};

use super::emit::{self, EmitterRng, Volume};
use super::gpu::{EmitterDynamic, ParticleAttr};
use super::library::EffectData;
use super::random::{SeadRandom, VecTables, angle_index, sin_cos, sin_cos_index};

#[path = "stream_out.rs"]
mod stream_out;
pub use stream_out::StreamOutEnv;

/// The two global vector tables (generated once, as the library does).
fn vectors() -> &'static VecTables {
    static TABLES: OnceLock<VecTables> = OnceLock::new();
    TABLES.get_or_init(VecTables::new)
}

/// Infinite particle life (`2^28` frames).
const INFINITE_LIFE: f32 = 2.684_354_6e8;

// ---------------------------------------------------------------------------
// Matrices: 3×4, row-major, as the library keeps them.

#[derive(Clone, Copy, Debug, PartialEq)]
struct M34([[f32; 4]; 3]);

impl M34 {
    const IDENTITY: Self = Self([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]);
    const ZERO: Self = Self([[0.0; 4]; 3]);

    fn from_affine(a: &Affine3A) -> Self {
        let m = a.matrix3;
        let t = a.translation;
        Self([
            [m.x_axis.x, m.y_axis.x, m.z_axis.x, t.x],
            [m.x_axis.y, m.y_axis.y, m.z_axis.y, t.y],
            [m.x_axis.z, m.y_axis.z, m.z_axis.z, t.z],
        ])
    }

    fn rows(&self) -> [Vec4; 3] {
        self.0.map(Vec4::from_array)
    }

    fn translation(&self) -> Vec3 {
        Vec3::new(self.0[0][3], self.0[1][3], self.0[2][3])
    }

    /// `self · b` (affine).
    fn mul(&self, b: &M34) -> M34 {
        let a = &self.0;
        let b = &b.0;
        let mut r = [[0.0f32; 4]; 3];
        for i in 0..3 {
            for j in 0..4 {
                let mut v = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
                if j == 3 {
                    v += a[i][3];
                }
                r[i][j] = v;
            }
        }
        M34(r)
    }

    fn point(&self, p: Vec3) -> Vec3 {
        let m = &self.0;
        Vec3::new(
            m[0][2] * p.z + m[0][0] * p.x + m[0][1] * p.y + m[0][3],
            m[1][2] * p.z + m[1][0] * p.x + m[1][1] * p.y + m[1][3],
            m[2][2] * p.z + m[2][0] * p.x + m[2][1] * p.y + m[2][3],
        )
    }

    fn vector(&self, v: Vec3) -> Vec3 {
        let m = &self.0;
        Vec3::new(
            v.z * m[0][2] + v.x * m[0][0] + v.y * m[0][1],
            v.z * m[1][2] + v.x * m[1][0] + v.y * m[1][1],
            v.z * m[2][2] + v.x * m[2][0] + v.y * m[2][1],
        )
    }

    /// The transposed 3×3 times `v`.
    fn transposed_vector(&self, v: Vec3) -> Vec3 {
        let m = &self.0;
        Vec3::new(
            v.z * m[2][0] + (v.x * m[0][0] + v.y * m[1][0]),
            v.z * m[2][1] + (v.x * m[0][1] + v.y * m[1][1]),
            v.z * m[2][2] + (v.x * m[0][2] + v.y * m[1][2]),
        )
    }

    /// The affine inverse (sead `Matrix34::setInverse`); a singular matrix
    /// leaves zero.
    fn inverse(&self) -> M34 {
        let m = &self.0;
        let c = |i: usize, j: usize| f64::from(m[i][j]);
        let a00 = c(1, 1) * c(2, 2) - c(1, 2) * c(2, 1);
        let a01 = c(0, 2) * c(2, 1) - c(0, 1) * c(2, 2);
        let a02 = c(0, 1) * c(1, 2) - c(0, 2) * c(1, 1);
        let det = c(0, 0) * a00 + c(1, 0) * a01 + c(2, 0) * a02;
        if det == 0.0 {
            return M34::ZERO;
        }
        let inv = f64::from((1.0 / det) as f32);
        let r00 = a00 * inv;
        let r01 = a01 * inv;
        let r02 = a02 * inv;
        let r10 = (c(1, 2) * c(2, 0) - c(1, 0) * c(2, 2)) * inv;
        let r11 = (c(0, 0) * c(2, 2) - c(0, 2) * c(2, 0)) * inv;
        let r12 = (c(0, 2) * c(1, 0) - c(0, 0) * c(1, 2)) * inv;
        let r20 = (c(1, 0) * c(2, 1) - c(1, 1) * c(2, 0)) * inv;
        let r21 = (c(0, 1) * c(2, 0) - c(0, 0) * c(2, 1)) * inv;
        let r22 = (c(0, 0) * c(1, 1) - c(0, 1) * c(1, 0)) * inv;
        let (tx, ty, tz) = (c(0, 3), c(1, 3), c(2, 3));
        let f = |v: f64| v as f32;
        M34([
            [f(r00), f(r01), f(r02), f(-(r02 * tz + r01 * ty + r00 * tx))],
            [f(r10), f(r11), f(r12), f(-(r12 * tz + r11 * ty + r10 * tx))],
            [f(r20), f(r21), f(r22), f(-(r22 * tz + r21 * ty + r20 * tx))],
        ])
    }

    fn with_translation(mut self, t: Vec3) -> M34 {
        self.0[0][3] = t.x;
        self.0[1][3] = t.y;
        self.0[2][3] = t.z;
        self
    }
}

/// sead `makeSRT` with the table sines (`eft_Emitter_RandomizeLocalMatrix`
/// `0x03b56a60`): `R = Rz·Ry·Rx`, the scale on the columns. Returns the
/// SRT and the RT.
fn make_srt(tables: &EffectTables, s: Vec3, r: Vec3, t: Vec3) -> (M34, M34) {
    let (sx, cx) = sin_cos(tables, r.x);
    let (sy, cy) = sin_cos(tables, r.y);
    let (sz, cz) = sin_cos(tables, r.z);
    let rot = [
        [cy * cz, (sx * sy) * cz - cx * sz, (cx * cz) * sy + sx * sz],
        [cy * sz, (sx * sy) * sz + cx * cz, (cx * sz) * sy - sx * cz],
        [-sy, sx * cy, cx * cy],
    ];
    let scale = [s.x, s.y, s.z];
    let srt = M34(std::array::from_fn(|i| {
        [
            scale[0] * rot[i][0],
            scale[1] * rot[i][1],
            scale[2] * rot[i][2],
            t[i],
        ]
    }));
    let rt = M34(std::array::from_fn(|i| {
        [rot[i][0], rot[i][1], rot[i][2], t[i]]
    }));
    (srt, rt)
}

// ---------------------------------------------------------------------------
// Resource reading.

/// Big-endian reads of the stored resource (fields past the static block,
/// which the load-time patch leaves alone apart from `0x800`/`0x874`).
struct Raw<'a>(&'a [u8]);

impl Raw<'_> {
    fn u8(&self, at: usize) -> u8 {
        self.0.get(at).copied().unwrap_or(0)
    }
    fn flag(&self, at: usize) -> bool {
        self.u8(at) != 0
    }
    fn u32(&self, at: usize) -> u32 {
        self.0
            .get(at..at + 4)
            .map_or(0, |b| u32::from_be_bytes(b.try_into().unwrap()))
    }
    fn i32(&self, at: usize) -> i32 {
        self.u32(at) as i32
    }
    fn f32(&self, at: usize) -> f32 {
        f32::from_bits(self.u32(at))
    }
    fn v3(&self, at: usize) -> Vec3 {
        Vec3::new(self.f32(at), self.f32(at + 4), self.f32(at + 8))
    }
}

/// The patched static block (resource offsets; words from 0x50 on are
/// little-endian), which is also the CPU's private copy (`er+0x14`).
struct Stat<'a>(&'a [u8]);

impl Stat<'_> {
    fn u32(&self, at: usize) -> u32 {
        self.0
            .get(at..at + 4)
            .map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()))
    }
    fn i32(&self, at: usize) -> i32 {
        self.u32(at) as i32
    }
    fn f32(&self, at: usize) -> f32 {
        f32::from_bits(self.u32(at))
    }
    fn v4(&self, at: usize) -> Vec4 {
        Vec4::new(
            self.f32(at),
            self.f32(at + 4),
            self.f32(at + 8),
            self.f32(at + 12),
        )
    }
}

/// Emitter animation slots (`em+0x460 + 0xc·i`, from `er+0x148 + 4·i`).
const ANIMS: [&str; 14] = [
    "EAES", "EAER", "EAET", "EAC0", "EAC1", "EATR", "EAPL", "EAA0", "EAA1", "EAOV", "EADV", "EASL",
    "EASS", "EAGV",
];
const EAES: usize = 0;
const EAER: usize = 1;
const EAET: usize = 2;
const EAC0: usize = 3;
const EAC1: usize = 4;
const EATR: usize = 5;
const EAPL: usize = 6;
const EAA0: usize = 7;
const EAA1: usize = 8;
const EAOV: usize = 9;
const EADV: usize = 10;
const EASL: usize = 11;
const EASS: usize = 12;
const EAGV: usize = 13;

/// An 8-key track of the static block.
#[derive(Clone, Copy, Debug, Default)]
struct Track {
    keys: [Vec4; 8],
    count: i32,
    /// Loop period (0: over the life) and random start (0/1).
    period: f32,
    random_start: f32,
}

/// A field parameter's animation over the particle's life
/// (`eft_FieldAnim_Eval` `0x03b5b8f8`).
#[derive(Clone, Debug, Default)]
struct FieldAnim {
    enabled: bool,
    looping: bool,
    random_start: f32,
    period: f32,
    keys: Vec<Vec4>,
}

impl FieldAnim {
    fn read(data: &[u8], at: usize) -> Self {
        let r = Raw(data);
        let count = r.i32(at + 0xc).max(0) as usize;
        let keys = (0..count)
            .take_while(|k| at + 0x14 + 16 * k + 16 <= data.len())
            .map(|k| {
                let o = at + 0x14 + 16 * k;
                Vec4::new(r.f32(o), r.f32(o + 4), r.f32(o + 8), r.f32(o + 12))
            })
            .collect();
        Self {
            enabled: r.u32(at) != 0,
            looping: r.u32(at + 4) != 0,
            random_start: r.u32(at + 8) as f32,
            period: r.i32(at + 0x10) as f32,
            keys,
        }
    }

    /// `rnd` is the random of particle slot 0 (`*(em+0x158)+0x10`): the
    /// library reads the first particle's, not this one's.
    fn eval(&self, age: f32, life: f32, rnd: f32) -> Vec3 {
        let keys = &self.keys;
        match keys.len() {
            0 => return Vec3::ZERO,
            1 => return keys[0].truncate(),
            _ => {}
        }
        let last = keys[keys.len() - 1];
        let t = if self.looping {
            let x = f64::from((self.random_start * rnd) as f64 as f32) * f64::from(self.period)
                + f64::from(age);
            let x = x as f32 as f64;
            (x % f64::from(self.period) / f64::from(self.period)) as f32
        } else {
            (f64::from(age) / f64::from(life)) as f32
        };
        if last.w <= t {
            return last.truncate();
        }
        if t < keys[0].w {
            return keys[0].truncate();
        }
        for pair in keys.windows(2) {
            let (k0, k1) = (pair[0], pair[1]);
            if k0.w <= t && t < k1.w {
                let inv = 1.0 / (k1.w - k0.w);
                let dt = t - k0.w;
                return Vec3::new(
                    k0.x + ((k1.x - k0.x) * inv) * dt,
                    k0.y + ((k1.y - k0.y) * inv) * dt,
                    dt * (inv * (k1.z - k0.z)) + k0.z,
                );
            }
        }
        Vec3::ZERO
    }

    /// The value: animated when enabled, else the constant.
    fn value(&self, constant: Vec3, age: f32, life: f32, rnd: f32) -> Vec3 {
        if self.enabled {
            self.eval(age, life, rnd)
        } else {
            constant
        }
    }
}

/// The fields the CPU applies (`er+0x124..0x144`).
#[derive(Clone, Debug, Default)]
struct Fields {
    /// FRND: noise; its CPU form is not ported.
    random: bool,
    random1: Option<Random1>,
    magnet: Option<Magnet>,
    spin: Option<Spin>,
    collision: Option<Collision>,
    converge: Option<Converge>,
    /// FCLN: curl noise; its CPU form is not ported.
    curl: bool,
    pos_add: Option<PosAdd>,
}

/// FRN1 (`eft_Field_RandomFe1` `0x03b5bfa8`).
#[derive(Clone, Debug)]
struct Random1 {
    amplitude: Vec3,
    interval: u32,
    anim: FieldAnim,
}

/// FMAG (`eft_Field_Magnet` `0x03b5c144`).
#[derive(Clone, Debug)]
struct Magnet {
    follow_emitter: bool,
    axes: [bool; 3],
    strength: f32,
    target: Vec3,
    anim: FieldAnim,
}

/// FSPN (`eft_Field_Spin` `0x03b5c670`).
#[derive(Clone, Debug)]
struct Spin {
    speed: f32,
    axis: u32,
    outward: f32,
    speed_anim: FieldAnim,
    outward_anim: FieldAnim,
}

/// FCOL (`eft_Field_Collision` `0x03b5cbbc`).
#[derive(Clone, Debug)]
struct Collision {
    kill: u8,
    world: bool,
    y: f32,
    bounce: f32,
    max_count: i32,
    friction: f32,
}

/// FCOV (`eft_Field_Convergence` `0x03b5d150`).
#[derive(Clone, Debug)]
struct Converge {
    follow_emitter: bool,
    target: Vec3,
    ratio: f32,
    anim: FieldAnim,
}

/// FPAD (`eft_Field_PosAdd` `0x03b5d668`).
#[derive(Clone, Debug)]
struct PosAdd {
    world: bool,
    add: Vec3,
    anim: FieldAnim,
}

impl Fields {
    fn read(attributes: &[(String, &[u8])]) -> Self {
        let mut f = Self::default();
        for (magic, data) in attributes {
            let r = Raw(data);
            match magic.as_str() {
                "FRND" => f.random = true,
                "FCLN" => f.curl = true,
                "FRN1" => {
                    f.random1 = Some(Random1 {
                        amplitude: r.v3(0),
                        interval: r.u32(0xc),
                        anim: FieldAnim::read(data, 0x10),
                    })
                }
                "FMAG" => {
                    f.magnet = Some(Magnet {
                        follow_emitter: r.flag(0),
                        axes: [r.flag(1), r.flag(2), r.flag(3)],
                        strength: r.f32(4),
                        target: r.v3(8),
                        anim: FieldAnim::read(data, 0x14),
                    })
                }
                "FSPN" => {
                    f.spin = Some(Spin {
                        speed: r.f32(0),
                        axis: r.u32(4),
                        outward: r.f32(8),
                        speed_anim: FieldAnim::read(data, 0xc),
                        outward_anim: FieldAnim::read(data, 0xa0),
                    })
                }
                "FCOL" => {
                    f.collision = Some(Collision {
                        kill: r.u8(0),
                        world: r.flag(1),
                        y: r.f32(4),
                        bounce: r.f32(8),
                        max_count: r.i32(0xc),
                        friction: r.f32(0x10),
                    })
                }
                "FCOV" => {
                    f.converge = Some(Converge {
                        follow_emitter: r.flag(0),
                        target: r.v3(4),
                        ratio: r.f32(0x10),
                        anim: FieldAnim::read(data, 0x14),
                    })
                }
                "FPAD" => {
                    f.pos_add = Some(PosAdd {
                        world: r.flag(0),
                        add: r.v3(4),
                        anim: FieldAnim::read(data, 0x10),
                    })
                }
                _ => {}
            }
        }
        f
    }
}

/// An emitter resource as the simulation reads it (`EmitterResource`).
struct ResData {
    /// Index in the set and, for a child, in its parent's children.
    top: usize,
    child: Option<usize>,
    params: EmitterParams,
    raw: Vec<u8>,
    anims: [Option<EmitterAnim>; 14],
    /// `er+0x180`: any emitter animation; `er+0x181`: a transform one.
    any_anim: bool,
    transform_anim: bool,
    tracks: [Track; 5],
    color_scale: f32,
    /// Fluctuation: X amp/period/phase random/phase, Y the same.
    fluct_x: [f32; 4],
    fluct_y: [f32; 4],
    rot_random: Vec3,
    rot_add: Vec3,
    rot_add_random: Vec3,
    rot_resistance: f32,
    init_rotate: Vec3,
    fields: Fields,
    /// Calc type 2: the stream-out program's constants.
    stream_out: Option<stream_out::Program>,
    primitive: Option<Primitive>,
    children: Vec<Arc<ResData>>,
}

const COLOR0: usize = 0;
const ALPHA0: usize = 1;
const COLOR1: usize = 2;
const ALPHA1: usize = 3;
const SCALE: usize = 4;

impl ResData {
    fn new(data: &EffectData, e: &fx::Emitter, top: usize, child: Option<usize>) -> Arc<Self> {
        let raw = data.block(e.resource).to_vec();
        let stat_bytes = data.block(e.static_block);
        let stat = Stat(stat_bytes);
        let r = Raw(&raw);
        let anims = ANIMS.map(|magic| {
            e.animations
                .iter()
                .find(|(m, _)| m == magic)
                .map(|(_, a)| a.clone())
        });
        let any_anim = anims.iter().any(Option::is_some);
        let transform_anim = anims[..3].iter().any(Option::is_some);
        // Tracks: keys, count, loop flag, period, random start.
        let track =
            |keys: usize, count: usize, looping: usize, period: usize, random: usize| Track {
                keys: std::array::from_fn(|k| stat.v4(keys + 16 * k)),
                count: stat.i32(count),
                period: if r.flag(looping) {
                    r.i32(period) as f32
                } else {
                    0.0
                },
                random_start: f32::from(r.u8(random)),
            };
        let tracks = [
            track(0x3c0, 0x60, 0x8d8, 0x8e4, 0x8dd),
            track(0x440, 0x64, 0x8d9, 0x8e8, 0x8de),
            track(0x4c0, 0x68, 0x8da, 0x8ec, 0x8df),
            track(0x540, 0x6c, 0x8db, 0x8f0, 0x8e0),
            track(0x600, 0x70, 0x8dc, 0x8f4, 0x8e1),
        ];
        let v3 = |at: usize| Vec3::new(stat.f32(at), stat.f32(at + 4), stat.f32(at + 8));
        let attributes: Vec<(String, &[u8])> = e
            .attributes
            .iter()
            .map(|(m, b)| (m.clone(), data.block(*b)))
            .collect();
        let primitive = e
            .params
            .shape_primitive
            .and_then(|id| data.file.primitive(id))
            .cloned();
        let children = e
            .children
            .iter()
            .enumerate()
            .map(|(k, c)| ResData::new(data, c, top, Some(k)))
            .collect();
        Arc::new(Self {
            top,
            child,
            params: e.params.clone(),
            anims,
            any_anim,
            transform_anim,
            tracks,
            color_scale: stat.f32(0x3b0),
            fluct_x: [
                stat.f32(0xe0),
                stat.f32(0xe8),
                stat.f32(0xf0),
                stat.f32(0xf8),
            ],
            fluct_y: [
                stat.f32(0xe4),
                stat.f32(0xec),
                stat.f32(0xf4),
                stat.f32(0xfc),
            ],
            rot_random: v3(0x710),
            rot_add: v3(0x720),
            rot_add_random: v3(0x730),
            rot_resistance: stat.f32(0x72c),
            init_rotate: v3(0x700),
            fields: Fields::read(&attributes),
            stream_out: (e.params.calc == 2).then(|| {
                let field = e.field_block.map(|b| data.block(b));
                stream_out::Program {
                    flags_a: stat.u32(0x50),
                    flags_b: stat.u32(0x54),
                    custom: stat.u32(0x5c),
                    gravity: v3(0xb0),
                    gravity_scale: stat.f32(0xbc),
                    air: stat.f32(0xc0),
                    field: std::array::from_fn(|k| field.map_or(0.0, |f| Stat(f).f32(4 * k))),
                }
            }),
            primitive,
            children,
            raw,
        })
    }

    fn raw(&self) -> Raw<'_> {
        Raw(&self.raw)
    }

    /// The emitter as baked.
    fn emitter<'a>(&self, file: &'a fx::EffectFile, set: usize) -> &'a fx::Emitter {
        let top = &file.sets[set].emitters[self.top];
        match self.child {
            Some(k) => &top.children[k],
            None => top,
        }
    }

    /// The particle buffer size (`eft_Emitter_AllocParticleBuffers`
    /// `0x03b57174`).
    fn capacity(&self) -> usize {
        let p = &self.params;
        if p.by_distance {
            return self.raw().u32(0x834) as usize;
        }
        let max_key = |anim: &Option<EmitterAnim>, v: f32| {
            anim.as_ref()
                .map_or(v, |a| a.keys.iter().fold(v, |m, k| m.max(k.value[0])))
        };
        let mut rate = p.rate;
        if self.anims[EATR].is_some() {
            rate = max_key(&self.anims[EATR], rate);
            if rate < 1.0 {
                rate = 1.0;
            }
        }
        let life = max_key(&self.anims[EAPL], p.life as f32);
        let per = (p.interval + 1) as f32;
        let ceil = |v: f32| f64::from(v).ceil();
        let duration = if p.duration != 0 {
            p.duration as u32 as f32
        } else {
            1.0
        };
        let n = if !(p.infinite_life && p.one_time) {
            let mut n = ceil(life / per) as u32;
            if p.one_time {
                n = n.min(ceil(duration / per) as i32 as u32);
            }
            n
        } else {
            ceil(duration / per) as i32 as u32
        };
        let mut capacity = (ceil(n as i32 as f32 * rate) + ceil(rate)) as i32;
        if p.division_mode == 0 {
            match p.volume {
                2 => capacity *= p.circle_divisions,
                13 => capacity *= p.line_divisions,
                _ => {}
            }
        }
        if !p.one_time && p.calc == 2 {
            let mut extra = ceil(rate) as i32;
            if p.division_mode == 0 {
                match p.volume {
                    2 => extra *= p.circle_divisions,
                    13 => extra *= p.line_divisions,
                    _ => {}
                }
            }
            capacity += extra;
        }
        // SI-EFX-14: the buffer is bounded so a bad resource cannot exhaust
        // memory; the game allocates what the formula says.
        capacity.clamp(0, 1 << 18) as usize
    }

    /// An 8-key track (`eft_Anim8Key_Eval` `0x03b5bc84`).
    fn track(&self, track: usize, rnd: f32, age: f32, life: f32) -> Vec3 {
        let t = &self.tracks[track];
        anim8(&t.keys, t.count, rnd, age, t.period, t.random_start, life)
    }

    /// A fluctuation wave (`eft_Wave_*` `0x03b5d82c..`), X or Y.
    fn wave(&self, y: bool, age: f32, rnd: f32) -> f32 {
        let [amp, period, phase_random, phase] = if y { self.fluct_y } else { self.fluct_x };
        let u = (f64::from(phase_random) * f64::from(rnd)
            + f64::from(((age + phase) as f64 / f64::from(period)) as f32)) as f32;
        match self.raw().u8(0x9ef) >> 4 {
            0 => {
                let a = (u * std::f32::consts::PI) as f64;
                let c = (a + a).cos();
                (-(f64::from((((c + 1.0) as f32) * 0.5) as f64 as f32) * f64::from(amp) - 1.0))
                    as f32
            }
            1 => {
                let frac = (f64::from(u) - f64::from(u).floor()) as f32;
                (-(f64::from(frac) * f64::from(amp) - 1.0)).abs() as f32
            }
            2 => {
                let frac = (f64::from(u) - f64::from(u).floor()) as f32;
                let high = if frac - 0.5 < 0.0 { 0.0 } else { 1.0 };
                (-(f64::from((1.0 - high) as f32) * f64::from(amp) - 1.0)).abs() as f32
            }
            _ => 1.0,
        }
    }

    /// The particle's current scale (`eft_CalcParticleScaleCpu`
    /// `0x03b5de40`), from its scale attribute.
    fn particle_scale(&self, life: f32, age: f32, scale: Vec4, random: Vec4) -> Vec3 {
        let r = self.raw();
        let mut s = scale.truncate();
        let t = &self.tracks[SCALE];
        if t.count < 2 {
            s *= t.keys[0].truncate();
        } else {
            s *= self.track(SCALE, random.x, age, life);
        }
        if !r.flag(0x9ed) {
            return s;
        }
        let x = self.wave(false, age, random.x);
        let y = if r.flag(0x9ee) {
            self.wave(true, age, random.x)
        } else {
            x
        };
        s.x = (f64::from(s.x) * f64::from(x)) as f32;
        s.y = (f64::from(s.y) * f64::from(y)) as f32;
        s
    }

    /// The particle's current colour 0 or 1 with alpha
    /// (`eft_CalcParticleColor0Cpu` `0x03b5e234` / `…Color1Cpu`
    /// `0x03b5e754`).
    fn particle_color(
        &self,
        second: bool,
        emitter_alpha: f32,
        life: f32,
        age: f32,
        random: Vec4,
        multiplier: Vec4,
        emitter_color: Vec3,
    ) -> Vec4 {
        let r = self.raw();
        let p = &self.params;
        let (constant, alpha_at, color, alpha) = if second {
            (0x9b8, 0x9c4, COLOR1, ALPHA1)
        } else {
            (0x9a8, 0x9b4, COLOR0, ALPHA0)
        };
        let (source, alpha_source) = if second {
            (p.color_sources[1], p.color_sources[3])
        } else {
            (p.color_sources[0], p.color_sources[2])
        };
        let mut c = r.v3(constant).extend(r.f32(alpha_at));
        let t = &self.tracks[color];
        if source == 3 {
            let rnd = if 0.0 <= random.x {
                random.x.min(0.999_999)
            } else {
                0.0
            };
            let index = ((rnd * t.count as f32) as i32).clamp(0, 7) as usize;
            c = t.keys[index].truncate().extend(c.w);
        }
        if source == 2 && 0 < t.count {
            c = self.track(color, random.x, age, life).extend(c.w);
        }
        if alpha_source == 2 && 0 < self.tracks[alpha].count {
            c.w = self.track(alpha, random.x, age, life).x;
        }
        let scale = self.color_scale;
        c.x = c.x * multiplier.x * emitter_color.x * scale;
        c.y = c.y * multiplier.y * emitter_color.y * scale;
        c.z = c.z * multiplier.z * emitter_color.z * scale;
        c.w *= (f64::from(multiplier.w) * f64::from(emitter_alpha)) as f32;
        if r.flag(0x9ec) {
            let a = c.w * self.wave(false, age, random.x);
            c.w = a.clamp(0.0, 1.0);
        }
        c
    }

    /// The particle's current rotation (`eft_CalcParticleRotateCpu`
    /// `0x03b5ec74`).
    fn particle_rotation(&self, age: f32, init: Vec3, rnd: Vec4) -> Vec3 {
        let r = self.raw();
        let mut init = init;
        let mut w = Vec3::new(
            (rnd.x + rnd.y) * 0.5 * self.rot_add_random.x + self.rot_add.x,
            (rnd.y + rnd.z) * 0.5 * self.rot_add_random.y + self.rot_add.y,
            (rnd.z + rnd.x) * 0.5 * self.rot_add_random.z + self.rot_add.z,
        );
        if r.flag(0x8ad) && 0.5 <= rnd.y {
            init.x = -init.x;
            w.x = -w.x;
        }
        if r.flag(0x8ae) && 0.5 <= rnd.z {
            init.y = -init.y;
            w.y = -w.y;
        }
        if r.flag(0x8af) && 0.5 <= rnd.x {
            init.z = -init.z;
            w.z = -w.z;
        }
        let r0 = Vec3::new(
            rnd.x * self.rot_random.x + init.x,
            rnd.y * self.rot_random.y + init.y,
            rnd.z * self.rot_random.z + init.z,
        );
        let q = self.rot_resistance;
        let t = if q != 1.0 {
            let qa = f64::from(q).powf(f64::from(age));
            ((1.0 - qa) as f32 / (1.0 - q)) as f64
        } else {
            f64::from(age)
        };
        Vec3::new(
            (f64::from(w.x) * t + f64::from(r0.x)) as f32,
            (f64::from(w.y) * t + f64::from(r0.y)) as f32,
            (f64::from(w.z) * t + f64::from(r0.z)) as f32,
        )
    }
}

/// `eft_Anim8Key_Eval` `0x03b5bc84`.
fn anim8(
    keys: &[Vec4; 8],
    count: i32,
    rnd: f32,
    age: f32,
    period: f32,
    random_start: f32,
    life: f32,
) -> Vec3 {
    let count = count.clamp(0, 8) as usize;
    match count {
        0 => return Vec3::ONE,
        1 => return keys[0].truncate(),
        _ => {}
    }
    let t = if period <= 0.0 {
        (f64::from(age) / f64::from(life)) as f32
    } else {
        let x = f64::from((f64::from(random_start) * f64::from(rnd)) as f32) * f64::from(period)
            + f64::from(age);
        let x = f64::from(x as f32);
        ((x % f64::from(period)) / f64::from(period)) as f32
    };
    let last = keys[count - 1];
    if last.w <= t {
        return last.truncate();
    }
    if t < keys[0].w {
        return keys[0].truncate();
    }
    for i in 0..count - 1 {
        let (k0, k1) = (keys[i], keys[i + 1]);
        if k0.w <= t && t < k1.w {
            let span = k1.w - k0.w;
            let dt = t - k0.w;
            return Vec3::new(
                ((k1.x - k0.x) / span) * dt + k0.x,
                ((k1.y - k0.y) / span) * dt + k0.y,
                ((k1.z - k0.z) / span) * dt + k0.z,
            );
        }
    }
    Vec3::ZERO
}

/// `eft_EmitterAnim_Eval` `0x03b75734` at the emitter frame: linear keys,
/// looped or held; `done` once past the last key.
fn emitter_anim(anim: &EmitterAnim, frame: f32, value: &mut Vec3, done: &mut bool) {
    let keys = &anim.keys;
    let Some(first) = keys.first() else { return };
    if keys.len() == 1 {
        *value = Vec3::from(first.value);
        return;
    }
    let last = keys[keys.len() - 1];
    let mut f = frame;
    if anim.looping {
        f = (f64::from(f) % f64::from(last.time)) as f32;
    }
    if f < first.time {
        *value = Vec3::from(first.value);
        return;
    }
    if last.time <= f {
        *value = Vec3::from(last.value);
        *done = true;
        return;
    }
    for pair in keys.windows(2) {
        let (k0, k1) = (pair[0], pair[1]);
        if k0.time <= f && f < k1.time {
            let inv = 1.0 / (k1.time - k0.time);
            let dt = f - k0.time;
            let a = Vec3::from(k0.value);
            let b = Vec3::from(k1.value);
            *value = Vec3::new(
                a.x + ((b.x - a.x) * inv) * dt,
                a.y + ((b.y - a.y) * inv) * dt,
                dt * (inv * (b.z - a.z)) + a.z,
            );
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// The set.

/// Set-level parameters (`set+…`, docs/research/eft-runtime.md §1.7):
/// the ELink asset's, and what the game's API may set.
#[derive(Clone, Debug, PartialEq)]
pub struct SetParams {
    /// The ELink asset's scale and offset, applied to the set matrix.
    pub scale: f32,
    pub offset: Vec3,
    /// Set colour rgba (`set+0xc0`).
    pub color: Vec4,
    /// Emission ratio (`set+0x3c`, ≤ 1), interval scale (`set+0x40`, ≥ 1),
    /// life scale (`set+0x44`, ≤ 1).
    pub emission_ratio: f32,
    pub interval_scale: f32,
    pub life_scale: f32,
    /// Particle scale (`set+0xdc`, × the matrix's column scales for the
    /// shader) and the scale applied at emission (`set+0xe8`).
    pub particle_scale: Vec3,
    pub emission_scale: Vec3,
    /// Emitter volume scale (`set+0xa8`).
    pub volume_scale: Vec3,
    /// All-direction, directional and random velocity scales
    /// (`set+0x154`, `+0x174`, `+0x158`) and an extra world velocity
    /// (`set+0x15c`).
    pub omni_velocity_scale: f32,
    pub directional_velocity_scale: f32,
    pub velocity_random_scale: f32,
    pub add_velocity: Vec3,
    /// Every emitter's colour 0/1 multipliers (`em+0x42c`, `em+0x43c`).
    pub color0: Vec4,
    pub color1: Vec4,
    /// The set resource's byte `+0x16`: near-culled one-time emitters hide
    /// instead of fading out.
    // SI-EFX-12: not baked; false.
    pub keep_when_near: bool,
    /// Draw path 6 switches to 7 nearer than this (m), else 8.
    // SI-EFX-13: a runtime game value (`*(0x1047c210)+0x13540`) not traced.
    pub path6_distance: f32,
}

impl Default for SetParams {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset: Vec3::ZERO,
            color: Vec4::ONE,
            emission_ratio: 1.0,
            interval_scale: 1.0,
            life_scale: 1.0,
            particle_scale: Vec3::ONE,
            emission_scale: Vec3::ONE,
            volume_scale: Vec3::ONE,
            omni_velocity_scale: 1.0,
            directional_velocity_scale: 1.0,
            velocity_random_scale: 1.0,
            add_velocity: Vec3::ZERO,
            color0: Vec4::ONE,
            color1: Vec4::ONE,
            keep_when_near: false,
            path6_distance: 0.0,
        }
    }
}

struct SetState {
    /// `set+0x48` SRT, `set+0x78` RT, `set+0xb4` column scales.
    srt: M34,
    rt: M34,
    column_scale: Vec3,
    /// `set+10`: the matrix changed (or an emitter emitted) this frame.
    matrix_changed: bool,
    fade: bool,
    stop: bool,
    /// `set+0`: some top-level emitter loops.
    has_loop: bool,
    random: u32,
    step: f32,
    params: SetParams,
    /// What the stream-out program reads from BotW's blocks and the scene.
    env: StreamOutEnv,
}

impl SetState {
    /// `eft_EmitterSet_SetMtx` `0x03b59550`.
    fn set_matrix(&mut self, m: M34) {
        let a = &m.0;
        let sx = f64::from(a[2][0] * a[2][0] + a[0][0] * a[0][0] + a[1][0] * a[1][0]).sqrt() as f32;
        let sy =
            f64::from(a[2][1] * a[2][1] + (a[0][1] * a[0][1] + a[1][1] * a[1][1])).sqrt() as f32;
        let sz =
            f64::from(a[2][2] * a[2][2] + (a[0][2] * a[0][2] + a[1][2] * a[1][2])).sqrt() as f32;
        let mut rt = m;
        for (j, s) in [sx, sy, sz].into_iter().enumerate() {
            let inv = if 0.0 < s { 1.0 / s } else { 0.0 };
            for row in rt.0.iter_mut() {
                row[j] = if 0.0 < s { row[j] * inv } else { 0.0 };
            }
        }
        self.srt = m;
        self.rt = rt;
        self.column_scale = Vec3::new(sx, sy, sz);
        self.matrix_changed = true;
    }

    /// `set+0xf4`: particle scale × matrix column scale.
    fn shader_scale(&self) -> Vec3 {
        self.params.particle_scale * self.column_scale
    }
}

/// What a child emitter knows of its parent particle (`em+0x3c0..`).
#[derive(Clone, Copy, Debug, Default)]
struct ParentParticle {
    life: f32,
    age: f32,
    position: Vec3,
    velocity: Vec3,
    scale: Vec4,
    init_rotate: Vec3,
    random: Vec4,
}

/// The parent emitter's current values a child reads.
struct ParentSnapshot {
    res: Arc<ResData>,
    alpha0: f32,
    alpha1: f32,
    color0_mul: Vec4,
    color1_mul: Vec4,
    color0: Vec3,
    alpha_fade: f32,
}

struct Slot {
    /// `info[0]`: the particle's number; 0 = free.
    id: u32,
    collisions: i32,
    birth: f32,
    life: f32,
    attr: ParticleAttr,
    /// Child emitters spawned by this particle, per child resource.
    children: Vec<Option<u64>>,
}

struct Emitter {
    res: Arc<ResData>,
    uid: u64,
    parent: Option<ParentParticle>,
    rng: EmitterRng,
    frame: f32,
    step: f32,
    counter: f32,
    accumulator: f32,
    interval: f32,
    lod_ratio: f32,
    last_emit: f32,
    fade_out: f32,
    fade_in: f32,
    emitted: bool,
    lod_fade: bool,
    lod_visible: bool,
    draw_path: u32,
    local_srt: M34,
    local_rt: M34,
    world_srt: M34,
    world_rt: M34,
    previous_position: Vec3,
    movement: Vec3,
    anim: [Vec3; 14],
    anim_done: [bool; 14],
    particle_count: u32,
    color_mul: [Vec4; 2],
    capacity: usize,
    ring: usize,
    count: usize,
    slots: Vec<Slot>,
    children: Vec<Vec<Emitter>>,
    out: Vec<ParticleAttr>,
    dynamic: EmitterDynamic,
}

/// One emitter's drawing input.
pub struct EmitterView<'a> {
    pub emitter: &'a fx::Emitter,
    /// `sysEmitterDynamicUniformBlock`.
    pub dynamic: EmitterDynamic,
    /// The live particles, in slot order.
    pub particles: &'a [ParticleAttr],
    /// The current draw path (`em+0x2b8`; LOD may switch 5 and 6).
    pub draw_path: u32,
    /// Drawn at all (`eft_EmitterSet_DrawEmitter` `0x03b5a5a4`).
    pub visible: bool,
}

/// A running emitter set.
pub struct EmitterSetSim {
    file: Arc<EffectData>,
    set: usize,
    tables: Arc<EffectTables>,
    state: SetState,
    resources: Vec<Arc<ResData>>,
    emitters: Vec<Emitter>,
    /// SI-EFX-10: emitters seeded from the game's random (mode 0) that are
    /// created later (children) draw from this generator, seeded from the
    /// game's at creation; the game's own sequence depends on everything
    /// else that runs.
    random: SeadRandom,
    next_uid: u64,
}

struct Ctx<'a> {
    tables: &'a EffectTables,
    vectors: &'a VecTables,
}

impl EmitterSetSim {
    /// `eft_System_CreateEmitterSetID` then `SetMtx`: the set's top-level
    /// emitters in resource order. `matrix` is the set's SRT; `set_random`
    /// the per-set random (`set+0x1c`); `game_random` seeds emitters of
    /// seed mode 0.
    pub fn new(
        file: Arc<EffectData>,
        set: usize,
        matrix: Affine3A,
        params: SetParams,
        tables: Arc<EffectTables>,
        game_random: &mut SeadRandom,
        set_random: u32,
    ) -> Self {
        let resources: Vec<Arc<ResData>> = file
            .file
            .sets
            .get(set)
            .map(|s| {
                s.emitters
                    .iter()
                    .enumerate()
                    .map(|(i, e)| ResData::new(&file, e, i, None))
                    .collect()
            })
            .unwrap_or_default();
        let mut params = params;
        params.emission_ratio = params.emission_ratio.min(1.0);
        params.interval_scale = params.interval_scale.max(1.0);
        params.life_scale = params.life_scale.min(1.0);
        let mut state = SetState {
            srt: M34::IDENTITY,
            rt: M34::IDENTITY,
            column_scale: Vec3::ONE,
            matrix_changed: false,
            fade: false,
            stop: false,
            has_loop: false,
            random: set_random,
            step: 1.0,
            params,
            // SI-EFX-33: no wind, drift or ground until the runner sets
            // them (`set_stream_out_env`); the game's are BotW's `cus1`
            // values of the frame (docs/research/eft-custom-blocks.md §4).
            env: StreamOutEnv::default(),
        };
        // Emitters are made before the matrix is set: their previous
        // position is the identity's.
        let ctx = Ctx {
            tables: &tables,
            vectors: vectors(),
        };
        let mut emitters = Vec::new();
        for res in &resources {
            let uid = emitters.len() as u64;
            if let Some(e) = Emitter::new(&ctx, res.clone(), &state, game_random, None, uid) {
                if !res.params.one_time {
                    state.has_loop = true;
                }
                emitters.push(e);
            }
        }
        let child_seed = game_random.next();
        let next_uid = emitters.len() as u64;
        let mut sim = Self {
            file,
            set,
            tables,
            state,
            resources,
            emitters,
            random: SeadRandom::new(child_seed),
            next_uid,
        };
        sim.set_matrix(matrix);
        sim
    }

    /// Moves the set (`eft_EmitterSet_SetMtx`).
    pub fn set_matrix(&mut self, matrix: Affine3A) {
        // SI-EFX-11: where xlink applies the ELink asset's scale and offset
        // was not traced; taken as the set's local offset and scale.
        let p = &self.state.params;
        let local = Affine3A::from_scale_rotation_translation(
            Vec3::splat(p.scale),
            bevy::math::Quat::IDENTITY,
            p.offset,
        );
        self.state.set_matrix(M34::from_affine(&(matrix * local)));
    }

    /// Changes the set's parameters while it plays (the ELink asset's live
    /// curves), clamped like the library's setters.
    pub fn update_params(&mut self, f: impl FnOnce(&mut SetParams)) {
        let p = &mut self.state.params;
        f(p);
        p.emission_ratio = p.emission_ratio.min(1.0);
        p.interval_scale = p.interval_scale.max(1.0);
        p.life_scale = p.life_scale.min(1.0);
    }

    /// The wind, drift and ground the stream-out program (calc type 2)
    /// reads; zero wind and no ground until set.
    pub fn set_stream_out_env(&mut self, env: StreamOutEnv) {
        self.state.env = env;
    }

    /// The set's parameters.
    pub fn params(&self) -> &SetParams {
        &self.state.params
    }

    /// `eft_EmitterSet_Fade`: emitters stop as their resources say.
    pub fn fade(&mut self) {
        self.state.fade = true;
    }

    /// Stops emission (`set+6`).
    pub fn stop(&mut self) {
        self.state.stop = true;
    }

    /// Whether any emitter is left.
    pub fn alive(&self) -> bool {
        !self.emitters.is_empty()
    }

    /// One frame (`eft_EmitterSet_Calc` `0x03b5a348`); `frames` is the
    /// step (1 at 30 fps), `camera` the eye for the LOD callback.
    pub fn step(&mut self, frames: f32, camera: Vec3) {
        self.state.step = frames;
        let ctx = Ctx {
            tables: &self.tables,
            vectors: vectors(),
        };
        let mut uid = self.next_uid;
        let mut keep = Vec::with_capacity(self.emitters.len());
        for mut e in std::mem::take(&mut self.emitters) {
            if self.state.stop && e.count == 0 {
                e.frame += frames;
                keep.push(e);
                continue;
            }
            let alive = e.calc_with_lod(
                &ctx,
                &mut self.state,
                camera,
                None,
                &mut self.random,
                &mut uid,
            );
            let snapshot = e.snapshot();
            let mut children_alive = false;
            for list in &mut e.children {
                list.retain_mut(|c| {
                    let a = c.calc_with_lod(
                        &ctx,
                        &mut self.state,
                        camera,
                        Some(&snapshot),
                        &mut self.random,
                        &mut uid,
                    );
                    children_alive |= a;
                    a
                });
            }
            if alive || children_alive {
                keep.push(e);
            }
        }
        self.emitters = keep;
        self.next_uid = uid;
        if 0.0 < frames {
            self.state.matrix_changed = false;
        }
    }

    /// Every emitter in draw order (`eft_EmitterSet_Draw` `0x03b5a8ac`):
    /// top-level emitters in creation order, each with its children drawn
    /// before (`0x7e1`) or after it.
    pub fn views(&self) -> Vec<EmitterView<'_>> {
        let file = &self.file.file;
        fn view<'a>(e: &'a Emitter, file: &'a fx::EffectFile, set: usize) -> EmitterView<'a> {
            EmitterView {
                emitter: e.res.emitter(file, set),
                dynamic: e.dynamic,
                particles: &e.out,
                draw_path: e.draw_path,
                visible: e.visible(),
            }
        }
        let view = |e| view(e, file, self.set);
        let mut views = Vec::new();
        for e in &self.emitters {
            for before in [true, false] {
                if !before {
                    views.push(view(e));
                }
                for list in &e.children {
                    for c in list {
                        if c.res.params.draw_before_parent == before {
                            views.push(view(c));
                        }
                    }
                }
            }
        }
        views
    }
}

impl Emitter {
    /// `eft_EmitterSet_CreateEmitter` → `eft_Emitter_Initialize`
    /// `0x03b579cc` → `eft_Emitter_AllocParticleBuffers` `0x03b57174`.
    fn new(
        ctx: &Ctx,
        res: Arc<ResData>,
        set: &SetState,
        game_random: &mut SeadRandom,
        parent: Option<ParentParticle>,
        uid: u64,
    ) -> Option<Self> {
        let p = &res.params;
        let seed = match p.seed_mode {
            0 => game_random.next(),
            1 => set.random,
            _ => p.seed.wrapping_mul(0xDFDC_1C35),
        };
        let capacity = res.capacity();
        if capacity == 0 {
            return None;
        }
        let mut anim = [Vec3::ZERO; 14];
        anim[EAES] = Vec3::from(p.scale);
        anim[EAER] = Vec3::from(p.rotate);
        anim[EAET] = Vec3::from(p.translate);
        anim[EAC0] = Vec3::new(p.color0[0], p.color0[1], p.color0[2]);
        anim[EAC1] = Vec3::new(p.color1[0], p.color1[1], p.color1[2]);
        anim[EASL] = Vec3::from(p.particle_scale);
        anim[EASS] = Vec3::from(p.form_scale);
        anim[EATR].x = p.rate;
        anim[EAPL].x = p.life as f32;
        anim[EAA0].x = p.color0[3];
        anim[EAA1].x = p.color1[3];
        anim[EAOV].x = p.omni_velocity;
        anim[EADV].x = p.directional_velocity;
        anim[EAGV].x = p.gravity_scale;
        let children = (0..res.children.len()).map(|_| Vec::new()).collect();
        let mut e = Self {
            uid,
            parent,
            rng: EmitterRng::seeded(seed),
            frame: 0.0,
            step: set.step,
            counter: 0.0,
            accumulator: 0.0,
            interval: 0.0,
            lod_ratio: 1.0,
            last_emit: 0.0,
            fade_out: 1.0,
            fade_in: if p.fade_in_alpha || p.fade_in_scale {
                0.0
            } else {
                1.0
            },
            emitted: false,
            lod_fade: false,
            lod_visible: true,
            draw_path: p.draw_path,
            local_srt: M34::IDENTITY,
            local_rt: M34::IDENTITY,
            world_srt: M34::IDENTITY,
            world_rt: M34::IDENTITY,
            previous_position: set.srt.translation(),
            movement: Vec3::ZERO,
            anim,
            anim_done: [false; 14],
            particle_count: 0,
            color_mul: [set.params.color0, set.params.color1],
            capacity,
            ring: 0,
            count: 0,
            slots: Vec::new(),
            children,
            out: Vec::new(),
            dynamic: EmitterDynamic::default(),
            res,
        };
        e.slots = (0..capacity)
            .map(|_| Slot {
                id: 0,
                collisions: 0,
                birth: 0.0,
                life: 0.0,
                attr: ParticleAttr::default(),
                children: vec![None; e.res.children.len()],
            })
            .collect();
        e.randomize_local_matrix(ctx);
        Some(e)
    }

    /// `eft_Emitter_RandomizeLocalMatrix` `0x03b56a60`: draws rx, ry, rz,
    /// tx, ty, tz.
    fn randomize_local_matrix(&mut self, ctx: &Ctx) {
        let p = &self.res.params;
        let mut d = |base: f32, random: f32| {
            let r = self.rng.lcg.f32();
            (r + r - 1.0) * random + base
        };
        let rx = d(p.rotate[0], p.rotate_random[0]);
        let ry = d(p.rotate[1], p.rotate_random[1]);
        let rz = d(p.rotate[2], p.rotate_random[2]);
        let tx = d(p.translate[0], p.translate_random[0]);
        let ty = d(p.translate[1], p.translate_random[1]);
        let tz = d(p.translate[2], p.translate_random[2]);
        let (srt, rt) = make_srt(
            ctx.tables,
            Vec3::from(p.scale),
            Vec3::new(rx, ry, rz),
            Vec3::new(tx, ty, tz),
        );
        self.local_srt = srt;
        self.local_rt = rt;
    }

    fn is_child(&self) -> bool {
        self.parent.is_some()
    }

    fn snapshot(&self) -> ParentSnapshot {
        let p = &self.res.params;
        let mut fade = if p.fade_in_alpha { self.fade_in } else { 1.0 };
        if p.fade_out_alpha {
            fade *= self.fade_out;
        }
        ParentSnapshot {
            res: self.res.clone(),
            alpha0: self.anim[EAA0].x,
            alpha1: self.anim[EAA1].x,
            color0_mul: self.color_mul[0],
            color1_mul: self.color_mul[1],
            color0: self.anim[EAC0],
            alpha_fade: fade,
        }
    }

    fn visible(&self) -> bool {
        self.res.params.visible && 0.0 < self.fade_out && self.lod_visible && !self.out.is_empty()
    }

    /// `eft_EmitterSet_CalcEmitter` `0x03b5a0f0`: the LOD callback, then
    /// the emitter. Returns whether it lives on.
    fn calc_with_lod(
        &mut self,
        ctx: &Ctx,
        set: &mut SetState,
        camera: Vec3,
        parent: Option<&ParentSnapshot>,
        random: &mut SeadRandom,
        uid: &mut u64,
    ) -> bool {
        let step = set.step;
        self.step = step;
        if step <= 0.0 {
            self.update_dynamic(set, parent);
            return true;
        }
        if self.lod(set, camera) == 2 {
            self.lod_fade = true;
        }
        self.calc(ctx, set, parent, random, uid)
    }

    /// `botw_EmitterCalcLodCallback` `0x0387b8d0` (the parts that do not
    /// depend on game state): draw path switching, near/far culling and
    /// the emission ratio by distance. Returns 2 to fade the emitter out.
    fn lod(&mut self, set: &SetState, camera: Vec3) -> u8 {
        let p = &self.res.params;
        let d = f64::from(camera.distance(set.rt.translation()));
        if p.draw_path == 5 {
            self.draw_path = if d < 80.0 { 8 } else { 0 };
        }
        if p.draw_path == 6 {
            self.draw_path = if d < f64::from(set.params.path6_distance) {
                7
            } else {
                8
            };
        }
        if !(p.lod_every_frame || self.frame == 0.0) {
            return 0;
        }
        let near = f64::from(p.near);
        let far = f64::from(p.far);
        let cull = d < near || (far != -1.0 && far < d && !(p.lod_emission && 0 < p.far_percent));
        if cull {
            if !set.has_loop && !set.params.keep_when_near {
                return 2;
            }
            self.lod_visible = false;
        } else {
            self.lod_visible = true;
        }
        if p.lod_emission {
            let t = ((d - near) as f32 / (far - near) as f32) as f64;
            let r = f64::from(p.far_percent as f32 * 0.01);
            let t = if 0.0 <= t { t.min(1.0) } else { 0.0 };
            let ratio = f64::from((1.0 - t) as f32) * f64::from((1.0 - r) as f32) + r;
            self.lod_ratio = (ratio as f32).min(1.0);
        }
        0
    }

    /// `eft_Emitter_Calc` `0x03b6ba64`. Returns whether the emitter lives.
    fn calc(
        &mut self,
        ctx: &Ctx,
        set: &mut SetState,
        parent: Option<&ParentSnapshot>,
        random: &mut SeadRandom,
        uid: &mut u64,
    ) -> bool {
        let res = self.res.clone();
        let p = &res.params;
        let step = set.step;
        let f = self.frame;
        // Emission window (§1.3).
        let (start, end) = match &self.parent {
            Some(pp) => {
                let start = pp.life * (p.child_start_percent as u32 as f32 / 100.0);
                let end = if p.one_time {
                    start + p.duration as u32 as f32
                } else {
                    pp.life
                };
                (start, end)
            }
            None => (
                p.start as u32 as f32,
                p.start.wrapping_add(p.duration) as u32 as f32,
            ),
        };
        // Emitter animations (§1.5).
        if res.any_anim {
            let mut evaluated = 0;
            for (i, anim) in res.anims.iter().enumerate() {
                if let Some(a) = anim
                    && a.enabled
                    && (!self.anim_done[i] || a.looping)
                {
                    emitter_anim(a, f, &mut self.anim[i], &mut self.anim_done[i]);
                    evaluated += 1;
                    if i == EAOV {
                        self.anim[EAOV].x *= set.params.omni_velocity_scale;
                    }
                    if i == EASS {
                        self.anim[EASS] *= set.params.volume_scale;
                    }
                }
            }
            let life = p.life as f32;
            if evaluated != 0 && life < self.anim[EAPL].x {
                self.anim[EAPL].x = life;
            }
        } else {
            self.anim[EAOV].x = p.omni_velocity * set.params.omni_velocity_scale;
            self.anim[EASS] = Vec3::from(p.form_scale) * set.params.volume_scale;
        }
        // World matrices (§1.2).
        if let Some(pp) = &self.parent {
            self.world_srt = self
                .local_srt
                .with_translation(self.local_srt.translation() + pp.position);
            self.world_rt = self
                .local_rt
                .with_translation(self.local_rt.translation() + pp.position);
        } else if res.transform_anim {
            let rotate = if res.anims[EAER].is_some() {
                self.anim[EAER] * 0.017_453_292
            } else {
                self.anim[EAER]
            };
            let (srt, rt) = make_srt(ctx.tables, self.anim[EAES], rotate, self.anim[EAET]);
            self.world_srt = set.srt.mul(&srt);
            self.world_rt = set.rt.mul(&rt);
        } else if set.matrix_changed {
            self.world_srt = set.srt.mul(&self.local_srt);
            self.world_rt = set.rt.mul(&self.local_rt);
        }
        // The emitter's movement in its own space.
        if f != 0.0 {
            let inv = self.world_srt.inverse();
            self.movement =
                inv.point(self.world_srt.translation()) - inv.point(self.previous_position);
        }
        // Fade in, fade out (§1.4).
        let mut emit = !set.stop;
        let fading_in = (p.fade_in_alpha || p.fade_in_scale) && self.fade_in < 1.0;
        if fading_in {
            let frames = p.fade_in_frames;
            let next = (f64::from(self.fade_in)
                + f64::from((f64::from(step) / f64::from(frames as f32)) as f32))
                as f32;
            self.fade_in = if frames < 1 || 1.0 < next { 1.0 } else { next };
        }
        if set.fade || self.lod_fade {
            if p.fade_stops_emission {
                emit = false;
            }
            if p.fade_out_alpha || p.fade_out_scale {
                let frames = f64::from(p.fade_out_frames as u32 as f32);
                let next = self.fade_out - (f64::from(step) / frames) as f32;
                if frames <= 0.0 || next < 0.0 {
                    self.fade_out = 0.0;
                    return false;
                }
                self.fade_out = next;
            }
        }
        // Emission (§2).
        let window_closed = (p.one_time || self.is_child()) && end <= f && self.emitted;
        if emit && !(f < start) && !window_closed {
            if !p.by_distance {
                if self.counter < self.interval {
                    self.counter += step;
                } else {
                    let over = self.counter - self.interval;
                    let reduce = (p.rate_random as f32 / 100.0) * p.rate;
                    let r = self.rng.lcg.f32();
                    let rate = if matches!(p.volume, 5 | 6) {
                        p.rate
                    } else {
                        self.anim[EATR].x
                    };
                    let mut n = (rate - reduce * r) * self.lod_ratio * set.params.emission_ratio
                        + self.accumulator;
                    if n < 0.0 {
                        n = 0.0;
                    }
                    let count = n as u32;
                    self.accumulator = n;
                    if count == 0 {
                        self.counter = 0.0;
                    } else {
                        self.emit_particles(ctx, set, parent, count, random, uid);
                        self.reset_interval(ctx, set);
                        set.matrix_changed = true;
                        self.accumulator -= count as i32 as f32;
                        self.counter = step + over;
                        self.last_emit = self.frame;
                    }
                }
            } else {
                self.emit_by_distance(ctx, set, parent, random, uid);
            }
        }
        // Particle update (§4).
        if p.calc == 0 {
            self.calc_particles_cpu(ctx);
        } else if p.calc == 2 {
            self.calc_stream_out(ctx, set);
        }
        if p.life_random != 0 && self.ring != 0 {
            let mut i = self.ring - 1;
            while i != 0 {
                let s = &self.slots[i];
                if self.frame - s.birth <= s.life {
                    break;
                }
                self.ring = i;
                self.count = i;
                i -= 1;
            }
        }
        if self.count != 0 {
            self.update_dynamic(set, parent);
        }
        self.collect(f);
        self.previous_position = self.world_srt.translation();
        self.frame += step;
        // Death (§1.4).
        let life = p.life as f32;
        if !p.one_time || set.stop {
            let fading = set.fade || self.lod_fade || self.is_child();
            !(fading && (start + self.last_emit) + life < f)
        } else if p.infinite_life {
            true
        } else {
            let mut t = end + life;
            if p.fade_out_alpha || p.fade_out_scale {
                t += p.fade_out_frames as u32 as f32;
            }
            // SI-EFX-15: stripe plugins (EP02/EP03) extend this by their
            // history length; not ported.
            !(t < f)
        }
    }

    /// `eft_Emitter_ResetEmitInterval` `0x03b57e2c`.
    fn reset_interval(&mut self, ctx: &Ctx, set: &SetState) {
        let p = &self.res.params;
        let extra = self.rng.lcg.below(p.interval_random);
        self.interval =
            ((p.interval as f32 + 1.0 + extra as f32) * 1.0) * set.params.interval_scale;
        if p.rerandomize_matrix {
            self.randomize_local_matrix(ctx);
        }
    }

    /// Distance emission (§2.2): particles spaced along the emitter's path.
    fn emit_by_distance(
        &mut self,
        ctx: &Ctx,
        set: &mut SetState,
        parent: Option<&ParentSnapshot>,
        random: &mut SeadRandom,
        uid: &mut u64,
    ) {
        let p = self.res.params.clone();
        let moved = self.movement.length();
        let distance = if moved < p.distance_threshold || moved == 0.0 {
            p.distance_min
        } else if moved < p.distance_min {
            (moved * p.distance_min) / moved
        } else if p.distance_max < moved {
            (moved * p.distance_max) / moved
        } else {
            moved
        };
        let mut accumulated = self.accumulator + distance;
        if p.distance_unit == 0.0 {
            self.accumulator = accumulated;
            return;
        }
        let n = (f64::from(accumulated) / f64::from(p.distance_unit)) as i32;
        if n < 1 {
            self.accumulator = accumulated;
            return;
        }
        let previous = self.previous_position;
        let current = self.world_rt.translation();
        let (srt, rt) = (self.world_srt, self.world_rt);
        for _ in 0..n {
            accumulated -= p.distance_unit;
            let t = if distance != 0.0 {
                accumulated / distance
            } else {
                0.0
            };
            let at = current * (1.0 - t) + previous * t;
            self.world_srt = srt.with_translation(at);
            self.world_rt = rt.with_translation(at);
            self.emit_particles(ctx, set, parent, 1, random, uid);
            self.world_srt = srt;
            self.world_rt = rt;
        }
        self.last_emit = self.frame;
        self.accumulator = accumulated;
    }

    /// `eft_Emitter_EmitParticles` `0x03b6ed38`.
    fn emit_particles(
        &mut self,
        ctx: &Ctx,
        set: &mut SetState,
        parent: Option<&ParentSnapshot>,
        count: u32,
        random: &mut SeadRandom,
        uid: &mut u64,
    ) {
        let res = self.res.clone();
        let p = &res.params;
        let e = self.rng.lcg.f32();
        let mut n = count;
        if p.division_mode == 0 {
            let divided = |divisions: i32, random: i32| {
                let cut = (e * random as u32 as f32) * 0.01 * divisions as u32 as f32;
                let cut = if cut < 2.147_483_6e9 {
                    cut as i32
                } else {
                    ((cut - 2.147_483_6e9) as i32).wrapping_add(i32::MIN)
                };
                divisions.wrapping_sub(cut) as u32
            };
            match p.volume {
                2 => n = n.wrapping_mul(divided(p.circle_divisions, p.circle_division_random)),
                13 => n = n.wrapping_mul(divided(p.line_divisions, p.line_division_random)),
                _ => {}
            }
        }
        for k in 0..n {
            // The slot (§2.3): the ring, or the first free one when lives
            // differ.
            let (slot, use_ring) = if p.life_random == 0 || p.one_time {
                (self.ring, true)
            } else if self.count == 0 {
                (0, true)
            } else {
                let found = if p.calc == 0 {
                    (0..self.count).find(|&i| self.slots[i].id == 0)
                } else {
                    (0..self.count).find(|&i| {
                        let s = &self.slots[i];
                        0.0 < (self.frame - s.birth) - s.life
                    })
                };
                found.map_or((self.ring, true), |i| (i, false))
            };
            if slot >= self.slots.len() {
                return;
            }
            self.emitted = true;
            if !self.init_particle(ctx, set, slot, e, k, random, uid) {
                continue;
            }
            let s = &mut self.slots[slot];
            let v = s.attr.local_vec;
            s.attr.local_diff = Vec4::new(v.x, v.y, v.z, s.attr.local_diff.w);
            if let Some(snapshot) = parent {
                self.inherit(slot, snapshot);
            }
            if use_ring {
                self.ring = slot + 1;
                if self.ring == self.capacity {
                    self.ring = 0;
                }
                if self.count < self.capacity {
                    self.count += 1;
                }
            }
        }
    }

    /// `eft_Emitter_InitParticle` `0x03b6d628` (§3).
    #[allow(clippy::too_many_arguments)]
    fn init_particle(
        &mut self,
        ctx: &Ctx,
        set: &SetState,
        slot: usize,
        e: f32,
        k: u32,
        random: &mut SeadRandom,
        uid: &mut u64,
    ) -> bool {
        let res = self.res.clone();
        let p = &res.params;
        let volume = Volume {
            params: p,
            tables: ctx.tables,
            vectors: ctx.vectors,
            primitive: res.primitive.as_ref(),
            form_scale: self.anim[EASS],
            omni: self.anim[EAOV].x,
        };
        let Some((mut pos, mut vel)) = emit::emit(&volume, &mut self.rng, e, k) else {
            return false;
        };
        let mut attr = ParticleAttr {
            emt_mat: self.world_srt.rows(),
            emt_rt_mat: self.world_rt.rows(),
            ..Default::default()
        };
        self.particle_count = self.particle_count.wrapping_add(1);
        let id = self.particle_count;
        // XZ diffusion.
        if p.xz_diffusion != 0.0 {
            let (mut x, mut z) = (pos.x, pos.z);
            if x * x + z * z <= f32::MIN_POSITIVE {
                let a = self.rng.lcg.f32();
                let b = self.rng.lcg.f32();
                x = (a + a) - 1.0;
                z = (b + b) - 1.0;
            }
            let l2 = x * x + z * z;
            let add = if l2 == 0.0 || !l2.is_finite() {
                Vec3::ZERO
            } else {
                let inv = (1.0 / f64::from(l2).sqrt()) as f32;
                Vec3::new(x * inv, 0.0, z * inv) * p.xz_diffusion
            };
            vel += add;
        }
        // Velocity random factor, directional velocity.
        let r = self.rng.lcg.f32();
        let directional = self.anim[EADV].x * set.params.directional_velocity_scale;
        let k_random = -(f64::from(r * (p.velocity_random / 100.0))
            * f64::from(set.params.velocity_random_scale)
            - 1.0) as f32;
        if p.position_random != 0.0 {
            pos += ctx.vectors.b(&mut self.rng.b) * p.position_random;
        }
        let mut dir = Vec3::from(p.direction);
        if p.world_direction {
            dir = self
                .world_srt
                .with_translation(Vec3::ZERO)
                .inverse()
                .vector(dir);
        }
        let add = if p.diffusion_angle == 0.0 {
            dir
        } else {
            let r1 = self.rng.lcg.f32();
            let c = (1.0 - f64::from(p.diffusion_angle / 90.0)) as f32;
            let (sin, cos) =
                sin_cos_index(ctx.tables, angle_index((r1 + r1) * std::f32::consts::PI));
            let r2 = self.rng.lcg.f32();
            let y = r2 * (1.0 - c) + c;
            let ring = -(y * y - 1.0);
            let s = if 0.0 < ring {
                f64::from(ring).sqrt() as f32
            } else {
                0.0
            };
            emit::rotate_from_y(ctx.tables, dir, Vec3::new(s * cos, y, s * sin))
        };
        vel = Vec3::new(
            (add.x * directional + vel.x) * k_random,
            (add.y * directional + vel.y) * k_random,
            (add.z * directional + vel.z) * k_random,
        );
        vel += ctx.vectors.a(&mut self.rng.a) * Vec3::from(p.velocity_random_axes);
        vel += self.movement * p.emitter_velocity_inherit;
        vel += set.rt.transposed_vector(set.params.add_velocity);
        if p.calc == 2 {
            pos += vel;
        }
        // Scale.
        let easl = self.anim[EASL];
        let es = set.params.emission_scale;
        let reduce =
            |random: f32, r: f32| -((f64::from(random / 100.0) * f64::from(r) - 1.0) as f32);
        let r = self.rng.lcg.f32();
        let scale = if p.scale_random[0] == p.scale_random[1] {
            let f = reduce(p.scale_random[0], r);
            Vec3::new(easl.x * f * es.x, easl.y * f * es.y, easl.z * f * es.z)
        } else {
            let fx = reduce(p.scale_random[0], r);
            let fy = reduce(p.scale_random[1], self.rng.lcg.f32());
            let fz = reduce(p.scale_random[2], self.rng.lcg.f32());
            Vec3::new(easl.x * fx * es.x, easl.y * fy * es.y, easl.z * fz * es.z)
        };
        let r = self.rng.lcg.f32();
        let speed = (p.speed_random * r) as f64 as f32;
        let speed = ((f64::from(p.speed_random) + 1.0) as f32) - (speed + speed);
        attr.scale = scale.extend(speed);
        let birth = self.frame;
        // Life.
        let life = if !p.infinite_life {
            let ri = self.rng.lcg.below(p.life_random);
            let l = self.anim[EAPL].x;
            -(l * ri as f32 * 0.01 - l) * 1.0 * set.params.life_scale
        } else {
            INFINITE_LIFE
        };
        attr.local_pos = pos.extend(life);
        attr.local_vec = vel.extend(birth);
        attr.random = Vec4::new(
            self.rng.lcg.f32(),
            self.rng.lcg.f32(),
            self.rng.lcg.f32(),
            self.rng.lcg.f32(),
        );
        attr.init_rotate = res.init_rotate.extend(0.0);
        attr.color0 = Vec4::ONE;
        attr.color1 = Vec4::ONE;
        // Children: one per child resource (§5); a child's own children
        // are never run by the game, so they are not made.
        let mut children = vec![None; res.children.len()];
        if self.parent.is_none() {
            for (k, child) in res.children.iter().enumerate() {
                let link = ParentParticle {
                    life,
                    age: 0.0,
                    position: Vec3::ZERO,
                    velocity: Vec3::ZERO,
                    scale: attr.scale,
                    init_rotate: attr.init_rotate.truncate(),
                    random: attr.random,
                };
                if let Some(c) = Emitter::new(ctx, child.clone(), set, random, Some(link), *uid) {
                    children[k] = Some(*uid);
                    *uid += 1;
                    self.children[k].push(c);
                }
            }
        }
        let s = &mut self.slots[slot];
        s.id = id;
        s.collisions = 0;
        s.birth = birth;
        s.life = life;
        s.attr = attr;
        s.children = children;
        true
    }

    /// `eft_Emitter_InheritFromParentParticle` `0x03b6eae4`, with the
    /// parent's resource and current values.
    fn inherit(&mut self, slot: usize, snap: &ParentSnapshot) {
        let Some(pp) = self.parent else { return };
        let p = &self.res.params;
        let s = &mut self.slots[slot];
        if p.inherit[0] {
            let v = s.attr.local_vec.truncate() + pp.velocity * p.inherit_velocity_scale;
            s.attr.local_vec = v.extend(s.attr.local_vec.w);
        }
        if p.inherit[1] {
            let scale = snap
                .res
                .particle_scale(pp.life, pp.age, pp.scale, pp.random)
                * p.inherit_scale_scale;
            s.attr.scale = scale.extend(s.attr.scale.w);
        }
        if p.inherit[2] {
            let r = snap
                .res
                .particle_rotation(pp.age, pp.init_rotate, pp.random);
            s.attr.init_rotate = r.extend(s.attr.init_rotate.w);
        }
        if p.inherit[4] || p.inherit[6] {
            let c = snap.res.particle_color(
                false,
                snap.alpha0,
                pp.life,
                pp.age,
                pp.random,
                snap.color0_mul,
                snap.color0,
            );
            if p.inherit[4] {
                s.attr.color0 = c.truncate().extend(s.attr.color0.w);
            }
            if p.inherit[6] {
                s.attr.color0.w = c.w;
            }
        }
        if p.inherit[5] || p.inherit[7] {
            // The library passes the parent's colour 0 animation here too.
            let c = snap.res.particle_color(
                true,
                snap.alpha0,
                pp.life,
                pp.age,
                pp.random,
                snap.color1_mul,
                snap.color0,
            );
            if p.inherit[5] {
                s.attr.color1 = c.truncate().extend(s.attr.color1.w);
            }
            if p.inherit[7] {
                s.attr.color1.w = c.w;
            }
        }
    }

    /// `eft_Emitter_CalcParticlesCpu` `0x03b6ae7c` and
    /// `eft_Particle_CalcCpu` `0x03b5d9b0` (§4.1).
    fn calc_particles_cpu(&mut self, ctx: &Ctx) {
        let res = self.res.clone();
        let p = &res.params;
        let dt = self.step;
        let frame = self.frame;
        let slot0_random = self.slots.first().map_or(0.0, |s| s.attr.random.x);
        let mut live = 0;
        for i in 0..self.count {
            if self.slots[i].id == 0 {
                continue;
            }
            let (age, alive) = {
                let s = &mut self.slots[i];
                let age = frame - s.birth;
                s.attr.local_pos.w = s.life;
                s.attr.local_vec.w = s.birth;
                (age, age < s.life)
            };
            if alive {
                let world_srt = self.world_srt;
                let world_rt = self.world_rt;
                let anim_gravity = self.anim[EAGV].x;
                let s = &mut self.slots[i];
                let old = s.attr.local_pos.truncate();
                if age <= 0.0 {
                    let v = s.attr.local_vec;
                    s.attr.local_diff = Vec4::new(v.x, v.y, v.z, s.attr.local_diff.w);
                }
                let mut pos = old;
                let mut vel = s.attr.local_vec.truncate();
                let k = (f64::from(s.attr.scale.w) * f64::from(dt)) as f32;
                pos = Vec3::new(vel.x * k + pos.x, vel.y * k + pos.y, vel.z * k + pos.z);
                if p.air_resistance < 1.0 {
                    let drag = f64::from(p.air_resistance).powf(f64::from(dt));
                    vel = Vec3::new(
                        (f64::from(vel.x) * drag) as f32,
                        (f64::from(vel.y) * drag) as f32,
                        (f64::from(vel.z) * drag) as f32,
                    );
                }
                if 0.0 < anim_gravity {
                    let g = Vec3::from(p.gravity) * anim_gravity;
                    let add = if p.world_gravity {
                        let rt = if p.follow == 1 {
                            M34(s.attr.emt_rt_mat.map(|r| r.to_array()))
                        } else {
                            world_rt
                        };
                        rt.transposed_vector(g)
                    } else {
                        g
                    };
                    vel = Vec3::new(
                        (f64::from(dt) * f64::from(add.x) + f64::from(vel.x)) as f32,
                        (f64::from(dt) * f64::from(add.y) + f64::from(vel.y)) as f32,
                        (f64::from(dt) * f64::from(add.z) + f64::from(vel.z)) as f32,
                    );
                }
                let mut particle = FieldParticle {
                    pos,
                    vel,
                    life: s.life,
                    pos_w: s.attr.local_pos.w,
                    birth: s.birth,
                    speed: s.attr.scale.w,
                    emt_mat: M34(s.attr.emt_mat.map(|r| r.to_array())),
                    collisions: s.collisions,
                };
                apply_fields(
                    ctx,
                    &res,
                    &mut particle,
                    &mut self.rng,
                    FieldFrame {
                        frame,
                        dt,
                        world_srt,
                        world_rt,
                        slot0_random,
                    },
                );
                s.life = particle.life;
                s.collisions = particle.collisions;
                s.attr.local_pos = particle.pos.extend(particle.pos_w);
                s.attr.local_vec = particle.vel.extend(s.attr.local_vec.w);
                let d = particle.pos - old;
                if 0.01 < d.x.abs() || 0.01 < d.y.abs() || 0.01 < d.z.abs() {
                    s.attr.local_diff = d.extend(s.attr.local_diff.w);
                }
                live += 1;
            } else {
                self.slots[i].id = 0;
            }
            // The particle's children follow it.
            let s = &self.slots[i];
            for (k, child) in s.children.iter().enumerate() {
                let Some(uid) = child else { continue };
                let Some(c) = self.children[k].iter_mut().find(|c| c.uid == *uid) else {
                    continue;
                };
                let Some(link) = c.parent.as_mut() else {
                    continue;
                };
                let local = s.attr.local_pos.truncate();
                link.age = frame - s.attr.local_vec.w;
                link.life = s.attr.local_pos.w;
                link.position = if p.follow == 0 {
                    self.world_srt.point(local)
                } else {
                    M34(s.attr.emt_mat.map(|r| r.to_array())).point(local)
                };
                link.velocity = self.world_rt.vector(s.attr.local_vec.truncate());
            }
        }
        if live == 0 {
            self.ring = 0;
            self.count = 0;
        }
    }

    /// Calc type 2: the stream-out program (`vs_12a043b6effc`) over the
    /// live particles, at the frame and with the matrices the dynamic block
    /// gets this frame. The result replaces the attributes' position and
    /// velocity (the draw shader reads them as `sysInPos`/`sysInVec`); the
    /// birth frame and life stay in their `w`, the program's `pos.w` (life
    /// plus FCSF's light fraction) goes to `local_diff.w`.
    fn calc_stream_out(&mut self, ctx: &Ctx, set: &SetState) {
        let res = self.res.clone();
        let Some(prog) = &res.stream_out else { return };
        let frame = stream_out::Frame {
            time: self.frame,
            step: self.step,
            srt: self.world_srt,
            rt: self.world_rt,
        };
        let curl = ctx.tables.curl_noise.as_slice();
        for s in &mut self.slots[..self.count] {
            if s.id == 0 || self.frame - s.birth >= s.life {
                continue;
            }
            let a = &mut s.attr;
            let q = stream_out::Particle {
                pos: a.local_pos.truncate().extend(a.local_diff.w),
                vel: a.local_vec,
                local_pos: a.local_pos,
                local_vec: a.local_vec,
                scale: a.scale,
                random: a.random,
                emt_mat: M34(a.emt_mat.map(|r| r.to_array())),
                emt_rt_mat: M34(a.emt_rt_mat.map(|r| r.to_array())),
            };
            let (pos, vel) = stream_out::run(prog, &frame, &set.env, curl, &q);
            a.local_pos = pos.truncate().extend(a.local_pos.w);
            a.local_vec = vel.truncate().extend(a.local_vec.w);
            a.local_diff.w = pos.w;
        }
    }

    /// `eft_Emitter_UpdateDynamicUbo` `0x03b6b724` (§7).
    fn update_dynamic(&mut self, set: &SetState, parent: Option<&ParentSnapshot>) {
        let p = &self.res.params;
        let c = set.params.color;
        let [m0, m1] = self.color_mul;
        let mut alpha0 = m0.w * self.anim[EAA0].x;
        let mut alpha1 = m1.w * self.anim[EAA1].x;
        let with_parent0 = p.inherit[6] && p.alpha0_with_parent;
        let with_parent1 = p.inherit[7] && p.alpha1_with_parent;
        if let Some(pp) = parent {
            if with_parent0 {
                alpha0 *= pp.alpha0;
            }
            if with_parent1 {
                alpha1 *= pp.alpha1;
            }
        }
        let mut alpha_fade = if p.fade_in_alpha { self.fade_in } else { 1.0 };
        if p.fade_out_alpha {
            alpha_fade *= self.fade_out;
        }
        alpha_fade *= c.w;
        if let Some(pp) = parent
            && (with_parent0 || with_parent1)
        {
            alpha_fade *= pp.alpha_fade;
        }
        let mut scale_fade = if p.fade_in_scale { self.fade_in } else { 1.0 };
        if p.fade_out_scale {
            scale_fade *= self.fade_out;
        }
        let ec0 = self.anim[EAC0];
        let ec1 = self.anim[EAC1];
        let shader_scale = set.shader_scale() * scale_fade;
        let rows = |m: &M34| {
            let r = m.rows();
            [r[0], r[1], r[2], Vec4::W]
        };
        self.dynamic = EmitterDynamic {
            color0: Vec4::new(
                m0.x * ec0.x * c.x,
                m0.y * ec0.y * c.y,
                m0.z * ec0.z * c.z,
                alpha0,
            ),
            color1: Vec4::new(
                m1.x * ec1.x * c.x,
                m1.y * ec1.y * c.y,
                m1.z * ec1.z * c.z,
                alpha1,
            ),
            frame: Vec4::new(self.frame, 1.0, 1.0, self.step),
            alpha_scale: Vec4::new(alpha_fade, shader_scale.x, shader_scale.y, shader_scale.z),
            srt: rows(&self.world_srt),
            rt: rows(&self.world_rt),
        };
    }

    /// The live particles for drawing, in slot order: those whose age at
    /// the frame the shader is given is below their life.
    fn collect(&mut self, frame: f32) {
        self.out.clear();
        for s in &self.slots[..self.count] {
            if s.id != 0 && frame - s.birth < s.life {
                self.out.push(s.attr);
            }
        }
    }
}

/// A particle as the field functions see it.
struct FieldParticle {
    pos: Vec3,
    vel: Vec3,
    /// `info+0xc` (collision kill shortens it).
    life: f32,
    /// `sysLocalPosAttr.w`.
    pos_w: f32,
    birth: f32,
    speed: f32,
    emt_mat: M34,
    collisions: i32,
}

#[derive(Clone, Copy)]
struct FieldFrame {
    frame: f32,
    dt: f32,
    world_srt: M34,
    world_rt: M34,
    slot0_random: f32,
}

/// The fields in the library's order (`eft_Particle_CalcCpu`): FRND,
/// FRN1, FMAG, FSPN, FCOL, FCOV, FCLN, FPAD (FCSF has no CPU part).
fn apply_fields(
    ctx: &Ctx,
    res: &ResData,
    q: &mut FieldParticle,
    rng: &mut EmitterRng,
    at: FieldFrame,
) {
    let f = &res.fields;
    let p = &res.params;
    let age = at.frame - q.birth;
    let res_life = p.life as f32;
    // SI-EFX-16: FRND (`0x03b73144`, sum-of-sines noise) and FCLN
    // (`eft_Field_CurlNoise` `0x03b69810`) are not ported for CPU
    // emitters; they leave the particle as is.
    let _ = (f.random, f.curl);
    if let Some(r1) = &f.random1 {
        let amp = r1.anim.value(r1.amplitude, age, res_life, at.slot0_random);
        let whole = age as i32 as u32;
        let hit = match whole.checked_div(r1.interval) {
            Some(q) => whole == q * r1.interval,
            None => whole == 0,
        };
        if hit {
            q.vel += ctx.vectors.a(&mut rng.a) * amp;
        }
    }
    // The emitter's position in the particle's space (magnet, convergence).
    let emitter_target = |m: &M34| m.inverse().point(at.world_rt.translation());
    let follow_matrix = if p.follow == 1 {
        q.emt_mat
    } else {
        at.world_srt
    };
    if let Some(m) = &f.magnet {
        let s = m
            .anim
            .value(Vec3::splat(m.strength), age, res_life, at.slot0_random)
            .x;
        let target = if m.follow_emitter {
            emitter_target(&follow_matrix) + m.target
        } else {
            m.target
        };
        for axis in 0..3 {
            if m.axes[axis] {
                q.vel[axis] = ((target[axis] - q.pos[axis]) - q.vel[axis]) * s + q.vel[axis];
            }
        }
    }
    if let Some(s) = &f.spin {
        let to_radians = |v: f32| ((1.0 / 180.0) * v) * std::f32::consts::PI;
        let speed = if s.speed_anim.enabled {
            to_radians(s.speed_anim.eval(age, res_life, at.slot0_random).x)
        } else {
            s.speed
        };
        let outward = if s.outward_anim.enabled {
            to_radians(s.outward_anim.eval(age, res_life, at.slot0_random).x)
        } else {
            s.outward
        };
        // The plane of the turn: about X (y, z), Y (z, x) or Z (x, y).
        let plane = match s.axis {
            0 => Some((1, 2)),
            1 => Some((2, 0)),
            2 => Some((0, 1)),
            _ => None,
        };
        if let Some((a, b)) = plane {
            let (sin, cos) = sin_cos(ctx.tables, (speed * q.speed) * at.dt);
            let u = q.pos[a] * cos + q.pos[b] * sin;
            let v = q.pos[b] * cos - q.pos[a] * sin;
            q.pos[a] = u;
            q.pos[b] = v;
            let l2 = u * u + v * v;
            if outward != 0.0 && 0.0 < l2 {
                let inv = (1.0 / f64::from(l2).sqrt()) as f32;
                let k = (f64::from(inv * outward) * f64::from(q.speed) * f64::from(at.dt)) as f32;
                q.pos[a] = (f64::from(u) * f64::from(k) + f64::from(q.pos[a])) as f32;
                q.pos[b] = (f64::from(v) * f64::from(k) + f64::from(q.pos[b])) as f32;
            }
        }
    }
    fields_tail(ctx, res, q, at, age);
}

/// FCOL, FCOV, FPAD.
fn fields_tail(_ctx: &Ctx, res: &ResData, q: &mut FieldParticle, at: FieldFrame, age: f32) {
    let f = &res.fields;
    let p = &res.params;
    let res_life = p.life as f32;
    if let Some(c) = &f.collision
        && (c.max_count == -1 || q.collisions < c.max_count)
    {
        if !c.world {
            if q.pos.y < c.y {
                q.pos.y = c.y;
                if c.kill == 0 {
                    q.vel.y = -(q.vel.y * c.bounce);
                    q.vel *= c.friction;
                    q.collisions += 1;
                } else if c.kill == 1 {
                    q.pos_w = age;
                    q.life = age;
                }
            }
        } else {
            let m = if p.follow == 0 {
                at.world_srt
            } else {
                q.emt_mat
            };
            let world = m.point(q.pos);
            if world.y < c.y {
                if c.kill == 0 {
                    let mut wv = m.vector(q.vel);
                    wv.y = -(wv.y * c.bounce);
                    let inv = m.inverse();
                    q.pos = inv.point(Vec3::new(world.x, c.y + 0.0001, world.z));
                    q.vel = inv.with_translation(Vec3::ZERO).vector(wv) * c.friction;
                    q.collisions += 1;
                } else if c.kill == 1 {
                    q.pos_w = age;
                    q.life = age;
                }
            }
        }
    }
    if let Some(c) = &f.converge {
        let ratio = c
            .anim
            .value(Vec3::splat(c.ratio), age, res_life, at.slot0_random)
            .x;
        let target = if c.follow_emitter {
            let m = if p.follow == 1 {
                q.emt_mat
            } else {
                at.world_srt
            };
            m.inverse().point(at.world_rt.translation()) + c.target
        } else {
            c.target
        };
        let k = |t: f32, x: f32| {
            (f64::from(((t - x) * ratio) * q.speed) * f64::from(at.dt) + f64::from(x)) as f32
        };
        q.pos = Vec3::new(
            k(target.x, q.pos.x),
            k(target.y, q.pos.y),
            k(target.z, q.pos.z),
        );
    }
    if let Some(a) = &f.pos_add {
        let add = a.anim.value(a.add, age, res_life, at.slot0_random);
        let add = Vec3::new(
            (f64::from(add.x * q.speed) * f64::from(at.dt)) as f32,
            (f64::from(add.y * q.speed) * f64::from(at.dt)) as f32,
            (f64::from(add.z * q.speed) * f64::from(at.dt)) as f32,
        );
        q.pos += if a.world {
            at.world_rt.transposed_vector(add)
        } else {
            add
        };
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// An emitter resource with everything off.
    pub fn params() -> EmitterParams {
        EmitterParams {
            visible: true,
            sort: 0,
            calc: 0,
            follow: 0,
            fade_stops_emission: true,
            fade_out_alpha: false,
            fade_out_scale: false,
            seed_mode: 2,
            rerandomize_matrix: false,
            lod_every_frame: false,
            lod_emission: false,
            fade_in_alpha: false,
            fade_in_scale: false,
            seed: 1,
            draw_path: 7,
            fade_out_frames: 10,
            fade_in_frames: 0,
            translate: [0.0; 3],
            translate_random: [0.0; 3],
            rotate: [0.0; 3],
            rotate_random: [0.0; 3],
            scale: [1.0; 3],
            color0: [1.0; 4],
            color1: [1.0; 4],
            near: 0.0,
            far: -1.0,
            far_percent: 0,
            inherit: [false; 8],
            draw_before_parent: false,
            alpha0_with_parent: false,
            alpha1_with_parent: false,
            inherit_velocity_scale: 1.0,
            inherit_scale_scale: 1.0,
            one_time: false,
            world_gravity: false,
            by_distance: false,
            world_direction: false,
            start: 0,
            child_start_percent: 0,
            duration: 0,
            rate: 1.0,
            rate_random: 0,
            interval: 0,
            interval_random: 0,
            position_random: 0.0,
            gravity_scale: 0.0,
            gravity: [0.0, -1.0, 0.0],
            distance_unit: 1.0,
            distance_min: 0.0,
            distance_max: 0.0,
            distance_threshold: 0.0,
            volume: 0,
            random_start_angle: false,
            latitude_mode: false,
            sphere_table: 0,
            sphere64_count: 0,
            latitude_axis: 2,
            sweep: std::f32::consts::TAU,
            latitude: std::f32::consts::PI,
            sweep_start: 0.0,
            division_angle_random: 0.0,
            caliber: 1.0,
            line_center: 0.0,
            line_length: 1.0,
            volume_radius: [1.0; 3],
            form_scale: [1.0; 3],
            division_mode: -1,
            shape_primitive: None,
            circle_divisions: 1,
            circle_division_random: 0,
            line_divisions: 1,
            line_division_random: 0,
            blend: true,
            depth_test: true,
            depth_func: 3,
            depth_write: false,
            alpha_test: false,
            alpha_func: 0,
            blend_type: 0,
            cull: 0,
            alpha_ref: 0.0,
            infinite_life: false,
            billboard: 0,
            life: 10,
            life_random: 0,
            speed_random: 0.0,
            primitive: None,
            omni_velocity: 0.0,
            directional_velocity: 0.0,
            direction: [0.0, 1.0, 0.0],
            diffusion_angle: 0.0,
            xz_diffusion: 0.0,
            velocity_random_axes: [0.0; 3],
            velocity_random: 0.0,
            emitter_velocity_inherit: 0.0,
            particle_scale: [1.0; 3],
            scale_random: [0.0; 3],
            air_resistance: 1.0,
            color_sources: [0; 4],
            ..Default::default()
        }
    }

    /// sead's sine table, computed.
    pub fn tables() -> Arc<EffectTables> {
        let step = std::f64::consts::TAU / 256.0;
        let sin_cos = (0..256)
            .map(|i| {
                let a = i as f64 * step;
                let b = a + step;
                [
                    a.sin() as f32,
                    (b.sin() - a.sin()) as f32,
                    a.cos() as f32,
                    (b.cos() - a.cos()) as f32,
                ]
            })
            .collect();
        Arc::new(EffectTables {
            sin_cos,
            sphere: Vec::new(),
            sphere64: Vec::new(),
            arc_epsilon: f32::from_bits(0x3480_0000),
            curl_noise: Vec::new(),
        })
    }

    /// A file with one set of one emitter.
    fn data(params: EmitterParams) -> Arc<EffectData> {
        let emitter = fx::Emitter {
            name: "test".into(),
            resource: fx::Block {
                offset: 0,
                size: 0xa88,
            },
            static_block: fx::Block {
                offset: 0xa88,
                size: 0x750,
            },
            field_block: None,
            attributes: Vec::new(),
            animations: Vec::new(),
            children: Vec::new(),
            params,
            samplers: [None, None, None],
        };
        Arc::new(EffectData {
            file: fx::EffectFile {
                name: "test".into(),
                sets: vec![fx::EmitterSet {
                    name: "set".into(),
                    emitters: vec![emitter],
                }],
                textures: Vec::new(),
                primitives: Vec::new(),
            },
            res: vec![0; 0xa88 + 0x750],
            images: Default::default(),
            swizzles: Default::default(),
        })
    }

    fn sim(params: EmitterParams) -> EmitterSetSim {
        EmitterSetSim::new(
            data(params),
            0,
            Affine3A::IDENTITY,
            SetParams::default(),
            tables(),
            &mut SeadRandom::new(7),
            0x1234_5678,
        )
    }

    fn emitted(sim: &EmitterSetSim) -> u32 {
        sim.emitters[0].particle_count
    }

    #[test]
    fn interval_skips_frames() {
        let mut p = params();
        p.interval = 2;
        let mut sim = sim(p);
        let mut at = Vec::new();
        for frame in 0..10 {
            let before = emitted(&sim);
            sim.step(1.0, Vec3::ZERO);
            if emitted(&sim) != before {
                at.push(frame);
            }
        }
        assert_eq!(at, vec![0, 3, 6, 9]);
    }

    #[test]
    fn fractional_rates_accumulate() {
        let mut p = params();
        p.rate = 0.5;
        let mut sim = sim(p);
        let mut at = Vec::new();
        for frame in 0..10 {
            let before = emitted(&sim);
            sim.step(1.0, Vec3::ZERO);
            if emitted(&sim) != before {
                at.push(frame);
            }
        }
        // A frame that emits nothing restarts the interval count, so half a
        // particle per frame comes out every third frame.
        assert_eq!(at, vec![1, 4, 7]);
        let mut p = params();
        p.rate = 2.5;
        let mut sim = super::tests::sim(p);
        for _ in 0..4 {
            sim.step(1.0, Vec3::ZERO);
        }
        assert_eq!(emitted(&sim), 10);
    }

    #[test]
    fn life_random_is_a_whole_percentage_reduction() {
        let mut p = params();
        p.life = 100;
        p.life_random = 50;
        p.rate = 4.0;
        let mut sim = sim(p);
        sim.step(1.0, Vec3::ZERO);
        let lives: Vec<f32> = sim.emitters[0].out.iter().map(|a| a.local_pos.w).collect();
        assert_eq!(lives.len(), 4);
        for life in lives {
            assert!((51.0..=100.0).contains(&life), "{life}");
            assert_eq!(life.fract(), 0.0);
        }
        // Deterministic for a fixed seed.
        let mut again = super::tests::sim(params_with(|p| {
            p.life = 100;
            p.life_random = 50;
            p.rate = 4.0;
        }));
        again.step(1.0, Vec3::ZERO);
        assert_eq!(again.emitters[0].out, sim.emitters[0].out);
    }

    fn params_with(f: impl FnOnce(&mut EmitterParams)) -> EmitterParams {
        let mut p = params();
        f(&mut p);
        p
    }

    #[test]
    fn cpu_particles_step_position_before_velocity() {
        let mut sim = sim(params_with(|p| {
            p.one_time = true;
            p.duration = 1;
            p.directional_velocity = 1.0;
            p.air_resistance = 0.5;
            p.life = 100;
        }));
        let mut ys = Vec::new();
        for _ in 0..4 {
            sim.step(1.0, Vec3::ZERO);
            ys.push(sim.emitters[0].out[0].local_pos.y);
        }
        assert_eq!(ys, vec![1.0, 1.5, 1.75, 1.875]);
        let v = sim.emitters[0].out[0].local_vec;
        assert_eq!((v.y, v.w), (0.0625, 0.0));
        // Gravity adds after the position step.
        let mut sim = super::tests::sim(params_with(|p| {
            p.one_time = true;
            p.duration = 1;
            p.gravity_scale = 0.5;
            p.gravity = [0.0, -2.0, 0.0];
            p.life = 100;
        }));
        let mut ys = Vec::new();
        for _ in 0..3 {
            sim.step(1.0, Vec3::ZERO);
            ys.push(sim.emitters[0].out[0].local_pos.y);
        }
        assert_eq!(ys, vec![0.0, -1.0, -3.0]);
    }

    #[test]
    fn stream_out_particles_integrate_each_frame() {
        let mut data = data(params_with(|p| {
            p.calc = 2;
            p.one_time = true;
            p.duration = 1;
            p.directional_velocity = 1.0;
            p.life = 100;
        }));
        // Static block (little-endian): follow type 0, gravity (0, −1, 0)
        // × 0.5, no air resistance.
        let block = &mut Arc::get_mut(&mut data).unwrap().res[0xa88..];
        let mut put = |at: usize, bits: u32| block[at..at + 4].copy_from_slice(&bits.to_le_bytes());
        put(0x54, 0x200);
        put(0xb4, (-1.0f32).to_bits());
        put(0xbc, 0.5f32.to_bits());
        put(0xc0, 1.0f32.to_bits());
        let mut sim = EmitterSetSim::new(
            data,
            0,
            Affine3A::IDENTITY,
            SetParams::default(),
            tables(),
            &mut SeadRandom::new(7),
            0x1234_5678,
        );
        let mut out = Vec::new();
        for _ in 0..4 {
            sim.step(1.0, Vec3::ZERO);
            let a = sim.emitters[0].out[0];
            out.push((a.local_pos, a.local_vec));
        }
        // Emission puts the particle at p + v = 1; each frame then moves it
        // by v and adds gravity. Life and birth frame stay in w.
        let ys: Vec<(f32, f32)> = out.iter().map(|(p, v)| (p.y, v.y)).collect();
        assert_eq!(ys, vec![(2.0, 0.5), (2.5, 0.0), (2.5, -0.5), (2.0, -1.0)]);
        assert!(out.iter().all(|(p, v)| p.w == 100.0 && v.w == 0.0));
    }

    #[test]
    fn particles_die_at_their_life() {
        let mut sim = sim(params_with(|p| {
            p.one_time = true;
            p.duration = 1;
            p.life = 3;
        }));
        let mut counts = Vec::new();
        for _ in 0..6 {
            sim.step(1.0, Vec3::ZERO);
            counts.push(sim.emitters.first().map_or(0, |e| e.out.len()));
        }
        assert_eq!(counts, vec![1, 1, 1, 0, 0, 0]);
        // One-time: gone after start + duration + life.
        assert!(!sim.alive());
    }

    #[test]
    fn gpu_particles_take_ring_slots() {
        let mut sim = sim(params_with(|p| {
            p.calc = 1;
            p.life = 3;
        }));
        // ceil(3 / 1)·1 + ceil(1).
        assert_eq!(sim.emitters[0].capacity, 4);
        for _ in 0..6 {
            sim.step(1.0, Vec3::ZERO);
        }
        let e = &sim.emitters[0];
        assert_eq!((e.count, e.ring), (4, 2));
        assert_eq!(e.out.len(), 3);
    }

    #[test]
    fn fades_scale_the_alpha_and_end_the_emitter() {
        let mut sim = sim(params_with(|p| {
            p.fade_in_alpha = true;
            p.fade_in_frames = 4;
            p.fade_out_alpha = true;
            p.fade_out_frames = 4;
            p.life = 100;
        }));
        let mut alphas = Vec::new();
        for _ in 0..5 {
            sim.step(1.0, Vec3::ZERO);
            alphas.push(sim.views()[0].dynamic.alpha_scale.x);
        }
        assert_eq!(alphas, vec![0.25, 0.5, 0.75, 1.0, 1.0]);
        sim.fade();
        let mut alphas = Vec::new();
        for _ in 0..3 {
            sim.step(1.0, Vec3::ZERO);
            alphas.push(sim.views()[0].dynamic.alpha_scale.x);
        }
        assert_eq!(alphas, vec![0.75, 0.5, 0.25]);
        sim.step(1.0, Vec3::ZERO);
        sim.step(1.0, Vec3::ZERO);
        assert!(!sim.alive());
    }

    /// Every baked set for 300 frames: no panics, finite output.
    /// `cargo test --release -p render baked_sets -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn baked_sets_run() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let tables: EffectTables =
            asset_format::read_ron(&assets.join(fx::TABLES)).expect("tables.ron");
        let tables = Arc::new(tables);
        let index: fx::EffectIndex =
            asset_format::read_ron(&assets.join(asset_format::paths::EFFECT_INDEX)).unwrap();
        let mut random = SeadRandom::new(1);
        let (mut sets, mut particles, mut emitters) = (0, 0usize, 0usize);
        let (mut peak, mut peak_emitters, mut peak_visible) = (0usize, 0usize, 0usize);
        for name in &index.files {
            let file: fx::EffectFile =
                asset_format::read_ron(&assets.join(fx::file_path(name))).unwrap();
            let res = asset_format::read(&assets.join(fx::resource_path(name))).unwrap();
            let data = Arc::new(EffectData {
                file,
                res,
                images: Default::default(),
                swizzles: Default::default(),
            });
            for set in 0..data.file.sets.len() {
                let set_random = random.next();
                let mut sim = EmitterSetSim::new(
                    data.clone(),
                    set,
                    Affine3A::from_translation(Vec3::new(0.0, 0.0, -10.0)),
                    SetParams::default(),
                    tables.clone(),
                    &mut random,
                    set_random,
                );
                sets += 1;
                for frame in 0..300 {
                    if frame == 200 {
                        sim.fade();
                    }
                    if frame == 199 {
                        for v in sim.views() {
                            peak_emitters += 1;
                            peak_visible += usize::from(v.visible);
                            peak += v.particles.len();
                        }
                    }
                    sim.step(1.0, Vec3::ZERO);
                    for v in sim.views() {
                        for a in v.particles {
                            let ok = a.local_pos.is_finite()
                                && a.local_vec.is_finite()
                                && a.scale.is_finite();
                            assert!(ok, "{name}/{}: {:?}", data.file.sets[set].name, a);
                        }
                        assert!(v.dynamic.alpha_scale.is_finite());
                    }
                }
                for v in sim.views() {
                    emitters += 1;
                    particles += v.particles.len();
                }
            }
        }
        println!(
            "{sets} sets; frame 199: {peak_emitters} emitters ({peak_visible} visible), \
             {peak} particles; after the fade: {emitters} emitters, {particles} particles"
        );
    }

    #[test]
    fn anim8_tracks() {
        let mut keys = [Vec4::ZERO; 8];
        keys[0] = Vec4::new(0.0, 0.0, 0.0, 0.0);
        keys[1] = Vec4::new(1.0, 2.0, 4.0, 0.5);
        keys[2] = Vec4::new(1.0, 1.0, 1.0, 1.0);
        assert_eq!(anim8(&keys, 0, 0.0, 0.0, 0.0, 0.0, 10.0), Vec3::ONE);
        assert_eq!(
            anim8(&keys, 3, 0.0, 2.5, 0.0, 0.0, 10.0),
            Vec3::new(0.5, 1.0, 2.0)
        );
        assert_eq!(anim8(&keys, 3, 0.0, 10.0, 0.0, 0.0, 10.0), Vec3::ONE);
        // Looping with a period of 4 frames: age 6 is half way.
        assert_eq!(
            anim8(&keys, 3, 0.0, 6.0, 4.0, 0.0, 10.0),
            Vec3::new(1.0, 2.0, 4.0)
        );
    }
}
