//! The placed objects' models: each model unit an actor in reach draws
//! (`objects/actors.ron`) as `models/<folder>/<unit>.glb`, and its folder's
//! textures as KTX2 (see `asset_format::model`).
//!
//! The geometry and the derived textures are the original renderer's
//! `models::load_folder` for static meshes, moved here from load time:
//! `build_mesh` (bind pose applied, tangents generated as Bevy's
//! `Mesh::generate_tangents`), the LODs `lod_meshes` keeps,
//! `normal_map_image`, `gloss_map_image`, `combine_alpha`,
//! `metal_roughness`, `emission`, and `gpu_image`'s rounding of odd
//! block-compressed sizes. What the renderer makes of the material (alpha,
//! colours, Bevy's material) stays in the renderer.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_format::model::{
    FaceComposition, FaceLayer, LayeredFace, Lod, Material as BakedMaterial, MaterialBehave,
    MaterialInfo, MaterialLook, Model as BakedModel, RenderState, SecondUv, Shape as BakedShape,
    TextureSrt,
};
use asset_format::objects::ActorModels;
use asset_format::paths;
use asset_format::texture::{Format, IDENTITY_SWIZZLE, Texture};
use botw_formats::actor::ActorPacks;
use botw_formats::bfres::gx2;
use botw_formats::bfres::maps::{Channel, Term};
use botw_formats::bfres::model::{Material, Model, Shape};
use botw_formats::bfres::{Bfres, TextureImage, assemble_texture, bc};
use botw_formats::content::ContentRoots;
use glam::{EulerRot, Mat4, Quat, Vec3};

use crate::objects::parallel;

/// Zstandard level of the models' KTX2 files: the derived RGBA8 maps
/// (metal-roughness above all) are mostly flat.
const KTX2_ZSTD_LEVEL: i32 = 9;

/// What one folder's bake wrote.
#[derive(Default)]
pub struct FolderStats {
    pub units: usize,
    pub missing_units: Vec<String>,
    pub textures: usize,
    pub bytes: u64,
}

pub fn bake(roots: &ContentRoots, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let actors: ActorModels = asset_format::read_ron(&out.join(paths::ACTOR_MODELS))
        .map_err(|e| format!("{e} (run the objects step first)"))?;
    let mut folders: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for refs in actors.actors.values() {
        for model in refs {
            folders
                .entry(model.folder.clone())
                .or_default()
                .extend(model.units.iter().cloned());
        }
    }
    let title_bg = roots
        .find("Pack/TitleBG.pack")
        .map(|p| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())))
        .transpose()?
        .map(Arc::new);
    let packs = ActorPacks::new(roots.clone(), title_bg);
    // Big models are split into `<folder>-NN.sbfres` parts.
    let model_files: Vec<String> = roots
        .list_dir("Model")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let folders: Vec<(String, BTreeSet<String>)> = folders.into_iter().collect();
    let dir = out.join(paths::MODELS);
    // Units and textures no longer in reach go.
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let results = parallel(&folders, |(folder, units)| {
        bake_folder(
            &packs,
            &model_files,
            folder,
            units,
            &dir.join(folder),
            &LoadOptions::default(),
        )
    });
    let mut total = FolderStats::default();
    let mut failed = 0;
    for ((folder, _), result) in folders.iter().zip(results) {
        match result {
            Ok(stats) => {
                total.units += stats.units;
                total.textures += stats.textures;
                total.bytes += stats.bytes;
                for unit in stats.missing_units {
                    println!("warning: model {folder}: no unit {unit}");
                }
            }
            // The viewer leaves such a folder out (`FolderEntry::Failed`).
            Err(error) => {
                failed += 1;
                println!("warning: model {folder}: {error}");
            }
        }
    }
    println!(
        "models: {} folders ({failed} failed), {} units, {} textures, {:.0} MB in {:.1} s",
        folders.len(),
        total.units,
        total.textures,
        total.bytes as f64 / 1e6,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// How to bake a folder beyond the defaults (static meshes, own textures):
/// the viewer's `models::LoadOptions` (its `albedo_swap`, for colour
/// variants of enemies, is not needed yet).
#[derive(Clone, Debug, Default)]
pub struct LoadOptions {
    /// Keep vertices bound to bones (see [`build_skinned_mesh`]) and bake
    /// the skeletons, instead of baking the bind pose in; the characters'
    /// material maps (see `asset_format::model::MaterialTextures`).
    pub skinned: bool,
    /// Folders to look in for textures missing from the folder's own files
    /// (outfits use Link's skin and hair from `Link.Tex1`).
    pub shared_textures: Vec<String>,
    /// Changes to the models' materials and textures made up for them (a
    /// UMii villager's colours, see `npcs`).
    pub edits: Option<MaterialEdits>,
    pub face_trig: Option<Arc<crate::texture_srt::TextureSrtTable>>,
}

/// A texture lookup in a folder's files.
pub type FindTexture<'a> = &'a dyn Fn(&str) -> Option<TextureImage>;

/// Material changes before baking: `material(model, material)` edits each
/// material; `texture(name, find)` makes the textures they then name that
/// the files lack (`find` looks a texture up in the files).
#[derive(Clone)]
pub struct MaterialEdits {
    pub material: Arc<dyn Fn(&str, &mut Material) + Send + Sync>,
    pub texture: Arc<dyn Fn(&str, FindTexture) -> Option<TextureImage> + Send + Sync>,
}

impl std::fmt::Debug for MaterialEdits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MaterialEdits")
    }
}

