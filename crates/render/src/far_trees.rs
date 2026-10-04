//! Far trees: beyond the range of the real tree models the game's terrain
//! system draws the forests as billboards, pictures of each tree species
//! from a few angles in two texture arrays of `Terrain.Tex1` (`Tree0Alb`,
//! `Tree1Alb`), at the spots listed in each map cell's `_TeraTree.sblwp`
//! (see archived FORMATS.md notes). Every listed tree whose actor has pictures gets one:
//! a quad standing on the tree's spot, turned to the camera around the
//! vertical, showing the picture taken from the camera's side of the tree
//! (`far_trees.wgsl`). The quads of a map cell share one mesh, so the whole
//! map's forests are a few dozen draw calls, fewer once the cells out of
//! view are culled.
//!
//! Ported from the original renderer's `far_trees.rs`, reading the baked trees
//! (`trees/`, `asset_format::trees`) instead of the dump: the atlases are
//! uploaded in the game's own format (BC3) instead of decoded to RGBA8.

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;

use asset_format::paths;
use asset_format::texture::Texture;
use asset_format::trees::{FarTreeIndex, atlas_path};
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::shader::ShaderRef;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::clouds::{CloudParams, CloudShadows};
use crate::deferred_light::CubeMean;
use crate::object_material::{HandOffMask, MASK_SIZE, dither_mask_image};
use crate::objects::Objects;

const SHADER: Handle<Shader> = uuid_handle!("3e8b5c71-2a94-4d06-b1f3-9c7d8e2a4b15");

pub type FarTreeMaterial = ExtendedMaterial<StandardMaterial, FarTreeExtension>;

/// Runtime equivalents of the far-tree comparison environment flags.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct TreeSettings {
    pub enabled: bool,
    pub shadows: bool,
    pub billboards_only: bool,
}

#[derive(Component)]
struct FarTreeCell;

pub struct FarTreesPlugin {
    /// The `assets/` folder; without the baked trees there are none to draw
    /// (`BOTW_NO_FAR_TREES` turns them off too, to compare).
    pub assets: PathBuf,
}

impl Plugin for FarTreesPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "far_trees.wgsl", Shader::from_wgsl);
        let baked = self.assets.join(paths::TREE_INDEX).exists();
        if !baked {
            warn!("no far trees; run `cargo bake --only trees`");
        }
        let enabled = baked && std::env::var_os("BOTW_NO_FAR_TREES").is_none();
        app.insert_resource(TreeSettings {
            enabled,
            shadows: std::env::var_os("BOTW_NO_TREE_SHADOWS").is_none(),
            billboards_only: std::env::var_os("BOTW_FAR_TREES_ONLY").is_some(),
        })
        .add_plugins(MaterialPlugin::<FarTreeMaterial>::default())
        .insert_resource(FarTrees {
            assets: self.assets.clone(),
            state: if enabled { State::Waiting } else { State::Off },
            hand_off: HashMap::new(),
            count: 0,
            materials: Vec::new(),
        })
        .add_systems(
            Update,
            (configure, start_loading, finish_loading, follow_clouds).chain(),
        )
        .add_systems(
            PostUpdate,
            update_ready_handoffs.after(crate::objects::update_far_handoffs),
        );
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct FarTreeExtension {
    /// One atlas's pictures (`Tree0Alb` or `Tree1Alb`), one per layer.
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub albedo: Handle<Image>,
    /// Their normals and translucency (`Tree0NrmTrs`, `Tree1NrmTrs`).
    #[texture(102, dimension = "2d_array")]
    pub normals: Handle<Image>,
    /// Cloud shadows, like the ground under the trees.
    #[texture(103)]
    #[sampler(104)]
    pub cloud_shadow_map: Handle<Image>,
    #[uniform(105)]
    pub clouds: CloudParams,
    /// The dissolve mask the models share (`object_material.rs`).
    #[texture(106)]
    pub mask: Handle<Image>,
    /// The shared look values (`look::LookTexture`).
    #[texture(107)]
    pub look: Handle<Image>,
    /// `TreeDitherMask` as the game samples it in the far-tree pixel shader
    /// (`tera_tree_mask`: its own values and mips, wrapping).
    #[texture(108)]
    #[sampler(109)]
    pub tree_mask: Handle<Image>,
    /// x: the atlas material's alpha-test reference (`Tree0` 0.5, `Tree1`
    /// 0.3, `TeraTree` render state).
    #[uniform(110)]
    pub params: Vec4,
    /// The environment's mean brightness (`deferred_light::CubeMean`).
    #[texture(111, sample_type = "float", filterable = false)]
    pub cube_mean: Handle<Image>,
}

