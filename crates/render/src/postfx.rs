// SI-PFX-02: curve output treated as linear light.
//! The game's post-processing in place of Bevy's bloom and tone mapping:
//! the bloom of `agl::pfx::Bloom` and the tone curve of KSys
//! `hdr_compose` (Wii U v208; programs and CPU uploads in
//! `docs/research/wiiu-postfx-materials.md`).
//!
//! Bloom (`0x03aa0714`): a mask at a quarter of the frame (`bloom_mask`:
//! what is brighter than the threshold, clamped, times the intensity),
//! four levels each half the one before (a bilinear halving, then a
//! nine-texel gaussian across and down), then the levels gathered from the
//! smallest up with the blend constant: level 3 = level 4 · colour 4 +
//! level 3 · colour 3, level 2 = level 3 · colour 3 + level 2 · colour 2,
//! level 1 = level 2 · gather + level 1 · gather · colour 1 (colours times
//! their alpha). `hdr_compose` adds level 1 to the frame before the curve.
//!
//! The curve's output is linear light: KSys draws into R10G10B10A2 and the
//! TV buffer is sRGB (`0x03a1147c`), which the viewer's sRGB surface
//! repeats. Bevy's tone mapping is off (`Tonemapping::None`).
//!
//! Open: how the viewer's exposed colour maps onto the game's HDR buffer
//! (`BOTW_HDR_SCALE`, 1 by default), which variant of the curve a frame
//! uses (`BOTW_TONE=lut` for the colour table, PS 549), later writers of
//! `cParam` and of the bloom's scale `+0xc10`, and how KSys hands the
//! palettes' `Bloom*` to the bloom (read by name here).

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::core_pipeline::tonemapping::tonemapping;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::extract_component::{
    ComponentUniforms, DynamicUniformIndex, ExtractComponent, ExtractComponentPlugin,
    UniformComponentPlugin,
};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::ViewTarget;
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::daynight::{Environment, Sky};

const SHADER: Handle<Shader> = uuid_handle!("5b0e8c3a-2f47-4d19-9a6e-71c4d2e8b035");

/// The format of the bloom's levels (internal format `0x1a`: GX2
/// R11_G11_B10_FLOAT).
const LEVEL_FORMAT: TextureFormat = TextureFormat::Rg11b10Ufloat;
/// The mask's size against the frame (`+0xc04`).
const MASK_SCALE: f32 = 0.25;
/// Levels after the mask.
const LEVELS: usize = 4;
/// The bloom is skipped for frames smaller than this.
const SMALLEST_FRAME: u32 = 64;

/// The mask's luminance weights (`0x03a9d594`, bits 0x3e990afe,
/// 0x3f162c23, 0x3dea7371), divided by their sum as the game does.
const MASK_WEIGHTS: Vec3 = Vec3::new(0.298_912, 0.586_611, 0.114_478);
/// `cParam` from the KSys constructor (`0x034051f0`): the saturation at
/// black and white and in the middle.
// SI-PFX-01: tone and bloom inputs not traced past the constructor.
const SATURATION_ENDS: f32 = 0.99;
const SATURATION_MIDDLE: f32 = 1.17;
/// The bloom's scale on the threshold and its range (`+0xc10`, 1 from the
/// constructor; later writers not traced).
const BLOOM_SCALE: f32 = 1.0;

pub struct PostFxPlugin;

impl Plugin for PostFxPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "postfx.wgsl", Shader::from_wgsl);
        app.insert_resource(ToneChoice::from_env())
            .add_plugins((
                ExtractComponentPlugin::<PostFx>::default(),
                UniformComponentPlugin::<PostFx>::default(),
            ))
            .add_systems(PostUpdate, update_postfx);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(
                Render,
                prepare_levels.in_set(RenderSystems::PrepareResources),
            )
            .add_systems(
                Core3d,
                game_postfx
                    .before(tonemapping)
                    .in_set(Core3dSystems::PostProcess),
            );
    }
}

