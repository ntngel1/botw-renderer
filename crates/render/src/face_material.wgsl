// Native UMii face/lip composition, uking_mat programs 11187 and 8523.
#import botw::face_io::{FaceVertexOutput, standard_vertex}
#import bevy_pbr::mesh_bindings::mesh
#import bevy_render::bindless::{bindless_textures_2d, bindless_samplers_filtering}
#ifdef PREPASS_PIPELINE
#import bevy_pbr::{mesh_view_bindings::view, prepass_bindings}
#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput
#endif
#ifdef DEFERRED_PREPASS
#import bevy_pbr::{pbr_fragment::pbr_input_from_standard_material, pbr_deferred_functions::deferred_output}
#endif
#else
#import bevy_pbr::{forward_io::FragmentOutput, pbr_fragment::pbr_input_from_standard_material}
#import botw::character_material::shade_character
#endif
struct FaceParams {
    composition: u32,
    colors: array<vec4<f32>, 4>,
    swizzles: array<vec4<f32>, 6>,
    bias: array<vec4<f32>, 2>,
}
#ifdef BINDLESS
struct FaceIndices {
    params: u32,
    layer0: u32,
    sampler0: u32,
    layer1: u32,
    sampler1: u32,
    layer2: u32,
    sampler2: u32,
    layer3: u32,
    sampler3: u32,
    layer4: u32,
    sampler4: u32,
    layer5: u32,
    sampler5: u32,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(213) var<storage> face_indices: array<FaceIndices>;
@group(#{MATERIAL_BIND_GROUP}) @binding(214) var<storage> face_params: array<FaceParams>;
#else
@group(#{MATERIAL_BIND_GROUP}) @binding(200) var<uniform> face_params: FaceParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(201) var face_layer0: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(202) var face_sampler0: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(203) var face_layer1: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(204) var face_sampler1: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(205) var face_layer2: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(206) var face_sampler2: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(207) var face_layer3: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(208) var face_sampler3: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(209) var face_layer4: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(210) var face_sampler4: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(211) var face_layer5: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(212) var face_sampler5: sampler;
#endif
fn select_channel(texel: vec4<f32>, channel: f32) -> f32 {
    if channel < 4.0 { return texel[u32(channel)]; }
    return select(0.0, 1.0, channel > 4.5);
}
fn swizzle(texel: vec4<f32>, channels: vec4<f32>) -> vec4<f32> {
    return vec4(select_channel(texel, channels.x), select_channel(texel, channels.y), select_channel(texel, channels.z), select_channel(texel, channels.w));
}
@fragment
fn fragment(face_in: FaceVertexOutput, @builtin(front_facing) is_front: bool)
#ifdef PREPASS_PIPELINE
#ifdef PREPASS_FRAGMENT
    -> FragmentOutput
#endif
#else
    -> FragmentOutput
#endif
{
    let in = standard_vertex(face_in);
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let indices = face_indices[slot];
    let params = face_params[indices.params];
#else
    let params = face_params;
#endif
    let lip = params.composition == 1u;
    let makeup_uv = select(face_in.face_uvs.zw, in.uv_b, lip);
    let mask_uv = select(in.uv_b, in.uv, lip);
#ifdef BINDLESS
    let t0 = swizzle(textureSampleBias(bindless_textures_2d[indices.layer0], bindless_samplers_filtering[indices.sampler0], in.uv, params.bias[0][0]), params.swizzles[0]);
#else
    let t0 = swizzle(textureSampleBias(face_layer0, face_sampler0, in.uv, params.bias[0][0]), params.swizzles[0]);
#endif
#ifdef BINDLESS
    let t1 = swizzle(textureSampleBias(bindless_textures_2d[indices.layer1], bindless_samplers_filtering[indices.sampler1], in.uv_b, params.bias[0][1]), params.swizzles[1]);
#else
    let t1 = swizzle(textureSampleBias(face_layer1, face_sampler1, in.uv_b, params.bias[0][1]), params.swizzles[1]);
#endif
#ifdef BINDLESS
    let t2 = swizzle(textureSampleBias(bindless_textures_2d[indices.layer2], bindless_samplers_filtering[indices.sampler2], in.uv, params.bias[0][2]), params.swizzles[2]);
#else
    let t2 = swizzle(textureSampleBias(face_layer2, face_sampler2, in.uv, params.bias[0][2]), params.swizzles[2]);
#endif
#ifdef BINDLESS
    let t3 = swizzle(textureSampleBias(bindless_textures_2d[indices.layer3], bindless_samplers_filtering[indices.sampler3], face_in.face_uvs.xy, params.bias[0][3]), params.swizzles[3]);
#else
    let t3 = swizzle(textureSampleBias(face_layer3, face_sampler3, face_in.face_uvs.xy, params.bias[0][3]), params.swizzles[3]);
#endif
#ifdef BINDLESS
    let t4 = swizzle(textureSampleBias(bindless_textures_2d[indices.layer4], bindless_samplers_filtering[indices.sampler4], makeup_uv, params.bias[1][0]), params.swizzles[4]);
#else
    let t4 = swizzle(textureSampleBias(face_layer4, face_sampler4, makeup_uv, params.bias[1][0]), params.swizzles[4]);
#endif
#ifdef BINDLESS
    let t5 = swizzle(textureSampleBias(bindless_textures_2d[indices.layer5], bindless_samplers_filtering[indices.sampler5], mask_uv, params.bias[1][1]), params.swizzles[5]);
#else
    let t5 = swizzle(textureSampleBias(face_layer5, face_sampler5, mask_uv, params.bias[1][1]), params.swizzles[5]);
#endif
    let shade = clamp(params.colors[2].rgb + (vec3(1.0) - params.colors[2].rgb) * clamp(t3.rgb + vec3(1.0 - t4.g), vec3(0.0), vec3(1.0)), vec3(0.0), vec3(1.0));
    let skin = clamp(params.colors[1].rgb * t0.rgb * shade, vec3(0.0), vec3(1.0));
    let mask = clamp(t3.g * t5.a, 0.0, 1.0);
    let paint = clamp(mix(params.colors[0].rgb * t5.rgb, skin, 1.0 - mask), vec3(0.0), vec3(1.0));
    let brow = clamp(mix(params.colors[3].rgb * t4.r, paint, 1.0 - t4.a), vec3(0.0), vec3(1.0));
    var albedo = clamp(brow * t1.r, vec3(0.0), vec3(1.0));
    var alpha = params.bias[1].z * clamp(t4.a + t3.a + 1.0 - t4.g, 0.0, 1.0);
    if lip {
        // Native program 8523: stages 200..203 have no clamp.
        let makeup = mix(t4.rgb * params.colors[0].rgb, params.colors[1].rgb, 1.0 - t4.a);
        albedo = mix(makeup * t1.r, params.colors[2].rgb, t5.rgb * params.colors[2].a) * t0.rgb;
        alpha = 1.0;
    }
    if alpha < params.bias[1].w { discard; }
#ifdef PREPASS_PIPELINE
#ifdef DEFERRED_PREPASS
    var input = pbr_input_from_standard_material(in, is_front);
    input.material.base_color = vec4(albedo, alpha);
    return deferred_output(in, input);
#else
#ifdef PREPASS_FRAGMENT
    var out: FragmentOutput;
#ifdef NORMAL_PREPASS
    out.normal = vec4(normalize(in.world_normal) * 0.5 + vec3(0.5), 1.0);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef MOTION_VECTOR_PREPASS
    let clip = view.unjittered_clip_from_world * in.world_position;
    let previous_clip = prepass_bindings::previous_view_uniforms.clip_from_world * in.previous_world_position;
    out.motion_vector = (clip.xy / clip.w - previous_clip.xy / previous_clip.w) * vec2(0.5, -0.5);
#endif
    return out;
#endif // PREPASS_FRAGMENT
#endif // DEFERRED_PREPASS
#else
    var input = pbr_input_from_standard_material(in, is_front);
    input.material.base_color = vec4(albedo, alpha);
    return shade_character(in, is_front, input, vec4(t2.r, 1.0, select(t2.a, 0.0, lip), 1.0));
#endif
}
