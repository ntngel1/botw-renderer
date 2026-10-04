// The effect library's particle shaders (NintendoWare eft2 in BotW, Wii U
// v208) as one shader: what docs/research/eft-shaders.md reads from the
// game's programs. Each game program is the über-shader compiled for one
// emitter; here the switches are the emitter's bytes (`DrawParams`) and its
// static uniform block (`sysEmitterStaticUniformBlock`, the emitter's
// patched bytes 0…0x74F), read at the same offsets as the game's code.
//
// One draw per emitter: vertex `v` is corner `v % 4` (or vertex `v % n` of
// the particle's primitive) of particle `v / 4` (`/ n`).

#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::prepass_utils
#import botw::look::{read_look, read_look_at, apply_haze, sky_cover}
#import botw::deferred_light::{field_ambient, cube_at, cube, cube_mean}

struct Particle {
    local_pos: vec4<f32>,
    local_vec: vec4<f32>,
    local_diff: vec4<f32>,
    scale: vec4<f32>,
    random: vec4<f32>,
    init_rotate: vec4<f32>,
    color0: vec4<f32>,
    color1: vec4<f32>,
    emt_mat: array<vec4<f32>, 3>,
    emt_rt_mat: array<vec4<f32>, 3>,
}

// `sysEmitterDynamicUniformBlock`.
struct Dynamic {
    color0: vec4<f32>,
    color1: vec4<f32>,
    // Emitter frame, 1, 1, frame step.
    frame: vec4<f32>,
    // Alpha, particle scale xyz.
    alpha_scale: vec4<f32>,
    srt: array<vec4<f32>, 4>,
    rt: array<vec4<f32>, 4>,
    // Ours: live particles (x), particles per copy (y), copies (z),
    // BotW's custom switches F1 (w).
    count: vec4<u32>,
    // The area loop's box (plugin block, as column matrices).
    area: mat4x4<f32>,
    area_inverse: mat4x4<f32>,
    step: vec4<f32>,
    fade: vec4<f32>,
    half_size: vec4<f32>,
    cut: vec4<f32>,
    // `sysCustomShaderReservedUniformBlockParam` (the `CSDP` node).
    reserved: array<vec4<f32>, 8>,
}

// The emitter's bytes beyond the static block that pick code paths.
struct DrawParams {
    // calc type, follow type, billboard, rotation order (0x8ab).
    kind: vec4<u32>,
    // Colour sources: colour0, colour1, alpha0, alpha1 (0 constant, 2
    // animated, 3 random key).
    sources: vec4<u32>,
    // Combiner bytes 0x8f8…0x909 (18), four to a vector.
    combiner: array<vec4<u32>, 5>,
    // Texture slots present (x), distance-size mode (y), squared (bits of w).
    textures: vec4<u32>,
    // Vertices per particle (4: the quad), custom shader index, alpha
    // test on (1), the ported program family (0 standard, 1 clouds).
    shape: vec4<u32>,
    // Each slot's component selection (a byte per channel).
    swizzles: vec4<u32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage, read> st: array<vec4<u32>>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<storage, read> particles: array<Particle>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<storage, read> dyn: Dynamic;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> draw: DrawParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var tex0: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var smp0: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var tex1: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var smp1: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var tex2: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var smp2: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var look_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var cube_means: texture_2d<f32>;

// --- The static block ---

fn su(off: u32) -> u32 {
    return st[off / 16u][(off / 4u) % 4u];
}
fn sf(off: u32) -> f32 {
    return bitcast<f32>(su(off));
}
fn si(off: u32) -> i32 {
    return bitcast<i32>(su(off));
}
fn sv2(off: u32) -> vec2<f32> {
    return vec2<f32>(sf(off), sf(off + 4u));
}
fn sv3(off: u32) -> vec3<f32> {
    return vec3<f32>(sf(off), sf(off + 4u), sf(off + 8u));
}
fn sv4(off: u32) -> vec4<f32> {
    return vec4<f32>(sf(off), sf(off + 4u), sf(off + 8u), sf(off + 12u));
}
fn flags_a() -> u32 {
    return su(0x50u);
}
fn combiner(i: u32) -> u32 {
    return draw.combiner[i / 4u][i % 4u];
}

// --- Animation tracks (§5.3) ---

// The time of track k (0 colour0, 1 alpha0, 2 colour1, 3 alpha1, 4 scale):
// its loop (period at 0x80 + 4k, random start at 0x94 + 4k) or life.
fn track_time(k: u32, age: f32, life: f32, r: f32) -> f32 {
    let period = sf(0x80u + 4u * k);
    if period > 0.0 {
        return fract(r * sf(0x94u + 4u * k) + age / period);
    }
    return age / life;
}

// Eight keys (xyz, time) at `table`, `count` of them (count 0: 1).
fn keys(table: u32, count: i32, t: f32) -> vec3<f32> {
    if count <= 0 {
        return vec3<f32>(1.0);
    }
    var value = sv3(table);
    for (var i = 0; i < 7; i += 1) {
        if i + 1 >= count {
            break;
        }
        let a = sv4(table + 16u * u32(i));
        let b = sv4(table + 16u * u32(i + 1));
        if t >= a.w {
            let span = b.w - a.w;
            let f = select(1.0, saturate((t - a.w) / span), span > 0.0);
            value = mix(a.xyz, b.xyz, f);
        }
    }
    return value;
}

// A colour track by its source: 0 constant (key 0), 2 animated, 3 a
// random key per particle.
fn color_track(source: u32, table: u32, count_off: u32, k: u32, age: f32, life: f32, r: vec4<f32>) -> vec3<f32> {
    let count = si(count_off);
    if source == 3u {
        let n = max(count, 1);
        let i = min(i32(min(r.x, 0.999999) * f32(n)), n - 1);
        return sv3(table + 16u * u32(i));
    }
    if source == 2u {
        return keys(table, count, track_time(k, age, life, r.x));
    }
    return sv3(table);
}

// --- Rotation (§5.5) ---

fn rot_x(a: f32) -> mat3x3<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat3x3<f32>(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, c, s), vec3<f32>(0.0, -s, c));
}
fn rot_y(a: f32) -> mat3x3<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat3x3<f32>(vec3<f32>(c, 0.0, -s), vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(s, 0.0, c));
}
fn rot_z(a: f32) -> mat3x3<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat3x3<f32>(vec3<f32>(c, s, 0.0), vec3<f32>(-s, c, 0.0), vec3<f32>(0.0, 0.0, 1.0));
}

