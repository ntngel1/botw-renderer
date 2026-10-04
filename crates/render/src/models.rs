//! The game's models on the GPU: the baked model units
//! (`models/<folder>/<unit>.glb`, `asset_format::model`) and their KTX2
//! textures turned into Bevy meshes and `ObjectMaterial`s, loaded in the
//! background and cached per folder (the original renderer's BFRES folder).
//!
//! An actor's models come from `objects/actors.ron`; `ModelLibrary::actor`
//! resolves that and reports the parts to draw once they are ready.
//!
//! Ported from the original renderer's `models.rs`, object part: what its
//! `load_folder` decoded from the dump (static meshes, derived textures) is
//! baked (`bake::models`); what it then made of a material (`cpu_material`,
//! `CpuFolder::upload`, `with_object_materials`) is here, building the
//! `ObjectMaterial` directly instead of a `CharacterMaterial` base and its
//! twin (same values). Characters (skinned meshes, `CharacterMaterial`)
//! and collision come later.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_format::model::{Alpha, DynamicExposure, MaterialInfo, Model, SecondUv};
use asset_format::objects::{ActorModels, ModelRef};
use asset_format::paths;
use asset_format::texture::{Swizzle, Texture};
use asset_format::xlu::XluLook;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, MeshVertexAttribute, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::VertexFormat;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::character_material::{CharacterMaterial, CharacterShading};
use crate::clouds::CloudShadows;
use crate::deferred_light::{CubeMean, SsaoNoise};
use crate::face_material::{FaceMaterial, FaceParams, FaceShading};
use crate::look::LookTexture;
use crate::model_water::{ModelWaterMaterial, ModelWaters};
use crate::object_material::{HandOffMask, ObjectMaterial, ObjectShading};
use crate::sky_occlusion::{SkyOccluder, SkyShape};
use crate::texture::{gpu_image, texture_layer_image};

/// Original `_u2` and `_u3` for the layered face vertex shader. They are
/// separate from Bevy's UV0/UV1, which ordinary character shading uses.
pub(crate) const ATTRIBUTE_UV_2: MeshVertexAttribute =
    MeshVertexAttribute::new("Face_Uv2", 0x48595202, VertexFormat::Float32x2);
pub(crate) const ATTRIBUTE_UV_3: MeshVertexAttribute =
    MeshVertexAttribute::new("Face_Uv3", 0x48595203, VertexFormat::Float32x2);

pub struct ModelsPlugin {
    /// The `assets/` folder.
    pub assets: PathBuf,
}

impl Plugin for ModelsPlugin {
    fn build(&self, app: &mut App) {
        let path = self.assets.join(paths::ACTOR_MODELS);
        let library = match asset_format::read_ron::<ActorModels>(&path) {
            Ok(table) => {
                let mut folder_units: HashMap<String, BTreeSet<String>> = HashMap::new();
                for model in table.actors.values().flatten() {
                    folder_units
                        .entry(model.folder.clone())
                        .or_default()
                        .extend(model.units.iter().cloned());
                }
                // Rocks and cliffs texture themselves with layers of the
                // terrain's albedo array.
                let albedo = self.assets.join(paths::TERRAIN_ALBEDO);
                let terrain = AsyncComputeTaskPool::get().spawn(async move {
                    Texture::read(&albedo)
                        .inspect_err(|e| warn!("terrain albedo for rocks: {e}"))
                        .ok()
                        .map(Arc::new)
                });
                Some(Library {
                    assets: self.assets.clone(),
                    actors: table.actors.into_iter().collect(),
                    folder_units: folder_units
                        .into_iter()
                        .map(|(folder, units)| (folder, Arc::new(units.into_iter().collect())))
                        .collect(),
                    terrain_albedo: TerrainAlbedo::Loading(terrain),
                    resolved: HashMap::new(),
                    folders: HashMap::new(),
                })
            }
            Err(error) => {
                warn!("no models ({error}); run `cargo bake --only objects,models`");
                None
            }
        };
        app.insert_resource(ModelLibrary(library))
            .init_resource::<DynamicEmission>()
            .add_systems(Update, (finish_loads, follow_dynamic_exposure).chain());
    }
}

/// One mesh with its material.
#[derive(Clone, Debug)]
pub struct Part {
    pub mesh: Handle<Mesh>,
    /// A placed object's material.
    pub object_material: Option<Handle<ObjectMaterial>>,
    /// Model water or glass (`model_water.rs`): drawn with this instead of
    /// `object_material`.
    pub water_material: Option<Handle<ModelWaterMaterial>>,
    /// The BFRES shape's name, e.g. `Skin__Mt_Upper_Skin`.
    pub shape: Arc<str>,
    /// Coarser versions of `mesh`, for distance (may be empty).
    pub lods: Vec<Handle<Mesh>>,
    /// Distance of the farthest vertex from the model's origin.
    pub radius: f32,
    /// Whether the game lets it cast shadows (`gsys_dynamic_depth_shadow`:
    /// a tree's inner leaf cards do not).
    pub casts_shadows: bool,
    /// Whether the game draws it into the environment's cube map
    /// (`gsys_cube_map`).
    pub in_cube_map: bool,
    /// The material to draw it with into the cube map, where that glows
    /// otherwise than `object_material` (`MaterialInfo::emission_cube`);
    /// `None`: the same.
    pub cube_material: Option<Handle<ObjectMaterial>>,
    /// What the material says about its shading beyond its textures.
    pub look: MaterialLook,
    /// A placed object's gloss map (the blue of its normal map), if any.
    pub gloss: Option<Handle<Image>>,
    /// Leaves' translucency map (see [`MaterialLook::leaf_light`]), if any.
    pub translucency: Option<Handle<Image>>,
}

/// What a `uking_mat` material says about its shading beyond its textures
/// (the original renderer's, baked as `asset_format::model::MaterialLook`; see
/// there and the viewer for the fields).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MaterialLook {
    /// Foliage: a leaf or plant material (`uking_material_behave` 105, or cut
    /// out by a mask with the shader's transmission on).
    pub leaf: bool,
    /// A tree crown whose normals the shader bends away from a centre
    /// (`uking_modify_normal_type` 1): the centre in model space (xyz,
    /// `const_vector0`) and `w`.
    pub crown: Option<Vec4>,
    /// How much of the normal map the shader uses
    /// (`uking_normalmap_blend_ratio`).
    pub normal_blend: Option<f32>,
    /// Rim light on (`uking_enable_fresnel_cheat`).
    pub fresnel_cheat: bool,
    /// `uking_material_behave`.
    pub behave: MaterialBehave,
    /// A water surface or waterfall drawn as a model (behave 103).
    pub water: bool,
    /// `uking_grossy_intensity` (1 where the material does not say).
    pub gloss_intensity: f32,
    /// `uking_chara_size` (2 where the material does not say).
    pub chara_size: f32,
    /// The skin lets light through (`uking_enable_transmission`), by the
    /// alpha of its occlusion map.
    pub transmission: bool,
    /// Light passes through (`uking_enable_transmission`).
    pub translucent: bool,
    /// Leaves lit through from behind and their sheen: `const_value1`–`5`.
    pub leaf_light: Option<[f32; 5]>,
}

