// Model water and glass, after the game's G-buffer programs of its
// translucent model materials (`uking_mat`, `gsys_assign_gbuffer`,
// variation 0; Wii U v208, the Cemu GLSL that matches them byte for byte,
// docs/research/model-water-glass.md):
//
// - water (PS 7965): the terrain water's formula with the material's
//   constants: a refracted look behind (two normal layers, faded in over
//   2 m of depth behind, rejected where it lands in front), the depth
//   behind through `const_color3/5` into opacity, shore and edge, the foam
//   map `_e0` shifted by `_t0`;
// - blended water (PS 7977): the same, blended into what is there by the
//   vertex alpha;
// - glass (PS 7845, 7869/7881): the fogged scene behind (bent by the normal
//   map for 7869/7881), its albedo `_a0` × vertex red ×
//   `saturate(const_value1 · (1 + V·N))`, gloss `const_value0`;
// - waterfalls (PS 11433, 11409): two colour (`_a0`, `_s0`) and two normal
//   (`_n0`, `_e0`) layers, opacity from the colour's alpha, the vertex
//   colour and the depth behind, blended by the vertex alpha (11409: its
//   square); their vertex shader (shared by programs 11404–11435) pushes
//   the vertices along the normal by the height texture `_v0`;
// - waterfall foam (PS 11265, 11301): the two layers mixed by the vertex
//   red, the second on the mesh's second UV set.
//
// The game writes the results into its G-buffer with material id 22 and
// lights them like the terrain's water (`field_water`, field_water.wgsl);
// here the same shading follows directly, in the transmissive phase.

#import bevy_pbr::{
    mesh_functions,
    pbr_fragment::pbr_input_from_standard_material,
    mesh_view_bindings::view,
    view_transformations::{frag_coord_to_uv, position_world_to_view, position_world_to_clip},
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
}
#import botw::look::{read_look_at, sky_cover, water_wetness_at}
#import botw::clouds::CloudParams
#import botw::field_water::{WaterPixel, field_water, scene_behind, scene_distance, unpack_normal}

// `ModelWaterParams::kind` (asset_format::xlu::XluKind).
const WATER: u32 = 1u;
const WATER_BLEND: u32 = 2u;
const GLASS: u32 = 3u;
const REFRACTING_GLASS: u32 = 4u;
const WATERFALL: u32 = 5u;
const WATERFALL_SQUARED_ALPHA: u32 = 6u;
const WATERFALL_MIXED: u32 = 7u;

