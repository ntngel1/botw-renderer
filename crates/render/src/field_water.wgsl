// botw::field_water — the game's deferred shading of a water pixel
// (G-buffer material id 22: `preshading_field_water`, uking_sys 140–143,
// and `field_water`, 24–27; docs/research/wiiu-deferred-shading.md), and
// what its G-buffer programs read of the frame (the opaque scene behind and
// its depth). Shared by the terrain's water (water_material.wgsl) and the
// models' water and glass (model_water.wgsl), which the game lights alike
// (docs/research/model-water-glass.md).
//
// Drawn in the transmissive phase: the scene behind is the view's
// transmission texture, its depth the camera's depth prepass.

#define_import_path botw::field_water

#import bevy_pbr::{
    mesh_view_types,
    shadows,
    mesh_view_bindings as view_bindings,
    mesh_view_bindings::{view, lights, light_probes},
    view_transformations::{depth_ndc_to_view_z, position_world_to_view},
}
#import botw::look::{Look, diffuse_gain, apply_haze}
#import botw::clouds::{CloudParams, cloud_shadow}
#import botw::deferred_light::cube_at

// View depth (m) where the depth buffer has nothing (the sky).
const NO_GROUND: f32 = 1.0e5;

// The game's unpacking of a normal map's channels: 255/127 · x − 128/127
// (native literals 0x40008102, 0xbf810204), so that 128/255 is flat.
fn unpack_normal(x: vec2<f32>) -> vec2<f32> {
    return x * bitcast<f32>(0x40008102u) + bitcast<f32>(0xbf810204u);
}

// Depth (reverse Z, 0 = nothing) of the opaque scene at screen position `uv`.
fn scene_depth(uv: vec2<f32>) -> f32 {
#ifdef DEPTH_PREPASS
    let size = vec2<f32>(textureDimensions(view_bindings::depth_prepass_texture));
    let texel = vec2<i32>(clamp(uv * size, vec2(0.0), size - 1.0));
    return textureLoad(view_bindings::depth_prepass_texture, texel, 0);
#else
    return 0.0;
#endif
}

// Distance along the view axis (m) to the opaque scene at screen position
// `uv`, what the game rebuilds from its normalized linear depth.
fn scene_distance(uv: vec2<f32>) -> f32 {
    let depth = scene_depth(uv);
    return select(NO_GROUND, -depth_ndc_to_view_z(depth), depth > 0.0);
}

// The scene behind the water at screen position `uv` (before any water was
// drawn), pre-exposed like the frame.
fn scene_behind(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(view_bindings::view_transmission_texture, view_bindings::view_transmission_sampler, uv, 0.0).rgb;
}

// The sky's radiance towards `dir` from the view's environment map (the
// atmosphere's), pre-exposed.
fn sky_radiance(dir: vec3<f32>, lod: f32) -> vec3<f32> {
#ifdef ENVIRONMENT_MAP
    if light_probes.view_cubemap_index < 0 {
        return vec3(0.0);
    }
    let q = light_probes.view_rotation;
    let t = 2.0 * cross(q.xyz, dir);
    var d = dir + q.w * t + cross(q.xyz, t);
    d.z = -d.z;
#ifdef MULTIPLE_LIGHT_PROBES_IN_ARRAY
    let radiance = textureSampleLevel(view_bindings::specular_environment_maps[light_probes.view_cubemap_index], view_bindings::environment_map_sampler, d, lod).rgb;
#else
    let radiance = textureSampleLevel(view_bindings::specular_environment_map, view_bindings::environment_map_sampler, d, lod).rgb;
#endif
    return radiance * light_probes.intensity_for_view * view.exposure;
#else
    return vec3(0.0);
#endif
}

