//! The map's placed objects (stage 1d): trees, rocks, ruins, plants … from
//! the map units around the camera, drawn with the game's models. Objects
//! appear within `spawn_radius` and go beyond `despawn_radius`; big ones
//! (trees, rocks, ruins) out to `far_radius`, so forests and landmarks
//! stay on the horizon. With distance objects switch to the models' coarser
//! meshes, and at the edge of their range shrink away instead of popping
//! (dithered fades leave a visible screen-door pattern). Beyond that the
//! game's own `_Far` stand-ins (cliffs, towers, castle walls) take over out
//! to `horizon`; objects that have one swap to it without shrinking.
//!
//! Ported from the original renderer's `objects.rs`, reading the baked cells
//! (`objects/cells/<cell>.ron`) and `_Far` index (`objects/far.ron`)
//! instead of the dump's map units; gameplay (collision near the player
//! and enemies) is left out. What the viewer walked every frame (every
//! actor of the loaded cells, every `_Far` actor of the map) is found
//! through a grid of the actors, and only after the camera has moved
//! [`RECHECK`] (`FAR_RECHECK` for the stand-ins) or while something is
//! still loading. Spawning and despawning reach that much farther than the
//! viewer's radii, so every object the viewer would have spawned at any
//! camera position since stands; what shows is decided as in the viewer,
//! by each mesh's `VisibilityRange` and `EdgeFade` (both measured from the
//! camera every frame). Levels of detail that the viewer gives an empty
//! range are not spawned, and cells far behind are dropped.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use asset_format::objects::{
    CELL_SIZE, FarIndex as BakedFarIndex, ObjectIndex, PlacedActor, PlacedActors, cell_name,
    cell_origin,
};
use asset_format::paths;
use bevy::camera::visibility::{RenderLayers, VisibilityRange};
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::far_trees::FarTrees;
use crate::models::{ActorModel, ModelLibrary, Part};
use crate::terrain_material::TerrainLook;

pub struct ObjectsPlugin {
    /// The `assets/` folder.
    pub assets: PathBuf,
}

impl Plugin for ObjectsPlugin {
    fn build(&self, app: &mut App) {
        let index = self.assets.join(paths::OBJECT_INDEX);
        let units = if index.exists() {
            Units::Loading(AsyncComputeTaskPool::get().spawn(async move {
                asset_format::read_ron::<ObjectIndex>(&index)
                    .inspect_err(|e| warn!("objects: {e}"))
                    .ok()
                    .map(|index| Arc::new(index.cells.into_iter().collect()))
            }))
        } else {
            warn!("no placed objects; run `cargo bake --only objects,models`");
            Units::None
        };
        let far_path = self.assets.join(paths::FAR_INDEX);
        let far_index = if far_path.exists() {
            FarIndex::Loading(AsyncComputeTaskPool::get().spawn(async move {
                let started = std::time::Instant::now();
                let baked = asset_format::read_ron::<BakedFarIndex>(&far_path)
                    .inspect_err(|e| warn!("far stand-ins: {e}"))
                    .ok()?;
                let far = FarActors::new(&baked);
                info!(
                    "{} far stand-ins indexed in {:.1} s",
                    far.actors.len(),
                    started.elapsed().as_secs_f32()
                );
                Some(far)
            }))
        } else {
            FarIndex::None
        };
        app.insert_resource(Objects {
            assets: self.assets.clone(),
            units,
            cells: HashMap::new(),
            spawned: HashMap::new(),
            no_model: HashSet::new(),
            // SI-WLD-03: object draw ranges and LOD choice are ours.
            spawn_radius: 220.0,
            despawn_radius: 240.0,
            far_radius: 700.0,
            large_size: 4.0,
            settled: false,
            checked_at: None,
        })
        .init_resource::<ReadyNearModels>()
        .insert_resource(FarModels {
            index: far_index,
            spawned: HashMap::new(),
            stand_ins: HashSet::new(),
            horizon: 3500.0,
            settled: false,
            checked_at: None,
        })
        .add_systems(Update, (stream_objects, stream_far_models, fade_edges))
        .add_systems(
            PostUpdate,
            update_far_handoffs
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        );
    }
}

/// How far the camera moves before the objects in range are looked for
/// again (m); spawning reaches this much past the viewer's radii.
const RECHECK: f32 = 25.0;
/// The same for the `_Far` stand-ins.
const FAR_RECHECK: f32 = 100.0;
/// Side of the grid squares the actors are sorted into (m).
const BUCKET: f32 = 100.0;
/// Squares per cell side.
const BUCKETS: usize = (CELL_SIZE / BUCKET) as usize;
/// Side of the `_Far` index's grid squares (m).
const FAR_BUCKET: f32 = 500.0;
/// Loaded cells farther than the cell probe's reach plus this go (m).
const EVICT_SLACK: f32 = 500.0;

