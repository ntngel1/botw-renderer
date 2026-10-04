// Far-tree billboards. Every tree is four corners at its base point; the
// vertex shader stands them up as a quad facing the camera (turning only
// around the vertical), picks the picture taken from the camera's side of
// the tree and hides trees still close enough for their model. In the
// shadow pass the quad faces the light instead, through the middle of the
// crown, so the tree casts its silhouette whatever the sun's height.
//
// Per vertex: position is the tree's origin; uv is the corner (0-1, v
// down); uv_b.x the tree's first atlas layer × 16 + its number of views,
// uv_b.y its yaw (radians); color.x the distance where its model hands over
// (measured to the origin, like the model's visibility range; it starts
// dissolving `DISSOLVE` of it earlier), color.y the bottom of the picture
// relative to the origin, color.z and w the picture's width and height (m).
// The fragment shader picks the picture and cuts the tree out as the game's
// far-tree pixel shader does (`uking_tree` program 7, PS
// `4b7ade61b236275c`, its VS `616104995c681afa`: docs/research/
// wiiu-field-shading.md, "Far tree: VS"), shows the part of the
// picture the model has given up (`botw::hand_off`) and lights it as the
// game's far-tree G-buffer
// (`uking_tree` program 7: docs/research/wiiu-field-shading.md, "Grass and
// far trees") holds it: foliage (`field_leaf`, class 7,
// `botw::deferred_light`) with the picture's own normals, light passing
// through where its translucency is above zero, darkened under cloud
// shadows, then haze (`botw::look`).

#import bevy_pbr::{
    view_transformations::position_world_to_clip,
    mesh_view_bindings::{globals, view},
}
#import botw::hand_off::{billboard_hidden, hand_off_level}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::prepass_io::{Vertex, VertexOutput}
#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput
#endif
#else
#import bevy_pbr::{
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
    view_transformations::{position_world_to_view, frag_coord_to_uv},
}
#import botw::clouds::{CloudParams, cloud_shadow}
#import botw::look::{read_look_at, apply_haze, sky_cover, field_wetness_at}
#import botw::deferred_light::{FieldSurface, field_color, game_frame}
#endif

const TAU: f32 = 6.28318530718;
// The share of the hand-off distance the dissolve takes (`DISSOLVE` in
// far_trees.rs).
const DISSOLVE: f32 = 0.2;

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var albedo: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var albedo_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var normals: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var cloud_shadow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var cloud_shadow_sampler: sampler;
#ifndef PREPASS_PIPELINE
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var<uniform> clouds: CloudParams;
#endif
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var dither_mask: texture_2d<f32>;
#ifndef PREPASS_PIPELINE
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var look_texture: texture_2d<f32>;
// `TreeDitherMask` as the game's pixel shader samples it (`tera_tree_mask`).
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var tree_mask: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var tree_mask_sampler: sampler;
// x: the atlas material's alpha-test reference (`Tree0` 0.5, `Tree1` 0.3).
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var<uniform> params: vec4<f32>;
// The environment's mean brightness, once a frame (deferred_light.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var cube_means: texture_2d<f32>;
#endif

// The shadow pass's cut-out (the game's shadow program is not read):
// pictures are empty outside the tree and 0.44-1 inside.
// SI-TRE-02: our alpha test and view choice.
const COVERAGE: f32 = 0.22;
// The game's fade-in of a billboard past its hand-off (the VS's `Sem1.x`,
// `f`) is 1 once 50 m past it; the viewer's own dissolve stands for the
// band before (`botw::hand_off`, GAPS RENDER-004), so the pixel shader
// sees f = 1 and the dither weight `Sem5.x = 0.1 + 0.1·16·(1 − f)` = 0.1.
// SI-TRE-02: f = 1 and Sem5.x = 0.1: the w16 band is not repeated.
const FADE: f32 = 1.0;
const DITHER_WEIGHT: f32 = 0.1 + 0.1 * 16.0 * (1.0 - FADE);

// A tree's quad corner, as both passes compute it.
struct Corner {
    position: vec3<f32>,
    // Horizontal direction the picture is seen from.
    side: vec2<f32>,
    layer: f32,
    level: i32,
}

