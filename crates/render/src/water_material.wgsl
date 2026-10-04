// Water, after the game's own water shader (Wii U v208,
// `uking_terrain_water`, the G-buffer program 9–11 whose formulas are
// recovered and checked in docs/research/wiiu-water-variants.md):
//
// - two flow-map phases scroll the water's normal maps (`WaterNrm`) and
//   foam (`WaterEmm`) along the current, in the texture spaces of
//   `TeraWater`'s `tex_srt0–5`, and are cross-faded;
// - the view ray's depth of water behind the surface (from the camera's
//   depth prepass) gives the water's opacity, its edge fade and its foam
//   through the kind's row of the water table (`WaterAlb`);
// - the normal maps shift where the scene behind is fetched, unless that
//   point lies in front of the water.
//
// The game writes the results into its G-buffer (albedo, normal, gloss)
// and the transmitted colour into the frame; its deferred `field_water`
// pass then adds the lit albedo, the cube map's reflection with a Schlick
// Fresnel (F0 0.02), the sun's GGX glint and fog
// (docs/research/wiiu-deferred-shading.md). Here the cube map is the
// viewer's (`cubemap.rs`) and the fog is the viewer's aerial perspective
// and haze; see the gaps in archived GAPS.md. notes
//
// The vertex shader adds the game's small standing waves (its vertex
// shader, read from the Cemu GLSL that matches program 9, not yet checked
// against the native code).
//
// UV x carries the water kind, UV y the water's depth over the terrain at
// the vertex (m), the second UV set the game's flow value (2 · channel − 1
// of `.water.extm`'s flow, see `mesh.rs`).

#import bevy_pbr::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    mesh_view_bindings::{globals, view},
    view_transformations::{frag_coord_to_uv, position_world_to_view, position_world_to_clip},
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
}
#import botw::look::{read_look_at, sky_cover, water_wetness_at}
#import botw::clouds::CloudParams
#import botw::field_water::{WaterPixel, field_water, scene_behind, scene_distance, unpack_normal}

// Texels per water kind in the table (`WaterAlb`).
const TEXELS: i32 = 7;

// Texture spaces (rows of `WaterParams::srt`): the base one from world x/z
// (`tex_srt3`), and from it those of the secondary normal map (`_s0`,
// `tex_srt0`), primary normal map (`_n0`, `tex_srt1`), foam (`_e0`,
// `tex_srt2`), third normal map (`_a1`, `tex_srt4`) and the foam's
// distortion (`_t0`, `tex_srt5`).
const SRT_BASE: i32 = 0;
const SRT_SECONDARY: i32 = 1;
const SRT_PRIMARY: i32 = 2;
const SRT_FOAM: i32 = 3;
const SRT_THIRD: i32 = 4;
const SRT_DISTORTION: i32 = 5;

