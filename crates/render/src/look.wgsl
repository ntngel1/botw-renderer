// botw::look — the game's stylized light and air, shared by every surface
// shader: the extra gain on lit surfaces and the coloured haze. The values change every frame (time of day, climate,
// weather); they live in one small texture that every material binds
// (`look.rs` fills it from the `Look` resource), so no material asset is
// touched when they change.
//
// Use in a material's fragment shader:
//
//     #import botw::look::{Look, read_look, diffuse_gain, apply_haze}
//     @group(#{MATERIAL_BIND_GROUP}) @binding(N) var look_texture: texture_2d<f32>;
//     ...
//     let look = read_look(look_texture);   // bindless: read_look(bindless_textures_2d[indices.look])
//     out.color = apply_pbr_lighting(pbr_input);
//     let lit = out.color.rgb * diffuse_gain(look);
//     out.color = vec4<f32>(apply_haze(look, lit, world_position, view_dir), out.color.a);
//     out.color = main_pass_post_lighting_processing(pbr_input, out.color);
//
// The signatures below are the contract between the tracks; the bodies are
// the tracks' to fill (light: diffuse_gain; air: apply_haze).
// With the default `Look` every function is an exact identity.

#define_import_path botw::look

#ifdef REDUCED_BUFFER
#import botw::cloud_view::view
#else
#import bevy_pbr::mesh_view_bindings::view
#endif

// Texels in the look texture (keep equal to `LOOK_TEXELS` in look.rs).
const LOOK_TEXELS: u32 = 28u;
// Side of the fog's height noise in rows 1–64 (`LOOK_NOISE` in look.rs).
const LOOK_NOISE: i32 = 64;
// Side of the game's sky table and its first row (`LOOK_SKY`,
// `LOOK_SKY_ROW` in look.rs).
const LOOK_SKY: i32 = 256;
const LOOK_SKY_ROW: i32 = 65;
// Side of the map of the sky's cover and its first row (`LOOK_COVER`,
// `LOOK_COVER_ROW` in look.rs).
const LOOK_COVER: i32 = 96;
const LOOK_COVER_ROW: i32 = LOOK_SKY_ROW + LOOK_SKY;
// First row of the volume mask (`LOOK_MASK_ROW` in look.rs), drawn there
// every frame by volume_mask.rs.
const LOOK_MASK_ROW: i32 = LOOK_COVER_ROW + LOOK_COVER;
// The mask's texels per view pixel each way: unit 0's `layer_reduce_level`
// 3 (`UNITS` in volume_mask.rs).
const LOOK_MASK_REDUCE: f32 = 8.0;