fn wrap_pi(a: f32) -> f32 {
    let two_pi = 6.2831853;
    return a - two_pi * floor((a + 3.14159265) / two_pi);
}

fn rotation_angles(p: Particle, age: f32) -> vec3<f32> {
    let r = p.random;
    let flags = flags_a();
    let flip_rand = vec3<f32>(r.y, r.z, r.x);
    let own = vec3<f32>(r.x, r.y, r.z);
    let spread = vec3<f32>((r.x + r.y) * 0.5, (r.y + r.z) * 0.5, (r.x + r.z) * 0.5);
    let rr = sf(0x72cu);
    var h = age;
    if rr != 1.0 {
        h = (1.0 - pow(rr, age)) / (1.0 - rr);
    }
    var angles = vec3<f32>(0.0);
    for (var a = 0u; a < 3u; a += 1u) {
        var s = 1.0;
        if (flags & (0x10000u << a)) != 0u && floor(flip_rand[a] * 2.0) > 0.0 {
            s = -1.0;
        }
        let w = (sf(0x720u + 4u * a) + (2.0 * spread[a] - 1.0) * sf(0x730u + 4u * a)) * s;
        let theta = p.init_rotate[a] * s + (own[a] - 0.5) * sf(0x710u + 4u * a) + w * h;
        angles[a] = wrap_pi(theta);
    }
    return angles;
}

fn rotation_matrix(order: u32, a: vec3<f32>) -> mat3x3<f32> {
    if order == 5u {
        return rot_z(a.z) * rot_y(a.y) * rot_x(a.x);
    }
    if order == 6u {
        return rot_y(a.y) * rot_x(a.x) * rot_z(a.z);
    }
    return rot_x(a.x) * rot_z(a.z) * rot_y(a.y);
}

// --- The emitter's matrices ---

fn mat_point(m: array<vec4<f32>, 3>, p: vec3<f32>) -> vec3<f32> {
    let h = vec4<f32>(p, 1.0);
    return vec3<f32>(dot(m[0], h), dot(m[1], h), dot(m[2], h));
}
fn mat_dir(m: array<vec4<f32>, 3>, v: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(m[0].xyz, v), dot(m[1].xyz, v), dot(m[2].xyz, v));
}
fn dyn_srt() -> array<vec4<f32>, 3> {
    return array<vec4<f32>, 3>(dyn.srt[0], dyn.srt[1], dyn.srt[2]);
}
fn dyn_rt() -> array<vec4<f32>, 3> {
    return array<vec4<f32>, 3>(dyn.rt[0], dyn.rt[1], dyn.rt[2]);
}

