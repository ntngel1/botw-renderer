//! The clouds' reduced buffer, like the game's gsys "ReducedBuffer"
//! (`mIsDrawReduceBuffer`, Wii U v208; docs/research/wiiu-sky-resources.md,
//! "Reduced cloud buffer"). After the opaque pass the frame's depth
//! gives two normalized linear depths, (z − n)/(f − n): the whole frame's
//! in R16F (`render_buffer_depth` PS 93) and one at half size in R32F from
//! the texel under each pixel (PS 109). The clouds are then drawn at half
//! size (⌊W/2⌋ × ⌊H/2⌋, RGBA16F by `gsys.bgmsconf` `Main`'s
//! `reduced_buffer_16bit`) into a target cleared to (0, 0, 0, 1), where the
//! half depth has no scene. In the transparent pass, where the clouds
//! stood (behind the stars and the sky's haze), `render_buffer_color`
//! DRAW_COLOR=5 (PS 133, `reduced_buffer_edge_adjust` on, coefficient 2)
//! lays the target over the frame: `compose.wgsl`.
//!
//! The cube map's cameras keep drawing the clouds straight into their faces
//! (the game's cube map callback draws them there, not through this buffer).
//! Not the game's: the planes n, f are the viewer camera's (the result
//! depends on them only through R16F's rounding), the frame's depth is
//! Bevy's (reversed, sample 0 under MSAA), and the dome still lies behind
//! all of the scene as in `clouds.rs`.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::camera::{Camera3dDepthTextureUsage, visibility::RenderLayers};
use bevy::core_pipeline::FullscreenShader;
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::schedule::{Core3d, Core3dSystems};
use bevy::ecs::system::{StaticSystemParam, SystemParamItem};
use bevy::image::ImageSampler;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::globals::{GlobalsBuffer, GlobalsUniform};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{
    texture_depth_2d, texture_depth_2d_multisampled, uniform_buffer,
};
use bevy::render::render_resource::*;
use bevy::render::renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::{Msaa, ViewDepthTexture, ViewUniform, ViewUniformOffset, ViewUniforms};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::{ShaderDefVal, ShaderRef};

use super::{CloudDome, CloudMaterial, DOME_RADIUS};

const DEPTH_SHADER: Handle<Shader> = uuid_handle!("c5e2a8f4-6d31-4b7e-9a05-3f18d6b2e947");
const COMPOSE_SHADER: Handle<Shader> = uuid_handle!("71b0d9c3-4e8a-4f25-b6d1-a2c4e7f93058");
/// `botw::cloud_view`: the reduced pass's view bindings.
const VIEW_SHADER: Handle<Shader> = uuid_handle!("e8a4c1d7-2b95-4f60-8c3e-5d7a0b9f1624");

/// `reduced_buffer_edge_adjust_coeff` of `gsys.bgmsconf` `Main` (the gsys
/// object's `+0xa4`, `FUN_039b0848`).
const EDGE_ADJUST_COEFF: f32 = 2.0;
/// The reduced colour: agl 0x2b, GX2 R16G16B16A16_FLOAT
/// (`reduced_buffer_16bit` on).
const COLOR_FORMAT: TextureFormat = TextureFormat::Rgba16Float;
/// The frame's NLD: GX2 R16_FLOAT (`nld_32bit` off).
const FULL_FORMAT: TextureFormat = TextureFormat::R16Float;
/// The half NLD: GX2 R32_FLOAT (`nld_half_32bit` on).
const HALF_FORMAT: TextureFormat = TextureFormat::R32Float;
/// The reduced buffer's clear colour (`gsys+0x94`, `FUN_03a0501c`).
const CLEAR: LinearRgba = LinearRgba::new(0.0, 0.0, 0.0, 1.0);

pub struct ReducedBufferPlugin;

