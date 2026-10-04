// Terrain shading with the game's material textures.
//
// Each tile has a 256×256 `.mate` texture: per sample two material indices
// and a blend. A fragment blends the four surrounding samples bilinearly;
// each sample mixes its two materials from the albedo array, and their
// normals (X, Y in red and green of `MaterialCmb`) and gloss (its blue) the
// same way. Textures are projected from above, and from the side on steep
// slopes so cliffs do not smear. The light is the game's deferred shading of
// the field (`botw::deferred_light`, docs/research/wiiu-field-shading.md).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    mesh_view_bindings::{globals, view},
}
#import botw::clouds::{CloudParams, cloud_shadow}
#import botw::look::{read_look_at, apply_haze, sky_cover, field_wetness_at}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
    view_transformations::{position_world_to_view, frag_coord_to_uv},
}
#import botw::deferred_light::{FieldSurface, field_color, game_frame, ssao}
#endif

struct MaterialTable {
    // Per material index: u scale, v scale, texture layer, unused.
    entries: array<vec4<f32>, 88>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var albedo_array: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var albedo_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var mate: texture_2d<u32>;
// Tile minimum corner (x, z) and edge length; w is 1 for tiles coloured by
// their vertices (no game textures).
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<uniform> tile: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var<uniform> table: MaterialTable;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var normal_array: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var cloud_shadow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var cloud_shadow_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var<uniform> clouds: CloudParams;
// The shared look values (see look.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var look_texture: texture_2d<f32>;
// The ambient occlusion's rotations (deferred_light.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var ssao_noise: texture_2d<f32>;
// The environment's mean brightness, once a frame (deferred_light.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(112) var cube_means: texture_2d<f32>;

// How strongly the normal maps bend the terrain's shading, near and far
// (from and to the distances in NORMAL_FADE, metres): the game's ground
// reads as soft, broad shapes, not crisp relief.
// SI-LGT-20: terrain normal fade, side projection and .mate blend are ours.
const NORMAL_NEAR: f32 = 0.5;
const NORMAL_FAR: f32 = 0.25;
const NORMAL_FADE: vec2<f32> = vec2<f32>(30.0, 300.0);

// Albedo (rgb), tangent-space normal slope (xy) and gloss of a surface
// point.
struct Surface {
    albedo: vec3<f32>,
    slope: vec2<f32>,
    gloss: f32,
}

struct Projection {
    uv: vec2<f32>,
    ddx: vec2<f32>,
    ddy: vec2<f32>,
}

fn sample_material(index: u32, p: Projection) -> Surface {
    let entry = table.entries[min(index, 87u)];
    let scale = entry.xy;
    let uv = p.uv * scale;
    let layer = i32(entry.z);
    let albedo = textureSampleGrad(albedo_array, albedo_sampler, uv, layer, p.ddx * scale, p.ddy * scale).rgb;
    let n = textureSampleGrad(normal_array, normal_sampler, uv, layer, p.ddx * scale, p.ddy * scale).rgb;
    return Surface(albedo, n.rg * 2.0 - 1.0, n.b);
}

fn mix_surface(a: Surface, b: Surface, t: f32) -> Surface {
    return Surface(mix(a.albedo, b.albedo, t), mix(a.slope, b.slope, t), mix(a.gloss, b.gloss, t));
}

fn sample_blend(texel: vec2<i32>, p: Projection) -> Surface {
    let size = vec2<i32>(textureDimensions(mate)) - vec2<i32>(1);
    let m = textureLoad(mate, clamp(texel, vec2<i32>(0), size), 0);
    let a = sample_material(m.x, p);
    if m.z == 0u {
        return a;
    }
    return mix_surface(a, sample_material(m.y, p), f32(m.z) / 255.0);
}

// A tangent-space slope turned into a world normal around `n`, with the
// texture's u axis along `u_axis` and v along `v_axis`.
fn bend(n: vec3<f32>, slope: vec2<f32>, u_axis: vec3<f32>, v_axis: vec3<f32>, strength: f32) -> vec3<f32> {
    let t = normalize(u_axis - n * dot(n, u_axis));
    let b = normalize(v_axis - n * dot(n, v_axis) - t * dot(t, v_axis));
    let s = slope * strength;
    return normalize(n + t * s.x + b * s.y);
}

fn terrain_surface(world: vec3<f32>, p: Projection) -> Surface {
    // SI-LGT-20: terrain normal fade, side projection and .mate blend are ours.
    let samples = f32(textureDimensions(mate).x - 1u);
    let local = (world.xz - tile.xy) / tile.z * samples;
    let base = floor(local);
    let f = local - base;
    let t = vec2<i32>(base);
    let c00 = sample_blend(t, p);
    let c10 = sample_blend(t + vec2<i32>(1, 0), p);
    let c01 = sample_blend(t + vec2<i32>(0, 1), p);
    let c11 = sample_blend(t + vec2<i32>(1, 1), p);
    return mix_surface(mix_surface(c00, c10, f.x), mix_surface(c01, c11, f.x), f.y);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let world = in.world_position.xyz;
    let normal = normalize(in.world_normal);
    // Derivatives must be taken in uniform control flow, before any branch.
    let top_uv = world.xz;
    let facing_z = abs(normal.z) > abs(normal.x);
    let side_uv = select(vec2<f32>(world.z, -world.y), vec2<f32>(world.x, -world.y), facing_z);
    let side_u = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), facing_z);
    let top = Projection(top_uv, dpdx(top_uv), dpdy(top_uv));
    let side = Projection(side_uv, dpdx(side_uv), dpdy(side_uv));

    let from_top = terrain_surface(world, top);
    var albedo = from_top.albedo;
    var gloss = from_top.gloss;
    let strength = mix(NORMAL_NEAR, NORMAL_FAR, smoothstep(NORMAL_FADE.x, NORMAL_FADE.y, distance(world, view.world_position)));
    var n = bend(normal, from_top.slope, vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.0, 1.0), strength);
    let steep = 1.0 - smoothstep(0.5, 0.8, abs(normal.y));
    if steep > 0.001 {
        let from_side = terrain_surface(world, side);
        albedo = mix(albedo, from_side.albedo, steep);
        gloss = mix(gloss, from_side.gloss, steep);
        n = normalize(mix(n, bend(normal, from_side.slope, side_u, vec3<f32>(0.0, -1.0, 0.0), strength), steep));
    }
