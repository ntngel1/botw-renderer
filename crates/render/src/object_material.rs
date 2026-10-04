//! The material of the map's placed objects: `StandardMaterial` darkened
//! under cloud shadows, and dissolved into the
//! far-tree billboards where a tree's model hands over to its picture. The
//! game dissolves with `TreeDitherMask` from `Terrain.Tex1` (a 32 × 32 blotchy
//! mask); the model hides the mask's texels below its fade, the billboard
//! shows exactly those, so between them every pixel is drawn once
//! (`hand_off.wgsl`).
//!
//! Everything but leaves is lit by the game's deferred shading of the field
//! (`field_hybrid`, `botw::deferred_light`; the objects' G-buffer in
//! docs/research/wiiu-field-shading.md): it needs the gloss the game keeps
//! in the normal maps' blue, and the objects in the camera's depth prepass
//! (for the ambient occlusion and the rim light), where a model handing
//! over to its billboard dissolves as in the main pass.
//!
//! Tree crowns shade as soft volumes, like the game's: their materials say
//! where the crown's centre is (`uking_modify_normal_type` 1,
//! `const_vector0`, see `models::MaterialLook`), and the shader bends the
//! leaves' normals out from it and weakens their normal maps.
//!
//! Ported from the original renderer as is; the environment's mean brightness
//! (`deferred_light::CubeMean`) is bound as on the terrain (texture 108),
//! and the dissolve mask's ranks come from [`dither_ranks`] here.

use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::image::ImageSampler;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
    MeshPipelineKey,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, FragmentState, RenderPipelineDescriptor, ShaderType,
    SpecializedMeshPipelineError, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;

use crate::clouds::{CloudParams, CloudShadows};
use crate::deferred_light::{CubeMean, SsaoNoise};
use crate::look::LookTexture;
use crate::models::MaterialLook;

const SHADER: Handle<Shader> = uuid_handle!("b7d2e4a1-5c93-4f08-a6e1-3d8f02c9b574");
/// `botw::hand_off`: the dissolve test, shared with the billboards.
const HAND_OFF: Handle<Shader> = uuid_handle!("4a1f9c3e-82d7-4b65-9e0a-c5f3b1d78e26");

pub type ObjectMaterial = ExtendedMaterial<StandardMaterial, ObjectShading>;

pub struct ObjectMaterialPlugin;

impl Plugin for ObjectMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, HAND_OFF, "hand_off.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, SHADER, "object_material.wgsl", Shader::from_wgsl);
        // A stand-in until the game's mask is read (far trees replace it).
        let mask = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(dither_mask_image(&fallback_mask()));
        app.insert_resource(HandOffMask(mask))
            .add_plugins(MaterialPlugin::<ObjectMaterial>::default());
    }
}

/// The dissolve mask models and billboards share.
#[derive(Resource, Clone)]
pub struct HandOffMask(pub Handle<Image>);

/// Bindless like `StandardMaterial`, so objects with different materials
/// still batch.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
#[data(100, ObjectParams, binding_array(110))]
#[bindless(index_table(range(100..109), binding(109)))]
pub struct ObjectShading {
    pub clouds: CloudParams,
    /// The cloud shadow's texture (see `clouds.rs`).
    #[texture(101)]
    #[sampler(102)]
    pub shadow_map: Handle<Image>,
    /// Ranks of the dissolve mask (see [`dither_mask_image`]).
    #[texture(103)]
    pub mask: Handle<Image>,
    /// The shared look values (`look::LookTexture`).
    #[texture(104)]
    pub look: Handle<Image>,
    /// The ambient occlusion's rotations (`deferred_light::SsaoNoise`).
    #[texture(105)]
    pub ssao_noise: Handle<Image>,
    /// The surface's gloss map (see `models::gloss_map_image`); unused
    /// without one ([`ObjectParams::gloss`]).
    #[texture(106)]
    pub gloss_map: Handle<Image>,
    /// Leaves' translucency (`models::MaterialLook::leaf_light`); unused
    /// without it ([`ObjectParams::leaf`] w 0).
    #[texture(107)]
    pub translucency_map: Handle<Image>,
    /// The environment's mean brightness (`deferred_light::CubeMean`):
    /// `R32Float`, filterable where the adapter has `FLOAT32_FILTERABLE`,
    /// which the bindless texture array needs.
    #[texture(108, sample_type = "float", filterable = false)]
    pub cube_mean: Handle<Image>,
    /// See [`ObjectParams::leaf`].
    pub leaf: Vec4,
    /// See [`ObjectParams::crown`].
    pub crown: Vec4,
    /// See [`ObjectParams::gloss`].
    pub gloss: Vec4,
    /// See [`ObjectParams::sheen`].
    pub sheen: Vec4,
}