/// `uking_material_behave`: a code for special shading, with the
/// material's `const_colorN`/`const_valueN` of the same N.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MaterialBehave {
    pub code: u32,
    pub color: Option<[f32; 4]>,
    pub value: Option<f32>,
}

impl From<asset_format::model::MaterialLook> for MaterialLook {
    fn from(look: asset_format::model::MaterialLook) -> Self {
        Self {
            leaf: look.leaf,
            crown: look.crown.map(Vec4::from),
            normal_blend: look.normal_blend,
            fresnel_cheat: look.fresnel_cheat,
            behave: MaterialBehave {
                code: look.behave.code,
                color: look.behave.color,
                value: look.behave.value,
            },
            water: look.water,
            gloss_intensity: look.gloss_intensity,
            chara_size: look.chara_size,
            transmission: look.transmission,
            translucent: look.translucent,
            leaf_light: look.leaf_light,
        }
    }
}

/// What an actor draws.
#[derive(Clone, Debug)]
pub enum ActorModel {
    Loading,
    Ready(Arc<Vec<Part>>),
    /// The actor has no model (or it failed to load).
    None,
}

#[derive(Resource)]
pub struct ModelLibrary(Option<Library>);

struct Library {
    assets: PathBuf,
    /// Actor name → its models (`objects/actors.ron`).
    actors: HashMap<String, Vec<ModelRef>>,
    /// Folder → the units baked for it.
    folder_units: HashMap<String, Arc<Vec<String>>>,
    /// The terrain's albedo array: rocks and cliffs texture themselves with its layers.
    terrain_albedo: TerrainAlbedo,
    /// Actors whose parts are all loaded, so streaming does not gather
    /// them again.
    resolved: HashMap<String, Arc<Vec<Part>>>,
    folders: HashMap<String, FolderEntry>,
}

enum TerrainAlbedo {
    Loading(Task<Option<Arc<Texture>>>),
    Ready(Option<Arc<Texture>>),
}

enum FolderEntry {
    Loading(Task<CpuFolder>),
    Ready {
        units: HashMap<String, Arc<Vec<Part>>>,
        sky: HashMap<String, Arc<SkyShape>>,
    },
}

impl ModelLibrary {
    /// The parts of `actor`'s models; starts loading on first request.
    pub fn actor(&mut self, actor: &str) -> ActorModel {
        let Some(library) = &mut self.0 else {
            return ActorModel::None;
        };
        if let Some(parts) = library.resolved.get(actor) {
            return ActorModel::Ready(parts.clone());
        }
        let Some(refs) = library.actors.get(actor) else {
            return ActorModel::None;
        };
        let TerrainAlbedo::Ready(terrain) = &library.terrain_albedo else {
            return ActorModel::Loading;
        };
        let mut parts = Vec::new();
        for model in refs {
            match library.folders.get(&model.folder) {
                Some(FolderEntry::Ready { units, .. }) => {
                    for unit in &model.units {
                        if let Some(unit_parts) = units.get(unit) {
                            parts.extend(unit_parts.iter().cloned());
                        }
                    }
                }
                Some(FolderEntry::Loading(_)) => return ActorModel::Loading,
                None => {
                    let dir = library.assets.join(paths::MODELS).join(&model.folder);
                    let units = library
                        .folder_units
                        .get(&model.folder)
                        .cloned()
                        .unwrap_or_default();
                    let terrain = terrain.clone();
                    let task = AsyncComputeTaskPool::get()
                        .spawn(async move { load_folder(&dir, &units, terrain.as_deref()) });
                    library
                        .folders
                        .insert(model.folder.clone(), FolderEntry::Loading(task));
                    return ActorModel::Loading;
                }
            }
        }
        if parts.is_empty() {
            ActorModel::None
        } else {
            let parts = Arc::new(parts);
            library.resolved.insert(actor.to_owned(), parts.clone());
            ActorModel::Ready(parts)
        }
    }

    /// Some model is still being loaded.
    pub fn is_loading(&self) -> bool {
        self.0.as_ref().is_some_and(|l| {
            matches!(l.terrain_albedo, TerrainAlbedo::Loading(_))
                || l.folders
                    .values()
                    .any(|f| matches!(f, FolderEntry::Loading(_)))
        })
    }

    pub fn loaded_folders(&self) -> usize {
        self.0.as_ref().map_or(0, |l| {
            l.folders
                .values()
                .filter(|f| matches!(f, FolderEntry::Ready { .. }))
                .count()
        })
    }

