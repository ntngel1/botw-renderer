//! The game's volume shadow mask: what reads as light shafts in the fog
//! (agl `VolumeMask` and KSys `merge_mask`, Wii U v208;
//! docs/research/light-shafts.md).
//!
//! gsys builds two VolumeMask units per view, KSys overrides them
//! ([`UNITS`]): unit 0 slices the first 20 m of the view into 96 layers at
//! 1/8 of its size, unit 1 the first 400 m into 128 layers at 1/16. Each
//! layer looks up the sun's cascade shadow map and adds lit or unlit air
//! into a float target; a 1-2-1 blur each way follows, then the merge
//! averages the two units ([`MERGE_K`]) into an R8G8 target that the
//! scene's fog reads as `gsys_user2`: `.x` lets lit air keep the full sun
//! in-scatter near the camera (`m` of `apply_haze` in look.wgsl), `.y` is
//! the indoor mask.
//!
//! Here each pixel of a unit's target sums its layers at once
//! (`volume_mask.wgsl`), after the depth prepass and the shadow maps and
//! before the main pass, and the merge is drawn into the look texture's
//! mask rows (`look::LOOK_MASK_ROW`), which every material that hazes
//! already binds (`read_look_at` reads it at the surface's pixel).
//!
//! Not the game's: the shadow map and its cascades are Bevy's (SI-LGT-05);
//! the frame's depth is the depth prepass (SI-VOL-05: grass and water are
//! not in it);
//! the samplers of the merge and of `gsys_user2` (SI-VOL-01), the layers'
//! depth test (SI-VOL-02) and the indoor mask (SI-VOL-03) are inferred; the
//! cube map's faces keep `gsys_user2` = 0 (SI-VOL-04).

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::Camera3dDepthTextureUsage;
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::core_3d::main_opaque_pass_3d;
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::pbr::{
    GpuLights, LightMeta, ShadowSamplers, ViewLightsUniformOffset, ViewShadowBindings,
};
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_2d_array, texture_depth_2d, uniform_buffer,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::{CachedTexture, GpuImage, TextureCache};
use bevy::render::view::{
    ExtractedView, ViewDepthTexture, ViewUniform, ViewUniformOffset, ViewUniforms,
};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::camera::MainCamera;
use crate::look::{LOOK_MASK_ROW, LOOK_MASK_SIZE, Look, LookSystems, LookTexture};

const SHADER: Handle<Shader> = uuid_handle!("6a1f4c2e-8d37-4b95-a0e6-2c9b7d413f58");

/// A VolumeMask unit's parameters (agl param list `unit_%d`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unit {
    /// `autoAdjustNear`: the layers start at the camera's near plane.
    pub auto_adjust_near: bool,
    /// `layer_number`.
    pub layers: u32,
    /// `layer_reduce_level`: the target is the view's size >> this.
    pub reduce: u32,
    /// `layer_dist_nonlinear` (1: evenly spaced).
    pub nonlinear: f32,
    /// `range_near`, `range_far` (m).
    pub near: f32,
    pub far: f32,
    /// `shadowmap_amp_low`, `shadowmap_amp_high`: what a shadowed and a lit
    /// layer add, times 1/(N − 1).
    pub amp_low: f32,
    pub amp_high: f32,
}

/// The two units as KSys sets them (`0x03405f48`, PPC
/// `0x0340671c..0x03406870`); both enabled, Gaussian kernel index 0
/// (GAUSSIAN_KERNEL=3), no PCF beyond the compare's.
pub const UNITS: [Unit; 2] = [
    Unit {
        auto_adjust_near: true,
        layers: 96,
        reduce: 3,
        nonlinear: 1.0,
        near: 0.1,
        far: 20.0,
        amp_low: 0.0,
        amp_high: 2.0,
    },
    Unit {
        auto_adjust_near: true,
        layers: 128,
        reduce: 4,
        nonlinear: 1.0,
        near: 0.1,
        far: 400.0,
        amp_low: -1.0,
        amp_high: 1.0,
    },
];

/// `cVolumeMask.x` of the merge: KSys `+0x868`, 0.5 (`0x034055e0`) with
/// both units on, so the mask is ½A + ½B.
pub const MERGE_K: f32 = 0.5;