/// Which variant of `hdr_compose` to draw, and the scale from the viewer's
/// exposed colour to the game's HDR values (both open, see the module).
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct ToneChoice {
    pub color_table: bool,
    pub hdr_scale: f32,
    /// `BOTW_NO_BLOOM`: the curve alone, to see what the bloom adds.
    pub no_bloom: bool,
}

impl ToneChoice {
    fn from_env() -> Self {
        // SI-PFX-01: tone and bloom inputs not traced past the constructor.
        let color_table = std::env::var("BOTW_TONE").is_ok_and(|v| v == "lut");
        let hdr_scale = std::env::var("BOTW_HDR_SCALE")
            .ok()
            .and_then(|v| v.parse().ok())
            .filter(|s: &f32| *s > 0.0)
            .unwrap_or(1.0);
        Self {
            color_table,
            hdr_scale,
            no_bloom: std::env::var_os("BOTW_NO_BLOOM").is_some(),
        }
    }
}

/// The camera's post-processing, as uploaded to `postfx.wgsl`.
#[derive(Component, ExtractComponent, ShaderType, Clone, Copy, Debug, PartialEq)]
pub struct PostFx {
    pub mask_weight: Vec4,
    pub mask_param: Vec4,
    pub layer1: Vec4,
    pub layer2: Vec4,
    pub layer3: Vec4,
    pub layer4: Vec4,
    pub final_gather: Vec4,
    pub tone: Vec4,
    pub correction: Vec4,
}

impl Default for PostFx {
    fn default() -> Self {
        postfx(&Environment::fallback(), None, ToneChoice::from_env())
    }
}

/// A colour times its alpha, as the bloom's CPU uploads it.
fn weighted(color: [f32; 4]) -> Vec4 {
    let c = Vec4::from(color);
    (c.truncate() * c.w).extend(1.0)
}

/// The post-processing for the palette and weather in `sky` (the renderer's
/// base values without one).
pub fn postfx(environment: &Environment, sky: Option<&Sky>, choice: ToneChoice) -> PostFx {
    let base = &environment.renderer.bloom;
    // The palettes' `BloomThreshhold`, `BloomIntencity` and
    // `BloomClampedLuminance` in place of the base's, times the weather's
    // factors (`TempMgr`'s chase on the field's row, `climate::Moisture`).
    // SI-PFX-01: tone and bloom inputs not traced past the constructor.
    let (threshold, intensity, clamped) = match sky {
        Some(sky) => (
            sky.palette.bloom.threshold * sky.scalars.bloom_threshold,
            sky.palette.bloom.intensity * sky.scalars.bloom_intensity,
            base.clamped_luminance
                .map(|_| sky.palette.bloom.clamped_luminance),
        ),
        None => (base.threshold, base.intensity, base.clamped_luminance),
    };
    // `0x03a9e648`: the threshold and its range times the scale; nothing
    // blooms with no range.
    let range = base.threshold_range * BLOOM_SCALE;
    let over_range = if range > 0.0 { 1.0 / range } else { 0.0 };
    let weights = MASK_WEIGHTS / 1.000_001;
    let mask_weight = (weights * over_range).extend(-threshold * BLOOM_SCALE * over_range);
    let mask_param = Vec4::new(
        clamped.unwrap_or(0.0) * over_range,
        0.0,
        intensity,
        if clamped.is_some() { 1.0 } else { 0.0 },
    );
    // `EnvPaletteStatic`'s layer colours where they are in use, the base's
    // `color1`–`color4` otherwise; the gather colour is `BloomComposeColor`.
    let statics = &environment.palette_static;
    let layer = |i: usize| {
        let (color, unused) = statics.bloom_layers[i];
        weighted(if unused { base.colors[i] } else { color })
    };
    let correction = &environment.renderer.color;
    let (saturation, brightness) = if correction.enable {
        (correction.saturation, correction.brightness)
    } else {
        (1.0, 1.0)
    };
    PostFx {
        mask_weight,
        mask_param,
        layer1: layer(0),
        layer2: layer(1),
        layer3: layer(2),
        layer4: layer(3),
        final_gather: weighted(statics.bloom_compose_color),
        tone: Vec4::new(
            if choice.color_table { 1.0 } else { 0.0 },
            SATURATION_ENDS,
            SATURATION_MIDDLE - SATURATION_ENDS,
            choice.hdr_scale,
        ),
        correction: Vec4::new(
            saturation,
            brightness,
            if base.enable && !choice.no_bloom {
                1.0
            } else {
                0.0
            },
            0.0,
        ),
    }
}

