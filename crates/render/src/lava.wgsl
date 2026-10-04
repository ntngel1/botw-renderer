// SI-LGT-25: lava shading on Bevy PBR with fitted numbers.
// Lava: a dark crust over glowing rock, from the lava layer of the game's
// water textures. `WaterEmm` says where it glows (bright veins, dark
// crust); the veins glow from the table's deep colour (texel 1, red) to its
// foam colour (texel 0, yellow) where they are brightest, as emitted light
// (HDR, so bloom picks it up); the crust is lit like rock, bent by
// `WaterNrm`. Both creep along the flow of `.water.extm`.
//
// UV_1 carries the flow (m/s), like the water's.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    mesh_view_bindings::{globals, view},
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
}
#import botw::look::{read_look_at, diffuse_gain, apply_haze}
#import botw::deferred_light::cube_at

// The water kind whose layers are the lava's.
const LAVA: i32 = 3;
// Seconds before a flow-map copy restarts (see the water shader).
const FLOW_CYCLE: f32 = 8.0;

struct LavaParams {
    // Hot colour (the table's texel 0) and glow colour (texel 1), linear.
    hot: vec4<f32>,
    glow: vec4<f32>,
    // x: emitted radiance of the hottest veins (nits, before exposure),
    // y: pattern repeats per metre, z: creep speed, w: normal strength.
    settings: vec4<f32>,
    // Crust albedo (rgb), how bright the glow pattern must be to break the
    // crust (w).
    crust: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var normal_array: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var lava_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var glow_array: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<uniform> params: LavaParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var look_texture: texture_2d<f32>;

struct Sample {
    glow: f32,
    slope: vec2<f32>,
}

// Glow and slope at world position `p`: a broad layer and a finer one
// drifting against it.
fn lava_at(p: vec2<f32>, t: f32) -> Sample {
    let scale = params.settings.y;
    let drift = vec2(0.004, 0.003) * t * params.settings.z;
    let a = p * scale + drift;
    let b = p * scale * 2.7 - drift * 1.7 + vec2(0.37, 0.61);
    var s: Sample;
    s.glow = textureSample(glow_array, lava_sampler, a, LAVA).r * 0.65 + textureSample(glow_array, lava_sampler, b, LAVA).r * 0.35;
    s.slope = (textureSample(normal_array, lava_sampler, a, LAVA).rg * 2.0 - 1.0)
        + (textureSample(normal_array, lava_sampler, b, LAVA).rg * 2.0 - 1.0) * 0.5;
    return s;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let world = in.world_position.xyz;
    let t = globals.time;
#ifdef VERTEX_UVS_B
    let flow = in.uv_b;
#else
    let flow = vec2(0.0);
#endif
    let cycle = FLOW_CYCLE * fract(t / FLOW_CYCLE + vec2(0.0, 0.5));
    let blend = abs(cycle.x / FLOW_CYCLE * 2.0 - 1.0);
    let s0 = lava_at(world.xz - flow * cycle.x, t);
    let s1 = lava_at(world.xz - flow * cycle.y, t);
    let glow = mix(s0.glow, s1.glow, blend);
    let slope = mix(s0.slope, s1.slope, blend) * params.settings.w;

    // The crust, lit like rock; the veins between its plates are flat.
    let heat = smoothstep(params.crust.w - 0.3, params.crust.w + 0.1, glow);
    let n = normalize(vec3<f32>(slope.x * (1.0 - heat), 1.0, slope.y * (1.0 - heat)));
    let crust = params.crust.rgb;
    pbr_input.N = n;
    pbr_input.world_normal = n;
    pbr_input.material.base_color = vec4<f32>(crust * (1.0 - heat), 1.0);
    pbr_input.material.perceptual_roughness = 0.85;
    pbr_input.material.reflectance = vec3(0.1);
    let look = read_look_at(look_texture, in.world_position.xyz);
    // The fill: the cube map towards the normal at LOD 3, as the game's
    // deferred `field_water` lights its water (docs/research/
    // wiiu-deferred-shading.md). Lava is water kind 3 of the same
    // `TeraWater` material; that it takes the same passes is inferred, not
    // traced (LAVA-001 in archived GAPS.md notes). The rest here is the viewer's.
    var color = apply_pbr_lighting(pbr_input).rgb * diffuse_gain(look) + crust * (1.0 - heat) * cube_at(n, 3.0);

    // The glow: red where the veins are dim, yellow where hottest.
    let colour = mix(params.glow.rgb, params.hot.rgb, smoothstep(params.crust.w - 0.1, params.crust.w + 0.4, glow));
    color += colour * heat * params.settings.x * view.exposure;

    color = apply_haze(look, color, world, normalize(world - view.world_position));
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(color, 1.0));
    return out;
}
