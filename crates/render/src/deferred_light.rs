//! The game's deferred shading shared by the materials that light
//! themselves (`botw::deferred_light` in `deferred_light.wgsl`): the
//! characters' passes and the field's (`field_hybrid` with the terrain's
//! pre-shading and ambient occlusion, docs/research/wiiu-field-shading.md).
//!
//! Also the ambient occlusion's rotations, [`SsaoNoise`]: the game's `ssao`
//! texture (`SystemModel.Tex2`, baked to `light/ssao_noise.ron`), else a
//! stand-in of sixteen evenly spread turns; and the environment's mean
//! brightness, [`CubeMean`], taken once a frame instead of in every pixel.

use asset_format::light::RgImage as RgTexture;
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::core_pipeline::FullscreenShader;
use bevy::image::ImageSampler;
use bevy::light::EnvironmentMapLight;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{sampler, texture_cube, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FragmentState,
    LoadOp, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, SamplerBindingType, ShaderStages, StoreOp, TextureDimension,
    TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice};
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems};

use crate::camera::MainCamera;

const SHADER: Handle<Shader> = uuid_handle!("4b7e0c2d-93a1-4f58-b6d2-8e1f3a9c5d70");
/// `botw::normal_map`: the models' normal from their normal maps.
const NORMAL_MAP_SHADER: Handle<Shader> = uuid_handle!("9d3f6a1e-2c84-4b7a-a5e0-71c2d8f4b936");
/// `cube_mean.wgsl`: the environment's mean brightness.
const CUBE_MEAN_SHADER: Handle<Shader> = uuid_handle!("c5a2e7d1-6b38-4f90-8e14-3d7b9a0f2c56");
/// The texel [`CubeMean`] is drawn into.
const CUBE_MEAN_FORMAT: TextureFormat = TextureFormat::R32Float;

pub struct DeferredLightPlugin {
    /// The `assets/` folder; without the baked noise, the stand-in rotations.
    pub assets: std::path::PathBuf,
}

impl Plugin for DeferredLightPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "deferred_light.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, NORMAL_MAP_SHADER, "normal_map.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, CUBE_MEAN_SHADER, "cube_mean.wgsl", Shader::from_wgsl);
        let path = self.assets.join(asset_format::paths::SSAO_NOISE);
        let noise = match asset_format::read_ron::<RgTexture>(&path) {
            Ok(noise) => {
                info!(
                    "ambient occlusion noise: the game's ssao, {}x{}",
                    noise.width, noise.height
                );
                noise
            }
            Err(error) => {
                warn!("ambient occlusion noise: {error}; stand-in rotations");
                stand_in_noise()
            }
        };
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let image = images.add(noise_image(&noise));
        let mean = images.add(Image::new_target_texture(1, 1, CUBE_MEAN_FORMAT, None));
        app.insert_resource(SsaoNoise(image))
            .insert_resource(CubeMean(mean))
            .add_plugins(ExtractResourcePlugin::<CubeMean>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<CubeMeanSource>()
            .add_systems(RenderStartup, init_cube_mean_pipeline)
            .add_systems(ExtractSchedule, extract_cube_mean_source)
            .add_systems(
                Render,
                draw_cube_mean
                    .after(bevy::pbr::generate::filtering_system)
                    .before(RenderSystems::Render),
            );
    }
}

/// The environment's mean brightness (`cube_mean()` in
/// `deferred_light.wgsl`): one texel the render world draws once a frame
/// from the view's environment map (`cube_mean.wgsl`), before the light's
/// strength and the exposure; every field material binds it. The value is
/// the same for every pixel of a frame, so its 14 samples are taken once.
#[derive(Resource, Clone, ExtractResource)]
pub struct CubeMean(pub Handle<Image>);

/// The view's environment map (its specular levels) and the turn the
/// shaders read it with (`light_probes.view_rotation`).
#[derive(Resource, Default)]
struct CubeMeanSource(Option<(AssetId<Image>, Quat)>);

#[derive(Resource)]
struct CubeMeanPipeline {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedRenderPipelineId,
}

