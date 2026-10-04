//! The effect library's random numbers and the lookup tables its CPU code
//! reads: the per-emitter LCG (`eft_Random_GetF32` `0x03b6d494`), the
//! game's xorshift128 (`sead::Random`), the two 512-entry vector tables
//! (`eft_Random_InitVecTables` `0x03b5f06c`) and sead's sine table
//! (`sead::Mathf::sinIdx`/`cosIdx`, read from the RPX at bake time).
//! docs/research/eft-runtime.md §0.

use asset_format::effects::EffectTables;
use bevy::math::Vec3;

/// `sead::Random`: xorshift128 seeded the sead way.
#[derive(Clone, Debug)]
pub struct SeadRandom([u32; 4]);

impl SeadRandom {
    pub fn new(seed: u32) -> Self {
        let step = |previous: u32, n: u32| {
            (previous ^ (previous >> 30))
                .wrapping_mul(0x6C07_8965)
                .wrapping_add(n)
        };
        let x = step(seed, 1);
        let y = step(x, 2);
        let z = step(y, 3);
        Self([x, y, z, step(z, 4)])
    }

    /// A generator in the given state (x, y, z, w).
    pub const fn from_state(state: [u32; 4]) -> Self {
        Self(state)
    }

    pub fn next(&mut self) -> u32 {
        let [x, y, z, w] = self.0;
        let t = x ^ (x << 11);
        let next = w ^ (w >> 19) ^ t ^ (t >> 8);
        self.0 = [y, z, w, next];
        next
    }

    /// `[0, 1)`: the top 23 bits as the mantissa of `[1, 2)`, − 1.
    pub fn unit(&mut self) -> f32 {
        f32::from_bits((self.next() >> 9) | 0x3F80_0000) - 1.0
    }
}

/// The emitter's LCG (`em+0x138`): the value is taken before the step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lcg(pub u32);

impl Lcg {
    /// The raw state, then a step (`0x03b57e2c`, the box volumes).
    pub fn raw(&mut self) -> u32 {
        let value = self.0;
        self.0 = value.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039);
        value
    }

    /// `[0, 1]`: `(float)state · 2⁻³²` (the u32 is rounded to f32 first,
    /// so states near 2³² give exactly 1).
    pub fn f32(&mut self) -> f32 {
        self.raw() as f32 * 2.328_306_4e-10
    }

    /// `[0, n)` from the top of the state (`(u64)state·n >> 32`).
    pub fn below(&mut self, n: i32) -> i32 {
        let s = self.raw();
        // `mulhwu(s, n) + s·(n >> 31)`: the signed high word.
        let high = ((u64::from(s) * u64::from(n as u32)) >> 32) as u32;
        high.wrapping_add(s.wrapping_mul((n >> 31) as u32)) as i32
    }
}

/// The two global vector tables: `A` (box `[-1, 1)³`, cursor `em+0x134`)
/// and `B` (unit vectors, cursor `em+0x136`).
pub struct VecTables {
    pub a: Vec<Vec3>,
    pub b: Vec<Vec3>,
}

/// The xorshift state `eft_Random_InitVecTables` starts from.
const VEC_TABLE_STATE: [u32; 4] = [0x178E_AB2C, 0xE318_145E, 0x45F0_CDB4, 0x720A_056D];

impl VecTables {
    pub fn new() -> Self {
        let mut random = SeadRandom::from_state(VEC_TABLE_STATE);
        let mut draw = || {
            let f = random.unit();
            (f + f) - 1.0
        };
        let mut a = Vec::with_capacity(512);
        let mut b = Vec::with_capacity(512);
        for _ in 0..512 {
            a.push(Vec3::new(draw(), draw(), draw()));
            let v = Vec3::new(draw(), draw(), draw());
            let length2 = v.z * v.z + v.x * v.x + v.y * v.y;
            let inv = (1.0 / f64::from(length2).sqrt()) as f32;
            b.push(v * inv);
        }
        Self { a, b }
    }

    pub fn a(&self, cursor: &mut u16) -> Vec3 {
        let v = self.a[usize::from(*cursor & 0x1ff)];
        *cursor = cursor.wrapping_add(1);
        v
    }

    pub fn b(&self, cursor: &mut u16) -> Vec3 {
        let v = self.b[usize::from(*cursor & 0x1ff)];
        *cursor = cursor.wrapping_add(1);
        v
    }
}

impl Default for VecTables {
    fn default() -> Self {
        Self::new()
    }
}

/// Radians → sead angle index: `(u32)(s64)(rad · 2³²/2π)` with the
/// product in f32 (`FUN_0421184c` truncates towards zero).
pub fn angle_index(radians: f32) -> u32 {
    (radians * 6.835_652_5e8) as i64 as u32
}

/// `sead::Mathf::sinIdx` / `cosIdx` over the table: entry `idx >> 24`,
/// linear within it.
pub fn sin_cos_index(tables: &EffectTables, index: u32) -> (f32, f32) {
    let [sin, dsin, cos, dcos] = tables.sin_cos[(index >> 24) as usize];
    let frac = (index & 0x00ff_ffff) as f32 * 5.960_464_5e-8;
    (dsin * frac + sin, dcos * frac + cos)
}

pub fn sin_cos(tables: &EffectTables, radians: f32) -> (f32, f32) {
    sin_cos_index(tables, angle_index(radians))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcg_takes_the_value_before_the_step() {
        let mut r = Lcg(0);
        assert_eq!(r.raw(), 0);
        assert_eq!(r.0, 0x3039);
        assert_eq!(r.raw(), 0x3039);
        assert_eq!(
            r.0,
            0x3039u32.wrapping_mul(0x41C6_4E6D).wrapping_add(0x3039)
        );
        // The top state rounds to exactly 1.
        assert_eq!(Lcg(u32::MAX).f32(), 1.0);
        assert_eq!(Lcg(0x8000_0000).f32(), 0.5);
        // `[0, n)` from the high word.
        assert_eq!(Lcg(0x8000_0000).below(10), 5);
        assert_eq!(Lcg(0xffff_ffff).below(10), 9);
        assert_eq!(Lcg(0x1234).below(0), 0);
    }

    #[test]
    fn xorshift_matches_the_reference_step() {
        // One standard xorshift128 step by hand.
        let mut r = SeadRandom::from_state([1, 2, 3, 4]);
        let t = 1u32 ^ (1 << 11);
        assert_eq!(r.next(), 4 ^ (4 >> 19) ^ t ^ (t >> 8));
    }

    #[test]
    fn vector_tables_are_a_box_and_unit_vectors() {
        let t = VecTables::new();
        assert_eq!((t.a.len(), t.b.len()), (512, 512));
        assert!(t.a.iter().all(|v| v.abs().max_element() <= 1.0));
        assert!(t.b.iter().all(|v| (v.length() - 1.0).abs() < 1e-5));
        // The first A entry is the first three draws of the state.
        let mut r = SeadRandom::from_state(VEC_TABLE_STATE);
        let x = r.unit() * 2.0 - 1.0;
        assert_eq!(t.a[0].x, x);
        let mut cursor = 0x2ffu16;
        assert_eq!(t.a(&mut cursor), t.a[0xff]);
        assert_eq!(cursor, 0x300);
    }

    #[test]
    fn angle_index_wraps_negative_angles() {
        assert_eq!(angle_index(0.0), 0);
        assert_eq!(angle_index(-1e-3), (-683_565.25f32 as i64) as u32);
        assert_eq!(angle_index(std::f32::consts::FRAC_PI_2) >> 24, 0x40);
    }
}