/// The model files of a folder: `<folder>.sbfres`, or its numbered parts.
pub(crate) fn model_files_of(files: &[String], folder: &str) -> Vec<String> {
    let whole = format!("{folder}.sbfres");
    if files.contains(&whole) {
        return vec![whole];
    }
    let prefix = format!("{folder}-");
    let mut parts: Vec<String> = files
        .iter()
        .filter(|f| {
            f.strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(".sbfres"))
                .is_some_and(|n| n.len() == 2 && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .cloned()
        .collect();
    parts.sort();
    parts
}

pub(crate) fn read_bfres(packs: &ActorPacks, relative: &str) -> Option<Vec<u8>> {
    packs.file(relative).ok().flatten()
}

fn parse_bfres(bytes: &Option<Vec<u8>>) -> Option<Bfres<'_>> {
    bytes.as_deref().and_then(|b| Bfres::parse(b).ok())
}

/// Bakes the `units` of a BFRES folder (the viewer's `load_folder`).
pub fn bake_folder(
    packs: &ActorPacks,
    model_files: &[String],
    folder: &str,
    units: &BTreeSet<String>,
    dir: &Path,
    options: &LoadOptions,
) -> Result<FolderStats, String> {
    // Not loose: maybe inside `TitleBG.pack`, which holds no split models.
    let files = model_files_of(model_files, folder);
    let whole = [format!("{folder}.sbfres")];
    let files = if files.is_empty() {
        &whole[..]
    } else {
        &files[..]
    };
    let mut models = Vec::new();
    // The files' material animations (texture SRTs that scroll water).
    let mut animations = Vec::new();
    for file in files {
        let bytes = read_bfres(packs, &format!("Model/{file}"))
            .ok_or_else(|| format!("{file} not found"))?;
        let bfres = Bfres::parse(&bytes).map_err(|e| format!("{file}: {e}"))?;
        models.extend(bfres.models().map_err(|e| format!("{file}: {e}"))?);
        animations.extend(crate::xlu::srt_animations(&bytes));
    }
    if let Some(edits) = &options.edits {
        for model in &mut models {
            for material in &mut model.materials {
                (edits.material)(&model.name, material);
            }
        }
    }

    // Level 0 of each texture is in Tex1 (base game), the mips in Tex2
    // (update). Folders with many textures continue Tex1 in `Tex1.1`,
    // whose mips are in the same Tex2. Then the shared folders' textures.
    let texture_files: Vec<_> = std::iter::once(folder)
        .chain(options.shared_textures.iter().map(String::as_str))
        .map(|f| {
            let level0 = [
                format!("Model/{f}.Tex1.sbfres"),
                format!("Model/{f}.Tex1.1.sbfres"),
            ]
            .map(|p| read_bfres(packs, &p));
            (level0, read_bfres(packs, &format!("Model/{f}.Tex2.sbfres")))
        })
        .collect();
    let texture_sets: Vec<(Option<Bfres>, Option<Bfres>)> = texture_files
        .iter()
        .flat_map(|(level0, tex2)| level0.iter().map(move |tex1| (tex1, tex2)))
        .filter(|(tex1, _)| tex1.is_some())
        .map(|(tex1, tex2)| (parse_bfres(tex1), parse_bfres(tex2)))
        .collect();
    let from_files = |name: &str| -> Option<TextureImage> {
        texture_sets.iter().find_map(|(tex1, tex2)| {
            let level0 = tex1.as_ref()?.texture(name).ok().flatten()?;
            let mips = tex2.as_ref().and_then(|t| t.texture(name).ok().flatten());
            assemble_texture(&level0, mips.as_ref()).ok()
        })
    };
    let find_texture = |name: &str| -> Option<TextureImage> {
        from_files(name).or_else(|| (options.edits.as_ref()?.texture)(name, &from_files))
    };

    let mut stats = FolderStats::default();
    let mut textures = Textures {
        dir: dir.to_owned(),
        cache: HashMap::new(),
        written: 0,
        bytes: 0,
        face_trig: options.face_trig.clone(),
    };
    for model in &models {
        if !units.contains(&model.name) {
            continue;
        }
        let baked = bake_model(
            model,
            &find_texture,
            &mut textures,
            options.skinned,
            &animations,
        )?;
        let bytes = baked.to_glb().map_err(|e| format!("{}: {e}", model.name))?;
        let path = dir.join(format!("{}.glb", model.name));
        asset_format::write(&path, &bytes).map_err(|e| e.to_string())?;
        stats.units += 1;
        stats.bytes += bytes.len() as u64;
    }
    let baked: BTreeSet<&str> = models.iter().map(|m| m.name.as_str()).collect();
    stats.missing_units = units
        .iter()
        .filter(|u| !baked.contains(u.as_str()))
        .cloned()
        .collect();
    stats.textures = textures.written;
    stats.bytes += textures.bytes;
    Ok(stats)
}

/// A model's shapes and the materials they use; `skinned` keeps vertices
/// bound to the bones and bakes the skeleton (the viewer's skinned loads);
/// `animations` are its file's material animations.
fn bake_model(
    model: &Model,
    find_texture: &dyn Fn(&str) -> Option<TextureImage>,
    textures: &mut Textures,
    skinned: bool,
    animations: &[crate::xlu::MaterialAnimation],
) -> Result<BakedModel, String> {
    let auto = crate::xlu::MaterialAnimation::auto_for(animations, &model.name);
    let bones = bone_world_matrices(model);
    let mut baked = BakedModel {
        name: model.name.clone(),
        skeleton: if skinned {
            bone_binds(model, &bones)
        } else {
            Vec::new()
        },
        ..Default::default()
    };
    // BFRES material index → baked material index.
    let mut materials: HashMap<usize, u32> = HashMap::new();
    // Skinned loads read the material first: it says whether the mesh
    // needs the second UV set.
    let mut infos: HashMap<usize, MaterialInfo> = HashMap::new();
    for shape in &model.shapes {
        // An index past the model's materials takes its first (the viewer
        // falls back to its folder's first).
        let source = if (shape.material as usize) < model.materials.len() {
            shape.material as usize
        } else {
            0
        };
        let Some(material) = model.materials.get(source) else {
            continue;
        };
        let mesh = if skinned {
            let info = match infos.get(&source) {
                Some(info) => info,
                None => {
                    let info = bake_material(material, find_texture, textures, true)?;
                    infos.entry(source).or_insert(info)
                }
            };
            build_skinned_mesh(model, shape, &bones, info.second_uv.any())
        } else {
            build_mesh(model, shape, &bones, false)
        };
        let Some(mut mesh) = mesh else {
            continue;
        };
        let index = match materials.get(&source) {
            Some(&index) => index,
            None => {
                let mut info = match infos.get(&source).cloned() {
                    Some(info) => info,
                    None => bake_material(material, find_texture, textures, false)?,
                };
                if let Some(xlu) = &mut info.xlu {
                    xlu.animation = auto.and_then(|a| a.material(&material.name)).cloned();
                }
                baked.materials.push(BakedMaterial {
                    name: material.name.clone(),
                    info,
                });
                let index = baked.materials.len() as u32 - 1;
                materials.insert(source, index);
                index
            }
        };
        mesh.material = index;
        // Only the translucent G-buffer programs read the vertex colours.
        // Skinned models also keep UV1 for layered face shader paths which
        // are not represented by the ordinary maps' `second_uv` flags.
        let info = &baked.materials[index as usize].info;
        if info.xlu.is_none() {
            mesh.colors.clear();
            if !skinned && !info.second_uv.any() {
                mesh.uvs1.clear();
            }
        }
        baked.shapes.push(mesh);
    }
    Ok(baked)
}

// --- Geometry (the viewer's build_mesh, lod_meshes) ---

/// A rotation from BFRES Euler angles (X, Y, Z applied in that order).
pub(crate) fn euler_rotation(x: f32, y: f32, z: f32) -> Quat {
    Quat::from_euler(EulerRot::ZYX, z, y, x)
}

/// A bone's rotation in its parent's space.
fn bone_rotation(bone: &botw_formats::bfres::model::Bone) -> Quat {
    if bone.euler {
        let [x, y, z, _] = bone.rotation;
        euler_rotation(x, y, z)
    } else {
        Quat::from_array(bone.rotation).normalize()
    }
}

fn bone_local(bone: &botw_formats::bfres::model::Bone) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::from(bone.scale),
        bone_rotation(bone),
        Vec3::from(bone.translation),
    )
}

/// The skeleton in its bind pose (the viewer's `bone_binds`): each bone's
/// local transform, and the inverse of its model-space one.
fn bone_binds(model: &Model, world: &[Mat4]) -> Vec<asset_format::model::Bone> {
    model
        .bones
        .iter()
        .zip(world)
        .map(|(bone, world)| asset_format::model::Bone {
            name: bone.name.clone(),
            parent: bone.parent.map(u32::from),
            translation: bone.translation,
            rotation: bone_rotation(bone).to_array(),
            scale: bone.scale,
            inverse_bind: world.inverse().to_cols_array(),
        })
        .collect()
}