impl MaterialExtension for FarTreeExtension {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn prepass_fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn enable_prepass() -> bool {
        false
    }

    // Trees cast their shadows within the sun's cascades.
    // SI-TRE-03: far-tree shadow quad turned to the light is ours.
    fn enable_shadows() -> bool {
        true
    }
}

/// The billboard forests, and which actors' models hand over to them.
#[derive(Resource)]
pub struct FarTrees {
    assets: PathBuf,
    state: State,
    /// Actor name → the distance where its model gives way to its billboard.
    hand_off: HashMap<String, f32>,
    count: usize,
    /// One per atlas.
    materials: Vec<Handle<FarTreeMaterial>>,
}

enum State {
    Off,
    /// Waiting for the objects' reach.
    Waiting,
    Loading(Task<Result<Loaded, String>>),
    Ready,
}

impl FarTrees {
    /// Objects wait for this: until it is known which trees have
    /// billboards, it is not known how far their models must reach.
    pub fn is_loading(&self) -> bool {
        matches!(self.state, State::Waiting | State::Loading(_))
    }

    /// Where `actor`'s model hands over to its billboard, if it has one.
    pub fn hand_off(&self, actor: &str) -> Option<f32> {
        self.hand_off.get(actor).copied()
    }

    /// Billboards drawn.
    pub fn count(&self) -> usize {
        self.count
    }
}

struct Loaded {
    /// Per atlas: its pictures and their normals, and its material's
    /// alpha-test reference.
    atlases: Vec<(Image, Image, f32)>,
    /// Per atlas: one mesh per map cell with trees, and its bounds.
    meshes: Vec<Vec<(Mesh, Aabb, TreeHandoffs)>>,
    /// `TreeDitherMask`, if it could be read: level 0, and every level.
    mask: Option<(Vec<u8>, Vec<Vec<u8>>)>,
    hand_off: HashMap<String, f32>,
    count: usize,
}

/// Models reach at least this far before handing over (m).
// SI-TRE-01: far-tree hand-off distance is our heuristic.
const MIN_HAND_OFF: f32 = 150.0;

/// The share of the hand-off distance over which a model dissolves into its
/// billboard.
const DISSOLVE: f32 = 0.2;

/// Where a model that hands over at `hand_off` starts dissolving.
pub fn dissolve_start(hand_off: f32) -> f32 {
    hand_off * (1.0 - DISSOLVE)
}

fn configure(
    settings: Res<TreeSettings>,
    mut trees: ResMut<FarTrees>,
    cells: Query<Entity, With<FarTreeCell>>,
    mut objects: ResMut<Objects>,
    mut far: ResMut<crate::objects::FarModels>,
    mut commands: Commands,
) {
    if !settings.is_changed() {
        return;
    }
    for entity in &cells {
        commands.entity(entity).despawn();
    }
    trees.hand_off.clear();
    trees.materials.clear();
    trees.count = 0;
    trees.state = if settings.enabled && trees.assets.join(paths::TREE_INDEX).exists() {
        State::Waiting
    } else {
        State::Off
    };
    objects.refresh(&mut commands);
    far.refresh(&mut commands);
}

fn start_loading(mut trees: ResMut<FarTrees>, objects: Res<Objects>, settings: Res<TreeSettings>) {
    if !matches!(trees.state, State::Waiting) {
        return;
    }
    // Models never reach past the objects' far radius.
    let reach = MIN_HAND_OFF..=objects.far_radius.max(MIN_HAND_OFF);
    let assets = trees.assets.clone();
    let billboards_only = settings.billboards_only;
    trees.state = State::Loading(
        AsyncComputeTaskPool::get().spawn(async move { load(&assets, reach, billboards_only) }),
    );
}