// The look texture unpacked, in sections of four texels, each owned by one
// wave-3 track (see `Look` in look.rs for what each texel holds):
struct Look {
    // Texels 0–3, light: [0].x diffuse gain; the rest is unused.
    light: array<vec4<f32>, 4>,
    // Texels 4–7, air (see `apply_haze` and `fog.rs`): [4].rgb haze colour
    // after exposure, [4].a its share far away (0: no haze); [5].x one over
    // the fog's depth range (per km), [5].y where it starts (km), [5].z its
    // attenuation exponent; [6].rgb glow towards the light after exposure,
    // [6].a how tightly it gathers; [7].xyz towards the light.
    haze: array<vec4<f32>, 4>,
    // Texels 8–11, where the map of the sky's cover lies (see `sky_cover`):
    // [8] its north-west corner, x = [8].x + [8].y, z = [8].z + [8].w;
    // [9].x its side (m), [9].w 1 when there is a map.
    sky_occlusion: array<vec4<f32>, 4>,
    // Texels 12–15, the height fog (see `apply_haze`): [12].rgb its colour
    // after exposure, [12].a its strength (0: none); [13].x one over its
    // depth range below the camera (per metre), [13].y where it starts (m);
    // [14].x the fog's height noise at the surface and [14].y 1 when it was
    // read (`read_look_at`); [15].x the sky table's brightness after
    // exposure (0: no table, the haze keeps its fitted colour), [15].y the
    // scattering fog's `horz` exponent, [15].z the cosine of the horizon
    // seen from the camera.
    spare: array<vec4<f32>, 4>,
    // Texels 16–19, the ad hoc fog (the game's fog B, see `apply_haze`):
    // [16].rgb its colour after exposure, [16].a its strength (0: none);
    // [17].x one over its depth range (per km), [17].y where it starts
    // (km), [17].z the exponent it thickens by; [18].x how fast it thins
    // looking up, [18].y how much of it looking straight up takes away.
    adhoc: array<vec4<f32>, 4>,
    // Texels 20–23, the volume mask (volume_mask.rs): [20].xy its size in
    // texels, [20].z the main view's `clip_from_view[1][1]`, [20].w 1 when
    // it is drawn.
    shafts: array<vec4<f32>, 4>,
    // Texels 24–27, the wet ground (see `field_wetness`): [24].x the rain
    // ratio, [24].y the rainfall, [24].z the rain pulse's clock.
    weather: array<vec4<f32>, 4>,
    // Not in the texture: the scattering fog's colour for the surface, from
    // the sky table (`read_look_at`); .a 1 when there is one.
    sky: vec4<f32>,
    // Not in the texture: the game's `gsys_user2` at the surface's pixel
    // (`read_look_at`): .x the volume mask, .y the indoor mask; 0 without.
    at_pixel: vec4<f32>,
}

// The values that change nothing: what `read_look` returns for a texture
// that is not a look texture (e.g. a material made without one).
fn identity_look() -> Look {
    var look: Look;
    look.light[0] = vec4<f32>(1.0, 0.0, 0.0, 0.0);
    look.light[1] = vec4<f32>(0.0);
    look.light[2] = vec4<f32>(0.0);
    look.light[3] = vec4<f32>(0.0);
    return look;
}

// Reads the look texture. Unused texels cost nothing: the shader compiler
// drops loads whose values are never used.
fn read_look(t: texture_2d<f32>) -> Look {
    if textureDimensions(t).x < LOOK_TEXELS {
        return identity_look();
    }
    var look: Look;
    for (var i = 0; i < 4; i += 1) {
        look.light[i] = textureLoad(t, vec2<i32>(i, 0), 0);
        look.haze[i] = textureLoad(t, vec2<i32>(4 + i, 0), 0);
        look.sky_occlusion[i] = textureLoad(t, vec2<i32>(8 + i, 0), 0);
        look.spare[i] = textureLoad(t, vec2<i32>(12 + i, 0), 0);
        look.adhoc[i] = textureLoad(t, vec2<i32>(16 + i, 0), 0);
        look.shafts[i] = textureLoad(t, vec2<i32>(20 + i, 0), 0);
        look.weather[i] = textureLoad(t, vec2<i32>(24 + i, 0), 0);
    }
    return look;
}

// `read_look` plus the fog's height noise at `world_position` (the game's
// `cloud_noise`, sampled at world x/z / 1000 m and repeating, filtered
// between its texels): what `apply_haze` needs for the height fog.
fn read_look_at(t: texture_2d<f32>, world_position: vec3<f32>) -> Look {
    var look = read_look(t);
    if textureDimensions(t).y >= u32(LOOK_NOISE + 1) {
        let p = fract(world_position.xz * 0.001) * f32(LOOK_NOISE) - 0.5;
        let i = vec2<i32>(floor(p));
        let f = p - floor(p);
        let a = noise_texel(t, i);
        let b = noise_texel(t, i + vec2(1, 0));
        let c = noise_texel(t, i + vec2(0, 1));
        let d = noise_texel(t, i + vec2(1, 1));
        look.spare[2] = vec4<f32>(mix(mix(a, b, f.x), mix(c, d, f.x), f.y), 1.0, 0.0, 0.0);
    }
    if look.spare[3].x > 0.0 && textureDimensions(t).y >= u32(LOOK_SKY_ROW + LOOK_SKY) {
        look.sky = vec4<f32>(sky_haze(t, look, world_position) * look.spare[3].x, 1.0);
    }
    look.at_pixel = vec4<f32>(gsys_user2(t, look.shafts[0], world_position), 0.0, 0.0);
    return look;
}