/// A mesh in model space with each vertex bound to up to four of the
/// model's bones (joint index = bone index), level 0 only (the viewer's
/// `build_skinned_mesh`). Rigid shapes and single-bone vertices are moved
/// from their bone's space into model space first (by [`build_mesh`]).
fn build_skinned_mesh(
    model: &Model,
    shape: &Shape,
    bones: &[Mat4],
    second_uv: bool,
) -> Option<BakedShape> {
    let mut mesh = build_mesh(model, shape, bones, second_uv)?;
    mesh.lods.truncate(1);
    let buffer = model.vertex_buffers.get(shape.vertex_buffer as usize)?;
    // The native layered face shader reads all four UV attributes. Keep the
    // extra sets verbatim; applying their SRTs belongs to the material shader.
    mesh.uvs2 = buffer.attribute("_u2").map_or_else(Vec::new, |u| {
        u.values.iter().map(|v| [v[0], v[1]]).collect()
    });
    mesh.uvs3 = buffer.attribute("_u3").map_or_else(Vec::new, |u| {
        u.values.iter().map(|v| [v[0], v[1]]).collect()
    });
    let count = mesh.positions.len();
    let bone_of = |matrix: f32| {
        model
            .matrix_to_bone
            .get(matrix as usize)
            .copied()
            .unwrap_or(0)
    };
    (mesh.joints, mesh.weights) = match (shape.skin_count, buffer.attribute("_i0")) {
        (0, _) | (_, None) => (
            vec![[shape.bone, 0, 0, 0]; count],
            vec![[1.0, 0.0, 0.0, 0.0]; count],
        ),
        (1, Some(indices)) => (
            indices
                .values
                .iter()
                .map(|i| [bone_of(i[0]), 0, 0, 0])
                .collect(),
            vec![[1.0, 0.0, 0.0, 0.0]; count],
        ),
        (n, Some(indices)) => {
            let n = (n as usize).min(4);
            let weights = buffer.attribute("_w0");
            (0..count)
                .map(|v| {
                    let i = indices.values[v];
                    let mut joints = [0u16; 4];
                    let mut w = [0.0f32; 4];
                    for k in 0..n {
                        joints[k] = bone_of(i[k]);
                        w[k] = weights.map_or(if k == 0 { 1.0 } else { 0.0 }, |wt| wt.values[v][k]);
                    }
                    let sum: f32 = w.iter().sum();
                    if sum > 1e-6 {
                        w.iter_mut().for_each(|x| *x /= sum);
                    } else {
                        w = [1.0, 0.0, 0.0, 0.0];
                    }
                    (joints, w)
                })
                .unzip()
        }
    };
    Some(mesh)
}

/// Each bone's model-space transform.
fn bone_world_matrices(model: &Model) -> Vec<Mat4> {
    let mut world: Vec<Mat4> = Vec::with_capacity(model.bones.len());
    for bone in &model.bones {
        let local = bone_local(bone);
        let parent = bone
            .parent
            .and_then(|p| world.get(p as usize))
            .copied()
            .unwrap_or(Mat4::IDENTITY);
        world.push(parent * local);
    }
    world
}

/// A shape's mesh in model space with its levels of detail (the viewer's
/// `build_mesh`, and the LODs its `lod_meshes` keeps: triangle lists whose
/// indices are in range); `second_uv` adds the second UV set (`_u1`, or
/// the first where there is none).
fn build_mesh(model: &Model, shape: &Shape, bones: &[Mat4], second_uv: bool) -> Option<BakedShape> {
    let lod = shape.lods.first()?;
    if lod.primitive != 4 {
        return None; // Only triangle lists so far.
    }
    let buffer = model.vertex_buffers.get(shape.vertex_buffer as usize)?;
    let positions = buffer.attribute("_p0")?;
    let bone_indices = buffer.attribute("_i0");
    // Rigid shapes live in their bone's space; single-bone skinning names the
    // bone per vertex; smooth skins are already in model space.
    let transform_of = |vertex: usize| -> Mat4 {
        match (shape.skin_count, bone_indices) {
            (0, _) => bones
                .get(shape.bone as usize)
                .copied()
                .unwrap_or(Mat4::IDENTITY),
            (1, Some(indices)) => {
                let matrix = indices.values[vertex][0] as usize;
                model
                    .matrix_to_bone
                    .get(matrix)
                    .and_then(|&b| bones.get(b as usize))
                    .copied()
                    .unwrap_or(Mat4::IDENTITY)
            }
            _ => Mat4::IDENTITY,
        }
    };
    let count = positions.values.len();
    let transforms: Vec<Mat4> = (0..count).map(transform_of).collect();
    let position: Vec<[f32; 3]> = (0..count)
        .map(|v| {
            transforms[v]
                .transform_point3(Vec3::from_slice(&positions.values[v][..3]))
                .to_array()
        })
        .collect();
    let normal: Vec<[f32; 3]> = match buffer.attribute("_n0") {
        Some(n) => (0..count)
            .map(|v| {
                transforms[v]
                    .transform_vector3(Vec3::from_slice(&n.values[v][..3]))
                    .normalize_or(Vec3::Y)
                    .to_array()
            })
            .collect(),
        None => vec![[0.0, 1.0, 0.0]; count],
    };
    let uv: Vec<[f32; 2]> = match buffer.attribute("_u0") {
        Some(u) => u.values.iter().map(|v| [v[0], v[1]]).collect(),
        None => vec![[0.0, 0.0]; count],
    };
    let indices: Vec<u32> = lod
        .indices
        .iter()
        .copied()
        .filter(|&i| (i as usize) < count)
        .collect();
    if indices.len() != lod.indices.len() {
        return None;
    }
    // Tangents for normal maps: the model's own (w = handedness), else
    // generated, else a placeholder so every mesh fits a normal-mapped material.
    let tangents = match buffer.attribute("_t0") {
        Some(t) => (0..count)
            .map(|v| {
                let [x, y, z, w] = t.values[v];
                let d = transforms[v]
                    .transform_vector3(Vec3::new(x, y, z))
                    .normalize_or(Vec3::X);
                [d.x, d.y, d.z, if w < 0.0 { -1.0 } else { 1.0 }]
            })
            .collect(),
        None => generate_tangents(&position, &normal, &uv, &indices)
            .unwrap_or_else(|| vec![[1.0, 0.0, 0.0, 1.0]; count]),
    };
    let colors = match buffer.attribute("_c0") {
        Some(c) => c.values.clone(),
        None => Vec::new(),
    };
    // The second UV set: `_u1` where the shape has it; `second_uv` (the
    // characters' maps) falls back to the first where there is none.
    let uvs1: Vec<[f32; 2]> = match buffer.attribute("_u1") {
        Some(u) => u.values.iter().map(|v| [v[0], v[1]]).collect(),
        None if second_uv => uv.clone(),
        None => Vec::new(),
    };
    let radius = position
        .iter()
        .map(|v| Vec3::from(*v).length())
        .fold(0.0, f32::max);
    let mut lods = vec![Lod { level: 0, indices }];
    // The coarser levels (the viewer's `lod_meshes`).
    for (level, lod) in shape.lods.iter().enumerate().skip(1) {
        if lod.primitive == 4 && lod.indices.iter().all(|&i| (i as usize) < count) {
            lods.push(Lod {
                level: level as u32,
                indices: lod.indices.clone(),
            });
        }
    }
    Some(BakedShape {
        name: shape.name.clone(),
        material: 0,
        radius,
        positions: position,
        normals: normal,
        uvs: uv,
        tangents,
        colors,
        uvs1,
        lods,
        uvs2: Vec::new(),
        uvs3: Vec::new(),
        joints: Vec::new(),
        weights: Vec::new(),
    })
}

