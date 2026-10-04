// The sky dome, drawn around the camera over the atmosphere's sky, under
// the stars and the clouds. Premultiplied alpha. See `fog.rs`.
//
// With the game's sky table (in the look texture): the game's sky, like
// its `sky_postfx_sky` (variants 8 and 12, PS 435 and 443 of
// `uking_pass_shader`, Wii U v208; docs/research/wiiu-sky-resources.md):
//   u = 1 − (2/π)·acos(0.5 + 0.5·dot(towards the light, d)),  v = 0.5 + 0.5·d.y
//   s = table(u, v);  sky = ground + (s·amp − ground)·saturate(s.a + ground.a)
// (amp, `cAmplifierAdhoc`, 1 in the main view), then its ad hoc fog
//   f = √saturate(4w)·mix(w, minscale, saturate(d.y)^atten_sky)
//   sky = mix(sky, fog colour, f)
// with w its strength at the horizon (the air's moisture in the field).
// The same table colours the distance (`apply_haze`), so the sky and the
// far land meet without a seam. The game's sun sprite is not reproduced:
// the stars' dome draws a disk (`sky.wgsl`), this one the fitted glow round
// it.
//
// Without the table: the atmosphere's sky fades into the distance fog's
// colour towards the horizon (glowing towards the sun), so that the far
// terrain, hazed by the same colour, meets the sky without a seam. Around
// the sun the sky glows, wide and faint, bright near the disk (the game's
// sun sprite is mostly glow; its haze, `mie_amplifier`, makes it larger at
// sunset). All fits.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}
#import botw::look::{read_look, sky_table}

struct SkyHaze {
    // Fog colour after exposure (rgb), its share at the horizon (a).
    color: vec4<f32>,
    // Glow towards the light at its centre (rgb), how tightly it gathers (w).
    glow: vec4<f32>,
    // Towards the light (xyz), how fast the haze thins upwards (w, per unit
    // of the sine of the elevation).
    light: vec4<f32>,
    // Towards the sun (xyz; w is unused).
    sun: vec4<f32>,
    // The sun's glow at its centre (rgb; w is unused).
    halo: vec4<f32>,
    // The low sun's band along the horizon, below the sun (rgb; w is unused).
    band: vec4<f32>,
    // The game's `cGroundColor` (game units): the ground's colour below the
    // horizon (rgb), how much of it shows above (a).
    ground: vec4<f32>,
    // The game's `cNormalFogColor` (rgb, game units; w is unused).
    fog_color: vec4<f32>,
    // The game's `cNormalFogCoeff`: (unused, how fast the ad hoc fog thins
    // upwards, its share at the zenith, its strength at the horizon).
    fog: vec4<f32>,
}

// The sun's glow: a wide faint lobe and a bright one close to the disk
// (shares of `halo` and how tightly each gathers; fit to the game's glare).
// SI-SKY-03: sun halo and disk drawn over the game's sky are ours.
const HALO_WIDE: vec2<f32> = vec2(0.2, 6.0);
const HALO_CORE: vec2<f32> = vec2(1.0, 120.0);
// How fast the low sun's band along the horizon thins upwards, per unit of
// the sine of the elevation (half gone some 5 degrees up: the game's band
// glows over a few degrees, and the hills round the plain hide the lowest
// ones; fit to R/719).
const BAND_THINNING: f32 = 8.0;

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> haze: SkyHaze;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var look_texture: texture_2d<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position);
    let towards_sun = max(dot(dir, normalize(haze.sun.xyz)), 0.0);
    // SI-SKY-03: sun halo and disk drawn over the game's sky are ours.
    let halo = haze.halo.rgb * (HALO_WIDE.x * pow(towards_sun, HALO_WIDE.y) + HALO_CORE.x * pow(towards_sun, HALO_CORE.y));
    let look = read_look(look_texture);
    let brightness = look.spare[3].x;
    if brightness > 0.0 {
        return vec4(game_sky(dir) * brightness + halo, 1.0);
    }
    let alpha = haze.color.a * exp(-max(dir.y, 0.0) * haze.light.w);
    let glow = haze.glow.rgb * pow(max(dot(dir, normalize(haze.light.xyz)), 0.0), haze.glow.w);
    // Under a low sun a band of its glow lies along the horizon, brightest
    // below the sun and thinning fast upwards.
    let across = normalize(vec3(dir.x, 0.0, dir.z) + vec3(0.0, 1e-4, 0.0));
    let sun_across = normalize(vec3(haze.light.x, 0.0, haze.light.z) + vec3(0.0, 1e-4, 0.0));
    let side = 0.5 + 0.5 * dot(across, sun_across);
    let band = haze.band.rgb * side * side * side * exp(-max(dir.y, 0.0) * BAND_THINNING);
    return vec4((haze.color.rgb + glow + band) * alpha + halo, alpha);
}

// The game's sky along the view direction `d` (normalized), in its units.
fn game_sky(d: vec3<f32>) -> vec3<f32> {
    // The game approximates acos by a polynomial (error below 1e-4).
    let c = dot(normalize(haze.light.xyz), d);
    let u = 1.0 - acos(clamp(0.5 + 0.5 * c, 0.0, 1.0)) * (2.0 / 3.14159265);
    let s = sky_table(look_texture, vec2(u, 0.5 + 0.5 * d.y));
    var sky = haze.ground.rgb + (s.rgb - haze.ground.rgb) * saturate(s.a + haze.ground.a);
    let w = haze.fog.w;
    if w > 0.0 {
        let up = pow(max(saturate(d.y), 1e-6), haze.fog.y);
        let f = sqrt(saturate(4.0 * w)) * mix(w, haze.fog.z, up);
        sky = mix(sky, haze.fog_color.rgb, f);
    }
    return sky;
}

