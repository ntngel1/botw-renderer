//! The 16 emission volumes (`eft_Emit_*`, table `0x1047efc4` indexed by
//! the emitter's `0x838`): where in the emitter a new particle starts and
//! its all-direction velocity. Each function draws from the emitter's LCG
//! and vector-table cursors in the game's order.
//! docs/research/eft-runtime.md §3.1.

use asset_format::effects::{EffectTables, EmitterParams, Primitive};
use bevy::math::Vec3;

use super::random::{Lcg, VecTables, angle_index, sin_cos, sin_cos_index};

/// The emitter's random state: LCG (`em+0x138`), table cursors
/// (`em+0x134` for A, `em+0x136` for B) and the sequential point counter
/// of the equal divisions (`em+0x34`).
#[derive(Clone, Copy, Debug, Default)]
pub struct EmitterRng {
    pub lcg: Lcg,
    pub a: u16,
    pub b: u16,
    pub sequence: u32,
}

impl EmitterRng {
    pub fn seeded(seed: u32) -> Self {
        Self {
            lcg: Lcg(seed),
            a: seed as u16,
            b: (seed >> 16) as u16,
            sequence: 0,
        }
    }
}

/// What a volume function reads besides the emitter's resource.
pub struct Volume<'a> {
    pub params: &'a EmitterParams,
    pub tables: &'a EffectTables,
    pub vectors: &'a VecTables,
    /// The shape primitive (`er+0x68`, volume 15).
    pub primitive: Option<&'a Primitive>,
    /// EASS: form scale (`em+0x4f0`).
    pub form_scale: Vec3,
    /// EAOV: all-direction velocity (`em+0x4cc`).
    pub omni: f32,
}

/// `(float)n` of an int the code converts as unsigned.
fn uf(n: i32) -> f32 {
    n as u32 as f32
}

/// `(int)f` for the code's `f < 2³¹ ? (int)f : (int)(f − 2³¹) − 2³¹`.
fn to_u32(f: f32) -> u32 {
    if f < 2.147_483_6e9 {
        f as i32 as u32
    } else {
        ((f - 2.147_483_6e9) as i32 as u32).wrapping_add(0x8000_0000)
    }
}

/// The arc's start: `0x848`, or `2π·e` when `0x839`.
fn arc_start(p: &EmitterParams, e: f32) -> f32 {
    if p.random_start_angle {
        let half = (f64::from(e) * std::f64::consts::PI) as f32;
        half + half
    } else {
        p.sweep_start
    }
}

/// `angle = rand·sweep + start − sweep/2`.
fn arc_angle(p: &EmitterParams, e: f32, rng: &mut EmitterRng) -> f32 {
    let start = arc_start(p, e);
    let r = rng.lcg.f32();
    -(p.sweep * 0.5 - (r * p.sweep + start))
}

/// The shortest-arc rotation from +Y to `dir`, applied to `v`, as the
/// library computes it (quaternion `(dir.z, 0, −dir.x)/s, w = s/2`, the
/// half turn about X when `1 + dir.y` is below the threshold).
pub fn rotate_from_y(tables: &EffectTables, dir: Vec3, v: Vec3) -> Vec3 {
    let d1 = ((0.0 * dir.x + dir.y) + 0.0 * dir.z) + 1.0;
    let (a, b, c, w) = if tables.arc_epsilon < d1 {
        let s = f64::from(d1 + d1).sqrt();
        let inv = (1.0 / s) as f32;
        let w = (s * 0.5) as f32;
        (
            (dir.z - 0.0 * dir.y) * inv,
            (0.0 * dir.x - 0.0 * dir.z) * inv,
            (0.0 * dir.y - dir.x) * inv,
            w,
        )
    } else {
        (1.0, 0.0, 0.0, 0.0)
    };
    let n = 2.0 / (w * w + (c * c + (a * a + b * b)));
    let (an, bn, cn) = (a * n, b * n, c * n);
    let r00 = 1.0 - (b * bn + c * cn);
    let r01 = a * bn - w * cn;
    let r02 = a * cn + w * bn;
    let r10 = a * bn + w * cn;
    let r11 = 1.0 - (a * an + c * cn);
    let r12 = b * cn - w * an;
    let r20 = a * cn - w * bn;
    let r21 = b * cn + w * an;
    let r22 = 1.0 - (a * an + b * bn);
    let dot = |r0: f32, r1: f32, r2: f32| {
        (f64::from(r2) * f64::from(v.z)
            + f64::from(r0) * f64::from(v.x)
            + f64::from(r1) * f64::from(v.y)) as f32
    };
    Vec3::new(dot(r00, r01, r02), dot(r10, r11, r12), dot(r20, r21, r22))
}

