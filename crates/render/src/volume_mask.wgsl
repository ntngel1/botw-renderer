// The game's volume shadow mask (agl `VolumeMask`, KSys `merge_mask`;
// docs/research/light-shafts.md), drawn by volume_mask.rs:
//
// `layers` — one unit's layer pass (agl `volume_mask_layer`, VS 494 /
// PS 495). The game draws N camera-facing quads back to front at view
// depths d_i = near + span·(i/(N−1))^nonlinear, each depth-tested against
// the unit's reduced depth, and adds (s·(hi − lo) + lo)·1/(N−1) into an
// R16F target per pixel, s the sun's shadow-map compare at the quad's
// point. Here each pixel of the reduced target sums the same layers at
// once: the same depths, the same test and the same weights.
//
// `blur` — agl `image_filter_gaussian` GAUSSIAN_KERNEL=3, horizontal then
// vertical: two bilinear taps ±0.5 texel apart, i.e. 1-2-1 each way; both
// ways in one pass (the same sums, edges clamped).
//
// `merge` — uking_pass_shader PS 553 into the merged target (R8G8 unorm,
// the larger unit's size): r = A + (B − A)·k, g = the indoor mask. The
// target is the look texture's mask rows (look.wgsl `gsys_user2`), so the
// R8 rounding is done here.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import bevy_render::view::View
#import bevy_pbr::mesh_view_types::{Lights, DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT}

// --- layers ---