struct WaterParams {
    // x: 1 when the game's maps are bound, y: the glint's weather factor
    // (`uking_dynamic_cloud_ratio`), z: strength of the
    // aerial perspective the water adds itself (the viewer's, fitted to
    // screenshots), w: unused.
    surface: vec4<f32>,
    // Six 2×3 texture matrices, two vectors each: (m00, m10, m01, m11),
    // (m02, m12, -, -); see the SRT_ constants.
    srt: array<vec4<f32>, 12>,
    // xy: `indirect_scale2` (refraction), zw: `indirect_scale4` (foam
    // distortion).
    indirect: vec4<f32>,
    // `const_color3.a`, `const_color5.a` (edge fade), `const_value2` (depth
    // gain looking down), `const_value3` (opacity exponent).
    depth: vec4<f32>,
    // `const_value6` (foam from opacity), `const_vector0.x` (third normal
    // map), `const_vector1.x` (normal fade with distance), unused.
    shape: vec4<f32>,
    // `const_color2`: the least weight of each normal component.
    normal_base: vec4<f32>,
    // Frames per second of the water's clock, phase per frame,
    // environment[38].x and .w (the flow's scale).
    clock: vec4<f32>,
    // The game's table, 7 texels per kind (see `WaterTable`).
    kinds: array<vec4<f32>, 56>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var normal_array: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var normal_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> params: WaterParams;
// The shared look values (see look.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var look_texture: texture_2d<f32>;
// The sky's clouds as they are drawn this frame (see clouds.rs), for their
// shadow.
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var cloud_noise: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var cloud_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var<uniform> clouds: CloudParams;
// Foam per water kind (`WaterEmm`).
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var foam_array: texture_2d_array<f32>;
// The game's cloud shadow texture (see `cloud_shadow`).
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var cloud_shadow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var cloud_shadow_sampler: sampler;

// Texture space `i` (see the SRT_ constants) of coordinates `uv`.
fn srt(i: i32, uv: vec2<f32>) -> vec2<f32> {
    let m = params.srt[2 * i];
    let t = params.srt[2 * i + 1];
    return m.xy * uv.x + m.zw * uv.y + t.xy;
}

// Sum of a few travelling sine waves' slopes, for when there is no dump.
fn analytic_slope(p: vec2<f32>, t: f32) -> vec2<f32> {
    var s = vec2<f32>(0.0);
    let dirs = array<vec2<f32>, 4>(vec2(0.8, 0.6), vec2(-0.5, 0.86), vec2(0.2, -0.98), vec2(-0.93, -0.37));
    let freq = array<f32, 4>(0.9, 1.7, 2.9, 4.3);
    for (var i = 0; i < 4; i++) {
        let phase = dot(p, dirs[i]) * freq[i] + t * (1.0 + f32(i) * 0.4);
        s += dirs[i] * cos(phase) * 0.35 / freq[i];
    }
    return s;
}

// Texel `i` of water kind `kind`'s row of the table.
fn table(kind: i32, i: i32) -> vec4<f32> {
    return params.kinds[kind * TEXELS + i];
}

// The water clock's phase (environment[37].x): frames times phase per frame.
fn clock_phase() -> f32 {
    return globals.time * params.clock.x * params.clock.y;
}

// An angle wrapped to [−π, π) the way the game's vertex shader does before
// its SIN/COS (1/2π = 0x3e22f983, 2π = 0x40c90fdb, π = 0x40490fdb).
fn wrapped(angle: f32) -> f32 {
    return fract(angle * bitcast<f32>(0x3e22f983u) + 0.5) * bitcast<f32>(0x40c90fdbu) - bitcast<f32>(0x40490fdbu);
}

// The game's standing waves for water kind `kind` at world x/z `p` with
// `depth` metres of water over the terrain: texel 6's green is the height
// (a tenth of it, 0x3dcccccd), blue over green the number of waves per
// 0.35 m (0x3eb33333), and the whole surface sits 2.5 cm lower
// (0xbccccccd) wherever it is deep enough to move.
fn wave_height(kind: i32, p: vec2<f32>, depth: f32) -> f32 {
    let shape = table(kind, 6);
    let height = shape.g * bitcast<f32>(0x3dcccccdu);
    let count = shape.b / max(shape.g, bitcast<f32>(0x3727c5acu));
    let t = clock_phase();
    let along_x = p.x * bitcast<f32>(0x3eb33333u) + t * bitcast<f32>(0x3fb33333u);
    let along_z = p.y * bitcast<f32>(0x3eb33333u) + t * 2.0;
    let lowered = bitcast<f32>(0xbccccccdu);
    // SI-WAT-05: zero-height water and wave phase are our heuristics.
    // (Where the table has no waves the game divides by zero: in or out.)
    let fade = select(select(0.0, 1.0, depth + lowered > 0.0), saturate((depth + lowered) / height + 0.5), height > 0.0);
    return (height * sin(wrapped(count * along_z)) * cos(wrapped(count * along_x)) + lowered) * fade;
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0)).xyz;
#ifdef VERTEX_UVS_A
    let kind = clamp(i32(round(vertex.uv.x)), 0, 7);
    position.y += wave_height(kind, position.xz, vertex.uv.y);
    out.uv = vertex.uv;