/// Bevy's `Mesh::generate_tangents` (`bevy_mesh::mikktspace`) over a
/// triangle list: mikktspace with the handedness flipped; vertices no
/// triangle uses keep zero.
fn generate_tangents(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    uvs: &[[f32; 2]],
    indices: &[u32],
) -> Option<Vec<[f32; 4]>> {
    struct Geometry<'a> {
        indices: &'a [u32],
        positions: &'a [[f32; 3]],
        normals: &'a [[f32; 3]],
        uvs: &'a [[f32; 2]],
        tangents: Vec<[f32; 4]>,
    }
    impl Geometry<'_> {
        fn index(&self, face: usize, vert: usize) -> usize {
            self.indices[face * 3 + vert] as usize
        }
    }
    impl bevy_mikktspace::Geometry for Geometry<'_> {
        fn num_faces(&self) -> usize {
            self.indices.len() / 3
        }
        fn num_vertices_of_face(&self, _: usize) -> usize {
            3
        }
        fn position(&self, face: usize, vert: usize) -> [f32; 3] {
            self.positions[self.index(face, vert)]
        }
        fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
            self.normals[self.index(face, vert)]
        }
        fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
            self.uvs[self.index(face, vert)]
        }
        fn set_tangent(
            &mut self,
            tangent_space: Option<bevy_mikktspace::TangentSpace>,
            face: usize,
            vert: usize,
        ) {
            let idx = self.index(face, vert);
            self.tangents[idx] = tangent_space.unwrap_or_default().tangent_encoded();
        }
    }
    let mut geometry = Geometry {
        indices,
        positions,
        normals,
        uvs,
        tangents: vec![[0.0; 4]; positions.len()],
    };
    bevy_mikktspace::generate_tangents(&mut geometry).ok()?;
    // mikktspace seems to assume left-handedness so we can flip the sign to correct for this
    for tangent in &mut geometry.tangents {
        tangent[3] = -tangent[3];
    }
    Some(geometry.tangents)
}

// --- Materials (the viewer's load_folder, cpu_material, material_look) ---

/// Preserve the layered face's source inputs rather than baking them at one
/// UV. Texture assignments are resolved by shader slot, not material order.
fn layered_face(
    material: &Material,
    find_texture: &dyn Fn(&str) -> Option<TextureImage>,
    textures: &mut Textures,
) -> Result<Option<LayeredFace>, String> {
    let (composition, slots) =
        if material.name == "Mt_Face" && material.sampler_texture("_a4").is_some() {
            (FaceComposition::Face, [0, 1, 2, 3, 4, 5])
        } else if is_layered_lip(material) {
            (FaceComposition::Lip, [0, 1, 3, 3, 4, 5])
        } else {
            return Ok(None);
        };
    let Some((colors, srts, alpha)) = face_parameters(material) else {
        return Err("layered face: incomplete shader parameters".into());
    };
    let mut layers = Vec::new();
    for slot in slots {
        let (sampler, name) = material
            .slot_texture(slot)
            .ok_or_else(|| format!("layered face: missing shader slot {slot}"))?;
        let words = material
            .samplers
            .iter()
            .find(|(s, _)| s == sampler)
            .ok_or_else(|| format!("layered face: missing sampler {sampler}"))?
            .1;
        let texture = textures
            .get(name, || game_texture(&find_texture(name)?))?
            .ok_or_else(|| format!("layered face: missing texture {name}"))?;
        layers.push(FaceLayer {
            texture,
            sampler: words,
        });
    }
    let matrices = textures
        .face_trig
        .as_ref()
        .map(|table| {
            srts.iter()
                .map(|srt| table.matrix(*srt))
                .collect::<Result<Vec<_>, _>>()?
                .try_into()
                .map_err(|_| "layered face: expected six matrices".to_string())
        })
        .transpose()?;
    Ok(Some(LayeredFace {
        composition,
        matrices,
        layers: layers.try_into().expect("six face slots"),
        const_colors: colors,
        tex_srts: srts,
        zprepass_alpha: alpha,
    }))
}

/// Identify the recovered UMii lip family by its shader-to-sampler assignments.
pub(crate) fn is_layered_lip(material: &Material) -> bool {
    material.name == "Mt_Lip"
        && [(0, "_a2"), (1, "_a0"), (3, "_ao0"), (4, "_a1"), (5, "_cm0")]
            .iter()
            .all(|&(slot, sampler)| {
                material
                    .slot_texture(slot)
                    .is_some_and(|(name, _)| name == sampler)
            })
}