// --- UV (§5.8) ---

fn pattern_cell(s: u32, age: f32, life: f32, r: f32) -> i32 {
    let base = 0x110u + 0x90u * s;
    let n = max(si(base), 1);
    let frames = max(si(base + 4u), 1);
    let mode = (flags_a() >> (4u + 4u * s)) & 0xfu;
    let i = i32(age / f32(frames));
    var cell = 0;
    if (mode & 1u) != 0u {
        cell = i32(age / life * f32(n));
    } else if (mode & 2u) != 0u {
        cell = select(i % n, n - 1, i >= n);
    } else if (mode & 4u) != 0u {
        var start = 0;
        if (flags_a() & (0x2000000u << s)) != 0u {
            start = i32(r * f32(n));
        }
        cell = i + start;
    } else if (mode & 8u) != 0u {
        cell = i32(r * f32(n));
    } else {
        return 0;
    }
    cell = ((cell % n) + n) % n;
    return si(base + 0x10u + 4u * u32(cell));
}

fn slot_uv(s: u32, corner: vec2<f32>, age: f32, life: f32, r: vec4<f32>) -> vec2<f32> {
    let base = 0x2c0u + 0x50u * s;
    let flags = flags_a();
    var x = corner.x;
    var y = corner.y;
    // Random flips of U and V.
    let flip_bits = select(select(0x800000u, 0x200000u, s == 1u), 0x80000u, s == 0u);
    if (flags & flip_bits) != 0u && r.y > 0.5 {
        x = -x;
    }
    if (flags & (flip_bits << 1u)) != 0u && r.z > 0.5 {
        y = -y;
    }
    let div = max(sv2(base + 0x48u), vec2<f32>(1.0));
    var cell = 0;
    if s < 2u {
        cell = pattern_cell(s, age, life, r.w);
    }
    let col = f32(cell % i32(div.x));
    let row = f32(cell / i32(div.x));
    let rep = sv2(base + 0x40u) / div;
    // SI-EFX-20: which random component each slot's animation takes (the
    // game switches it by `0x54 & 1`) is taken as x.
    let rnd = r.x;
    let scale = age * sv2(base + 0x18u) + rnd * sv2(base + 0x28u) + sv2(base + 0x20u) + sv2(base + 0x28u);
    let scroll = age * sv2(base + 0x00u) + sv2(base + 0x08u) + sv2(base + 0x10u) * (1.0 - 2.0 * rnd);
    let u = x * (scale.x - 1.0) + rep.x * (x + 0.5 + col) - scroll.x;
    let v = y * (1.0 - scale.y) - rep.y * (y - 0.5 - row) - scroll.y;
    return vec2<f32>(u, v);
}

struct Vertex {
    @builtin(vertex_index) index: u32,
    // The primitive's vertex (or the quad corner).
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    // The particle (x) and the vertex in its shape (y).
    @location(4) owner: vec2<f32>,
}

struct Varyings {
    @builtin(position) clip: vec4<f32>,
    @location(0) color0: vec4<f32>,
    @location(1) color1: vec4<f32>,
    @location(2) uv01: vec4<f32>,
    @location(3) uv2: vec2<f32>,
    // Alpha factor, param track, view depth.
    @location(4) factors: vec4<f32>,
    @location(5) world: vec3<f32>,
    @location(6) vertex_color: vec4<f32>,
    // The shape's normal, turned with the particle (custom shader 3's cube
    // light).
    @location(7) normal: vec3<f32>,
}

fn dead() -> Varyings {
    var out: Varyings;
    out.clip = vec4<f32>(0.0, 0.0, 2.0, 1.0);
    return out;
}

// The emitter's RT as the polygon billboards use it: column j over the
// length of row j (zero when that row is; `vs_c3ba42e73198`,
// `vs_032ddc41a257`).
fn rt_columns(m: array<vec4<f32>, 3>) -> mat3x3<f32> {
    let l = vec3<f32>(length(m[0].xyz), length(m[1].xyz), length(m[2].xyz));
    let k = select(vec3<f32>(0.0), 1.0 / l, l > vec3<f32>(0.0));
    return mat3x3<f32>(
        vec3<f32>(m[0].x, m[1].x, m[2].x) * k.x,
        vec3<f32>(m[0].y, m[1].y, m[2].y) * k.y,
        vec3<f32>(m[0].z, m[1].z, m[2].z) * k.z,
    );
}