fn horizontal(v: vec3<f32>) -> vec2<f32> {
    let len = length(v.xz);
    return select(vec2<f32>(0.0, 1.0), v.xz / len, len > 1e-3);
}

// Picture k shows the model turned by k/n of a full turn, seen from +Z:
// the one where that matches the side it is seen from.
// SI-TRE-02: our alpha test and view choice.
fn picture_layer(packed: f32, yaw: f32, side: vec2<f32>) -> f32 {
    let first = floor(packed / 16.0);
    let views = packed - first * 16.0;
    let turn = (yaw - atan2(side.x, side.y)) / TAU * views;
    let picture = (i32(round(turn)) % i32(views) + i32(views)) % i32(views);
    return first + f32(picture);
}

// The corner seen from the camera, or from the light in the shadow pass.
// SI-TRE-02: a quad over the AABB instead of the game's five vertices.
fn corner(vertex: Vertex, from_light: bool) -> Corner {
    var out: Corner;
    let origin = vertex.position;
    let base = origin + vec3<f32>(0.0, vertex.color.y, 0.0);
    let size = vertex.color.zw;
    let c = vertex.uv;
    // SI-TRE-03: far-tree shadow quad turned to the light is ours.
    if from_light {
        // A directional light's view looks along its light: its +Z points
        // back at the light.
        let light = normalize(view.world_from_view[2].xyz);
        out.side = horizontal(light);
        let right = vec3<f32>(out.side.y, 0.0, -out.side.x);
        let up = normalize(cross(light, right));
        let crown = base + vec3<f32>(0.0, size.y * 0.5, 0.0);
        out.position = crown + right * (c.x - 0.5) * size.x + up * (0.5 - c.y) * size.y;
    } else {
        out.side = horizontal(view.world_position - origin);
        // Screen right for a picture facing the camera.
        let right = vec3<f32>(out.side.y, 0.0, -out.side.x);
        out.position = base + right * (c.x - 0.5) * size.x + vec3<f32>(0.0, (1.0 - c.y) * size.y, 0.0);
    }
    // Close trees are real models: collapse their quads until the models
    // start dissolving (distance to the camera, like the models' visibility
    // ranges, in both passes).
    let hand_off = vertex.color.x;
    out.level = hand_off_level(length(view.lod_view_world_position - origin), hand_off * (1.0 - DISSOLVE), hand_off);
    if out.level == 0 {
        out.position = base;
    }
    out.layer = picture_layer(vertex.uv_b.x, vertex.uv_b.y, out.side);
    return out;
}

#ifdef PREPASS_PIPELINE

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let c = corner(vertex, true);
    out.world_position = vec4<f32>(c.position, 1.0);
    out.position = position_world_to_clip(c.position);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    out.uv = vertex.uv;
    out.uv_b = vec2<f32>(c.layer, 0.0);
    out.color = vec4<f32>(0.0, f32(c.level), 0.0, 1.0);
    return out;
}

fn outside(in: VertexOutput) -> bool {
    let layer = i32(in.uv_b.x + 0.5);
    let alpha = textureSampleLevel(albedo, albedo_sampler, in.uv, layer, 0.0).a;
    return alpha < COVERAGE || billboard_hidden(dither_mask, in.position.xy, i32(in.color.g + 0.5));
}

#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    if outside(in) {
        discard;
    }
    var out: FragmentOutput;
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
    if outside(in) {
        discard;
    }
}
#endif

#else

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let c = corner(vertex, false);
    // The mesh's flags and material slot are looked up by instance.
    out.instance_index = vertex.instance_index;
    out.world_position = vec4<f32>(c.position, 1.0);
    out.position = position_world_to_clip(c.position);
    out.world_normal = vec3<f32>(c.side.x, 0.0, c.side.y);
    out.uv = vertex.uv;
    // The mask's place on the tree (the VS's `Sem0.zw`): 0.6 × the picture's
    // width and height in metres (`gsys_user0[5].xy` = 1) × its uv.
    out.uv_b = 0.6 * vertex.color.zw * vertex.uv;
    // y the hand-off level, z the view phase, w the tree's layers (the
    // same for the whole quad, so interpolation keeps them whole); x is
    // unused.
    out.color = vec4<f32>(0.0, f32(c.level), view_phase(vertex.uv_b.x, vertex.uv_b.y, c.side), vertex.uv_b.x);
    return out;
}

