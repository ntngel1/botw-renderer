// The sky's two cloud layers, shared by the sky (`clouds.wgsl`) and the
// water that mirrors them, and the game's cloud shadow on the surfaces.

#define_import_path botw::clouds

// The clouds' reduced buffer (`clouds.wgsl`, REDUCED_BUFFER) has its own
// view bindings and casts no shadow.
#ifndef REDUCED_BUFFER
#import bevy_pbr::view_transformations::position_world_to_view
#import bevy_pbr::mesh_view_bindings as view_bindings
#endif

// One cloud layer (see `CloudLayerParams` in clouds.rs).
struct CloudLayer {
    // Height (m), density (share of the sky covered), opacity scale, the
    // coverage at which the clouds turn opaque.
    shape: vec4<f32>,
    // Pattern repeats per metre, blend from puffs to overcast (0-1), warp
    // strength, unused.
    pattern: vec4<f32>,
    // Drift in repeats per second (xy), how far towards the light the relief
    // shading looks (in repeats, z), how far the pattern is drawn out along
    // the drift (w).
    drift: vec4<f32>,
    // How far each value of `shape` sways either way under the clear sky's
    // look and its phase (`CloudSway` in clouds.rs).
    shape_swing: vec4<f32>,
    shape_phase: vec4<f32>,
    // The same for the pattern scale (x) and the warp strength (z) of
    // `pattern`.
    pattern_swing: vec4<f32>,
    pattern_phase: vec4<f32>,
    // The cloudy sky's look: its swings and phases.
    shape_swing_cloudy: vec4<f32>,
    shape_phase_cloudy: vec4<f32>,
    pattern_swing_cloudy: vec4<f32>,
    pattern_phase_cloudy: vec4<f32>,
}

// The layer as it sways like the game's `Min`/`Max`/`SinSeedAdd`: the clear
// and the cloudy look each by the sine of its phase, blended by the
// cloudiness; unclamped, like `SkyMgr` writes them.
fn swayed(layer: CloudLayer) -> CloudLayer {
    var out = layer;
    out.shape = layer.shape + layer.shape_swing * sin(layer.shape_phase)
        + layer.shape_swing_cloudy * sin(layer.shape_phase_cloudy);
    let pattern = layer.pattern + layer.pattern_swing * sin(layer.pattern_phase)
        + layer.pattern_swing_cloudy * sin(layer.pattern_phase_cloudy);
    out.pattern = vec4(pattern.x, layer.pattern.y, pattern.z, layer.pattern.w);
    return out;
}

struct CloudParams {
    // Towards the sun (xyz).
    sun: vec4<f32>,
    // Sunlit colour, brightness.
    lit: vec4<f32>,
    // Shaded colour.
    shade: vec4<f32>,
    // Colour seen looking into the light (rgb), its strength (w).
    backlight: vec4<f32>,
    // Colour of the sky the layers fade into at the horizon (rgb), how
    // brightly it glows towards the light (w, in the backlight's colour).
    haze: vec4<f32>,
    // The upper and the lower layer.
    upper: CloudLayer,
    lower: CloudLayer,
    // The cloud shadow's texture coordinates of a world point (x, y) and
    // their divisor (the game's `gsys_context[35..37]`, see clouds.rs).
    shadow_u: vec4<f32>,
    shadow_v: vec4<f32>,
    shadow_w: vec4<f32>,
    // Its strength, `1 − proj_shadow_off` (x); 1 where surfaces take it (y);
    // the texture's velocity a second of `globals.time` (zw).
    shadow: vec4<f32>,
}

// Texture coordinate of world position `xz` in a layer's drifting pattern.
fn cloud_uv(layer: CloudLayer, xz: vec2<f32>, time: f32) -> vec2<f32> {
    return xz * layer.pattern.x - layer.drift.xy * time;
}