@vertex
fn vertex(in: Vertex) -> Varyings {
    let owner = u32(in.owner.x + 0.5);
    let per_copy = max(dyn.count.y, 1u);
    let copy = owner / per_copy;
    let index = owner % per_copy;
    if index >= dyn.count.x {
        return dead();
    }
    let p = particles[index];
    let frame = dyn.frame.x;
    let age = frame - p.local_vec.w;
    let life = p.local_pos.w;
    if age < 0.0 || age >= f32(i32(life)) {
        return dead();
    }
    let r = p.random;
    let calc = draw.kind.x;
    let follow = draw.kind.y;
    let billboard = draw.kind.z;

    // Position (§5.4).
    var local = p.local_pos.xyz;
    var velocity = p.local_vec.xyz;
    // Calc 2 (stream-out): the attributes hold the program's output
    // (`sysInPos`/`sysInVec`), run on the CPU each frame (effects/stream_out.rs).
    if calc == 1u {
        let t = age + dyn.frame.w;
        let g = sv3(0xb0u) * sf(0xbcu);
        let rr = sf(0xc0u);
        var gt = g * t * t * 0.5;
        var vt = t;
        if rr != 1.0 {
            let rt = pow(rr, t);
            gt = g * (t - (rt - 1.0) / log(rr)) / (1.0 - rr);
            vt = (1.0 - rt) / (1.0 - rr);
        }
        local = p.local_pos.xyz + p.scale.w * (gt + p.local_vec.xyz * vt);
        velocity = p.local_vec.xyz * pow(max(rr, 1e-6), t) + g * t;
    }
    var mat = dyn_srt();
    var rt_mat = dyn_rt();
    if follow != 0u {
        mat = p.emt_mat;
        rt_mat = p.emt_rt_mat;
        if follow == 2u {
            mat[0].w = dyn.srt[0].w;
            mat[1].w = dyn.srt[1].w;
            mat[2].w = dyn.srt[2].w;
        }
    }
    var center = mat_point(mat, local);

    // The area loop (plugin 4, eft-shaders §5.10, eft-custom-blocks §2):
    // copy k shifted by k·step, wrapped into the box, faded at its edges,
    // cut above or below a height.
    var area_fade = 1.0;
    if dyn.count.z > 0u && dyn.half_size.x > 0.0 {
        let s = dyn.half_size.xyz;
        let box_local = (dyn.area_inverse * vec4<f32>(center + dyn.step.xyz * f32(copy), 1.0)).xyz;
        let f = fract(box_local / s / 2.0 + 0.5);
        center = (dyn.area * vec4<f32>(s * (2.0 * f - 1.0), 1.0)).xyz;
        for (var i = 0; i < 3; i += 1) {
            if dyn.fade[i] > 0.0 {
                area_fade *= saturate((1.0 - abs(2.0 * f[i] - 1.0)) / dyn.fade[i]);
            }
        }
        let mode = i32(dyn.half_size.w + 0.5);
        if (mode == 1 && center.y > dyn.cut.x) || (mode == 2 && center.y < dyn.cut.x) {
            return dead();
        }
    }
    // Custom shader 3 with the sky's cover map (F1 & 0x400): no particle
    // under a roof (`cus1[0x20C]·(1 − depth) > y`).
    if draw.shape.y == 3u && (dyn.count.w & 0x400u) != 0u {
        let look = read_look(look_texture);
        if sky_cover(look_texture, look, center) > center.y {
            return dead();
        }
    }

    // Tracks.
    let c0 = color_track(draw.sources.x, 0x3c0u, 0x60u, 0u, age, life, r);
    let a0 = color_track(draw.sources.z, 0x440u, 0x64u, 1u, age, life, r).x;
    let c1 = color_track(draw.sources.y, 0x4c0u, 0x68u, 2u, age, life, r);
    let a1 = color_track(draw.sources.w, 0x540u, 0x6cu, 3u, age, life, r).x;
    let scale_keys = keys(0x600u, si(0x70u), track_time(4u, age, life, r.x));
    let param = keys(0x680u, si(0x74u), age / life);
    let color_scale = sf(0x3b0u);

    // The local vertex, sized and rotated.
    let size = p.scale.xy * scale_keys.xy * dyn.alpha_scale.yz;
    let corner = in.position.xy;
    var v = vec3<f32>(
        size.x * (corner.x + sf(0xd0u) * 0.5),
        size.y * (corner.y + sf(0xd4u) * 0.5),
        // Meshes: z sized too (every mesh program: Pos.z·Scale.z·keys.z·dyn 0x03C).
        in.position.z * p.scale.z * scale_keys.z * dyn.alpha_scale.w,
    );

    var angles = rotation_angles(p, age);
    let eye = view.world_position;
    if billboard == 2u {
        // Y billboard (`vs_e6cc513b8fef`): the angle atan2(c.x − eye.x,
        // eye.z − c.z) taken off the Y angle, i.e. the yaw to the eye added.
        let to_eye = eye - center;
        angles.y = angles.y + atan2(to_eye.x, to_eye.z);
    } else if billboard == 7u {
        // Y billboard by the camera's direction (`vs_558bccd44820`): the
        // yaw of the view block's 0x100 (the view matrix's row 2, which
        // points to the eye side) added to the Y angle.
        let back = view.world_from_view[2].xyz;
        angles.y = angles.y + atan2(back.x, back.z);
    }
    let rot = rotation_matrix(draw.kind.w, angles);
    v = rot * v;
    var n = rot * in.normal;

    var world: vec3<f32>;
    if billboard == 0u {
        // Camera-aligned: the view block's billboard matrix, the camera's
        // rotation.
        let b = mat3x3<f32>(view.world_from_view[0].xyz, view.world_from_view[1].xyz, view.world_from_view[2].xyz);
        world = center + b * v;
        n = b * n;
    } else if billboard == 1u {
        let f = normalize(eye - center);
        let up = view.world_from_view[1].xyz;
        let x = normalize(cross(up, f));
        let y = cross(f, x);
        world = center + x * v.x + y * v.y + f * v.z;
    } else if billboard == 2u || billboard == 7u {
        world = center + v;
    } else if billboard == 4u {
        // A polygon in the emitter's XZ plane (`vs_c3ba42e73198`): the
        // vertex's (x, y, z) goes to (x, z, −y).
        // SI-EFX-22: that program has no rotation; the rotation is taken
        // to come before the swap, as in the other billboards.
        let basis = rt_columns(rt_mat);
        world = center + basis * vec3<f32>(v.x, v.z, -v.y);
        n = basis * vec3<f32>(n.x, n.z, -n.y);
    } else if billboard == 6u {
        // Along the motion in the emitter's frame (`vs_032ddc41a257`):
        // y = the motion, x = normalize(y × RT column 1) (nudged by
        // 0.001·column 2 when the cross's x is exactly 0), z = x × y.
        // SI-EFX-22: the program's motion is p(age + Δ) − p(age) (calc 1);
        // the velocity stands for it, the shift since birth when it is
        // under 0.001.
        var d = mat_dir(mat, velocity);
        if length(d) < 0.001 {
            d = mat_dir(mat, local - p.local_pos.xyz);
        }
        if length(d) == 0.0 {
            d = vec3<f32>(0.0, 1.0, 0.0);
        }
        d = normalize(d);
        let c = rt_columns(rt_mat);
        var c1 = c[1];
        if -d.y * c1.z + c1.y * d.z == 0.0 {
            c1 = c1 + c[2] * 0.001;
        }
        let x = normalize(cross(d, c1));
        let basis = mat3x3<f32>(x, d, cross(x, d));
        world = center + basis * v;
        n = basis * n;
    } else if billboard == 3u {
        // A polygon in the emitter's plane.
        let m = array<vec4<f32>, 3>(
            vec4<f32>(normalize(vec3<f32>(rt_mat[0].x, rt_mat[1].x, rt_mat[2].x)), 0.0),
            vec4<f32>(normalize(vec3<f32>(rt_mat[0].y, rt_mat[1].y, rt_mat[2].y)), 0.0),
            vec4<f32>(normalize(vec3<f32>(rt_mat[0].z, rt_mat[1].z, rt_mat[2].z)), 0.0),
        );
        let basis = mat3x3<f32>(m[0].xyz, m[1].xyz, m[2].xyz);
        world = center + basis * v;
        n = basis * n;
    } else {
        // 5: along the motion.
        var d = mat_dir(mat, velocity);
        if length(d) < 0.001 {
            d = vec3<f32>(0.0, 1.0, 0.0);
        }
        d = normalize(d);
        let look_dir = normalize(center - eye);
        var x = cross(d, look_dir);
        if length(x) < 1e-5 {
            x = vec3<f32>(1.0, 0.0, 0.0);
        }
        x = normalize(x);
        world = center + x * v.x + d * v.y + cross(x, d) * v.z;
    }

    // Stretched along the motion (`0x5F0`): the side of the shape facing
    // the motion moves with the velocity.
    let stretch = sf(0x5f0u);
    if stretch != 0.0 {
        let off = world - center;
        let vel = mat_dir(mat, velocity);
        if length(off) > 0.0 && length(vel) > 0.0 {
            world += dot(normalize(vel), normalize(off)) * vel * stretch;
        }
    }

    // Size by camera distance (§5.7): only programs compiled with this
    // operation read these uniforms. Other programs retain 50/50 defaults
    // without using them (e.g. lava haze and the volcano's clouds).
    let size_mode = draw.textures.y;
    let near_size = sf(0x740u);
    let far_size = sf(0x744u);
    if size_mode != 0u {
        let dist = length(center - eye);
        var k = 1.0;
        if (size_mode & 1u) != 0u && near_size > 0.0 {
            k = min(dist, near_size) / near_size;
        }
        if (size_mode & 2u) != 0u && far_size > 0.0 {
            if size_mode == 2u || dist > near_size {
                k = max(dist, far_size) / far_size;
            }
        }
        world = center + (world - center) * k;
    }

    var out: Varyings;
    out.clip = view.clip_from_world * vec4<f32>(world, 1.0);
    out.world = world;
    let z = -(view.view_from_world * vec4<f32>(world, 1.0)).z;

    // Alpha factors (§5.9).
    var factor = dyn.alpha_scale.x;
    let na = sv2(0x5d0u);
    if na.y > na.x {
        factor *= saturate((z - na.x) / (na.y - na.x));
    }
    let fa = sv2(0x5d8u);
    if fa.y > fa.x {
        factor *= 1.0 - saturate((z - fa.x) / (fa.y - fa.x));
    }
    factor *= area_fade;
    out.factors = vec4<f32>(factor, param.x, z, 0.0);
    out.color0 = vec4<f32>(c0 * dyn.color0.rgb * color_scale * p.color0.rgb, a0 * dyn.color0.a * p.color0.a);
    out.color1 = vec4<f32>(c1 * dyn.color1.rgb * color_scale * p.color1.rgb, a1 * dyn.color1.a * p.color1.a);
    out.uv01 = vec4<f32>(slot_uv(0u, corner, age, life, r), slot_uv(1u, corner, age, life, r));
    out.uv2 = slot_uv(2u, corner, age, life, r);
    out.vertex_color = in.color;
    out.normal = n;

    return out;
}