#ifdef VERTEX_COLORS
    if tile.w > 0.5 {
        albedo = in.color.rgb;
    }
#endif
    pbr_input.material.base_color = vec4<f32>(albedo, 1.0);
    pbr_input.N = n;
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    var out: FragmentOutput;
    let look = read_look_at(look_texture, world);
    // SI-LGT-21: terrain gloss without floor(63.75 * gl).
    // The game's G-buffer of the terrain: no metal, normal.w's flag set
    // (ambient occlusion and rim light on). Its gloss from `MaterialCmb`'s
    // blue goes through a formula not read; taken as it is.
    var surface: FieldSurface;
    surface.albedo = albedo;
    surface.N = n;
    surface.gloss = gloss;
    surface.metal = 0.0;
    surface.flag = 1.0;
    let frame = game_frame(position_world_to_view(world), frag_coord_to_uv(in.position.xy));
    // The ambient occlusion only where it can change the ambient: past
    // 150 m the game's ambient is the analyzer's sky alone.
    var ao = 1.0;
    if -frame.p.z < 150.0 {
        ao = ssao(frame, ssao_noise, in.position.xy);
    }
    // The clouds' shadow on the land (the game's `gsys_projection0`).
    let shade = cloud_shadow(cloud_shadow_map, cloud_shadow_sampler, clouds, world);
    let cover = sky_cover(look_texture, look, world);
    let rain = vec2<f32>(field_wetness_at(look_texture, look, world, cover, surface.N.y, surface.flag), look.weather[0].y);
    var lit = field_color(pbr_input, surface, frame, 1.0 - shade, ao, cover, cube_means, rain);
    // The air between the camera and the ground.
    lit = apply_haze(look, lit, world, normalize(world - view.world_position));
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(lit, 1.0));
#endif
    return out;
}