// `color` seen through `dist` metres of air along `dir`: dimmed by the
// air's extinction near the ground (Earth's Rayleigh and Mie at sea level,
// what Bevy's `ScatteringMedium::earth` uses) and filled with the sky's
// light from just above the horizon in that direction; `strength` scales
// the distance.
// SI-WAT-03: aerial perspective over water is our own.
fn aerial_perspective(color: vec3<f32>, dist: f32, dir: vec3<f32>, strength: f32) -> vec3<f32> {
    let extinction = vec3<f32>(5.802e-6, 13.558e-6, 33.1e-6) + vec3(4.44e-6);
    let through = exp(-extinction * dist * strength);
    let towards = normalize(vec3<f32>(dir.x, max(dir.y, 0.03), dir.z));
    return color * through + sky_radiance(towards, 0.0) * (1.0 - through);
}

// The sun's share of the game's deferred water shading (`field_water`,
// docs/research/wiiu-deferred-shading.md): x = its diffuse factor without
// N·L, `1 − Fresnel(L·H)`; the returned colour is the sun's GGX glint
// (F0 0.02, no 1/π: the game's D, divided by π here like the diffuse
// light so that both keep the game's ratio to each other). `n` is the
// surface normal, `v` points to the eye, `gloss` is the G-buffer gloss.
// `visibility` is the sun's share that reaches the surface (the game's
// pre-shading shadow: cloud shadow × cascaded shadow, here Bevy's cascades
// and the viewer's clouds), 0 without a main light.
struct SunShare {
    diffuse: vec3<f32>,
    glint: vec3<f32>,
    visibility: f32,
}

fn sun_share(
    n: vec3<f32>,
    v: vec3<f32>,
    gloss: f32,
    world: vec3<f32>,
    view_z: f32,
    frag: vec2<f32>,
    shadow_map: texture_2d<f32>,
    shadow_sampler: sampler,
    clouds: CloudParams,
) -> SunShare {
    var share: SunShare;
    share.diffuse = vec3(0.0);
    share.glint = vec3(0.0);
    share.visibility = 0.0;
    for (var i = 0u; i < lights.n_directional_lights; i++) {
        // The main light (`daynight.rs`: the game's `dir_main`, the sun by
        // day, the night light at night) is the only one.
        let light = lights.directional_lights[i];
        let l = light.direction_to_light;
        let h = normalize(l + v);
        let n_dot_l = saturate(dot(n, l));
        let n_dot_v = saturate(dot(n, v));
        let n_dot_h = saturate(dot(n, h));
        let l_dot_h = saturate(dot(l, h));
        let rough = 1.0 - gloss;
        // α = (1 − gloss)² + 0.01 (0x3c23d70a), squared for D.
        let alpha = saturate(rough * rough + bitcast<f32>(0x3c23d70au));
        let a2 = alpha * alpha;
        let k = (0.5 + 0.5 * rough) * (0.5 + 0.5 * rough) / 2.0;
        let dn = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
        let d = a2 / (dn * dn);
        let f_lh = pow(1.0 - l_dot_h, 5.0);
        let fresnel = f_lh + 0.02 * (1.0 - f_lh);
        var visibility = 1.0 - cloud_shadow(shadow_map, shadow_sampler, clouds, world);
        if (light.flags & mesh_view_types::DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            visibility *= shadows::fetch_directional_shadow(i, vec4<f32>(world, 1.0), n, view_z, frag);
        }
        let radiance = light.color.rgb * view.exposure / 3.14159265;
        // saturate(shadow + 0.25): the sun still lights the water's body
        // a little in shadow.
        share.diffuse = radiance * saturate(visibility + 0.25) * (1.0 - f_lh);
        share.glint = radiance * visibility * fresnel * 0.25 / (k + n_dot_v * (1.0 - k)) * n_dot_l * d / (k + n_dot_l * (1.0 - k));
        share.visibility = visibility;
        return share;
    }
    return share;
}

// What a water program leaves in the G-buffer for its pixel: albedo
// (before the G-buffer's encoding), the colour buffer (what shows through),
// the gloss (6 bits, steps of 4/255) and the world-space normal (the
// deferred pass normalizes what it reads).
struct WaterPixel {
    albedo: vec3<f32>,
    transmitted: vec3<f32>,
    gloss: f32,
    normal: vec3<f32>,
}