// --- Pixel shader (§8) ---

fn sample_slot(s: u32, uv: vec2<f32>) -> vec4<f32> {
    if ((draw.textures.x >> s) & 1u) == 0u {
        return vec4<f32>(1.0);
    }
    var t: vec4<f32>;
    if s == 0u {
        t = textureSample(tex0, smp0, uv);
    } else if s == 1u {
        t = textureSample(tex1, smp1, uv);
    } else {
        t = textureSample(tex2, smp2, uv);
    }
    // The game's component selection (GX2 `compSel`).
    let sw = draw.swizzles[s];
    var o: vec4<f32>;
    for (var c = 0u; c < 4u; c += 1u) {
        let src = (sw >> (8u * c)) & 0xffu;
        if src < 4u {
            o[c] = t[src];
        } else {
            o[c] = select(0.0, 1.0, src == 5u);
        }
    }
    t = o;
    if ((draw.textures.w >> s) & 1u) != 0u {
        t = vec4<f32>(t.rgb * t.rgb, t.a);
    }
    return t;
}

// A slot's texel with the component selection, not squared.
fn sample_raw(s: u32, uv: vec2<f32>) -> vec4<f32> {
    var t: vec4<f32>;
    if s == 0u {
        t = textureSample(tex0, smp0, uv);
    } else if s == 1u {
        t = textureSample(tex1, smp1, uv);
    } else {
        t = textureSample(tex2, smp2, uv);
    }
    let sw = draw.swizzles[s];
    var o: vec4<f32>;
    for (var c = 0u; c < 4u; c += 1u) {
        let src = (sw >> (8u * c)) & 0xffu;
        if src < 4u {
            o[c] = t[src];
        } else {
            o[c] = select(0.0, 1.0, src == 5u);
        }
    }
    return o;
}