#endif
    out.world_position = vec4<f32>(position, 1.0);
    out.position = position_world_to_clip(position);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let world = in.world_position.xyz;
#ifdef VERTEX_UVS_A
    let kind = clamp(i32(round(in.uv.x)), 0, 7);
#else
    let kind = 0;
#endif
#ifdef VERTEX_UVS_B
    let flow_value = in.uv_b;
#else
    let flow_value = vec2(0.0);
#endif

    // The water's clock: the game accumulates frames and turns them into a
    // phase on the CPU (environment[37]).
    let phase_a = fract(clock_phase());
    let phase_b = fract(clock_phase() + 0.5);

    // The current (in the vertex shader): the flow value times
    // environment[38].w, then .x.
    let flow = (flow_value * params.clock.w) * params.clock.z;
    let flow_length = length(flow);
    // At most 0.7 texture units per phase (literals 0x3fb6db6e, 0x3f333333).
    let flow_dir = select(vec2(0.0), flow / flow_length, flow_length > 0.0);
    let flow_shift = flow_dir * saturate(flow_length * bitcast<f32>(0x3fb6db6eu)) * bitcast<f32>(0x3f333333u);
    // The phase also runs across the world (literals 0x3cbe82fa ≈ 1/43
    // along z, 0x3cdd67c9 ≈ 1/37 along x), so that it restarts in waves.
    // SI-WAT-05: zero-height water and wave phase are our heuristics.
    let spread = world.z * bitcast<f32>(0x3cbe82fau) + world.x * bitcast<f32>(0x3cdd67c9u);
    let t0 = fract(spread + phase_a);
    let t1 = fract(spread + phase_b);
    let phase = abs(2.0 * t0 - 1.0);
    let shift0 = flow_shift * t0;
    let shift1 = flow_shift * t1;

    // Every map at once (uniform control flow for derivatives).
    let base = srt(SRT_BASE, world.xz);
    let primary_uv = srt(SRT_PRIMARY, base);
    let secondary_uv = srt(SRT_SECONDARY, base);
    let n0 = textureSample(normal_array, normal_sampler, primary_uv + shift0, kind).rg;
    let n1 = textureSample(normal_array, normal_sampler, primary_uv + shift1, kind).rg;
    let s0 = textureSample(normal_array, normal_sampler, secondary_uv + shift0, kind).rg;
    let s1 = textureSample(normal_array, normal_sampler, secondary_uv + shift1, kind).rg;
    let third = textureSample(normal_array, normal_sampler, srt(SRT_THIRD, base), kind).rg;
    let distortion = textureSample(normal_array, normal_sampler, srt(SRT_DISTORTION, base), kind).rg;
    let foam_uv = srt(SRT_FOAM, base) + distortion * params.indirect.zw;
    let e0 = textureSample(foam_array, normal_sampler, foam_uv + shift0, kind).r;
    let e1 = textureSample(foam_array, normal_sampler, foam_uv + shift1, kind).r;

    let game_maps = params.surface.x > 0.5;
    let primary = select(analytic_slope(world.xz, globals.time), unpack_normal(mix(n0, n1, phase)), game_maps);
    let secondary = select(vec2(0.0), unpack_normal(mix(s0, s1, phase)), game_maps);
    let detail = select(vec2(0.0), unpack_normal(third), game_maps);

    // Where the scene behind is fetched: shifted by the primary normal map,
    // more with more water behind (up to 2 m), less far away; unless the
    // shifted point is in front of the water.
    let view_position = position_world_to_view(world);
    let tan_half_fov = 1.0 / view.clip_from_view[1][1];
    let inverse_factor = 1.0 / (-view_position.z * tan_half_fov);
    let straight_uv = frag_coord_to_uv(in.position.xy);
    let straight_weight = saturate((view_position.z + scene_distance(straight_uv)) / 2.0);
    let candidate_uv = straight_uv + straight_weight * params.indirect.xy * inverse_factor * primary;
    let behind_uv = select(candidate_uv, straight_uv, -view_position.z > scene_distance(candidate_uv));
    let behind = scene_behind(behind_uv);

    // Metres of water along the view axis, deepened when looking down.
    let to_water = normalize(world - view.world_position);
    let level = saturate(to_water.y + 1.0);
    let depth = (view_position.z + scene_distance(behind_uv)) * (1.0 + params.depth.z * (1.0 - level));

    // The kind's row of the table (the game passes texels 0–5's RGB and a
    // few alphas through the vertex shader).
    let foam_color = table(kind, 0).rgb;
    let deep = table(kind, 1).rgb;
    let rate = table(kind, 2).rgb;
    let offset = table(kind, 3).rgb;
    let mixing = table(kind, 4).rgb;
    let gloss_range = table(kind, 5).rgb;

    let edge = saturate(params.depth.x * (params.depth.y + depth));
    let thickness = saturate(rate.r * (offset.r + depth));
    // SI-WAT-05: zero-height water and wave phase are our heuristics.
    // (The game's log of zero is not recovered; here it is fully clear.)
    let opacity = select(0.0, saturate(exp2(params.depth.w * log2(thickness))), thickness > 0.0);
    let shallow_g = saturate(rate.g * (offset.g + depth));
    let shallow_b = saturate(rate.b * (offset.b + depth));
    let churn = saturate(mixing.r * (10.0 * flow_length + params.shape.x * thickness));
    let spill = saturate(gloss_range.r * (1.0 - shallow_b) + mixing.g * churn);
    let foam = saturate(mix(e0, e1, phase) * (churn + 1.0 - shallow_g) + spill);

    let albedo = edge * (deep * opacity + (foam_color - deep * opacity) * mixing.b * foam);
    let transmitted = behind * (1.0 - opacity);
    // The game stores it as 6 bits (steps of 4/255).
    let gloss = 4.0 * floor(63.75 * saturate(edge * (gloss_range.g + (gloss_range.b - gloss_range.g) * foam))) / 255.0;

    // The normal: primary and secondary maps together, flattened with
    // distance, plus the third map, put into the G-buffer's view space the
    // game's way: context[11]·N.x + (0, 1, 0)·N.y + context[12]·N.z, where
    // context[11..13] are the rows of the inverse of its view matrix, so
    // context[11] and [12] are the world's x and y seen from the camera
    // (docs/research/wiiu-water-variants.md, "Context matrices"). In the
    // world: N.x along x, N.z up, N.y along the camera's up. The deferred
    // pass normalizes what it reads.
    let joined = normalize(vec3(primary + secondary, sqrt(1.0 - saturate(dot(secondary, secondary)))));
    let weight = saturate(inverse_factor * params.shape.z + params.normal_base.rgb);
    let tangent_n = normalize(weight * joined + params.shape.y * vec3(detail, 0.0));
    let camera_up = view.world_from_view[1].xyz;
    let n = normalize(vec3(tangent_n.x, tangent_n.z, 0.0) + tangent_n.y * camera_up);

    // The game's deferred shading of the water pixel (field_water.wgsl).
    let look = read_look_at(look_texture, in.world_position.xyz);
    var pixel: WaterPixel;
    pixel.albedo = albedo;
    pixel.transmitted = transmitted;
    pixel.gloss = gloss;
    pixel.normal = n;
    let rain = vec2<f32>(water_wetness_at(look_texture, look, world, sky_cover(look_texture, look, world)), look.weather[0].y);
    let color = field_water(pixel, world, pbr_input.V, in.position.xy, look, params.surface.y, params.surface.z, cloud_shadow_map, cloud_shadow_sampler, clouds, rain);

    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(color, 1.0));
    return out;
}