// The game's `gsys_user2` at the pixel of `world_position`: the merged
// volume mask (volume_mask.rs) in .x, the indoor mask in .y, read like a
// texture of the mask's size over the view, filtered between its texels.
// Only for the view the mask is drawn for (its size and lens), else 0.
// SI-VOL-04: other views (the cube map's faces) read no mask (0).
// SI-VOL-01: the sampler the game reads `gsys_user2` with is not read;
// filtered between the texels.
fn gsys_user2(t: texture_2d<f32>, m: vec4<f32>, world_position: vec3<f32>) -> vec2<f32> {
    let size = floor(view.viewport.zw / LOOK_MASK_REDUCE);
    let lens = view.clip_from_view[1][1];
    if m.w < 0.5 || any(size != m.xy) || abs(lens - m.z) > 0.01 * m.z
        || textureDimensions(t).y < u32(LOOK_MASK_ROW) + u32(m.y) {
        return vec2<f32>(0.0);
    }
    let clip = view.clip_from_world * vec4<f32>(world_position, 1.0);
    let uv = clip.xy / clip.w * vec2<f32>(0.5, -0.5) + 0.5;
    let p = uv * m.xy - 0.5;
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    let last = vec2<i32>(m.xy) - 1;
    let a = mask_texel(t, i, last);
    let b = mask_texel(t, i + vec2(1, 0), last);
    let c = mask_texel(t, i + vec2(0, 1), last);
    let d = mask_texel(t, i + vec2(1, 1), last);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn mask_texel(t: texture_2d<f32>, at: vec2<i32>, last: vec2<i32>) -> vec2<f32> {
    let c = clamp(at, vec2(0), last);
    return textureLoad(t, vec2<i32>(c.x, LOOK_MASK_ROW + c.y), 0).rg;
}

// The scattering fog's colour for a surface at `world_position`: the game's
// PS 140 reads its sky table (`gsys_user0`) at
//   u = 1 − (2/π)·acos(0.5 + 0.5·max(c, −0.99)),
//   c = mix(cos of the azimuth to the light, cos of the angle to it, 1 − t),
//   v = 0.5 + 0.5·((1 − h)·(1 − t)^horz + h),
// t the fog's depth fraction, h the cosine of the horizon: near surfaces
// take the sky higher up and nearer the light's side, far ones the sky at
// the horizon in their azimuth. The azimuth is taken in view space, like
// the game. (The game's light vector points away from the light; the sign
// is folded in here, so the glow faces the sun.)
fn sky_haze(t: texture_2d<f32>, look: Look, world_position: vec3<f32>) -> vec3<f32> {
    let forward = -view.world_from_view[2].xyz;
    let ray = world_position - view.world_position;
    let z = dot(ray, forward);
    let fog = look.haze[1];
    let depth = saturate((z * 0.001 - fog.y) * fog.x);
    let v_dir = (view.view_from_world * vec4<f32>(normalize(ray), 0.0)).xyz;
    let l_dir = (view.view_from_world * vec4<f32>(look.haze[3].xyz, 0.0)).xyz;
    let azimuth = dot(v_dir.xz, l_dir.xz) * inverseSqrt(max(dot(v_dir.xz, v_dir.xz) * dot(l_dir.xz, l_dir.xz), 1e-8));
    let c = mix(azimuth, dot(v_dir, l_dir), 1.0 - depth);
    // Clamped at 1 too: towards the light `c` can round past it.
    let u = 1.0 - acos(clamp(0.5 + 0.5 * c, 0.005, 1.0)) * (2.0 / 3.14159265);
    let horizon = look.spare[3].z;
    let v = 0.5 + 0.5 * ((1.0 - horizon) * pow(1.0 - depth, look.spare[3].y) + horizon);
    return sky_table(t, vec2<f32>(u, v)).rgb;
}

// The sky table at normalized (u, v), filtered between its texels and
// clamped at its edges; alpha 0 below the horizon.
fn sky_table(t: texture_2d<f32>, uv: vec2<f32>) -> vec4<f32> {
    let p = uv * f32(LOOK_SKY) - 0.5;
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    let a = sky_texel(t, i);
    let b = sky_texel(t, i + vec2(1, 0));
    let c = sky_texel(t, i + vec2(0, 1));
    let d = sky_texel(t, i + vec2(1, 1));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn sky_texel(t: texture_2d<f32>, at: vec2<i32>) -> vec4<f32> {
    let c = clamp(at, vec2(0), vec2(LOOK_SKY - 1));
    return textureLoad(t, vec2<i32>(c.x, LOOK_SKY_ROW + c.y), 0);
}

fn noise_texel(t: texture_2d<f32>, at: vec2<i32>) -> f32 {
    let wrapped = (at % LOOK_NOISE + LOOK_NOISE) % LOOK_NOISE;
    return textureLoad(t, vec2<i32>(wrapped.x, 1 + wrapped.y), 0).r;
}

// A texel of the sky's cover; 0 m beyond the map, like the game's border
// colour.
fn cover_texel(t: texture_2d<f32>, at: vec2<i32>) -> f32 {
    if any(at < vec2<i32>(0)) || any(at >= vec2<i32>(LOOK_COVER)) {
        return 0.0;
    }
    let h = textureLoad(t, vec2<i32>(at.x, LOOK_COVER_ROW + at.y), 0);
    return h.r + h.g;
}

// The height (m) of what covers the sky above `world_position`: the top of
// the terrain and of the models that shade the field from above (the game's
// `gsys_depth_shadow_quarter` read as `e39.w·(1 − DSQ)`, sky_occlusion.rs),
// filtered between the texels. 0 beyond the map and without one (the
// game's white border and white stand-in).
fn sky_cover(t: texture_2d<f32>, look: Look, world_position: vec3<f32>) -> f32 {
    let at = look.sky_occlusion;
    if at[1].w < 0.5 || textureDimensions(t).y < u32(LOOK_COVER_ROW + LOOK_COVER) {
        return 0.0;
    }
    let corner = vec2<f32>(at[0].x + at[0].y, at[0].z + at[0].w);
    let p = (world_position.xz - corner) * (f32(LOOK_COVER) / at[1].x) - 0.5;
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    let a = cover_texel(t, i);
    let b = cover_texel(t, i + vec2(1, 0));
    let c = cover_texel(t, i + vec2(0, 1));
    let d = cover_texel(t, i + vec2(1, 1));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// Extra gain on lit surfaces (not the sky), multiplied onto the colour
// `apply_pbr_lighting` returns: the game's ground is nearly as bright as its
// sky.
fn diffuse_gain(look: Look) -> f32 {
    return look.light[0].x;
}

// The air between the camera and a surface at `world_position`, seen along
// `view_dir` (normalized, from the camera towards the surface): returns
// `color` with the haze laid over it, by the game's scattering fog (its
// pre-shading pass, docs/research/wiiu-deferred-shading.md; its
// `gsys_user2` in `look.at_pixel`): over the view
// depth z the fog takes `A = density·(1 − (1 − t)^attenuation)` of the
// scene, `t = saturate((z − near)/(far − near))`, and brings in its own
// colour only past 360 m (all of it past 560 m), so the near distance
// darkens rather than pales, except where the volume mask (u2.x) finds
// the air between lit by the sun:
//   m = lerp(saturate(0.005z − 1.8), 1, saturate(u2.x)).
// The indoor mask scales A, B and H by w = 1 − 0.85·u2.y. Below the camera (at most 250 m up) the game's
// height fog adds its colour, up to its strength, between the palette's
// `FogStart` and `FogEnd` metres of drop. The scattering fog's colour is
// the game's: read from the sky table baked from the game's (`sky_haze`,
// via `read_look_at`); without it, the fitted haze colour, glowing
// towards the light. Over both lies the game's ad hoc fog (B of its PS 140):
//   B = w·(1 − (1 − tB)^atten_grd)·(1 − (1 − minscale)·saturate(V·up)^atten_sky),
//   tB = saturate((z − FogStart)/(FogEnd − FogStart)),
// w the air's moisture in the field, in the palette's fog colour; like the
// game, only with the sky table. The layers compose like the game's:
//   colour·(1 − A)(1 − B)(1 − H) + sky·m·A·(1 − B) + fogB·B + fogH·H·(1 − A)(1 − B).
// While the camera has
// Bevy's `DistanceFog` (`DISTANCE_FOG`), that
// fog already lays a haze over every material, so this returns `color`
// untouched: nothing is hazed twice (`fog.rs` decides which of the two is
// on).
fn apply_haze(look: Look, color: vec3<f32>, world_position: vec3<f32>, view_dir: vec3<f32>) -> vec3<f32> {
#ifdef DISTANCE_FOG
    return color;
#else
    let far = look.haze[0].a;
    if far <= 0.0 {
        return color;
    }
    let fog = look.haze[1];
    let forward = -view.world_from_view[2].xyz;
    let z = dot(world_position - view.world_position, forward);
    let t = saturate((z * 0.001 - fog.y) * fog.x);
    // The indoor mask thins every layer outdoors' fog: w = 1 − 0.85·u2.y.
    let w = 1.0 - 0.85 * look.at_pixel.y;
    let amount = far * (1.0 - pow(1.0 - t, fog.z)) * w;
    // The game's `m = lerp(0.1k + (1 − 0.1k)·saturate(0.005z − 1.8), 1,
    // saturate(u2.x))`: lit air (the volume mask, volume_mask.rs) keeps the
    // full in-scatter near the camera. k, read by its vertex shader from
    // `gsys_user4`, is not known and taken as 0.
    // SI-LGT-12: 0.1k from gsys_user4 taken as 0 (texel not read).
    let inscatter = mix(saturate(0.005 * z - 1.8), 1.0, saturate(look.at_pixel.x));
    let glow = look.haze[2].rgb * pow(max(dot(view_dir, look.haze[3].xyz), 0.0), look.haze[2].a);
    let haze = select(look.haze[0].rgb + glow, look.sky.rgb, look.sky.a > 0.5);
    // The height fog, the surface's height shifted by ±25 m of noise
    // (without `read_look_at`, none).
    let shift = select(0.0, 25.0 * (2.0 * look.spare[2].x - 1.0), look.spare[2].y > 0.5);
    let drop = min(view.world_position.y, 250.0) - (world_position.y + shift);
    let height = saturate((drop - look.spare[1].y) * look.spare[1].x) * look.spare[0].a * w;
    // The ad hoc fog, thinner looking up.
    let b = look.adhoc;
    let tb = saturate((z * 0.001 - b[1].y) * b[1].x);
    let up = pow(max(saturate(view_dir.y), 1e-6), b[2].x);
    let adhoc = b[0].a * (1.0 - pow(max(1.0 - tb, 1e-6), b[1].z)) * (1.0 - b[2].y * up) * w;
    let through = (1.0 - amount) * (1.0 - adhoc);
    return color * (through * (1.0 - height)) + haze * (inscatter * amount * (1.0 - adhoc))
        + b[0].rgb * adhoc + look.spare[0].rgb * (height * through);
#endif
}

// The noise (`cloud_noise`, red) at texture coordinates `uv`, repeating and
// filtered between its texels (the pre-shading's `uking_tex0`: wrap,
// bilinear, level 0).
fn noise_at(t: texture_2d<f32>, uv: vec2<f32>) -> f32 {
    let p = uv * f32(LOOK_NOISE) - 0.5;
    let i = vec2<i32>(floor(p));
    let f = p - floor(p);
    let a = noise_texel(t, i);
    let b = noise_texel(t, i + vec2(1, 0));
    let c = noise_texel(t, i + vec2(0, 1));
    let d = noise_texel(t, i + vec2(1, 1));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

// How wet a field surface is in the rain: the rain pre-shading's
// `Shadow.y` (programs 113, 109, 145; docs/research/weather.md §11.4).
// Wet only under open sky (`cover`, the height of what covers the sky,
// read as in `sky_visibility`), fading out from 80 to 200 m (`z`, view
// depth), modulated by a pulse of the noise that runs with the clock.
// `tan_half` is tan(fovy/2); `flag` and `n_y` the G-buffer's flag and the
// normal's up part.
fn field_wetness(t: texture_2d<f32>, look: Look, world: vec3<f32>, cover: f32, n_y: f32, flag: f32, z: f32, tan_half: f32) -> f32 {
    let ratio = look.weather[0].x;
    if ratio <= 0.0 || textureDimensions(t).y < u32(LOOK_NOISE + 1) {
        return 0.0;
    }
    let w = saturate(cover - world.y + 1.3 - flag * (2.0 * n_y - 1.0));
    let n = noise_at(t, world.xz * 0.5);
    let k = 4.0 + 0.03 / (z * tan_half);
    let q = noise_at(t, k * (world.xz + n));
    // SI-WTH-10: the pulse is read at level 0 (the game: implicit level,
    // bias 0.5 or 0.25).
    let p = noise_at(t, vec2<f32>(look.weather[0].z + 4.0 * (world.y + 0.5 * q), 0.0));
    let fade = saturate(z / 120.0 - 2.0 / 3.0);
    // (1 − the inner mask): outside.
    return ratio * (1.0 - w * w * w) * (1.0 - fade) * (0.5 + 0.5 * p);
}

// `field_wetness` for a surface at `world_position`, with the view depth
// and tan(fovy/2) from the view.
fn field_wetness_at(t: texture_2d<f32>, look: Look, world_position: vec3<f32>, cover: f32, n_y: f32, flag: f32) -> f32 {
    let z = -(view.view_from_world * vec4<f32>(world_position, 1.0)).z;
    let tan_half = 1.0 / view.clip_from_view[1][1];
    return field_wetness(t, look, world_position, cover, n_y, flag, max(z, 1e-3), tan_half);
}

// How wet a water surface is in the rain: the water's rain pre-shading
// (program 141; docs/research/weather.md §11.4): as `field_wetness`
// without the +1.3 m and the normal's term in the cover test, and without
// the height in the pulse.
fn water_wetness_at(t: texture_2d<f32>, look: Look, world: vec3<f32>, cover: f32) -> f32 {
    let ratio = look.weather[0].x;
    if ratio <= 0.0 || textureDimensions(t).y < u32(LOOK_NOISE + 1) {
        return 0.0;
    }
    let z = max(-(view.view_from_world * vec4<f32>(world, 1.0)).z, 1e-3);
    let tan_half = 1.0 / view.clip_from_view[1][1];
    let w = saturate(cover - world.y);
    let n = noise_at(t, world.xz * 0.5);
    let k = 4.0 + 0.03 / (z * tan_half);
    let q = noise_at(t, k * (world.xz + n));
    // SI-WTH-10: the pulse is read at level 0.
    let p = noise_at(t, vec2<f32>(look.weather[0].z + 2.0 * q, 0.0));
    let fade = saturate(z / 120.0 - 2.0 / 3.0);
    return ratio * (1.0 - w * w * w) * (1.0 - fade) * (0.5 + 0.5 * p);
}