// The analyzer's effect texels 8 and 9 (`light_analyzer` step 4,
// docs/research/eft-custom-blocks.md §9): the cube map ahead and up or
// down, through the effect saturation curve, times the field's ambient
// scale.
const EFFECT_SAT: f32 = 2.0;
const FIELD_OFFSET_MIN: f32 = 0.1;
const FIELD_OFFSET_MAX: f32 = 0.5;
const FIELD_SCALE: f32 = 1.0;
// SI-LGT-07 (shared with the field): the frame's sunlit share.
const SUNLIT_SHARE: f32 = 0.85;

fn effect_texel(dir: vec3<f32>) -> vec3<f32> {
    let c = cube(normalize(dir));
    let m = max(c.r, max(c.g, c.b));
    let s = 1.0 - min(c.r, min(c.g, c.b)) / (m + 1e-10);
    let f = pow(s + 1e-4, EFFECT_SAT - 1.0);
    let e = SUNLIT_SHARE * SUNLIT_SHARE * SUNLIT_SHARE;
    let mean = cube_mean(cube_means);
    return (m + (c - m) * f) * FIELD_SCALE / (mean + mix(FIELD_OFFSET_MIN, FIELD_OFFSET_MAX, e));
}

// The custom shader 3 ambient from the analyzer's table: the clouds'
// programs between its sky up and down (texels 2, 3) by the height on
// screen; the rain's between the effect texels 8 and 9 by the param track
// times `reserved[0x30]`.
fn custom_light(in: Varyings) -> vec3<f32> {
    if draw.swizzles.w == 1u {
        let forward = -view.world_from_view[2].xyz;
        let ahead = vec3<f32>(forward.x, 0.0, forward.z);
        let t = 1.0 - saturate(in.factors.y * dyn.reserved[3].x * 0.25 + 0.5);
        return mix(effect_texel(ahead + vec3<f32>(0.0, 1.0, 0.0)), effect_texel(ahead - vec3<f32>(0.0, 1.0, 0.0)), t);
    }
    let amb = field_ambient(cube_means);
    let screen_v = in.clip.y / view.viewport.w;
    let u = 1.0 - saturate((1.0 - screen_v) * 0.7 + 0.15);
    return mix(amb.up, amb.down, u);
}