// The layer's pattern (0-1, roughly evenly spread) at texture coordinate
// `uv`: broad cover decides where clouds are, puffs and billows shape them,
// fine noise eats into the edges, all drawn out downwind into wisps; a
// smooth field for overcast. `lod` is the mip level for the puffs' scale
// (the other scales follow).
// SI-SKY-08: the no-dump cloud pattern is our own.
fn cloud_pattern(noise: texture_2d<f32>, noise_sampler: sampler, layer: CloudLayer, uv_in: vec2<f32>, lod: f32) -> f32 {
    let along = normalize(layer.drift.xy + vec2(1e-7, 0.0));
    let uv = vec2(dot(uv_in, along) / max(layer.drift.w, 1.0), dot(uv_in, vec2(-along.y, along.x)));
    let warp = textureSampleLevel(noise, noise_sampler, uv * 0.61 + vec2(0.37, 0.11), max(lod - 0.7, 0.0)).ra - 0.5;
    let p = uv + warp * layer.pattern.z * 0.06;
    let cover = textureSampleLevel(noise, noise_sampler, p * 0.23, max(lod - 2.12, 0.0)).r;
    let puffs = textureSampleLevel(noise, noise_sampler, p, max(lod, 0.0)).g;
    let billows = textureSampleLevel(noise, noise_sampler, p * 1.7 + vec2(0.31, 0.57), max(lod + 0.77, 0.0)).b;
    let fine = textureSampleLevel(noise, noise_sampler, p * 4.3 + vec2(0.13, 0.71), max(lod + 2.1, 0.0)).a;
    // A wide ramp: soft, thinning edges rather than hard puffs.
    let cumulus = smoothstep(0.47, 1.08, cover * 0.9 + (puffs * 0.65 + billows * 0.35) * 0.75 - fine * 0.2);
    let overcast = saturate(0.55 + cover * 0.5 + (fine - 0.5) * 0.2);
    return mix(cumulus, overcast, layer.pattern.y);
}

// How opaque the layer is (0-1) where its pattern is `n`: the densest
// `density` share of the pattern is cloud, thickening towards the threshold
// (the opacity scale makes it thicker still).
fn cloud_alpha(layer: CloudLayer, n: f32) -> f32 {
    let density = max(layer.shape.y, 0.01);
    let coverage = saturate((n - (1.0 - density)) / density);
    return 1.0 - exp(-2.2 * coverage / max(layer.shape.w, 0.05) * layer.shape.z);
}

// How much of the layers shows along a view ray rising at `dir_y` (sine of
// its elevation): right down to the horizon, where they thin out.
fn horizon_fade(dir_y: f32) -> f32 {
    return smoothstep(0.004, 0.05, dir_y);
}

// How much of the main light the clouds take away at world point `p` (0
// none, up to the shadow's strength), like the game's pre-shading (PS 112
// `bec68ec6f40a864f`, docs/research/wiiu-render-cpu.md): the shadow texture
// (`gsys_projection0`, `p_shadow_clouds`) projected from 8 km up, read at
// mip 0.005 per metre of view depth, gives `proj0`; the light is scaled by
// `sat(proj0 + proj_shadow_off)`, `proj_shadow_off = 1 − strength`.
#ifndef REDUCED_BUFFER
fn cloud_shadow(map: texture_2d<f32>, map_sampler: sampler, params: CloudParams, p: vec3<f32>) -> f32 {
    let w = vec4(p, 1.0);
    let q = vec3(dot(params.shadow_u, w), dot(params.shadow_v, w), dot(params.shadow_w, w));
    let depth = max(-position_world_to_view(p).z, 0.0);
    // The texture sails with the wind (`ShadowDrift` in clouds.rs).
    let uv = q.xy / q.z + params.shadow.zw * view_bindings::globals.time;
    let proj0 = textureSampleLevel(map, map_sampler, uv, 0.005 * depth).r;
    return 1.0 - saturate(proj0 + 1.0 - params.shadow.x * params.shadow.y);
}
#endif
