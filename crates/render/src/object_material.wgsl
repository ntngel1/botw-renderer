// The map's objects: lit by the game's deferred shading of the field
// (`botw::deferred_light`: `field_hybrid`, leaves `field_leaf`) with
// their gloss, ambient occlusion and the leaves' light from behind,
// darkened where a cloud's shadow falls, dissolved into the billboard where
// a tree hands over to it (the visibility range's crossfade, with the
// game's mask instead of Bevy's ordered dither, in the depth prepass too).

#import bevy_pbr::{
    mesh_functions,
    mesh_bindings::mesh,
    pbr_bindings,
}
#import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_2d}
#import botw::clouds::CloudParams
#import botw::hand_off::model_hidden

#ifdef BINDLESS
#import bevy_pbr::pbr_bindings::material_indices
#endif

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::VertexOutput,
    pbr_prepass_functions::prepass_alpha_discard,
}
#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput
#endif
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, main_pass_post_lighting_processing},
    pbr_types::PbrInput,
    mesh_view_bindings::{globals, view},
    view_transformations::{position_world_to_view, frag_coord_to_uv},
}
#import botw::clouds::cloud_shadow
#import botw::look::{read_look_at, apply_haze, sky_cover, field_wetness_at}
#import botw::deferred_light::{FieldSurface, Frame, field_color, game_frame, main_light, ssao}
#import botw::normal_map::mapped_normal
#endif

// Light left deep inside a crown, next to its centre: the game's crowns are
// dark inside and bright on their rims (a fit, standing for the vertex
// shader's factor on the leaves' albedo, `Sem6.x`, not read).
// SI-LGT-18: crown core light and bend are ours, not VS field_leaf.
const CROWN_CORE_LIGHT: f32 = 0.4;

struct ObjectParams {
    clouds: CloudParams,
    // Foliage: x 1 for leaves, y the share of the normal map kept, z how far
    // the normals bend out from the crown's centre, w how far the light wraps
    // around them.
    leaf: vec4<f32>,
    // The crown's centre in model space (xyz) and radius (w, m).
    crown: vec4<f32>,
    // Gloss: x 1 where the gloss map holds it, y the gloss without one.
    gloss: vec4<f32>,
    // The leaves' sheen: its fade per metre and offset, exponent, strength.
    sheen: vec4<f32>,
}

#ifdef BINDLESS
// Per material slot: where its parameters, textures and sampler are.
struct ObjectIndices {
    params: u32,
    shadow_map: u32,
    shadow_sampler: u32,
    mask: u32,
    look: u32,
    ssao_noise: u32,
    gloss_map: u32,
    translucency_map: u32,
    cube_mean: u32,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var<storage> object_indices: array<ObjectIndices>;
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var<storage> object_params: array<ObjectParams>;
#else
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> params: ObjectParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var cloud_shadow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var cloud_shadow_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var dither_mask: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var look_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var ssao_noise: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var gloss_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var translucency_map: texture_2d<f32>;
// The environment's mean brightness, once a frame (deferred_light.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var cube_means: texture_2d<f32>;
#endif

// Whether the model's pixel is dissolved into its billboard.
fn dissolved(in: VertexOutput) -> bool {
#ifdef VISIBILITY_RANGE_DITHER
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let mask = bindless_textures_2d[object_indices[slot].mask];
#else
    let mask = dither_mask;
#endif
    return model_hidden(mask, in.position.xy, in.visibility_range_dither);
#else
    return false;
#endif
}

#ifdef PREPASS_PIPELINE

// The depth prepass and the shadows of a dissolving model (see
// `ObjectShading::specialize`): only its depth is drawn.
#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    if dissolved(in) {
        discard;
    }
    prepass_alpha_discard(in);
    var out: FragmentOutput;
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
    if dissolved(in) {
        discard;
    }
    prepass_alpha_discard(in);
}
#endif

#else

// The surface's gloss map and translucency map, read like its albedo and
// normal map (the first UV set): x the gloss (the material's constant
// without a map), y the translucency.
fn surface_maps(in: VertexOutput, params: ObjectParams) -> vec2<f32> {
#ifdef VERTEX_UVS_A
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let uv_transform = pbr_bindings::material_array[material_indices[slot].material].uv_transform;
    let gloss_texture = bindless_textures_2d[object_indices[slot].gloss_map];
    let translucency_texture = bindless_textures_2d[object_indices[slot].translucency_map];
    let map_sampler = bindless_samplers_filtering[material_indices[slot].base_color_sampler];
#else
    let uv_transform = pbr_bindings::material.uv_transform;
    let gloss_texture = gloss_map;
    let translucency_texture = translucency_map;
    let map_sampler = pbr_bindings::base_color_sampler;
#endif
    let uv = (uv_transform * vec3<f32>(in.uv, 1.0)).xy;
    let gloss = textureSampleBias(gloss_texture, map_sampler, uv, view.mip_bias).r;
    let translucency = textureSampleBias(translucency_texture, map_sampler, uv, view.mip_bias).r;
    return vec2<f32>(mix(params.gloss.y, gloss, params.gloss.x), translucency);
#else
    return vec2<f32>(params.gloss.y, 0.0);
#endif
}