    /// `actor`'s shapes that cover the sky above the field
    /// (`sky_occlusion.rs`), once its model has loaded; `None` if it has
    /// none.
    pub fn sky_occluder(&self, actor: &str) -> Option<SkyOccluder> {
        let library = self.0.as_ref()?;
        let refs = library.actors.get(actor)?;
        let shapes: Vec<Arc<SkyShape>> = refs
            .iter()
            .filter_map(|model| match library.folders.get(&model.folder) {
                Some(FolderEntry::Ready { sky, .. }) => {
                    Some(model.units.iter().filter_map(|u| sky.get(u).cloned()))
                }
                _ => None,
            })
            .flatten()
            .collect();
        (!shapes.is_empty()).then(|| SkyOccluder(shapes.into()))
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_loads(
    mut library: ResMut<ModelLibrary>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut object_materials: ResMut<Assets<ObjectMaterial>>,
    mask: Res<HandOffMask>,
    ssao_noise: Option<Res<SsaoNoise>>,
    cube_mean: Option<Res<CubeMean>>,
    clouds: Option<Res<CloudShadows>>,
    look: Option<Res<LookTexture>>,
    mut dynamic: ResMut<DynamicEmission>,
    mut waters: ResMut<ModelWaters>,
    mut water_materials: ResMut<Assets<ModelWaterMaterial>>,
) {
    let Some(library) = &mut library.0 else {
        return;
    };
    if let TerrainAlbedo::Loading(task) = &mut library.terrain_albedo
        && let Some(albedo) = block_on(poll_once(task))
    {
        library.terrain_albedo = TerrainAlbedo::Ready(albedo);
    }
    let mut shading = None;
    for entry in library.folders.values_mut() {
        let FolderEntry::Loading(task) = entry else {
            continue;
        };
        let Some(mut folder) = block_on(poll_once(task)) else {
            continue;
        };
        let shading = shading.get_or_insert_with(|| {
            ObjectShading::new(
                clouds.as_deref(),
                &mask,
                look.as_deref(),
                ssao_noise.as_deref(),
                cube_mean.as_deref(),
            )
        });
        let sky = std::mem::take(&mut folder.sky);
        *entry = FolderEntry::Ready {
            units: folder.upload(
                &mut meshes,
                &mut images,
                &mut object_materials,
                shading,
                &mut dynamic,
                (&mut waters, &mut water_materials, look.as_deref()),
            ),
            sky,
        };
    }
}

/// The object and character materials whose glow follows the game's
/// `uking_dynamic_exposure` (combiner input 507), the sky palette's
/// `Exposure`: 1 by day, 0 at night, dawn and dusk (windows lit at night
/// take `1 − e`).
#[derive(Resource, Default)]
pub struct DynamicEmission {
    /// Material, its glow without the factor, how it follows.
    materials: Vec<(Handle<ObjectMaterial>, LinearRgba, DynamicExposure)>,
    characters: Vec<(Handle<CharacterMaterial>, LinearRgba, DynamicExposure)>,
    /// The exposure the materials before `fresh` were given.
    applied: Option<f32>,
    fresh: usize,
    fresh_characters: usize,
}

impl DynamicEmission {
    fn add(
        &mut self,
        material: Handle<ObjectMaterial>,
        glow: LinearRgba,
        exposure: DynamicExposure,
    ) {
        self.materials.push((material, glow, exposure));
    }
}

/// Gives the new materials, and all of them when the palette's exposure
/// has changed, their glow at the sky's exposure.
fn follow_dynamic_exposure(
    sky: Option<Res<crate::daynight::Sky>>,
    mut dynamic: ResMut<DynamicEmission>,
    mut materials: ResMut<Assets<ObjectMaterial>>,
    mut characters: Option<ResMut<Assets<CharacterMaterial>>>,
) {
    let e = sky.map_or(1.0, |sky| sky.palette.exposure.clamp(0.0, 1.0));
    let from = match dynamic.applied {
        Some(applied) if (applied - e).abs() < 1.0 / 512.0 => dynamic.fresh,
        _ => 0,
    };
    for (handle, glow, exposure) in &dynamic.materials[from..] {
        if let Some(mut material) = materials.get_mut(handle) {
            let f = exposure.factor(e);
            material.base.emissive = LinearRgba::rgb(glow.red * f, glow.green * f, glow.blue * f);
        }
    }
    let from_character = match dynamic.applied {
        Some(applied) if (applied - e).abs() < 1.0 / 512.0 => dynamic.fresh_characters,
        _ => 0,
    };
    if let Some(characters) = characters.as_mut() {
        for (handle, glow, exposure) in &dynamic.characters[from_character..] {
            if let Some(mut material) = characters.get_mut(handle) {
                let f = exposure.factor(e);
                material.base.emissive =
                    LinearRgba::rgb(glow.red * f, glow.green * f, glow.blue * f);
            }
        }
        dynamic.fresh_characters = dynamic.characters.len();
    }
    if from == 0 {
        dynamic.applied = Some(e);
    }
    dynamic.fresh = dynamic.materials.len();
}

/// A folder's model units, decoded but not yet on the GPU.
struct CpuFolder {
    meshes: Vec<Mesh>,
    images: Vec<Image>,
    materials: Vec<CpuMaterial>,
    /// Unit name → its shapes.
    units: HashMap<String, Vec<CpuShape>>,
    /// Unit name → its shapes that cover the sky.
    sky: HashMap<String, Arc<SkyShape>>,
}

/// A shape of a decoded model: indices into its folder's meshes and materials.
struct CpuShape {
    mesh: usize,
    material: usize,
    name: Arc<str>,
    radius: f32,
    /// Coarser levels of detail.
    lods: Vec<usize>,
}

struct CpuMaterial {
    albedo: Option<usize>,
    /// Multiplies the albedo texture; the colour itself without one.
    base_color: LinearRgba,
    normal: Option<usize>,
    /// Roughness in green, metal in blue.
    metal_roughness: Option<usize>,
    /// Placed objects' gloss.
    gloss_map: Option<usize>,
    /// Leaves' translucency (see [`MaterialLook::leaf_light`]).
    translucency_map: Option<usize>,
    /// What glows (grey or colour) and how brightly.
    emissive: Option<usize>,
    emissive_color: LinearRgba,
    /// What glows in the cube map, likewise.
    emissive_cube: Option<usize>,
    emissive_cube_color: LinearRgba,
    /// Whether the two glows follow `uking_dynamic_exposure`.
    emission_exposure: DynamicExposure,
    emission_cube_exposure: DynamicExposure,
    alpha: AlphaMode,
    double_sided: bool,
    uv_transform: bevy::math::Affine2,
    casts_shadows: bool,
    /// Drawn into the environment's cube map (`cubemap.rs`).
    in_cube_map: bool,
    look: MaterialLook,
    /// Characters' extra maps (`MaterialTextures::character_maps`).
    character_maps: Option<usize>,
    face: Option<CpuFace>,
    /// Which maps a character material reads with the second UV set.
    second_uv: SecondUv,
    /// Model water or glass: what its program reads, and its samplers'
    /// images (folder indices) and component selections
    /// (`model_water::SLOTS` order).
    xlu: Option<(XluLook, [Option<(usize, Swizzle)>; 6])>,
}

struct CpuFace {
    params: FaceParams,
    images: [usize; 6],
}

/// The FMAT sampler's wrap/filter/LOD state; unsupported modes fail loading.
fn face_sampler(words: [u32; 3]) -> Result<(bevy::image::ImageSampler, f32), String> {
    use bevy::image::{
        ImageAddressMode as Address, ImageFilterMode as Filter, ImageSampler,
        ImageSamplerDescriptor,
    };
    let wrap = |value| match value {
        0 => Ok(Address::Repeat),
        1 => Ok(Address::MirrorRepeat),
        2 => Ok(Address::ClampToEdge),
        _ => Err(format!("unsupported face sampler wrap {value}")),
    };
    let filter = |value| match value {
        0 => Ok(Filter::Nearest),
        1 => Ok(Filter::Linear),
        _ => Err(format!("unsupported face sampler filter {value}")),
    };
    let mip = match (words[0] >> 17) & 3 {
        1 => Filter::Nearest,
        2 => Filter::Linear,
        value => return Err(format!("unsupported face sampler mip filter {value}")),
    };
    let sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: wrap(words[0] & 7)?,
        address_mode_v: wrap((words[0] >> 3) & 7)?,
        mag_filter: filter((words[0] >> 9) & 7)?,
        min_filter: filter((words[0] >> 12) & 7)?,
        mipmap_filter: mip,
        lod_min_clamp: (words[1] & 1023) as f32 / 64.0,
        lod_max_clamp: ((words[1] >> 10) & 1023) as f32 / 64.0,
        ..default()
    });
    let bias = ((words[1] as i32) >> 20) as f32 / 64.0;
    Ok((sampler, bias))
}