// The game's deferred shading of the water pixel (`field_water`, blend
// `src + dst·src.a`, see docs/research/wiiu-deferred-shading.md):
//   albedo·(sun·(1 − F_LH) + E·(1 − f)) + S·F + glint + transmitted,
// the albedo decoded and the diffuse re-encoded with its literals; E is
// the cube map towards the normal at LOD 3. The glint's
// `uking_dynamic_cloud_ratio` is `glint` (follows the weather),
// `uking_dynamic_base_light_change_ratio` is taken as 1. Then the air
// between the camera and the pixel (`air`: the aerial perspective's
// strength, SI-WAT-03) and the look's haze, like every surface. `v` points
// to the eye; `frag` is the fragment's position. `rain`: the wetness
// (`botw::look::water_wetness_at`) and `uking_dynamic_rainfall`; the
// rain's main pass (program 25) takes part of the normal's up component
// away once the water is more than half wet (weather.md §11.5).
fn field_water(
    pixel: WaterPixel,
    world: vec3<f32>,
    v: vec3<f32>,
    frag: vec2<f32>,
    look: Look,
    glint: f32,
    air: f32,
    shadow_map: texture_2d<f32>,
    shadow_sampler: sampler,
    clouds: CloudParams,
    rain: vec2<f32>,
) -> vec3<f32> {
    var n = normalize(pixel.normal);
    let wet_n = saturate(2.0 * rain.x - 1.0);
    n = normalize(n - vec3<f32>(0.0, 1.0, 0.0) * n.y * 1.6 * rain.y * wet_n);
    let view_position = position_world_to_view(world);
    let tan_half_fov = 1.0 / view.clip_from_view[1][1];
    let view_dir = -v;
    // The game leans the normal it uses for the Fresnel and the sun towards
    // the horizontal with distance: its world-up part (context[0..2] column
    // 1) keeps w = 0.5 + 0.5·saturate(25 / (z·tan(fovy/2))) of itself.
    let lean = 0.5 + 0.5 * saturate(25.0 / (-view_position.z * tan_half_fov));
    let leaned = normalize(vec3(n.x, n.y * lean, n.z));
    let sun = sun_share(leaned, v, pixel.gloss, world, view_position.z, frag, shadow_map, shadow_sampler, clouds);
    let facing = saturate(dot(leaned, v));
    // The game's Fresnel dims in shadow: saturate(0.8 + shadow).
    let f = pow(1.0 - facing, 5.0) * saturate(0.8 + sun.visibility);
    let fresnel = f + 0.02 * (1.0 - f);
    let stored = saturate(pixel.albedo * bitcast<f32>(0x3f895ef0u) + bitcast<f32>(0xba0a8ec8u));
    let diffuse = stored * (sun.diffuse * diffuse_gain(look) + cube_at(n, 3.0) * (1.0 - f));
    let encoded = diffuse * bitcast<f32>(0x3f6e896bu) + bitcast<f32>(0x3a011b1eu);

    // SI-WAT-01: the cube map's centre is the camera, without the game's easing.
    // What the water mirrors (S): the cube map towards where the reflected
    // ray leaves a sphere of radius |P| + 200 m round the cube map's centre
    // (environment[31]: the centre in view space), at LOD 3 − 3·gloss. The
    // viewer draws its cube map where the camera stands (`cubemap.rs`), so
    // the centre is the camera; the game's centre eases towards where its
    // cube map was drawn (`0x038c28e4`).
    let to_pixel = world - view.world_position;
    let reach = length(to_pixel) + 200.0;
    let r = reflect(view_dir, n);
    let b = dot(r, to_pixel);
    let t = -b + sqrt(b * b - (dot(to_pixel, to_pixel) - reach * reach));
    let mirrored = cube_at(normalize(to_pixel + r * t), 3.0 - 3.0 * pixel.gloss);
    var color = encoded + mirrored * fresnel + sun.glint * glint + pixel.transmitted;

    // The air between the camera and the water: Bevy's atmosphere lays its
    // aerial perspective over the opaque scene before the transmissive
    // phase, so the water adds a matching one itself, then the look's haze
    // like every surface.
    color = aerial_perspective(color, distance(world, view.world_position), view_dir, air);
    return apply_haze(look, color, world, view_dir);
}