/// See `object_material.wgsl`.
#[derive(ShaderType, Clone, Debug)]
pub struct ObjectParams {
    pub clouds: CloudParams,
    /// Foliage (the game's `field_leaf`): x 1 for leaves (0 for everything
    /// else), y 1 where light passes through (albedo.a bit 1), z how far the
    /// normals bend out from the crown's centre (0 without a crown), w how
    /// strongly they are lit through from behind (`const_value3`; 0 without
    /// a translucency map).
    pub leaf: Vec4,
    /// The crown's centre in model space (xyz) and its radius (w, m).
    pub crown: Vec4,
    /// Gloss: x 1 where the gloss map holds it, y the gloss without one.
    pub gloss: Vec4,
    /// The leaves' sheen and its fade (`const_value1`, `2`, `4`, `5`): the
    /// fade per metre and its offset, the sheen's exponent and strength.
    pub sheen: Vec4,
}

impl From<&ObjectShading> for ObjectParams {
    fn from(shading: &ObjectShading) -> Self {
        Self {
            clouds: shading.clouds.clone(),
            leaf: shading.leaf,
            crown: shading.crown,
            gloss: shading.gloss,
            sheen: shading.sheen,
        }
    }
}

/// The gloss of a surface without a gloss map: a BC5 normal map's missing
/// blue reads 0, and so do surfaces without a normal map (an assumption for
/// those; the rocks with the terrain's textures take theirs from
/// `MaterialCmb`, not loaded for objects).
// SI-LGT-19: gloss without a map, metal and SSAO range are ours.
const GLOSS_WITHOUT_MAP: f32 = 0.0;

/// How far a crown's leaf normals bend out from its centre (a fit: the game's
/// blend with `const_vector0`, in its vertex shader, is not read).
// SI-LGT-18: crown core light and bend are ours, not VS field_leaf.
const CROWN_BEND: f32 = 0.75;

impl MaterialExtension for ObjectShading {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    /// A model handing over to its billboard dissolves with the game's mask
    /// in the depth prepass and the shadows too (`model_hidden`), or its
    /// depth would hide what shows through the pixels dissolved in the main
    /// pass: Bevy draws it there without a fragment shader (opaque) or with
    /// its own dither (cut out).
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let prepass = descriptor
            .vertex
            .shader_defs
            .contains(&"PREPASS_PIPELINE".into());
        if prepass
            && key
                .mesh_key
                .contains(MeshPipelineKey::VISIBILITY_RANGE_DITHER)
        {
            let fragment = descriptor.fragment.take();
            descriptor.fragment = Some(FragmentState {
                shader: SHADER,
                shader_defs: fragment.as_ref().map_or_else(
                    || descriptor.vertex.shader_defs.clone(),
                    |f| f.shader_defs.clone(),
                ),
                targets: fragment.map(|f| f.targets).unwrap_or_default(),
                ..default()
            });
        }
        Ok(())
    }
}

