//! Model water and glass (see `model_water.wgsl`): the materials the game
//! draws in its translucent G-buffer pass and lights like the terrain's
//! water (`asset_format::xlu`, `docs/research/model-water-glass.md`). An
//! extension of `StandardMaterial` drawn in the transmissive phase like
//! `water_material`, so that its shader sees the scene behind and its
//! depth. It takes the weather, the cloud shadow and the air from the
//! terrain's water material, in the same steps (SI-WAT-07), and plays the
//! models' texture-SRT animations.

use std::sync::Arc;

use asset_format::texture::{IDENTITY_SWIZZLE, Swizzle};
use asset_format::xlu::{XluKind, XluLook, srt_matrix};
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDimension, TextureFormat,
};
use bevy::shader::{ShaderDefVal, ShaderRef};

use crate::clouds::CloudParams;
use crate::look::LookTexture;
use crate::water_material::{WaterLook, WaterMaterial};

pub type ModelWaterMaterial = ExtendedMaterial<StandardMaterial, ModelWaterExtension>;

const SHADER: Handle<Shader> = uuid_handle!("6b1f0c2e-93d4-4a57-8e21-c5a7d94f3b60");

pub struct ModelWaterPlugin;

impl Plugin for ModelWaterPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "model_water.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<ModelWaterMaterial>::default())
            .init_resource::<ModelWaters>()
            .add_systems(Startup, create_blank)
            .add_systems(Update, play_animations)
            .add_systems(
                PostUpdate,
                follow_terrain_water.after(crate::water_material::follow_clouds),
            );
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct ModelWaterExtension {
    #[uniform(100)]
    pub params: ModelWaterParams,
    /// The shader's samplers `_a0`, `_s0`, `_n0`, `_e0`, `_t0` and the
    /// waterfalls' vertex texture `_v0` (a blank texel where the family
    /// reads none); all sampled with `_a0`'s sampler (repeating, filtered,
    /// as every model texture).
    // SI-MAT-01: alpha test and samplers are not the FMAT ones.
    #[texture(101)]
    #[sampler(102)]
    pub a0: Handle<Image>,
    #[texture(103)]
    pub s0: Handle<Image>,
    #[texture(104)]
    pub n0: Handle<Image>,
    #[texture(105)]
    pub e0: Handle<Image>,
    #[texture(106)]
    pub t0: Handle<Image>,
    /// The shared look values (`look::LookTexture`).
    #[texture(107)]
    pub look: Handle<Image>,
    /// The clouds shading the water, as the terrain water's.
    #[uniform(108)]
    pub clouds: CloudParams,
    #[texture(109)]
    #[sampler(110)]
    pub cloud_shadow_map: Handle<Image>,
    #[texture(111)]
    pub v0: Handle<Image>,
}

/// See `ModelWaterParams` in `model_water.wgsl`.
#[derive(ShaderType, Clone, Debug, PartialEq)]
pub struct ModelWaterParams {
    pub surface: Vec4,
    pub mapping: Vec4,
    pub srt: [Vec4; 8],
    pub color: [Vec4; 6],
    pub value: [Vec4; 2],
    pub indirect: [Vec4; 3],
    pub swizzle: [Vec4; 6],
}

/// The shader's samplers, in `ModelWaterParams::swizzle` order.
pub const SLOTS: [&str; 6] = ["_a0", "_s0", "_n0", "_e0", "_t0", "_v0"];

impl ModelWaterParams {
    /// The parameters of `look` at its animation's `frame`, with the
    /// samplers' component selections.
    pub fn new(look: &XluLook, frame: f32, swizzles: [Swizzle; 6]) -> Self {
        let kind = match look.kind {
            XluKind::None => 0.0,
            XluKind::Water => 1.0,
            XluKind::WaterBlend => 2.0,
            XluKind::Glass => 3.0,
            XluKind::RefractingGlass => 4.0,
            XluKind::Waterfall => 5.0,
            XluKind::WaterfallSquaredAlpha => 6.0,
            XluKind::WaterfallMixed => 7.0,
        };
        let mut params = Self {
            surface: Vec4::new(kind, 0.0, 1.0, 1.0),
            mapping: Vec4::from_array(look.texcoords.map(|t| t.mapping as f32)),
            srt: [Vec4::ZERO; 8],
            color: look.const_color.map(Vec4::from_array),
            value: [
                Vec4::from_slice(&look.const_value[..4]),
                Vec4::from_slice(&look.const_value[4..]),
            ],
            indirect: std::array::from_fn(|i| {
                let [a, b] = [look.indirect_scale[2 * i], look.indirect_scale[2 * i + 1]];
                Vec4::new(a[0], a[1], b[0], b[1])
            }),
            swizzle: swizzles.map(|s| Vec4::from_array(s.map(f32::from))),
        };
        params.animate(look, frame);
        params
    }

