//! Models: one GLB (glTF 2.0, binary) per BFRES model unit,
//! `models/<folder>/<unit>.glb`, with its textures as KTX2 files next to
//! it, shared by the folder's units (`models/<folder>/<texture>.ktx2`).
//!
//! What the original renderer's `models::load_folder` builds for a static (rigid)
//! model, baked:
//! - a node per BFRES shape, named like it, with a mesh whose primitives
//!   are the shape's levels of detail: the same vertex accessors (POSITION,
//!   NORMAL, TEXCOORD_0, TANGENT; the bind pose applied, tangents generated
//!   where the model has none; COLOR_0 and TEXCOORD_1 where the material
//!   reads the vertex colours and second UV set) and one index list each, tagged with
//!   `extras.lod` (0 the full mesh; only triangle lists whose indices are
//!   in range, as the viewer keeps them). The mesh's `extras.radius` is the
//!   farthest vertex from the model's origin;
//! - a material per BFRES material the shapes use, its `extras` a
//!   [`MaterialInfo`]: the FMAT facts the viewer's `cpu_material` and the
//!   field shading read, and the KTX2 files of its textures (the game's
//!   and the derived ones, see [`MaterialTextures`]).
//!
//! The glTF materials also say alpha mode, cutoff and double-sidedness for
//! other tools; the renderer reads only the extras.
//!
//! Skinned models (characters, `characters/models/…`, what the viewer's
//! `load_folder` makes with `LoadOptions::skinned`) also carry:
//! - their skeleton as a glTF skin: a node per bone after the shapes' nodes
//!   (name, local translation, rotation, scale; children), the skin's joints
//!   in bone order (a vertex's joint index is the BFRES bone index) and
//!   their inverse bind matrices (the inverse of each bone's model-space
//!   bind transform);
//! - per vertex JOINTS_0 (unsigned short) and WEIGHTS_0 (summing to 1), and
//!   TEXCOORD_1 where the material reads maps with the second UV set
//!   ([`MaterialInfo::second_uv`]);
//! - one level of detail per shape (the viewer keeps none for skinned
//!   loads).
//!
//! Static models write none of this, so their files are as before.

use serde::{Deserialize, Serialize};

use crate::glb::{
    ARRAY_BUFFER, Asset, Attributes, Glb, Gltf, GltfMaterial, Mesh, MeshExtras, Node, Primitive,
    PrimitiveExtras, Scene, Skin, TRIANGLES, Writer,
};
use crate::{FormatError, Result};

/// One model unit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Model {
    /// The BFRES model's name (the unit's).
    pub name: String,
    pub materials: Vec<Material>,
    pub shapes: Vec<Shape>,
    /// The bones of a skinned model (empty for static ones).
    pub skeleton: Vec<Bone>,
}

/// A bone of a skinned model in its bind pose.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bone {
    pub name: String,
    /// Index of the parent bone (parents come before their children).
    pub parent: Option<u32>,
    pub translation: [f32; 3],
    /// A quaternion (x, y, z, w).
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// The inverse of the bone's model-space bind transform, column-major.
    pub inverse_bind: [f32; 16],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Material {
    pub name: String,
    pub info: MaterialInfo,
}

/// A BFRES shape in model space.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shape {
    pub name: String,
    /// Index into [`Model::materials`].
    pub material: u32,
    /// Distance of the farthest vertex from the model's origin.
    pub radius: f32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// xyz the tangent, w its handedness (±1).
    pub tangents: Vec<[f32; 4]>,
    /// The vertex colours (`_c0`, RGBA), kept only where the material
    /// reads them (translucent G-buffer materials); empty otherwise.
    pub colors: Vec<[f32; 4]>,
    /// The second UV set (`_u1`; empty: none): kept for skinned loads,
    /// including layered faces, and for translucent G-buffer materials.
    pub uvs1: Vec<[f32; 2]>,
    /// Layered character faces: lash UVs (`_u2`) before their texture SRTs.
    /// Empty when the source mesh has no such attribute.
    pub uvs2: Vec<[f32; 2]>,
    /// Layered character faces: brow UVs (`_u3`) before their texture SRTs.
    pub uvs3: Vec<[f32; 2]>,
    /// Skinned shapes: up to four bones per vertex (indices into
    /// [`Model::skeleton`]) and their weights (empty: a static shape).
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
    /// Finest first; the first is level 0, the full mesh.
    pub lods: Vec<Lod>,
}