#[allow(clippy::too_many_arguments)]
fn finish_loading(
    settings: Res<TreeSettings>,
    mut commands: Commands,
    mut trees: ResMut<FarTrees>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<FarTreeMaterial>>,
    clouds: Option<Res<CloudShadows>>,
    mask: Res<HandOffMask>,
    look: Option<Res<crate::look::LookTexture>>,
    cube_mean: Option<Res<CubeMean>>,
) {
    let State::Loading(task) = &mut trees.state else {
        return;
    };
    let Some(result) = block_on(poll_once(task)) else {
        return;
    };
    let loaded = match result {
        Ok(loaded) => loaded,
        Err(error) => {
            warn!("far trees unavailable: {error}");
            trees.state = State::Off;
            return;
        }
    };
    let mut tree_mask = Handle::default();
    if let Some((values, levels)) = &loaded.mask {
        // The models' materials see the game's mask through the same handle.
        let _ = images.insert(&mask.0, dither_mask_image(values));
        tree_mask = images.add(tree_mask_image(levels));
    }
    for ((albedo, normals, alpha_ref), atlas_meshes) in
        loaded.atlases.into_iter().zip(loaded.meshes)
    {
        let material = materials.add(FarTreeMaterial {
            base: StandardMaterial {
                // Cut out (the shaders do it): so the shadow pass runs them too.
                alpha_mode: AlphaMode::Mask(0.5),
                ..default()
            },
            extension: FarTreeExtension {
                albedo: images.add(albedo),
                normals: images.add(normals),
                cloud_shadow_map: clouds
                    .as_ref()
                    .map(|c| c.shadow_map.clone())
                    .unwrap_or_default(),
                clouds: clouds
                    .as_ref()
                    .map_or_else(CloudParams::without_shadows, |c| c.params.clone()),
                mask: mask.0.clone(),
                look: look.as_ref().map(|l| l.0.clone()).unwrap_or_default(),
                tree_mask: tree_mask.clone(),
                params: Vec4::new(alpha_ref, 0.0, 0.0, 0.0),
                cube_mean: cube_mean.as_ref().map(|m| m.0.clone()).unwrap_or_default(),
            },
        });
        trees.materials.push(material.clone());
        for (mesh, bounds, handoffs) in atlas_meshes {
            let mut cell = commands.spawn((
                Name::new("far trees"),
                FarTreeCell,
                handoffs,
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                // The quads grow out of their origins in the shader: the
                // mesh's own bounds would miss them.
                bounds,
                NoAutoAabb,
            ));
            // `BOTW_NO_TREE_SHADOWS` leaves their shadows out, to compare.
            // SI-TRE-03: far-tree shadow quad turned to the light is ours.
            if !settings.shadows {
                cell.insert(NotShadowCaster);
            }
        }
    }
    trees.hand_off = loaded.hand_off;
    trees.count = loaded.count;
    trees.state = State::Ready;
}

/// Keeps the trees' cloud shadows on the moving sun.
fn follow_clouds(
    trees: Res<FarTrees>,
    clouds: Option<Res<CloudShadows>>,
    mut materials: ResMut<Assets<FarTreeMaterial>>,
) {
    let Some(clouds) = clouds else { return };
    if !clouds.is_changed() {
        return;
    }
    for handle in &trees.materials {
        if let Some(mut material) = materials.get_mut(handle) {
            material.extension.clouds = clouds.params.clone();
        }
    }
}

/// SI-WLD-08: per-instance readiness for the four vertices of each billboard.
#[derive(Component)]
struct TreeHandoffs(Vec<(crate::objects::InstanceKey, f32)>);

fn update_ready_handoffs(
    ready: Res<crate::objects::ReadyNearModels>,
    cells: Query<(&Mesh3d, Ref<TreeHandoffs>)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    use bevy::mesh::VertexAttributeValues;
    for (handle, handoffs) in &cells {
        if !ready.is_changed() && !handoffs.is_added() {
            continue;
        }
        let target = |i: usize| {
            let (key, distance) = &handoffs.0[i / 4];
            if ready.0.contains(key) {
                *distance
            } else {
                0.0
            }
        };
        let Some(mesh) = meshes.get(handle) else {
            continue;
        };
        let Some(VertexAttributeValues::Float32x4(colors)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
        else {
            continue;
        };
        if !colors
            .iter()
            .enumerate()
            .any(|(i, color)| color[0] != target(i))
        {
            continue;
        }
        if let Some(mut mesh) = meshes.get_mut(handle)
            && let Some(VertexAttributeValues::Float32x4(colors)) =
                mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR)
        {
            for (i, color) in colors.iter_mut().enumerate() {
                color[0] = target(i);
            }
        }
    }
}

/// An atlas as a GPU texture array, clamped at the edges so a tree's top
/// does not bleed into its roots.
fn array_image(texture: &Texture) -> Result<Image, String> {
    let mut image = crate::texture::gpu_image(texture, true)
        .ok_or_else(|| format!("atlas format {:?} not supported", texture.format))?;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        // The materials' `a0`, `n0`: bilinear, the nearest mip.
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    Ok(image)
}