// The VS's view phase `N·fract(yaw − φ + 1/(2N))`: the tree's turn in
// turns as its vertex byte holds it (`int(fract(rotY/360)·255)`), φ the
// direction from the tree to the camera, in turns from +Z towards +X.
fn view_phase(packed: f32, yaw: f32, side: vec2<f32>) -> f32 {
    let views = packed - floor(packed / 16.0) * 16.0;
    let turn = floor(fract(yaw / TAU) * 255.0) / 255.0;
    let phi = atan2(side.x, side.y) / TAU;
    return views * fract(turn - phi + 0.5 / views);
}

// The game's pixel shader's picture (`Sem1.w = phase + 0.5 − u`): the view
// shifts by one across the quad, dithered by the mask, so neighbouring
// views mix near the change.
fn game_layer(packed: f32, phase: f32, u: f32, mask: f32) -> i32 {
    let first = floor(packed / 16.0);
    let views = packed - first * 16.0;
    let w = phase + 0.5 - u;
    return i32(first + round(views * fract((w - 0.25 * mask) / views) - 0.5));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let mask = textureSample(tree_mask, tree_mask_sampler, in.uv_b).r;
    let layer = game_layer(in.color.w, in.color.z, in.uv.x, mask);
    let color = textureSample(albedo, albedo_sampler, in.uv, layer);
    // Normal x and y (picture right and up) in red and green,
    // translucency in blue.
    let packed = textureSample(normals, albedo_sampler, in.uv, layer).rgb;
    // The game's alpha test: f·(2a − 0.5 − sat(4a − 3)) + 0.07 +
    // Sem5.x·(mask − 1) against the material's reference.
    let cut = FADE * (2.0 * color.a - 0.5 - saturate(4.0 * color.a - 3.0)) + 0.07 + DITHER_WEIGHT * (mask - 1.0);
    if cut < params.x || billboard_hidden(dither_mask, in.position.xy, i32(in.color.g + 0.5)) {
        discard;
    }

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = vec4<f32>(color.rgb, 1.0);
    let xy = packed.rg * 2.007874 - 1.007874;
    let facing = normalize(in.world_normal);
    let right = vec3<f32>(facing.z, 0.0, -facing.x);
    let normal = normalize(right * xy.x + vec3<f32>(0.0, xy.y, 0.0) + facing * sqrt(saturate(1.0 - dot(xy, xy))));
    pbr_input.N = normal;
    pbr_input.world_normal = normal;

    // The G-buffer: the picture's albedo as it is, no gloss, metal or
    // normal.w flag; light passes through (albedo.a bit 1) wherever the
    // translucency is above zero (leaves; trunks hold 0).
    var surface: FieldSurface;
    surface.albedo = color.rgb;
    surface.N = normal;
    surface.gloss = 0.0;
    surface.metal = 0.0;
    surface.flag = 0.0;
    surface.leaf = 1.0;
    surface.translucent = select(0.0, 1.0, packed.b > 0.0);
    let world = in.world_position.xyz;
    let frame = game_frame(position_world_to_view(world), frag_coord_to_uv(in.position.xy));
    let look = read_look_at(look_texture, world);
    let cover = sky_cover(look_texture, look, world);
    // The cloud shadow at the pixel: the game's far trees are G-buffer
    // class 7, shaded by the foliage's pre-shading (PS 108), which reads it
    // at each pixel's world position like the ground's.
    let shade = 1.0 - cloud_shadow(cloud_shadow_map, cloud_shadow_sampler, clouds, world);
    let rain = vec2<f32>(field_wetness_at(look_texture, look, world, cover, surface.N.y, surface.flag), look.weather[0].y);
    var lit = field_color(pbr_input, surface, frame, shade, 1.0, cover, cube_means, rain);
    lit = apply_haze(look, lit, world, normalize(world - view.world_position));
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(lit, 1.0));
    return out;
}

#endif