/// A level of detail: triangles over the shape's vertices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lod {
    /// The BFRES LOD index.
    pub level: u32,
    pub indices: Vec<u32>,
}

/// The FMAT's render state (0x20) as the viewer reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RenderState {
    /// 0 custom, 1 opaque, 2 alpha mask, 3 translucent.
    pub mode: u32,
    /// Alpha-test reference when alpha testing is on.
    pub alpha_test: Option<f32>,
    /// Back faces culled.
    pub cull_back: bool,
}

/// How a material's pixels are kept (Bevy's `AlphaMode` subset).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Alpha {
    Opaque,
    Mask(f32),
    Blend,
}

/// What a material says beyond its geometry (the original renderer's
/// `models::CpuMaterial` inputs).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialInfo {
    pub render_state: RenderState,
    /// `tex_srt0` when it has its six values: mode, scale x/y, rotation,
    /// translation x/y.
    pub tex_srt0: Option<[f32; 6]>,
    /// The colour without an albedo texture (`Material::albedo_constant`).
    pub albedo_constant: Option<[f32; 3]>,
    /// Malice (`Material::is_malice`): the game supplies its colour.
    pub malice: bool,
    /// `uking_enable_transmission` is 1.
    pub transmission: bool,
    /// `gsys_dynamic_depth_shadow` is not 0.
    pub casts_shadows: bool,
    /// `gsys_cube_map` is on (see the viewer's `models::in_cube_map`).
    pub in_cube_map: bool,
    /// `uking_edit_sky_occlusion` is on (see [`MaterialInfo::covers_sky`]).
    pub edit_sky_occlusion: bool,
    pub look: MaterialLook,
    /// Rocks and cliffs (a `tma` sampler): the layer of the terrain's
    /// albedo array (`terrain/albedo.ktx2`) they take as albedo
    /// (`texture_array_index0`, 0 without it).
    pub terrain_layer: Option<u32>,
    pub textures: MaterialTextures,
    /// What the material emits (`Material::emission`), in the game's units
    /// (zero for none, or when its mask could not be read).
    pub emission: [f32; 3],
    /// What it emits into the environment's cube map, likewise: the game
    /// draws the cube map with the program variation that emits only
    /// `uking_emission_color_cubemap`, and that only with
    /// `uking_enable_emission_cubemap` on (`gsys_assign_material`
    /// variation 2, see `crates/bake/src/models.rs`, `cube_map_emission`).
    pub emission_cube: [f32; 3],
    /// Whether [`Self::emission`] and [`Self::emission_cube`] are scaled by
    /// the game's `uking_dynamic_exposure` (combiner input 507: the sky
    /// palette's `Exposure`, 1 by day, 0 at night, dawn and dusk); the
    /// colours above leave that factor out.
    pub emission_exposure: DynamicExposure,
    pub emission_cube_exposure: DynamicExposure,
    /// The FMAT's samplers and their textures, e.g. `("_a0", "…_Alb")`.
    pub samplers: Vec<(String, String)>,
    /// Skinned loads: which maps the shader reads with the second UV set.
    #[serde(skip_serializing_if = "SecondUv::is_none")]
    pub second_uv: SecondUv,
    /// Original layered UMii face inputs, after sampled colour animations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layered_face: Option<LayeredFace>,
    /// Drawn by the game's translucent G-buffer pass (model water, glass):
    /// what its program reads; `None` for the ordinary materials, and for
    /// the families not ported (see [`crate::xlu`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xlu: Option<crate::xlu::XluLook>,
}

/// Inputs of the layered UMii face shader (see docs/research/umii.md).
/// This records source facts; it does not turn raw SRTs into uploaded matrices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LayeredFace {
    #[serde(default)]
    pub composition: FaceComposition,
    /// Shader slots 0..5: skin, line, AO, lash, brow, makeup. Their order is
    /// different from the material sampler names (`_a0` there means lash).
    /// Lip composition uses slots [0, 1, 3, 3, 4, 5]; AO is duplicated.
    pub layers: [FaceLayer; 6],
    pub const_colors: [[f32; 4]; 5],
    pub tex_srts: [TextureSrt; 6],
    pub zprepass_alpha: f32,
    /// Uploaded kind-30 matrices (six column-major coefficients each).
    /// Absent in older bakes; layered rendering requires a fresh bake.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrices: Option<[[f32; 6]; 6]>,
}

