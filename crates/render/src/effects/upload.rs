//! Per-frame particle data written straight into its GPU buffers: the
//! buffers are made once per draw entity (their assets never change, so
//! the materials binding them stay prepared) and this frame's bytes go to
//! the GPU through the render queue.

use std::sync::Arc;

use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::renderer::RenderQueue;
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::{Render, RenderApp, RenderSystems};

/// This frame's writes: buffer, bytes from its start.
#[derive(Resource, Clone, Default, ExtractResource)]
pub struct EffectUploads(pub Vec<(AssetId<ShaderBuffer>, Arc<[u8]>)>);

pub fn register(app: &mut App) {
    app.init_resource::<EffectUploads>()
        .add_plugins(ExtractResourcePlugin::<EffectUploads>::default());
    if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
        render_app.add_systems(
            Render,
            write_uploads.in_set(RenderSystems::PrepareResources),
        );
    }
}

fn write_uploads(
    uploads: Res<EffectUploads>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    queue: Res<RenderQueue>,
) {
    for (id, bytes) in &uploads.0 {
        if let Some(gpu) = buffers.get(*id)
            && bytes.len() as u64 <= gpu.buffer.size()
        {
            queue.write_buffer(&gpu.buffer, 0, bytes);
        }
    }
}
