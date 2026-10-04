// The normalized linear depths the game composes its reduced buffer with
// (gsys `render_buffer_depth`, Wii U v208; docs/research/wiiu-sky-
// resources.md, "Reduced cloud buffer"):
//   NLD = cParam0.x / (1 − d·cParam0.y) − cParam0.x = (z − n)/(f − n)
// of the depth d, the view depth z between the planes n and f. `full` is
// variant 16 (PS 93) into R16F over the whole frame, `half` variant 24
// (PS 109) into R32F at half size, from one texel of the frame's depth,
// the one under the pixel's uv (the sampler's filters are POINT,
// `FUN_03a080b0`). Bevy's depth is reversed and infinite, d = n/z, 0 where
// nothing was drawn: the game's far plane, NLD 1.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

// The view's planes n, f (x, y).
struct Planes {
    near_far: vec4<f32>,
}

@group(0) @binding(0) var<uniform> planes: Planes;
#ifdef MULTISAMPLED
@group(0) @binding(1) var scene_depth: texture_depth_multisampled_2d;
#else
@group(0) @binding(1) var scene_depth: texture_depth_2d;
#endif

fn normalized_linear_depth(d: f32) -> f32 {
    let n = planes.near_far.x;
    let f = planes.near_far.y;
    if d <= 0.0 {
        return 1.0;
    }
    return min((n / d - n) / (f - n), 1.0);
}

@fragment
fn full(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let d = textureLoad(scene_depth, vec2<i32>(in.position.xy), 0);
    return vec4(normalized_linear_depth(d));
}

@fragment
fn half(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let frame = vec2<f32>(textureDimensions(scene_depth));
    let texel = min(vec2<i32>(in.uv * frame), vec2<i32>(frame) - 1);
    return vec4(normalized_linear_depth(textureLoad(scene_depth, texel, 0)));
}