impl CpuFolder {
    fn upload(
        self,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<ObjectMaterial>,
        shading: &ObjectShading,
        dynamic: &mut DynamicEmission,
        (waters, water_materials, look_texture): (
            &mut ModelWaters,
            &mut Assets<ModelWaterMaterial>,
            Option<&LookTexture>,
        ),
    ) -> HashMap<String, Arc<Vec<Part>>> {
        let mesh_handles: Vec<Handle<Mesh>> =
            self.meshes.into_iter().map(|m| meshes.add(m)).collect();
        let image_handles: Vec<Handle<Image>> =
            self.images.into_iter().map(|i| images.add(i)).collect();
        let image = |i: Option<usize>| i.map(|i| image_handles[i].clone());
        let mut material = |m: &CpuMaterial,
                            emissive: Option<usize>,
                            emissive_color: LinearRgba,
                            exposure: DynamicExposure| {
            let base = StandardMaterial {
                base_color_texture: image(m.albedo),
                base_color: m.base_color.into(),
                normal_map_texture: image(m.normal),
                metallic_roughness_texture: image(m.metal_roughness),
                emissive_texture: image(emissive),
                emissive: emissive_color,
                alpha_mode: m.alpha,
                double_sided: m.double_sided,
                cull_mode: if m.double_sided {
                    None
                } else {
                    Some(bevy::render::render_resource::Face::Back)
                },
                // The texture holds the values where there is one.
                // SI-MAT-03: metal and roughness tables for Bevy PBR are ours.
                perceptual_roughness: if m.metal_roughness.is_some() {
                    1.0
                } else {
                    OBJECT_GLOSS.roughness.0
                },
                metallic: if m.metal_roughness.is_some() {
                    1.0
                } else {
                    0.0
                },
                reflectance: OBJECT_GLOSS.reflectance,
                uv_transform: m.uv_transform,
                ..default()
            };
            let handle = materials.add(ObjectMaterial {
                base,
                extension: shading.for_material(
                    &m.look,
                    image(m.gloss_map).as_ref(),
                    image(m.translucency_map).as_ref(),
                ),
            });
            if exposure != DynamicExposure::None {
                dynamic.add(handle.clone(), emissive_color, exposure);
            }
            handle
        };
        let material_handles: Vec<Handle<ObjectMaterial>> = self
            .materials
            .iter()
            .map(|m| material(m, m.emissive, m.emissive_color, m.emission_exposure))
            .collect();
        let water_handles: Vec<Option<Handle<ModelWaterMaterial>>> = self
            .materials
            .iter()
            .map(|m| {
                let (look, slots) = m.xlu.as_ref()?;
                let slots =
                    slots.map(|s| s.map(|(i, swizzle)| (image_handles[i].clone(), swizzle)));
                Some(waters.add(water_materials, look, slots, m.double_sided, look_texture))
            })
            .collect();
        let cube_handles: Vec<Option<Handle<ObjectMaterial>>> = self
            .materials
            .iter()
            .map(|m| {
                let differs = (
                    m.emissive_cube,
                    m.emissive_cube_color,
                    m.emission_cube_exposure,
                ) != (m.emissive, m.emissive_color, m.emission_exposure);
                (m.in_cube_map && differs).then(|| {
                    material(
                        m,
                        m.emissive_cube,
                        m.emissive_cube_color,
                        m.emission_cube_exposure,
                    )
                })
            })
            .collect();
        self.units
            .into_iter()
            .map(|(unit, shapes)| {
                let parts = shapes
                    .into_iter()
                    .map(|shape| {
                        let material = &self.materials[shape.material];
                        Part {
                            mesh: mesh_handles[shape.mesh].clone(),
                            object_material: Some(material_handles[shape.material].clone()),
                            water_material: water_handles[shape.material].clone(),
                            shape: shape.name,
                            lods: shape
                                .lods
                                .into_iter()
                                .map(|l| mesh_handles[l].clone())
                                .collect(),
                            radius: shape.radius,
                            casts_shadows: material.casts_shadows,
                            in_cube_map: material.in_cube_map,
                            cube_material: cube_handles[shape.material].clone(),
                            look: material.look,
                            gloss: image(material.gloss_map),
                            translucency: image(material.translucency_map),
                        }
                    })
                    .collect();
                (unit, Arc::new(parts))
            })
            .collect()
    }
}

/// Reads a folder's baked units and the textures they use (the viewer's
/// `load_folder`, from `assets/`). Units or textures that cannot be read
/// are left out, as the viewer leaves out what it cannot decode.
fn load_folder(dir: &Path, units: &[String], terrain: Option<&Texture>) -> CpuFolder {
    let mut folder = CpuFolder {
        meshes: Vec::new(),
        images: Vec::new(),
        materials: Vec::new(),
        units: HashMap::new(),
        sky: HashMap::new(),
    };
    let mut image_cache: HashMap<String, Option<(usize, Swizzle)>> = HashMap::new();
    let mut layer_cache: HashMap<u32, Option<usize>> = HashMap::new();
    for unit in units {
        let model = match Model::read(&dir.join(format!("{unit}.glb"))) {
            Ok(model) => model,
            Err(error) => {
                debug!("model unit {unit}: {error}");
                continue;
            }
        };
        let mut material_indices = Vec::with_capacity(model.materials.len());
        for material in &model.materials {
            let info = &material.info;
            let textures = &info.textures;
            // Rocks and cliffs sample a layer of the terrain's material array.
            let albedo = match info.terrain_layer {
                Some(layer) => *layer_cache.entry(layer).or_insert_with(|| {
                    let image = texture_layer_image(terrain?, layer)?;
                    folder.images.push(image);
                    Some(folder.images.len() - 1)
                }),
                None => texture(dir, &textures.albedo, &mut image_cache, &mut folder.images),
            };
            let mut map =
                |name: &Option<String>| texture(dir, name, &mut image_cache, &mut folder.images);
            let mut cpu = cpu_material(info, albedo);
            cpu.normal = map(&textures.normal);
            cpu.gloss_map = map(&textures.gloss);
            cpu.translucency_map = map(&textures.translucency);
            cpu.metal_roughness = map(&textures.metal_roughness);
            cpu.emissive = map(&textures.emissive);
            cpu.emissive_cube = map(&textures.emissive_cube);
            // A mask that cannot be read would light the whole surface.
            if textures.emissive.is_some() && cpu.emissive.is_none() {
                cpu.emissive_color = LinearRgba::BLACK;
            }
            if textures.emissive_cube.is_some() && cpu.emissive_cube.is_none() {
                cpu.emissive_cube_color = LinearRgba::BLACK;
            }
            cpu.xlu = info.xlu.as_ref().map(|look| {
                let slots = crate::model_water::SLOTS.map(|slot| {
                    let name = look.sampler(slot).map(str::to_owned);
                    swizzled_texture(dir, &name, &mut image_cache, &mut folder.images)
                });
                (look.clone(), slots)
            });
            folder.materials.push(cpu);
            material_indices.push((folder.materials.len() - 1, info.covers_sky()));
        }

        let mut shapes = Vec::new();
        // The shapes that cover the sky: their coarsest meshes, merged.
        let (mut sky_vertices, mut sky_triangles): (Vec<Vec3>, Vec<[u32; 3]>) =
            (Vec::new(), Vec::new());
        for shape in &model.shapes {
            let Some(&(material, covers_sky)) = material_indices.get(shape.material as usize)
            else {
                continue;
            };
            let Some(mesh) = build_mesh(shape) else {
                continue;
            };
            let lods = lod_meshes(&mesh, &shape.lods);
            if covers_sky {
                append_triangles(
                    lods.last().unwrap_or(&mesh),
                    &mut sky_vertices,
                    &mut sky_triangles,
                );
            }
            folder.meshes.push(mesh);
            let index = folder.meshes.len() - 1;
            let lods: Vec<usize> = lods
                .into_iter()
                .map(|lod| {
                    folder.meshes.push(lod);
                    folder.meshes.len() - 1
                })
                .collect();
            shapes.push(CpuShape {
                mesh: index,
                material,
                name: Arc::from(shape.name.as_str()),
                radius: shape.radius,
                lods,
            });
        }
        folder.units.insert(unit.clone(), shapes);
        if !sky_triangles.is_empty() {
            folder.sky.insert(
                unit.clone(),
                Arc::new(SkyShape::new(sky_vertices, sky_triangles)),
            );
        }
    }
    folder
}