/// Where a unit's layers lie: the first one's depth and the span to the
/// last (`0x03a63108`): with `autoAdjustNear` and a near plane `n_c` > 0,
/// near = n_c and span = max(½(far − near), far − n_c).
pub fn layer_range(unit: &Unit, camera_near: f32) -> (f32, f32) {
    if unit.auto_adjust_near && camera_near > 0.0 {
        let span = (0.5 * (unit.far - unit.near)).max(unit.far - camera_near);
        (camera_near, span)
    } else {
        (unit.near, unit.far - unit.near)
    }
}

/// The view depth of layer `i` of `unit` (before the game's tiny offset).
pub fn layer_depth(unit: &Unit, near: f32, span: f32, i: u32) -> f32 {
    let t = i as f32 / (unit.layers - 1) as f32;
    let t = if unit.nonlinear == 1.0 {
        t
    } else {
        t.powf(unit.nonlinear)
    };
    near + span * t
}

/// A unit's target size for a view of `frame` pixels.
pub fn unit_size(unit: &Unit, frame: UVec2) -> UVec2 {
    (frame >> unit.reduce).max(UVec2::ONE)
}

/// One unit's layers as `volume_mask.wgsl` reads them (`Unit` there).
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub struct LayerParams {
    pub range: Vec4,
    pub amp: Vec4,
    pub frame: Vec4,
}

pub fn layer_params(unit: &Unit, camera_near: f32, frame: UVec2) -> LayerParams {
    let (near, span) = layer_range(unit, camera_near);
    LayerParams {
        range: Vec4::new(
            near,
            span,
            1.0 / (unit.layers - 1) as f32,
            unit.layers as f32,
        ),
        amp: Vec4::new(
            unit.amp_high - unit.amp_low,
            unit.amp_low,
            camera_near,
            unit.nonlinear,
        ),
        frame: Vec4::new(frame.x as f32, frame.y as f32, unit.reduce as f32, 0.0),
    }
}

/// The merged mask's size: the larger unit's.
pub fn merged_size(frame: UVec2) -> UVec2 {
    UNITS
        .iter()
        .map(|u| unit_size(u, frame))
        .fold(UVec2::ONE, UVec2::max)
}

pub struct VolumeMaskPlugin;