fn init_cube_mean_pipeline(
    mut commands: Commands,
    fullscreen_shader: Res<FullscreenShader>,
    pipeline_cache: Res<PipelineCache>,
) {
    let layout = BindGroupLayoutDescriptor::new(
        "cube_mean",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_cube(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<Vec4>(false),
            ),
        ),
    );
    let pipeline = pipeline_cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("cube_mean".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen_shader.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: CUBE_MEAN_SHADER,
            entry_point: Some("mean".into()),
            targets: vec![Some(ColorTargetState {
                format: CUBE_MEAN_FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        ..default()
    });
    commands.insert_resource(CubeMeanPipeline { layout, pipeline });
}

fn extract_cube_mean_source(
    views: Extract<Query<&EnvironmentMapLight, MainCamera>>,
    mut source: ResMut<CubeMeanSource>,
) {
    source.0 = views
        .single()
        .ok()
        .map(|light| (light.specular_map.id(), light.rotation.inverse()));
}

/// Draws [`CubeMean`]'s texel, after Bevy has filtered the environment map
/// this frame (when it does) and before the views are drawn.
fn draw_cube_mean(
    mean: Option<Res<CubeMean>>,
    source: Res<CubeMeanSource>,
    pipeline: Option<Res<CubeMeanPipeline>>,
    pipeline_cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    render_device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (Some(mean), Some((environment, rotation)), Some(pipeline)) = (mean, source.0, pipeline)
    else {
        return;
    };
    let (Some(target), Some(environment), Some(render_pipeline)) = (
        images.get(&mean.0),
        images.get(environment),
        pipeline_cache.get_render_pipeline(pipeline.pipeline),
    ) else {
        return;
    };
    let rotation: [f32; 4] = rotation.into();
    let contents: Vec<u8> = rotation.iter().flat_map(|v| v.to_ne_bytes()).collect();
    let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("cube_mean_rotation"),
        contents: &contents,
        usage: BufferUsages::UNIFORM,
    });
    let bind_group = render_device.create_bind_group(
        "cube_mean",
        &pipeline_cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            &environment.texture_view,
            &environment.sampler,
            buffer.as_entire_binding(),
        )),
    );
    let mut pass = ctx
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("cube_mean"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &target.texture_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(Default::default()),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(render_pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// The rotations the field's ambient occlusion turns its samples by
/// (`ssao_rotation` in `deferred_light.wgsl`); every field material binds
/// it.
#[derive(Resource, Clone)]
pub struct SsaoNoise(pub Handle<Image>);

/// Sixteen turns, a sixteenth of a circle apart, spread over 4×4 texels in
/// the order of a Bayer matrix (without the baked noise).
fn stand_in_noise() -> RgTexture {
    const BAYER: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];
    let texels = BAYER
        .iter()
        .map(|&i| {
            let angle = f32::from(i) * std::f32::consts::TAU / 16.0;
            let encode = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
            [encode(angle.cos()), encode(angle.sin())]
        })
        .collect();
    RgTexture {
        width: 4,
        height: 4,
        texels,
    }
}

fn noise_image(noise: &RgTexture) -> Image {
    let data = noise.texels.iter().flatten().copied().collect();
    let mut image = Image::new(
        Extent3d {
            width: noise.width,
            height: noise.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rg8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mean_samples_the_cube_like_the_shading() {
        let line = |shader: &str| {
            shader
                .lines()
                .find(|l| l.starts_with("const CUBE_LOD_FROM_TOP"))
                .map(str::to_owned)
        };
        let shading = line(include_str!("deferred_light.wgsl"));
        assert!(shading.is_some());
        assert_eq!(line(include_str!("cube_mean.wgsl")), shading);
    }

    #[test]
    fn stand_in_turns_are_unit_vectors() {
        let noise = stand_in_noise();
        assert_eq!(noise.texels.len(), 16);
        for [r, g] in noise.texels {
            let (x, y) = (
                f32::from(r) / 255.0 * 2.0 - 1.0,
                f32::from(g) / 255.0 * 2.0 - 1.0,
            );
            assert!(((x * x + y * y).sqrt() - 1.0).abs() < 0.02, "({x}, {y})");
        }
    }
}