/// Native UMii combiner and vertex coordinate paths.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum FaceComposition {
    #[default]
    Face,
    Lip,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceLayer {
    /// KTX2 file stem beside the model GLB.
    pub texture: String,
    /// Original SQ_TEX_SAMPLER_WORD0..2, including wrap, filtering and LOD.
    pub sampler: [u32; 3],
}

/// Raw BFRES TexSrt (kind 30). The first word is an integer mode, unlike
/// the five floating-point scale/rotation/translation components.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSrt {
    pub mode: u32,
    pub scale: [f32; 2],
    pub rotation: f32,
    pub translation: [f32; 2],
}

impl TextureSrt {
    /// Native kind-30 matrix conversion, Wii U v208 callbacks
    /// 0x03c081b0 / 0x03c082bc / 0x03c083b8. `sin_cos` must come from
    /// the native interpolated trig table, not the host's trigonometry.
    /// Returns column-major (m00, m10, m01, m11, m02, m12).
    pub fn matrix_from_sin_cos(self, (sin, cos): (f32, f32)) -> Option<[f32; 6]> {
        let [sx, sy] = self.scale;
        let [tx, ty] = self.translation;
        let xc = sx * cos;
        let xs = sx * sin;
        let yc = sy * cos;
        let ys = sy * sin;
        Some(match self.mode {
            0 => {
                let a = -0.5 * cos;
                let b = 0.5_f32.mul_add(sin, -0.5);
                [
                    xc,
                    -ys,
                    xs,
                    yc,
                    sx * (a - b - tx),
                    sy.mul_add(a + b + ty, 1.0),
                ]
            }
            1 => [
                xc,
                -ys,
                xs,
                yc,
                xs.mul_add(ty - 0.5, -(xc * (tx + 0.5))) + 0.5,
                ys.mul_add(tx + 0.5, yc * (ty - 0.5)) + 0.5,
            ],
            2 => [
                xc,
                ys,
                -xs,
                yc,
                -xs.mul_add(ty, xc.mul_add(tx, -xs)),
                yc.mul_add(ty, -ys.mul_add(tx, yc)) + 1.0,
            ],
            _ => return None,
        })
    }
}

/// Maps a character material reads with the second UV set (the viewer's
/// `models::SecondUv`; `uking_textureN_texcoord` 1 of the shader slot the
/// sampler is assigned to): faces lay their normal and specular maps out
/// on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SecondUv {
    pub normal: bool,
    pub specular: bool,
    /// The characters' extra maps ([`MaterialTextures::character_maps`]).
    pub maps: bool,
}

impl SecondUv {
    pub fn any(&self) -> bool {
        self.normal || self.specular || self.maps
    }

    fn is_none(&self) -> bool {
        !self.any()
    }
}

/// How a colour depends on `uking_dynamic_exposure` (`e`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DynamicExposure {
    /// Not at all.
    #[default]
    None,
    /// Times `e` (input 507, channel 0).
    Exposure,
    /// Times `1 − e` (input 507, channel 1): lit at night.
    OneMinus,
}

impl DynamicExposure {
    /// The factor at exposure `e`.
    pub fn factor(self, e: f32) -> f32 {
        match self {
            Self::None => 1.0,
            Self::Exposure => e,
            Self::OneMinus => 1.0 - e,
        }
    }
}

