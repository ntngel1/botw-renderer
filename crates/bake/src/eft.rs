//! An emitter's resource (`EMTR` data, 0xA88 bytes, big-endian) as the
//! effect library (NintendoWare eft2 in `U-King.rpx` v208) reads it:
//! the load-time patch (`eft_EmitterResource_Setup` `0x03b5f62c`), the
//! uniform blocks the GPU gets (`sysEmitterStaticUniformBlock` = the
//! patched bytes `0x000..0x750`, `sysEmitterFieldUniformBlock` from the
//! field nodes) and the fields the CPU side uses. Offsets and meanings:
//! docs/research/eft-runtime.md (they replace the third-party meanings of
//! `botw_formats::ptcl::emitter` where they differ).

use asset_format::effects as fx;

/// Bytes of an emitter's resource in version 20.
pub const SIZE: usize = 0xA88;
/// The static uniform block: resource bytes `0x000..0x750`
/// (`eft_Resource_InitEmitterResource` `0x03b61e98`: `er+8 = res`,
/// `er+0xc = 0x750`); the first 0x50 bytes keep their stored order.
pub const STATIC_KEEP: usize = 0x50;
pub const STATIC_END: usize = 0x750;
/// The field uniform block's size.
pub const FIELD_SIZE: usize = 0x120;

/// Big-endian reads over a resource.
pub struct Res<'a>(pub &'a [u8]);

impl Res<'_> {
    pub fn u8(&self, at: usize) -> u8 {
        self.0[at]
    }
    pub fn flag(&self, at: usize) -> bool {
        self.0[at] != 0
    }
    pub fn u32(&self, at: usize) -> u32 {
        u32::from_be_bytes(self.0[at..at + 4].try_into().unwrap())
    }
    pub fn i32(&self, at: usize) -> i32 {
        self.u32(at) as i32
    }
    pub fn u64(&self, at: usize) -> u64 {
        u64::from_be_bytes(self.0[at..at + 8].try_into().unwrap())
    }
    pub fn f32(&self, at: usize) -> f32 {
        f32::from_bits(self.u32(at))
    }
    pub fn v3(&self, at: usize) -> [f32; 3] {
        [self.f32(at), self.f32(at + 4), self.f32(at + 8)]
    }
}

