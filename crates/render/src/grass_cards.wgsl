// Grass tufts (`grass/cards.rs`): the game's tuft cards, standing along the
// ground's normal, that grow in and shrink away over their type's distances,
// bend in the game's grass wind and catch the sun in its gusts, cut out by
// the tuft's alpha and on steep ground, with the game's tuft G-buffer
// (`uking_grass_cross`, programs 3 and 7: docs/research/wiiu-field-shading.md,
// "Grass") lit like the blades (`field_hybrid` with no ambient occlusion or
// rim light, `botw::deferred_light`).
//
// Per vertex: position the ground under it (the game's vertex shader takes
// each vertex's own place in `grass_summary0`); uv the card's texture (the
// game's `Sem1`, v 1 at the ground); uv_b.x the type's full height (`tera+0x10fc` × `T.1`: 1.56 m, 1.44 m),
// uv_b.y the grass amount there (0-1, the game's `grass_summary0.w`), which
// scales it; normal the ground's; color.rgb the map's
// grass colour there (`.grass.extm`, 0-1), color.a the tuft's type (0 or 1).

#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    view_transformations::{position_world_to_clip, position_world_to_view, frag_coord_to_uv},
    mesh_view_bindings::{globals, view},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::main_pass_post_lighting_processing,
}
#import botw::clouds::{CloudParams, cloud_shadow}
#import botw::look::{read_look_at, apply_haze, sky_cover, field_wetness_at}
#import botw::deferred_light::{FieldSurface, field_color, game_frame, main_light}

