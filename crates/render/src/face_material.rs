//! Layered UMii face material, using the recovered native colour combiner
//! and the ordinary character lighting. UVs are prepared by models.rs.
use crate::character_material::CharacterMaterial;
use crate::clouds::{CloudShadows, update_clouds};
use crate::models::{ATTRIBUTE_UV_2, ATTRIBUTE_UV_3};
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const IO: Handle<Shader> = uuid_handle!("d3950827-4f0e-4b89-b4ce-1c1b46fad101");
const VERTEX: Handle<Shader> = uuid_handle!("d3950827-4f0e-4b89-b4ce-1c1b46fad102");
const FRAGMENT: Handle<Shader> = uuid_handle!("d3950827-4f0e-4b89-b4ce-1c1b46fad103");
pub type FaceMaterial = ExtendedMaterial<CharacterMaterial, FaceShading>;
pub struct FaceMaterialPlugin;
impl Plugin for FaceMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, IO, "face_io.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, VERTEX, "face_vertex.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, FRAGMENT, "face_material.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<FaceMaterial>::default())
            .add_systems(PostUpdate, share_clouds.after(update_clouds));
    }
}
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
#[data(200, FaceParams, binding_array(214))]
#[bindless(index_table(range(200..213), binding(213)))]
pub struct FaceShading {
    pub params: FaceParams,
    #[texture(201)]
    #[sampler(202)]
    pub layer0: Handle<Image>,
    #[texture(203)]
    #[sampler(204)]
    pub layer1: Handle<Image>,
    #[texture(205)]
    #[sampler(206)]
    pub layer2: Handle<Image>,
    #[texture(207)]
    #[sampler(208)]
    pub layer3: Handle<Image>,
    #[texture(209)]
    #[sampler(210)]
    pub layer4: Handle<Image>,
    #[texture(211)]
    #[sampler(212)]
    pub layer5: Handle<Image>,
}
#[derive(ShaderType, Clone, Debug)]
pub struct FaceParams {
    /// 0: face; 1: lips (uking_mat program 8523).
    pub composition: u32,
    /// const_color0, 1, 3, 4.
    pub colors: [Vec4; 4],
    pub swizzles: [Vec4; 6],
    /// Sampler LOD biases 0..5, then zprepass alpha and alpha-test reference.
    pub bias: [Vec4; 2],
}
impl From<&FaceShading> for FaceParams {
    fn from(value: &FaceShading) -> Self {
        value.params.clone()
    }
}
impl FaceShading {
    pub fn new(params: FaceParams, layers: [Handle<Image>; 6]) -> Self {
        let [layer0, layer1, layer2, layer3, layer4, layer5] = layers;
        Self {
            params,
            layer0,
            layer1,
            layer2,
            layer3,
            layer4,
            layer5,
        }
    }
}
impl MaterialExtension for FaceShading {
    fn vertex_shader() -> ShaderRef {
        VERTEX.into()
    }
    fn fragment_shader() -> ShaderRef {
        FRAGMENT.into()
    }
    fn prepass_vertex_shader() -> ShaderRef {
        VERTEX.into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        FRAGMENT.into()
    }
    fn deferred_vertex_shader() -> ShaderRef {
        VERTEX.into()
    }
    fn deferred_fragment_shader() -> ShaderRef {
        FRAGMENT.into()
    }
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let extra = layout.0.get_layout(&[
            ATTRIBUTE_UV_2.at_shader_location(9),
            ATTRIBUTE_UV_3.at_shader_location(10),
        ])?;
        descriptor.vertex.buffers[0]
            .attributes
            .extend(extra.attributes);
        Ok(())
    }
}
fn share_clouds(
    shadows: Option<Res<CloudShadows>>,
    mut shared: Local<u32>,
    mut materials: ResMut<Assets<FaceMaterial>>,
) {
    let Some(shadows) = shadows else { return };
    if *shared == shadows.revision {
        return;
    }
    *shared = shadows.revision;
    for (_, material) in materials.iter_mut() {
        let params = &mut material.base.extension.params.clouds;
        if params.shadow.y > 0.0 {
            params.upper = shadows.params.upper.clone();
            params.lower = shadows.params.lower.clone();
            crate::clouds::share_shadow(params, &shadows.params);
        }
    }
}