/// The folder's KTX2 file `name` as a GPU image, read once per folder.
fn texture(
    dir: &Path,
    name: &Option<String>,
    cache: &mut HashMap<String, Option<(usize, Swizzle)>>,
    images: &mut Vec<Image>,
) -> Option<usize> {
    swizzled_texture(dir, name, cache, images).map(|(i, _)| i)
}

/// [`texture`] with the game's component selection of the texture (which
/// the GPU image does not apply).
fn swizzled_texture(
    dir: &Path,
    name: &Option<String>,
    cache: &mut HashMap<String, Option<(usize, Swizzle)>>,
    images: &mut Vec<Image>,
) -> Option<(usize, Swizzle)> {
    let name = name.as_ref()?;
    *cache.entry(name.clone()).or_insert_with(|| {
        let path = dir.join(format!("{name}.ktx2"));
        let texture = Texture::read(&path)
            .inspect_err(|e| warn!("texture {}: {e}", path.display()))
            .ok()?;
        images.push(gpu_image(&texture, true)?);
        Some((images.len() - 1, texture.swizzle))
    })
}

/// How a material's specular and metal masks become physical roughness
/// and metal (the game has no roughness parameter): the viewer's `Gloss`
/// (the metal-roughness maps are baked with it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gloss {
    /// Perceptual roughness without and with a full specular mask.
    pub roughness: (f32, f32),
    /// Metal at a full metal mask.
    pub metal: f32,
    pub reflectance: f32,
}

impl Gloss {
    /// Objects of the world.
    // SI-MAT-03: metal and roughness tables for Bevy PBR are ours.
    pub const OBJECT: Self = Self {
        roughness: (0.85, 0.3),
        metal: 1.0,
        reflectance: 0.3,
    };
    /// Characters and their clothes: read as gloss and metal, `_Spm` makes
    /// rubbery skin and chrome armour; the game's characters are matte.
    // SI-MAT-03: metal and roughness tables for Bevy PBR are ours.
    pub const CHARACTER: Self = Self {
        roughness: (0.9, 0.6),
        metal: 0.4,
        reflectance: 0.2,
    };
}

const OBJECT_GLOSS: Gloss = Gloss::OBJECT;

/// Malice's colour, which the game supplies at run time (hand-picked: a
/// purple so dark it reads as black).
// SI-MAT-04: malice albedo is fitted.
const MALICE_ALBEDO: LinearRgba = LinearRgba::rgb(0.02, 0.004, 0.012);

/// Luminance (nits) of the game's emission value 1: one unit of the
/// frame. The G-buffer programs write the combiner's emission as it is
/// into the scene colour (agl format 0x1a: R11G11B10 float), which the
/// main pass adds to the shaded colour (`src + dst·src.a`, `src.a` the
/// fog's transmittance squared: `object_material.wgsl`); one frame unit is
/// `1 / exposure` nits at the camera's fixed exposure
/// (`daynight::DAY_EV100`, see `Environment::lux_per_intensity`).
fn emission_nits() -> f32 {
    1.0 / bevy::camera::Exposure {
        ev100: crate::daynight::DAY_EV100,
    }
    .exposure()
}

/// The material's values (the viewer's `cpu_material`); `albedo` is its
/// albedo image, if it has one.
// SI-MAT-01: alpha test and samplers are not the FMAT ones.
fn cpu_material(info: &MaterialInfo, albedo: Option<usize>) -> CpuMaterial {
    let alpha = match info.alpha() {
        Alpha::Opaque => AlphaMode::Opaque,
        Alpha::Mask(cutoff) => AlphaMode::Mask(cutoff),
        Alpha::Blend => AlphaMode::Blend,
    };
    // tex_srt0: mode, scale x/y, rotation, translation x/y.
    let uv_transform = match info.tex_srt0 {
        Some([_, sx, sy, rotation, tx, ty]) => bevy::math::Affine2::from_scale_angle_translation(
            Vec2::new(sx, sy),
            rotation,
            Vec2::new(tx, ty),
        ),
        None => bevy::math::Affine2::IDENTITY,
    };
    // Without a texture the albedo is a constant colour, or malice's.
    let base_color = match albedo {
        Some(_) => LinearRgba::WHITE,
        None if info.malice && info.albedo_constant.is_none() => MALICE_ALBEDO,
        None => info.albedo_constant.map_or(LinearRgba::WHITE, |[r, g, b]| {
            LinearRgba::rgb(r.min(1.0), g.min(1.0), b.min(1.0))
        }),
    };
    let nits = emission_nits();
    let [r, g, b] = info.emission.map(|c| c * nits);
    let [cr, cg, cb] = info.emission_cube.map(|c| c * nits);
    CpuMaterial {
        albedo,
        base_color,
        normal: None,
        metal_roughness: None,
        gloss_map: None,
        translucency_map: None,
        emissive: None,
        emissive_color: LinearRgba::rgb(r, g, b),
        emissive_cube: None,
        emissive_cube_color: LinearRgba::rgb(cr, cg, cb),
        emission_exposure: info.emission_exposure,
        emission_cube_exposure: info.emission_cube_exposure,
        alpha,
        double_sided: !info.render_state.cull_back,
        uv_transform,
        casts_shadows: info.casts_shadows,
        in_cube_map: info.in_cube_map,
        look: info.look.into(),
        character_maps: None,
        face: None,
        second_uv: info.second_uv,
        xlu: None,
    }
}

/// A shape's full mesh (level 0) in model space.
fn build_mesh(shape: &asset_format::model::Shape) -> Option<Mesh> {
    let lod = shape.lods.first()?;
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, shape.positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, shape.normals.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, shape.uvs.clone());
    mesh.insert_indices(Indices::U32(lod.indices.clone()));
    mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, shape.tangents.clone());
    if !shape.colors.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, shape.colors.clone());
    }
    if !shape.uvs1.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, shape.uvs1.clone());
    }
    Some(mesh)
}

/// Adds a mesh's triangles to a trimesh being built.
fn append_triangles(mesh: &Mesh, vertices: &mut Vec<Vec3>, triangles: &mut Vec<[u32; 3]>) {
    let (
        Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)),
        Some(Indices::U32(indices)),
    ) = (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.indices())
    else {
        return;
    };
    let first = vertices.len() as u32;
    vertices.extend(positions.iter().map(|&p| Vec3::from(p)));
    triangles.extend(
        indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| [first + t[0], first + t[1], first + t[2]]),
    );
}