fn face_parameters(material: &Material) -> Option<([[f32; 4]; 5], [TextureSrt; 6], f32)> {
    let colors = (0..5)
        .map(|i| {
            material
                .shader_param(&format!("const_color{i}"))?
                .try_into()
                .ok()
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let srts = (0..6)
        .map(|i| {
            let param = material
                .shader_params
                .iter()
                .find(|p| p.name == format!("tex_srt{i}"))?;
            let &[mode, sx, sy, rotation, tx, ty] = param.values.as_slice() else {
                return None;
            };
            if param.kind != 30 {
                return None;
            }
            Some(TextureSrt {
                // botw-formats preserves kind 30's words as f32. Recover the
                // integer bits here; a numeric cast would erase modes 1 and 2.
                mode: mode.to_bits(),
                scale: [sx, sy],
                rotation,
                translation: [tx, ty],
            })
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    let alpha = *material.shader_param("gsys_xlu_zprepass_alpha")?.first()?;
    Some((colors, srts, alpha))
}

/// A material's facts and textures; `skinned` (characters) as the viewer's
/// skinned loads: no gloss or translucency maps, the characters' extra
/// maps and second UV set, metal and roughness by the characters' gloss.
fn bake_material(
    material: &Material,
    find_texture: &dyn Fn(&str) -> Option<TextureImage>,
    textures: &mut Textures,
    skinned: bool,
) -> Result<MaterialInfo, String> {
    let mut info = material_info(material);
    if skinned {
        info.layered_face = layered_face(material, find_texture, textures)?;
    }
    // Rocks and cliffs sample a layer of the terrain's material array.
    if material.texture("tma").is_some() {
        info.terrain_layer = Some(
            material
                .shader_param("texture_array_index0")
                .and_then(|v| v.first())
                .map_or(0, |&l| l as u32),
        );
        emission(material, find_texture, textures, &mut info)?;
        return Ok(info);
    }
    let albedo = material.sampler_texture("_a0").map(str::to_owned);
    let mask = material.sampler_texture("_ms0").map(str::to_owned);
    if let Some(a) = &albedo {
        let stem = match &mask {
            Some(m) => format!("{a}+{m}"),
            None => a.clone(),
        };
        info.textures.albedo = textures.get(&stem, || {
            let image = find_texture(a)?;
            match mask.as_deref().and_then(find_texture) {
                Some(mask) => combine_alpha(&image, &mask),
                None => game_texture(&image),
            }
        })?;
    }
    if let Some(n) = material.sampler_texture("_n0") {
        info.textures.normal = match find_texture(n) {
            Some(image) if matches!(image.format, gx2::Format::Bc5 { .. }) => {
                textures.get(n, || game_texture(&image))?
            }
            Some(image) => textures.get(&format!("{n}.nrm"), || normal_map_texture(&image))?,
            None => None,
        };
    }
    // Placed objects' gloss, for the field's lighting: the blue of shader
    // slot `_s0` where it holds the normal map (the characters' comes from
    // their specular masks).
    if !skinned
        && material.shader_option("uking_grossy_color") == Some("402")
        && let Some(("_n0", n)) = material.slot_texture(1)
    {
        info.textures.gloss = textures.get(&format!("{n}.gloss"), || {
            gloss_map_texture(&find_texture(n)?)
        })?;
    }
    // Leaves lit through from behind: the texture of shader slot 2.
    if !skinned
        && info.look.leaf_light.is_some()
        && let Some((_, t)) = material.slot_texture(2)
    {
        info.textures.translucency = textures.get(t, || game_texture(&find_texture(t)?))?;
    }
    // Faces lay their normal and specular maps out on the second UV set;
    // only characters get it (static meshes' coarser levels of detail do
    // not carry it).
    let gloss = if skinned {
        let specular = material
            .specular_mask()
            .and_then(|t| t.texture)
            .map(|(sampler, _, _)| sampler);
        let maps = character_maps(material, find_texture, textures)?;
        info.second_uv = SecondUv {
            normal: texcoord(material, "_n0") == 1,
            specular: specular.is_some_and(|s| texcoord(material, &s) == 1),
            maps: maps.as_ref().is_some_and(|(_, uv)| *uv == 1),
        };
        info.textures.character_maps = maps.map(|(name, _)| name);
        CHARACTER_GLOSS
    } else {
        OBJECT_GLOSS
    };
    metal_roughness(material, gloss, find_texture, textures, &mut info)?;
    emission(material, find_texture, textures, &mut info)?;
    info.xlu = crate::xlu::look(material, find_texture, textures)?;
    Ok(info)
}

/// The UV set (`uking_textureN_texcoord`: 0 first, 1 second; 6 and 7 are
/// procedural, e.g. an eye's) the shader reads the material's sampler
/// `sampler` with, found through the shader slot it is assigned to (the
/// viewer's `texcoord`).
fn texcoord(material: &Material, sampler: &str) -> u32 {
    (0..8)
        .find(|&slot| {
            material
                .slot_texture(slot)
                .is_some_and(|(s, _)| s == sampler)
        })
        .and_then(|slot| material.shader_option(&format!("uking_texture{slot}_texcoord")))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// A character material's extra maps as one texture (`cm_…`), and the UV
/// set they are read with (the first map's): the viewer's `character_maps`.
/// - red: ambient occlusion (`_ao0`, e.g. `Link_Head_AO`, its red channel;
///   faces read it with the second UV set);
/// - green: the eyes' shadow (`_sd0`, `Link_Eyeball_*_Shadow_Alb`: white
///   where the eyeball is lit, grey under the upper lid);
/// - blue: the skin's transmission (the alpha of `_ao0`, when the material
///   lets light through, `MaterialLook::transmission`), else black.
///
/// White where a map is missing; none without either.
fn character_maps(
    material: &Material,
    find_texture: &dyn Fn(&str) -> Option<TextureImage>,
    textures: &mut Textures,
) -> Result<Option<(String, u32)>, String> {
    let ao = material.texture("_ao0");
    let eye_shadow = material.texture("_sd0");
    if ao.is_none() && eye_shadow.is_none() {
        return Ok(None);
    }
    let sampler = if ao.is_some() { "_ao0" } else { "_sd0" };
    let transmission = material_look(material, false).transmission;
    let stem = format!(
        "cm_{}+{}{}",
        ao.unwrap_or("-"),
        eye_shadow.unwrap_or("-"),
        if transmission { "+t" } else { "" }
    );
    let texture = textures.get(&stem, || {
        let mask = |name: Option<&str>| name.and_then(|n| Mask::new(&find_texture(n)?, Channel::R));
        let through = ao
            .filter(|_| transmission)
            .and_then(|n| Mask::new(&find_texture(n)?, Channel::A));
        let (ao, eye_shadow) = (mask(ao), mask(eye_shadow));
        let (w, h) = [&ao, &eye_shadow]
            .into_iter()
            .flatten()
            .map(|m| (m.width, m.height))
            .max()?;
        let at = |m: &Option<Mask>, x: u32, y: u32| m.as_ref().map_or(255, |m| m.at(x, y, w, h));
        let through_at = |x: u32, y: u32| through.as_ref().map_or(0, |m| m.at(x, y, w, h));
        let level = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .flat_map(|(x, y)| [at(&ao, x, y), at(&eye_shadow, x, y), through_at(x, y), 255])
            .collect();
        Some(rgba8_texture(level, w, h, false))
    })?;
    Ok(texture.map(|name| (name, texcoord(material, sampler))))
}

/// The FMAT's facts the renderer's `cpu_material` reads.
fn material_info(material: &Material) -> MaterialInfo {
    let state = material.render_state;
    let mut info = MaterialInfo {
        render_state: RenderState {
            mode: state.mode,
            alpha_test: state.alpha_test,
            cull_back: state.cull_back,
        },
        tex_srt0: match material.shader_param("tex_srt0") {
            Some(&[mode, sx, sy, rotation, tx, ty]) => Some([mode, sx, sy, rotation, tx, ty]),
            _ => None,
        },
        albedo_constant: material.albedo_constant(),
        malice: material.is_malice(),
        transmission: material.shader_option("uking_enable_transmission") == Some("1"),
        casts_shadows: material.render_info("gsys_dynamic_depth_shadow") != Some("0"),
        in_cube_map: render_flag(material, "gsys_cube_map"),
        edit_sky_occlusion: render_flag(material, "uking_edit_sky_occlusion"),
        samplers: material.textures.clone(),
        ..Default::default()
    };
    info.look = material_look(material, info.leaves());
    info
}

/// A render-info flag that is on (a non-zero number), as the viewer's
/// `in_cube_map` and `covers_sky` read theirs.
fn render_flag(material: &Material, key: &str) -> bool {
    material
        .render_info(key)
        .and_then(|v| v.trim().parse::<i32>().ok())
        .is_some_and(|v| v != 0)
}

/// The look fields of `material`; `leaves` is the foliage test (masked,
/// transmission on). The viewer's `models::material_look`.
fn material_look(material: &Material, leaves: bool) -> MaterialLook {
    let option = |key: &str| {
        material
            .shader_option(key)
            .and_then(|v| v.parse::<u32>().ok())
    };
    let constant = |kind: &str, source: u32| {
        (100..=107)
            .contains(&source)
            .then(|| material.shader_param(&format!("const_{kind}{}", source - 100)))
            .flatten()
    };
    let behave = option("uking_material_behave").unwrap_or(0);
    let behave = MaterialBehave {
        code: behave,
        color: constant("color", behave).and_then(|c| c.try_into().ok()),
        value: constant("value", behave).and_then(|v| v.first().copied()),
    };
    let normal_blend = match option("uking_normalmap_blend_ratio") {
        Some(300) => Some(1.0),
        Some(source) => constant("value", source).and_then(|v| v.first().copied()),
        None => None,
    };
    let crown = (option("uking_modify_normal_type") == Some(1))
        .then(|| {
            material
                .shader_param("const_vector0")
                .and_then(|v| <[f32; 4]>::try_from(v).ok())
        })
        .flatten();
    MaterialLook {
        // 105: foliage; 103: water.
        leaf: leaves || behave.code == 105,
        crown,
        normal_blend,
        fresnel_cheat: option("uking_enable_fresnel_cheat") == Some(1),
        behave,
        water: behave.code == 103,
        gloss_intensity: material
            .shader_option("uking_grossy_intensity")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0),
        chara_size: option("uking_chara_size").unwrap_or(2) as f32,
        transmission: option("uking_enable_transmission") == Some(1)
            && option("uking_transmission_channel") == Some(40),
        translucent: option("uking_enable_transmission") == Some(1),
        leaf_light: (leaves
            && option("uking_enable_transmission") == Some(1)
            && option("uking_transmission_color") == Some(402))
        .then(|| {
            let value = |n: u32| constant("value", 100 + n).and_then(|v| v.first().copied());
            Some([value(1)?, value(2)?, value(3)?, value(4)?, value(5)?])
        })
        .flatten(),
    }
}

/// How a material's specular and metal masks become physical roughness
/// and metal (the viewer's `Gloss`; the renderer has the same tables).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Gloss {
    /// Perceptual roughness without and with a full specular mask.
    roughness: (f32, f32),
    /// Metal at a full metal mask.
    metal: f32,
    /// Prefix of the maps' file names.
    prefix: &'static str,
}

/// The world's objects (`Gloss::OBJECT`).
// SI-MAT-03: metal and roughness tables for Bevy PBR are ours.
const OBJECT_GLOSS: Gloss = Gloss {
    roughness: (0.85, 0.3),
    metal: 1.0,
    prefix: "mr",
};

/// Characters and their clothes (`Gloss::CHARACTER`): the game's
/// characters are matte.
// SI-MAT-03: metal and roughness tables for Bevy PBR are ours.
const CHARACTER_GLOSS: Gloss = Gloss {
    roughness: (0.9, 0.6),
    metal: 0.4,
    prefix: "mrc",
};

fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Rgb => "rgb",
        Channel::R => "r",
        Channel::G => "g",
        Channel::B => "b",
        Channel::A => "a",
    }
}

/// The material's specular and metal masks as one metallic-roughness
/// texture (the viewer's `metal_roughness`).
fn metal_roughness(
    material: &Material,
    gloss: Gloss,
    find_texture: &dyn Fn(&str) -> Option<TextureImage>,
    textures: &mut Textures,
    info: &mut MaterialInfo,
) -> Result<(), String> {
    let specular = material.specular_mask().and_then(|t| t.texture);
    let metal = material.metalness().and_then(|t| t.texture);
    if specular.is_none() && metal.is_none() {
        return Ok(());
    }
    let key = |t: &Option<(String, String, Channel)>| {
        t.as_ref().map_or("-".to_owned(), |(_, name, c)| {
            format!("{name}-{}", channel_name(*c))
        })
    };
    let stem = format!("{}_{}_{}", gloss.prefix, key(&specular), key(&metal));
    info.textures.metal_roughness = textures.get(&stem, || {
        let mask = |t: &Option<(String, String, Channel)>| -> Option<Mask> {
            let (_, name, channel) = t.as_ref()?;
            Mask::new(&find_texture(name)?, *channel)
        };
        metal_roughness_texture(mask(&specular).as_ref(), mask(&metal).as_ref(), gloss)
    })?;
    Ok(())
}

/// What the material emits, in the view and into the cube map: its mask as
/// a texture, if any, and its colour in the game's units (the viewer's
/// `emission`, before its brightness scale, which the renderer applies).
fn emission(
    material: &Material,
    find_texture: &dyn Fn(&str) -> Option<TextureImage>,
    textures: &mut Textures,
    info: &mut MaterialInfo,
) -> Result<(), String> {
    if let Some(emission) = crate::emission::emission(material) {
        (info.textures.emissive, info.emission) =
            emission_term(&emission.term, find_texture, textures)?;
        info.emission_exposure = emission.exposure;
    }
    if let Some(emission) = cube_map_emission(material) {
        (info.textures.emissive_cube, info.emission_cube) =
            emission_term(&emission.term, find_texture, textures)?;
        info.emission_cube_exposure = emission.exposure;
    }
    Ok(())
}

/// What the material emits into the environment's cube map. The cube map
/// pass draws with assign type 7 (`gsys_assign_cubemap`, `FUN_03a22ed4`),
/// which `uking_mat` lacks and which falls back to `gsys_assign_material`
/// (`FUN_039e2e78`, table `0x1047e3a4`: 7 → 0). Across the `uking_mat`
/// archive (v208), programs that differ only in
/// `uking_emission_color_cubemap` differ only in `gsys_assign_material`
/// variation 2, and those that differ only in the main emission differ in
/// every other emitting variation but that one: the cube map's variation
/// emits `uking_emission_color_cubemap` when
/// `uking_enable_emission_cubemap` is on, and nothing else. That the
/// cube map pass picks variation 2 is read from the archive, not traced
/// in the CPU.
fn cube_map_emission(material: &Material) -> Option<crate::emission::Emission> {
    crate::emission::cube_map_emission(material)
}

/// An emission term's mask as a texture (`None` without one) and its
/// colour; black when its mask cannot be read, which would otherwise light
/// the whole surface.
fn emission_term(
    term: &Term,
    find_texture: &dyn Fn(&str) -> Option<TextureImage>,
    textures: &mut Textures,
) -> Result<(Option<String>, [f32; 3]), String> {
    let Some((_, name, channel)) = &term.texture else {
        return Ok((None, term.color));
    };
    let second = term.second.as_ref().map(|(_, n, c)| (n.clone(), *c));
    let mut stem = format!("em_{name}-{}", channel_name(*channel));
    if let Some((other, c)) = &second {
        stem.push_str(&format!("_{other}-{}", channel_name(*c)));
    }
    let texture = textures.get(&stem, || {
        let texture = find_texture(name)?;
        let (w, h) = (texture.width, texture.height);
        let pick = |p: &[u8; 4], channel: Channel| -> [u8; 3] {
            match channel.index() {
                Some(i) => [p[i]; 3],
                None => [p[0], p[1], p[2]],
            }
        };
        let rgba = texture.decode_rgba8(0)?;
        let mut level: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| {
                let [r, g, b] = pick(p, *channel);
                [r, g, b, 255]
            })
            .collect();
        // Times the second channel, sampled at the nearest texel.
        if let Some((name, channel)) = &second {
            let other = find_texture(name)?;
            let (ow, oh) = (other.width, other.height);
            let values = other.decode_rgba8(0)?;
            for y in 0..h {
                for x in 0..w {
                    let (ox, oy) = (x * ow / w, y * oh / h);
                    let o = (oy * ow + ox) as usize * 4;
                    let factor = pick(
                        &[values[o], values[o + 1], values[o + 2], values[o + 3]],
                        *channel,
                    );
                    let i = (y * w + x) as usize * 4;
                    for c in 0..3 {
                        level[i + c] = (u16::from(level[i + c]) * u16::from(factor[c]) / 255) as u8;
                    }
                }
            }
        }
        Some(rgba8_texture(level, w, h, false))
    })?;
    Ok(match texture {
        Some(texture) => (Some(texture), term.color),
        None => (None, [0.0; 3]),
    })
}