struct GrassSettings {
    // The game's grass wind, `gsys_environment` 32, 33, 57, 58, 59, 60
    // (grass/wind.rs): e32 = (strength, direction x z, swell clock), e33 =
    // (share of the previous direction w, previous direction, 1 − w), e57 =
    // both directions weighted, e58 = (swell max, min, spot phase, spot
    // shift), e59 and e60 the gust spots' directions.
    wind: array<vec4<f32>, 6>,
    // The material's swell: frequency and dispersion over the world
    // coefficient, the swell's scale, the world coefficient.
    swell: vec4<f32>,
    // `gsys_environment` 34, 35, 36: uv = xz·scale + offset of `grass_lie`,
    // `grass_mow` and `grass_mow_wide` (grass/interact.rs).
    maps: array<vec4<f32>, 3>,
    // Per tuft type (grass/lod.rs), metres at tan 25°: where tufts start
    // growing in, over how far, where the last shrinks away, over how far.
    lod: array<vec4<f32>, 2>,
    // Unused.
    flags: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> settings: GrassSettings;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var cloud_shadow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var cloud_shadow_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<uniform> clouds: CloudParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var look_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var tuft: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var tuft_sampler: sampler;
// The game's `grass_wind_swell` (grass/wind.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var swell_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var swell_sampler: sampler;
// The game's `grass_mow_wide` (1 not mown) and `grass_lie` (0.5 upright)
// around the camera (grass/interact.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var mow_wide_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var mow_wide_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var lie_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(112) var lie_sampler: sampler;
// The environment's mean brightness, once a frame (deferred_light.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(113) var cube_means: texture_2d<f32>;

// The map's usual grass colour, the reference its colour shifts from (as
// on the blades, `grass.wgsl`).
const MAP_GREEN: vec3<f32> = vec3<f32>(0.0941, 0.204, 0.0314);
// The tuft's base colour where the grass is not mown (the vertex shader's
// `Sem2`).
const TUFT_GREEN: vec3<f32> = vec3<f32>(0.09, 0.15, 0.04);
// tan(20°/2) and tan 25°: the least half-angle tangent of the grass
// distances and the one the table is made for.
const MIN_K: f32 = 0.17632698;
const REFERENCE_K: f32 = 0.46631;
const TAU: f32 = 6.2831853;

// The swell under a tuft's root along one of the wind's directions.
fn swell_along(direction: vec2<f32>, root: vec2<f32>) -> f32 {
    let u = settings.swell.w * (settings.swell.x * settings.wind[0].w - dot(direction, root));
    return textureSampleLevel(swell_texture, swell_sampler, vec2<f32>(u, settings.wind[0].x), 0.0).x;
}

// Two saw waves rolling along the gust spots' directions (e59, e60 against
// e58.w), each blended from the previous direction to the current.
fn gust_saw(spots: vec4<f32>, root: vec2<f32>) -> f32 {
    let shift = settings.wind[3].w;
    return mix(fract(dot(spots.xy, root) - shift), fract(dot(spots.zw, root) - shift), settings.wind[1].x);
}

fn gust_wave(direction: vec2<f32>, root: vec2<f32>) -> f32 {
    let a = TAU * 3.0 * fract(0.0212766 * dot(direction, root)) - settings.wind[3].z;
    return sin(a) + sin(a * 5.0 / 3.0);
}

// The game's gust (the tuft shader's `Sem4.w`, VS `9fb39f15a611f4ec`):
// spots of the two saws squared, under a wave 47 m long, rolling with the
// wind; none within 15 m of the camera, full beyond 25 m, as strong as the
// wind and the grass there. `amount` the grass amount, `mown` its mowing.
fn tuft_gust(root: vec3<f32>, amount: f32, mown: f32, to_light: vec3<f32>) -> f32 {
    let e32 = settings.wind[0];
    let spot = gust_saw(settings.wind[4], root.xz) * gust_saw(settings.wind[5], root.xz);
    let wave = 0.5 * mix(gust_wave(e32.yz, root.xz), gust_wave(settings.wind[1].yz, root.xz), settings.wind[1].x);
    let near = saturate(0.1 * length(root - view.world_position) - 1.5);
    let gust = 0.8 * saturate(1.25 * e32.x * amount * mown * spot * spot * wave * near);
    // (Only a light straight along ±x turns it off.)
    return gust * saturate(200.0 - 200.0 * abs(to_light.x));
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let base = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0)).xyz;
    let along = 1.0 - vertex.uv.y;
    let ground = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);

    // The game's tuft size: sat((F₁ − d)/L₁)·sat((d − N₁)/L₂) (the vertex
    // shader's `gsys_shape[3]`, `0x03704e10`), every distance × tan 25°/k,
    // k = max(tan 10°, tan(fovy/2)).
    let distance = length(base - view.world_position);
    // SI-GRS-05: grass LOD k from the viewer's 45 degree FOV.
    let lod = settings.lod[select(0u, 1u, vertex.color.a >= 0.5)] * (REFERENCE_K / max(MIN_K, 1.0 / view.clip_from_view[1][1]));
    let keep = saturate((lod.z - distance) / lod.w) * saturate((distance - lod.x) / lod.y);

    // Mown tufts: `grass_mow_wide` y × clamp(x, 0.35, 1.35), half where .w
    // is 0 (the wide map, 1 m texels, remembers cuts over 512 m).
    let wide = textureSampleLevel(mow_wide_texture, mow_wide_sampler, base.xz * settings.maps[2].xy + settings.maps[2].zw, 0.0);
    let standing = 0.5 + 0.5 * wide.w;
    let mown = standing * wide.y * clamp(wide.x, 0.35, 1.35);
    // Pressed flat (`grass_lie`), fading out over the map's outer tenth.
    let lie_uv = base.xz * settings.maps[0].xy + settings.maps[0].zw;
    let pressed = textureSampleLevel(lie_texture, lie_sampler, lie_uv, 0.0).xy;
    let lie_edge = saturate(5.0 - abs(10.0 * lie_uv - 5.0));
    let lie = vec2<f32>(2.0 * pressed.x - 1.0, 1.0 - 2.0 * pressed.y) * standing * lie_edge.x * lie_edge.y;

    // The game's tuft wind: the bend b (metres, x and z) is the swell along
    // the current and the previous direction (0.3 × the type's full height
    // × the swell) plus 0.15 × the gust, weighted by the two directions
    // (e57); the grass's lie adds to it.
    let to_light = -normalize(clouds.sun.xyz);
    let gust = tuft_gust(base, vertex.uv_b.y, mown, to_light);
    let lift = 0.3 * vertex.uv_b.x * settings.swell.z;
    let a1 = 0.15 * gust + lift * swell_along(settings.wind[0].yz, base.xz);
    let a2 = 0.15 * gust + lift * swell_along(settings.wind[1].yz, base.xz);
    let bend = lie + settings.wind[2].xy * a1 + settings.wind[2].zw * a2;
    // The point h up the tuft stands along the ground's normal leaned by
    // 2h·b: P = root + h·normalize(n + 2h·b).
    // The tuft's share f = grass × mown × keep (the game's `s0.w·m·r₃`).
    let size = keep * mown * vertex.uv_b.y;
    let h = along * vertex.uv_b.x * size;
    let position = base + h * normalize(ground + 2.0 * h * vec3<f32>(bend.x, 0.0, bend.y));

    out.world_position = vec4<f32>(position, 1.0);
    out.position = position_world_to_clip(position);
    // The tuft's normal before the pixel shader's blend (the vertex shader's
    // `Sem3`): the ground's plus the bend × (1 − v), drawn a tenth of the
    // gust towards the light's direction.
    let bent = ground + along * vec3<f32>(bend.x, 0.0, bend.y);
    out.world_normal = mix(bent, to_light, 0.1 * gust);