impl ObjectShading {
    pub fn new(
        clouds: Option<&CloudShadows>,
        mask: &HandOffMask,
        look: Option<&LookTexture>,
        ssao_noise: Option<&SsaoNoise>,
        cube_mean: Option<&CubeMean>,
    ) -> Self {
        Self {
            clouds: clouds.map_or_else(CloudParams::without_shadows, |c| c.params.clone()),
            shadow_map: clouds.map(|c| c.shadow_map.clone()).unwrap_or_default(),
            mask: mask.0.clone(),
            look: look.map(|l| l.0.clone()).unwrap_or_default(),
            ssao_noise: ssao_noise.map(|n| n.0.clone()).unwrap_or_default(),
            gloss_map: Handle::default(),
            translucency_map: Handle::default(),
            cube_mean: cube_mean.map(|m| m.0.clone()).unwrap_or_default(),
            leaf: Vec4::ZERO,
            crown: Vec4::ZERO,
            gloss: Vec4::new(0.0, GLOSS_WITHOUT_MAP, 0.0, 0.0),
            sheen: Vec4::ZERO,
        }
    }

    /// This shading for a part whose material says `look`, with its gloss
    /// and translucency maps if it has them: leaves and tree crowns get
    /// their foliage parameters.
    pub fn for_material(
        &self,
        look: &MaterialLook,
        gloss_map: Option<&Handle<Image>>,
        translucency_map: Option<&Handle<Image>>,
    ) -> Self {
        let mut shading = self.clone();
        if let Some(map) = gloss_map {
            shading.gloss_map = map.clone();
            shading.gloss.x = 1.0;
        }
        if look.leaf {
            let crown = look.crown.filter(|c| c.w > 0.0);
            let lit_through = look.leaf_light.zip(translucency_map);
            if let Some(([v1, v2, v3, v4, v5], map)) = lit_through {
                shading.translucency_map = map.clone();
                shading.sheen = Vec4::new(v1, v2, v4, v5);
                shading.leaf.w = v3;
            }
            shading.leaf.x = 1.0;
            shading.leaf.y = if look.translucent { 1.0 } else { 0.0 };
            shading.leaf.z = if crown.is_some() { CROWN_BEND } else { 0.0 };
            // `const_vector0.w` is 10 in almost every crown, about twice
            // the reach of broadleaf crowns' leaves around their centre:
            // taken as the crown's diameter.
            // SI-LGT-18: crown core light and bend are ours, not VS field_leaf.
            shading.crown = crown.map_or(Vec4::ZERO, |c| c.truncate().extend(c.w * 0.5));
        }
        shading
    }
}

/// Side of the dissolve mask (texels; one per screen pixel).
pub const MASK_SIZE: u32 = 32;

/// A dissolve mask (`MASK_SIZE`² values) as a texture of its ranks.
pub fn dither_mask_image(values: &[u8]) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: MASK_SIZE,
            height: MASK_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        dither_ranks(values),
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// A dissolve mask's values replaced by their ranks, spread evenly over
/// 0–255: hiding the texels below a threshold `t` then hides the share `t`
/// of them, whatever the mask's own histogram (`TreeDitherMask` is mostly
/// dark). Equal values are ordered by a hash of their position, so a flat
/// area does not dissolve row by row. the original format parser's `trees::dither_ranks`.
pub fn dither_ranks(values: &[u8]) -> Vec<u8> {
    let scramble = |i: usize| {
        (i as u32)
            .wrapping_mul(0x9E37_79B9)
            .rotate_left(13)
            .wrapping_mul(0x85EB_CA6B)
    };
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by_key(|&i| (values[i], scramble(i)));
    let mut ranks = vec![0; values.len()];
    for (rank, &i) in order.iter().enumerate() {
        ranks[i] = (rank * 256 / values.len().max(1)) as u8;
    }
    ranks
}

/// White noise, for when there is no game mask (nothing hands over then).
fn fallback_mask() -> Vec<u8> {
    (0..MASK_SIZE * MASK_SIZE)
        .map(|i| {
            (i.wrapping_mul(0x9E37_79B9)
                .rotate_left(7)
                .wrapping_mul(0x85EB_CA6B)
                >> 24) as u8
        })
        .collect()
}