fn put_u32(data: &mut [u8], at: usize, v: u32) {
    data[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

fn put_f32(data: &mut [u8], at: usize, v: f32) {
    put_u32(data, at, v.to_bits());
}

fn copy_u32(data: &mut [u8], to: usize, from: usize) {
    let v = Res(data).u32(from);
    put_u32(data, to, v);
}

/// The field (`F***`) attribute nodes an emitter has, by kind: their data.
#[derive(Default)]
pub struct Fields<'a> {
    pub random: Option<&'a [u8]>,    // FRND, er+0x124
    pub random1: Option<&'a [u8]>,   // FRN1, er+0x128
    pub magnet: Option<&'a [u8]>,    // FMAG, er+0x12c
    pub spin: Option<&'a [u8]>,      // FSPN, er+0x130
    pub collision: Option<&'a [u8]>, // FCOL, er+0x134
    pub converge: Option<&'a [u8]>,  // FCOV, er+0x138
    pub pos_add: Option<&'a [u8]>,   // FPAD, er+0x13c
    pub custom: Option<&'a [u8]>,    // FCSF, er+0x140
    pub curl: Option<&'a [u8]>,      // FCLN, er+0x144
}

impl<'a> Fields<'a> {
    pub fn from_attributes(attributes: &[(String, &'a [u8])]) -> Self {
        let mut f = Self::default();
        for (magic, data) in attributes {
            let slot = match magic.as_str() {
                "FRND" => &mut f.random,
                "FRN1" => &mut f.random1,
                "FMAG" => &mut f.magnet,
                "FSPN" => &mut f.spin,
                "FCOL" => &mut f.collision,
                "FCOV" => &mut f.converge,
                "FPAD" => &mut f.pos_add,
                "FCSF" => &mut f.custom,
                "FCLN" => &mut f.curl,
                _ => continue,
            };
            *slot = Some(data);
        }
        f
    }

    fn any(&self) -> bool {
        [
            self.random,
            self.random1,
            self.magnet,
            self.spin,
            self.collision,
            self.converge,
            self.pos_add,
            self.custom,
            self.curl,
        ]
        .iter()
        .any(Option::is_some)
    }
}

/// `eft_EmitterResource_Setup` on a copy of the resource: constant colours
/// into key 0, key tables padded to 8, loop periods and random-start flags
/// as floats, texture UV tiling and disabled animations zeroed, rotation
/// axes, the equal-division emission counts, the flag words `0x50` and
/// `0x54`, the custom field's first word at `0x5c`. `vertices`: the
/// shape primitive's vertex count (volume type 15).
pub fn setup(res: &[u8], fields: &Fields, vertices: Option<u32>) -> Vec<u8> {
    let mut d = res.to_vec();
    let r = |d: &[u8], at| Res(d).u8(at);
    // Constant colour/alpha sources: the constants into key 0.
    if r(&d, 0x9a4) == 0 {
        for k in 0..3 {
            copy_u32(&mut d, 0x3c0 + 4 * k, 0x9a8 + 4 * k);
        }
    }
    if r(&d, 0x9a6) == 0 {
        copy_u32(&mut d, 0x440, 0x9b4);
    }
    if r(&d, 0x9a5) == 0 {
        for k in 0..3 {
            copy_u32(&mut d, 0x4c0 + 4 * k, 0x9b8 + 4 * k);
        }
    }
    if r(&d, 0x9a7) == 0 {
        copy_u32(&mut d, 0x540, 0x9c4);
    }
    // UV tiling of each sampler (`+4` of its flags): 0 → (1, 1),
    // 1 → (2, 1), 2 → (1, 2), 3 → (2, 2), else left.
    for s in 0..3 {
        let tiling = match r(&d, 0xa5c + 0x10 * s) {
            0 => Some((1.0, 1.0)),
            1 => Some((2.0, 1.0)),
            2 => Some((1.0, 2.0)),
            3 => Some((2.0, 2.0)),
            _ => None,
        };
        if let Some((u, v)) = tiling {
            put_f32(&mut d, 0x300 + 0x50 * s, u);
            put_f32(&mut d, 0x304 + 0x50 * s, v);
        }
    }
    // Disabled scroll, rotate, scale animations: zero (scale initial 1).
    for s in 0..3 {
        let (flags, uv) = (0xa58 + 0x10 * s, 0x2c0 + 0x50 * s);
        if r(&d, flags + 1) == 0 {
            for at in [0x08, 0x0c, 0x00, 0x04, 0x10, 0x14] {
                put_u32(&mut d, uv + at, 0);
            }
        }
        if r(&d, flags + 2) == 0 {
            for at in [0x30, 0x34, 0x38] {
                put_u32(&mut d, uv + at, 0);
            }
        }
        if r(&d, flags + 3) == 0 {
            put_f32(&mut d, uv + 0x20, 1.0);
            put_f32(&mut d, uv + 0x24, 1.0);
            for at in [0x18, 0x1c, 0x28, 0x2c] {
                put_u32(&mut d, uv + at, 0);
            }
        }
    }
    // Rotation axes that are off.
    for axis in 0..3 {
        if r(&d, 0x8b0 + axis) == 0 {
            for at in [0x700, 0x710, 0x720, 0x730] {
                put_u32(&mut d, at + 4 * axis, 0);
            }
        }
    }
    // Key tables: the last key repeated up to 8.
    for (count_at, table) in [
        (0x60, 0x3c0),
        (0x68, 0x4c0),
        (0x64, 0x440),
        (0x6c, 0x540),
        (0x70, 0x600),
        (0x74, 0x680),
    ] {
        let count = Res(&d).i32(count_at);
        if count > 0 && count < 8 {
            let last = table + 0x10 * (count as usize - 1);
            for k in count as usize..8 {
                for w in 0..4 {
                    copy_u32(&mut d, table + 0x10 * k + 4 * w, last + 4 * w);
                }
            }
        }
    }
    // Loop periods (frames) and random starts of the five tracks.
    for track in 0..5 {
        let period = if r(&d, 0x8d8 + track) != 0 {
            Res(&d).i32(0x8e4 + 4 * track) as f32
        } else {
            0.0
        };
        put_f32(&mut d, 0x80 + 4 * track, period);
        let random = if r(&d, 0x8dd + track) != 0 { 1.0 } else { 0.0 };
        put_f32(&mut d, 0x94 + 4 * track, random);
    }
    // Equal-division emission: how many particles one emission makes.
    let volume = r(&d, 0x838);
    let mode = Res(&d).i32(0x874);
    match volume {
        2 | 13 => {
            if mode == 0 {
                put_f32(&mut d, 0x800, 1.0);
            }
        }
        5 => {
            if mode == 0 {
                const POINTS: [f32; 8] = [2.0, 3.0, 4.0, 6.0, 8.0, 12.0, 20.0, 32.0];
                let table = r(&d, 0x83c) as usize;
                put_f32(&mut d, 0x800, POINTS.get(table).copied().unwrap_or(0.0));
            }
        }
        6 => {
            if mode == 0 {
                let points = f32::from(r(&d, 0x83d));
                put_f32(&mut d, 0x800, points);
            }
        }
        15 => match vertices {
            None => put_f32(&mut d, 0x800, 1.0),
            Some(n) => {
                if mode == 0 {
                    put_f32(&mut d, 0x800, n as f32);
                }
            }
        },
        _ => put_u32(&mut d, 0x874, u32::MAX),
    }
    // Flag word 0x50.
    let mut w: u32 = if r(&d, 0x7f1) != 0 { 8 } else { 0 };
    w |= match r(&d, 0x9ef) >> 4 {
        0 => 1,
        1 => 2,
        2 => 4,
        _ => 0,
    };
    for s in 0..3usize {
        let pattern = 0x110 + 0x90 * s;
        match r(&d, 0xa58 + 0x10 * s) {
            1 => w |= 0x10 << (4 * s),
            2 => w |= 0x20 << (4 * s),
            3 => w |= 0x40 << (4 * s),
            4 => {
                // The table becomes 0, 1, 2 … below the count at +8.
                let count = Res(&d).f32(pattern + 8);
                let mut i = 0;
                while (i as f32) < count && i < 32 {
                    put_u32(&mut d, pattern + 0x10 + 4 * i, i as u32);
                    i += 1;
                }
                put_f32(&mut d, pattern, count);
                w |= 0x80 << (4 * s);
            }
            _ => {}
        }
    }
    for (at, bit) in [
        (0x8ad, 0x10000),
        (0x8ae, 0x20000),
        (0x8af, 0x40000),
        (0xa5d, 0x80000),
        (0xa5e, 0x100000),
        (0xa6d, 0x200000),
        (0xa6e, 0x400000),
        (0xa7d, 0x800000),
        (0xa7e, 0x1000000),
        (0xa5f, 0x2000000),
        (0xa6f, 0x4000000),
        (0xa7f, 0x8000000),
        (0x8b3, 0x10000000),
    ] {
        if r(&d, at) != 0 {
            w |= bit;
        }
    }
    match r(&d, 0x8ac) {
        1 => w |= 0x40000000,
        0 => w |= 0x20000000,
        _ => {}
    }
    // Flag word 0x54: fields and the follow type.
    let mut v: u32 = u32::from(r(&d, 0x8b4) != 0);
    for (present, bit) in [
        (fields.random.is_some(), 2),
        (fields.random1.is_some(), 0x100),
        (fields.pos_add.is_some(), 4),
        (fields.magnet.is_some(), 8),
        (fields.converge.is_some(), 0x10),
        (fields.spin.is_some(), 0x20),
        (fields.collision.is_some(), 0x40),
        (fields.curl.is_some(), 0x80),
    ] {
        if present {
            v |= bit;
        }
    }
    v |= match r(&d, 0x753) {
        0 => 0x200,
        1 => 0x800,
        2 => 0x400,
        _ => 0,
    };
    put_u32(&mut d, 0x50, w);
    put_u32(&mut d, 0x54, v);
    if let Some(custom) = fields.custom {
        let first = Res(custom).u32(0);
        put_u32(&mut d, 0x5c, first);
    }
    d
}

/// The static uniform block of a patched resource: the library saves the
/// first 0x50 bytes, byte-swaps the block word by word to the GPU's order
/// and puts those bytes back (`0x03b61e98`).
pub fn static_block(patched: &[u8]) -> Vec<u8> {
    let mut block = patched[..STATIC_KEEP].to_vec();
    block.extend(swap_words(&patched[STATIC_KEEP..STATIC_END]));
    block
}

fn swap_words(bytes: &[u8]) -> Vec<u8> {
    bytes
        .chunks_exact(4)
        .flat_map(|w| [w[3], w[2], w[1], w[0]])
        .collect()
}

/// `sysEmitterFieldUniformBlock` (0x120 bytes, little-endian) from the
/// field nodes, as the setup's tail fills it; `None` without fields.
pub fn field_block(fields: &Fields) -> Option<Vec<u8>> {
    if !fields.any() {
        return None;
    }
    let mut u = vec![0u8; FIELD_SIZE];
    let mut word = |at: usize, v: u32| put_u32(&mut u, at, v);
    if let Some(n) = fields.random {
        let n = Res(n);
        for k in 0..3 {
            word(4 * k, n.u32(4 + 4 * k));
        }
        word(0x0c, (n.i32(0x10) as f32).to_bits());
        for (k, at) in [0x2c, 0x30, 0x34, 0x38].into_iter().enumerate() {
            word(0x10 + 4 * k, n.u32(at));
        }
        for (k, at) in [0x1c, 0x20, 0x24, 0x28].into_iter().enumerate() {
            word(0x20 + 4 * k, n.u32(at));
        }
        word(0x30, f32::from(n.u8(0)).to_bits());
        word(0x34, f32::from(n.u8(1)).to_bits());
        word(0x3c, f32::from(n.u8(2)).to_bits());
        word(0x40, n.u32(0x14));
        word(0x44, n.u32(0x18));
    }
    if let Some(n) = fields.random1 {
        let n = Res(n);
        for k in 0..3 {
            word(0x50 + 4 * k, n.u32(4 * k));
        }
        word(0x5c, (n.i32(0xc) as f32).to_bits());
    }
    if let Some(n) = fields.pos_add {
        let n = Res(n);
        for k in 0..3 {
            word(0x60 + 4 * k, n.u32(4 + 4 * k));
        }
        word(0x6c, f32::from(n.u8(0)).to_bits());
    }
    if let Some(n) = fields.magnet {
        let n = Res(n);
        word(0x70, n.u32(8));
        word(0x74, n.u32(0xc));
        word(0x78, n.u32(0x10));
        word(0x7c, n.u32(4));
        word(0x80, f32::from(n.u8(0)).to_bits());
    }
    if let Some(n) = fields.converge {
        let n = Res(n);
        for k in 0..4 {
            word(0x90 + 4 * k, n.u32(4 + 4 * k));
        }
        word(0xa0, f32::from(n.u8(0)).to_bits());
    }
    if let Some(n) = fields.spin {
        let n = Res(n);
        word(0xb0, n.u32(0));
        word(0xb4, n.u32(8));
        word(0xb8, (n.u32(4) as f32).to_bits());
    }
    if let Some(n) = fields.collision {
        let n = Res(n);
        let value = ((i32::from(n.u8(0)) << 1) as f32) + f32::from(n.u8(1));
        word(0xc0, value.to_bits());
        word(0xc4, n.u32(8));
        word(0xc8, n.u32(0x10));
        word(0xcc, n.u32(4));
    }
    if let Some(n) = fields.curl {
        let n = Res(n);
        for k in 0..4 {
            word(0xd0 + 4 * k, n.u32(0x10 + 4 * k));
        }
        for k in 0..3 {
            word(0xe0 + 4 * k, n.u32(4 + 4 * k));
        }
        word(0xec, n.u32(0x20));
        word(0xf0, f32::from(n.u8(1)).to_bits());
        word(0xf4, f32::from(n.u8(2)).to_bits());
    }
    if let Some(n) = fields.custom {
        let n = Res(n);
        for k in 0..8 {
            word(0x100 + 4 * k, n.u32(4 + 4 * k));
        }
    }
    Some(swap_words(&u))
}

/// An emitter animation node (`EA**`): `u8 enabled, u8 loop, u16, u32
/// key count, u32, keys {x, y, z, frame}` (`eft_EmitterAnim_Eval`).
pub fn emitter_anim(data: &[u8]) -> Option<fx::EmitterAnim> {
    let n = Res(data);
    if data.len() < 0xc {
        return None;
    }
    let count = n.u32(4) as usize;
    let keys = (0..count)
        .filter(|k| 0xc + 16 * k + 16 <= data.len())
        .map(|k| {
            let at = 0xc + 16 * k;
            fx::Key {
                value: n.v3(at),
                time: n.f32(at + 12),
            }
        })
        .collect();
    Some(fx::EmitterAnim {
        enabled: n.flag(0),
        looping: n.flag(1),
        keys,
    })
}

/// The CPU side's view of a patched resource.
pub fn emitter_params(d: &Res) -> fx::EmitterParams {
    fx::EmitterParams {
        visible: d.flag(0x750),
        sort: d.u8(0x751),
        calc: d.u8(0x752),
        follow: d.u8(0x753),
        fade_stops_emission: d.flag(0x754),
        fade_out_alpha: d.flag(0x755),
        fade_out_scale: d.flag(0x756),
        seed_mode: d.u8(0x757),
        rerandomize_matrix: d.flag(0x758),
        lod_every_frame: d.flag(0x759),
        lod_emission: d.flag(0x75a),
        fade_in_alpha: d.flag(0x75b),
        fade_in_scale: d.flag(0x75c),
        seed: d.u32(0x760),
        draw_path: d.u32(0x764),
        fade_out_frames: d.i32(0x768),
        fade_in_frames: d.i32(0x76c),
        translate: d.v3(0x770),
        translate_random: d.v3(0x77c),
        rotate: d.v3(0x788),
        rotate_random: d.v3(0x794),
        scale: d.v3(0x7a0),
        color0: [d.f32(0x7ac), d.f32(0x7b0), d.f32(0x7b4), d.f32(0x7b8)],
        color1: [d.f32(0x7bc), d.f32(0x7c0), d.f32(0x7c4), d.f32(0x7c8)],
        near: d.f32(0x7cc),
        far: d.f32(0x7d0),
        far_percent: d.i32(0x7d4),
        inherit: std::array::from_fn(|k| d.flag(0x7d8 + k)),
        draw_before_parent: d.flag(0x7e1),
        alpha0_with_parent: d.flag(0x7e2),
        alpha1_with_parent: d.flag(0x7e3),
        inherit_velocity_scale: d.f32(0x7e8),
        inherit_scale_scale: d.f32(0x7ec),
        one_time: d.flag(0x7f0),
        world_gravity: d.flag(0x7f1),
        by_distance: d.flag(0x7f2),
        world_direction: d.flag(0x7f3),
        start: d.i32(0x7f4),
        child_start_percent: d.i32(0x7f8),
        duration: d.i32(0x7fc),
        rate: d.f32(0x800),
        rate_random: d.i32(0x804),
        interval: d.i32(0x808),
        interval_random: d.i32(0x80c),
        position_random: d.f32(0x810),
        gravity_scale: d.f32(0x814),
        gravity: d.v3(0x818),
        distance_unit: d.f32(0x824),
        distance_min: d.f32(0x828),
        distance_max: d.f32(0x82c),
        distance_threshold: d.f32(0x830),
        volume: d.u8(0x838),
        random_start_angle: d.flag(0x839),
        latitude_mode: d.flag(0x83a),
        sphere_table: d.u8(0x83c),
        sphere64_count: d.u8(0x83d),
        latitude_axis: d.u8(0x83e),
        sweep: d.f32(0x840),
        latitude: d.f32(0x844),
        sweep_start: d.f32(0x848),
        division_angle_random: d.f32(0x84c),
        caliber: d.f32(0x850),
        line_center: d.f32(0x854),
        line_length: d.f32(0x858),
        volume_radius: d.v3(0x85c),
        form_scale: d.v3(0x868),
        division_mode: d.i32(0x874),
        shape_primitive: (d.u64(0x878) != u64::MAX).then(|| d.u64(0x878)),
        circle_divisions: d.i32(0x880),
        circle_division_random: d.i32(0x884),
        line_divisions: d.i32(0x888),
        line_division_random: d.i32(0x88c),
        blend: d.flag(0x898),
        depth_test: d.flag(0x899),
        depth_func: d.u8(0x89a),
        depth_write: d.flag(0x89b),
        alpha_test: d.flag(0x89c),
        alpha_func: d.u8(0x89d),
        blend_type: d.u8(0x89e),
        cull: d.u8(0x89f),
        alpha_ref: d.f32(0x8a0),
        infinite_life: d.flag(0x8a8),
        billboard: d.u8(0x8aa),
        rotation_order: d.u8(0x8ab),
        life: d.i32(0x8b8),
        life_random: d.i32(0x8bc),
        speed_random: d.f32(0x8c0),
        primitive: (d.u64(0x8c8) != u64::MAX).then(|| d.u64(0x8c8)),
        omni_velocity: d.f32(0x96c),
        directional_velocity: d.f32(0x970),
        direction: d.v3(0x974),
        diffusion_angle: d.f32(0x980),
        xz_diffusion: d.f32(0x984),
        velocity_random_axes: d.v3(0x988),
        velocity_random: d.f32(0x994),
        emitter_velocity_inherit: d.f32(0x998),
        particle_scale: d.v3(0x9c8),
        scale_random: d.v3(0x9d4),
        air_resistance: d.f32(0xc0),
        color_sources: [d.u8(0x9a4), d.u8(0x9a5), d.u8(0x9a6), d.u8(0x9a7)],
        combiner: std::array::from_fn(|k| d.u8(0x8f8 + k)),
        texture_squared: std::array::from_fn(|s| d.u8(0x9f8 + 0x20 * s + 0x17) == 1),
        custom_shader: d.u32(0x92c),
        custom_switches: [d.u32(0x930), d.u32(0x934)],
        area_loop: None,
        custom_params: Vec::new(),
        vertex_program: String::new(),
        pixel_program: String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_fills_constants_and_pads_keys() {
        let mut res = vec![0u8; SIZE];
        // Colour0 constant (source 0), alpha0 animated with two keys.
        put_f32(&mut res, 0x9a8, 0.5);
        res[0x9a6] = 2;
        put_u32(&mut res, 0x64, 2);
        put_f32(&mut res, 0x440, 0.25);
        put_f32(&mut res, 0x450, 0.75);
        put_f32(&mut res, 0x45c, 1.0);
        // A looping scale track.
        res[0x8dc] = 1;
        put_u32(&mut res, 0x8f4, 40);
        res[0x838] = 4; // sphere
        res[0x753] = 1;
        let d = setup(&res, &Fields::default(), None);
        let r = Res(&d);
        assert_eq!(r.f32(0x3c0), 0.5);
        for k in 2..8 {
            assert_eq!(r.f32(0x440 + 0x10 * k), 0.75);
            assert_eq!(r.f32(0x44c + 0x10 * k), 1.0);
        }
        assert_eq!(r.f32(0x90), 40.0);
        assert_eq!(r.i32(0x874), -1);
        // Scale wave 0 (sine) and the follow type.
        assert_eq!(r.u32(0x50) & 0xf, 1);
        assert_eq!(r.u32(0x54), 0x800);
        // The block the GPU gets is little-endian.
        let block = static_block(&d);
        assert_eq!(block.len(), 0x750);
        let at = 0x3c0;
        assert_eq!(
            f32::from_le_bytes(block[at..at + 4].try_into().unwrap()),
            0.5
        );
    }
}

/// The `EP04` node's payload (0x50 bytes, `0x03b68820`).
pub fn area_loop(data: &[u8]) -> Option<fx::AreaLoop> {
    if data.len() < 0x4c {
        return None;
    }
    let n = Res(data);
    Some(fx::AreaLoop {
        step: n.v3(0x00),
        extra_draws: n.f32(0x0c),
        half_size: n.v3(0x10),
        cut_height: n.f32(0x1c),
        centre: n.v3(0x20),
        cut_mode: n.i32(0x2c),
        fade: n.v3(0x30),
        follows_camera: n.f32(0x3c) != 0.0,
        rotation: n.v3(0x40),
    })
}

/// The `CSDP` node as the reserved block: 0x80 bytes (`0x03b646f4`), the
/// payload's words, zero past it.
pub fn custom_params(data: &[u8]) -> Vec<f32> {
    (0..32)
        .map(|k| {
            data.get(4 * k..4 * k + 4)
                .map_or(0.0, |w| f32::from_be_bytes(w.try_into().unwrap()))
        })
        .collect()
}

/// SHA-1 of `data` as 40 hex digits: the name the research gives a shader
/// program (docs/research/eft-shaders.md, `tools/research/eft_shaders.py`).
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[4 * i..4 * i + 4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// The vertex and pixel shader programs of a PTCL file's GFX2 file
/// (`SHDA` → `SHDB`, docs/research/eft-shaders.md §1): each program's
/// SHA-1 over its code, in file order (an emitter's `0x914`/`0x918` index
/// these lists).
pub fn program_hashes(gfx2: &[u8]) -> (Vec<String>, Vec<String>) {
    const VS_HEADER: u32 = 3;
    const VS_PROGRAM: u32 = 5;
    const PS_HEADER: u32 = 6;
    const PS_PROGRAM: u32 = 7;
    const END: u32 = 1;
    let r = Res(gfx2);
    if gfx2.len() < 0x20 || &gfx2[..4] != b"Gfx2" {
        return (Vec::new(), Vec::new());
    }
    let mut at = r.u32(4) as usize;
    let (mut vs_sizes, mut ps_sizes) = (Vec::new(), Vec::new());
    let (mut vs_code, mut ps_code) = (Vec::new(), Vec::new());
    while at + 0x20 <= gfx2.len() {
        if &gfx2[at..at + 4] != b"BLK{" {
            break;
        }
        let header = r.u32(at + 4) as usize;
        let kind = r.u32(at + 0x10);
        let size = r.u32(at + 0x14) as usize;
        let data = at + header;
        let Some(block) = gfx2.get(data..data + size) else {
            break;
        };
        match kind {
            VS_HEADER if size >= 0xD4 => vs_sizes.push(Res(block).u32(0xD0) as usize),
            PS_HEADER if size >= 0xA8 => ps_sizes.push(Res(block).u32(0xA4) as usize),
            VS_PROGRAM => vs_code.push(block),
            PS_PROGRAM => ps_code.push(block),
            END => break,
            _ => {}
        }
        at = data + size;
    }
    let hash = |sizes: &[usize], codes: &[&[u8]]| {
        codes
            .iter()
            .zip(sizes)
            .map(|(code, &size)| sha1_hex(&code[..size.min(code.len())]))
            .collect()
    };
    (hash(&vs_sizes, &vs_code), hash(&ps_sizes, &ps_code))
}

#[cfg(test)]
mod sha1_tests {
    #[test]
    fn sha1_of_abc() {
        assert_eq!(
            super::sha1_hex(b"abc"),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            super::sha1_hex(b""),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }
}