// --- Textures (the viewer's load-time conversions, SI-FMT-10) ---

/// The folder's KTX2 files, each written once.
pub(crate) struct Textures {
    dir: PathBuf,
    /// File stem → written (or `None`: it could not be made).
    cache: HashMap<String, Option<String>>,
    written: usize,
    bytes: u64,
    face_trig: Option<Arc<crate::texture_srt::TextureSrtTable>>,
}

impl Textures {
    /// The file `stem` names, made by `make` on first use.
    pub(crate) fn get(
        &mut self,
        stem: &str,
        make: impl FnOnce() -> Option<Texture>,
    ) -> Result<Option<String>, String> {
        let stem: String = stem
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || "_-+.".contains(c) {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        if let Some(done) = self.cache.get(&stem) {
            return Ok(done.clone());
        }
        let done = match make() {
            Some(texture) => {
                let bytes = texture
                    .to_ktx2_zstd(KTX2_ZSTD_LEVEL)
                    .map_err(|e| format!("{stem}: {e}"))?;
                asset_format::write(&self.dir.join(format!("{stem}.ktx2")), &bytes)
                    .map_err(|e| e.to_string())?;
                self.written += 1;
                self.bytes += bytes.len() as u64;
                Some(stem.clone())
            }
            None => None,
        };
        self.cache.insert(stem, done.clone());
        Ok(done)
    }
}

/// A game texture as the GPU takes it (the viewer's `gpu_image`): its own
/// format and texels, block-compressed sizes rounded up to whole blocks
/// (then level 0 only: the stored blocks already cover it). `None` for
/// formats the viewer does not upload.
// SI-FMT-10: texture mips and BC conversions are ours.
pub(crate) fn game_texture(image: &TextureImage) -> Option<Texture> {
    if matches!(image.format, gx2::Format::Other(_)) {
        return None;
    }
    let mut texture = crate::sky::texture(image)?;
    if texture.format.is_block_compressed()
        && (!texture.width.is_multiple_of(4) || !texture.height.is_multiple_of(4))
    {
        texture
            .data
            .truncate(texture.level_bytes(0) * texture.layers as usize);
        texture.width = texture.width.next_multiple_of(4);
        texture.height = texture.height.next_multiple_of(4);
        texture.mip_levels = 1;
    }
    Some(texture)
}

/// A BC1 normal map's red and green as BC5, with the map's levels (the
/// viewer's `normal_map_image`; BC5 maps are used as they are).
// SI-FMT-10: texture mips and BC conversions are ours.
pub(crate) fn normal_map_texture(image: &TextureImage) -> Option<Texture> {
    if !matches!(image.format, gx2::Format::Bc1 { .. })
        || !image.width.is_multiple_of(4)
        || !image.height.is_multiple_of(4)
    {
        return None;
    }
    let mut data = Vec::new();
    for level in 0..image.mip_levels {
        let (w, h) = image.level_size(level);
        for layer in 0..image.layers {
            data.extend(bc::bc1_to_bc5(image.layer_data(level, layer)?, w, h));
        }
    }
    let mut texture = game_texture(image)?;
    texture.format = Format::Bc5 { signed: false };
    texture.data = data;
    Some(texture)
}

/// A BC1 normal map's blue, the surface's gloss, as BC4 with the map's
/// levels (the viewer's `gloss_map_image`).
// SI-FMT-10: texture mips and BC conversions are ours.
fn gloss_map_texture(image: &TextureImage) -> Option<Texture> {
    if !matches!(image.format, gx2::Format::Bc1 { .. })
        || !image.width.is_multiple_of(4)
        || !image.height.is_multiple_of(4)
    {
        return None;
    }
    let mut texture = game_texture(image)?;
    let mut data = Vec::new();
    for level in 0..texture.mip_levels {
        let (w, h) = image.level_size(level);
        for layer in 0..image.layers {
            data.extend(bc::bc1_blue_to_bc4(image.layer_data(level, layer)?, w, h));
        }
    }
    texture.format = Format::Bc4 { signed: false };
    texture.data = data;
    Some(texture)
}

/// Albedo with the mask texture as alpha (leaves, grass), as RGBA8 with
/// every level both textures share (the viewer's `combine_alpha`).
// SI-FMT-10: texture mips and BC conversions are ours.
fn combine_alpha(albedo: &TextureImage, mask: &TextureImage) -> Option<Texture> {
    let levels = albedo.mip_levels.min(mask.mip_levels);
    let mut data = Vec::new();
    for level in 0..levels {
        let color = albedo.decode_rgba8(level)?;
        let alpha = mask.decode_rgba8(level)?;
        let (w, h) = albedo.level_size(level);
        let (mw, mh) = mask.level_size(level);
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = (y * w as usize + x) * 4;
                // Masks may be smaller: sample the nearest texel.
                let (mx, my) = (x * mw as usize / w as usize, y * mh as usize / h as usize);
                let a = alpha[(my * mw as usize + mx) * 4];
                data.extend_from_slice(&[color[i], color[i + 1], color[i + 2], a]);
            }
        }
    }
    let srgb = matches!(
        albedo.format,
        gx2::Format::Bc1 { srgb: true }
            | gx2::Format::Bc3 { srgb: true }
            | gx2::Format::Rgba8 { srgb: true }
    );
    Some(Texture {
        format: Format::Rgba8 { srgb },
        width: albedo.width,
        height: albedo.height,
        layers: 1,
        mip_levels: levels,
        data,
        swizzle: IDENTITY_SWIZZLE,
    })
}