/// A placed object that has been spawned, with its actor name.
#[derive(Component)]
pub struct PlacedObject(pub Arc<str>);

/// A `_Far` stand-in.
#[derive(Component)]
pub struct FarObject;

/// Shrinks an object to nothing between `start` and `end` metres from the
/// camera, so it leaves the view without popping.
#[derive(Component)]
struct EdgeFade {
    scale: Vec3,
    start: f32,
    end: f32,
}

// SI-WLD-04: EdgeFade shrinks objects at the range edge (our model).
impl EdgeFade {
    fn new(transform: &Transform, end: f32) -> Self {
        Self {
            scale: transform.scale,
            start: end * 0.88,
            end,
        }
    }

    /// The scale factor at `distance`.
    fn factor(&self, distance: f32) -> f32 {
        let t = ((self.end - distance) / (self.end - self.start)).clamp(0.0, 1.0);
        // Never exactly zero: degenerate transforms upset normals and bounds.
        (t * t * (3.0 - 2.0 * t)).max(0.01)
    }
}

#[derive(Resource)]
pub struct Objects {
    assets: PathBuf,
    units: Units,
    cells: HashMap<String, Cell>,
    /// Spawned objects by hash id.
    spawned: HashMap<u32, Spawned>,
    /// Actors known to have no model.
    no_model: HashSet<Arc<str>>,
    pub spawn_radius: f32,
    pub despawn_radius: f32,
    /// Big objects show out to here.
    pub far_radius: f32,
    /// Models reaching this far from their origin (metres) count as big.
    pub large_size: f32,
    /// Everything in range that has a model was spawned in the last pass.
    settled: bool,
    /// Where the camera was at the last pass.
    checked_at: Option<Vec2>,
}

struct Spawned {
    entity: Entity,
    /// The distance it goes at.
    end: f32,
    at: Vec2,
}

enum Units {
    None,
    Loading(Task<Option<Arc<HashSet<String>>>>),
    /// The baked cells.
    Ready(Arc<HashSet<String>>),
}

enum Cell {
    Loading(Task<CellActors>),
    Ready(Arc<CellActors>),
}

/// A placed actor, ready to spawn.
struct Placed {
    name: Arc<str>,
    hash_id: u32,
    transform: Transform,
}

impl Placed {
    fn at(&self) -> Vec2 {
        self.transform.translation.xz()
    }
}

/// A cell's actors the objects may draw, sorted into a grid.
struct CellActors {
    origin: Vec2,
    actors: Vec<Placed>,
    /// `BUCKETS`² squares (row-major, z then x): indices into `actors`.
    buckets: Vec<Vec<u32>>,
}

impl CellActors {
    fn new(cell: &str, baked: &PlacedActors) -> Self {
        let origin = Vec2::from(cell_origin(cell).unwrap_or_default());
        let names: Vec<Arc<str>> = baked.names.iter().map(|n| Arc::from(n.as_str())).collect();
        let actors: Vec<Placed> = baked
            .actors
            .iter()
            .filter_map(|actor| {
                let name = names.get(actor.name as usize)?.clone();
                (!skipped(&name)).then(|| Placed {
                    name,
                    hash_id: actor.hash_id,
                    transform: placement(actor),
                })
            })
            .collect();
        let mut buckets = vec![Vec::new(); BUCKETS * BUCKETS];
        for (i, actor) in actors.iter().enumerate() {
            let local = (actor.at() - origin) / BUCKET;
            let x = (local.x.floor().max(0.0) as usize).min(BUCKETS - 1);
            let z = (local.y.floor().max(0.0) as usize).min(BUCKETS - 1);
            buckets[z * BUCKETS + x].push(i as u32);
        }
        Self {
            origin,
            actors,
            buckets,
        }
    }

    /// The actors whose grid square comes within `reach` of `eye`.
    fn near(&self, eye: Vec2, reach: f32) -> impl Iterator<Item = &Placed> {
        let cells = BUCKETS as f32;
        let lo = ((eye - reach - self.origin) / BUCKET)
            .floor()
            .max(Vec2::ZERO);
        let hi = ((eye + reach - self.origin) / BUCKET)
            .floor()
            .min(Vec2::splat(cells - 1.0));
        let (x0, z0, x1, z1) = (lo.x as usize, lo.y as usize, hi.x as i64, hi.y as i64);
        (z0 as i64..=z1)
            .flat_map(move |z| (x0 as i64..=x1).map(move |x| (x as usize, z as usize)))
            .flat_map(move |(x, z)| self.buckets[z * BUCKETS + x].iter())
            .map(move |&i| &self.actors[i as usize])
    }
}

