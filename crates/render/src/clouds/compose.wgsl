// The clouds' reduced buffer laid over the frame like the game's gsys
// `render_buffer_color` variant DRAW_COLOR=5 (VS 132, PS 133, Wii U v208;
// docs/research/wiiu-sky-resources.md, "Reduced cloud buffer"): one
// bilinear sample of the reduced colour whose weights are pushed towards
// the half-size texels lying at the pixel's own depth, so the clouds keep
// to the sky's edge along the land.
//   t = uv·cParam2.xy + 0.5;  D = cDepth(uv);  g = gather(cHalfNLD, uv + 0.0001)
//   e = |g − D|;  k = cParam1.w / (D + cParam0.x)
//   u' = sat(fract(t.x) + k·(e00 + e01 − e10 − e11))
//   v' = sat(fract(t.y) + k·(e00 + e10 − e01 − e11))
//   out = cColor(uv + ((u' − fract(t.x))·cParam2.z, (v' − fract(t.y))·cParam2.w))
// (e_ij: texel i across, j down of the four around uv.) The pipeline
// blends out + frame·out.a (`0x04010401`) with no depth test.

#import bevy_pbr::forward_io::VertexOutput

struct Compose {
    // cParam0: (n/(f−n), 1 − n/f, (f−n)/f, (f−n)/n).
    param0: vec4<f32>,
    // cParam1: (f − n, n, f, coeff/tan(fovy/2)).
    param1: vec4<f32>,
    // cParam2: (W/2, H/2, 2/W, 2/H).
    param2: vec4<f32>,
    // The frame's size W, H (xy).
    frame: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> compose: Compose;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var reduced_color: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var reduced_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var depth_full: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var depth_half: texture_2d<f32>;

fn half_depth(at: vec2<i32>, last: vec2<i32>) -> f32 {
    return textureLoad(depth_half, clamp(at, vec2(0), last), 0).x;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.position.xy / compose.frame.xy;
    let t = uv * compose.param2.xy + 0.5;
    let d = textureLoad(depth_full, vec2<i32>(in.position.xy), 0).x;
    // The gather's four texels: those whose centres surround the point.
    let size = vec2<i32>(textureDimensions(depth_half));
    let last = size - 1;
    let base = vec2<i32>(floor((uv + 0.0001) * vec2<f32>(size) - 0.5));
    let e00 = abs(half_depth(base, last) - d);
    let e10 = abs(half_depth(base + vec2(1, 0), last) - d);
    let e01 = abs(half_depth(base + vec2(0, 1), last) - d);
    let e11 = abs(half_depth(base + vec2(1, 1), last) - d);
    let k = compose.param1.w / (d + compose.param0.x);
    let f = fract(t);
    let across = saturate(f.x + k * (e00 + e01 - e10 - e11));
    let down = saturate(f.y + k * (e00 + e10 - e01 - e11));
    let shift = (vec2(across, down) - f) * compose.param2.zw;
    return textureSample(reduced_color, reduced_sampler, uv + shift);
}