/// An RGBA8 texture from its level 0, with the rest of the mip chain built
/// (the viewer's `rgba8_image`).
// SI-FMT-10: texture mips and BC conversions are ours.
fn rgba8_texture(level0: Vec<u8>, width: u32, height: u32, srgb: bool) -> Texture {
    let (mut level, mut w, mut h) = (level0, width, height);
    let mut data = level.clone();
    let mut levels = 1;
    while w > 1 || h > 1 {
        (level, w, h) = bc::downsample(&level, w, h);
        data.extend_from_slice(&level);
        levels += 1;
    }
    Texture {
        format: Format::Rgba8 { srgb },
        width,
        height,
        layers: 1,
        mip_levels: levels,
        data,
        swizzle: IDENTITY_SWIZZLE,
    }
}

/// One channel of a texture's level 0, as bytes.
struct Mask {
    width: u32,
    height: u32,
    values: Vec<u8>,
}

impl Mask {
    fn new(texture: &TextureImage, channel: Channel) -> Option<Self> {
        let rgba = texture.decode_rgba8(0)?;
        let values = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| match channel.index() {
                Some(i) => p[i],
                // SI-MAT-05: foliage detection and mask mean are our heuristics.
                None => ((u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2])) / 3) as u8,
            })
            .collect();
        Some(Self {
            width: texture.width,
            height: texture.height,
            values,
        })
    }

    /// The nearest value at `(x, y)` of a `w`×`h` grid.
    fn at(&self, x: u32, y: u32, w: u32, h: u32) -> u8 {
        let (mx, my) = (x * self.width / w, y * self.height / h);
        self.values[(my * self.width + mx) as usize]
    }
}