/// The KTX2 files (names in the model's folder) of a material's maps, as
/// the viewer makes them at load time:
/// - `albedo`: the game's `_a0` (`a0`), or `<albedo>+<mask>` (RGBA8 with
///   the `_ms0` mask as alpha, every level both share);
/// - `normal`: `<name>.nrm`, a BC1 normal map re-encoded as BC5 (a BC5
///   map is used as it is, under its own name);
/// - `gloss`: `<name>.gloss`, a BC1 normal map's blue as BC4
///   (`uking_grossy_color` 402);
/// - `translucency`: the game's texture of shader slot 2 (leaves lit from
///   behind);
/// - `metal_roughness`: `mr_…`, RGBA8 (roughness green, metal blue) with
///   its mip chain, from the specular and metal masks;
/// - `emissive`, `emissive_cube`: `em_…`, RGBA8 with its mip chain;
/// - `character_maps` (skinned loads): `cm_…`, the viewer's
///   `character_maps`: RGBA8 with its mip chain, red the ambient occlusion
///   (`_ao0`), green the eyes' shadow (`_sd0`), blue the skin's
///   transmission (the alpha of `_ao0`), white where a map is missing.
///   Skinned loads also make `metal_roughness` with the characters' gloss
///   (the viewer's `Gloss::CHARACTER`) and no `gloss` or `translucency`.
///
/// Game textures keep their texels; block-compressed ones whose size is
/// not a multiple of 4 are rounded up to it with level 0 only (the stored
/// blocks already cover it). Mips missing from a BC1 texture without a
/// `Tex2` part are rebuilt.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialTextures {
    pub albedo: Option<String>,
    pub normal: Option<String>,
    pub gloss: Option<String>,
    pub translucency: Option<String>,
    pub metal_roughness: Option<String>,
    pub emissive: Option<String>,
    /// The cube map's emission mask (`em_…` like `emissive`).
    pub emissive_cube: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character_maps: Option<String>,
}

impl MaterialInfo {
    /// How the material keeps its pixels (the viewer's `cpu_material`).
    // SI-MAT-01: alpha test and samplers are not the FMAT ones.
    pub fn alpha(&self) -> Alpha {
        let state = self.render_state;
        match state.mode {
            2 => Alpha::Mask(state.alpha_test.unwrap_or(0.5)),
            3 => Alpha::Blend,
            _ if state.alpha_test.is_some() => Alpha::Mask(state.alpha_test.unwrap_or(0.5)),
            _ => Alpha::Opaque,
        }
    }

    /// Whether the game draws the material's shapes into its sky occlusion
    /// map: `uking_edit_sky_occlusion` on and drawn opaque or cut out (the
    /// viewer's `models::covers_sky`).
    pub fn covers_sky(&self) -> bool {
        self.edit_sky_occlusion && self.alpha() != Alpha::Blend
    }

    /// Foliage (cut out by a mask) with the shader's transmission on lets
    /// the light through, like the far trees' leaves (the viewer's
    /// `cpu_material`).
    // SI-MAT-05: foliage detection and mask mean are our heuristics.
    pub fn leaves(&self) -> bool {
        self.alpha() != Alpha::Opaque && self.transmission
    }
}

/// the original renderer's `models::MaterialLook`: what a `uking_mat` material says
/// about its shading beyond its textures (see the field docs there).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialLook {
    pub leaf: bool,
    /// `const_vector0` of a crown (`uking_modify_normal_type` 1).
    pub crown: Option<[f32; 4]>,
    pub normal_blend: Option<f32>,
    pub fresnel_cheat: bool,
    pub behave: MaterialBehave,
    pub water: bool,
    pub gloss_intensity: f32,
    pub chara_size: f32,
    pub transmission: bool,
    pub translucent: bool,
    pub leaf_light: Option<[f32; 5]>,
}

/// the original renderer's `models::MaterialBehave` (`uking_material_behave`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialBehave {
    pub code: u32,
    pub color: Option<[f32; 4]>,
    pub value: Option<f32>,
}

// --- GLB ---

