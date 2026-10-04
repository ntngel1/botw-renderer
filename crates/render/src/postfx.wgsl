// The game's post-processing (`postfx.rs`): the bloom of
// `agl::pfx::Bloom` (programs `bloom_mask`, `bloom_compose`,
// `bloom_gaussian` of `agl_technique_pfx`) and the tone curve of KSys
// `hdr_compose` (`uking_pass_shader`, PS 547 and 549).
// docs/research/wiiu-postfx-materials.md has the native programs.

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct PostFx {
    // `cLuminanceWeight`: luminance weights over the threshold range, and
    // minus the threshold over it.
    mask_weight: vec4<f32>,
    // `cThresholdParam`: x the clamped luminance over the threshold range,
    // z the intensity; w 1 when the luminance is clamped.
    mask_param: vec4<f32>,
    // The blur levels' colours times their alpha (`color1`–`color4`), and
    // the colour the levels are gathered with (`finalgather`).
    layer1: vec4<f32>,
    layer2: vec4<f32>,
    layer3: vec4<f32>,
    layer4: vec4<f32>,
    final_gather: vec4<f32>,
    // x: 1 for the colour-table variant (PS 549), 0 for `cParam` (PS 547);
    // y, z: `cParam.xy`; w: scale from the viewer's exposed colour to the
    // game's HDR buffer.
    tone: vec4<f32>,
    // The colour table's saturation and brightness (`baglccr`), and 1 when
    // there is a bloom to add.
    correction: vec4<f32>,
};

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var<uniform> postfx: PostFx;

// Native f32 luminance weights of `hdr_compose` (bits 0x3e99096c,
// 0x3f162b6b, 0x3dea4a8c); their sum is not exactly 1.
const TONE_WEIGHTS: vec3<f32> = vec3<f32>(0.29890001, 0.58660001, 0.11440000);
// log2(e), bits 0x3fb8aa3b.
const LOG2_E: f32 = 1.44269502;

fn scene(uv: vec2<f32>) -> vec3<f32> {
    return textureSample(input_texture, input_sampler, uv).rgb * postfx.tone.w;
}

// `bloom_mask`, variant with `BLM_LUMINANCE_CLAMP` (PS 141): four bilinear
// taps one source texel off the centre (a 4×4 box at a quarter size), then
//   out = c · sat(L/R − T/R) · sat(C/L) · I.
@fragment
fn mask(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(input_texture));
    let c = 0.25 * (scene(in.uv + texel) + scene(in.uv + vec2(texel.x, -texel.y))
        + scene(in.uv + vec2(-texel.x, texel.y)) + scene(in.uv - texel));
    // The HDR colour's alpha is 1 (R11G11B10 in the game).
    let over = dot(c, postfx.mask_weight.xyz);
    let m = saturate(over + postfx.mask_weight.w);
    var s = 1.0;
    if postfx.mask_param.w > 0.5 {
        s = saturate(postfx.mask_param.x / max(over, 1e-20));
    }
    return vec4(c * (m * s * postfx.mask_param.z), 1.0);
}

// `bloom_compose` STEP 3 (PS 657), which the blur also uses to halve a
// level: one bilinear tap.
@fragment
fn copy(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4(textureSample(input_texture, input_sampler, in.uv).rgb, 1.0);
}

// `bloom_gaussian` (PS 651/653; offsets from VS 650/652): five bilinear
// taps, nine texels.
fn gaussian(uv: vec2<f32>, step: vec2<f32>) -> vec4<f32> {
    var c = textureSample(input_texture, input_sampler, uv).rgb * 0.227027029;
    c += textureSample(input_texture, input_sampler, uv + step * 1.38460004).rgb * 0.316216230;
    c += textureSample(input_texture, input_sampler, uv - step * 1.38460004).rgb * 0.316216230;
    c += textureSample(input_texture, input_sampler, uv + step * 3.23077011).rgb * 0.0702702701;
    c += textureSample(input_texture, input_sampler, uv - step * 3.23077011).rgb * 0.0702702701;
    return vec4(c, 1.0);
}

@fragment
fn blur_x(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(input_texture));
    return gaussian(in.uv, vec2(1.0 / size.x, 0.0));
}

@fragment
fn blur_y(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let size = vec2<f32>(textureDimensions(input_texture));
    return gaussian(in.uv, vec2(0.0, 1.0 / size.y));
}

// `bloom_compose` STEP 2 (PS 655): the smaller level times
// `cComposeColor`; the blend adds the larger level times the blend
// constant.
@fragment
fn compose4(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4(textureSample(input_texture, input_sampler, in.uv).rgb * postfx.layer4.rgb, 1.0);
}

@fragment
fn compose3(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4(textureSample(input_texture, input_sampler, in.uv).rgb * postfx.layer3.rgb, 1.0);
}

@fragment
fn compose_final(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    return vec4(textureSample(input_texture, input_sampler, in.uv).rgb * postfx.final_gather.rgb, 1.0);
}

@group(0) @binding(3) var bloom_texture: texture_2d<f32>;

// KSys `hdr_compose`: bloom plus colour, the exponential curve blended
// between luminance and per channel, then the saturation of `cParam`
// (PS 547) or the colour table (PS 549).
@fragment
fn hdr_compose(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    var c = scene(in.uv);
    if postfx.correction.z > 0.5 {
        c += textureSample(bloom_texture, input_sampler, in.uv).rgb;
    }
    c = max(c, vec3(0.0));
    let l = dot(c, TONE_WEIGHTS);
    if l <= 0.0 {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    let a = 1.0 - exp2(-LOG2_E * l);
    let b = c * (a / l);
    let p = 1.0 - exp2(-LOG2_E * c);
    let q = saturate(b + (p - b) * (a * a));
    let m = max(q.r, max(q.g, q.b));
    // SI-PFX-01: tone and bloom inputs not traced past the constructor.
    if postfx.tone.x > 0.5 {
        // The colour table as `color_correction_map` builds it for the
        // field (hue 0, gamma 1, identity curves, neutral toy camera):
        // saturation from the brightest channel, then brightness. The
        // table's 8³ samples and their filtering are not repeated.
        let s = postfx.correction.x;
        return vec4(saturate((m + (q - m) * s) * postfx.correction.y), 1.0);
    }
    let d = 0.666666687 * (q.r + q.g + q.b) - 1.0;
    let s = postfx.tone.y + (1.0 - d * d) * postfx.tone.z;
    return vec4(m + (q - m) * s, 1.0);
}