// The cloud program (`ps_991ea4f8496e`): texture 0's red and alpha,
// weighted by texture 2's alpha and the param track, move texture 1 and a
// second read of texture 0; colour c1 + (c0 − c1)·t1²; alpha
// a1·t1.a·sat(t0′.a + 2·a0 − 1); light the analyzer's plus the cube map's
// along the normal; soft against the scene over 100 m, nearer for faces
// seen edge on.
fn cloud_program(in: Varyings) -> vec4<f32> {
    let t0 = sample_raw(0u, in.uv01.xy);
    let t2a = sample_raw(2u, in.uv2).a;
    let param = in.factors.y;
    let shift = vec2<f32>(param * (t0.r * 2.0 - 1.0) * t2a, param * (t0.a * 2.0 - 1.0) * t2a);
    let t1 = sample_raw(1u, in.uv01.zw + shift);
    let t0b = sample_raw(0u, in.uv2 + shift).a;
    var light = custom_light(in);
    // SI-EFX-26: the scene's cube map stands for drawCtx+0x18[CSDP[0x28]].
    light += dyn.reserved[0].y * cube_at(normalize(in.normal), 0.0);
    let c0 = in.color0;
    let c1 = in.color1;
    var color = ((c0.rgb - c1.rgb) * t1.rgb * t1.rgb + c1.rgb) * light;
    let a = c1.a * t1.a * saturate(t0b + c0.a * 2.0 - 1.0);
    var soft = 1.0;
#ifdef DEPTH_PREPASS
    let depth = prepass_utils::prepass_depth(in.clip, 0u);
    let scene_z = view.clip_from_view[3][2] / max(depth, 1e-7);
    let n_view = (view.view_from_world * vec4<f32>(in.normal, 0.0)).xyz;
    let facing = n_view.z / max(length(n_view), 1e-6);
    let edge = 1.0 - (clamp(abs(facing), 0.3, 1.0) - 0.3) * 1.428571;
    let near_z = in.factors.z + 100.0 * edge * edge;
    soft = saturate(clamp(scene_z - near_z, 0.0, 100.0) * 0.01);
#endif
    let alpha = in.factors.x * saturate(a * soft);
    let look = read_look_at(look_texture, in.world);
    let view_dir = normalize(in.world - view.world_position);
    color = apply_haze(look, color, in.world, view_dir);
    return vec4<f32>(color, alpha);
}

fn input_color(kind: u32, t: vec3<f32>) -> vec3<f32> {
    if kind == 1u {
        return vec3<f32>(1.0);
    }
    if kind == 2u {
        return 1.0 - t;
    }
    return t;
}

fn input_alpha(kind: u32, a: f32) -> f32 {
    if kind == 2u {
        return 1.0;
    }
    if kind == 3u {
        return 1.0 - a;
    }
    return a;
}