// One unit's layers (`LayerParams` in volume_mask.rs).
struct Unit {
    // near, span, 1/(N−1), N
    range: vec4<f32>,
    // hi − lo, lo, the camera's near plane, `layer_dist_nonlinear`
    amp: vec4<f32>,
    // the view's size (pixels), `layer_reduce_level`, unused
    frame: vec4<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> lights: Lights;
@group(0) @binding(2) var shadow_maps: texture_depth_2d_array;
@group(0) @binding(3) var shadow_sampler: sampler_comparison;
@group(0) @binding(4) var scene_depth: texture_depth_2d;
@group(0) @binding(5) var<uniform> unit: Unit;

// `0x37000000` (`0x10360d98`): added to each layer's depth.
const LAYER_EPSILON: f32 = 7.6294e-6;

// The frame's texel the unit's reduced depth holds at `texel`: the game
// halves the depth `level` times, each pass copying the texel under its
// pixel's uv (`volume_mask_reducedepth` PS 463, COVOLVE_MAX=0).
fn reduced_depth_texel(texel: vec2<u32>, frame: vec2<u32>, level: u32) -> vec2<u32> {
    var p = texel;
    for (var l = level; l > 0u; l -= 1u) {
        let src = max(frame >> vec2<u32>(l - 1u), vec2<u32>(1u));
        let to = max(frame >> vec2<u32>(l), vec2<u32>(1u));
        p = vec2<u32>((vec2<f32>(p) + 0.5) * vec2<f32>(src) / vec2<f32>(to));
        p = min(p, src - 1u);
    }
    return p;
}

// The sun's shadow at `world`, `d` metres deep in the view: one compare in
// the cascade that holds that depth (the game: one of its three by the
// layer's depth and the split lengths, PCF 1, no bias); 1 lit, 0 shadowed.
// SI-LGT-05: Bevy's cascades and shadow map stand in for the game's.
fn sun_shadow(light_id: u32, world: vec3<f32>, d: f32) -> f32 {
    let light = &lights.directional_lights[light_id];
    var c = 0u;
    loop {
        if c >= (*light).num_cascades || d < (*light).cascades[c].far_bound {
            break;
        }
        c += 1u;
    }
    if c >= (*light).num_cascades {
        return 1.0;
    }
    let clip = (*light).cascades[c].clip_from_world * vec4<f32>(world, 1.0);
    if clip.w <= 0.0 {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    if any(ndc.xy < vec2<f32>(-1.0)) || ndc.z < 0.0 || any(ndc > vec3<f32>(1.0)) {
        return 1.0;
    }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + 0.5;
    return textureSampleCompareLevel(shadow_maps, shadow_sampler, uv, i32((*light).depth_texture_base_index + c), ndc.z);
}

@fragment
fn layers(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let texel = vec2<u32>(in.position.xy);
    let frame = vec2<u32>(unit.frame.xy);
    let level = u32(unit.frame.z);
    let size = max(frame >> vec2<u32>(level), vec2<u32>(1u));

    // The scene's view depth behind the pixel (reversed depth: 0 = nothing
    // drawn, the layers all pass).
    let uv = (vec2<f32>(texel) + 0.5) / vec2<f32>(size);
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let depth = textureLoad(scene_depth, vec2<i32>(reduced_depth_texel(texel, frame, level)), 0);
    var scene = 3.0e38;
    if depth > 0.0 {
        let s = view.view_from_clip * vec4<f32>(ndc, depth, 1.0);
        scene = -s.z / s.w;
    }

    // The pixel's ray, scaled to one metre of view depth, in the world.
    let r = view.view_from_clip * vec4<f32>(ndc, 1.0, 1.0);
    let ray_view = r.xyz / r.w;
    let ray = (view.world_from_view * vec4<f32>(ray_view / -ray_view.z, 0.0)).xyz;

    var light_id = 0u;
    var shadowed = false;
    for (var i = 0u; i < lights.n_directional_lights; i += 1u) {
        if (lights.directional_lights[i].flags & DIRECTIONAL_LIGHT_FLAGS_SHADOWS_ENABLED_BIT) != 0u {
            light_id = i;
            shadowed = true;
            break;
        }
    }

    let near = unit.range.x;
    let span = unit.range.y;
    let step = unit.range.z;
    let count = u32(unit.range.w);
    let camera_near = unit.amp.z;
    let nonlinear = unit.amp.w;
    var sum = 0.0;
    // Back to front, as drawn; the first layer at the camera's near plane
    // or nearer stops the draw.
    for (var i = count - 1u; i > 0u; i -= 1u) {
        let t = f32(i) * step;
        let d = near + span * select(pow(t, nonlinear), t, nonlinear == 1.0);
        if d <= camera_near {
            break;
        }
        let z = d + LAYER_EPSILON;
        // SI-VOL-02: the layers' depth test against the reduced depth is
        // inferred (the render state's depth word is not decoded).
        if z >= scene {
            continue;
        }
        var s = 1.0;
        if shadowed {
            s = sun_shadow(light_id, view.world_position + ray * z, z);
        }
        sum += (s * unit.amp.x + unit.amp.y) * step;
    }
    return vec4<f32>(sum, 0.0, 0.0, 1.0);
}

// --- blur ---

@group(0) @binding(10) var blur_source: texture_2d<f32>;

@fragment
fn blur(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let at = vec2<i32>(in.position.xy);
    let last = vec2<i32>(textureDimensions(blur_source)) - 1;
    let weights = vec3<f32>(0.25, 0.5, 0.25);
    var sum = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let p = clamp(at + vec2<i32>(x, y), vec2<i32>(0), last);
            sum += weights[x + 1] * weights[y + 1] * textureLoad(blur_source, p, 0).r;
        }
    }
    return vec4<f32>(sum, 0.0, 0.0, 1.0);
}

// --- merge ---

// x: k (`cVolumeMask.x`), y: the first row of the target, zw: its size.
struct Merge {
    params: vec4<f32>,
}

@group(0) @binding(20) var mask_a: texture_2d<f32>;
@group(0) @binding(21) var mask_b: texture_2d<f32>;
@group(0) @binding(22) var mask_sampler: sampler;
@group(0) @binding(23) var<uniform> merge_params: Merge;

@fragment
fn merge(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let p = merge_params.params;
    let uv = (in.position.xy - vec2<f32>(0.0, p.y)) / p.zw;
    // SI-VOL-01: the merge's samplers are not read; filtered between the
    // texels.
    let a = textureSampleLevel(mask_a, mask_sampler, uv, 0.0).r;
    let b = textureSampleLevel(mask_b, mask_sampler, uv, 0.0).r;
    let r = a + (b - a) * p.x;
    // The indoor mask: outdoors, none.
    // SI-VOL-03: no inner mask models; the indoor mask is 0 (outside).
    let indoor = 0.0;
    // R8G8 unorm.
    let packed = round(saturate(vec2<f32>(r, indoor)) * 255.0) / 255.0;
    return vec4<f32>(packed, 0.0, 0.0);
}