/// Roughness (green, lower where the specular mask is high) and metal
/// (blue) as RGBA8 with a full mip chain, at the larger mask's size (the
/// viewer's `metal_roughness_image`).
fn metal_roughness_texture(
    specular: Option<&Mask>,
    metal: Option<&Mask>,
    gloss: Gloss,
) -> Option<Texture> {
    let (w, h) = [specular, metal]
        .into_iter()
        .flatten()
        .map(|m| (m.width, m.height))
        .max()?;
    let mut level = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let s = specular.map_or(0.0, |m| f32::from(m.at(x, y, w, h)) / 255.0);
            let rough = gloss.roughness.0 + (gloss.roughness.1 - gloss.roughness.0) * s;
            let metal = metal
                .map_or(0.0, |m| f32::from(m.at(x, y, w, h)) * gloss.metal)
                .round() as u8;
            level.extend_from_slice(&[0, (rough * 255.0).round() as u8, metal, 255]);
        }
    }
    Some(rgba8_texture(level, w, h, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_parameters_preserve_integer_srt_modes_and_all_components() {
        use botw_formats::bfres::model::ShaderParam;
        let mut material = glowing(&[]);
        material.shader_params.clear();
        for i in 0..5 {
            material.shader_params.push(ShaderParam {
                name: format!("const_color{i}"),
                kind: 15,
                values: vec![i as f32, 0.2, 0.3, 0.4],
            });
        }
        for i in 0..6 {
            material.shader_params.push(ShaderParam {
                name: format!("tex_srt{i}"),
                kind: 30,
                values: vec![f32::from_bits(i % 3), 2.0, 3.0, 0.25, 0.4, 0.5],
            });
        }
        material.shader_params.push(ShaderParam {
            name: "gsys_xlu_zprepass_alpha".into(),
            kind: 12,
            values: vec![0.75],
        });
        let (colors, srts, alpha) = face_parameters(&material).unwrap();
        assert_eq!(colors[4], [4.0, 0.2, 0.3, 0.4]);
        assert_eq!(srts.map(|s| s.mode), [0, 1, 2, 0, 1, 2]);
        assert_eq!(srts[1].scale, [2.0, 3.0]);
        assert_eq!(srts[1].rotation, 0.25);
        assert_eq!(srts[1].translation, [0.4, 0.5]);
        assert_eq!(alpha, 0.75);
        material.shader_params[5].kind = 15;
        assert!(face_parameters(&material).is_none());
    }

    fn glowing(options: &[(&str, &str)]) -> Material {
        let mut material = Material {
            name: "Mt_Glow".into(),
            textures: Vec::new(),
            render_info: Vec::new(),
            shader_archive: "uking_mat".into(),
            shading_model: "uking_mat".into(),
            shader_options: Vec::new(),
            sampler_assign: Vec::new(),
            samplers: Vec::new(),
            render_state: Default::default(),
            shader_params: vec![
                botw_formats::bfres::model::ShaderParam {
                    name: "const_color0".into(),
                    kind: 15,
                    values: vec![20.0, 5.0, 0.3, 1.0],
                },
                botw_formats::bfres::model::ShaderParam {
                    name: "const_color1".into(),
                    kind: 15,
                    values: vec![0.0, 0.5, 1.0, 1.0],
                },
            ],
        };
        material.shader_options = options
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        material
    }

    #[test]
    fn the_cube_map_glows_only_with_its_own_option() {
        let main = [
            ("uking_enable_emission", "1"),
            ("uking_emission_color", "100"),
        ];
        let off = glowing(&[
            main[0],
            main[1],
            ("uking_enable_emission_cubemap", "0"),
            ("uking_emission_color_cubemap", "101"),
        ]);
        let view = crate::emission::emission(&off).unwrap();
        assert_eq!(view.term.color, [20.0, 5.0, 0.3]);
        assert!(cube_map_emission(&off).is_none());
        let on = glowing(&[
            main[0],
            main[1],
            ("uking_enable_emission_cubemap", "1"),
            ("uking_emission_color_cubemap", "101"),
        ]);
        assert_eq!(cube_map_emission(&on).unwrap().term.color, [0.0, 0.5, 1.0]);
    }

    #[test]
    fn night_windows_take_a_value_and_one_minus_the_exposure() {
        // Program 6081's recipe: colour 0 = const_value0 · (1 − e), the
        // emission colour 0 times const_color1.
        let mut window = glowing(&[
            ("uking_enable_emission", "1"),
            ("uking_emission_color", "201"),
            ("uking_enable_calc_color0", "1"),
            ("uking_color0_calc_type", "2"),
            ("uking_color0_A", "104"),
            ("uking_color0_B", "507"),
            ("uking_color0_B_channel", "1"),
            ("uking_enable_calc_color1", "1"),
            ("uking_color1_calc_type", "2"),
            ("uking_color1_A", "200"),
            ("uking_color1_B", "101"),
        ]);
        window
            .shader_params
            .push(botw_formats::bfres::model::ShaderParam {
                name: "const_value0".into(),
                kind: 12,
                values: vec![5.0],
            });
        let emission = crate::emission::emission(&window).unwrap();
        assert_eq!(emission.term.color, [0.0, 2.5, 5.0]);
        assert_eq!(
            emission.exposure,
            asset_format::model::DynamicExposure::OneMinus
        );
    }

    #[test]
    fn rounds_odd_block_sizes_to_level_zero() {
        let image = TextureImage {
            name: "T".into(),
            format: gx2::Format::Bc1 { srgb: true },
            width: 6,
            height: 4,
            layers: 1,
            mip_levels: 3,
            // 2×1 blocks, then 1×1, 1×1.
            data: vec![0; 16 + 8 + 8],
            component_select: [0, 1, 2, 3],
        };
        let texture = game_texture(&image).unwrap();
        assert_eq!(
            (texture.width, texture.height, texture.mip_levels),
            (8, 4, 1)
        );
        assert_eq!(texture.data.len(), 16);
        assert!(texture.to_ktx2().is_ok());
    }

    #[test]
    fn builds_full_rgba_chains() {
        let texture = rgba8_texture(vec![255; 4 * 4 * 2], 4, 2, false);
        assert_eq!(texture.mip_levels, 3);
        assert_eq!(texture.data.len(), (8 + 2 + 1) * 4);
        assert!(texture.to_ktx2().is_ok());
    }
}