impl Plugin for ReducedBufferPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, VIEW_SHADER, "../cloud_view.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, DEPTH_SHADER, "reduced_depth.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, COMPOSE_SHADER, "compose.wgsl", Shader::from_wgsl);
        app.init_resource::<ReducedFrame>()
            .add_plugins((
                MaterialPlugin::<CloudComposeMaterial>::default(),
                ExtractResourcePlugin::<ReducedFrame>::default(),
                ExtractComponentPlugin::<ReducedClouds>::default(),
            ))
            .add_systems(Startup, spawn_compose)
            .add_systems(
                PostUpdate,
                (mark_cameras, follow_frame)
                    .chain()
                    .after(super::update_clouds)
                    .before(TransformSystems::Propagate),
            );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init_pipelines)
            .add_systems(
                Render,
                prepare_buffers.in_set(RenderSystems::PrepareBindGroups),
            )
            .add_systems(
                Core3d,
                draw_reduced_clouds
                    .after(main_opaque_pass_3d)
                    .before(main_transparent_pass_3d)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

/// A camera whose clouds go through the reduced buffer (the main one).
#[derive(Component, ExtractComponent, Clone, Copy, Debug)]
pub struct ReducedClouds;

/// The targets and what the reduced pass draws: the camera's planes and
/// the clouds' material as `clouds.rs` leaves it this frame.
#[derive(Resource, ExtractResource, Clone, Default)]
pub struct ReducedFrame {
    color: Handle<Image>,
    full: Handle<Image>,
    half: Handle<Image>,
    /// The frame's size W, H; zero until the camera has one.
    frame: UVec2,
    near: f32,
    far: f32,
    clouds: Option<CloudMaterial>,
}

/// `render_buffer_color` DRAW_COLOR=5's `Context` (`FUN_03a05de0`) and the
/// frame's size.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq)]
pub struct ComposeParams {
    pub param0: Vec4,
    pub param1: Vec4,
    pub param2: Vec4,
    pub frame: Vec4,
}

/// The planes n, f, the view's fovy (tan of its half) and the frame's size
/// W, H give the compose's `Context` (the game's `rec+4` bit 13 is clear:
/// its edge-adjust depth pass never shows in the game's frames).
pub fn compose_params(near: f32, far: f32, tan_half_fovy: f32, frame: Vec2) -> ComposeParams {
    let span = far - near;
    ComposeParams {
        param0: Vec4::new(near / span, 1.0 - near / far, span / far, span / near),
        param1: Vec4::new(span, near, far, EDGE_ADJUST_COEFF / tan_half_fovy),
        param2: Vec4::new(frame.x * 0.5, frame.y * 0.5, 2.0 / frame.x, 2.0 / frame.y),
        frame: frame.extend(0.0).extend(0.0),
    }
}

/// The reduced buffer's size: half the frame each way, cut to whole texels
/// (`FUN_03a05de0` sets the area, `FUN_03a071f8` truncates it).
pub fn reduced_size(frame: UVec2) -> UVec2 {
    (frame / 2).max(UVec2::ONE)
}

/// The reduced buffer laid over the frame (`compose.wgsl`).
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct CloudComposeMaterial {
    #[uniform(0)]
    pub compose: ComposeParams,
    #[texture(1)]
    #[sampler(2)]
    pub color: Handle<Image>,
    #[texture(3, sample_type = "float", filterable = false)]
    pub full: Handle<Image>,
    #[texture(4, sample_type = "float", filterable = false)]
    pub half: Handle<Image>,
}

impl Material for CloudComposeMaterial {
    fn fragment_shader() -> ShaderRef {
        COMPOSE_SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    /// Where the clouds were drawn: over the stars and the sky's haze,
    /// behind every other transparent thing.
    fn depth_bias(&self) -> f32 {
        -1.0e8
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        // CB_BLEND_CONTROL 0x04010401: src·1 + dst·src.α, colour and alpha.
        let blend = BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::SrcAlpha,
            operation: BlendOperation::Add,
        };
        if let Some(fragment) = &mut descriptor.fragment {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState {
                    color: blend,
                    alpha: blend,
                });
            }
        }
        // Over the whole frame (`FUN_03a078fc` turns the depth test off).
        if let Some(depth) = &mut descriptor.depth_stencil {
            depth.depth_compare = Some(CompareFunction::Always);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// The dome the compose is drawn on, around the main camera.
#[derive(Component)]
struct ComposeDome;

fn spawn_compose(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CloudComposeMaterial>>,
) {
    let material = materials.add(CloudComposeMaterial {
        compose: ComposeParams::default(),
        color: Handle::default(),
        full: Handle::default(),
        half: Handle::default(),
    });
    commands.spawn((
        Name::new("clouds (reduced buffer)"),
        ComposeDome,
        Mesh3d(meshes.add(Sphere::new(DOME_RADIUS).mesh().uv(48, 24))),
        MeshMaterial3d(material),
        Transform::default(),
        // Shown once the targets are there.
        Visibility::Hidden,
        NoFrustumCulling,
        NotShadowCaster,
        NotShadowReceiver,
        RenderLayers::layer(0),
    ));
}

/// Main cameras draw their clouds through the reduced buffer and let it
/// read their depth.
fn mark_cameras(
    mut commands: Commands,
    mut cameras: Query<
        (Entity, &mut Camera3d),
        (crate::camera::MainCamera, Without<ReducedClouds>),
    >,
) {
    for (entity, mut camera) in &mut cameras {
        let usages =
            TextureUsages::from(camera.depth_texture_usages) | TextureUsages::TEXTURE_BINDING;
        camera.depth_texture_usages = Camera3dDepthTextureUsage::from(usages);
        commands.entity(entity).insert(ReducedClouds);
    }
}

/// A target of `size` (`fill`: one texel's bytes).
fn target(size: UVec2, format: TextureFormat, fill: &[u8]) -> Image {
    let mut image = Image::new_target_texture(size.x, size.y, format, None);
    image.data = Some(fill.repeat((size.x * size.y) as usize));
    image
}

/// Sizes the targets to the camera's frame, hands the clouds' material and
/// the camera's planes to the render world and keeps the compose on the
/// camera.
#[allow(clippy::type_complexity)]
fn follow_frame(
    cameras: Query<(&Camera, &Projection, &GlobalTransform), With<ReducedClouds>>,
    clouds: Query<&MeshMaterial3d<CloudMaterial>, With<CloudDome>>,
    cloud_materials: Res<Assets<CloudMaterial>>,
    mut domes: Query<
        (
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<CloudComposeMaterial>,
        ),
        With<ComposeDome>,
    >,
    mut materials: ResMut<Assets<CloudComposeMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut reduced: ResMut<ReducedFrame>,
) {
    let Ok((mut transform, mut visibility, material)) = domes.single_mut() else {
        return;
    };
    let Ok((camera, Projection::Perspective(projection), eye)) = cameras.single() else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let Some(frame) = camera.physical_target_size().filter(|s| s.x > 1 && s.y > 1) else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    if transform.translation != eye.translation() {
        transform.translation = eye.translation();
    }
    if !materials.contains(&material.0) {
        return;
    }
    // Only a changed material is re-extracted and re-bound.
    let resized = frame != reduced.frame;
    if resized {
        let half = reduced_size(frame);
        // What shows of the frame starts at 1, so an undrawn buffer changes
        // nothing (f16 1.0 = 0x3c00).
        let mut color = target(half, COLOR_FORMAT, &[0, 0, 0, 0, 0, 0, 0x00, 0x3c]);
        color.sampler = ImageSampler::linear();
        reduced.color = images.add(color);
        reduced.full = images.add(target(frame, FULL_FORMAT, &[0, 0]));
        reduced.half = images.add(target(half, HALF_FORMAT, &[0, 0, 0, 0]));
        reduced.frame = frame;
    }
    reduced.near = projection.near;
    reduced.far = projection.far;
    let params = compose_params(
        projection.near,
        projection.far,
        (projection.fov * 0.5).tan(),
        frame.as_vec2(),
    );
    if (resized
        || materials
            .get(&material.0)
            .is_some_and(|c| c.compose != params))
        && let Some(mut compose) = materials.get_mut(&material.0)
    {
        if resized {
            compose.color = reduced.color.clone();
            compose.full = reduced.full.clone();
            compose.half = reduced.half.clone();
        }
        compose.compose = params;
    }
    reduced.clouds = clouds
        .single()
        .ok()
        .and_then(|handle| cloud_materials.get(&handle.0))
        .cloned();
    visibility.set_if_neq(if reduced.clouds.is_some() {
        Visibility::Visible
    } else {
        Visibility::Hidden
    });
}

#[derive(Resource)]
struct ReducedPipelines {
    /// Single-sampled and multisampled frames.
    depth_layouts: [BindGroupLayoutDescriptor; 2],
    view_layouts: [BindGroupLayoutDescriptor; 2],
    clouds_layout: BindGroupLayoutDescriptor,
    full: [CachedRenderPipelineId; 2],
    half: [CachedRenderPipelineId; 2],
    clouds: [CachedRenderPipelineId; 2],
}

fn init_pipelines(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    fullscreen_shader: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
) {
    let depth = |multisampled: bool| {
        if multisampled {
            texture_depth_2d_multisampled()
        } else {
            texture_depth_2d()
        }
    };
    let depth_layouts = [false, true].map(|ms| {
        BindGroupLayoutDescriptor::new(
            "reduced_depth",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (uniform_buffer::<Vec4>(false), depth(ms)),
            ),
        )
    });
    let view_layouts = [false, true].map(|ms| {
        BindGroupLayoutDescriptor::new(
            "reduced_view",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::FRAGMENT,
                (
                    uniform_buffer::<ViewUniform>(true),
                    uniform_buffer::<GlobalsUniform>(false),
                    depth(ms),
                ),
            ),
        )
    });
    let clouds_layout = CloudMaterial::bind_group_layout_descriptor(&render_device);
    let defs = |ms: bool| -> Vec<ShaderDefVal> {
        if ms {
            vec!["MULTISAMPLED".into()]
        } else {
            vec![]
        }
    };
    let pipeline = |label: &'static str,
                    layout: Vec<BindGroupLayoutDescriptor>,
                    shader: Handle<Shader>,
                    entry: &'static str,
                    shader_defs: Vec<ShaderDefVal>,
                    format: TextureFormat,
                    blend: Option<BlendState>| {
        pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(label.into()),
            layout,
            vertex: fullscreen_shader.to_vertex_state(),
            fragment: Some(FragmentState {
                shader,
                shader_defs,
                entry_point: Some(entry.into()),
                targets: vec![Some(ColorTargetState {
                    format,
                    blend,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            ..default()
        })
    };
    // The clouds' own blend into the buffer: colour src·α + dst·(1 − α)
    // (premultiplied here), alpha src·0 + dst·(1 − α) (`FUN_03a5adf8` with
    // `mIsDrawReduceBuffer`).
    let clouds_blend = BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent {
            src_factor: BlendFactor::Zero,
            dst_factor: BlendFactor::OneMinusSrcAlpha,
            operation: BlendOperation::Add,
        },
    };
    let full = [false, true].map(|ms| {
        pipeline(
            "reduced_nld_full",
            vec![depth_layouts[ms as usize].clone()],
            DEPTH_SHADER,
            "full",
            defs(ms),
            FULL_FORMAT,
            None,
        )
    });
    let half = [false, true].map(|ms| {
        pipeline(
            "reduced_nld_half",
            vec![depth_layouts[ms as usize].clone()],
            DEPTH_SHADER,
            "half",
            defs(ms),
            HALF_FORMAT,
            None,
        )
    });
    let clouds = [false, true].map(|ms| {
        let mut shader_defs = defs(ms);
        shader_defs.push("REDUCED_BUFFER".into());
        shader_defs.push(ShaderDefVal::UInt("MATERIAL_BIND_GROUP".into(), 1));
        pipeline(
            "reduced_clouds",
            vec![view_layouts[ms as usize].clone(), clouds_layout.clone()],
            super::SHADER,
            "reduced",
            shader_defs,
            COLOR_FORMAT,
            Some(clouds_blend),
        )
    });
    commands.insert_resource(ReducedPipelines {
        depth_layouts,
        view_layouts,
        clouds_layout,
        full,
        half,
        clouds,
    });
}

/// This frame's clouds' bindings and the planes.
#[derive(Resource)]
struct ReducedBuffers {
    clouds: BindGroup,
    planes: UniformBuffer<Vec4>,
}

fn prepare_buffers(
    mut commands: Commands,
    frame: Option<Res<ReducedFrame>>,
    pipelines: Res<ReducedPipelines>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    pipeline_cache: Res<PipelineCache>,
    param: StaticSystemParam<<CloudMaterial as AsBindGroup>::Param>,
) {
    let Some(material) = frame.as_ref().and_then(|f| f.clouds.as_ref()) else {
        commands.remove_resource::<ReducedBuffers>();
        return;
    };
    let mut param: SystemParamItem<<CloudMaterial as AsBindGroup>::Param> = param.into_inner();
    let Ok(prepared) = material.as_bind_group(
        &pipelines.clouds_layout,
        &render_device,
        &pipeline_cache,
        &mut param,
    ) else {
        return;
    };
    let frame = frame.as_deref().expect("checked above");
    let mut planes = UniformBuffer::from(Vec4::new(frame.near, frame.far, 0.0, 0.0));
    planes.write_buffer(&render_device, &render_queue);
    commands.insert_resource(ReducedBuffers {
        clouds: prepared.bind_group,
        planes,
    });
}

/// One fullscreen draw into `target`, cleared to `clear`.
fn draw(
    encoder: &mut CommandEncoder,
    label: &'static str,
    target: &TextureView,
    clear: LinearRgba,
    pipeline: &RenderPipeline,
    groups: &[(&BindGroup, &[u32])],
) {
    let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Clear(clear.into()),
                store: StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_pipeline(pipeline);
    for (index, (group, offsets)) in groups.iter().enumerate() {
        pass.set_bind_group(index as u32, *group, offsets);
    }
    pass.draw(0..3, 0..1);
}

#[allow(clippy::too_many_arguments)]
fn draw_reduced_clouds(
    view: ViewQuery<(&ViewDepthTexture, &ViewUniformOffset, &Msaa), With<ReducedClouds>>,
    frame: Option<Res<ReducedFrame>>,
    buffers: Option<Res<ReducedBuffers>>,
    images: Res<RenderAssets<GpuImage>>,
    pipelines: Res<ReducedPipelines>,
    pipeline_cache: Res<PipelineCache>,
    view_uniforms: Res<ViewUniforms>,
    globals: Res<GlobalsBuffer>,
    mut ctx: RenderContext,
) {
    let (depth, view_offset, msaa) = view.into_inner();
    let (Some(frame), Some(buffers)) = (frame, buffers) else {
        return;
    };
    let (Some(color), Some(full), Some(half)) = (
        images.get(&frame.color),
        images.get(&frame.full),
        images.get(&frame.half),
    ) else {
        return;
    };
    let ms = usize::from(msaa.samples() > 1);
    let get = |id| pipeline_cache.get_render_pipeline(id);
    let (Some(full_pipeline), Some(half_pipeline), Some(clouds_pipeline)) = (
        get(pipelines.full[ms]),
        get(pipelines.half[ms]),
        get(pipelines.clouds[ms]),
    ) else {
        return;
    };
    let (Some(view_binding), Some(globals_binding), Some(planes)) = (
        view_uniforms.uniforms.binding(),
        globals.buffer.binding(),
        buffers.planes.binding(),
    ) else {
        return;
    };
    let device = ctx.render_device().clone();
    let depth_group = device.create_bind_group(
        "reduced_depth",
        &pipeline_cache.get_bind_group_layout(&pipelines.depth_layouts[ms]),
        &BindGroupEntries::sequential((planes, depth.view())),
    );
    let view_group = device.create_bind_group(
        "reduced_view",
        &pipeline_cache.get_bind_group_layout(&pipelines.view_layouts[ms]),
        &BindGroupEntries::sequential((view_binding, globals_binding, depth.view())),
    );
    let encoder = ctx.command_encoder();
    encoder.push_debug_group("reduced_clouds");
    let none = LinearRgba::NONE;
    draw(
        encoder,
        "reduced_nld_full",
        &full.texture_view,
        none,
        full_pipeline,
        &[(&depth_group, &[])],
    );
    draw(
        encoder,
        "reduced_nld_half",
        &half.texture_view,
        none,
        half_pipeline,
        &[(&depth_group, &[])],
    );
    draw(
        encoder,
        "reduced_clouds",
        &color.texture_view,
        CLEAR,
        clouds_pipeline,
        &[(&view_group, &[view_offset.offset]), (&buffers.clouds, &[])],
    );
    encoder.pop_debug_group();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buffer_is_half_the_frame_cut_to_texels() {
        assert_eq!(reduced_size(UVec2::new(1280, 720)), UVec2::new(640, 360));
        assert_eq!(reduced_size(UVec2::new(1281, 719)), UVec2::new(640, 359));
        assert_eq!(reduced_size(UVec2::new(1, 1)), UVec2::ONE);
    }

    #[test]
    fn the_edge_adjustment_does_not_depend_on_the_planes() {
        // k·|ΔNLD| = coeff·|Δz| / (tan(fovy/2)·z): the same for any n, f.
        let shift = |near: f32, far: f32| {
            let c = compose_params(near, far, 0.5, Vec2::new(1280.0, 720.0));
            let nld = |z: f32| (z - near) / (far - near);
            let (z, dz) = (100.0, 10.0);
            let k = c.param1.w / (nld(z) + c.param0.x);
            k * (nld(z + dz) - nld(z))
        };
        let expected = EDGE_ADJUST_COEFF * 10.0 / (0.5 * 100.0);
        assert!((shift(0.3, 30_000.0) - expected).abs() < 1e-3);
        assert!((shift(1.0, 10_000.0) - expected).abs() < 1e-3);
    }

    #[test]
    fn the_context_is_the_games_layout() {
        let c = compose_params(1.0, 101.0, 1.0, Vec2::new(200.0, 100.0));
        assert_eq!(
            c.param0,
            Vec4::new(0.01, 1.0 - 1.0 / 101.0, 100.0 / 101.0, 100.0)
        );
        assert_eq!(c.param1, Vec4::new(100.0, 1.0, 101.0, EDGE_ADJUST_COEFF));
        assert_eq!(c.param2, Vec4::new(100.0, 50.0, 0.01, 0.02));
    }
}