/// Distance in the ground plane from `eye` to the square at `origin` of
/// side `size` (0 inside).
fn square_distance(eye: Vec2, origin: Vec2, size: f32) -> f32 {
    let d = (origin - eye).max(eye - (origin + size)).max(Vec2::ZERO);
    d.length()
}

impl Objects {
    /// Rebuild ranges and fades after changing viewer settings.
    pub fn refresh(&mut self, commands: &mut Commands) {
        for (_, spawned) in self.spawned.drain() {
            commands.entity(spawned.entity).despawn();
        }
        self.checked_at = None;
        self.settled = false;
    }

    pub fn spawned(&self) -> usize {
        self.spawned.len()
    }

    /// The map around the camera is read and every object in range that
    /// has a model stands in it.
    pub fn is_settled(&self) -> bool {
        matches!(self.units, Units::None) || self.settled
    }
}

/// The map's `_Far` actors: low-detail stand-ins the game shows where the
/// real object is out of range (the baked index around the region).
#[derive(Resource)]
pub struct FarModels {
    index: FarIndex,
    spawned: HashMap<u32, (Entity, Vec2)>,
    /// Names of the actors that have a `_Far` stand-in.
    stand_ins: HashSet<String>,
    pub horizon: f32,
    /// Every stand-in within the horizon was spawned in the last pass.
    settled: bool,
    /// Where the camera was at the last pass.
    checked_at: Option<Vec2>,
}

/// A stand-in only leaves when this exact placed instance has a drawable
/// near model. Another instance of the same actor must not hide it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct InstanceKey {
    name: Arc<str>,
    position: [u32; 3],
}

impl InstanceKey {
    pub(crate) fn new(name: Arc<str>, position: Vec3) -> Self {
        Self {
            name,
            position: position
                .to_array()
                .map(|v| if v == 0.0 { 0 } else { v.to_bits() }),
        }
    }
}

#[derive(Component)]
pub(crate) struct FarHandoff(InstanceKey);

#[derive(Resource, Default)]
pub(crate) struct ReadyNearModels(pub HashSet<InstanceKey>);