/// `TreeDitherMask` with its mips as `tera_tree_mask` is sampled
/// (`TeraTree` materials' sampler: wrap, bilinear, the nearest mip).
fn tree_mask_image(levels: &[Vec<u8>]) -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: MASK_SIZE,
            height: MASK_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::R8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(levels.concat());
    image.texture_descriptor.mip_level_count = levels.len() as u32;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });
    image
}

fn load(
    assets: &std::path::Path,
    reach: RangeInclusive<f32>,
    billboards_only: bool,
) -> Result<Loaded, String> {
    let started = std::time::Instant::now();
    let index: FarTreeIndex =
        asset_format::read_ron(&assets.join(paths::TREE_INDEX)).map_err(|e| e.to_string())?;
    let read = |name: &str| {
        Texture::read(&assets.join(atlas_path(name))).map_err(|e| format!("{name}: {e}"))
    };
    let mut atlases = Vec::new();
    for atlas in &index.atlases {
        atlases.push((
            array_image(&read(&atlas.albedo)?)?,
            array_image(&read(&atlas.normals)?)?,
            atlas.alpha_ref,
        ));
    }

    let hand_off_of = |traverse_dist: Option<f32>| {
        // Where the game stops keeping the actor (its traverse distance).
        // `BOTW_FAR_TREES_ONLY` draws every tree as its billboard, to
        // compare them with the models.
        // SI-TRE-01: far-tree hand-off distance is our heuristic.
        if billboards_only {
            0.0
        } else {
            traverse_dist
                .unwrap_or(100.0)
                .clamp(*reach.start(), *reach.end())
        }
    };
    let species: Vec<Species> = index
        .species
        .iter()
        .enumerate()
        .map(|(id, s)| Species {
            id,
            atlas: s.atlas as usize,
            first_layer: s.first_layer,
            views: s.views,
            frame: Frame {
                bottom: s.bottom,
                height: s.height,
                width: s.width,
            },
            hand_off: hand_off_of(s.traverse_dist),
        })
        .collect();
    let mut per_cell: Vec<Vec<Vec<(Tree, Species)>>> = vec![Vec::new(); atlases.len()];
    let mut count = 0;
    for cell in &index.cells {
        let mut trees: Vec<Vec<(Tree, Species)>> = vec![Vec::new(); atlases.len()];
        for tree in &cell.trees {
            let Some(&found) = species.get(tree.species as usize) else {
                continue;
            };
            trees[found.atlas].push((Tree::from(tree), found));
            count += 1;
        }
        for (atlas, trees) in trees.into_iter().enumerate() {
            if !trees.is_empty() {
                per_cell[atlas].push(trees);
            }
        }
    }
    let meshes = per_cell
        .iter()
        .map(|cells| {
            cells
                .iter()
                .map(|trees| {
                    let (mesh, bounds) = billboard_mesh(trees);
                    let handoffs = trees
                        .iter()
                        .map(|(tree, species)| {
                            (
                                crate::objects::InstanceKey::new(
                                    index.species[species.id].name.as_str().into(),
                                    tree.position,
                                ),
                                species.hand_off,
                            )
                        })
                        .collect();
                    (mesh, bounds, TreeHandoffs(handoffs))
                })
                .collect()
        })
        .collect();
    let hand_off = index
        .species
        .iter()
        .zip(&species)
        .map(|(baked, s)| (baked.name.clone(), s.hand_off))
        .collect();
    let mask = Texture::read(&assets.join(paths::TREE_DITHER_MASK))
        .map_err(|e| e.to_string())
        .and_then(|mask| dither_mask(&mask))
        .inspect_err(|e| warn!("TreeDitherMask: {e}"))
        .ok();
    info!(
        "{count} far trees ready in {:.1} s",
        started.elapsed().as_secs_f32()
    );
    Ok(Loaded {
        atlases,
        meshes,
        mask,
        hand_off,
        count,
    })
}

/// The values of the game's dissolve mask: level 0, and every level.
fn dither_mask(texture: &Texture) -> Result<(Vec<u8>, Vec<Vec<u8>>), String> {
    if (texture.width, texture.height) != (MASK_SIZE, MASK_SIZE) {
        return Err(format!(
            "{}x{}, expected {MASK_SIZE}x{MASK_SIZE}",
            texture.width, texture.height
        ));
    }
    let levels = (0..texture.mip_levels)
        .map(|level| texture.layer_data(level, 0).map(<[u8]>::to_vec))
        .collect::<Option<Vec<_>>>()
        .ok_or("truncated")?;
    Ok((levels[0].clone(), levels))
}