    /// The texture coordinate sets' matrices at animation frame `frame`.
    pub fn animate(&mut self, look: &XluLook, frame: f32) {
        for (i, texcoord) in look.texcoords.iter().enumerate() {
            let [m, t] = match usize::try_from(texcoord.srt) {
                Ok(srt) if srt < look.tex_srt.len() => srt_matrix(look.tex_srt_at(srt, frame)),
                _ => srt_matrix([0.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
            };
            self.srt[2 * i] = Vec4::from_array(m);
            self.srt[2 * i + 1] = Vec4::from_array(t);
        }
    }
}

impl MaterialExtension for ModelWaterExtension {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    // Like the terrain's water: no shadows, no depth prepass (it reads it).
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// `specular_transmission > 0` only puts it into the transmissive
    /// phase; the shader refracts on its own (as `WaterExtension`).
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.retain(|def| {
                !matches!(def, ShaderDefVal::Bool(name, _)
                    if name == "STANDARD_MATERIAL_SPECULAR_TRANSMISSION" || name == "STANDARD_MATERIAL_DIFFUSE_OR_SPECULAR_TRANSMISSION")
            });
        }
        Ok(())
    }
}

/// The model water materials: what they share with the terrain's water,
/// and the ones whose texture SRTs play an animation.
#[derive(Resource, Default)]
pub struct ModelWaters {
    /// A blank texel for the samplers a family does not read.
    blank: Handle<Image>,
    all: Vec<Handle<ModelWaterMaterial>>,
    animated: Vec<(Handle<ModelWaterMaterial>, Arc<XluLook>)>,
    /// The terrain water's weather factor, air and clouds, as last handed on.
    shared: Option<Shared>,
}

#[derive(Clone, PartialEq)]
struct Shared {
    glint: f32,
    air: f32,
    clouds: CloudParams,
    shadow_map: Handle<Image>,
}

impl ModelWaters {
    /// A material for `look`, its samplers' images and component
    /// selections in [`SLOTS`] order.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &mut self,
        materials: &mut Assets<ModelWaterMaterial>,
        look: &XluLook,
        slots: [Option<(Handle<Image>, Swizzle)>; 6],
        double_sided: bool,
        look_texture: Option<&LookTexture>,
    ) -> Handle<ModelWaterMaterial> {
        let swizzles = slots
            .each_ref()
            .map(|s| s.as_ref().map_or(IDENTITY_SWIZZLE, |(_, s)| *s));
        let image = |i: usize| {
            slots[i]
                .as_ref()
                .map_or_else(|| self.blank.clone(), |(h, _)| h.clone())
        };
        let mut params = ModelWaterParams::new(look, 0.0, swizzles);
        let shared = self.shared.clone();
        if let Some(shared) = &shared {
            params.surface.z = shared.glint;
            params.surface.w = shared.air;
        }
        // The programs that blend their outputs into the G-buffer by the
        // vertex alpha blend over what is drawn.
        // SI-MWT-02: blended programs blend their lit result over the frame.
        let blends = matches!(
            look.kind,
            XluKind::WaterBlend
                | XluKind::Waterfall
                | XluKind::WaterfallSquaredAlpha
                | XluKind::WaterfallMixed
        );
        let handle = materials.add(ModelWaterMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                alpha_mode: if blends {
                    AlphaMode::Blend
                } else {
                    AlphaMode::Opaque
                },
                // Reads the scene behind: drawn in the transmissive phase
                // (or, blending, the transparent one) with the view's
                // transmission texture (see `specialize`).
                specular_transmission: 1.0,
                double_sided,
                cull_mode: if double_sided {
                    None
                } else {
                    Some(bevy::render::render_resource::Face::Back)
                },
                ..default()
            },
            extension: ModelWaterExtension {
                params,
                a0: image(0),
                s0: image(1),
                n0: image(2),
                e0: image(3),
                t0: image(4),
                v0: image(5),
                look: look_texture.map(|l| l.0.clone()).unwrap_or_default(),
                clouds: shared
                    .as_ref()
                    .map_or_else(CloudParams::without_shadows, |s| s.clouds.clone()),
                cloud_shadow_map: shared.map(|s| s.shadow_map).unwrap_or_default(),
            },
        });
        self.all.push(handle.clone());
        if look.animation.as_ref().is_some_and(|a| {
            look.texcoords
                .iter()
                .any(|t| t.srt >= 0 && a.moves(t.srt as usize))
        }) {
            self.animated.push((handle.clone(), Arc::new(look.clone())));
        }
        handle
    }
}

