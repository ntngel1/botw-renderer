// Grass blades: bent by the game's grass wind as its blade shader bends
// them, drawn and turned to the far colour by the game's distances for
// their type (vertex), and the game's blade G-buffer (`uking_grass_blade`,
// programs 7 and 11: docs/research/wiiu-field-shading.md, "Grass") lit by
// its deferred shading of the field (`field_hybrid`, class 4, with no
// ambient occlusion or rim light: `botw::deferred_light`), then haze
// (`botw::look`).
//
// Per vertex, the game's blade buffer (grass/buffer.rs; one material per
// type): the position the blade's root, on the ground; uv the game's
// (u/2, 1 − row/3), u across the blade (0–2), row up it (0 root, 2 middle,
// 3 tip); uv_b.x the blade's place in the order a thinned cell draws,
// uv_b.y its two random bytes (`Sem8`: phase + 256 × length); tangent.xz
// its lean (`Sem1`), tangent.yw the middle of its 3 m cell; normal the
// ground's; color.rgb the map's grass colour there (`.grass.extm`, 0-1),
// color.a the map's grass (its height / 255, the game's `summary0.w`).

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
#import botw::deferred_light::{FieldSurface, field_color, game_frame}

struct GrassSettings {
    // The game's grass wind, `gsys_environment` 32, 33, 57, 58, 59, 60
    // (grass/wind.rs): e32 = (strength, direction x z, swell clock), e33 =
    // (share of the previous direction w, previous direction, 1 − w), e57 =
    // both directions weighted, e58–e60 the gust spots.
    wind: array<vec4<f32>, 6>,
    // The material's swell: frequency and per-blade dispersion over the world
    // coefficient, the swell's scale, the world coefficient.
    swell: vec4<f32>,
    // `gsys_environment` 34, 35, 36: uv = xz·scale + offset of `grass_lie`,
    // `grass_mow` and `grass_mow_wide` (grass/interact.rs).
    maps: array<vec4<f32>, 3>,
    // Per blade type (grass/lod.rs): the apparent size 1/(d·k) where the
    // full count ends, where the last blade goes, the far colour's share
    // (`uking_grass_lod_color.w`), the width per unit of lean
    // (`gsys_shape[2].x`).
    lod: array<vec4<f32>, 2>,
    // 1 once the `grass_color` map is filled; the blade height (`tera+0x10fc`
    // × `T.2`, three times `gsys_shape[2].y`); this material's blade type;
    // its blades per cell.
    flags: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> settings: GrassSettings;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var cloud_shadow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var cloud_shadow_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<uniform> clouds: CloudParams;
// The shared look values (see look.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var look_texture: texture_2d<f32>;
// The game's blade (`GrassAlb`): colour along one blade, tip at the top.
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var blade_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var blade_sampler: sampler;
// The game's `grass_color` (grass/color_map.rs): the mean colour of the
// terrain's materials under the grass, 47×47 cells of 3 m, wrapping.
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var ground_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var ground_sampler: sampler;
const GROUND_MAP_SPAN: f32 = 3.0 * 47.0;
// The game's `grass_wind_swell` (grass/wind.rs): u along the wind, v its
// strength.
@group(#{MATERIAL_BIND_GROUP}) @binding(109) var swell_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(110) var swell_sampler: sampler;
// The game's grass interaction maps around the camera (grass/interact.rs):
// mowing (x·y, 1 not mown), the wide mowing's .z and .w (1 not mown), and
// the lie (0.5 upright).
@group(#{MATERIAL_BIND_GROUP}) @binding(111) var mow_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(112) var mow_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(113) var mow_wide_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(114) var mow_wide_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(115) var lie_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(116) var lie_sampler: sampler;
// The environment's mean brightness, once a frame (deferred_light.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(117) var cube_means: texture_2d<f32>;

// The map's usual grass colour (24, 52, 8 of 255 in `.grass.extm`): the
// blade shader's reference, from which the map's colour shifts the blade's.
const MAP_GREEN: vec3<f32> = vec3<f32>(0.0941, 0.204, 0.0314);
// The colour the blades turn to where they fade out far away (the far
// cards' own base, `uking_grass_cross`), its red lower where the map's
// green is below the reference.
const FAR_GREEN: vec3<f32> = vec3<f32>(0.09, 0.15, 0.04);
const FAR_RED_DARK: f32 = 0.081;
// tan(20°/2), the least half-angle tangent of the grass distances.
const MIN_K: f32 = 0.17632698;
const TAU: f32 = 6.2831853;
// How far a blade sags below its lean where not mown (`gsys_shape[2].z`,
// the blade class's `p[7]`, `0x037026c4`); mown, 0.62.
const BLADE_SAG: f32 = 0.12;
const MOWN_SAG: f32 = 0.62;

// The swell under a blade's root along one of the wind's directions (the
// blade shader's two `grass_wind_swell` reads).
fn swell_along(direction: vec2<f32>, root: vec2<f32>, clock: f32) -> f32 {
    let u = settings.swell.w * (clock - dot(direction, root));
    return textureSampleLevel(swell_texture, swell_sampler, vec2<f32>(u, settings.wind[0].x), 0.0).x;
}

// The game's blade wind (`uking_grass_blade`, VS `7f39f0470fc4928a`): the
// swell along the current and the previous direction, plus a flutter of the
// blade's own phase, weighted by the two directions (e57): metres, x and z.
fn blade_wind(root: vec2<f32>, phase: f32) -> vec2<f32> {
    let e32 = settings.wind[0];
    let e33 = settings.wind[1];
    let e57 = settings.wind[2];
    let clock = settings.swell.x * e32.w + settings.swell.y * phase;
    let turn = phase + e32.w;
    let flutter = 0.057144 * saturate(e32.x - 0.3) * sin(TAU * fract(200.0 * turn))
        + 0.02 * saturate(2.0 * min(e32.x, 1.0 - e32.x)) * sin(TAU * fract(50.0 * turn));
    // The swell's scale × `gsys_shape[2].y` (a third of the blade height).
    let lift = settings.swell.z * settings.flags.y / 3.0;
    let a1 = flutter + lift * swell_along(e32.yz, root, clock);
    let a2 = flutter + lift * swell_along(e33.yz, root, clock);
    return e57.xy * a1 + e57.zw * a2;
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let base = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(vertex.position, 1.0)).xyz;
    let cell = (world_from_local * vec4<f32>(vertex.tangent.y, 0.0, vertex.tangent.w, 1.0)).xz;
    let u = 2.0 * vertex.uv.x;
    let row = 3.0 * (1.0 - vertex.uv.y);
    let length_byte = floor(vertex.uv_b.y / 256.0);
    let phase = (vertex.uv_b.y - 256.0 * length_byte) / 255.0;
    let length_random = length_byte / 255.0;
    let sem1 = vec3<f32>(vertex.tangent.x, 0.0, vertex.tangent.z);
    let grass = vertex.color.a;
    let lod = settings.lod[u32(settings.flags.z)];

    // The game's grass distances scale with k = max(tan 10°, tan(fovy/2)).
    // A cell draws the share f = sat((1/(d·k) − .y)/(.x − .y)) of its
    // blades (`0x03703140`), the first ⌊N²·f⌋ of the thinning order, all
    // above 0.99 (`0x03925f64`); d here from the cell's middle at the
    // blade's ground.
    // SI-GRS-04: fade without gsys_shape_ex[3].y; no shortening near the camera.
    let distance = length(base - view.world_position);
    let k = max(MIN_K, 1.0 / view.clip_from_view[1][1]);
    let cell_distance = length(vec3<f32>(cell.x, base.y, cell.y) - view.world_position);
    let size = 1.0 / max(cell_distance * k, 1e-4);
    let share = saturate((size - lod.y) / (lod.x - lod.y));
    let drawn = share > 0.99 || vertex.uv_b.x < floor(settings.flags.w * share);
    let keep = select(0.0, 1.0, drawn);

    // The game's blade (VS `7f39f0470fc4928a`): a third of its length H =
    // S·T.2/3 × the map's grass × (0.75 + length/2), shorter where mown
    // (`grass_mow`.x·y, half where `grass_mow_wide.w` is 0; the shortening
    // near the camera by `view+0x2a0` is not run). From the root two
    // segments of 1.5H, each along its lean over its sag (the grass's lie
    // adds to the lean and flattens it) plus the wind, the upper with 1.5 ×
    // the wind; the wide map's mown grass sags more and takes no wind.
    let mow = textureSampleLevel(mow_texture, mow_sampler, base.xz * settings.maps[1].xy + settings.maps[1].zw, 0.0).xy;
    let mown_wide = textureSampleLevel(mow_wide_texture, mow_wide_sampler, base.xz * settings.maps[2].xy + settings.maps[2].zw, 0.0).w;
    let pressed = textureSampleLevel(lie_texture, lie_sampler, base.xz * settings.maps[0].xy + settings.maps[0].zw, 0.0).xy;
    let standing = 0.5 + 0.5 * mown_wide;
    let lie = vec2<f32>(2.0 * pressed.x - 1.0, 1.0 - 2.0 * pressed.y) * standing;
    // SI-GRS-04: share taken at the cell's middle.
    let third = keep * mow.x * mow.y * standing * (0.75 + 0.5 * length_random) * grass * settings.flags.y / 3.0;
    let wind = blade_wind(base.xz, phase) * mown_wide;
    let rise = third * (max(1.0 - dot(lie, lie), 0.2) - mix(MOWN_SAG, BLADE_SAG, mown_wide));
    let direction = sem1 + 0.5 * vec3<f32>(lie.x, 0.0, lie.y);
    let leaning = direction * third + vec3<f32>(0.0, rise, 0.0);
    let lower = normalize(leaning + vec3<f32>(wind.x, 0.0, wind.y));
    let upper = normalize(leaning + 1.5 * vec3<f32>(wind.x, 0.0, wind.y));
    let half_length = 1.5 * third;
    let middle = base + lower * half_length;
    let tip = middle + upper * half_length;
    // Across the blade: its lean (lie′ + 2·Sem1) turned a quarter, × w(1 −
    // w/2) × the type's width, u − 1 of it.
    let side = keep * 2.0 * vec3<f32>(-direction.z, 0.0, direction.x) * (1.0 - 0.5 * grass) * grass * lod.w;
    let along = select(select(base, middle, row > 0.0), tip, row > 2.0);
    let position = along + (u - 1.0) * side;

    out.world_position = vec4<f32>(position, 1.0);
    out.position = position_world_to_clip(position);
    // The game's blade normal: the ground's plus 0.3 × sat(row/3) × the
    // upper segment, not normalized.
    let ground = mesh_functions::mesh_normal_local_to_world(vertex.normal, vertex.instance_index);
    out.world_normal = ground + 0.3 * saturate(row / 3.0) * upper * half_length;
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    // How far the blade has turned to the far colour (y): sat(w − r)², r
    // falling from 1 to 0 until its type's full count ends (`gsys_shape[3]`,
    // `0x03702c18`). x is unused.
    let last = 1.0 / (k * lod.y);
    let span = max(last - 1.0 / (k * lod.x), 0.1);
    let s3 = vec2<f32>(last / span, 1.0 / span);
    let r = saturate((s3.x - distance * s3.y - 1.0) / (s3.x - 1.0));
    // SI-GRS-04: fade without gsys_shape_ex[3].y; no shortening near the camera.
    let far = saturate(lod.z - r);
    out.uv_b = vec2<f32>(0.0, far * far);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(
        vertex.instance_index, world_from_local[3]);
#endif
    return out;
}

// The blade's albedo as the game's G-buffer holds it (layer 0, grass not
// mown): the blade texture, blended with the ground's colour (`grass_color`)
// where it is dark, turning to the far colour as it fades, shifted by the
// map's grass colour against the reference in proportion to the blade's
// green.
// SI-GRS-04: albedo without mow_wide.z/.w, Sem0.w; layer 0.
fn blade_albedo(map: vec3<f32>, ground: vec3<f32>, blade: vec3<f32>, fade: f32) -> vec3<f32> {
    let shift = map - MAP_GREEN;
    let below = shift.y < 0.0;
    let g = select(blade.g, mix(blade.g, 0.11, fade), below);
    let far = vec3<f32>(select(FAR_GREEN.x, FAR_RED_DARK, below), FAR_GREEN.y, FAR_GREEN.z);
    let base = mix(mix(ground, blade, saturate(10.0 * g)), far, fade);
    return base + shift * (6.0 * g - 0.06);
}

// The blade's gloss (G-buffer normal.w): up to 0.4 on one side of the
// upper blade, in the G-buffer's 6 bits.
fn blade_gloss(uv: vec2<f32>) -> f32 {
    return floor(25.5 * saturate(uv.x * (10.0 - 20.0 * uv.y) - 3.0)) / 63.0;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let blade = textureSample(blade_texture, blade_sampler, in.uv).rgb;
    // The game reads the map at the blade's root; here at the fragment
    // (a blade leans well under a cell). Before the map is filled, the
    // map's grass colour stands in.
    let world = in.world_position.xyz;
    let mapped = textureSample(ground_map, ground_sampler, world.xz / GROUND_MAP_SPAN).rgb;
    let ground = select(in.color.rgb, mapped, settings.flags.x > 0.5);
    let albedo = blade_albedo(in.color.rgb, ground, blade, in.uv_b.y);
    pbr_input.material.base_color = vec4<f32>(albedo, 1.0);

    var surface: FieldSurface;
    surface.albedo = albedo;
    surface.N = normalize(in.world_normal);
    surface.gloss = blade_gloss(in.uv);
    // No metal, no light through the blade (albedo.a 0); normal.w's flag
    // 0: no ambient occlusion, no rim light.
    surface.metal = 0.0;
    surface.flag = 0.0;
    surface.leaf = 0.0;
    surface.translucent = 0.0;
    let frame = game_frame(position_world_to_view(world), frag_coord_to_uv(in.position.xy));
    let look = read_look_at(look_texture, world);
    let cover = sky_cover(look_texture, look, world);
    // The cloud shadow at the pixel, like the ground: the game's blades are
    // G-buffer class 4, shaded by the terrain's pre-shading (PS 112).
    let shade = cloud_shadow(cloud_shadow_map, cloud_shadow_sampler, clouds, world);
    let rain = vec2<f32>(field_wetness_at(look_texture, look, world, cover, surface.N.y, surface.flag), look.weather[0].y);
    var lit = field_color(pbr_input, surface, frame, 1.0 - shade, 1.0, cover, cube_means, rain);
    lit = apply_haze(look, lit, world, normalize(world - view.world_position));
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(lit, 1.0));
    return out;
}