/// The shape's coarser levels of detail (`lods[1..]`, index lists into the
/// same vertices), each keeping only the vertices it uses.
fn lod_meshes(full: &Mesh, lods: &[asset_format::model::Lod]) -> Vec<Mesh> {
    use bevy::mesh::VertexAttributeValues::{Float32x2, Float32x3, Float32x4};
    let (
        Some(Float32x3(positions)),
        Some(Float32x3(normals)),
        Some(Float32x2(uvs)),
        Some(Float32x4(tangents)),
    ) = (
        full.attribute(Mesh::ATTRIBUTE_POSITION),
        full.attribute(Mesh::ATTRIBUTE_NORMAL),
        full.attribute(Mesh::ATTRIBUTE_UV_0),
        full.attribute(Mesh::ATTRIBUTE_TANGENT),
    )
    else {
        return Vec::new();
    };
    let colors = match full.attribute(Mesh::ATTRIBUTE_COLOR) {
        Some(Float32x4(colors)) => Some(colors),
        _ => None,
    };
    let uvs1 = match full.attribute(Mesh::ATTRIBUTE_UV_1) {
        Some(Float32x2(uvs)) => Some(uvs),
        _ => None,
    };
    lods.iter()
        .skip(1)
        .filter(|lod| lod.indices.iter().all(|&i| (i as usize) < positions.len()))
        .map(|lod| {
            let mut remap = HashMap::new();
            let (mut p, mut n, mut u, mut t) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            let (mut c, mut u1) = (Vec::new(), Vec::new());
            let indices = lod
                .indices
                .iter()
                .map(|&i| {
                    *remap.entry(i).or_insert_with(|| {
                        p.push(positions[i as usize]);
                        n.push(normals[i as usize]);
                        u.push(uvs[i as usize]);
                        t.push(tangents[i as usize]);
                        if let Some(colors) = colors {
                            c.push(colors[i as usize]);
                        }
                        if let Some(uvs1) = uvs1 {
                            u1.push(uvs1[i as usize]);
                        }
                        p.len() as u32 - 1
                    })
                })
                .collect();
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, u)
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, t)
            .with_inserted_indices(Indices::U32(indices));
            if colors.is_some() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, c);
            }
            if uvs1.is_some() {
                mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, u1);
            }
            mesh
        })
        .collect()
}

// --- Skinned models (characters): the viewer's skinned loads ---

/// A bone of a model's skeleton in its bind pose (the viewer's `BoneBind`,
/// with the inverse of its model-space transform as baked).
#[derive(Clone, Debug)]
pub struct BoneBind {
    pub name: String,
    pub parent: Option<usize>,
    pub local: Transform,
    pub inverse_bind: Mat4,
}

/// One skinned mesh of a character with its character material.
#[derive(Clone, Debug)]
pub struct SkinnedPart {
    pub mesh: Handle<Mesh>,
    pub material: SkinnedMaterial,
    /// The BFRES shape's name, e.g. `Skin__Mt_Upper_Skin`.
    pub shape: Arc<str>,
}

#[derive(Clone, Debug)]
pub enum SkinnedMaterial {
    Character(Handle<CharacterMaterial>),
    Face(Handle<FaceMaterial>),
}
impl SkinnedMaterial {
    pub fn insert(&self, entity: &mut bevy::ecs::system::EntityCommands<'_>) {
        match self {
            Self::Character(handle) => {
                entity.insert(MeshMaterial3d(handle.clone()));
            }
            Self::Face(handle) => {
                entity.insert(MeshMaterial3d(handle.clone()));
            }
        }
    }
}

/// A character folder's units (`characters/models/<folder>`), decoded but
/// not yet on the GPU: the viewer's `CpuFolder` of a skinned load.
pub struct CpuSkinnedFolder {
    meshes: Vec<Mesh>,
    images: Vec<Image>,
    materials: Vec<CpuMaterial>,
    /// Unit name → its shapes.
    units: HashMap<String, Vec<CpuShape>>,
    /// Unit name → its skeleton.
    pub skeletons: HashMap<String, Vec<BoneBind>>,
}

/// Reads a character folder's units `units` and the textures they use
/// (the viewer's `load_folder` with `LoadOptions::skinned`, from `assets/`).
pub fn load_skinned_folder(dir: &Path, units: &[String]) -> Result<CpuSkinnedFolder, String> {
    let mut folder = CpuSkinnedFolder {
        meshes: Vec::new(),
        images: Vec::new(),
        materials: Vec::new(),
        units: HashMap::new(),
        skeletons: HashMap::new(),
    };
    let mut image_cache: HashMap<String, Option<(usize, Swizzle)>> = HashMap::new();
    for unit in units {
        let path = dir.join(format!("{unit}.glb"));
        let model = Model::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut material_indices = Vec::with_capacity(model.materials.len());
        for material in &model.materials {
            let info = &material.info;
            let textures = &info.textures;
            let mut map =
                |name: &Option<String>| texture(dir, name, &mut image_cache, &mut folder.images);
            let mut cpu = cpu_material(info, map(&textures.albedo));
            cpu.normal = map(&textures.normal);
            cpu.metal_roughness = map(&textures.metal_roughness);
            cpu.emissive = map(&textures.emissive);
            cpu.character_maps = map(&textures.character_maps);
            if let Some(face) = &info.layered_face {
                let mut indices = [0; 6];
                let mut swizzles = [Vec4::ZERO; 6];
                let mut biases = [0.0; 6];
                for (i, layer) in face.layers.iter().enumerate() {
                    let (source, swizzle) = swizzled_texture(
                        dir,
                        &Some(layer.texture.clone()),
                        &mut image_cache,
                        &mut folder.images,
                    )
                    .ok_or_else(|| format!("layered face texture {} missing", layer.texture))?;
                    let mut image = folder.images[source].clone();
                    let (sampler, bias) = face_sampler(layer.sampler)?;
                    image.sampler = sampler;
                    folder.images.push(image);
                    indices[i] = folder.images.len() - 1;
                    swizzles[i] = Vec4::from_array(swizzle.map(f32::from));
                    biases[i] = bias;
                }
                let cutoff = match cpu.alpha {
                    AlphaMode::Mask(value) => value,
                    _ => 0.0,
                };
                cpu.face = Some(CpuFace {
                    images: indices,
                    params: FaceParams {
                        composition: u32::from(
                            face.composition == asset_format::model::FaceComposition::Lip,
                        ),
                        colors: [0, 1, 3, 4].map(|i| Vec4::from_array(face.const_colors[i])),
                        swizzles,
                        bias: [
                            Vec4::new(biases[0], biases[1], biases[2], biases[3]),
                            Vec4::new(biases[4], biases[5], face.zprepass_alpha, cutoff),
                        ],
                    },
                });
                cpu.albedo = None;
                cpu.base_color = LinearRgba::WHITE;
                cpu.uv_transform = bevy::math::Affine2::IDENTITY;
            }
            // A mask that cannot be read would light the whole surface.
            if textures.emissive.is_some() && cpu.emissive.is_none() {
                cpu.emissive_color = LinearRgba::BLACK;
            }
            folder.materials.push(cpu);
            material_indices.push(folder.materials.len() - 1);
        }
        let mut shapes = Vec::new();
        for shape in &model.shapes {
            let Some(&material) = material_indices.get(shape.material as usize) else {
                continue;
            };
            let Some(mut mesh) = build_skinned_mesh(shape) else {
                continue;
            };
            if let Some(face) = &model.materials[shape.material as usize].info.layered_face {
                prepare_face_uvs(&mut mesh, shape, face)?;
            }
            folder.meshes.push(mesh);
            shapes.push(CpuShape {
                mesh: folder.meshes.len() - 1,
                material,
                name: Arc::from(shape.name.as_str()),
                radius: shape.radius,
                lods: Vec::new(),
            });
        }
        folder.units.insert(unit.clone(), shapes);
        folder.skeletons.insert(
            unit.clone(),
            model
                .skeleton
                .iter()
                .map(|bone| BoneBind {
                    name: bone.name.clone(),
                    parent: bone.parent.map(|p| p as usize),
                    local: Transform {
                        translation: Vec3::from(bone.translation),
                        rotation: Quat::from_array(bone.rotation),
                        scale: Vec3::from(bone.scale),
                    },
                    inverse_bind: Mat4::from_cols_array(&bone.inverse_bind),
                })
                .collect(),
        );
    }
    Ok(folder)
}

