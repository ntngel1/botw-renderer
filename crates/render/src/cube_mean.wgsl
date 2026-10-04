// The mean brightness of the environment for `cube_mean()` in
// deferred_light.wgsl, drawn once a frame into one texel
// (deferred_light.rs, `CubeMean`): `LightAnalyzer` folds its cube map into
// one texel from 14 directions, here the six axes and eight corners. The
// same samples as `cube()` there, without the light's strength and the
// exposure, which `cube_mean()` applies to the mean.

// The view's environment map (its specular levels) and its sampler.
@group(0) @binding(0) var environment: texture_cube<f32>;
@group(0) @binding(1) var environment_sampler: sampler;
// The view's turn of the map (`light_probes.view_rotation`).
@group(0) @binding(2) var<uniform> rotation: vec4<f32>;

// As in deferred_light.wgsl.
// SI-LGT-08: cube-map light mean, LOD and offsets are ours.
const CUBE_LOD_FROM_TOP: f32 = 4.0;

// The game's luminance (its shaders' weights).
fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2989, 0.5866, 0.1144));
}

// `cube_at(dir, 3.0)` of deferred_light.wgsl before its scale.
fn cube(dir: vec3<f32>) -> vec3<f32> {
    let q = rotation;
    let t = 2.0 * cross(q.xyz, dir);
    var d = dir + q.w * t + cross(q.xyz, t);
    d.z = -d.z;
    let level = max(f32(textureNumLevels(environment)) - CUBE_LOD_FROM_TOP - 3.0 + 3.0, 0.0);
    return textureSampleLevel(environment, environment_sampler, d, level).rgb;
}

@fragment
fn mean() -> @location(0) vec4<f32> {
    var sum = vec3<f32>(0.0);
    for (var i = 0; i < 3; i += 1) {
        let axis = vec3<f32>(select(0.0, 1.0, i == 0), select(0.0, 1.0, i == 1), select(0.0, 1.0, i == 2));
        sum += cube(axis) + cube(-axis);
    }
    for (var i = 0; i < 8; i += 1) {
        let corner = vec3<f32>(select(1.0, -1.0, (i & 1) != 0), select(1.0, -1.0, (i & 2) != 0), select(1.0, -1.0, (i & 4) != 0));
        sum += cube(corner * 0.57735);
    }
    return vec4<f32>(luma(sum / 14.0), 0.0, 0.0, 1.0);
}