impl Model {
    pub fn to_glb(&self) -> Result<Vec<u8>> {
        let mut writer = Writer::default();
        let mut meshes = Vec::new();
        for shape in &self.shapes {
            let count = shape.positions.len();
            if [shape.normals.len(), shape.uvs.len(), shape.tangents.len()] != [count; 3]
                || ![0, count].contains(&shape.colors.len())
                || ![0, count].contains(&shape.uvs1.len())
                || ![0, count].contains(&shape.uvs2.len())
                || ![0, count].contains(&shape.uvs3.len())
                || ![0, count].contains(&shape.joints.len())
                || shape.weights.len() != shape.joints.len()
            {
                return Err(FormatError::Invalid(
                    "model: a shape's attributes differ in length",
                ));
            }
            let attributes = Attributes {
                position: writer.floats(&shape.positions, "VEC3", true, Some(ARRAY_BUFFER)),
                normal: writer.floats(&shape.normals, "VEC3", false, Some(ARRAY_BUFFER)),
                texcoord0: writer.floats(&shape.uvs, "VEC2", false, Some(ARRAY_BUFFER)),
                tangent: writer.floats(&shape.tangents, "VEC4", false, Some(ARRAY_BUFFER)),
                color0: (!shape.colors.is_empty())
                    .then(|| writer.floats(&shape.colors, "VEC4", false, Some(ARRAY_BUFFER))),
                texcoord1: (!shape.uvs1.is_empty())
                    .then(|| writer.floats(&shape.uvs1, "VEC2", false, Some(ARRAY_BUFFER))),
                texcoord2: (!shape.uvs2.is_empty())
                    .then(|| writer.floats(&shape.uvs2, "VEC2", false, Some(ARRAY_BUFFER))),
                texcoord3: (!shape.uvs3.is_empty())
                    .then(|| writer.floats(&shape.uvs3, "VEC2", false, Some(ARRAY_BUFFER))),
                joints0: (!shape.joints.is_empty()).then(|| writer.u16s(&shape.joints, "VEC4")),
                weights0: (!shape.weights.is_empty())
                    .then(|| writer.floats(&shape.weights, "VEC4", false, Some(ARRAY_BUFFER))),
            };
            let primitives = shape
                .lods
                .iter()
                .map(|lod| Primitive {
                    attributes,
                    indices: writer.indices(&lod.indices),
                    material: shape.material,
                    mode: TRIANGLES,
                    extras: PrimitiveExtras { lod: lod.level },
                })
                .collect();
            meshes.push(Mesh {
                name: shape.name.clone(),
                primitives,
                extras: MeshExtras {
                    radius: shape.radius,
                },
            });
        }
        let materials = self
            .materials
            .iter()
            .map(|m| {
                let (alpha_mode, alpha_cutoff) = match m.info.alpha() {
                    Alpha::Opaque => ("OPAQUE", None),
                    Alpha::Mask(cutoff) => ("MASK", Some(cutoff)),
                    Alpha::Blend => ("BLEND", None),
                };
                GltfMaterial {
                    name: m.name.clone(),
                    alpha_mode: alpha_mode.into(),
                    alpha_cutoff,
                    double_sided: !m.info.render_state.cull_back,
                    extras: m.info.clone(),
                }
            })
            .collect();
        // The shapes' nodes, then the bones'.
        let first_bone = self.shapes.len() as u32;
        let skinned = !self.skeleton.is_empty();
        let mut nodes: Vec<Node> = self
            .shapes
            .iter()
            .enumerate()
            .map(|(i, s)| Node {
                name: s.name.clone(),
                mesh: Some(i as u32),
                skin: (skinned && !s.joints.is_empty()).then_some(0),
                ..Default::default()
            })
            .collect();
        let mut roots = Vec::new();
        for (i, bone) in self.skeleton.iter().enumerate() {
            match bone.parent {
                Some(parent) if (parent as usize) < i => {
                    nodes[(first_bone + parent) as usize]
                        .children
                        .push(first_bone + i as u32);
                }
                Some(_) => {
                    return Err(FormatError::Invalid(
                        "model: a bone comes before its parent",
                    ));
                }
                None => roots.push(first_bone + i as u32),
            }
            nodes.push(Node {
                name: bone.name.clone(),
                translation: Some(bone.translation),
                rotation: Some(bone.rotation),
                scale: Some(bone.scale),
                ..Default::default()
            });
        }
        let mut skins = Vec::new();
        if skinned {
            let inverse_binds: Vec<[f32; 16]> =
                self.skeleton.iter().map(|b| b.inverse_bind).collect();
            skins.push(Skin {
                inverse_bind_matrices: writer.floats(&inverse_binds, "MAT4", false, None),
                joints: (first_bone..first_bone + self.skeleton.len() as u32).collect(),
            });
        }
        let gltf = Gltf {
            asset: Asset::ours(),
            scene: 0,
            scenes: vec![Scene {
                name: self.name.clone(),
                nodes: (0..first_bone).chain(roots).collect(),
            }],
            nodes,
            meshes,
            materials,
            skins,
            ..Default::default()
        };
        writer.finish(gltf, "model: cannot write the glTF JSON")
    }