/// The axis a sphere's latitude points along (`0x83e`): +X, −X, +Y, −Y,
/// +Z, −Z; other values leave it on +Y.
fn latitude_axis(axis: u8) -> Option<Vec3> {
    match axis {
        0 => Some(Vec3::X),
        1 => Some(Vec3::NEG_X),
        2 => None,
        3 => Some(Vec3::NEG_Y),
        4 => Some(Vec3::Z),
        5 => Some(Vec3::NEG_Z),
        _ => Some(Vec3::Y),
    }
}

fn axis_rotate(v: &Volume, dir: Vec3) -> Vec3 {
    match latitude_axis(v.params.latitude_axis) {
        Some(axis) => rotate_from_y(v.tables, axis, dir),
        None => dir,
    }
}

fn radii(v: &Volume) -> Vec3 {
    Vec3::from(v.params.volume_radius) * v.form_scale
}

/// `p / |p| · omni`; the uninitialised vector of the zero case is taken
/// as zero.
fn outward(p: Vec3, omni: f32) -> Vec3 {
    if p == Vec3::ZERO {
        return Vec3::ZERO;
    }
    let length2 = p.z * p.z + (p.x * p.x + p.y * p.y);
    let inv = (1.0 / f64::from(length2).sqrt()) as f32;
    (p * inv) * omni
}

/// One particle's local position and velocity, or `None` when the volume
/// drops it. `e` is the emission's extra random, `k` the particle's index
/// in the emission.
pub fn emit(v: &Volume, rng: &mut EmitterRng, e: f32, k: u32) -> Option<(Vec3, Vec3)> {
    match v.params.volume {
        1 => Some(circle(v, rng, e)),
        2 => Some(circle_divided(v, rng, e, k)),
        3 => Some(circle_fill(v, rng, e)),
        4 => Some(sphere(v, rng, e, false)),
        5 | 6 => sphere_divided(v, rng, k),
        7 => Some(sphere(v, rng, e, true)),
        8 => {
            let (mut p, vel) = circle(v, rng, e);
            p.y = (rng.lcg.f32() * 2.0 - 1.0) * v.params.volume_radius[1] * v.form_scale.y;
            Some((p, vel))
        }
        9 => {
            let (mut p, vel) = circle_fill(v, rng, e);
            p.y = (rng.lcg.f32() * 2.0 - 1.0) * v.params.volume_radius[1] * v.form_scale.y;
            Some((p, vel))
        }
        10 => Some(cube(v, rng)),
        11 => Some(cube_fill(v, rng)),
        12 => Some(line(v, rng)),
        13 => Some(line_divided(v, rng, e, k)),
        14 => Some(rectangle(v, rng)),
        15 => Some(match v.primitive {
            Some(primitive) => primitive_point(v, rng, primitive, k),
            None => point(v, rng),
        }),
        // 0, and anything unknown.
        _ => Some(point(v, rng)),
    }
}

/// `eft_Emit_Point` `0x03b6f32c`.
fn point(v: &Volume, rng: &mut EmitterRng) -> (Vec3, Vec3) {
    (Vec3::ZERO, v.vectors.b(&mut rng.b) * v.omni)
}

fn circle_at(v: &Volume, angle: f32) -> (Vec3, Vec3) {
    let (sin, cos) = sin_cos(v.tables, angle);
    let p = Vec3::new(
        sin * v.params.volume_radius[0] * v.form_scale.x,
        0.0,
        cos * v.params.volume_radius[2] * v.form_scale.z,
    );
    (p, Vec3::new(sin * v.omni, 0.0, cos * v.omni))
}