/// A 1×1 texel, mid grey with full alpha: a flat normal (128/255 unpacks
/// to 0) where a family reads no texture.
fn create_blank(mut waters: ResMut<ModelWaters>, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_fill(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[128, 128, 128, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = crate::texture::sampler();
    waters.blank = images.add(image);
}

/// Steps the texture SRTs of the animated materials (30 animation frames
/// a second, looping; nothing changes while time stands still).
// SI-MWT-03: the self-playing animation is the unit's `_Auto` one.
fn play_animations(
    time: Res<Time>,
    waters: Res<ModelWaters>,
    mut materials: ResMut<Assets<ModelWaterMaterial>>,
) {
    let seconds = time.elapsed_secs();
    for (handle, look) in &waters.animated {
        let Some(animation) = &look.animation else {
            continue;
        };
        let frame = animation.frame_at(seconds);
        let Some(material) = materials.get(handle) else {
            continue;
        };
        let mut params = material.extension.params.clone();
        params.animate(look, frame);
        if params != material.extension.params
            && let Some(mut material) = materials.get_mut(handle)
        {
            material.extension.params = params;
        }
    }
}

/// Hands the terrain water's weather factor, air and clouds on to the
/// model water whenever they change there (in its steps, SI-WAT-07).
fn follow_terrain_water(
    look: Option<Res<WaterLook>>,
    water: Res<Assets<WaterMaterial>>,
    mut waters: ResMut<ModelWaters>,
    mut materials: ResMut<Assets<ModelWaterMaterial>>,
) {
    let Some(water) = look.and_then(|l| water.get(&l.material)) else {
        return;
    };
    let shared = Shared {
        glint: water.extension.params.surface.y,
        air: water.extension.params.surface.z,
        clouds: water.extension.clouds.clone(),
        shadow_map: water.extension.cloud_shadow_map.clone(),
    };
    if waters.shared.as_ref() == Some(&shared) {
        return;
    }
    for handle in &waters.all {
        if let Some(mut material) = materials.get_mut(handle) {
            let extension = &mut material.extension;
            extension.params.surface.z = shared.glint;
            extension.params.surface.w = shared.air;
            extension.clouds = shared.clouds.clone();
            extension.cloud_shadow_map = shared.shadow_map.clone();
        }
    }
    waters.shared = Some(shared);
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_format::xlu::{CurveKind, SrtAnimation, SrtCurve, XluTexCoord};

    #[test]
    fn texcoords_take_their_srts_and_animation() {
        let mut look = XluLook {
            kind: XluKind::Water,
            texcoords: [
                XluTexCoord {
                    mapping: 11,
                    srt: 0,
                },
                XluTexCoord { mapping: 0, srt: 1 },
                XluTexCoord {
                    mapping: 0,
                    srt: -1,
                },
                XluTexCoord { mapping: 0, srt: 2 },
            ],
            const_value: [0.3, 0.99, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0],
            indirect_scale: [
                [0.0, -1.0],
                [0.15, 0.15],
                [0.3, 0.3],
                [0.3, 0.3],
                [-0.1, -0.1],
                [1.0, 1.0],
            ],
            ..Default::default()
        };
        look.tex_srt[0] = [0.0, 0.1, 0.1, 0.0, 0.0, 0.0];
        look.tex_srt[1] = [0.0, 0.2, 0.2, 0.0, 0.0, 0.0];
        look.animation = Some(SrtAnimation {
            frames: 4000.0,
            looping: true,
            curves: vec![SrtCurve {
                srt: 0,
                component: 4,
                kind: CurveKind::Linear,
                end: 4000.0,
                frames: vec![0.0, 4000.0],
                keys: vec![[0.0, 30.0, 0.0, 0.0], [30.0, 0.0, 0.0, 0.0]],
                ..Default::default()
            }],
            ..Default::default()
        });
        let params = ModelWaterParams::new(&look, 2000.0, [IDENTITY_SWIZZLE; 6]);
        assert_eq!(params.surface.x, 1.0);
        assert_eq!(params.mapping, Vec4::new(11.0, 0.0, 0.0, 0.0));
        assert_eq!(params.srt[0], Vec4::new(0.1, 0.0, 0.0, 0.1));
        assert_eq!(params.srt[1], Vec4::new(15.0, 0.0, 0.0, 0.0));
        assert_eq!(params.srt[3], Vec4::ZERO);
        // Set 2 has no SRT: identity.
        assert_eq!(params.srt[4], Vec4::new(1.0, 0.0, 0.0, 1.0));
        assert_eq!(params.indirect[0], Vec4::new(0.0, -1.0, 0.15, 0.15));
        assert_eq!(params.value[0], Vec4::new(0.3, 0.99, 0.0, 0.25));
    }
}
