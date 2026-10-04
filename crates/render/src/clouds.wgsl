// The sky's clouds: every pixel of a dome around the camera follows its
// view ray up to each of the two layers and looks up the layer's pattern
// there (`botw::clouds`). The clouds are lit by the sun or the moon in the
// time of day's colours: bright where lit, the shadow colour where more
// cloud lies towards the light, a lining looking into the light. The lower
// layer covers the upper one; towards the horizon both thin out into the
// sky's haze. An overcast layer is a deck in the haze's colour, like the
// game's grey-green rainy skies.
// With the game's sky table the layers are drawn like the game's `cloud`
// shader instead (`agl_technique`, variant 9: PS 449, VS 448, Wii U v208;
// docs/research/wiiu-sky-resources.md): each layer on the game's dome
// mesh, its texture laid over the mesh's plan and drawn out towards the
// rim; the palette's colours, shaded where more cloud lies towards the
// sun, highlit and glowing into it, blended with the sky's colour behind
// the cloud, taken from the table at the cloud's distance — all sky at the
// horizon, most of the cloud's own colour higher up — and last the ad hoc
// fog's colour (`cNormalFogColor`), by its strength w (the air's moisture)
// at the horizon and none straight up:
//   cloud = mix(cloud, fog colour, w·(1 − saturate(n.y)^atten_sky)).
// The layers are drawn with the game's textures (`cloudtexture02`–`04` of
// `collect.genvres`), warped by its noise, once they are read; until then
// this renderer's pattern stands in for them. Premultiplied alpha.