// A leaf as the game's G-buffer holds it (`uking_mat` 8061,
// docs/research/wiiu-field-shading.md, "Foliage"): its normal, and its
// albedo brightened where it is lit through from behind and by its sheen.
fn leaf_surface(in: VertexOutput, pbr: PbrInput, params: ObjectParams, frame: Frame, translucency: f32) -> FieldSurface {
    var n = pbr.N;
    // Light left inside the crown (1 but deep inside).
    var reach = 1.0;
    if params.leaf.z > 0.0 {
        // A crown shades as one soft volume: normals bend out from its
        // centre, on both sides of every card…
        let world_from_local = mesh_functions::get_world_from_local(in.instance_index);
        let centre = (world_from_local * vec4<f32>(params.crown.xyz, 1.0)).xyz;
        let outward = in.world_position.xyz - centre;
        if dot(outward, outward) > 1e-4 {
            n = normalize(mix(n, normalize(outward), params.leaf.z));
        }
        // …and darkens towards its core, where little light gets in.
        let radius = params.crown.w * length(world_from_local[0].xyz);
        let depth = length(outward) / max(radius, 0.1);
        // SI-LGT-18: crown core light and bend are ours, not VS field_leaf.
        reach = mix(CROWN_CORE_LIGHT, 1.0, smoothstep(0.2, 0.95, depth));
    }
    var albedo = pbr.material.base_color.rgb * reach;
    if params.leaf.w > 0.0 {
        // From the camera, and the way the main light travels.
        let V = -pbr.V;
        let L = -main_light(pbr, frame.p.z).to_light;
        let H = normalize(V + L);
        let fade = saturate(params.sheen.y - params.sheen.x * frame.p.z);
        let s = saturate(dot(n, -L) * dot(n, -H) * (1.0 - fade));
        let rim = saturate(1.0 + dot(V, n));
        let behind = saturate(-dot(V, L));
        let through = params.leaf.w * saturate(rim * rim + 2.0 * translucency - 1.0) * behind * behind;
        let sheen = rim * params.sheen.w * pow(s, params.sheen.z);
        albedo *= 1.0 + through + sheen;
    }
    var surface: FieldSurface;
    surface.albedo = albedo;
    surface.N = n;
    surface.leaf = 1.0;
    surface.translucent = params.leaf.y;
    return surface;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    if dissolved(in) {
        discard;
    }
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let indices = object_indices[slot];
    let params = object_params[indices.params];
#endif

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.N = mapped_normal(in, is_front, pbr_input);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    let maps = surface_maps(in, params);
    let world = in.world_position.xyz;
#ifdef BINDLESS
    let look = read_look_at(bindless_textures_2d[indices.look], world);
    let cover = sky_cover(bindless_textures_2d[indices.look], look, world);
    let shade = cloud_shadow(
        bindless_textures_2d[indices.shadow_map],
        bindless_samplers_filtering[indices.shadow_sampler],
        params.clouds,
        world,
    );
    let noise = bindless_textures_2d[indices.ssao_noise];
    let means = bindless_textures_2d[indices.cube_mean];
    let look_t = bindless_textures_2d[indices.look];
#else
    let look = read_look_at(look_texture, world);
    let cover = sky_cover(look_texture, look, world);
    let shade = cloud_shadow(cloud_shadow_map, cloud_shadow_sampler, params.clouds, world);
    let noise = ssao_noise;
    let means = cube_means;
    let look_t = look_texture;
#endif

    let frame = game_frame(position_world_to_view(world), frag_coord_to_uv(in.position.xy));
    var surface: FieldSurface;
    var ao = 1.0;
    if params.leaf.x > 0.5 {
        // Foliage (`field_leaf`): no metal, gloss or normal.w flag, so no
        // ambient occlusion or rim light.
        surface = leaf_surface(in, pbr_input, params, frame, maps.y);
    } else {
        // The objects' G-buffer (docs/research/wiiu-field-shading.md): no
        // metal but where the viewer reads a metal map, normal.w's flag set
        // (ambient occlusion and rim light on), gloss from the normal map.
        surface.albedo = pbr_input.material.base_color.rgb;
        surface.N = pbr_input.N;
        surface.gloss = maps.x;
        // SI-LGT-19: gloss without a map, metal and SSAO range are ours.
        surface.metal = pbr_input.material.metallic;
        surface.flag = 1.0;
        // As on the terrain: past 150 m the ambient is the analyzer's sky.
        if -frame.p.z < 150.0 {
            ao = ssao(frame, noise, in.position.xy);
        }
    }
    let rain = vec2<f32>(field_wetness_at(look_t, look, world, cover, surface.N.y, surface.flag), look.weather[0].y);
    var lit = field_color(pbr_input, surface, frame, 1.0 - shade, ao, cover, means, rain);
    // Glowing parts: the game's G-buffer writes them into the scene colour,
    // which its main pass adds under the shaded colour times `src.a`, the
    // fog's transmittance squared (`field_hybrid` PS 32: bit 0 of the
    // albedo's alpha × Shadow.z²); the haze below takes one of the two.
    let view_dir = normalize(world - view.world_position);
    let emissive = pbr_input.material.emissive;
    if any(emissive.rgb > vec3<f32>(0.0)) {
        let through = apply_haze(look, vec3<f32>(1.0), world, view_dir)
            - apply_haze(look, vec3<f32>(0.0), world, view_dir);
        lit += emissive.rgb * mix(1.0, view.exposure, emissive.a) * through;
    }
    // The air between the camera and the object.
    lit = apply_haze(look, lit, world, view_dir);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(lit, pbr_input.material.base_color.a));
    return out;
}

#endif
