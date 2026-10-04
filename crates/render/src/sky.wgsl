// Night sky and the sun, added over the sky: stars scattered on the
// sphere (one candidate per cell of a 3D grid, kept a constant pixel size),
// the moon with its phase and darker seas, plus a soft halo, and the sun's
// disk (where the game's sky has its sun, apart from where its light
// shades from; see `daynight.rs`). The clouds hide them by their own alpha,
// like the game's (`clouds.wgsl` blends them over this).

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{globals, view},
}

struct SkyParams {
    // World direction to star-map direction.
    stars: mat4x4<f32>,
    // Towards the moon (xyz), angular radius (w).
    moon: vec4<f32>,
    // Moon colour (rgb), phase 0-1 with 0 full (w).
    moon_color: vec4<f32>,
    // Night-sky visibility (x), how clear the sky is (y: 0 under overcast).
    night: vec4<f32>,
    // Towards the sun (xyz), its disk's angular radius (w).
    sun: vec4<f32>,
    // The disk's radiance (rgb).
    sun_color: vec4<f32>,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: SkyParams;

fn hash3(p: vec3<f32>) -> vec4<f32> {
    var q = fract(p * vec3(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    let a = fract((q.xxy + q.yxx) * q.zyx);
    let b = fract((a.x + a.y) * a.z + dot(a, vec3(17.3, 5.1, 9.7)));
    return vec4(a, b);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash3(vec3(i, 1.0)).w;
    let b = hash3(vec3(i + vec2(1.0, 0.0), 1.0)).w;
    let c = hash3(vec3(i + vec2(0.0, 1.0), 1.0)).w;
    let d = hash3(vec3(i + vec2(1.0, 1.0), 1.0)).w;
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Stars on the unit sphere direction `s`, scaled to `cells` grid cells.
// SI-SKY-05: procedural stars and moon are ours.
fn stars(s: vec3<f32>, cells: f32, threshold: f32) -> vec3<f32> {
    let p = s * cells;
    let cell = floor(p);
    let h = hash3(cell);
    let centre = cell + 0.25 + 0.5 * h.xyz;
    let d = length(p - centre);
    // Constant size on screen: the grid's footprint in one pixel.
    let pixel = max(length(fwidth(p)), 1e-4);
    let core = exp(-pow(d / (pixel * 0.8), 2.0));
    let brightness = smoothstep(threshold, 1.0, h.w);
    let twinkle = 0.75 + 0.25 * sin(globals.time * (1.5 + 3.0 * h.x) + h.y * 40.0);
    let tint = mix(vec3(0.75, 0.85, 1.0), vec3(1.0, 0.9, 0.75), h.z);
    return tint * core * brightness * brightness * twinkle;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dir = normalize(in.world_position.xyz - view.world_position);
    let night = params.night.x;
    // Overcast hides the stars and the moon.
    let clear = params.night.y;
    // Thick air near the horizon hides the faint ones.
    let above = smoothstep(-0.02, 0.3, dir.y);

    let moon_dir = normalize(params.moon.xyz);
    let radius = params.moon.w;
    let cos_angle = dot(dir, moon_dir);
    let angle = acos(clamp(cos_angle, -1.0, 1.0));

    var color = vec3(0.0);
    let s = normalize((params.stars * vec4(dir, 0.0)).xyz);
    // A sparse, faint field like the game's: few bright stars, a thin veil
    // of faint ones.
    color += (stars(s, 150.0, 0.95) * 0.6 + stars(s, 400.0, 0.985) * 0.3) * above * night * clear;

    // The moon: a sphere lit from the side its phase says.
    if angle < radius * 1.02 {
        let right = normalize(cross(moon_dir, vec3(0.0, 1.0, 0.0)));
        let up = cross(right, moon_dir);
        let local = vec2(dot(dir - moon_dir, right), dot(dir - moon_dir, up)) / radius;
        let z = sqrt(max(1.0 - dot(local, local), 0.0));
        let phase = params.moon_color.w * 6.2831853;
        let light = vec3(sin(phase), 0.0, cos(phase));
        let lit = smoothstep(-0.08, 0.12, dot(vec3(local, z), light));
        let seas = value_noise(local * 2.3 + 4.0) * 0.6 + value_noise(local * 6.0) * 0.4;
        let albedo = mix(0.62, 1.0, smoothstep(0.35, 0.65, seas));
        let rim = smoothstep(1.0, 0.96, length(local));
        let face = params.moon_color.rgb * albedo * (lit * 2.2 + 0.03);
        // The moon hides the stars behind it.
        color = mix(color, face * clear, rim);
    }
    // A soft halo, stronger the fuller the moon.
    let fullness = 0.5 + 0.5 * cos(params.moon_color.w * 6.2831853);
    let halo = exp(-max(angle - radius, 0.0) / (radius * 1.5)) * step(radius, angle);
    color += params.moon_color.rgb * halo * 0.25 * fullness * smoothstep(-0.05, 0.1, moon_dir.y) * clear;

    // SI-SKY-03: sun halo and disk drawn over the game's sky are ours.
    // The sun's disk, with a pixel-soft edge, above the horizon.
    let sun_angle = acos(clamp(dot(dir, normalize(params.sun.xyz)), -1.0, 1.0));
    let edge = max(0.5 * fwidth(sun_angle), 1e-6);
    let disk = 1.0 - smoothstep(params.sun.w - edge, params.sun.w + edge, sun_angle);
    color += params.sun_color.rgb * disk * f32(dir.y > 0.0);

    // Premultiplied with zero alpha: added to what is behind.
    return vec4(color, 0.0);
}