impl Plugin for VolumeMaskPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "volume_mask.wgsl", Shader::from_wgsl);
        app.add_plugins(ExtractComponentPlugin::<VolumeMaskView>::default())
            .add_systems(PostUpdate, follow_view.before(LookSystems::Upload));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(
                Render,
                prepare_textures.in_set(RenderSystems::PrepareResources),
            )
            .add_systems(
                Core3d,
                draw_volume_mask
                    .before(main_opaque_pass_3d)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

/// A view the mask is drawn for (the main camera) and its near plane.
#[derive(Component, ExtractComponent, Clone, Copy, Debug)]
pub struct VolumeMaskView {
    pub near: f32,
}

/// Marks the main camera, lets the mask read its depth, and tells the
/// materials where the mask is ([`Look::volume_mask`]).
#[allow(clippy::type_complexity)]
fn follow_view(
    mut commands: Commands,
    mut cameras: Query<
        (
            Entity,
            &Camera,
            &Projection,
            &mut Camera3d,
            Option<&VolumeMaskView>,
        ),
        MainCamera,
    >,
    mut look: ResMut<Look>,
) {
    let mut texels = [Vec4::ZERO; 4];
    if let Ok((entity, camera, projection, mut camera3d, marked)) = cameras.single_mut() {
        let usages = TextureUsages::from(camera3d.depth_texture_usages);
        if !usages.contains(TextureUsages::TEXTURE_BINDING) {
            camera3d.depth_texture_usages =
                Camera3dDepthTextureUsage::from(usages | TextureUsages::TEXTURE_BINDING);
        }
        if let Projection::Perspective(perspective) = projection {
            let near = perspective.near;
            if marked.is_none_or(|m| m.near != near) {
                commands.entity(entity).insert(VolumeMaskView { near });
            }
            if let Some(frame) = camera.physical_target_size().filter(|s| s.x > 1 && s.y > 1) {
                let size = merged_size(frame);
                let fits =
                    size.x as usize <= LOOK_MASK_SIZE[0] && size.y as usize <= LOOK_MASK_SIZE[1];
                let lens = projection.get_clip_from_view().y_axis.y;
                if fits {
                    texels[0] = Vec4::new(size.x as f32, size.y as f32, lens, 1.0);
                }
            }
        }
    }
    if look.volume_mask != texels {
        look.volume_mask = texels;
    }
}

#[derive(Resource)]
struct VolumeMaskPipelines {
    layers_layout: BindGroupLayoutDescriptor,
    blur_layout: BindGroupLayoutDescriptor,
    merge_layout: BindGroupLayoutDescriptor,
    layers: CachedRenderPipelineId,
    blur: CachedRenderPipelineId,
    merge: CachedRenderPipelineId,
    linear: Sampler,
}

/// A unit's layer target and blurred mask: agl format 9, R16 float.
const UNIT_FORMAT: TextureFormat = TextureFormat::R16Float;

fn init_pipelines(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
) {
    let layers_layout = BindGroupLayoutDescriptor::new(
        "volume_mask_layers",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            (
                (0, uniform_buffer::<ViewUniform>(true)),
                (1, uniform_buffer::<GpuLights>(true)),
                (2, texture_2d_array(TextureSampleType::Depth)),
                (3, sampler(SamplerBindingType::Comparison)),
                (4, texture_depth_2d()),
                (5, uniform_buffer::<LayerParams>(false)),
            ),
        ),
    );
    let blur_layout = BindGroupLayoutDescriptor::new(
        "volume_mask_blur",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            ((
                10,
                texture_2d(TextureSampleType::Float { filterable: false }),
            ),),
        ),
    );
    let merge_layout = BindGroupLayoutDescriptor::new(
        "volume_mask_merge",
        &BindGroupLayoutEntries::with_indices(
            ShaderStages::FRAGMENT,
            (
                (
                    20,
                    texture_2d(TextureSampleType::Float { filterable: true }),
                ),
                (
                    21,
                    texture_2d(TextureSampleType::Float { filterable: true }),
                ),
                (22, sampler(SamplerBindingType::Filtering)),
                (23, uniform_buffer::<Vec4>(false)),
            ),
        ),
    );
    let pipeline = |label: &'static str,
                    layout: &BindGroupLayoutDescriptor,
                    entry: &'static str,
                    format: TextureFormat| {
        pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(label.into()),
            layout: vec![layout.clone()],
            vertex: fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader: SHADER,
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        })
    };
    let layers = pipeline("volume_mask_layers", &layers_layout, "layers", UNIT_FORMAT);
    let blur = pipeline("volume_mask_blur", &blur_layout, "blur", UNIT_FORMAT);
    // The look texture's format.
    let merge = pipeline(
        "volume_mask_merge",
        &merge_layout,
        "merge",
        TextureFormat::Rgba16Float,
    );
    let linear = render_device.create_sampler(&SamplerDescriptor {
        label: Some("volume_mask_linear"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..default()
    });
    commands.insert_resource(VolumeMaskPipelines {
        layers_layout,
        blur_layout,
        merge_layout,
        layers,
        blur,
        merge,
        linear,
    });
}

/// Each unit's layer target and blurred mask for this frame.
#[derive(Component)]
struct VolumeMaskTextures {
    layers: [CachedTexture; 2],
    masks: [CachedTexture; 2],
}

fn prepare_textures(
    mut commands: Commands,
    views: Query<(Entity, &ExtractedView), With<VolumeMaskView>>,
    mut cache: ResMut<TextureCache>,
    render_device: Res<RenderDevice>,
) {
    for (entity, view) in &views {
        let frame = view.viewport.zw();
        let mut texture = |label: &'static str, unit: &Unit| {
            let size = unit_size(unit, frame);
            cache.get(
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
                    format: UNIT_FORMAT,
                    usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                },
            )
        };
        let layers = [
            texture("volume mask - layer 0", &UNITS[0]),
            texture("volume mask - layer 1", &UNITS[1]),
        ];
        let masks = [
            texture("volume mask - mask 0", &UNITS[0]),
            texture("volume mask - mask 1", &UNITS[1]),
        ];
        commands
            .entity(entity)
            .insert(VolumeMaskTextures { layers, masks });
    }
}