    /// Reads a GLB written by [`Model::to_glb`].
    pub fn from_glb(bytes: &[u8]) -> Result<Self> {
        let invalid = FormatError::Invalid;
        let glb = Glb::parse(bytes)?;
        let gltf = &glb.gltf;
        let mut shapes = Vec::with_capacity(gltf.nodes.len());
        for node in &gltf.nodes {
            let Some(mesh) = node.mesh else {
                continue; // A bone.
            };
            let mesh = gltf
                .meshes
                .get(mesh as usize)
                .ok_or(invalid("glb: mesh out of range"))?;
            let first = mesh
                .primitives
                .first()
                .ok_or(invalid("glb: mesh without primitives"))?;
            let a = &first.attributes;
            let mut lods = Vec::with_capacity(mesh.primitives.len());
            for primitive in &mesh.primitives {
                lods.push(Lod {
                    level: primitive.extras.lod,
                    indices: glb.u32s(primitive.indices)?,
                });
            }
            shapes.push(Shape {
                name: node.name.clone(),
                material: first.material,
                radius: mesh.extras.radius,
                positions: glb.floats::<3>(a.position)?,
                normals: glb.floats::<3>(a.normal)?,
                uvs: glb.floats::<2>(a.texcoord0)?,
                tangents: glb.floats::<4>(a.tangent)?,
                colors: a.color0.map_or(Ok(Vec::new()), |i| glb.floats::<4>(i))?,
                uvs1: a.texcoord1.map_or(Ok(Vec::new()), |i| glb.floats::<2>(i))?,
                uvs2: a.texcoord2.map_or(Ok(Vec::new()), |i| glb.floats::<2>(i))?,
                uvs3: a.texcoord3.map_or(Ok(Vec::new()), |i| glb.floats::<2>(i))?,
                joints: a.joints0.map_or(Ok(Vec::new()), |i| glb.u16s::<4>(i))?,
                weights: a.weights0.map_or(Ok(Vec::new()), |i| glb.floats::<4>(i))?,
                lods,
            });
        }
        let mut skeleton = Vec::new();
        if let Some(skin) = gltf.skins.first() {
            let inverse_binds = glb.floats::<16>(skin.inverse_bind_matrices)?;
            if inverse_binds.len() != skin.joints.len() {
                return Err(invalid("glb: a skin's matrices and joints differ"));
            }
            for (&joint, inverse_bind) in skin.joints.iter().zip(inverse_binds) {
                let node = gltf
                    .nodes
                    .get(joint as usize)
                    .ok_or(invalid("glb: joint out of range"))?;
                let parent = skin
                    .joints
                    .iter()
                    .position(|&j| {
                        gltf.nodes
                            .get(j as usize)
                            .is_some_and(|n| n.children.contains(&joint))
                    })
                    .map(|p| p as u32);
                skeleton.push(Bone {
                    name: node.name.clone(),
                    parent,
                    translation: node.translation.unwrap_or([0.0; 3]),
                    rotation: node.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0]),
                    scale: node.scale.unwrap_or([1.0; 3]),
                    inverse_bind,
                });
            }
        }
        let Glb { gltf, .. } = glb;
        Ok(Self {
            name: gltf
                .scenes
                .first()
                .map(|s| s.name.clone())
                .unwrap_or_default(),
            materials: gltf
                .materials
                .into_iter()
                .map(|m| Material {
                    name: m.name,
                    info: m.extras,
                })
                .collect(),
            shapes,
            skeleton,
        })
    }

    pub fn read(path: &std::path::Path) -> Result<Self> {
        Self::from_glb(&crate::read(path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_model() {
        let info = MaterialInfo {
            render_state: RenderState {
                mode: 2,
                alpha_test: Some(0.25),
                cull_back: false,
            },
            tex_srt0: Some([0.0, 2.0, 2.0, 0.5, 0.1, 0.2]),
            transmission: true,
            casts_shadows: true,
            look: MaterialLook {
                leaf: true,
                crown: Some([0.0, 9.0, 0.0, 10.0]),
                leaf_light: Some([0.005, -0.15, 10.0, 20.0, 30.0]),
                gloss_intensity: 1.0,
                chara_size: 2.0,
                ..Default::default()
            },
            terrain_layer: Some(12),
            textures: MaterialTextures {
                albedo: Some("Leaf_Alb+Leaf_Msk".into()),
                gloss: Some("Leaf_Nrm.gloss".into()),
                ..Default::default()
            },
            emission: [0.0, 1.5, 0.0],
            emission_cube: [0.5, 0.0, 0.0],
            samplers: vec![("_a0".into(), "Leaf_Alb".into())],
            ..Default::default()
        };
        assert_eq!(info.alpha(), Alpha::Mask(0.25));
        assert!(info.leaves() && !info.covers_sky());
        let model = Model {
            name: "Obj_Tree".into(),
            materials: vec![Material {
                name: "Mt_Leaf".into(),
                info,
            }],
            shapes: vec![Shape {
                name: "Leaf__Mt_Leaf".into(),
                material: 0,
                radius: 3.5,
                positions: vec![[0.0, 1.0, 2.0], [1.0, -1.0, 0.5], [0.1, 0.2, 0.3]],
                normals: vec![[0.0, 1.0, 0.0]; 3],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                tangents: vec![[1.0, 0.0, 0.0, -1.0]; 3],
                lods: vec![
                    Lod {
                        level: 0,
                        indices: vec![0, 1, 2, 2, 1, 0],
                    },
                    Lod {
                        level: 2,
                        indices: vec![0, 1, 2],
                    },
                ],
                ..Default::default()
            }],
            skeleton: Vec::new(),
        };
        let glb = model.to_glb().unwrap();
        assert_eq!(glb.len() % 4, 0);
        assert_eq!(Model::from_glb(&glb).unwrap(), model);
        // A static model's JSON says nothing of skins.
        let json = String::from_utf8_lossy(&glb);
        for absent in [
            "skins",
            "JOINTS_0",
            "TEXCOORD_1",
            "second_uv",
            "character_maps",
        ] {
            assert!(!json.contains(absent), "{absent}");
        }
        // A translucent G-buffer material with vertex colours and the
        // second UV set.
        let mut water = model.clone();
        water.materials[0].info.xlu = Some(crate::xlu::XluLook {
            kind: crate::xlu::XluKind::WaterBlend,
            const_value: [0.3, 0.99, 0.0, 0.25, 0.0, 0.0, 0.0, 0.0],
            samplers: vec![("_n0".into(), "Water_Nrm.nrm".into())],
            animation: Some(crate::xlu::SrtAnimation {
                name: "Obj_Tree_Auto".into(),
                frames: 600.0,
                looping: true,
                curves: vec![crate::xlu::SrtCurve {
                    component: 5,
                    frames: vec![0.0, 600.0],
                    keys: vec![[0.0, -1.0, 0.0, 0.0]; 2],
                    ..Default::default()
                }],
                constants: vec![(0, 0, 0.0)],
            }),
            ..Default::default()
        });
        water.shapes[0].colors = vec![[1.0, 0.5, 0.25, 0.0]; 3];
        water.shapes[0].uvs1 = vec![[0.5, 0.25]; 3];
        let glb = water.to_glb().unwrap();
        assert!(String::from_utf8_lossy(&glb).contains("COLOR_0"));
        assert_eq!(Model::from_glb(&glb).unwrap(), water);
    }

    /// Baked static models read back and write out byte for byte
    /// (`cargo test -p asset-format baked_models -- --ignored`, after
    /// `cargo bake --only objects,models`).
    #[test]
    #[ignore]
    fn rewrites_baked_models_byte_for_byte() {
        let models = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/models");
        let mut checked = 0;
        for folder in std::fs::read_dir(models).unwrap().flatten() {
            for file in std::fs::read_dir(folder.path()).unwrap().flatten() {
                let path = file.path();
                if path.extension().is_some_and(|e| e == "glb") {
                    let bytes = std::fs::read(&path).unwrap();
                    let model = Model::from_glb(&bytes).unwrap();
                    assert!(model.to_glb().unwrap() == bytes, "{}", path.display());
                    checked += 1;
                }
            }
        }
        assert!(checked > 0);
        println!("{checked} models");
    }

    #[test]
    fn round_trips_a_skinned_model() {
        let bone = |name: &str, parent, y: f32| Bone {
            name: name.into(),
            parent,
            translation: [0.0, y, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            inverse_bind: [
                1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, -y, 0.0, 1.0,
            ],
        };
        let model = Model {
            name: "Link".into(),
            materials: vec![Material {
                name: "Mt_Face".into(),
                info: MaterialInfo {
                    second_uv: SecondUv {
                        normal: true,
                        maps: true,
                        ..Default::default()
                    },
                    layered_face: Some(LayeredFace {
                        composition: FaceComposition::Face,
                        layers: std::array::from_fn(|i| FaceLayer {
                            texture: format!("face-slot-{i}"),
                            sampler: [i as u32, 0x1234, 0x5678],
                        }),
                        const_colors: std::array::from_fn(|i| [i as f32 * 0.1; 4]),
                        tex_srts: std::array::from_fn(|i| TextureSrt {
                            mode: (i % 3) as u32,
                            scale: [2.0, 3.0],
                            rotation: 0.25,
                            translation: [0.4, 0.5],
                        }),
                        zprepass_alpha: 0.75,
                        matrices: Some([[1.0, 0.0, 0.0, 1.0, 0.25, 0.5]; 6]),
                    }),
                    textures: MaterialTextures {
                        character_maps: Some("cm_Link_Head_AO".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            }],
            shapes: vec![Shape {
                name: "Face__Mt_Face".into(),
                positions: vec![[0.0, 1.0, 0.0], [0.1, 1.5, 0.0], [0.0, 1.5, 0.1]],
                normals: vec![[0.0, 0.0, 1.0]; 3],
                uvs: vec![[0.0, 0.0]; 3],
                tangents: vec![[1.0, 0.0, 0.0, 1.0]; 3],
                uvs1: vec![[0.5, 0.5]; 3],
                uvs2: vec![[0.2, 0.3], [0.4, 0.5], [0.6, 0.7]],
                uvs3: vec![[0.8, 0.9], [1.0, 1.1], [1.2, 1.3]],
                joints: vec![[0, 0, 0, 0], [1, 2, 0, 0], [2, 0, 0, 0]],
                weights: vec![
                    [1.0, 0.0, 0.0, 0.0],
                    [0.25, 0.75, 0.0, 0.0],
                    [1.0, 0.0, 0.0, 0.0],
                ],
                lods: vec![Lod {
                    level: 0,
                    indices: vec![0, 1, 2],
                }],
                ..Default::default()
            }],
            skeleton: vec![
                bone("Root", None, 0.0),
                bone("Spine", Some(0), 1.0),
                bone("Head", Some(1), 1.5),
            ],
        };
        let glb = model.to_glb().unwrap();
        assert_eq!(Model::from_glb(&glb).unwrap(), model);
    }

    #[test]
    fn native_texture_srt_modes_have_distinct_pivots_and_translation_signs() {
        let matrix = |mode, angle| {
            TextureSrt {
                mode,
                scale: [2.0, 3.0],
                rotation: 0.0,
                translation: [0.25, -0.5],
            }
            .matrix_from_sin_cos(angle)
        };
        assert_eq!(
            matrix(0, (0.0, 1.0)),
            Some([2.0, 0.0, 0.0, 3.0, -0.5, -3.5])
        );
        assert_eq!(
            matrix(1, (0.0, 1.0)),
            Some([2.0, 0.0, 0.0, 3.0, -1.0, -2.5])
        );
        assert_eq!(
            matrix(0, (1.0, 0.0)),
            Some([0.0, -3.0, 2.0, 0.0, -0.5, -0.5])
        );
        assert_eq!(
            matrix(1, (1.0, 0.0)),
            Some([0.0, -3.0, 2.0, 0.0, -1.5, 2.75])
        );
        assert_eq!(
            matrix(2, (1.0, 0.0)),
            Some([0.0, 3.0, -2.0, 0.0, 3.0, 0.25])
        );
        assert!(matrix(3, (0.0, 1.0)).is_none());
    }

    #[test]
    fn reads_the_viewer_alpha_rules() {
        let with = |mode, alpha_test| MaterialInfo {
            render_state: RenderState {
                mode,
                alpha_test,
                cull_back: true,
            },
            edit_sky_occlusion: true,
            ..Default::default()
        };
        assert_eq!(with(2, None).alpha(), Alpha::Mask(0.5));
        assert_eq!(with(3, Some(0.1)).alpha(), Alpha::Blend);
        assert_eq!(with(1, Some(0.1)).alpha(), Alpha::Mask(0.1));
        assert_eq!(with(0, None).alpha(), Alpha::Opaque);
        assert!(with(1, None).covers_sky() && with(2, None).covers_sky());
        assert!(!with(3, None).covers_sky());
    }
}
