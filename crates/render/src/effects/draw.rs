//! Drawing an emitter's particles: one mesh entity per running emitter,
//! drawn by `particles.wgsl` with the emitter's static block, its live
//! particles' attributes and its dynamic block, blended the way the
//! library's render state says (`eft_SetRenderState` `0x03b5f5ac`,
//! `eft_SetBlendType` `0x03b5f420`; docs/research/eft-shaders.md §9).

use asset_format::effects as fx;
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction, Face,
    RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::render::storage::ShaderBuffer;
use bevy::shader::ShaderRef;

use super::gpu::{EmitterDynamic, ParticleAttr};

/// Bytes of the dynamic buffer: the block and the particle count.
pub const DYNAMIC_BYTES: usize =
    std::mem::size_of::<EmitterDynamic>() + 16 + std::mem::size_of::<Extra>();

/// What follows the dynamic block and the counts in the dynamic buffer:
/// the area loop's box (the plugin block, docs/research/eft-custom-blocks.md
/// §2.4, as column matrices) and BotW's custom parameters (the reserved
/// block, `CSDP`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Extra {
    /// The box: local → world, and its inverse.
    pub area: Mat4,
    pub area_inverse: Mat4,
    /// Offset between copies (xyz).
    pub step: Vec4,
    /// Edge fade widths (xyz), in box fractions.
    pub fade: Vec4,
    /// Half-size (xyz), cut mode (w).
    pub half_size: Vec4,
    /// Cut height (x).
    pub cut: Vec4,
    /// `sysCustomShaderReservedUniformBlockParam` (32 floats).
    pub reserved: [Vec4; 8],
}

/// The dynamic buffer's bytes: the block, the counts (live particles,
/// particles per copy, copies, BotW's custom switches F1), the extras.
pub fn dynamic_bytes(dynamic: &EmitterDynamic, counts: [u32; 4], extra: &Extra) -> Vec<u8> {
    let mut bytes = bytemuck::bytes_of(dynamic).to_vec();
    bytes.extend_from_slice(bytemuck::cast_slice(&counts));
    bytes.extend_from_slice(bytemuck::bytes_of(extra));
    bytes
}

pub const SHADER: Handle<Shader> = uuid_handle!("8c1d3f7a-52e4-4b9c-a6d0-7e2f19c4b385");

pub fn register(app: &mut App) {
    load_internal_asset!(app, SHADER, "particles.wgsl", Shader::from_wgsl);
    app.add_plugins(MaterialPlugin::<ParticleMaterial>::default());
}

/// The emitter's bytes beyond the static block that pick code paths
/// (`DrawParams` in `particles.wgsl`).
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct DrawParams {
    /// Calc type, follow type, billboard, rotation order.
    pub kind: UVec4,
    /// Colour sources: colour0, colour1, alpha0, alpha1.
    pub sources: UVec4,
    /// Combiner bytes, four to a vector; `[4].w` the alpha test's
    /// reference as bits.
    pub combiner: [UVec4; 5],
    /// Slots present (bits of x), distance-size mode (y), squared (bits of w).
    pub textures: UVec4,
    /// Vertices per particle, custom shader, alpha test on.
    pub shape: UVec4,
    /// Each slot's component selection, a byte per output channel (R in
    /// the low byte): 0–3 a channel, 4 zero, 5 one.
    pub swizzles: UVec4,
}

impl DrawParams {
    pub fn new(p: &fx::EmitterParams, textures: [bool; 3], vertices: u32) -> Self {
        let mut combiner = [UVec4::ZERO; 5];
        for (i, b) in p.combiner.iter().enumerate() {
            combiner[i / 4][i % 4] = u32::from(*b);
        }
        combiner[4].w = p.alpha_ref.to_bits();
        let bits = |b: [bool; 3]| {
            b.iter()
                .enumerate()
                .fold(0, |m, (i, on)| m | (u32::from(*on) << i))
        };
        Self {
            kind: UVec4::new(
                p.calc.into(),
                p.follow.into(),
                p.billboard.into(),
                p.rotation_order.into(),
            ),
            sources: UVec4::new(
                p.color_sources[0].into(),
                p.color_sources[1].into(),
                p.color_sources[2].into(),
                p.color_sources[3].into(),
            ),
            combiner,
            textures: UVec4::new(bits(textures), 0, 0, bits(p.texture_squared)),
            shape: UVec4::new(vertices, p.custom_shader, u32::from(p.alpha_test), 0),
            swizzles: UVec4::new(0x0302_0100, 0x0302_0100, 0x0302_0100, 0),
        }
    }
}