/// `eft_Emit_Circle` `0x03b6f390`.
fn circle(v: &Volume, rng: &mut EmitterRng, e: f32) -> (Vec3, Vec3) {
    let angle = arc_angle(v.params, e, rng);
    circle_at(v, angle)
}

/// `eft_Emit_CircleEquallyDivided` `0x03b6f50c`.
fn circle_divided(v: &Volume, rng: &mut EmitterRng, e: f32, k: u32) -> (Vec3, Vec3) {
    let p = v.params;
    let start = arc_start(p, e);
    let mut n = p.circle_divisions as u32;
    if p.division_mode == 0 {
        let cut = (e * uf(p.circle_division_random)) * 0.01 * n as f32;
        n = n.wrapping_sub(to_u32(cut));
    }
    if p.sweep != std::f32::consts::TAU && n > 1 {
        n -= 1;
    }
    let index = match p.division_mode {
        1 => to_u32(rng.lcg.f32() * n as f32),
        2 => {
            let i = rng.sequence.checked_rem(n).unwrap_or(0);
            rng.sequence = if rng.sequence + 1 < n {
                rng.sequence + 1
            } else {
                0
            };
            i
        }
        _ => k,
    };
    let jitter = rng.lcg.f32() - 0.5;
    let angle = start
        + (jitter + jitter) * p.division_angle_random
        + (index as f32 * (p.sweep / n as f32) - p.sweep * 0.5);
    circle_at(v, angle)
}

/// `eft_Emit_CircleFill` `0x03b6f8bc`.
fn circle_fill(v: &Volume, rng: &mut EmitterRng, e: f32) -> (Vec3, Vec3) {
    let p = v.params;
    let angle = arc_angle(p, e, rng);
    let (sin, cos) = sin_cos(v.tables, angle);
    let inner = 1.0 - p.caliber;
    let r = rng.lcg.f32();
    let rx = p.volume_radius[0] * v.form_scale.x;
    let rz = p.volume_radius[2] * v.form_scale.z;
    let f = inner * inner * (1.0 - r) + r;
    let (pos, mut vx, mut vz) = if 0.0 < f {
        let s = f64::from(f).sqrt();
        let (ss, sc) = ((s * f64::from(sin)) as f32, (s * f64::from(cos)) as f32);
        (
            Vec3::new(
                (f64::from(ss) * f64::from(rx)) as f32,
                0.0,
                (f64::from(sc) * f64::from(rz)) as f32,
            ),
            (f64::from(ss) * s) as f32,
            (f64::from(sc) * s) as f32,
        )
    } else {
        (Vec3::new(sin * 0.0 * rx, 0.0, cos * 0.0 * rz), 0.0, 0.0)
    };
    // The direction in the unit circle: undo the aspect.
    if rx <= rz {
        vx *= rx / rz;
    } else {
        vz *= rz / rx;
    }
    let length2 = vx * vx + vz * vz;
    let vel = if 0.0 < length2 {
        let inv = (1.0 / f64::from(length2).sqrt()) as f32;
        Vec3::new(vx * inv * v.omni, 0.0, vz * inv * v.omni)
    } else {
        Vec3::new(0.0, 0.0, v.omni)
    };
    (pos, vel)
}

/// `eft_Emit_Sphere` `0x03b6fbd4` and `eft_Emit_SphereFill` `0x03b71544`.
fn sphere(v: &Volume, rng: &mut EmitterRng, e: f32, fill: bool) -> (Vec3, Vec3) {
    let p = v.params;
    let latitude = p.latitude_mode;
    let angle = if latitude {
        let r = rng.lcg.f32() * std::f32::consts::PI;
        r + r
    } else {
        arc_angle(p, e, rng)
    };
    let (sin, cos) = sin_cos(v.tables, angle);
    let y = if latitude {
        let (_, cos_lat) = sin_cos(v.tables, p.latitude);
        -((1.0 - cos_lat) * rng.lcg.f32() - 1.0)
    } else {
        let r = rng.lcg.f32();
        (r + r) - 1.0
    };
    let ring = -(y * y - 1.0);
    let s = if 0.0 < ring {
        f64::from(ring).sqrt() as f32
    } else {
        0.0
    };
    let factor = if fill {
        let r = rng.lcg.f32();
        let root = if 0.0 < r {
            f64::from(r).sqrt() as f32
        } else {
            0.0
        };
        (root * p.caliber + 1.0) - p.caliber
    } else {
        1.0
    };
    let mut dir = Vec3::new(s * sin, y, s * cos);
    if latitude {
        dir = axis_rotate(v, dir);
    }
    let r = radii(v);
    ((dir * r) * factor, dir * v.omni)
}