impl PostFx {
    /// The blend constants of the three gathering passes (level 3, 2, 1).
    fn blend_constants(&self) -> [Vec4; 3] {
        let gather = (self.final_gather.truncate() * self.layer1.truncate()).extend(1.0);
        [self.layer3, self.layer2, gather]
    }
}

fn update_postfx(
    sky: Option<Res<Sky>>,
    environment: Option<Res<Environment>>,
    choice: Res<ToneChoice>,
    mut commands: Commands,
    mut cameras: Query<(Entity, Option<&mut PostFx>), crate::camera::MainCamera>,
) {
    let Some(environment) = environment else {
        return;
    };
    let new = postfx(&environment, sky.as_deref(), *choice);
    for (entity, current) in &mut cameras {
        match current {
            Some(mut current) => {
                current.set_if_neq(new);
            }
            None => {
                commands.entity(entity).insert(new);
            }
        }
    }
}

#[derive(Resource)]
struct PostFxPipelines {
    single: BindGroupLayoutDescriptor,
    compose: BindGroupLayoutDescriptor,
    sampler: Sampler,
    mask: CachedRenderPipelineId,
    copy: CachedRenderPipelineId,
    blur_x: CachedRenderPipelineId,
    blur_y: CachedRenderPipelineId,
    gather: [CachedRenderPipelineId; 3],
    hdr_compose: CachedRenderPipelineId,
}

fn init_pipelines(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
) {
    let single = BindGroupLayoutDescriptor::new(
        "postfx_single",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<PostFx>(true),
            ),
        ),
    );
    let compose = BindGroupLayoutDescriptor::new(
        "postfx_compose",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<PostFx>(true),
                texture_2d(TextureSampleType::Float { filterable: true }),
            ),
        ),
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("postfx"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    let pipeline = |entry: &'static str,
                    layout: &BindGroupLayoutDescriptor,
                    format: TextureFormat,
                    blend: Option<BlendState>| {
        pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(format!("postfx_{entry}").into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: SHADER,
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    };
    // The gathering blend (`0x03a9f934`, CB_BLEND_CONTROL 0x0d010d01):
    // source · 1 + destination · constant.
    let gathering = Some(BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::Constant,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent::REPLACE,
    });
    commands.insert_resource(PostFxPipelines {
        mask: pipeline("mask", &single, LEVEL_FORMAT, None),
        copy: pipeline("copy", &single, LEVEL_FORMAT, None),
        blur_x: pipeline("blur_x", &single, LEVEL_FORMAT, None),
        blur_y: pipeline("blur_y", &single, LEVEL_FORMAT, None),
        gather: [
            pipeline("compose4", &single, LEVEL_FORMAT, gathering),
            pipeline("compose3", &single, LEVEL_FORMAT, gathering),
            pipeline("compose_final", &single, LEVEL_FORMAT, gathering),
        ],
        hdr_compose: pipeline(
            "hdr_compose",
            &compose,
            // The HDR main texture (the camera has `Hdr`).
            TextureFormat::Rgba16Float,
            None,
        ),
        single,
        compose,
        sampler,
    });
}

/// The bloom's mask and levels (`levels[0]` is the mask) and a scratch
/// texture per level for the blur's first pass.
#[derive(Component)]
struct BloomLevels {
    levels: Vec<CachedTexture>,
    scratch: Vec<CachedTexture>,
}