/// How the pipeline blends and tests depth (the emitter's render state).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ParticleKey {
    pub blend: bool,
    pub blend_type: u8,
    pub depth_test: bool,
    pub depth_write: bool,
    pub cull: u8,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
#[bind_group_data(ParticleKey)]
pub struct ParticleMaterial {
    /// `sysEmitterStaticUniformBlock`.
    #[storage(0, read_only)]
    pub statics: Handle<ShaderBuffer>,
    /// The live particles' attributes (`ParticleAttr`).
    #[storage(1, read_only)]
    pub particles: Handle<ShaderBuffer>,
    /// `sysEmitterDynamicUniformBlock` and the live particle count
    /// (`EmitterDynamic`, then a `u32` × 4), written every frame.
    #[storage(2, read_only)]
    pub dynamic: Handle<ShaderBuffer>,
    #[uniform(3)]
    pub draw: DrawParams,
    #[texture(4)]
    #[sampler(5)]
    pub tex0: Handle<Image>,
    #[texture(6)]
    #[sampler(7)]
    pub tex1: Handle<Image>,
    #[texture(8)]
    #[sampler(9)]
    pub tex2: Handle<Image>,
    /// The shared look values (`look::LookTexture`).
    #[texture(10)]
    pub look: Handle<Image>,
    /// The environment's mean brightness (`deferred_light::CubeMean`).
    #[texture(11, sample_type = "float", filterable = false)]
    pub means: Handle<Image>,
    pub key: ParticleKey,
}

impl From<&ParticleMaterial> for ParticleKey {
    fn from(material: &ParticleMaterial) -> Self {
        material.key
    }
}

impl Material for ParticleMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
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
        layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let vertex = layout.0.get_layout(&[
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
            Mesh::ATTRIBUTE_COLOR.at_shader_location(3),
            Mesh::ATTRIBUTE_UV_1.at_shader_location(4),
        ])?;
        descriptor.vertex.buffers = vec![vertex];
        let k = key.bind_group_data;
        descriptor.primitive.cull_mode = match k.cull {
            1 => Some(Face::Back),
            2 => Some(Face::Front),
            _ => None,
        };
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(k.depth_write);
            // Bevy's depth is reversed: the game's "less or equal" is
            // "greater or equal" here.
            depth.depth_compare = Some(if k.depth_test {
                CompareFunction::GreaterEqual
            } else {
                CompareFunction::Always
            });
        }
        if let Some(fragment) = descriptor.fragment.as_mut()
            && let Some(Some(target)) = fragment.targets.first_mut()
        {
            target.blend = k.blend.then(|| blend_state(k.blend_type));
        }
        Ok(())
    }
}

/// `eft_SetBlendType`: colour and alpha use the same factors (alpha of
/// multiply: inverse destination colour, one).
fn blend_state(blend_type: u8) -> BlendState {
    let c = |src, dst, operation| BlendComponent {
        src_factor: src,
        dst_factor: dst,
        operation,
    };
    let same = |b: BlendComponent| BlendState { color: b, alpha: b };
    match blend_type {
        1 => same(c(
            BlendFactor::SrcAlpha,
            BlendFactor::One,
            BlendOperation::Add,
        )),
        2 => same(c(
            BlendFactor::SrcAlpha,
            BlendFactor::One,
            BlendOperation::ReverseSubtract,
        )),
        3 => BlendState {
            color: c(BlendFactor::Zero, BlendFactor::Src, BlendOperation::Add),
            alpha: c(
                BlendFactor::OneMinusDst,
                BlendFactor::One,
                BlendOperation::Add,
            ),
        },
        4 => same(c(
            BlendFactor::OneMinusDst,
            BlendFactor::One,
            BlendOperation::Add,
        )),
        _ => same(c(
            BlendFactor::SrcAlpha,
            BlendFactor::OneMinusSrcAlpha,
            BlendOperation::Add,
        )),
    }
}