impl CpuSkinnedFolder {
    /// The units' parts on the GPU with their character materials (the
    /// viewer's `CpuFolder::upload` for a skinned load).
    pub fn upload(
        self,
        meshes: &mut Assets<Mesh>,
        images: &mut Assets<Image>,
        materials: &mut Assets<CharacterMaterial>,
        faces: &mut Assets<FaceMaterial>,
        shading: &CharacterShading,
        dynamic: &mut DynamicEmission,
    ) -> HashMap<String, Arc<Vec<SkinnedPart>>> {
        let mesh_handles: Vec<Handle<Mesh>> =
            self.meshes.into_iter().map(|m| meshes.add(m)).collect();
        let image_handles: Vec<Handle<Image>> =
            self.images.into_iter().map(|i| images.add(i)).collect();
        let image = |i: Option<usize>| i.map(|i| image_handles[i].clone());
        let channel = |second: bool| {
            if second {
                bevy::mesh::UvChannel::Uv1
            } else {
                bevy::mesh::UvChannel::Uv0
            }
        };
        let gloss = Gloss::CHARACTER;
        let material_handles: Vec<SkinnedMaterial> = self
            .materials
            .iter()
            .map(|m| {
                let base = StandardMaterial {
                    base_color_texture: image(m.albedo),
                    base_color: m.base_color.into(),
                    normal_map_texture: image(m.normal),
                    metallic_roughness_texture: image(m.metal_roughness),
                    emissive_texture: image(m.emissive),
                    emissive: m.emissive_color,
                    normal_map_channel: channel(m.second_uv.normal),
                    metallic_roughness_channel: channel(m.second_uv.specular),
                    alpha_mode: m.alpha,
                    double_sided: m.double_sided,
                    cull_mode: if m.double_sided {
                        None
                    } else {
                        Some(bevy::render::render_resource::Face::Back)
                    },
                    // The texture holds the values where there is one.
                    // SI-MAT-03: metal and roughness tables for Bevy PBR are ours.
                    perceptual_roughness: if m.metal_roughness.is_some() {
                        1.0
                    } else {
                        gloss.roughness.0
                    },
                    metallic: if m.metal_roughness.is_some() {
                        1.0
                    } else {
                        0.0
                    },
                    reflectance: gloss.reflectance,
                    uv_transform: m.uv_transform,
                    ..default()
                };
                let character = CharacterMaterial {
                    base,
                    extension: shading.for_material(
                        &m.look,
                        gloss,
                        image(m.character_maps),
                        m.second_uv.maps,
                    ),
                };
                if let Some(face) = &m.face {
                    return SkinnedMaterial::Face(faces.add(FaceMaterial {
                        base: character,
                        extension: FaceShading::new(
                            face.params.clone(),
                            face.images.map(|i| image_handles[i].clone()),
                        ),
                    }));
                }
                let handle = materials.add(character);
                if m.emission_exposure != DynamicExposure::None {
                    dynamic.characters.push((
                        handle.clone(),
                        m.emissive_color,
                        m.emission_exposure,
                    ));
                }
                SkinnedMaterial::Character(handle)
            })
            .collect();
        self.units
            .into_iter()
            .map(|(unit, shapes)| {
                let parts = shapes
                    .into_iter()
                    .map(|shape| SkinnedPart {
                        mesh: mesh_handles[shape.mesh].clone(),
                        material: material_handles[shape.material].clone(),
                        shape: shape.name,
                    })
                    .collect();
                (unit, Arc::new(parts))
            })
            .collect()
    }
}

/// Native vertex shader paths, evaluated for the fixed per-villager SRTs.
/// Skin/AO keep UV0; line/makeup use UV1; lash and brow keep separate
/// attributes for the layered material's vertex shader.
fn prepare_face_uvs(
    mesh: &mut Mesh,
    shape: &asset_format::model::Shape,
    face: &asset_format::model::LayeredFace,
) -> Result<(), String> {
    let matrices = face
        .matrices
        .as_ref()
        .ok_or("layered face needs UV matrices; rebake NPCs")?;
    let count = shape.positions.len();
    let apply = |m: &[f32; 6], [u, v]: [f32; 2]| {
        [
            m[2].mul_add(v, m[0].mul_add(u, m[4])),
            m[3].mul_add(v, m[1].mul_add(u, m[5])),
        ]
    };
    if face.composition == asset_format::model::FaceComposition::Lip {
        if shape.uvs1.len() != count || shape.uvs.len() != count {
            return Err("layered lips need UV0 and UV1; rebake NPCs".into());
        }
        // Program 8523 exports raw UV1.xy and SRT2(UV1).zw together.
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, shape.uvs1.clone());
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_1,
            shape
                .uvs1
                .iter()
                .map(|&uv| apply(&matrices[2], uv))
                .collect::<Vec<_>>(),
        );
        // Retain source UV0 for the still-unported native toon-specular path.
        // SI-LGT-17: lighting continues through the ordinary character shader.
        mesh.insert_attribute(ATTRIBUTE_UV_2, shape.uvs.clone());
        mesh.insert_attribute(ATTRIBUTE_UV_3, vec![[0.0; 2]; count]);
        return Ok(());
    }
    if [shape.uvs1.len(), shape.uvs2.len(), shape.uvs3.len()] != [count; 3] {
        return Err("layered face needs all four UV attributes; rebake NPCs".into());
    }
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_UV_1,
        shape
            .uvs1
            .iter()
            .map(|&uv| apply(&matrices[2], uv))
            .collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        ATTRIBUTE_UV_2,
        shape
            .uvs2
            .iter()
            .map(|&uv| apply(&matrices[5], apply(&matrices[0], uv)))
            .collect::<Vec<_>>(),
    );
    mesh.insert_attribute(
        ATTRIBUTE_UV_3,
        shape
            .uvs3
            .iter()
            .map(|&uv| apply(&matrices[4], apply(&matrices[1], uv)))
            .collect::<Vec<_>>(),
    );
    Ok(())
}