/// `eft_Emit_SphereEquallyDivided` `0x03b70400` (5) and `…64`
/// `0x03b70ca4` (6): points of a table; points below the latitude are
/// dropped.
fn sphere_divided(v: &Volume, rng: &mut EmitterRng, k: u32) -> Option<(Vec3, Vec3)> {
    let p = v.params;
    let table = if p.volume == 5 {
        v.tables.sphere.get(usize::from(p.sphere_table))?
    } else {
        v.tables
            .sphere64
            .get(usize::from(p.sphere64_count).checked_sub(4)?)?
    };
    let n = table.len() as u32;
    let index = match p.division_mode {
        1 => to_u32(rng.lcg.f32() * n as f32),
        2 => {
            let i = rng.sequence % n.max(1);
            rng.sequence = if rng.sequence + 1 < n {
                rng.sequence + 1
            } else {
                0
            };
            i
        }
        _ => k,
    };
    let dir = Vec3::from(*table.get(index as usize)?);
    if p.latitude <= 3.141_492_8 {
        let (_, cos_lat) = sin_cos_index(v.tables, angle_index(p.latitude));
        if dir.y <= cos_lat {
            return None;
        }
    }
    let dir = axis_rotate(v, dir);
    Some((dir * radii(v), dir * v.omni))
}

/// `eft_Emit_Box` `0x03b71ff0`: a point on a face; the face and its side
/// come from the raw LCG states.
fn cube(v: &Volume, rng: &mut EmitterRng) -> (Vec3, Vec3) {
    let s0 = rng.lcg.raw();
    let s1 = rng.lcg.raw();
    let r2 = rng.lcg.f32();
    let r3 = rng.lcg.f32();
    let r4 = rng.lcg.f32();
    let r = radii(v);
    let pos = if s0 < 0x5555_5555 {
        let z = if s1 < 0x7fff_ffff { r.z } else { -r.z };
        Vec3::new(r.x * (r2 * 2.0 - 1.0), r.y * (r3 * 2.0 - 1.0), z)
    } else {
        let z = r.z * (r4 * 2.0 - 1.0);
        if s0 < 0xaaaa_aaaa {
            let y = if s1 < 0x7fff_ffff { r.y } else { -r.y };
            Vec3::new(r.x * (r2 * 2.0 - 1.0), y, z)
        } else {
            let x = if 0x7fff_fffe < s1 { -r.x } else { r.x };
            Vec3::new(x, r.y * (r3 * 2.0 - 1.0), z)
        }
    };
    (pos, outward(pos, v.omni))
}

/// `eft_Emit_BoxFill` `0x03b72318`: a point in the shell between the
/// inner box `1 − caliber` and the box.
fn cube_fill(v: &Volume, rng: &mut EmitterRng) -> (Vec3, Vec3) {
    let p = v.params;
    let mut c = 1.0 - p.caliber;
    if c == 1.0 {
        c = 0.999;
    }
    let d = 1.0 - c;
    let pick = rng.lcg.f32() * -((c * c) * c - 1.0);
    let mut unit = if d <= pick {
        if d + d * c <= pick {
            Vec3::new(rng.lcg.f32() * c, rng.lcg.f32() * c, rng.lcg.f32() * d + c)
        } else {
            Vec3::new(rng.lcg.f32() * d + c, rng.lcg.f32() * c, rng.lcg.f32())
        }
    } else {
        Vec3::new(rng.lcg.f32(), rng.lcg.f32() * d + c, rng.lcg.f32())
    };
    if rng.lcg.f32() < 0.5 {
        unit.x = -unit.x;
    }
    if rng.lcg.f32() < 0.5 {
        unit.y = -unit.y;
    }
    if rng.lcg.f32() < 0.5 {
        unit.z = -unit.z;
    }
    let pos = Vec3::new(
        unit.x * p.volume_radius[0] * v.form_scale.x,
        unit.y * p.volume_radius[1] * v.form_scale.y,
        unit.z * p.volume_radius[2] * v.form_scale.z,
    );
    let dir = if pos == Vec3::ZERO {
        // (Not reached with a non-zero box: a random direction.)
        Vec3::new(rng.lcg.f32(), rng.lcg.f32(), rng.lcg.f32())
    } else {
        pos
    };
    (pos, outward(dir, v.omni))
}

