//! GPU calc type 2: the effect library's stream-out program (GameResident
//! VS 956, `vs_12a043b6effc`) run on the CPU, one call per particle and
//! frame. The game runs it on the GPU before each draw and reads the two
//! stream-out buffers back as `sysInPos`/`sysInVec`; here its result goes
//! straight into the particle's attributes.
//!
//! The steps, in the program's order (docs/research/eft-shaders.md §6, the
//! field block layout eft-runtime.md §7, `crates/bake/src/eft.rs`
//! `field_block`):
//!
//! 1. `p' = p + Δ·v`, `v' = v·r^Δ + Δ·g` (world gravity: `g = RTᵀ·g`);
//! 2. the eft fields the flag word B (static 0x054) enables: FRND (0x2),
//!    FRN1 (0x100), FPAD (0x4), FMAG (0x8), FCOV (0x10), FSPN (0x20),
//!    FCLN (0x80), FCOL (0x40);
//! 3. BotW's custom field FCSF (static 0x05C, the node's first word):
//!    wind (0x1), screen-depth collision (0x4), light from the shadow map
//!    kept in the fraction of `pos.w` (0x2).
//!
//! Every constant and the order of every sum are the program's.

use std::sync::Arc;

use asset_format::effects::CURL_NOISE_SIZE;
use bevy::math::{Vec3, Vec4};

use super::M34;

/// `sysEmitterFieldUniformBlock` in words (0x120 bytes).
pub(super) const FIELD_WORDS: usize = 0x120 / 4;

/// An emitter's constants the program reads.
#[derive(Clone, Debug)]
pub(super) struct Program {
    /// Static 0x050 (flags A), 0x054 (flags B), 0x05C (FCSF's first word).
    pub(super) flags_a: u32,
    pub(super) flags_b: u32,
    pub(super) custom: u32,
    /// Static 0x0B0 (gravity), 0x0BC (its scale), 0x0C0 (air resistance).
    pub(super) gravity: Vec3,
    pub(super) gravity_scale: f32,
    pub(super) air: f32,
    /// The field block, by word.
    pub(super) field: [f32; FIELD_WORDS],
}

impl Program {
    /// Field block word at byte offset `at`.
    fn f(&self, at: usize) -> f32 {
        self.field[at / 4]
    }

    fn f3(&self, at: usize) -> Vec3 {
        Vec3::new(self.f(at), self.f(at + 4), self.f(at + 8))
    }
}

/// What BotW's `cus1` block and the scene give the program
/// (docs/research/eft-custom-blocks.md §4.3, §4.4).
#[derive(Clone, Default)]
pub struct StreamOutEnv {
    /// `cus1[0x02C]`: T(smoothed wind speed).
    pub wind_speed: f32,
    /// `cus1[0x018]`: T(sheltered smoothed wind speed).
    pub sheltered_wind_speed: f32,
    /// `cus1[0x020…0x028]`: wind direction (y is not read).
    pub wind_direction: Vec3,
    /// `cus1[0x240]`, `cus1[0x248]`: the drift vector x, z.
    pub drift: [f32; 2],
    /// The ground's height at world (x, z), for FCSF's collision.
    // SI-EFX-31: the program collides with the scene's depth buffer
    // (`sysCustomShaderTextureSampler0`); the CPU has the terrain only.
    pub ground: Option<Arc<dyn Fn(f32, f32) -> Option<f32> + Send + Sync>>,
}

/// The frame: `sysEmitterDynamicUniformBlock` 0x020, 0x02C, 0x040, 0x080.
#[derive(Clone, Copy, Debug)]
pub(super) struct Frame {
    pub(super) time: f32,
    pub(super) step: f32,
    pub(super) srt: M34,
    pub(super) rt: M34,
}

/// One particle's attributes.
#[derive(Clone, Copy, Debug)]
pub(super) struct Particle {
    /// `sysInPos`, `sysInVec`: the last output.
    pub(super) pos: Vec4,
    pub(super) vel: Vec4,
    /// `sysLocalPosAttr` (birth position, life), `sysLocalVecAttr`
    /// (initial velocity, birth frame).
    pub(super) local_pos: Vec4,
    pub(super) local_vec: Vec4,
    /// `sysScaleAttr`, `sysRandomAttr`.
    pub(super) scale: Vec4,
    pub(super) random: Vec4,
    /// `sysEmtMat*`, `sysEmtRTMat*`.
    pub(super) emt_mat: M34,
    pub(super) emt_rt_mat: M34,
}

