#define_import_path botw::character_material
// The characters' lighting (character_material.rs): the game's deferred
// shading of characters, `uking_sys_shading` programs 0 (nonmetal), 4
// (metal), 12 (hair), 16 (skin) and 20 (eyes) read from the game's shaders
// (docs/research/wiiu-character-shading.md): a hard step where the main
// light turns away, an ambient that does not depend on the normal
// (`LightAnalyzer`'s sky colours above and below the view), rim lights
// found in the depth around the pixel, and the albedo brightened where the
// highlights are. Cloud shadows and the shared haze as elsewhere. Replaces
// `apply_pbr_lighting`. The environment's mean brightness comes from the
// texel `deferred_light::CubeMean` draws once a frame (binding 106).

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    mesh_view_bindings::globals,
    mesh_bindings::mesh,
}
#import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_2d}
#import botw::clouds::{CloudParams, cloud_shadow}
#import botw::look::{Look, read_look_at, apply_haze}

#ifdef PREPASS_PIPELINE
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_deferred_functions::deferred_output,
}
#else
#import bevy_pbr::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
    pbr_types::PbrInput,
    mesh_view_bindings as view_bindings,
    mesh_view_bindings::view,
    view_transformations::{direction_world_to_view, position_world_to_view, frag_coord_to_uv},
}
#import botw::deferred_light::{
    Frame, luma, max3, min3, cube, cube_mean, SUNLIT_SHARE, edge, game_frame, main_light, local_lights,
}
#import botw::normal_map::mapped_normal
#endif

struct CharacterParams {
    clouds: CloudParams,
    // x: `uking_chara_size`, y: `uking_material_behave`, z: 1 when the
    // extra maps carry the skin's transmission (blue), w: the extra maps' UV
    // set (0, 1).
    kind: vec4<f32>,
    // The behaviour's colour.
    behave: vec4<f32>,
    // Roughness without and with a full specular mask (xy), metal at a full
    // metal mask (z): to read the masks back; highlight strength (w).
    gloss: vec4<f32>,
}