fn line_z(p: &EmitterParams, length: f32, t: f32) -> f32 {
    t * length - (p.line_center * length + length) * 0.5
}

/// `eft_Emit_Line` `0x03b72890`.
fn line(v: &Volume, rng: &mut EmitterRng) -> (Vec3, Vec3) {
    let length = v.params.line_length * v.form_scale.z;
    let z = line_z(v.params, length, rng.lcg.f32());
    (Vec3::new(0.0, 0.0, z), Vec3::new(0.0, 0.0, v.omni))
}

/// `eft_Emit_LineEquallyDivided` `0x03b72934`.
fn line_divided(v: &Volume, rng: &mut EmitterRng, e: f32, k: u32) -> (Vec3, Vec3) {
    let p = v.params;
    let length = p.line_length * v.form_scale.z;
    let mut n = p.line_divisions as u32;
    let index = match p.division_mode {
        0 => {
            let cut = (e * uf(p.line_division_random)) * 0.01 * n as f32;
            n = n.wrapping_sub(to_u32(cut));
            k
        }
        1 => to_u32(rng.lcg.f32() * n as f32),
        2 => {
            let i = rng.sequence.checked_rem(n).unwrap_or(0);
            rng.sequence = if rng.sequence + 1 < n {
                rng.sequence + 1
            } else {
                0
            };
            i
        }
        _ => k,
    };
    let t = if n == 1 {
        index as f32 * 0.0 + 0.5
    } else {
        (1.0 / n.wrapping_sub(1) as f32) * index as f32 + 0.0
    };
    (
        Vec3::new(0.0, 0.0, line_z(p, length, t)),
        Vec3::new(0.0, 0.0, v.omni),
    )
}

/// `eft_Emit_Rectangle` `0x03b72c4c`: a point on the outline.
fn rectangle(v: &Volume, rng: &mut EmitterRng) -> (Vec3, Vec3) {
    let s0 = rng.lcg.raw();
    let s1 = rng.lcg.raw();
    let r2 = rng.lcg.f32();
    let r3 = rng.lcg.f32();
    let rx = v.params.volume_radius[0] * v.form_scale.x;
    let rz = v.params.volume_radius[2] * v.form_scale.z;
    let pos = if s0 < 0x7fff_ffff {
        let z = if s1 < 0x7fff_ffff { rz } else { -rz };
        Vec3::new(rx * (r2 * 2.0 - 1.0), 0.0, z)
    } else {
        let z = rz * (r3 * 2.0 - 1.0);
        let x = if s1 < 0x7fff_ffff { rx } else { -rx };
        Vec3::new(x, 0.0, z)
    };
    (pos, outward(pos, v.omni))
}