/// The mask's size: a quarter of the frame, rounded up to four texels
/// (`0x03aa0714`).
fn mask_size(frame: UVec2) -> UVec2 {
    let quarter = (frame.as_vec2() * MASK_SCALE).as_uvec2();
    (quarter + 3) & !3
}

fn prepare_levels(
    mut commands: Commands,
    mut texture_cache: ResMut<TextureCache>,
    render_device: Res<RenderDevice>,
    views: Query<(Entity, &ExtractedCamera), With<PostFx>>,
) {
    for (entity, camera) in &views {
        let Some(frame) = camera.physical_viewport_size else {
            continue;
        };
        if frame.x < SMALLEST_FRAME || frame.y < SMALLEST_FRAME {
            commands.entity(entity).remove::<BloomLevels>();
            continue;
        }
        let base = mask_size(frame);
        let mut texture = |label: &'static str, level: usize| {
            let size = (base >> level as u32).max(UVec2::ONE);
            texture_cache.get(
                &render_device,
                TextureDescriptor {
                    label: Some(label),
                    size: Extent3d {
                        width: size.x,
                        height: size.y,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: LEVEL_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let levels = (0..=LEVELS)
            .map(|i| texture("postfx_bloom_level", i))
            .collect();
        let scratch = (1..=LEVELS)
            .map(|i| texture("postfx_bloom_scratch", i))
            .collect();
        commands
            .entity(entity)
            .insert(BloomLevels { levels, scratch });
    }
}

/// One fullscreen draw into `target` (cleared first unless `blend_constant`
/// asks to blend with what is there).
fn draw(
    encoder: &mut CommandEncoder,
    label: &'static str,
    target: &TextureView,
    (pipeline, bind_group, offset): (&RenderPipeline, &BindGroup, u32),
    blend_constant: Option<Vec4>,
) {
    let load = match blend_constant {
        Some(_) => LoadOp::Load,
        None => LoadOp::Clear(Default::default()),
    };
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load,
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[offset]);
    if let Some(c) = blend_constant {
        pass.set_blend_constant(LinearRgba::new(c.x, c.y, c.z, c.w).into());
    }
    pass.draw(0..3, 0..1);
}

fn game_postfx(
    view: ViewQuery<(
        &ViewTarget,
        &PostFx,
        &DynamicUniformIndex<PostFx>,
        Option<&BloomLevels>,
    )>,
    pipelines: Res<PostFxPipelines>,
    pipeline_cache: Res<PipelineCache>,
    uniforms: Res<ComponentUniforms<PostFx>>,
    mut ctx: RenderContext,
) {
    let (target, settings, uniform_index, levels) = view.into_inner();
    let Some(uniform) = uniforms.binding() else {
        return;
    };
    let get = |id| pipeline_cache.get_render_pipeline(id);
    let (Some(mask), Some(copy), Some(blur_x), Some(blur_y), Some(hdr_compose)) = (
        get(pipelines.mask),
        get(pipelines.copy),
        get(pipelines.blur_x),
        get(pipelines.blur_y),
        get(pipelines.hdr_compose),
    ) else {
        return;
    };
    let gather: Option<Vec<&RenderPipeline>> = pipelines.gather.iter().map(|&id| get(id)).collect();
    let Some(gather) = gather else { return };
    let offset = uniform_index.index();
    let single_layout = pipeline_cache.get_bind_group_layout(&pipelines.single);
    let device = ctx.render_device().clone();
    let single = |input: &TextureView| {
        device.create_bind_group(
            "postfx_single",
            &single_layout,
            &BindGroupEntries::sequential((input, &pipelines.sampler, uniform.clone())),
        )
    };

    let post_process = target.post_process_write();
    let bloom_on = settings.correction.z > 0.5;
    let encoder = ctx.command_encoder();
    encoder.push_debug_group("game_postfx");
    let bloom = match levels {
        Some(levels) if bloom_on => {
            let view = |t: &CachedTexture| t.default_view.clone();
            let lv: Vec<TextureView> = levels.levels.iter().map(view).collect();
            let sc: Vec<TextureView> = levels.scratch.iter().map(view).collect();
            let source = single(post_process.source);
            draw(encoder, "bloom_mask", &lv[0], (mask, &source, offset), None);
            for i in 0..LEVELS {
                let (level, halved, across) = (single(&lv[i]), single(&lv[i + 1]), single(&sc[i]));
                draw(
                    encoder,
                    "bloom_reduce",
                    &lv[i + 1],
                    (copy, &level, offset),
                    None,
                );
                draw(
                    encoder,
                    "bloom_gaussian",
                    &sc[i],
                    (blur_x, &halved, offset),
                    None,
                );
                draw(
                    encoder,
                    "bloom_gaussian",
                    &lv[i + 1],
                    (blur_y, &across, offset),
                    None,
                );
            }
            let constants = settings.blend_constants();
            for (k, level) in [3, 2, 1].into_iter().enumerate() {
                let smaller = single(&lv[level + 1]);
                let pass = (gather[k], &smaller, offset);
                draw(
                    encoder,
                    "bloom_compose",
                    &lv[level],
                    pass,
                    Some(constants[k]),
                );
            }
            Some(lv[1].clone())
        }
        _ => None,
    };
    // Without a bloom the shader skips the texture; bind the frame there.
    let bloom_view = bloom.as_ref().unwrap_or(post_process.source);
    let compose_group = device.create_bind_group(
        "postfx_compose",
        &pipeline_cache.get_bind_group_layout(&pipelines.compose),
        &BindGroupEntries::sequential((
            post_process.source,
            &pipelines.sampler,
            uniform.clone(),
            bloom_view,
        )),
    );
    let pass = (hdr_compose, &compose_group, offset);
    draw(encoder, "hdr_compose", post_process.destination, pass, None);
    encoder.pop_debug_group();
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_format::env::Influence;

    #[test]
    fn the_weather_lowers_the_threshold_and_strengthens_the_glow() {
        let environment = Environment::fallback();
        let mut sky = crate::fog::tests::sky_at(&environment, 12.0);
        let choice = ToneChoice {
            color_table: false,
            hdr_scale: 1.0,
            no_bloom: false,
        };
        let clear = postfx(&environment, Some(&sky), choice);
        sky.scalars = Influence {
            bloom_threshold: 0.1,
            bloom_intensity: 1.5,
            ..Influence::default()
        };
        let rain = postfx(&environment, Some(&sky), choice);
        // The threshold is minus w over the range (x over the weight).
        let threshold =
            |p: &PostFx| -p.mask_weight.w / p.mask_weight.x * MASK_WEIGHTS.x / 1.000_001;
        assert!((threshold(&clear) - sky.palette.bloom.threshold).abs() < 1e-4);
        assert!((threshold(&rain) - 0.1 * sky.palette.bloom.threshold).abs() < 1e-4);
        assert!(rain.mask_param.z > clear.mask_param.z);
    }

    #[test]
    fn a_layer_marked_unused_keeps_the_base_colour() {
        let mut environment = Environment::fallback();
        environment.renderer.bloom.colors[2] = [1.0, 0.5, 0.25, 1.0];
        environment.palette_static.bloom_layers[2] = ([0.0; 4], true);
        environment.palette_static.bloom_layers[0] = ([1.0, 1.0, 1.0, 0.5], false);
        let p = postfx(&environment, None, ToneChoice::from_env());
        assert_eq!(p.layer3, Vec4::new(1.0, 0.5, 0.25, 1.0));
        assert_eq!(p.layer1, Vec4::new(0.5, 0.5, 0.5, 1.0));
    }

    #[test]
    fn the_mask_is_a_quarter_rounded_up_to_four() {
        assert_eq!(mask_size(UVec2::new(1280, 720)), UVec2::new(320, 180));
        assert_eq!(mask_size(UVec2::new(1920, 1080)), UVec2::new(480, 272));
    }
}