/// GLSL `fract`.
fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// The program's sine: the argument reduced to [−π, π) first.
fn sin_r(x: f32) -> f32 {
    let r = (fract(x * 0.159_154_9 + 0.5) * 6.283_185 - 3.141_593) * 0.159_154_9;
    (r / 0.159_154_94).sin()
}

fn cos_r(x: f32) -> f32 {
    let r = (fract(x * 0.159_154_9 + 0.5) * 6.283_185 - 3.141_593) * 0.159_154_9;
    (r / 0.159_154_94).cos()
}

fn sat(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// `p·m` for the 3×3 part: the transposed matrix times `v`.
fn transposed(m: &M34, v: Vec3) -> Vec3 {
    m.transposed_vector(v)
}

/// The inverse of the 3×3 part times `v` (the program inverts the
/// matrix with a zero translation).
fn inverse_vector(m: &M34, v: Vec3) -> Vec3 {
    m.with_translation(Vec3::ZERO).inverse().vector(v)
}

/// A texel of the curl-noise texture (`0x03b69644`): RGBA8 with
/// `R = b2 + 0x80, G = b1 + 0x80, B = b0 + 0x80` of the table's triple.
fn curl_texel(table: &[u8], x: usize, y: usize, z: usize) -> Vec3 {
    let n = CURL_NOISE_SIZE;
    let o = ((z * n + y) * n + x) * 3;
    let c = |b: u8| f32::from(b.wrapping_add(0x80)) / 255.0;
    Vec3::new(c(table[o + 2]), c(table[o + 1]), c(table[o]))
}

/// `texture(sysCurlNoiseTextureArray, uvw).xyz`: a 32³ volume, linear
/// filtering, repeat on all axes (the renderer's sampler `+0x24`,
/// `0x03b6adf0`).
// SI-EFX-30: the sampler leaves the Z filter at GX2InitSampler's default,
// not traced; filtered linearly in z as in x and y.
pub(super) fn curl(table: &[u8], uvw: Vec3) -> Vec3 {
    let n = CURL_NOISE_SIZE;
    if table.len() < n * n * n * 3 {
        // Tables baked without the curl noise: the neutral value.
        return Vec3::splat(0.5);
    }
    let axis = |u: f32| {
        let s = u * n as f32 - 0.5;
        let i = s.floor();
        let f = s - i;
        let i0 = (i as i64).rem_euclid(n as i64) as usize;
        (i0, (i0 + 1) % n, f)
    };
    let (x0, x1, fx) = axis(uvw.x);
    let (y0, y1, fy) = axis(uvw.y);
    let (z0, z1, fz) = axis(uvw.z);
    let lerp = |a: Vec3, b: Vec3, t: f32| a + (b - a) * t;
    let plane = |z: usize| {
        lerp(
            lerp(
                curl_texel(table, x0, y0, z),
                curl_texel(table, x1, y0, z),
                fx,
            ),
            lerp(
                curl_texel(table, x0, y1, z),
                curl_texel(table, x1, y1, z),
                fx,
            ),
            fy,
        )
    };
    lerp(plane(z0), plane(z1), fz)
}

/// Flags B: the follow type's matrix choice.
const FOLLOW_CURRENT: u32 = 0x200;
const FOLLOW_TRANSLATION: u32 = 0x400;
const FOLLOW_NONE: u32 = 0x800;

/// The sum of four sines of the random field.
fn waves(prog: &Program, amps: [f32; 4], periods: [f32; 4], s: f32) -> f32 {
    let scale = prog.f(0x0c);
    let mut acc = 0.0;
    for k in 0..4 {
        acc = amps[k] * sin_r(s * (1.0 / (scale * periods[k])) * 6.283_184) + acc;
    }
    acc
}

/// One step of the program: the new `sysInPos`, `sysInVec`.
pub(super) fn run(
    prog: &Program,
    frame: &Frame,
    env: &StreamOutEnv,
    curl_table: &[u8],
    q: &Particle,
) -> (Vec4, Vec4) {
    let dt = frame.step;
    let t = frame.time;
    let flags = prog.flags_b;
    let w = q.scale.w;
    let rnd = q.random;
    let (rx, ry, rz) = (rnd.x - 0.5, rnd.y - 0.5, rnd.z - 0.5);
    let since_birth = t - q.local_vec.w;
    let age = since_birth.max(0.0);
    let first = 0.001 >= age;
    let (p_in, v_in) = if first {
        (q.local_pos, q.local_vec)
    } else {
        (q.pos, q.vel)
    };
    let srt = frame.srt;
    let srt_translation = srt.translation();
    // The rotation the fields work in: the particle's at birth for follow
    // types 1 and 2, else the current one.
    let rt3 = if flags & (FOLLOW_NONE | FOLLOW_TRANSLATION) != 0 {
        q.emt_rt_mat
    } else {
        frame.rt
    };
    let mut p = Vec3::new(
        p_in.x + dt * v_in.x,
        p_in.y + dt * v_in.y,
        p_in.z + dt * v_in.z,
    );
    let pw = p_in.w;
    let mut g = prog.gravity * prog.gravity_scale;
    if prog.flags_a & 0x8 != 0 {
        g = transposed(&rt3, g);
    }
    let drag = (dt * prog.air.max(0.0).log2()).exp2();
    let mut v = Vec3::new(
        v_in.x * drag + dt * g.x,
        v_in.y * drag + dt * g.y,
        v_in.z * drag + dt * g.z,
    );
    // Where the emitter's point is in the particle's space (FMAG, FCOV
    // with "follow the emitter").
    let emitter_target = || {
        if flags & FOLLOW_NONE != 0 {
            q.emt_mat.inverse().point(srt_translation)
        } else {
            Vec3::ZERO
        }
    };
    // The particle's world position.
    let world_point = |p: Vec3| -> Option<Vec3> {
        if flags & FOLLOW_CURRENT != 0 {
            Some(srt.point(p))
        } else if flags & FOLLOW_TRANSLATION != 0 {
            Some(q.emt_mat.with_translation(srt_translation).point(p))
        } else if flags & FOLLOW_NONE != 0 {
            Some(q.emt_mat.point(p))
        } else {
            None
        }
    };

    // FRND (0x2): position moved by the change of four sine waves.
    if flags & 0x2 != 0 {
        let r1 = prog.air + (1.0 - dt) * (1.0 - prog.air);
        let time = if prog.f(0x3c) == 1.0 && r1 != 1.0 {
            (-(age * r1.max(0.0).log2()).exp2() + 1.0) * (1.0 / (1.0 - r1))
        } else {
            age
        };
        let unified = prog.f(0x30) == 1.0;
        let scale = prog.f(0x0c);
        let drift = scale * (prog.f(0x40) * 0.01);
        let mut phase = if unified {
            Vec3::new(0.0, 0.31415, 0.92653) * scale
        } else {
            Vec3::new(rnd.x, rnd.y, rnd.z) * scale
        };
        if unified {
            let spread = scale * (prog.f(0x44) * 0.01);
            phase.x += spread * (rx * 2.0) + t * drift;
            phase.y += t * drift + spread * (ry * 2.0);
            phase.z += t * drift + spread * (rz * 2.0);
        }
        let shift = if unified { dt * drift } else { 0.0 };
        let (amps, periods) = if prog.f(0x34) == 1.0 {
            (
                [prog.f(0x20), prog.f(0x24), prog.f(0x28), prog.f(0x2c)],
                [prog.f(0x10), prog.f(0x14), prog.f(0x18), prog.f(0x1c)],
            )
        } else {
            ([4.0, 3.0, 2.0, 1.5], [0.6, 0.42, 0.23, 0.15])
        };
        let wave = |s: f32| waves(prog, amps, periods, s);
        let delta = |phase: f32| {
            let s0 = time + phase;
            let s1 = shift + (dt + s0);
            if shift == 0.0 {
                -wave(s0) + wave(s1)
            } else {
                let base = -wave(phase) + wave(phase + shift);
                -base + (-wave(s0) + wave(s1))
            }
        };
        let d = Vec3::new(delta(phase.x), delta(phase.y), delta(phase.z));
        p += prog.f3(0x00) * d;
    }
    // FRN1 (0x100): a velocity kick every `interval` frames of age.
    if flags & 0x100 != 0 {
        let whole = age as i32;
        let interval = prog.f(0x5c) as i32;
        let hit = if interval == 0 {
            whole == 0
        } else {
            whole % interval.abs() == 0
        };
        if hit {
            let a2 = age * age;
            v += Vec3::new(
                prog.f(0x50) * sin_r(rz * 6.283_184 + rnd.z * a2),
                prog.f(0x54) * sin_r(rx * 6.283_184 + rnd.y * a2),
                prog.f(0x58) * sin_r(ry * 6.283_184 + rnd.x * a2),
            );
        }
    }
    // FPAD (0x4): a position step, in the emitter's or the world's axes.
    if flags & 0x4 != 0 {
        let mut add = Vec3::new(
            dt * (w * prog.f(0x60)),
            dt * (w * prog.f(0x64)),
            dt * (w * prog.f(0x68)),
        );
        if prog.f(0x6c) as i32 == 1 {
            add = if flags & FOLLOW_CURRENT != 0 {
                transposed(&srt, add)
            } else {
                transposed(&rt3, add)
            };
        }
        p += add;
    }
    // FMAG (0x8): velocity pulled towards a point.
    if flags & 0x8 != 0 {
        let base = if prog.f(0x80) as i32 != 0 {
            emitter_target()
        } else {
            Vec3::ZERO
        };
        let target = base + prog.f3(0x70);
        let k = prog.f(0x7c);
        v += (-v + (-p + target)) * k * w * dt;
    }
    // FCOV (0x10): position pulled towards a point.
    if flags & 0x10 != 0 {
        let base = if prog.f(0xa0) as i32 > 0 {
            emitter_target()
        } else {
            Vec3::ZERO
        };
        let target = base + prog.f3(0x90);
        let k = prog.f(0x9c);
        p += (-p + target) * k * w * dt;
    }
    // FSPN (0x20): a turn about an axis, then an outward push.
    if flags & 0x20 != 0 {
        let axis = prog.f(0xb8) as i32;
        let angle = dt * (w * prog.f(0xb0));
        let outward = prog.f(0xb4);
        let (s, c) = (sin_r(angle), cos_r(angle));
        // The plane of the turn: (a, b) → (a·c − b·s, a·s + b·c).
        let plane = match axis {
            0 => Some((2, 1)),
            1 => Some((0, 2)),
            2 => Some((1, 0)),
            _ => None,
        };
        if let Some((a, b)) = plane {
            let u = p[a] * c + p[b] * -s;
            let k = p[a] * s + p[b] * c;
            p[a] = u;
            p[b] = k;
            if outward != 0.0 {
                let l2 = u * u + k * k;
                if l2 > 0.0 {
                    let push = dt * (w * (outward * (1.0 / l2.sqrt())));
                    p[a] += u * push;
                    p[b] += k * push;
                }
            }
        }
    }
    // FCLN (0x80): velocity from the curl-noise volume.
    if flags & 0x80 != 0 {
        let world = prog.f(0xf4) > 0.0;
        let at = if world {
            world_point(p).unwrap_or(p)
        } else {
            p
        };
        let offset = if prog.f(0xf0) != 0.0 {
            rnd.x * prog.f(0xec)
        } else {
            prog.f(0xec)
        };
        let speed = prog.f3(0xe0);
        let scale = prog.f(0xdc);
        let c = Vec3::new(
            (t * speed.x + at.x * scale) + offset,
            (t * speed.y + at.y * scale) + offset,
            (t * speed.z + at.z * scale) + offset,
        );
        let n = curl(curl_table, c.abs() * 0.031_25);
        let mut add = prog.f3(0xd0) * ((n * 2.0 - 1.0) / 2.0);
        if world {
            if flags & FOLLOW_CURRENT != 0 {
                add = inverse_vector(&srt, add);
            } else if flags & (FOLLOW_TRANSLATION | FOLLOW_NONE) != 0 {
                add = inverse_vector(&q.emt_mat, add);
            }
        }
        v += add * dt;
    }
    // FCOL (0x40): a bounce off a plane y = const (local or world). The
    // program has no kill (types 2, 3 do nothing).
    let mut out = p.extend(pw);
    if flags & 0x40 != 0 {
        let mode = prog.f(0xc0) as i32;
        let plane = prog.f(0xcc);
        let bounce = prog.f(0xc4);
        let friction = prog.f(0xc8);
        let local_hit = mode == 0 && plane > p.y;
        let world_y = world_point(p).map_or(p.y, |q| q.y);
        let world_hit = mode == 1 && plane > world_y;
        let y = if local_hit { plane } else { p.y };
        let vx = if local_hit { v.x * friction } else { v.x };
        let vz = if local_hit { v.z * friction } else { v.z };
        let vy = if local_hit {
            (v.y * -bounce) * friction
        } else {
            v.y
        };
        if world_hit {
            out.y = y + (-world_y + plane);
            v = Vec3::new(friction * vx, friction * (-bounce * vy), friction * vz);
        } else {
            out.y = y;
            v = Vec3::new(vx, vy, vz);
        }
    }

    let custom = prog.custom;
    let column_scale = Vec3::new(srt.0[0][1], srt.0[1][1], srt.0[2][1]).length();
    // FCSF 0x1: wind, growing over `[0x114] + 1` frames of age.
    if custom & 0x1 != 0 {
        let grow = sat(since_birth * (1.0 / (prog.f(0x114) + 1.0)));
        let s = if custom & 0x8 == 0 {
            env.wind_speed
        } else {
            env.sheltered_wind_speed
        };
        let k = (if 8.0 > s {
            ((s * 0.125).max(0.0).log2() * 1.5).exp2()
        } else {
            1.0
        }) / 2.0
            * grow;
        let dir = Vec3::new(env.wind_direction.x, 0.0, env.wind_direction.z);
        let mut d = if flags & FOLLOW_CURRENT != 0 {
            transposed(&srt, dir)
        } else {
            transposed(&rt3, dir)
        };
        d *= if flags & FOLLOW_CURRENT == 0 {
            1.0 / column_scale
        } else {
            1.0 / (column_scale * column_scale)
        };
        if custom & 0x10 != 0 {
            let n = curl(curl_table, out.truncate() * 0.03);
            d += (n * 2.0 - 1.0) * 0.3;
        }
        let r = -(rnd.x * prog.f(0x108)) + 1.0;
        let dp = d * (prog.f(0x100) * s) * k * r * dt;
        let dv = d * (prog.f(0x104) * s) * k * r * dt;
        if custom & 0x40 != 0 {
            v.x = (prog.f(0x118) * -0.1) * env.drift[0];
            v.z = (prog.f(0x118) * -0.1) * env.drift[1];
        }
        out += dp.extend(0.0);
        v += dv;
    }
    // FCSF 0x4: collision with what is drawn, pushed out along its normal.
    if custom & 0x4 != 0
        && let (Some(ground), Some(at)) = (&env.ground, world_point(out.truncate()))
        && let Some(h) = ground(at.x, at.z)
    {
        // SI-EFX-31: the depth along the view ray and the normal of the
        // depth buffer are the terrain's height difference and normal.
        let depth = h - at.y;
        if depth > 0.0 && 1.0 > depth {
            let e = 0.5;
            let hx = |x: f32| ground(x, at.z).unwrap_or(h);
            let hz = |z: f32| ground(at.x, z).unwrap_or(h);
            let normal = Vec3::new(
                hx(at.x - e) - hx(at.x + e),
                2.0 * e,
                hz(at.z - e) - hz(at.z + e),
            )
            .normalize();
            let local = if flags & FOLLOW_CURRENT != 0 {
                transposed(&srt, normal)
            } else {
                transposed(&rt3, normal)
            };
            let n = local.normalize();
            if custom & 0x20 != 0 {
                out.y += prog.f(0x10c) * n.y;
                v.y += prog.f(0x110) * n.y;
            } else {
                let inv = 1.0 / column_scale;
                out.y += ((depth * n.y) / 2.0) * inv;
                out.z += ((depth * n.z) / 2.0) * inv;
                out.x += ((depth * n.x) / 2.0) * inv;
                let vn = v.dot(n);
                v = (v + -n * vn) * prog.f(0x110) + (-n * vn) * prog.f(0x10c);
            }
        }
    }
    // FCSF 0x2: the light from the shadow map, smoothed over frames in
    // the fraction of pos.w.
    if custom & 0x2 != 0 {
        // SI-EFX-32: the shadow-map compare (`ShadowArraySampler0`) and
        // `TextureSampler1` are not sampled on the CPU; taken as no shadow
        // term (`tex1·cus1[0x014]` = 0).
        let shade = 0.0;
        let target = 1.0 - shade;
        let whole = out.w as i32;
        let frac = fract(out.w);
        let next = if first {
            target
        } else {
            frac + (target - frac) * 0.05
        };
        let clamped = if 0.0 > next { 0.0 } else { next };
        let clamped = if clamped > 0.999 { 0.999 } else { clamped };
        out.w = clamped + whole as f32;
    }
    (out, v.extend(age))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program() -> Program {
        Program {
            flags_a: 0,
            flags_b: FOLLOW_CURRENT,
            custom: 0,
            gravity: Vec3::new(0.0, -1.0, 0.0),
            gravity_scale: 0.5,
            air: 1.0,
            field: [0.0; FIELD_WORDS],
        }
    }

    fn frame(time: f32) -> Frame {
        Frame {
            time,
            step: 1.0,
            srt: M34::IDENTITY,
            rt: M34::IDENTITY,
        }
    }

    fn particle(v: Vec3) -> Particle {
        Particle {
            pos: Vec4::ZERO,
            vel: Vec4::ZERO,
            local_pos: Vec4::new(0.0, 0.0, 0.0, 30.0),
            local_vec: v.extend(0.0),
            scale: Vec4::ONE,
            random: Vec4::splat(0.25),
            emt_mat: M34::IDENTITY,
            emt_rt_mat: M34::IDENTITY,
        }
    }

    /// Runs `n` frames from birth at frame 0.
    fn steps(prog: &Program, q: &mut Particle, n: usize) -> Vec<(Vec4, Vec4)> {
        let env = StreamOutEnv::default();
        (0..n)
            .map(|f| {
                let out = run(prog, &frame(f as f32), &env, &[], q);
                q.pos = out.0;
                q.vel = out.1;
                out
            })
            .collect()
    }

    #[test]
    fn integrates_position_then_velocity() {
        let mut prog = program();
        prog.air = 0.5;
        let mut q = particle(Vec3::new(0.0, 4.0, 0.0));
        let out = steps(&prog, &mut q, 3);
        // Frame 0 starts from the birth attributes: p = 0 + 4, v = 4·0.5 − 0.5.
        assert_eq!(out[0].0, Vec4::new(0.0, 4.0, 0.0, 30.0));
        assert_eq!(out[0].1, Vec4::new(0.0, 1.5, 0.0, 0.0));
        assert_eq!(out[1].0.y, 5.5);
        assert_eq!(out[1].1, Vec4::new(0.0, 0.25, 0.0, 1.0));
        assert_eq!(out[2].0.y, 5.75);
        assert_eq!(out[2].1.y, -0.375);
    }

    #[test]
    fn world_gravity_turns_with_the_emitter() {
        let mut prog = program();
        prog.flags_a = 0x8;
        let mut f = frame(0.0);
        // The emitter turned a quarter about Z: world down is local −x... .
        f.rt = M34([
            [0.0, -1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]);
        let out = run(
            &prog,
            &f,
            &StreamOutEnv::default(),
            &[],
            &particle(Vec3::ZERO),
        );
        assert_eq!(out.1.truncate(), Vec3::new(-0.5, 0.0, 0.0));
    }

    #[test]
    fn random_field_moves_by_the_change_of_the_waves() {
        let mut prog = program();
        prog.gravity_scale = 0.0;
        prog.flags_b |= 0x2;
        prog.field[0] = 1.0; // amplitude x
        prog.field[3] = 10.0; // period scale
        let mut q = particle(Vec3::ZERO);
        let out = steps(&prog, &mut q, 2);
        let wave = |s: f32| waves(&prog, [4.0, 3.0, 2.0, 1.5], [0.6, 0.42, 0.23, 0.15], s);
        let phase = 0.25 * 10.0;
        let d0 = wave(phase + 1.0) - wave(phase);
        assert!((out[0].0.x - d0).abs() < 1e-5);
        let d1 = wave(phase + 2.0) - wave(phase + 1.0);
        assert!((out[1].0.x - (d0 + d1)).abs() < 1e-5);
        assert_eq!((out[1].0.y, out[1].0.z), (0.0, 0.0));
    }

    #[test]
    fn spin_turns_about_the_axis() {
        let mut prog = program();
        prog.gravity_scale = 0.0;
        prog.flags_b |= 0x20;
        prog.field[0xb0 / 4] = std::f32::consts::FRAC_PI_2;
        prog.field[0xb8 / 4] = 1.0; // about Y
        let mut q = particle(Vec3::ZERO);
        q.local_pos = Vec4::new(1.0, 0.0, 0.0, 30.0);
        let out = run(&prog, &frame(0.0), &StreamOutEnv::default(), &[], &q);
        // x' = x·c − z·s, z' = x·s + z·c.
        assert!((out.0.x).abs() < 1e-5 && (out.0.z - 1.0).abs() < 1e-5);
    }

    #[test]
    fn collision_bounces_off_a_local_plane() {
        let mut prog = program();
        prog.gravity_scale = 0.0;
        prog.flags_b |= 0x40;
        prog.field[0xc4 / 4] = 0.5; // bounce
        prog.field[0xc8 / 4] = 0.8; // friction
        prog.field[0xcc / 4] = 0.0; // plane y
        let q = particle(Vec3::new(1.0, -2.0, 0.0));
        let out = run(&prog, &frame(0.0), &StreamOutEnv::default(), &[], &q);
        assert_eq!(out.0.truncate(), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(out.1.truncate(), Vec3::new(0.8, 0.8, 0.0));
    }

    #[test]
    fn curl_texels_and_wrap() {
        let n = CURL_NOISE_SIZE;
        let mut table = vec![0u8; n * n * n * 3];
        // Texel (1, 0, 0): bytes (−128, 0, 127).
        table[3] = 0x80;
        table[4] = 0;
        table[5] = 0x7f;
        let at = |x: usize| (x as f32 + 0.5) / n as f32;
        let c = curl(&table, Vec3::new(at(1), at(0), at(0)));
        assert_eq!(c, Vec3::new(255.0 / 255.0, 128.0 / 255.0, 0.0));
        // One full turn further is the same texel.
        let c2 = curl(&table, Vec3::new(at(1) + 1.0, at(0) - 1.0, at(0) + 3.0));
        assert!((c2 - c).abs().max_element() < 1e-5);
        // Half way to texel 2 (which is 0 → 0x80).
        let mid = curl(&table, Vec3::new(at(1) + 0.5 / n as f32, at(0), at(0)));
        assert!((mid.x - (1.0 + 128.0 / 255.0) / 2.0).abs() < 1e-5);
    }

    #[test]
    fn frn1_kicks_on_the_interval() {
        let mut prog = program();
        prog.gravity_scale = 0.0;
        prog.flags_b |= 0x100;
        prog.field[0x50 / 4] = 1.0;
        prog.field[0x5c / 4] = 2.0;
        let mut q = particle(Vec3::ZERO);
        let out = steps(&prog, &mut q, 4);
        let kick = |age: f32| sin_r(-0.25 * 6.283_184 + 0.25 * age * age);
        assert!((out[0].1.x - kick(0.0)).abs() < 1e-6);
        assert_eq!(out[1].1.x, out[0].1.x);
        assert!((out[2].1.x - (out[0].1.x + kick(2.0))).abs() < 1e-6);
    }
}