impl ParticleKey {
    pub fn new(p: &fx::EmitterParams) -> Self {
        Self {
            blend: p.blend,
            blend_type: p.blend_type,
            depth_test: p.depth_test,
            depth_write: p.depth_write,
            cull: p.cull,
        }
    }
}

/// The library's default particle quad (`0x03b6aca8`): corners
/// (−½, ½), (−½, −½), (½, −½), (½, ½), drawn as a quad.
fn quad() -> fx::Primitive {
    fx::Primitive {
        id: 0,
        positions: vec![
            [-0.5, 0.5, 0.0],
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
        ],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        tangents: Vec::new(),
        colors: vec![[1.0; 4]; 4],
        uvs: vec![[0.0, 0.0], [0.0, 1.0], [1.0, 1.0], [1.0, 0.0]],
        indices: vec![0, 1, 2, 0, 2, 3],
    }
}

/// `count` copies of the particle's shape; each vertex carries its copy
/// (the particle) and its vertex in the shape (`UV_1`).
pub fn particles_mesh(shape: Option<&fx::Primitive>, count: usize) -> (Mesh, u32) {
    // (`count` covers every copy of an area-looped emitter.)
    let quad = quad();
    let shape = shape.filter(|p| !p.positions.is_empty()).unwrap_or(&quad);
    let n = shape.positions.len();
    let attr = |i: usize, v: &Vec<[f32; 3]>, d: [f32; 3]| v.get(i).copied().unwrap_or(d);
    let mut positions = Vec::with_capacity(n * count);
    let mut normals = Vec::with_capacity(n * count);
    let mut uvs = Vec::with_capacity(n * count);
    let mut colors = Vec::with_capacity(n * count);
    // The particle each vertex belongs to (Bevy shares vertex buffers
    // between meshes, so `vertex_index` does not start at 0).
    let mut owners = Vec::with_capacity(n * count);
    let mut indices = Vec::with_capacity(shape.indices.len() * count);
    for copy in 0..count {
        for v in 0..n {
            positions.push(shape.positions[v]);
            normals.push(attr(v, &shape.normals, [0.0, 0.0, 1.0]));
            uvs.push(shape.uvs.get(v).copied().unwrap_or([0.0, 0.0]));
            colors.push(shape.colors.get(v).copied().unwrap_or([1.0; 4]));
            owners.push([copy as f32, v as f32]);
        }
        indices.extend(shape.indices.iter().map(|i| (copy * n) as u32 + i));
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, owners);
    mesh.insert_indices(Indices::U32(indices));
    (mesh, n as u32)
}

/// Bytes of the particles for the storage buffer (at least one entry).
pub fn particle_bytes(particles: &[ParticleAttr]) -> Vec<u8> {
    if particles.is_empty() {
        return bytemuck::bytes_of(&ParticleAttr::default()).to_vec();
    }
    bytemuck::cast_slice(particles).to_vec()
}

/// A texture with the emitter's sampler: wrap (0 mirror, 1 repeat,
/// 2 clamp, 3 mirror once) and filter (0 linear, else point).
pub fn sampler(s: &fx::Sampler) -> ImageSampler {
    let wrap = |w: u8| match w {
        0 => ImageAddressMode::MirrorRepeat,
        1 => ImageAddressMode::Repeat,
        // SI-EFX-24: mirror once taken as clamp.
        _ => ImageAddressMode::ClampToEdge,
    };
    let filter = if s.filter == 0 {
        ImageFilterMode::Linear
    } else {
        ImageFilterMode::Nearest
    };
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: wrap(s.wrap[0]),
        address_mode_v: wrap(s.wrap[1]),
        mag_filter: filter,
        min_filter: filter,
        mipmap_filter: ImageFilterMode::Linear,
        lod_max_clamp: s.max_lod.max(0.0),
        ..default()
    })
}