/// SI-WLD-08: keep the far model while its near replacement is missing/loading.
/// Global transforms match the distance used by the meshes' VisibilityRange.
pub(crate) fn update_far_handoffs(
    camera: Query<&GlobalTransform, crate::camera::MainCamera>,
    objects: Res<Objects>,
    near: Query<(&PlacedObject, &GlobalTransform, &Children)>,
    meshes: Query<(), With<Mesh3d>>,
    mut far: Query<(&FarHandoff, &mut Visibility)>,
    mut available: ResMut<ReadyNearModels>,
) {
    let Ok(camera) = camera.single() else { return };
    let eye = camera.translation();
    let ready: HashSet<_> = near
        .iter()
        .filter(|(_, transform, children)| {
            transform.translation().distance(eye) < objects.far_radius
                && children.iter().any(|child| meshes.contains(child))
        })
        .map(|(object, transform, _)| InstanceKey::new(object.0.clone(), transform.translation()))
        .collect();
    if available.0 != ready {
        available.0 = ready;
    }
    for (handoff, mut visibility) in &mut far {
        let wanted = if available.0.contains(&handoff.0) {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}

enum FarIndex {
    /// Nothing baked.
    None,
    Loading(Task<Option<FarActors>>),
    Ready(Arc<FarActors>),
}

struct FarActors {
    actors: Vec<Placed>,
    stand_ins: Vec<String>,
    /// Grid square → indices into `actors`, in the index's order.
    grid: HashMap<(i32, i32), Vec<u32>>,
}

impl FarActors {
    fn new(baked: &BakedFarIndex) -> Self {
        let names: Vec<Arc<str>> = baked
            .placed
            .names
            .iter()
            .map(|n| Arc::from(n.as_str()))
            .collect();
        let actors: Vec<Placed> = baked
            .placed
            .actors
            .iter()
            .filter_map(|actor| {
                Some(Placed {
                    name: names.get(actor.name as usize)?.clone(),
                    hash_id: actor.hash_id,
                    transform: placement(actor),
                })
            })
            .collect();
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, actor) in actors.iter().enumerate() {
            let at = (actor.at() / FAR_BUCKET).floor();
            grid.entry((at.x as i32, at.y as i32))
                .or_default()
                .push(i as u32);
        }
        Self {
            actors,
            stand_ins: baked.stand_ins.clone(),
            grid,
        }
    }

    /// Indices of the actors whose grid square comes within `reach` of
    /// `eye`, in the index's order.
    fn near(&self, eye: Vec2, reach: f32) -> Vec<u32> {
        let lo = ((eye - reach) / FAR_BUCKET).floor();
        let hi = ((eye + reach) / FAR_BUCKET).floor();
        let mut found: Vec<u32> = (lo.y as i32..=hi.y as i32)
            .flat_map(|z| (lo.x as i32..=hi.x as i32).map(move |x| (x, z)))
            .filter_map(|square| self.grid.get(&square))
            .flatten()
            .copied()
            .collect();
        found.sort_unstable();
        found
    }
}

impl FarModels {
    /// Rebuild stand-in ranges after changing viewer settings.
    pub fn refresh(&mut self, commands: &mut Commands) {
        for (_, (entity, _)) in self.spawned.drain() {
            commands.entity(entity).despawn();
        }
        self.checked_at = None;
        self.settled = false;
    }

    pub fn spawned(&self) -> usize {
        self.spawned.len()
    }

    /// Every stand-in within the horizon is spawned (or there are none).
    pub fn is_settled(&self) -> bool {
        matches!(self.index, FarIndex::None) || self.settled
    }

    /// The index is read: which actors have a stand-in is known.
    fn is_indexed(&self) -> bool {
        !matches!(self.index, FarIndex::Loading(_))
    }
}

fn stream_far_models(
    mut commands: Commands,
    mut far: ResMut<FarModels>,
    mut library: ResMut<ModelLibrary>,
    cameras: Query<&GlobalTransform, crate::camera::MainCamera>,
) {
    let far = &mut *far;
    match &mut far.index {
        FarIndex::None => return,
        FarIndex::Loading(task) => {
            far.settled = false;
            if let Some(index) = block_on(poll_once(task)) {
                far.index = match index {
                    Some(actors) => {
                        far.stand_ins = actors.stand_ins.iter().cloned().collect();
                        FarIndex::Ready(Arc::new(actors))
                    }
                    None => FarIndex::None,
                };
            }
            return;
        }
        FarIndex::Ready(_) => {}
    }
    let (FarIndex::Ready(actors), Ok(camera)) = (&far.index, cameras.single()) else {
        return;
    };
    let eye = camera.translation().xz();
    let moved = far
        .checked_at
        .is_none_or(|at| at.distance(eye) > FAR_RECHECK);
    if !moved && far.settled {
        return;
    }
    far.checked_at = Some(eye);
    // Near visibility is controlled by a per-instance readiness handoff;
    // stand-ins remain available when detailed assets were not baked.
    let horizon = far.horizon;
    // SI-WLD-03: object draw ranges and LOD choice are ours.
    let range = VisibilityRange::abrupt(0.0, horizon);
    // SI-WLD-03: object draw ranges and LOD choice are ours.
    let gone = horizon * 1.05 + FAR_RECHECK;
    far.spawned.retain(|_, &mut (entity, at)| {
        let keep = at.distance(eye) <= gone;
        if !keep {
            commands.entity(entity).despawn();
        }
        keep
    });
    let mut budget = 100;
    let mut loading = 0;
    // Stand-ins in range left for a later frame.
    let mut deferred = 0;
    let reach = horizon + FAR_RECHECK;
    for i in actors.near(eye, reach) {
        let actor = &actors.actors[i as usize];
        if far.spawned.contains_key(&actor.hash_id) || actor.at().distance(eye) >= reach {
            continue;
        }
        if budget == 0 || loading >= 16 {
            deferred += 1;
            continue;
        }
        match library.actor(&actor.name) {
            ActorModel::Loading => loading += 1,
            ActorModel::None => {}
            ActorModel::Ready(parts) => {
                budget -= 1;
                let transform = actor.transform;
                let entity = commands
                    .spawn((
                        Name::new(actor.name.to_string()),
                        FarObject,
                        FarHandoff(InstanceKey::new(
                            actor
                                .name
                                .strip_suffix("_Far")
                                .unwrap_or(&actor.name)
                                .into(),
                            transform.translation,
                        )),
                        EdgeFade::new(&transform, horizon),
                        transform,
                        Visibility::default(),
                    ))
                    .with_children(|object| {
                        for part in parts.iter() {
                            // The coarsest mesh: these are only ever seen from afar.
                            // SI-WLD-03: object draw ranges and LOD choice are ours.
                            let mesh = part.lods.last().unwrap_or(&part.mesh);
                            // SI-MWT-04: translucent model materials stay out of the cube map.
                            if let Some(water) = &part.water_material {
                                object.spawn((
                                    Mesh3d(mesh.clone()),
                                    MeshMaterial3d(water.clone()),
                                    range.clone(),
                                    NotShadowCaster,
                                ));
                                continue;
                            }
                            let Some(material) = &part.object_material else {
                                continue;
                            };
                            object.spawn((
                                Mesh3d(mesh.clone()),
                                MeshMaterial3d(material.clone()),
                                range.clone(),
                            ));
                        }
                    })
                    .id();
                far.spawned.insert(actor.hash_id, (entity, actor.at()));
            }
        }
    }
    far.settled = loading == 0 && deferred == 0;
}

/// Actors drawn elsewhere (enemies, the weapons they pick up) or not at
/// all yet. Camp props (stands, cooking pots) are drawn here.
// SI-WLD-06: actors skipped by name prefix (our heuristic).
fn skipped(name: &str) -> bool {
    const PREFIXES: [&str; 6] = ["Enemy_", "Npc_", "Animal_", "Weapon_", "Player", "Horse_"];
    // `_Far` actors are low-detail stand-ins the game shows only far away.
    PREFIXES.iter().any(|p| name.starts_with(p)) || name.ends_with("_Far")
}

#[allow(clippy::too_many_arguments)]
fn stream_objects(
    mut commands: Commands,
    look: Res<TerrainLook>,
    far_models: Res<FarModels>,
    far_trees: Res<FarTrees>,
    mut objects: ResMut<Objects>,
    mut library: ResMut<ModelLibrary>,
    cameras: Query<&GlobalTransform, crate::camera::MainCamera>,
) {
    let Ok(camera) = cameras.single() else { return };
    let objects = &mut *objects;
    if look.is_loading() {
        objects.settled = false;
        return; // Rocks and cliffs need the terrain textures.
    }
    let eye = camera.translation().xz();

    if let Units::Loading(task) = &mut objects.units
        && let Some(result) = block_on(poll_once(task))
    {
        objects.units = match result {
            Some(cells) => Units::Ready(cells),
            None => Units::None,
        };
    }
    let Units::Ready(baked) = &objects.units else {
        objects.settled = false;
        return;
    };
    // How far trees reach depends on their billboards; which objects hand
    // over to a stand-in, on the stand-ins.
    if far_trees.is_loading() || !far_models.is_indexed() {
        objects.settled = false;
        return;
    }

    let mut cells_done = false;
    for cell in objects.cells.values_mut() {
        if let Cell::Loading(task) = cell
            && let Some(actors) = block_on(poll_once(task))
        {
            *cell = Cell::Ready(Arc::new(actors));
            cells_done = true;
        }
    }
    let moved = objects
        .checked_at
        .is_none_or(|at| at.distance(eye) > RECHECK);
    if !moved && !cells_done && objects.settled {
        return;
    }
    objects.checked_at = Some(eye);

    // Load the cells around the camera (cells are 1000 m, so a 3 × 3 probe
    // at the far radius finds every cell it touches); drop those far behind.
    let r = objects.far_radius * 1.1;
    let probes = [-r, 0.0, r];
    let wanted_cells: HashSet<String> = probes
        .iter()
        .flat_map(|&dx| probes.iter().map(move |&dz| (dx, dz)))
        .filter_map(|(dx, dz)| cell_name(eye.x + dx, eye.y + dz))
        .filter(|name| baked.contains(name))
        .collect();
    objects.cells.retain(|name, _| {
        wanted_cells.contains(name)
            || cell_origin(name)
                .is_some_and(|o| square_distance(eye, Vec2::from(o), CELL_SIZE) < r + EVICT_SLACK)
    });
    for name in wanted_cells {
        objects.cells.entry(name.clone()).or_insert_with(|| {
            let path = objects.assets.join(paths::object_cell(&name));
            Cell::Loading(AsyncComputeTaskPool::get().spawn(async move {
                let baked = asset_format::read_ron::<PlacedActors>(&path)
                    .inspect_err(|e| warn!("objects: {e}"))
                    .unwrap_or_default();
                CellActors::new(&name, &baked)
            }))
        });
    }
    let cells_loading = objects
        .cells
        .values()
        .any(|cell| matches!(cell, Cell::Loading(_)));

    // Drop what is out of range.
    objects.spawned.retain(|_, spawned| {
        let keep = spawned.at.distance(eye) <= spawned.end + RECHECK;
        if !keep {
            commands.entity(spawned.entity).despawn();
        }
        keep
    });

    // Spawn what is near and has a model, nearest first; a few per frame
    // keep frames smooth, and a cap on models loading at once keeps the
    // nearest ones from waiting behind the horizon.
    let mut budget = 200;
    let mut loading = 0;
    let (spawn, far) = (objects.spawn_radius, objects.far_radius);
    let reach = far + RECHECK;
    let ready: Vec<Arc<CellActors>> = objects
        .cells
        .values()
        .filter_map(|cell| match cell {
            Cell::Ready(actors) => Some(actors.clone()),
            Cell::Loading(_) => None,
        })
        .filter(|cell| square_distance(eye, cell.origin, CELL_SIZE) < reach)
        .collect();
    let mut wanted: Vec<(f32, &Placed)> = Vec::new();
    for cell in &ready {
        for actor in cell.near(eye, reach) {
            if objects.spawned.contains_key(&actor.hash_id)
                || objects.no_model.contains(&actor.name)
            {
                continue;
            }
            let distance = actor.at().distance(eye);
            if distance < reach {
                wanted.push((distance, actor));
            }
        }
    }
    wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut deferred = false;
    for (distance, actor) in wanted {
        if budget == 0 || loading >= 24 {
            deferred = true;
            break;
        }
        match library.actor(&actor.name) {
            ActorModel::Loading => loading += 1,
            ActorModel::None => {
                objects.no_model.insert(actor.name.clone());
            }
            ActorModel::Ready(parts) => {
                let size = parts.iter().map(|p| p.radius).fold(0.0, f32::max)
                    * actor.transform.scale.max_element();
                let large = size >= objects.large_size;
                // Trees with a billboard hand over to it; other big objects
                // reach far, the rest end at the spawn radius.
                let billboard = far_trees.hand_off(&actor.name);
                let fade_end = match billboard {
                    Some(hand_off) => hand_off.min(far),
                    None if large || far_models.stand_ins.contains(&*actor.name) => far,
                    None => spawn,
                };
                if distance >= fade_end + RECHECK {
                    continue;
                }
                budget -= 1;
                // Objects with a stand-in or a billboard hand over to it (a
                // tree's billboard has its silhouette); the rest shrink away.
                let edge_fade = billboard.is_none() && !far_models.stand_ins.contains(&*actor.name);
                let entity = spawn_object(
                    &mut commands,
                    &library,
                    actor,
                    &parts,
                    ObjectRange {
                        size,
                        fade_end,
                        edge_fade,
                        dissolve_from: billboard
                            .map(|_| crate::far_trees::dissolve_start(fade_end)),
                    },
                );
                objects.spawned.insert(
                    actor.hash_id,
                    Spawned {
                        entity,
                        // SI-WLD-03: object draw ranges and LOD choice are ours.
                        end: fade_end + (objects.despawn_radius - spawn),
                        at: actor.at(),
                    },
                );
            }
        }
    }

    objects.settled = !cells_loading && !deferred && loading == 0;
}

/// How far a placed object shows and how it leaves.
struct ObjectRange {
    /// How far its model reaches from its origin, scaled.
    size: f32,
    /// Where it ends.
    fade_end: f32,
    /// It shrinks away at its end.
    edge_fade: bool,
    /// It dissolves into its billboard from here.
    dissolve_from: Option<f32>,
}

fn spawn_object(
    commands: &mut Commands,
    library: &ModelLibrary,
    actor: &Placed,
    parts: &[Part],
    range: ObjectRange,
) -> Entity {
    // Big models keep their detail farther out.
    // SI-WLD-05: model LOD switch distances are fitted.
    let reach = (range.size / LOD_REFERENCE_SIZE).clamp(1.0, 8.0);
    let transform = actor.transform;
    let mut entity = commands.spawn((
        Name::new(actor.name.to_string()),
        PlacedObject(actor.name.clone()),
        transform,
        Visibility::default(),
    ));
    if range.edge_fade {
        entity.insert(EdgeFade::new(&transform, range.fade_end));
    }
    if let Some(occluder) = library.sky_occluder(&actor.name) {
        entity.insert(occluder);
    }
    entity
        .with_children(|object| {
            for part in parts {
                let Some(material) = &part.object_material else {
                    continue;
                };
                let water = part.water_material.as_ref();
                let meshes: Vec<&Handle<Mesh>> =
                    std::iter::once(&part.mesh).chain(&part.lods).collect();
                for (level, mesh) in meshes.iter().enumerate() {
                    let visible = lod_range(
                        level,
                        meshes.len(),
                        reach,
                        range.fade_end,
                        range.dissolve_from,
                    );
                    // A level the distances leave no room for never shows.
                    if visible.end_margin.end <= 0.0 {
                        continue;
                    }
                    // Model water and glass: their own material, drawn
                    // in the transmissive phase; no shadow.
                    // SI-MWT-04: translucent model materials stay out of the cube map.
                    if let Some(water) = water {
                        object.spawn((
                            Mesh3d((*mesh).clone()),
                            MeshMaterial3d(water.clone()),
                            visible,
                            NotShadowCaster,
                        ));
                        continue;
                    }
                    let mut lod = object.spawn((
                        Mesh3d((*mesh).clone()),
                        MeshMaterial3d(material.clone()),
                        visible.clone(),
                    ));
                    if !part.casts_shadows {
                        lod.insert(NotShadowCaster);
                    }
                    if !part.in_cube_map {
                        continue;
                    }
                    // The cube map draws a glowing material with a program
                    // of its own (`Part::cube_material`): a copy of the
                    // level for the cube map only.
                    match &part.cube_material {
                        None => {
                            lod.insert(crate::cubemap::in_cube_map());
                        }
                        Some(cube) => {
                            object.spawn((
                                Mesh3d((*mesh).clone()),
                                MeshMaterial3d(cube.clone()),
                                visible,
                                NotShadowCaster,
                                RenderLayers::layer(crate::cubemap::CUBE_LAYER),
                            ));
                        }
                    }
                }
            }
        })
        .id()
}

/// Scales objects between their `EdgeFade` distances from the camera.
fn fade_edges(
    cameras: Query<&GlobalTransform, crate::camera::MainCamera>,
    mut objects: Query<(&EdgeFade, &mut Transform)>,
) {
    let Ok(camera) = cameras.single() else { return };
    let eye = camera.translation();
    for (fade, mut transform) in &mut objects {
        let scale = fade.scale * fade.factor(transform.translation.distance(eye));
        // Only touch objects whose scale changes, to keep change detection quiet.
        if transform.scale.distance_squared(scale) > 1e-8 {
            transform.scale = scale;
        }
    }
}

/// Distances where each level of detail takes over for a model of
/// `LOD_REFERENCE_SIZE`: full detail up close, the coarsest out to the end.
/// Switches are abrupt (the levels look alike at those distances; a
/// dithered crossfade would show a screen-door pattern). Bigger models
/// switch farther out.
// SI-WLD-05: model LOD switch distances are fitted.
const LOD_SWITCHES: [f32; 2] = [45.0, 110.0];
const LOD_REFERENCE_SIZE: f32 = 5.0;

/// The visibility range of level `level` of `levels`, with the switches
/// pushed out by `reach`, the last level shown up to `end`, dissolving from
/// `dissolve_from` on if given.
fn lod_range(
    level: usize,
    levels: usize,
    reach: f32,
    end: f32,
    dissolve_from: Option<f32>,
) -> VisibilityRange {
    let last_switch = dissolve_from.unwrap_or(end).min(end * 0.85);
    let switches: Vec<f32> = LOD_SWITCHES
        .iter()
        .map(|d| d * reach)
        .take(levels.saturating_sub(1))
        .filter(|&d| d < last_switch)
        .collect();
    let levels = switches.len() + 1;
    if level >= levels {
        // A level beyond the distances in use: never shown.
        return VisibilityRange::abrupt(0.0, 0.0);
    }
    let start = if level == 0 { 0.0 } else { switches[level - 1] };
    if level + 1 == levels
        && let Some(from) = dissolve_from
    {
        return VisibilityRange {
            start_margin: start..start,
            end_margin: from..end,
            use_aabb: false,
        };
    }
    let end = if level + 1 == levels {
        end
    } else {
        switches[level]
    };
    VisibilityRange::abrupt(start, end)
}

/// The object's transform from its map placement (rotation in radians).
fn placement(actor: &PlacedActor) -> Transform {
    let [x, y, z] = actor.rotate;
    Transform::from_translation(Vec3::from(actor.translate))
        .with_rotation(Quat::from_euler(EulerRot::ZYX, z, y, x))
        .with_scale(Vec3::from(actor.scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_far_instance_waits_for_its_drawable_near_replacement() {
        let mut app = App::new();
        app.insert_resource(Objects {
            assets: PathBuf::new(),
            units: Units::None,
            cells: HashMap::new(),
            spawned: HashMap::new(),
            no_model: HashSet::new(),
            spawn_radius: 220.0,
            despawn_radius: 240.0,
            far_radius: 700.0,
            large_size: 4.0,
            settled: true,
            checked_at: None,
        })
        .init_resource::<ReadyNearModels>()
        .add_systems(Update, update_far_handoffs);
        let camera = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                crate::camera::MainView,
                GlobalTransform::IDENTITY,
            ))
            .id();
        let at = Vec3::new(100.0, 0.0, 0.0);
        let far = app
            .world_mut()
            .spawn((
                FarHandoff(InstanceKey::new("Rock".into(), at)),
                Visibility::Inherited,
            ))
            .id();
        let other = app
            .world_mut()
            .spawn((
                FarHandoff(InstanceKey::new("Rock".into(), at + Vec3::X)),
                Visibility::Inherited,
            ))
            .id();
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(far).unwrap(),
            Visibility::Inherited
        );
        // An actor without drawable children is still loading or unsupported.
        let near = app
            .world_mut()
            .spawn((
                PlacedObject("Rock".into()),
                GlobalTransform::from_translation(at),
            ))
            .id();
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(far).unwrap(),
            Visibility::Inherited
        );
        app.world_mut().entity_mut(near).with_children(|parent| {
            parent.spawn(Mesh3d(Handle::default()));
        });
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(far).unwrap(),
            Visibility::Hidden
        );
        assert_eq!(
            *app.world().get::<Visibility>(other).unwrap(),
            Visibility::Inherited
        );
        // Crossing the near range re-enables the stand-in without unloading it.
        *app.world_mut().get_mut::<GlobalTransform>(camera).unwrap() =
            GlobalTransform::from_translation(Vec3::new(1000.0, 0.0, 0.0));
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(far).unwrap(),
            Visibility::Inherited
        );
        *app.world_mut().get_mut::<GlobalTransform>(camera).unwrap() = GlobalTransform::IDENTITY;
        app.world_mut().despawn(near);
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(far).unwrap(),
            Visibility::Inherited
        );
    }

    #[test]
    fn levels_of_detail_hand_over_without_gaps() {
        let ranges: Vec<VisibilityRange> = (0..3)
            .map(|level| lod_range(level, 3, 1.0, 220.0, None))
            .collect();
        assert!(
            ranges.iter().all(VisibilityRange::is_abrupt),
            "no dithered crossfades"
        );
        assert_eq!(ranges[0].start_margin, 0.0..0.0);
        assert_eq!(ranges[0].end_margin, ranges[1].start_margin);
        assert_eq!(ranges[1].end_margin, ranges[2].start_margin);
        assert_eq!(ranges[2].end_margin.end, 220.0);
        let single = lod_range(0, 1, 1.0, 220.0, None);
        assert_eq!(
            (single.start_margin, single.end_margin),
            (0.0..0.0, 220.0..220.0)
        );
        // Switches that would fall past the end are dropped.
        let short = lod_range(1, 3, 1.0, 100.0, None);
        assert_eq!(short.end_margin, 100.0..100.0);
        assert!(
            lod_range(2, 3, 1.0, 100.0, None).end_margin.end == 0.0,
            "unused level stays hidden"
        );
        // A big model keeps full detail farther out.
        assert_eq!(lod_range(0, 3, 2.0, 220.0, None).end_margin, 90.0..90.0);
    }

    #[test]
    fn trees_dissolve_at_the_end_of_their_last_level() {
        let ranges: Vec<VisibilityRange> = (0..3)
            .map(|level| lod_range(level, 3, 1.0, 150.0, Some(120.0)))
            .collect();
        assert!(ranges[0].is_abrupt() && ranges[1].is_abrupt());
        assert_eq!(ranges[1].end_margin, ranges[2].start_margin);
        assert_eq!(ranges[2].end_margin, 120.0..150.0);
        // No switch inside the dissolve.
        let big: Vec<VisibilityRange> = (0..3)
            .map(|level| lod_range(level, 3, 2.5, 150.0, Some(120.0)))
            .collect();
        assert_eq!(big[1].end_margin, 120.0..150.0);
        assert_eq!(big[2].end_margin.end, 0.0);
    }

    #[test]
    fn objects_shrink_away_at_the_edge() {
        let fade = EdgeFade::new(&Transform::from_scale(Vec3::splat(2.0)), 100.0);
        assert_eq!(fade.factor(10.0), 1.0);
        assert_eq!(fade.factor(88.0), 1.0);
        assert!((fade.factor(94.0) - 0.5).abs() < 1e-5);
        assert_eq!(fade.factor(100.0), 0.01);
    }

    #[test]
    fn the_grid_finds_every_actor_in_reach() {
        let mut baked = PlacedActors::default();
        let mut lookup = HashMap::new();
        // H-6: x 2000..3000, z 1000..2000; a scatter over it.
        for i in 0..400u32 {
            let x = 2000.0 + (i * 37 % 1000) as f32 + 0.5;
            let z = 1000.0 + (i * 91 % 1000) as f32 + 0.25;
            let actor = PlacedActor {
                name: 0,
                hash_id: i,
                translate: [x, 0.0, z],
                rotate: [0.0; 3],
                scale: [1.0; 3],
            };
            baked.push(&mut lookup, "Obj_Rock", actor);
        }
        baked.push(
            &mut lookup,
            "Enemy_Bokoblin",
            PlacedActor {
                name: 0,
                hash_id: 999,
                translate: [2500.0, 0.0, 1500.0],
                rotate: [0.0; 3],
                scale: [1.0; 3],
            },
        );
        let cell = CellActors::new("H-6", &baked);
        assert_eq!(cell.actors.len(), 400, "skipped actors are left out");
        for (eye, reach) in [
            (Vec2::new(2500.0, 1500.0), 120.0),
            (Vec2::new(1900.0, 900.0), 300.0),
            (Vec2::new(3400.0, 1500.0), 725.0),
        ] {
            let mut found: Vec<u32> = cell
                .near(eye, reach)
                .filter(|a| a.at().distance(eye) < reach)
                .map(|a| a.hash_id)
                .collect();
            let mut all: Vec<u32> = cell
                .actors
                .iter()
                .filter(|a| a.at().distance(eye) < reach)
                .map(|a| a.hash_id)
                .collect();
            found.sort();
            all.sort();
            assert_eq!(found, all);
        }
        assert_eq!(cell.near(Vec2::new(5000.0, 5000.0), 100.0).count(), 0);
    }
}