/// One fullscreen draw into `target`, cleared; within `area` (x, y, w, h)
/// of it when given, the rest kept.
fn draw(
    encoder: &mut CommandEncoder,
    label: &'static str,
    target: &TextureView,
    area: Option<UVec4>,
    pipeline: &RenderPipeline,
    group: &BindGroup,
    offsets: &[u32],
) {
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: if area.is_some() {
                    LoadOp::Load
                } else {
                    LoadOp::Clear(LinearRgba::NONE.into())
                },
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(a) = area {
        let f = a.as_vec4();
        pass.set_viewport(f.x, f.y, f.z, f.w, 0.0, 1.0);
        pass.set_scissor_rect(a.x, a.y, a.z, a.w);
    }
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, group, offsets);
    pass.draw(0..3, 0..1);
}

#[allow(clippy::too_many_arguments)]
fn draw_volume_mask(
    view: ViewQuery<(
        &ExtractedView,
        &ViewDepthTexture,
        &ViewUniformOffset,
        &ViewLightsUniformOffset,
        &ViewShadowBindings,
        &VolumeMaskView,
        &VolumeMaskTextures,
    )>,
    pipelines: Option<Res<VolumeMaskPipelines>>,
    pipeline_cache: Res<PipelineCache>,
    view_uniforms: Res<ViewUniforms>,
    light_meta: Res<LightMeta>,
    shadow_samplers: Res<ShadowSamplers>,
    look: Option<Res<LookTexture>>,
    images: Res<RenderAssets<GpuImage>>,
    render_queue: Res<RenderQueue>,
    mut ctx: RenderContext,
) {
    let (extracted, depth, view_offset, lights_offset, shadows, mask_view, textures) =
        view.into_inner();
    let (Some(pipelines), Some(look)) = (pipelines, look) else {
        return;
    };
    let Some(look) = images.get(&look.0) else {
        return;
    };
    let get = |id| pipeline_cache.get_render_pipeline(id);
    let (Some(layers_pipeline), Some(blur_pipeline), Some(merge_pipeline)) = (
        get(pipelines.layers),
        get(pipelines.blur),
        get(pipelines.merge),
    ) else {
        return;
    };
    let (Some(view_binding), Some(lights_binding)) = (
        view_uniforms.uniforms.binding(),
        light_meta.view_gpu_lights.binding(),
    ) else {
        return;
    };
    let frame = extracted.viewport.zw();
    let size = merged_size(frame);
    if size.x as usize > LOOK_MASK_SIZE[0] || size.y as usize > LOOK_MASK_SIZE[1] {
        return;
    }
    let device = ctx.render_device().clone();
    let params = UNITS.map(|unit| {
        let mut buffer = UniformBuffer::from(layer_params(&unit, mask_view.near, frame));
        buffer.write_buffer(&device, &render_queue);
        buffer
    });
    let mut merge = UniformBuffer::from(Vec4::new(
        MERGE_K,
        LOOK_MASK_ROW as f32,
        size.x as f32,
        size.y as f32,
    ));
    merge.write_buffer(&device, &render_queue);
    let layer_groups: Vec<BindGroup> = params
        .iter()
        .filter_map(|p| p.binding())
        .map(|p| {
            device.create_bind_group(
                "volume_mask_layers",
                &pipeline_cache.get_bind_group_layout(&pipelines.layers_layout),
                &BindGroupEntries::with_indices((
                    (0, view_binding.clone()),
                    (1, lights_binding.clone()),
                    (2, &shadows.directional_light_depth_texture_view),
                    (3, &shadow_samplers.directional_light_comparison_sampler),
                    // SI-VOL-05: the depth prepass stands in for the G-buffer's depth.
                    (4, depth.view()),
                    (5, p),
                )),
            )
        })
        .collect();
    let Some(merge_binding) = merge.binding() else {
        return;
    };
    if layer_groups.len() != 2 {
        return;
    }
    let blur_groups = textures.layers.each_ref().map(|layer| {
        device.create_bind_group(
            "volume_mask_blur",
            &pipeline_cache.get_bind_group_layout(&pipelines.blur_layout),
            &BindGroupEntries::with_indices(((10, &layer.default_view),)),
        )
    });
    let merge_group = device.create_bind_group(
        "volume_mask_merge",
        &pipeline_cache.get_bind_group_layout(&pipelines.merge_layout),
        &BindGroupEntries::with_indices((
            (20, &textures.masks[0].default_view),
            (21, &textures.masks[1].default_view),
            (22, &pipelines.linear),
            (23, merge_binding),
        )),
    );
    let encoder = ctx.command_encoder();
    encoder.push_debug_group("volume_mask");
    for unit in 0..2 {
        draw(
            encoder,
            "volume_mask_layers",
            &textures.layers[unit].default_view,
            None,
            layers_pipeline,
            &layer_groups[unit],
            &[view_offset.offset, lights_offset.offset],
        );
        draw(
            encoder,
            "volume_mask_blur",
            &textures.masks[unit].default_view,
            None,
            blur_pipeline,
            &blur_groups[unit],
            &[],
        );
    }
    draw(
        encoder,
        "volume_mask_merge",
        &look.texture_view,
        Some(UVec4::new(0, LOOK_MASK_ROW as u32, size.x, size.y)),
        merge_pipeline,
        &merge_group,
        &[],
    );
    encoder.pop_debug_group();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_start_at_the_camera_near_plane() {
        let close =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-4;
        assert!(close(layer_range(&UNITS[0], 0.3), (0.3, 19.7)));
        assert!(close(layer_range(&UNITS[1], 0.3), (0.3, 399.7)));
        // Without a near plane the unit's own range.
        assert!(close(layer_range(&UNITS[0], 0.0), (0.1, 19.9)));
        // A far near plane keeps at least half the range.
        assert!(close(layer_range(&UNITS[0], 15.0), (15.0, 9.95)));
    }

    #[test]
    fn layers_span_the_range_evenly() {
        let (near, span) = layer_range(&UNITS[1], 0.3);
        assert!((layer_depth(&UNITS[1], near, span, 0) - 0.3).abs() < 1e-6);
        assert!((layer_depth(&UNITS[1], near, span, 127) - 400.0).abs() < 1e-3);
        let step = layer_depth(&UNITS[1], near, span, 1) - near;
        assert!((step - 399.7 / 127.0).abs() < 1e-4);
    }

    /// A ray lit all the way adds `hi` per layer drawn, a shadowed one `lo`
    /// (each over N − 1): unit 0 reaches 2·95/95, unit 1 ±127/127 (the
    /// layer at the near plane is not drawn).
    #[test]
    fn a_lit_ray_sums_to_the_amplitude() {
        for (unit, lit, dark) in [(&UNITS[0], 2.0, 0.0), (&UNITS[1], 1.0, -1.0)] {
            let p = layer_params(unit, 0.3, UVec2::new(1600, 900));
            let (near, span) = layer_range(unit, 0.3);
            let drawn = (0..unit.layers)
                .filter(|&i| layer_depth(unit, near, span, i) > 0.3)
                .count() as f32;
            let sum = |s: f32| drawn * (s * p.amp.x + p.amp.y) * p.range.z;
            assert!((sum(1.0) - lit).abs() < 1e-5, "{}", sum(1.0));
            assert!((sum(0.0) - dark).abs() < 1e-5, "{}", sum(0.0));
        }
    }

    #[test]
    fn the_merged_mask_is_the_finer_units_size() {
        let frame = UVec2::new(1600, 900);
        assert_eq!(unit_size(&UNITS[0], frame), UVec2::new(200, 112));
        assert_eq!(unit_size(&UNITS[1], frame), UVec2::new(100, 56));
        assert_eq!(merged_size(frame), UVec2::new(200, 112));
        // The largest view the look texture holds a mask for.
        let largest = UVec2::new(4096, 2304);
        assert_eq!(
            merged_size(largest),
            UVec2::new(LOOK_MASK_SIZE[0] as u32, LOOK_MASK_SIZE[1] as u32)
        );
    }

    #[test]
    fn the_shader_reduces_like_unit_0() {
        let shader = include_str!("look.wgsl");
        let factor = (1u32 << UNITS[0].reduce) as f32;
        assert!(shader.contains(&format!("const LOOK_MASK_REDUCE: f32 = {factor:.1};")));
    }
}