struct ModelWaterParams {
    // x: the family (constants above), y: unused, z: the glint's weather
    // factor, w: the air's strength (SI-WAT-03), both as the terrain
    // water's.
    surface: vec4<f32>,
    // Texture coordinate sets 0–3: their mapping (0 the mesh's first UV
    // set, 1 its second, 11 world x/z); then each set's `tex_srt` as a 2×3
    // matrix, two vectors.
    mapping: vec4<f32>,
    srt: array<vec4<f32>, 8>,
    // `const_color0–5`.
    color: array<vec4<f32>, 6>,
    // `const_value0–7`.
    value: array<vec4<f32>, 2>,
    // `indirect_scale0–5`, two per vector.
    indirect: array<vec4<f32>, 3>,
    // Each sampler's component selection (`_a0`, `_s0`, `_n0`, `_e0`,
    // `_t0`, `_v0`): 0–3 a channel, 4 zero, 5 one.
    swizzle: array<vec4<f32>, 6>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> params: ModelWaterParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var tex_a0: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var tex_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var tex_s0: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var tex_n0: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var tex_e0: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var tex_t0: texture_2d<f32>;
// The shared look values (see look.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var look_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var<uniform> clouds: CloudParams;
// The game's cloud shadow texture (see `cloud_shadow`).
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var cloud_shadow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var cloud_shadow_sampler: sampler;
// The waterfalls' vertex texture (heights along the normal).
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var tex_v0: texture_2d<f32>;

fn value(i: u32) -> f32 {
    return params.value[i / 4u][i % 4u];
}

fn indirect(i: u32) -> vec2<f32> {
    let v = params.indirect[i / 2u];
    return select(v.xy, v.zw, i % 2u == 1u);
}

// Texture coordinate set `i`: the mesh's first or second UV set or world
// x/z, through its `tex_srt` (the vertex shader's; per fragment it is the
// same, both are linear).
fn texcoord(i: u32, uv: vec2<f32>, uv_b: vec2<f32>, world: vec3<f32>) -> vec2<f32> {
    let mapping = params.mapping[i];
    let p = select(select(uv, uv_b, mapping > 0.5), world.xz, mapping > 5.5);
    let m = params.srt[2u * i];
    let t = params.srt[2u * i + 1u];
    return m.xy * p.x + m.zw * p.y + t.xy;
}

// A sampled texel with its texture's component selection.
fn swizzled(texel: vec4<f32>, slot: u32) -> vec4<f32> {
    let pick = params.swizzle[slot];
    var out: vec4<f32>;
    for (var c = 0u; c < 4u; c++) {
        let s = u32(pick[c]);
        out[c] = select(select(texel[min(s, 3u)], 0.0, s == 4u), 1.0, s == 5u);
    }
    return out;
}

fn a0(uv: vec2<f32>) -> vec4<f32> {
    return swizzled(textureSample(tex_a0, tex_sampler, uv), 0u);
}
fn s0(uv: vec2<f32>) -> vec4<f32> {
    return swizzled(textureSample(tex_s0, tex_sampler, uv), 1u);
}
fn n0(uv: vec2<f32>) -> vec4<f32> {
    return swizzled(textureSample(tex_n0, tex_sampler, uv), 2u);
}
fn e0(uv: vec2<f32>) -> vec4<f32> {
    return swizzled(textureSample(tex_e0, tex_sampler, uv), 3u);
}
fn t0(uv: vec2<f32>) -> vec4<f32> {
    return swizzled(textureSample(tex_t0, tex_sampler, uv), 4u);
}

// Bevy's mesh vertex shader (no skins, no morphs), plus the waterfalls'
// push along the normal (VS of programs 11404–11435, families 11400,
// 11412, 11424): by the height texture `_v0` at texture coordinate set 2
// (level 0), times `const_value2` and the vertex green, per axis.
// SI-MWT-01: the push is along the model-space normal (the bake applied the bone).
@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var position = vertex.position;
#ifdef VERTEX_UVS_A
#ifdef VERTEX_COLORS
    let kind = u32(params.surface.x);
    if kind == WATERFALL || kind == WATERFALL_SQUARED_ALPHA {
        let at = texcoord(2u, vertex.uv, vertex.uv, vec3(0.0));
        let height = swizzled(textureSampleLevel(tex_v0, tex_sampler, at, 0.0), 5u).xyz;
        position += vertex.normal * height * value(2u) * vertex.color.g;
    }
#endif
#endif
    out.world_normal = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(position, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
    return out;
}

// `saturate(exp2(p · log2(max(x, 0))))` as the GPU computes it: its
// LOG_CLAMPED gives −FLT_MAX for 0 (Cemu's `isinf` guard), and a product
// with 0 is 0, so x ≤ 0 gives 0 for p > 0, 1 for p ≤ 0.
fn clamped_pow(x: f32, p: f32) -> f32 {
    if x <= 0.0 {
        return select(0.0, 1.0, p <= 0.0);
    }
    return saturate(exp2(p * log2(x)));
}

// The game's 6-bit gloss (steps of 4/255).
fn stored_gloss(x: f32) -> f32 {
    return 4.0 * floor(63.75 * saturate(x)) / 255.0;
}

// Tangent-space `v` in the world: T·v.x + B·v.y + N·v.z, B = w·(N × T)
// (Sem4.w the tangent's sign), as the programs build it.
fn to_world(v: vec3<f32>, normal: vec3<f32>, tangent: vec4<f32>) -> vec3<f32> {
    let bitangent = tangent.w * cross(normal, tangent.xyz);
    return tangent.xyz * v.x + bitangent * v.y + normal * v.z;
}

// Where the scene behind is fetched: `candidate`, unless the scene there
// lies in front of the pixel (the programs compare depths at the texel).
fn guarded(candidate: vec2<f32>, straight: vec2<f32>, view_z: f32) -> vec2<f32> {
    return select(candidate, straight, -view_z > scene_distance(candidate));
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let kind = u32(params.surface.x);
    let world = in.world_position.xyz;
#ifdef VERTEX_UVS_A
    let uv = in.uv;
#else
    let uv = vec2(0.0);
#endif
#ifdef VERTEX_UVS_B
    let uv_b = in.uv_b;
#else
    let uv_b = vec2(0.0);
#endif
    // The vertex colour (Sem5, `_c0`); white for a mesh without one.
#ifdef VERTEX_COLORS
    let vertex_color = in.color;
#else
    let vertex_color = vec4(1.0);
#endif
    // The interpolated vertex normal and tangent, not renormalised (Sem3,
    // Sem4).
    let normal = in.world_normal;
#ifdef VERTEX_TANGENTS
    let tangent = in.world_tangent;
#else
    let tangent = vec4(1.0, 0.0, 0.0, 1.0);
#endif

    let view_position = position_world_to_view(world);
    let tan_half_fov = 1.0 / view.clip_from_view[1][1];
    // 1 / (−P.z · context[17].y): screen units per tangent unit.
    let q = 1.0 / (-view_position.z * tan_half_fov);
    let straight = frag_coord_to_uv(in.position.xy);
    let to_pixel = normalize(world - view.world_position);

    // Every texture at once (uniform control flow for derivatives).
    let tc0 = texcoord(0u, uv, uv_b, world);
    let tc1 = texcoord(1u, uv, uv_b, world);
    let tc3 = texcoord(3u, uv, uv_b, world);
    let sample_a0 = a0(tc0);
    let sample_s0_0 = s0(tc0);
    let sample_s0_1 = s0(tc1);
    let sample_n0_0 = n0(tc0);
    let sample_n0_1 = n0(tc1);
    let sample_e0_1 = e0(tc1);
    let sample_t0 = t0(tc1);

    var pixel: WaterPixel;
    // The vertex alpha the blended programs mix their outputs by (1: none).
    var blend = 1.0;

    if kind == WATER || kind == WATER_BLEND {
        let c0 = params.color[0];
        let c1 = params.color[1];
        let c3 = params.color[3];
        let c4 = params.color[4];
        let c5 = params.color[5];
        let s = unpack_normal(sample_s0_0.xy);
        let n = unpack_normal(sample_n0_1.xy);
        let t = unpack_normal(sample_t0.xy);
        // The refraction fades in over 2 m of depth behind.
        let weight = saturate((view_position.z + scene_distance(straight)) / 2.0);
        let behind_uv = guarded(straight + weight * indirect(1u) * q * s, straight, view_position.z);
        let behind = scene_behind(behind_uv);
        let depth = view_position.z + scene_distance(behind_uv);
        let b = saturate(c3.x * (c5.x + depth));
        let j = saturate(c3.y * (c5.y + depth));
        let k = saturate(c3.z * (c5.z + depth));
        let g = saturate(c3.w * (c5.w + depth));
        let a = clamped_pow(b, value(3u));
        let foam = e0(tc3 + indirect(4u) * t).rgb;
        let c = saturate(foam * (1.0 - j) + saturate(value(0u) * (1.0 - k)));
        let color = c1.rgb * a + (c0.rgb - c1.rgb * a) * c4.z * c;
        pixel.albedo = g * color;
        pixel.transmitted = behind * (1.0 - a);
        pixel.gloss = stored_gloss(value(1u) * (1.0 - c.r));
        let v = normalize(vec3(s + n, sqrt(1.0 - saturate(dot(n, n)))));
        pixel.normal = to_world(v, normal, tangent);
        if kind == WATER_BLEND {
            blend = vertex_color.a;
        }
    } else if kind == GLASS || kind == REFRACTING_GLASS {
        let s = unpack_normal(sample_s0_0.xy);
        let n = to_world(vec3(s, sqrt(1.0 - saturate(dot(s, s)))), normal, tangent);
        // 0 facing the camera, 1 at grazing angles.
        let f = saturate(1.0 + dot(to_pixel, n));
        let behind_uv = select(straight, straight + indirect(1u) * s * q, kind == REFRACTING_GLASS);
        pixel.albedo = sample_a0.rgb * vertex_color.r * saturate(value(1u) * f);
        pixel.transmitted = scene_behind(behind_uv);
        pixel.gloss = stored_gloss(value(0u));
        pixel.normal = n;
    } else {
        // Waterfalls: two normal layers bend the look behind; the colour
        // layers show where opaque, over the depth behind's own colour.
        let c0 = params.color[0];
        let c1 = params.color[1];
        let n = unpack_normal(sample_n0_0.xy);
        let e = unpack_normal(sample_e0_1.xy);
        let behind_uv = guarded(straight + q * (indirect(2u) * n + indirect(3u) * e), straight, view_position.z);
        let behind = scene_behind(behind_uv);
        let depth = view_position.z + scene_distance(behind_uv);
        let t = saturate(c1.x * depth - c1.y);
        let flat_n = sqrt(1.0 - saturate(dot(n, n)));
        let flat_e = sqrt(1.0 - saturate(dot(e, e)));
        var v: vec3<f32>;
        if kind == WATERFALL_MIXED {
            // PS 11265/11301: the layers mixed by saturate(2·red²).
            let m = saturate(2.0 * vertex_color.r * vertex_color.r);
            let layers = saturate(sample_a0 + (sample_s0_1 - sample_a0) * m);
            let k = saturate(t * (layers.a + vertex_color.r - 1.0));
            let e2 = clamped_pow(saturate(value(7u) * depth), value(3u));
            pixel.albedo = saturate(layers.rgb * k + saturate(c0.rgb * e2));
            pixel.transmitted = behind * (1.0 - e2);
            v = vec3(mix(n, e, m), mix(flat_n, flat_e, m));
            blend = vertex_color.w;
        } else {
            // PS 11433 / 11409: the layers' mean.
            let layers = saturate((sample_a0 + sample_s0_1) / 2.0);
            var k = saturate(layers.a + vertex_color.z - 1.0) + value(3u) * (1.0 - saturate(value(0u) * depth - value(1u)));
            if kind == WATERFALL {
                k = saturate(vertex_color.w * k);
                blend = vertex_color.w;
            } else {
                k = saturate(k);
                blend = saturate(vertex_color.w * vertex_color.w);
            }
            let e2 = clamped_pow(saturate(value(7u) * depth), value(6u));
            pixel.albedo = saturate(c0.rgb * e2 + saturate(t * layers.rgb * k));
            pixel.transmitted = behind * (1.0 - e2);
            v = vec3((n + e) / 2.0, (flat_n + flat_e) / 2.0);
        }
        // The programs blend their gloss towards 1.
        pixel.gloss = stored_gloss(1.0);
        var world_n = to_world(normalize(v), normal, tangent);
        // PS 11433 mirrors the normal of back faces across the surface (the
        // geometric normal re-orthogonalised against the tangent,
        // T × (N × T)).
        if kind == WATERFALL && !is_front {
            let m = cross(tangent.xyz, cross(normal, tangent.xyz));
            world_n = world_n - 2.0 * dot(world_n, m) * m;
        }
        pixel.normal = world_n;
    }

    let look = read_look_at(look_texture, world);
    let rain = vec2<f32>(water_wetness_at(look_texture, look, world, sky_cover(look_texture, look, world)), look.weather[0].y);
    var color = field_water(pixel, world, pbr_input.V, in.position.xy, look, params.surface.z, params.surface.w, cloud_shadow_map, cloud_shadow_sampler, clouds, rain);
    // SI-MWT-02: blended programs blend their lit result over the frame.
    // The blended programs mix every G-buffer output with what
    // earlier draws left there (the opaque scene, the terrain's water) and
    // the pixel is then lit as water; here the lit result is blended over
    // what is drawn by the same factor (their materials blend: Bevy's
    // transparent phase, after the transmissive one).
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(color, blend));
    return out;
}