#ifdef VERTEX_UVS_A
    // The tuft sinks into the ground rather than shrinking: its texture's v
    // is scaled with its height (`Sem1.y = v·size`), so the top keeps its
    // scale and the rest is below the ground.
    out.uv = vec2<f32>(vertex.uv.x, vertex.uv.y * size);
#endif
#ifdef VERTEX_UVS_B
    // The gust (y); x is unused.
    out.uv_b = vec2<f32>(0.0, gust);
#endif
#ifdef VERTEX_COLORS
    // The map's colour; in alpha the slope's cut (the game's `Sem2.w`):
    // tufts fade out of the alpha test where the ground's normal falls
    // below y 0.6 (the tuft's type is the vertex shader's only).
    out.color = vec4<f32>(vertex.color.rgb, min(1000.0 * ground.y - 600.0, 1.0));
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

// The tuft's albedo as the game's G-buffer holds it (grass not mown): the
// base colour shifted by the map's colour against the reference, as far
// as the tuft's green reaches (its colour is not read, only green and
// alpha).
fn tuft_albedo(map: vec3<f32>, tuft_green: f32) -> vec3<f32> {
    var shift = map - MAP_GREEN;
    var g = tuft_green;
    if shift.y < 0.0 {
        shift = vec3<f32>(shift.x * 1.15, shift.y * 0.95, shift.z);
        g = 0.1 * (g - 0.09) + 0.09;
    }
    return TUFT_GREEN + shift * saturate(min(8.0 * g - 0.08, 0.8));
}

// The tuft's normal as the game's G-buffer holds it: over the upper half
// the gust blends the bent normal towards the half vector between the
// camera and the light (the vertex shader's `Sem4`, there from the tuft's
// root, here from the pixel), so a gust lights the tufts' sheen.
fn tuft_normal(bent: vec3<f32>, to_camera: vec3<f32>, to_light: vec3<f32>, gust: f32, v: f32) -> vec3<f32> {
    let half_vector = normalize(to_camera + to_light);
    return normalize(mix(bent, half_vector, saturate(min(gust, 1.0 - 2.0 * v))));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let texel = textureSample(tuft, tuft_sampler, in.uv);
    // `Sem2.w·a`, against the material's alpha test (`Cross1/2` render
    // state: 0.5, pass at or above).
    if in.color.a * texel.a < 0.5 {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let albedo = tuft_albedo(in.color.rgb, texel.g);
    pbr_input.material.base_color = vec4<f32>(albedo, 1.0);

    let world = in.world_position.xyz;
    let frame = game_frame(position_world_to_view(world), frag_coord_to_uv(in.position.xy));
    var surface: FieldSurface;
    surface.albedo = albedo;
    surface.N = normalize(in.world_normal);
    if in.uv_b.y > 0.0 {
        let to_light = main_light(pbr_input, frame.p.z).to_light;
        surface.N = tuft_normal(in.world_normal, pbr_input.V, to_light, in.uv_b.y, in.uv.y);
    }
    // Up to 0.3 over the upper half, in the G-buffer's 6 bits.
    surface.gloss = floor(19.125 * saturate(1.0 - 2.0 * in.uv.y)) / 63.0;
    surface.metal = 0.0;
    surface.flag = 0.0;
    surface.leaf = 0.0;
    surface.translucent = 0.0;
    let look = read_look_at(look_texture, world);
    let cover = sky_cover(look_texture, look, world);
    // The cloud shadow at the pixel, like the ground: the game's tufts are
    // G-buffer class 4, shaded by the terrain's pre-shading (PS 112).
    let shade = cloud_shadow(cloud_shadow_map, cloud_shadow_sampler, clouds, world);
    let rain = vec2<f32>(field_wetness_at(look_texture, look, world, cover, surface.N.y, surface.flag), look.weather[0].y);
    var lit = field_color(pbr_input, surface, frame, 1.0 - shade, 1.0, cover, cube_means, rain);
    lit = apply_haze(look, lit, world, normalize(world - view.world_position));
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(lit, 1.0));
    return out;
}