#ifdef REDUCED_BUFFER
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput
#import botw::cloud_view::{globals, scene_depth, view}
#else
#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{globals, view},
}
#endif
#import botw::clouds::{CloudLayer, CloudParams, cloud_alpha, cloud_pattern, cloud_uv, horizon_fade, swayed}
#import botw::look::{Look, read_look, sky_table}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var noise_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var noise_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> params: CloudParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var look_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<uniform> game: CloudLight;
// Each layer's `cBaseTexture`, `cBaseTexture_Blend`, `cNoiseTexture`,
// `cNoiseTexture_Blend` (PS 449's samplers 0–3), when `game.scale.z` is 1.
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var upper_base: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var cloud_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var upper_base_blend: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var upper_noise: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var upper_noise_blend: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var lower_base: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var lower_base_blend: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(12) var lower_noise: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(13) var lower_noise_blend: texture_2d<f32>;
// The noises' sampler: the game's noise samplers mirror where the bases'
// (`cloud_sampler`) repeat (`FUN_03a59734`).
@group(#{MATERIAL_BIND_GROUP}) @binding(14) var noise_mirror_sampler: sampler;

// The game's light for one layer (`CloudLayerLight` in clouds.rs).
struct CloudLayerLight {
    // `sysColor0Vary`, `sysColor1Vary`, `cShadowCol`.
    base: vec4<f32>,
    hilight: vec4<f32>,
    shadow: vec4<f32>,
    // `cBackLightCol`, `mBacklightPower`.
    backlight: vec4<f32>,
    // `mSkyScale` (m), `mScatterHeight`, 1 − `mScatterAmb`, metres per
    // repeat of this renderer's pattern at `mBaseTexScale` 1 (the layer's
    // swaying pattern scale times this is `mBaseTexScale`).
    dome: vec4<f32>,
    // `mShadowPower`, `mHilightPower`, `mHighlightRange`, `mHighlightAmbient`.
    relief: vec4<f32>,
    // `mBacklightRange`, `mBacklightParam0`, `mBacklightParam1`.
    back: vec4<f32>,
    // `mEmbossWidth`, `mEmbossDensity`, `mFarUVMul`, `mFarUVPow`.
    far_uv: vec4<f32>,
    // `mFarDensityChgStart`, `…End`, `…Power`.
    far_density: vec4<f32>,
    // `mFarAlphaChgStart`, `…End`, `…Power`.
    far_alpha: vec4<f32>,
    // The texture's offset in the dome's units (xy; the uniforms
    // `mBaseTexScrollSpdX/Y`, the sum of the speeds so far).
    scroll: vec4<f32>,
    // `mNoiseScale1`, `mNoiseScale2`, `mNoiseDensity1`, `mNoiseDensity2`.
    noise: vec4<f32>,
    // The noises' offsets (`mNoiseSpeed1X`, `1Y`, `2X`, `2Y`, sums like
    // `scroll`).
    noise_offset: vec4<f32>,
    // `mFarDistotionChgStart`, `…End`, `…Power`.
    far_distortion: vec4<f32>,
    // `mDarkSideNoiseParam`, `mLightSideNoiseParam`.
    side_noise: vec4<f32>,
    // `mCloudTexBlendRate` (x).
    texture_blend: vec4<f32>,
    // The channel read as x and as w from the base, base blend, noise and
    // noise blend texture (0–3 a channel, 4 zero, 5 one: GX2's component
    // selection).
    channel_x: vec4<f32>,
    channel_w: vec4<f32>,
    // Where the layer thins or thickens (`mPosDensityChgX`, `Y` on the
    // mesh), its radius and strength (`mPosDensityChgRange`, `…Power`).
    spot: vec4<f32>,
}

// The game's cloud light (`CloudLight` in clouds.rs).
struct CloudLight {
    // `mCloudColorScale`, 1 when the layers are lit like the game, 1 when
    // they are drawn with the game's textures.
    scale: vec4<f32>,
    upper: CloudLayerLight,
    lower: CloudLayerLight,
}

// How far an overcast layer takes the haze's colour (fit to the game's
// cloudy and rainy skies).
// SI-SKY-08: the no-dump cloud look is our own.
const DECK: f32 = 0.85;

// One layer's pattern along `dir` from `eye` (x), and a relief sample's
// and twice as far towards the sun (y, z).
fn layer_pattern(layer: CloudLayer, eye: vec3<f32>, dir: vec3<f32>) -> vec3<f32> {
    let along = max(layer.shape.x - eye.y, 1.0) / dir.y;
    let world = eye + dir * along;
    let uv = cloud_uv(layer, world.xz, globals.time);
    // The mip level a texture lookup would pick, from how fast uv changes.
    let size = f32(textureDimensions(noise_texture).x);
    let footprint = max(length(dpdx(uv * size)), length(dpdy(uv * size)));
    let lod = log2(max(footprint, 1e-6));

    let here = cloud_pattern(noise_texture, noise_sampler, layer, uv, lod);
    // Cloud towards the sun shades this point: look a little that way.
    let sun = normalize(params.sun.xyz);
    let towards = normalize(sun.xz + vec2(1e-4, 0.0)) * layer.drift.z;
    let near = cloud_pattern(noise_texture, noise_sampler, layer, uv + towards, lod);
    let far = cloud_pattern(noise_texture, noise_sampler, layer, uv + towards * 2.2, lod);
    return vec3(here, near, far);
}

// One layer seen along `dir` from `eye`: colour (premultiplied) and opacity.
fn layer_color(layer: CloudLayer, eye: vec3<f32>, dir: vec3<f32>) -> vec4<f32> {
    let pattern = layer_pattern(layer, eye, dir);
    let alpha = cloud_alpha(layer, pattern.x);
    let near = cloud_alpha(layer, pattern.y);
    let far = cloud_alpha(layer, pattern.z);
    let sun = normalize(params.sun.xyz);
    let light = clamp(1.0 - (near * 0.6 + far * 0.4) * 0.75, 0.0, 1.0);

    // Thick cores are darker seen from below.
    var color = mix(params.shade.rgb, params.lit.rgb, light);
    color *= mix(1.0, 0.82, alpha * alpha);
    // Thin edges seen against the light glow: a wide rim towards it and a
    // bright one close by (at sunset the game's clouds burn orange there).
    let facing = max(dot(dir, sun), 0.0);
    let into_sun = 0.1 * pow(facing, 6.0) + pow(facing, 32.0);
    let edge = saturate(4.0 * alpha * (1.0 - alpha) + (1.0 - alpha) * 0.3);
    color += params.backlight.rgb * into_sun * edge * params.backlight.w * light;
    // Overcast: a deck in the colour of the haze (already exposed), barely
    // shaded, as the game's cloudy and rainy skies are.
    let deck = smoothstep(0.5, 0.95, layer.pattern.y) * DECK;
    let decked = mix(color * params.lit.w, params.haze.rgb * mix(0.85, 1.05, light), deck);
    return vec4(decked * alpha, alpha);
}

// Rings of the game's dome mesh (`AGL_BuildCloudDomeVertices`, 24 around
// by 12 up and the apex).
const DOME_RINGS: u32 = 12u;

// Ring `j` of the dome mesh (radius, height; 1 across), the apex for j =
// 12: r = 1 − (j/12)³, y = √(1 − r²) − 0.07, heights under 0.1 pressed
// down to 0.1 − 0.3·(0.1 − y) first.
fn dome_ring(j: u32) -> vec2<f32> {
    let r = 1.0 - pow(f32(j) / f32(DOME_RINGS), 3.0);
    let y = sqrt(max(1.0 - r * r, 0.0));
    let pressed = select(y, 0.1 - 0.3 * (0.1 - y), y < 0.1);
    return select(vec2(r, pressed - 0.07), vec2(0.0, 1.0), j >= DOME_RINGS);
}

// VS 448 moves the rings under 0.1 out by 5 % and down by 0.007.
fn dome_moved(ring: vec2<f32>) -> vec2<f32> {
    return select(ring, vec2(ring.x * 1.05, ring.y - 0.007), ring.y < 0.1);
}

// Where the view ray `dir` from the dome's centre meets the moved mesh of a
// layer `scale` across and `height` high (`cScaleMat`: `mSkyScale`,
// `mSkyHeight`; its factor `Cloud+0x4cbc` stays the constructor's 1): the
// ring below (x), how far towards the next (y), 1 where the ray meets the
// mesh (z; below its lowest ring it does not). Turned about the vertical,
// the mesh's 24 sides are taken as round.
// SI-SKY-07: the dome mesh's 24 sides are taken as round.
fn dome_hit(dir: vec3<f32>, scale: f32, height: f32) -> vec3<f32> {
    let across = length(dir.xz);
    let size = vec2(scale, height);
    var below = dome_moved(dome_ring(0u)) * size;
    // Above the ray (> 0) or under it.
    var side_below = across * below.y - dir.y * below.x;
    var hit = vec3(0.0);
    for (var j = 0u; j < DOME_RINGS; j++) {
        let above = dome_moved(dome_ring(j + 1u)) * size;
        let side_above = across * above.y - dir.y * above.x;
        let here = side_below <= 0.0 && side_above >= 0.0 && hit.z == 0.0;
        let along = side_below / min(side_below - side_above, -1e-9);
        hit = select(hit, vec3(f32(j), saturate(along), 1.0), here);
        below = above;
        side_below = side_above;
    }
    return hit;
}

// What VS 448 passes on from one vertex of a layer's dome.
struct DomeVertex {
    // The texture coordinate (`mBaseTexScale` repeats), with its wave.
    uv: vec2<f32>,
    // The sky behind the cloud from the sky table (rgb), the scattering
    // fog's share S (a).
    sky: vec4<f32>,
    // The direction's height n.y (x), the density factor m around the
    // layer's spot (y).
    up: vec4<f32>,
}

// VS 448 at ring `j` of a layer's dome, in the direction `plan` (unit xz):
//   uv  = mBaseTexScale·(scroll + p.xz − 0.5), waved by 0.035·sin(10·u − scroll.y)
//         and −0.0175·sin(10·v − scroll.x)  (p: the moved vertex);
//   n   = normalize(cScaleMat·q)  (q: the vertex as built);
//   tA  = sat(depth·Dist.x − Dist.y),  S = sat(Coeff.z·(1 − (1 − tA)^Coeff.x))·mScatterHeight;
//   sky = table(1 − (2/π)·acos(0.5 + 0.5·n·(−L)), 0.5 + 0.5·(1 − tA)^Coeff.y).
fn dome_vertex(j: u32, plan: vec2<f32>, lit: CloudLayerLight, height: f32, tex: f32, look: Look) -> DomeVertex {
    let built = dome_ring(j);
    let moved = dome_moved(built);
    let scroll = lit.scroll.xy;
    let p = plan * moved.x;
    var uv = tex * (scroll + p - 0.5);
    uv = vec2(uv.x + 0.035 * sin(10.0 * uv.x - scroll.y), uv.y - 0.0175 * sin(10.0 * uv.y - scroll.x));
    let scale = lit.dome.x;
    let n = normalize(vec3(plan.x * built.x * scale, built.y * height, plan.y * built.x * scale));
    let world = vec3(plan.x * moved.x * scale, moved.y * height, plan.y * moved.x * scale);
    let depth = dot(world, -view.world_from_view[2].xyz);
    let fog = look.haze[1];
    let clear = max(1.0 - saturate((depth * 0.001 - fog.y) * fog.x), 0.0);
    let scatter = saturate(look.haze[0].a * (1.0 - pow(clear, fog.z))) * lit.dome.y;
    let v = 0.5 + 0.5 * pow(clear, look.spare[3].y);
    let c = dot(n, look.haze[3].xyz);
    let u = 1.0 - acos(clamp(0.5 + 0.5 * c, 0.0, 1.0)) * (2.0 / 3.14159265);
    let sky = sky_table(look_texture, vec2(u, v)).rgb * look.spare[3].x;
    // VS 448: within `mPosDensityChgRange` of the spot the density is
    // scaled by 1 + `mPosDensityChgPower`·(1 − d/range)² (on the moved mesh).
    let spot = lit.spot;
    let d = length(p - spot.xy);
    let factor = 1.0 - d / spot.z;
    let m = select(1.0, 1.0 + spot.w * factor * factor, spot.z > d);
    var out: DomeVertex;
    out.uv = uv;
    out.sky = vec4(sky, scatter);
    out.up = vec4(n.y, m, 0.0, 0.0);
    return out;
}

// Channel `pick` of the texel `t` as GX2's component selection gives it:
// 0–3 a channel, 4 zero, 5 one.
fn channel(t: vec4<f32>, pick: f32) -> f32 {
    let i = u32(pick);
    if i < 4u {
        return t[i];
    }
    return select(0.0, 1.0, i == 5u);
}

// PS 449's x and w of the noise texture (T2) and its blend (T3) at `uv`,
// blended by `mCloudTexBlendRate`: T2 + (T3 − T2)·rate.
fn noise_xw(noise: texture_2d<f32>, noise_blend: texture_2d<f32>, lit: CloudLayerLight, uv: vec2<f32>, rate: f32) -> vec2<f32> {
    let t2 = textureSample(noise, noise_mirror_sampler, uv);
    let t3 = textureSample(noise_blend, noise_mirror_sampler, uv);
    let a = vec2(channel(t2, lit.channel_x.z), channel(t2, lit.channel_w.z));
    let b = vec2(channel(t3, lit.channel_x.w), channel(t3, lit.channel_w.w));
    return mix(a, b, rate);
}

// The two noises at `p` (x and w of each): the first at
// `mNoiseScale1`·p + offset 1, the second at `mNoiseScale2`·p + offset 2
// with u and v swapped.
fn noises(noise: texture_2d<f32>, noise_blend: texture_2d<f32>, lit: CloudLayerLight, p: vec2<f32>, rate: f32) -> vec4<f32> {
    let o = lit.noise_offset;
    let first = noise_xw(noise, noise_blend, lit, lit.noise.x * p + o.xy, rate);
    let second = noise_xw(noise, noise_blend, lit, (lit.noise.y * p + o.zw).yx, rate);
    return vec4(first, second);
}

// The game's textures of a layer at `uv` (its texture coordinate drawn out
// to the rim) and `uv_e` (the relief sample), PS 449 (Wii U v208, matching
// Cemu `1e5b65a56cff348a`): both are warped by the noises there,
//   N  = (d1·n1 + d2·n2)/(d1 + d2)  (x and w; d = `mNoiseDensity1`, `2`),
//   uv' = uv + (1 + sat((r − start)/end)·power)·`mDistotion`·(N.x − 0.5, N.w − 0.5)
// (`mFarDistotionChg*`), and read in the base texture (T0) and its blend
// (T1), T0 + (T1 − T0)·rate, x (x, y). The noise at the relief sample also
// shades the sides:
//   N' = 6·(d1·n1.w + d2·n2.x) + 2·(d1·n1.x + d2·n2.w),
//   D  = 1 + (N' − 1)·`mDarkSideNoiseParam`  (z),
//   LS = 1 + (0.5·D − 0.4)·`mLightSideNoiseParam`  (w).
fn game_textures(
    base: texture_2d<f32>,
    base_blend: texture_2d<f32>,
    noise: texture_2d<f32>,
    noise_blend: texture_2d<f32>,
    layer: CloudLayer,
    lit: CloudLayerLight,
    uv: vec2<f32>,
    uv_e: vec2<f32>,
    r: f32,
) -> vec4<f32> {
    let rate = lit.texture_blend.x;
    let d = lit.noise.zw;
    let n = noises(noise, noise_blend, lit, uv, rate);
    let n_e = noises(noise, noise_blend, lit, uv_e, rate);
    let blend = (d.x * n.xy + d.y * n.zw) / (d.x + d.y);
    let blend_e = (d.x * n_e.xy + d.y * n_e.zw) / (d.x + d.y);
    let far = lit.far_distortion;
    let warp = (1.0 + saturate((r - far.x) / far.y) * far.z) * layer.pattern.z;
    let at = uv + warp * (blend - 0.5);
    let at_e = uv_e + warp * (blend_e - 0.5);
    let here = mix(
        channel(textureSample(base, cloud_sampler, at), lit.channel_x.x),
        channel(textureSample(base_blend, cloud_sampler, at), lit.channel_x.y),
        rate,
    );
    let there = mix(
        channel(textureSample(base, cloud_sampler, at_e), lit.channel_x.x),
        channel(textureSample(base_blend, cloud_sampler, at_e), lit.channel_x.y),
        rate,
    );
    let sides = 6.0 * (d.x * n_e.y + d.y * n_e.z) + 2.0 * (d.x * n_e.x + d.y * n_e.w);
    let dark = 1.0 + (sides - 1.0) * lit.side_noise.x;
    let light = 1.0 + (0.5 * dark - 0.4) * lit.side_noise.y;
    return vec4(here, there, dark, light);
}

// One layer along `dir` like the game's `cloud` shader (VS 448, PS 449),
// colour (premultiplied) and opacity. What the vertex shader passes on is
// worked out at the two vertices of the mesh around the pixel and blended
// between them, as the rasterizer would. The textures are the game's
// (`game_textures`) once read; until then this renderer's pattern stands in
// for the base textures, without the noises.
fn game_layer(
    layer: CloudLayer,
    lit: CloudLayerLight,
    look: Look,
    dir: vec3<f32>,
    base_texture: texture_2d<f32>,
    base_blend: texture_2d<f32>,
    noise: texture_2d<f32>,
    noise_blend: texture_2d<f32>,
) -> vec4<f32> {
    let height = layer.shape.x;
    let hit = dome_hit(dir, lit.dome.x, height);
    let plan = normalize(dir.xz + vec2(1e-7, 0.0));
    let tex = layer.pattern.x * lit.dome.w;
    let j = u32(hit.x);
    let lo = dome_vertex(j, plan, lit, height, tex, look);
    let hi = dome_vertex(j + 1u, plan, lit, height, tex, look);
    let uv = mix(lo.uv, hi.uv, hit.y);
    let sky = mix(lo.sky, hi.sky, hit.y);
    let up = mix(lo.up.x, hi.up.x, hit.y);
    let m = mix(lo.up.y, hi.up.y, hit.y);

    // PS 449: towards the rim (r, the mesh's radius here) the texture is
    // drawn out, the density and the opacity change; the relief sample e
    // lies towards the sun by `mEmbossWidth` of the way there.
    let scroll = lit.scroll.xy;
    let d = uv / tex - scroll + 0.5;
    let r = length(d);
    let far_uv = lit.far_uv;
    let drawn = uv + far_uv.z * d * pow(max(r * r, 1e-20), far_uv.w);
    // The sun on the mesh (`mIsSyncSunPosition`, `FUN_03a5b8a0` from
    // `ENV_CopySkyFogsToCloudObject`): the sky's light direction 0.07 up,
    // its height scaled by `mSkyScale/mSkyHeight`, normalized.
    let s = normalize(params.sun.xyz);
    let sun = normalize(vec3(s.x, (s.y + 0.07) * lit.dome.x / height, s.z)).xz;
    let to_sun = tex * (d - sun);
    let emboss = drawn - far_uv.x * to_sun;
    let far = saturate((r - lit.far_density.x) / lit.far_density.y) * lit.far_density.z;
    // The texture here and at the relief sample, the sides' shading D, LS.
    var read = vec4(0.0, 0.0, 1.0, 1.0);
    if game.scale.z > 0.5 {
        read = game_textures(base_texture, base_blend, noise, noise_blend, layer, lit, drawn, emboss, r);
    } else {
        let size = f32(textureDimensions(noise_texture).x);
        let footprint = max(length(dpdx(drawn * size)), length(dpdy(drawn * size)));
        let lod = log2(max(footprint, 1e-6));
        read.x = cloud_pattern(noise_texture, noise_sampler, layer, drawn, lod);
        read.y = cloud_pattern(noise_texture, noise_sampler, layer, emboss, lod);
    }
    let base = read.x;
    let base_e = read.y;
    let a = saturate(m * (base + layer.shape.y + far));
    let e = saturate(m * (base_e + layer.shape.y + far - 4.0 * far_uv.y));
    // `mAlphaMul` over 1 − `mAlphaThreshold` (`AGL_WriteCloudLayerUniforms`,
    // unless the threshold is 1).
    let threshold = layer.shape.w;
    let alpha_mul = select(layer.shape.z / (1.0 - threshold), layer.shape.z, threshold == 1.0);
    let opacity = saturate(alpha_mul * (a - threshold)) + 0.2 * clamp(e - threshold, 0.0, 0.95);
    // The cloud's own colour: the shadow colour where more cloud lies
    // towards the sun, the highlight where less does and near the sun
    // (s, the distance to it in the texture's repeats), the glow into it.
    let s_dist = length(to_sun);
    let rl = lit.relief;
    var color = mix(a * lit.base.rgb * read.w, a * lit.shadow.rgb, saturate(9.0 * read.z * (e + rl.x - a)));
    let h = saturate(10.0 * (a + rl.y - e));
    let highlight = pow(saturate(rl.z - s_dist), 2.0) + rl.w;
    color = mix(color, max(color, highlight * h * a * lit.hilight.rgb), saturate(10.0 * h));
    let back = lit.back;
    let glow = lit.backlight.w * pow(clamp(back.x - s_dist, 0.0, 0.5), 2.0);
    let rim = clamp(back.z * (0.25 - back.y * opacity), 0.0, 100.0);
    color = (color + 55.0 * glow * rim * lit.backlight.rgb) * clamp(1.0 - 5.0 * glow, 0.8, 1.0);
    let own = saturate(0.2 * glow + min(lit.dome.z, pow(max(sky.a * up, 0.0), 0.25)));
    var cloud = mix(sky.rgb, color * game.scale.x * look.spare[3].x, own);
    // The ad hoc fog, by the direction's height.
    let adhoc = look.adhoc;
    let f = adhoc[0].a * (1.0 - pow(saturate(up), adhoc[2].x));
    cloud = mix(cloud, adhoc[0].rgb, f);
    let far_alpha = saturate((r - lit.far_alpha.x) / lit.far_alpha.y) * lit.far_alpha.z;
    let alpha = saturate(opacity * (1.0 + 6.0 * glow) + far_alpha) * hit.z;
    return vec4(cloud * alpha, alpha);
}

#ifdef REDUCED_BUFFER
// The game draws the clouds into its reduced buffer (gsys "ReducedBuffer",
// half the frame each way; docs/research/wiiu-sky-resources.md, "Reduced
// cloud buffer") over the half depth that PS 109 copies from the frame's
// depth at the point of each of its pixels (point filter, the full texel
// under the pixel's uv). The dome lies behind everything, so a cloud is
// drawn where that texel holds no scene (Bevy's reversed depth is 0 there).
// The target starts at (0, 0, 0, 1) and blends `src + dst·(1 − α)` into the
// colour and `dst·(1 − α)` into the alpha: what shows of the frame.
@fragment
fn reduced(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let frame = vec2<f32>(textureDimensions(scene_depth));
    let texel = min(vec2<i32>(in.uv * frame), vec2<i32>(frame) - 1);
    let scene = textureLoad(scene_depth, texel, 0) > 0.0;
    let ndc = vec2(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let near = view.world_from_clip * vec4(ndc, 1.0, 1.0);
    let dir = normalize(near.xyz / near.w - view.world_position);
    return select(sky_clouds(dir), vec4(0.0), scene);
}
#else
@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return sky_clouds(normalize(in.world_position.xyz - view.world_position));
}
#endif

// The clouds seen along `dir`, premultiplied.
fn sky_clouds(dir: vec3<f32>) -> vec4<f32> {
    let eye = view.world_position;
    let look = read_look(look_texture);
    if game.scale.y > 0.5 {
        // The layers end where their meshes do, just under the horizon.
        let upper = game_layer(
            swayed(params.upper), game.upper, look, dir,
            upper_base, upper_base_blend, upper_noise, upper_noise_blend,
        );
        let lower = game_layer(
            swayed(params.lower), game.lower, look, dir,
            lower_base, lower_base_blend, lower_noise, lower_noise_blend,
        );
        return lower + upper * (1.0 - lower.a);
    }
    // Only the sky above the horizon has clouds.
    if dir.y < 0.005 {
        return vec4(0.0);
    }
    let upper = layer_color(swayed(params.upper), eye, dir);
    let lower = layer_color(swayed(params.lower), eye, dir);
    var sum = lower + upper * (1.0 - lower.a);
    // Low in the sky the layers are far away: they fade into the haze,
    // which glows towards the light (a bright band under the sunset).
    let haze = 1.0 - smoothstep(0.02, 0.35, dir.y);
    let glow = params.backlight.rgb * params.haze.w * pow(max(dot(dir, normalize(params.sun.xyz)), 0.0), 8.0);
    sum = vec4(mix(sum.rgb, (params.haze.rgb + glow) * sum.a, haze * 0.7), sum.a);
    return sum * horizon_fade(dir.y);
}