/// How the game frames an actor in an atlas.
#[derive(Clone, Copy, Debug)]
struct Species {
    id: usize,
    atlas: usize,
    first_layer: u32,
    views: u32,
    frame: Frame,
    hand_off: f32,
}

/// the original format parser's `trees::BillboardFrame` (baked).
#[derive(Clone, Copy, Debug)]
struct Frame {
    bottom: f32,
    height: f32,
    width: f32,
}

/// A tree where the terrain system draws one.
#[derive(Clone, Copy, Debug)]
struct Tree {
    position: Vec3,
    /// Turn about the vertical (radians).
    yaw: f32,
    scale: f32,
}

impl From<&asset_format::trees::Tree> for Tree {
    fn from(tree: &asset_format::trees::Tree) -> Self {
        let [x, y, z] = tree.rotate;
        // Trees stand upright; some are flipped over X and Z at once, which
        // is only a turn about Y.
        let forward = Quat::from_euler(EulerRot::ZYX, z, y, x) * Vec3::Z;
        Self {
            position: Vec3::from(tree.translate),
            yaw: forward.x.atan2(forward.z),
            scale: tree.scale,
        }
    }
}

/// A cell's billboards as one mesh: four corners per tree, each carrying
/// the tree's data (see `far_trees.wgsl`: the shadow pass gets no normals,
/// so everything is in the positions, UVs and colours); and the box the
/// quads stay in whichever way they turn, towards the camera or the light.
fn billboard_mesh(trees: &[(Tree, Species)]) -> (Mesh, Aabb) {
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let n = trees.len() * 4;
    let (mut positions, mut corners, mut layers, mut extra) = (
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
    );
    let mut indices = Vec::with_capacity(trees.len() * 6);
    for (tree, species) in trees {
        let frame = species.frame;
        let (width, height, bottom) = (
            frame.width * tree.scale,
            frame.height * tree.scale,
            frame.bottom * tree.scale,
        );
        let half_width = width / 2.0;
        min = min.min(tree.position + Vec3::new(-half_width, bottom, -half_width));
        max = max.max(tree.position + Vec3::new(half_width, bottom + height, half_width));
        // Turned towards the light, the quad spins about the crown's middle.
        let crown = tree.position + Vec3::Y * (bottom + height / 2.0);
        let reach = Vec3::splat(width.hypot(height) / 2.0);
        (min, max) = (min.min(crown - reach), max.max(crown + reach));
        let first = positions.len() as u32;
        for corner in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]] {
            positions.push(tree.position.to_array());
            corners.push(corner);
            layers.push([(species.first_layer * 16 + species.views) as f32, tree.yaw]);
            extra.push([species.hand_off, bottom, width, height]);
        }
        indices.extend([0, 2, 1, 1, 2, 3].map(|i| first + i));
    }
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, corners)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, layers)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, extra)
    .with_inserted_indices(Indices::U32(indices));
    (mesh, Aabb::from_min_max(min, max))
}

#[cfg(test)]
mod readiness_tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn billboards_stay_visible_until_the_exact_tree_model_is_ready() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .init_resource::<crate::objects::ReadyNearModels>()
            .add_systems(Update, update_ready_handoffs);
        let key = crate::objects::InstanceKey::new("Tree".into(), Vec3::ZERO);
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[150.0, 0.0, 1.0, 2.0]; 4]);
        let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        app.world_mut().spawn((
            Mesh3d(handle.clone()),
            TreeHandoffs(vec![(key.clone(), 150.0)]),
        ));
        let distance = |app: &App| {
            let meshes = app.world().resource::<Assets<Mesh>>();
            let Some(VertexAttributeValues::Float32x4(colors)) = meshes
                .get(&handle)
                .unwrap()
                .attribute(Mesh::ATTRIBUTE_COLOR)
            else {
                panic!("missing tree data")
            };
            assert!(colors.iter().all(|color| color[1..] == [0.0, 1.0, 2.0]));
            colors[0][0]
        };
        app.update();
        assert_eq!(distance(&app), 0.0);
        app.world_mut()
            .resource_mut::<crate::objects::ReadyNearModels>()
            .0
            .insert(crate::objects::InstanceKey::new("Tree".into(), Vec3::X));
        app.update();
        assert_eq!(distance(&app), 0.0);
        app.world_mut()
            .resource_mut::<crate::objects::ReadyNearModels>()
            .0
            .insert(key);
        app.update();
        assert_eq!(distance(&app), 150.0);
        app.world_mut()
            .resource_mut::<crate::objects::ReadyNearModels>()
            .0
            .clear();
        app.update();
        assert_eq!(distance(&app), 0.0);
    }
}