/// `eft_Emit_Primitive` `0x03b72eb8`: a vertex of the shape primitive.
fn primitive_point(
    v: &Volume,
    rng: &mut EmitterRng,
    primitive: &Primitive,
    k: u32,
) -> (Vec3, Vec3) {
    let n = primitive.positions.len() as u32;
    let index = match v.params.division_mode {
        1 => to_u32(rng.lcg.f32() * n as f32),
        2 => {
            let i = rng.sequence.checked_rem(n).unwrap_or(0);
            rng.sequence = rng.sequence.wrapping_add(1);
            i
        }
        _ => k,
    } as usize;
    let pos = primitive
        .positions
        .get(index)
        .map_or(Vec3::ZERO, |p| Vec3::from(*p) * v.form_scale);
    let vel = primitive
        .normals
        .get(index)
        .map_or(Vec3::ZERO, |n| Vec3::from(*n) * v.omni);
    (pos, vel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables() -> EffectTables {
        // sead's table is sin/cos at 256 steps with linear deltas.
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
        EffectTables {
            sin_cos,
            sphere: vec![vec![[0.0, 1.0, 0.0], [0.0, -1.0, 0.0]]],
            sphere64: Vec::new(),
            arc_epsilon: f32::from_bits(0x3480_0000),
            curl_noise: Vec::new(),
        }
    }

    #[test]
    fn arc_rotation_maps_y_onto_the_axes() {
        let t = tables();
        for (axis, expect) in [
            (0, Vec3::X),
            (1, Vec3::NEG_X),
            (3, Vec3::NEG_Y),
            (4, Vec3::Z),
            (5, Vec3::NEG_Z),
        ] {
            let got = rotate_from_y(&t, latitude_axis(axis).unwrap(), Vec3::Y);
            assert!((got - expect).length() < 1e-6, "axis {axis}: {got}");
        }
        // A cone direction tilts with the target.
        let d = Vec3::new(1.0, 1.0, 0.0).normalize();
        assert!((rotate_from_y(&t, d, Vec3::Y) - d).length() < 1e-6);
    }

    #[test]
    fn sphere_division_drops_points_below_the_latitude() {
        let t = tables();
        let vectors = VecTables::new();
        let mut params = super::super::sim::tests::params();
        params.volume = 5;
        params.sphere_table = 0;
        params.latitude_axis = 2;
        params.latitude = std::f32::consts::FRAC_PI_2;
        let v = Volume {
            params: &params,
            tables: &t,
            vectors: &vectors,
            primitive: None,
            form_scale: Vec3::ONE,
            omni: 2.0,
        };
        let mut rng = EmitterRng::seeded(1);
        let (p, vel) = emit(&v, &mut rng, 0.0, 0).unwrap();
        assert_eq!(p, Vec3::Y);
        assert_eq!(vel, Vec3::Y * 2.0);
        assert!(emit(&v, &mut rng, 0.0, 1).is_none());
    }

    #[test]
    fn cube_points_lie_on_a_face() {
        let t = tables();
        let vectors = VecTables::new();
        let mut params = super::super::sim::tests::params();
        params.volume = 10;
        params.volume_radius = [1.0, 2.0, 3.0];
        let v = Volume {
            params: &params,
            tables: &t,
            vectors: &vectors,
            primitive: None,
            form_scale: Vec3::ONE,
            omni: 1.0,
        };
        let mut rng = EmitterRng::seeded(12345);
        for _ in 0..100 {
            let before = rng.lcg.0;
            let (p, vel) = emit(&v, &mut rng, 0.0, 0).unwrap();
            let on_face = p.x.abs() == 1.0 || p.y.abs() == 2.0 || p.z.abs() == 3.0;
            assert!(on_face, "{p}");
            assert!((vel.length() - 1.0).abs() < 1e-5);
            // Five states per particle.
            let mut l = Lcg(before);
            for _ in 0..5 {
                l.raw();
            }
            assert_eq!(rng.lcg.0, l.0);
        }
    }

    #[test]
    fn line_division_spreads_points_over_the_length() {
        let t = tables();
        let vectors = VecTables::new();
        let mut params = super::super::sim::tests::params();
        params.volume = 13;
        params.line_divisions = 5;
        params.line_length = 4.0;
        params.division_mode = 0;
        let v = Volume {
            params: &params,
            tables: &t,
            vectors: &vectors,
            primitive: None,
            form_scale: Vec3::ONE,
            omni: 1.0,
        };
        let mut rng = EmitterRng::seeded(0);
        let z: Vec<f32> = (0..5)
            .map(|k| emit(&v, &mut rng, 0.0, k).unwrap().0.z)
            .collect();
        assert_eq!(z, vec![-2.0, -1.0, 0.0, 1.0, 2.0]);
    }
}