/// A skinned shape's mesh in model space, each vertex bound to up to four
/// bones (joint index = the unit's bone index), with the second UV set
/// where it was baked.
fn build_skinned_mesh(shape: &asset_format::model::Shape) -> Option<Mesh> {
    if shape.joints.len() != shape.positions.len() {
        return None;
    }
    let mut mesh = build_mesh(shape)?;
    if !shape.uvs1.is_empty() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, shape.uvs1.clone());
    }
    if !shape.uvs2.is_empty() {
        mesh.insert_attribute(ATTRIBUTE_UV_2, shape.uvs2.clone());
    }
    if !shape.uvs3.is_empty() {
        mesh.insert_attribute(ATTRIBUTE_UV_3, shape.uvs3.clone());
    }
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_JOINT_INDEX,
        bevy::mesh::VertexAttributeValues::Uint16x4(shape.joints.clone()),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, shape.weights.clone());
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_sampler_preserves_mirror_clamp_nearest_mip_and_signed_lod_bias() {
        use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler};
        let words = [
            1 | (2 << 3) | (1 << 9) | (1 << 12) | (1 << 17),
            (832 << 10) | (0xfc0 << 20),
            0,
        ];
        let (ImageSampler::Descriptor(sampler), bias) = face_sampler(words).unwrap() else {
            panic!("face must have an explicit sampler")
        };
        assert_eq!(sampler.address_mode_u, ImageAddressMode::MirrorRepeat);
        assert_eq!(sampler.address_mode_v, ImageAddressMode::ClampToEdge);
        assert_eq!(sampler.mag_filter, ImageFilterMode::Linear);
        assert_eq!(sampler.min_filter, ImageFilterMode::Linear);
        assert_eq!(sampler.mipmap_filter, ImageFilterMode::Nearest);
        assert_eq!(sampler.lod_max_clamp, 13.0);
        assert_eq!(sampler.anisotropy_clamp, 1);
        assert_eq!(bias, -1.0);
        assert!(face_sampler([words[0] | 7, words[1], 0]).is_err());
    }

    #[test]
    fn face_uv_paths_keep_layers_distinct_and_compose_in_native_order() {
        use asset_format::model::{FaceLayer, LayeredFace, Shape, TextureSrt};
        use bevy::mesh::VertexAttributeValues::Float32x2;
        let shape = Shape {
            positions: vec![[0.0; 3]],
            uvs: vec![[0.9, 0.8]],
            uvs1: vec![[0.5, 0.25]],
            uvs2: vec![[1.0, 2.0]],
            uvs3: vec![[3.0, 4.0]],
            ..Default::default()
        };
        let mut matrices = [[1.0, 0.0, 0.0, 1.0, 0.0, 0.0]; 6];
        matrices[0] = [2.0, 0.0, 0.0, 3.0, 0.0, 0.0];
        matrices[5] = [1.0, 0.0, 0.0, 1.0, 5.0, 7.0];
        matrices[1] = [1.0, 0.0, 0.0, 1.0, 11.0, 13.0];
        matrices[4] = [17.0, 0.0, 0.0, 19.0, 0.0, 0.0];
        matrices[2] = [0.0, 1.0, 1.0, 0.0, 0.0, 0.0];
        let mut face = LayeredFace {
            composition: asset_format::model::FaceComposition::Face,
            layers: std::array::from_fn(|_| FaceLayer {
                texture: String::new(),
                sampler: [0; 3],
            }),
            const_colors: [[1.0; 4]; 5],
            tex_srts: [TextureSrt {
                mode: 0,
                scale: [1.0; 2],
                rotation: 0.0,
                translation: [0.0; 2],
            }; 6],
            zprepass_alpha: 1.0,
            matrices: Some(matrices),
        };
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, shape.uvs.clone());
        prepare_face_uvs(&mut mesh, &shape, &face).unwrap();
        for (attribute, expected) in [
            (Mesh::ATTRIBUTE_UV_0, [0.9, 0.8]),
            (Mesh::ATTRIBUTE_UV_1, [0.25, 0.5]),
            (ATTRIBUTE_UV_2, [7.0, 13.0]),
            (ATTRIBUTE_UV_3, [238.0, 323.0]),
        ] {
            let Some(Float32x2(values)) = mesh.attribute(attribute) else {
                panic!("missing face UV attribute")
            };
            assert_eq!(values, &[expected]);
        }
        face.composition = asset_format::model::FaceComposition::Lip;
        let lip_shape = Shape {
            uvs2: vec![],
            uvs3: vec![],
            ..shape.clone()
        };
        prepare_face_uvs(&mut mesh, &lip_shape, &face).unwrap();
        for (attribute, expected) in [
            (Mesh::ATTRIBUTE_UV_0, [0.5, 0.25]),
            (Mesh::ATTRIBUTE_UV_1, [0.25, 0.5]),
            (ATTRIBUTE_UV_2, [0.9, 0.8]),
        ] {
            let Some(Float32x2(values)) = mesh.attribute(attribute) else {
                panic!("missing lip UV attribute")
            };
            assert_eq!(values, &[expected]);
        }
        face.matrices = None;
        assert!(prepare_face_uvs(&mut mesh, &shape, &face).is_err());
    }

    #[test]
    fn late_character_materials_receive_the_current_exposure() {
        let mut app = App::new();
        app.init_resource::<DynamicEmission>()
            .init_resource::<Assets<ObjectMaterial>>()
            .init_resource::<Assets<CharacterMaterial>>()
            .add_systems(Update, follow_dynamic_exposure);
        let material = || CharacterMaterial {
            base: StandardMaterial {
                emissive: LinearRgba::rgb(2.0, 1.0, 0.5),
                ..default()
            },
            extension: CharacterShading {
                params: crate::character_material::CharacterParams {
                    clouds: crate::clouds::CloudParams::without_shadows(),
                    kind: Vec4::ZERO,
                    behave: Vec4::ZERO,
                    gloss: Vec4::ZERO,
                },
                shadow_map: Handle::default(),
                look: Handle::default(),
                maps: None,
                cube_mean: Handle::default(),
            },
        };
        // The current daytime exposure is already cached before this load.
        app.update();
        let handle = app
            .world_mut()
            .resource_mut::<Assets<CharacterMaterial>>()
            .add(material());
        app.world_mut()
            .resource_mut::<DynamicEmission>()
            .characters
            .push((
                handle.clone(),
                LinearRgba::rgb(2.0, 1.0, 0.5),
                DynamicExposure::OneMinus,
            ));
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<CharacterMaterial>>()
                .get(&handle)
                .unwrap()
                .base
                .emissive,
            LinearRgba::rgb(0.0, 0.0, 0.0)
        );
        // A later, day-lit load must be updated without disturbing this one.
        let day = app
            .world_mut()
            .resource_mut::<Assets<CharacterMaterial>>()
            .add(material());
        app.world_mut()
            .resource_mut::<DynamicEmission>()
            .characters
            .push((
                day.clone(),
                LinearRgba::rgb(3.0, 2.0, 1.0),
                DynamicExposure::Exposure,
            ));
        app.update();
        let materials = app.world().resource::<Assets<CharacterMaterial>>();
        assert_eq!(
            materials.get(&day).unwrap().base.emissive,
            LinearRgba::rgb(3.0, 2.0, 1.0)
        );
        assert_eq!(
            materials.get(&handle).unwrap().base.emissive,
            LinearRgba::rgb(0.0, 0.0, 0.0)
        );
    }

    /// Reads every baked unit (`cargo test -p render baked_models --
    /// --ignored`, after `cargo bake --only objects,models`).
    #[test]
    #[ignore]
    fn baked_models_load() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let table: ActorModels = asset_format::read_ron(&assets.join(paths::ACTOR_MODELS)).unwrap();
        let mut folders: HashMap<String, BTreeSet<String>> = HashMap::new();
        for model in table.actors.values().flatten() {
            folders
                .entry(model.folder.clone())
                .or_default()
                .extend(model.units.iter().cloned());
        }
        let (mut units, mut shapes) = (0, 0);
        for (folder, names) in &folders {
            let names: Vec<String> = names.iter().cloned().collect();
            let cpu = load_folder(&assets.join(paths::MODELS).join(folder), &names, None);
            units += cpu.units.len();
            shapes += cpu.units.values().map(Vec::len).sum::<usize>();
        }
        assert!(units > 0 && shapes > 0);
        println!("{} folders, {units} units, {shapes} shapes", folders.len());
    }
}