#ifdef BINDLESS
// Per material slot: where its parameters and textures are.
struct CharacterIndices {
    params: u32,
    shadow_map: u32,
    shadow_sampler: u32,
    look: u32,
    maps: u32,
    maps_sampler: u32,
    // Not `cube_mean`: naga_oil would rename it as the imported function.
    mean_texel: u32,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(107) var<storage> character_indices: array<CharacterIndices>;
@group(#{MATERIAL_BIND_GROUP}) @binding(108) var<storage> character_params: array<CharacterParams>;
#else
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> material_params: CharacterParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var cloud_shadow_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var cloud_shadow_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var look_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var maps_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var maps_sampler: sampler;
// The environment's mean brightness, once a frame (deferred_light.rs).
@group(#{MATERIAL_BIND_GROUP}) @binding(106) var cube_means: texture_2d<f32>;
#endif

#ifndef PREPASS_PIPELINE
// `uking_material_behave` of the shading classes.
const BEHAVE_METAL: f32 = 1.0;
const BEHAVE_HAIR: f32 = 100.0;
const BEHAVE_SKIN: f32 = 101.0;
const BEHAVE_EYE: f32 = 104.0;

fn is_behave(params: CharacterParams, code: f32) -> bool {
    return abs(params.kind.y - code) < 0.5;
}

// Catchlights on the eyes: two discs on the iris, in the eye texture's UV
// (centre and radius; the iris is round the middle). The game's G-buffer
// carries them in the eyes' albedo alpha (drawn procedurally, proctexture 4
// and 5 of the eyeball material); these are a fit to its frames.
// SI-LGT-16: eye catchlights fitted to game frames.
const CATCHLIGHTS: array<vec3<f32>, 2> = array<vec3<f32>, 2>(vec3<f32>(0.47, 0.41, 0.07), vec3<f32>(0.4, 0.62, 0.035));

// How much of a catchlight the eye shows at texture coordinate `uv`.
fn catchlight(uv: vec2<f32>) -> f32 {
    let blur = max(length(fwidth(uv)), 1e-3);
    var light = 0.0;
    for (var i = 0; i < 2; i += 1) {
        let spot = CATCHLIGHTS[i];
        light = max(light, 1.0 - smoothstep(spot.z - blur, spot.z + blur, distance(uv, spot.xy)));
    }
    return light;
}

// How much more saturated skin and hair are than their textures (fits: the
// game's skin is a saturated orange in every frame, shade included, its
// hair a strong gold; its lighting adds no saturation, so the colour must
// come before it — the materials' colour combiners are not read,
// RENDER-003).
// SI-LGT-17: character saturation, gloss and AO are our fits.
const SKIN_SATURATION: f32 = 1.45;
const HAIR_SATURATION: f32 = 1.3;

// Skin's and hair's albedo, more saturated at the same brightness.
fn styled_albedo(albedo: vec3<f32>, params: CharacterParams) -> vec3<f32> {
    var saturation = 1.0;
    if is_behave(params, BEHAVE_SKIN) {
        saturation = SKIN_SATURATION;
    } else if is_behave(params, BEHAVE_HAIR) {
        saturation = HAIR_SATURATION;
    }
    let grey = dot(albedo, vec3<f32>(0.2126, 0.7152, 0.0722));
    return max(mix(vec3<f32>(grey), albedo, saturation), vec3<f32>(0.0));
}

// --- LightAnalyzer: the characters' ambient (gsys_user4 texels 0, 4, 5) ---

// The analyzer's parameters (`ksysla` defaults, 0x038b37c0).
const CHARA_SAT: f32 = 0.5;
const CHARA_SAT_MIN: f32 = 0.75;
const CHARA_OFFSET_MIN: f32 = 0.1;
const CHARA_OFFSET_MAX: f32 = 0.5;
const CHARA_SCALE: f32 = 1.75;
// One of the analyzer's character colours: the cube map towards `dir`,
// its saturation raised, scaled against the environment's mean.
fn chara_ambient(dir: vec3<f32>, mean: f32, e: f32) -> vec3<f32> {
    let c = cube(normalize(dir));
    let m = max3(c);
    let s = 1.0 - min3(c) / (m + 1e-10);
    let f = mix(CHARA_SAT_MIN, 1.0, e) * pow(s + 1e-4, CHARA_SAT - 1.0);
    // SI-LGT-15: game ambient multipliers and toon_adjust taken as 1.
    return (m + (c - m) * f) * CHARA_SCALE / (mean + mix(CHARA_OFFSET_MIN, CHARA_OFFSET_MAX, e));
}

fn screen_dir(v: vec3<f32>) -> vec2<f32> {
    return normalize(vec2<f32>(v.x, v.y) + vec2<f32>(1e-6, 0.0));
}

// --- The main pass ---

// A character's surface lit by the game's deferred shading: colour before
// the haze, in the frame's exposed units.
fn character_color(
    pbr: PbrInput,
    params: CharacterParams,
    maps: vec4<f32>,
    cloud_shade: f32,
    frame: Frame,
    catchlights: f32,
    means: texture_2d<f32>,
) -> vec3<f32> {
    let albedo_in = styled_albedo(pbr.material.base_color.rgb, params);
    let N = pbr.N;
    // From the camera to the point.
    let V = -pbr.V;
    let Nv = normalize(direction_world_to_view(N));

    // The main light (sun or moon): where it goes (`L`, the game's
    // environment[4]) and its colour on a white surface.
    let sun = main_light(pbr, frame.p.z);
    let to_light = sun.to_light;
    let C = sun.color;
    let shadow = sun.shadow;
    let L = -to_light;
    let Lv = direction_world_to_view(L);

    // The analyzer's ambient: the sky ahead above (top of the screen) and
    // below (bottom), its share of shadow weight `w`.
    let x = SUNLIT_SHARE;
    let e = x * x * x;
    let w = saturate(1.2 * x);
    // Towards the view's horizontal (the camera's forward, `cLook`).
    let forward = -view.world_from_view[2].xyz;
    let ahead = vec3<f32>(forward.x, 0.0, forward.z);
    let mean = cube_mean(means);
    // SI-LGT-17: character saturation, gloss and AO are our fits.
    let amb = mix(chara_ambient(ahead + vec3(0.0, 1.0, 0.0), mean, e), chara_ambient(ahead - vec3(0.0, 1.0, 0.0), mean, e), frame.uv.y);

    // The G-buffer's inputs: occlusion (the game codes it in the normal's
    // length), gloss, the skin's transmission, the character's size.
    let ao = saturate((maps.r - 0.5) * 4.0);
    let spec_mask = saturate((params.gloss.x - pbr.material.perceptual_roughness) / max(params.gloss.x - params.gloss.y, 1e-3));
    let gloss2 = spec_mask * spec_mask;
    let transmission = select(0.0, floor(maps.b * 15.0 + 0.5) / 15.0, params.kind.z > 0.5);
    var k = frame.d * exp2(params.kind.x - 2.0);
    let eye = is_behave(params, BEHAVE_EYE);
    if eye {
        k = frame.d * 0.25;
    }

    let r = saturate(1.0 + dot(N, V));
    let b = saturate(dot(V, to_light));
    let s = (1.0 - cloud_shade) * saturate((shadow - 0.4) * 10.0);
    let n_l = saturate(dot(N, to_light));
    let t = saturate((n_l - 0.4) * 75.0 * (1.0 + 0.1 * frame.d));
    let lit = mix(1.0, s, saturate(w));
    let cw = saturate(w + s);
    let amb_mod = 1.0 + (saturate(ao + 0.8) - 1.0) * saturate(5.0 * b);
    let Dc = mix(0.8 * amb, C, cw);

    // The rim lights: against the light at the silhouette (e1), along the
    // normal (e2) and towards the light (e3).
    let n2 = screen_dir(Nv);
    let l2 = screen_dir(Lv);
    let reach = min(k, 2.0);
    let e1 = saturate(edge(frame, n2 * vec2<f32>(6.0, 1.2) * reach) * 0.01);
    let e2 = saturate(0.12 * frame.fovy * edge(frame, n2 * 2.0 * saturate(k)));
    let e3 = saturate(0.12 * frame.fovy * edge(frame, l2 * 3.0 * saturate(k)));
    var q = saturate(4.0 * k) * saturate((e1 * saturate(dot(N, normalize(V - 0.8 * L)) + 1.0) * b - 0.4) * 75.0);
    let rim_to_light = saturate(r * b * saturate((e3 - 0.5) * 40.0) * 8.0) * saturate(2.0 * k) * 0.25;
    let t_plain = saturate((n_l - 0.4) * 75.0);
    let rim_lit = saturate(6.0 * k) * saturate(75.0 * (r * e2 * saturate(5.0 - 5.0 * dot(V, L)) * t_plain - 0.4)) * 0.4;
    let Hs = normalize(to_light - 0.3 * V);

    var light = vec3<f32>(0.0);
    var h = 0.0;
    var albedo = albedo_in;
    var sparkle = vec3<f32>(0.0);
    if is_behave(params, BEHAVE_METAL) {
        // Metal (PS 4): half the shadow, a highlight fixed to the view.
        let lit_m = 0.5 * lit + 0.5;
        let Hm = normalize(vec3<f32>(-1.5 * Lv.x, 0.6, 1.0));
        let g = pow(saturate(dot(Nv, Hm)), exp(gloss2 + 3.0));
        let hm = saturate(saturate((g - 0.005) * 100.0) * 0.2 + saturate((g - 0.14) * 70.0) * 0.3);
        let D = ao * lit_m * t;
        let E = saturate(lit_m * saturate(dot(N, to_light) + 0.9) * hm * gloss2 * 8.0 + rim_to_light) * 3.0 + saturate(D);
        light = Dc * E + amb * amb_mod;
    } else if eye {
        // Eyes (PS 20): dimmer from afar, their catchlights on top.
        let fade = 0.6 * saturate(0.15 * frame.d) + 0.4;
        let D = ao * mix(q, lit, t) * fade;
        light = Dc * D + amb * 1.8 * fade * amb_mod;
        // The eyeball in the upper lid's shadow (the `_sd0` mask), brighter
        // by its behaviour's first value (const_color4.x = 1.4).
        albedo *= maps.g * max(params.behave.x, 1.0);
        sparkle = catchlights * (0.6 * saturate(0.075 * frame.d) + 0.4) * (0.8 * Dc + 0.2);
    } else {
        let hair = is_behave(params, BEHAVE_HAIR);
        let skin = is_behave(params, BEHAVE_SKIN);
        let m = max3(albedo_in);
        let sat = 1.0 - min3(albedo_in) / (m + 1e-10);
        var D = 0.0;
        if hair {
            // Hair (PS 12): a broad highlight in two steps, warm shade.
            let g2 = saturate(6.0 * k) * gloss2 * pow(saturate(dot(N, Hs)), 6.0);
            let ha = saturate(saturate((g2 - 0.14) * 70.0) * 0.7) * saturate(sat + 1.9 * m + 0.1);
            h = saturate(rim_to_light + saturate(ha * 0.1 * saturate(lit + 0.5) + rim_lit));
            D = saturate(ao * mix(saturate(q), lit, t));
            light = Dc * D + (w * (1.0 - D) * Dc * 0.08 + amb) * amb_mod;
        } else {
            // Skin (PS 16) and everything else (PS 0): a narrow highlight
            // at the silhouette; skin lets light through where it is thin
            // and warms its shade.
            var g = 0.0;
            if skin {
                g = r * pow(saturate(dot(N, Hs)), 20.0);
                q = saturate(q + b * saturate((transmission * saturate(dot(N, normalize(-V - L))) - 0.5) * 40.0) * 0.15);
            } else {
                g = pow(r, 0.4) * pow(saturate(dot(N, Hs)), 12.0);
            }
            let g2 = g * saturate(1.2 * k) * gloss2;
            let h0 = saturate((g2 - 0.01) * 50.0) * 0.05 + saturate((g2 - 0.05) * 20.0) * 0.12;
            h = saturate(rim_to_light + saturate(saturate(lit + 0.08) * saturate(h0) + rim_lit));
            if skin {
                D = ao * mix(q, lit, t);
                light = Dc * D + (w * (1.0 - D) * Dc * vec3<f32>(0.08, 0.04, 0.0) + amb) * amb_mod;
            } else {
                // SI-LGT-17: character saturation, gloss and AO are our fits.
                D = saturate(ao * mix(q, lit, t));
                light = Dc * D + amb * amb_mod;
            }
        }
        // The albedo where the highlights are: paler and brighter.
        // Achromatic albedo is already paled. Evaluate its limit directly:
        // the saturation ratio otherwise becomes indeterminate on Metal
        // and contaminates the bloom pyramid (UMii faces expose this).
        var paled = vec3(m);
        if sat > 0.0 {
            paled = m + (albedo_in - m) * (saturate((1.0 - h) * sat) / (sat + 1e-10));
        }
        albedo = vec3(0.0);
        if m > 0.0 {
            albedo = paled * saturate(m + h * pow(m, 0.6) * (4.0 - 3.0 * sat)) / (m + 1e-10);
        }
    }

    // Local lights (campfires): Bevy's, soft, without the step. The game's
    // path for them on characters is not traced (RENDER-003).
    // SI-LGT-17: character saturation, gloss and AO are our fits.
    let local = local_lights(pbr, N, albedo, frame.p.z);

    return albedo * light + sparkle + local;
}
#endif

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);

#ifdef PREPASS_PIPELINE
    let out = deferred_output(in, pbr_input);
#else
    let out = shade_character(in, is_front, pbr_input, vec4<f32>(-1.0));
#endif
    return out;
}

#ifndef PREPASS_PIPELINE
fn shade_character(in: VertexOutput, is_front: bool, input: PbrInput, maps_override: vec4<f32>) -> FragmentOutput {
    var pbr_input = input;
    var out: FragmentOutput;
    pbr_input.N = mapped_normal(in, is_front, pbr_input);
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let indices = character_indices[slot];
    let params = character_params[indices.params];
    let look = read_look_at(bindless_textures_2d[indices.look], in.world_position.xyz);
    let shade = cloud_shadow(
        bindless_textures_2d[indices.shadow_map],
        bindless_samplers_filtering[indices.shadow_sampler],
        params.clouds,
        in.world_position.xyz,
    );
    let means = bindless_textures_2d[indices.mean_texel];
#else
    let params = material_params;
    let look = read_look_at(look_texture, in.world_position.xyz);
    let shade = cloud_shadow(cloud_shadow_map, cloud_shadow_sampler, params.clouds, in.world_position.xyz);
    let means = cube_means;
#endif
    // The extra maps, on their UV set.
    var maps = vec4<f32>(1.0, 1.0, 0.0, 1.0);
#ifdef VERTEX_UVS_A
    var map_uv = in.uv;
#ifdef VERTEX_UVS_B
    map_uv = select(in.uv, in.uv_b, params.kind.w > 0.5);
#endif
#ifdef BINDLESS
    maps = textureSample(bindless_textures_2d[indices.maps], bindless_samplers_filtering[indices.maps_sampler], map_uv);
#else
    maps = textureSample(maps_texture, maps_sampler, map_uv);
#endif
#endif
    if maps_override.w > 0.0 { maps = maps_override; }
    let frame = game_frame(position_world_to_view(in.world_position.xyz), frag_coord_to_uv(in.position.xy));

    var catchlights = 0.0;
#ifdef VERTEX_UVS_A
    catchlights = catchlight(in.uv);
#endif
    var lit = character_color(pbr_input, params, maps, shade, frame, catchlights, means);
    let emissive = pbr_input.material.emissive;
    lit += emissive.rgb * mix(1.0, view.exposure, emissive.a);
    let world = in.world_position.xyz;
    lit = apply_haze(look, lit, world, normalize(world - view_bindings::view.world_position));
    out.color = main_pass_post_lighting_processing(pbr_input, vec4<f32>(lit, pbr_input.material.base_color.a));
    return out;
}
#endif