fn blend_color(mode: u32, a: vec3<f32>, b: vec3<f32>) -> vec3<f32> {
    if mode == 1u {
        return a + b;
    }
    if mode == 2u {
        return a - b;
    }
    return a * b;
}

fn blend_alpha(mode: u32, a: f32, b: f32) -> f32 {
    if mode == 1u {
        return a + b;
    }
    if mode == 2u {
        return a - b;
    }
    return a * b;
}

@fragment
fn fragment(in: Varyings) -> @location(0) vec4<f32> {
    if draw.shape.w == 1u {
        let out = cloud_program(in);
        if draw.shape.z != 0u && out.a <= bitcast<f32>(draw.combiner[4].w) {
            discard;
        }
        return out;
    }
    var uv1 = in.uv01.zw;
    var uv2 = in.uv2;
    let t0 = sample_slot(0u, in.uv01.xy);
    // Shader type 2: texture 0's red and green move the other two.
    if combiner(16u) == 2u {
        let offset = (2.0 * t0.rg - 1.0) * sv2(0x100u);
        uv1 += offset;
        uv2 += offset;
    }
    let t1 = sample_slot(1u, uv1);
    let t2 = sample_slot(2u, uv2);

    // Textures combined (inputs 8–13, blends 2, 3, 5, 6).
    var tc = input_color(combiner(8u), t0.rgb);
    var ta = input_alpha(combiner(11u), t0.a);
    if ((draw.textures.x >> 1u) & 1u) != 0u {
        tc = blend_color(combiner(2u), tc, input_color(combiner(9u), t1.rgb));
        ta = blend_alpha(combiner(5u), ta, input_alpha(combiner(12u), t1.a));
    }
    if ((draw.textures.x >> 2u) & 1u) != 0u {
        tc = blend_color(combiner(3u), tc, input_color(combiner(10u), t2.rgb));
        ta = blend_alpha(combiner(6u), ta, input_alpha(combiner(13u), t2.a));
    }
    // The primitive's vertex colour (blends 4, 7; inputs 14, 15).
    if draw.shape.x != 4u {
        tc = blend_color(combiner(4u), tc, input_color(combiner(14u), in.vertex_color.rgb));
        ta = blend_alpha(combiner(7u), ta, input_alpha(combiner(15u), in.vertex_color.a));
    }

    let c0 = in.color0;
    let c1 = in.color1;
    var color: vec3<f32>;
    switch combiner(0u) {
        case 0u: { color = c0.rgb; }
        case 1u: { color = c0.rgb * tc; }
        case 2u: { color = c1.rgb + (c0.rgb - c1.rgb) * tc; }
        default: { color = c0.rgb * tc + c1.rgb; }
    }
    var a: f32;
    switch combiner(1u) {
        case 1u: { a = c0.a * c1.a * ta; }
        case 3u: { a = c1.a * (ta - c0.a); }
        case 4u: { a = c1.a * saturate((ta - c0.a) * 4.0); }
        default: { a = c0.a * ta; }
    }

    // Soft particles: the scene's depth behind (§8).
    var soft = 1.0;
    let soft_range = sf(0x5f4u);
#ifdef DEPTH_PREPASS
    if soft_range > 0.0 {
        let depth = prepass_utils::prepass_depth(in.clip, 0u);
        let scene_z = view.clip_from_view[3][2] / max(depth, 1e-7);
        soft = saturate((scene_z - in.factors.z) / soft_range);
    }
#endif
    var alpha = in.factors.x * saturate(a) * soft;
    if draw.shape.z != 0u && alpha <= bitcast<f32>(draw.combiner[4].w) {
        discard;
    }


    // Custom shader 3: ambient light from the analyzer's table, between
    // its sky up and down by the height on screen (the game takes it per
    // vertex), then the scene's fog.
    if draw.shape.y == 3u {
        var light = custom_light(in);
        // F1 & 0x1: plus the cube map's light along the normal,
        // `cus1[0x01C]·reserved[0x004]·cube(N, cus1[0x010])` (both cus1
        // terms 1 and 0 in ordinary play).
        // SI-EFX-26: the scene's cube map stands for drawCtx+0x18[CSDP[0x28]].
        if (dyn.count.w & 1u) != 0u {
            light += dyn.reserved[0].y * cube_at(normalize(in.normal), 0.0);
        }
        color *= light;
        let look = read_look_at(look_texture, in.world);
        let view_dir = normalize(in.world - view.world_position);
        color = apply_haze(look, color, in.world, view_dir);
    }
    return vec4<f32>(color, alpha);
}
