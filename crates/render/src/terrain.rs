//! Streams terrain tiles around the camera, following the game's quadtree.
//!
//! Each frame the tree is walked from the root. When the camera is close
//! relative to a tile's size, each quadrant of the tile is replaced by the
//! child tile covering it, once that child is loaded. Finer tiles only exist
//! where the game needs the detail, so the parent's quadrant stays in place of
//! children that do not exist or are still loading; there are never holes.
//! Tiles are loaded and meshed on the async compute pool.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use asset_format::terrain::{MAX_LOD, TileId, WORLD_HEIGHT};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::clouds::{CloudParams, CloudShadows};
use crate::deferred_light::{CubeMean, SsaoNoise};
use crate::look::LookTexture;
use crate::mesh::{self, ColorMode, TileMesh, WaterSource};
use crate::source::TerrainSource;
use crate::terrain_material::{self, TerrainExtension, TerrainLook, TerrainMaterial};
use crate::water_material::{self, WaterLook};

pub struct TerrainPlugin {
    pub source: Arc<TerrainSource>,
    pub settings: TerrainSettings,
}

impl Plugin for TerrainPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings.clone())
            .insert_resource(TerrainStream::new(self.source.clone()))
            .insert_resource(TerrainStats {
                source: self.source.describe(),
                ..default()
            })
            .add_systems(Update, (finish_loads, update_tiles).chain());
    }
}

#[derive(Resource, Clone, Debug)]
pub struct TerrainSettings {
    /// Quads per tile edge (the source data has 255).
    pub mesh_resolution: u32,
    /// Split a tile when the camera is closer than `world_size × split_factor`.
    pub split_factor: f32,
    pub max_lod: u8,
    pub max_loads_in_flight: usize,
    /// Loaded tiles to keep before evicting the least recently used.
    pub cache_budget: usize,
    pub color_mode: ColorMode,
}

impl Default for TerrainSettings {
    fn default() -> Self {
        Self {
            // SI-WLD-07: terrain LOD split and mesh resolution are ours.
            mesh_resolution: 64,
            split_factor: 2.0,
            max_lod: MAX_LOD,
            max_loads_in_flight: 12,
            cache_budget: 1500,
            color_mode: ColorMode::default(),
        }
    }
}

#[derive(Resource, Default, Debug)]
pub struct TerrainStats {
    pub source: String,
    pub visible_per_lod: [u32; MAX_LOD as usize + 1],
    pub loading: usize,
    pub resident: usize,
    pub failures: usize,
    pub last_error: Option<String>,
}

impl TerrainStats {
    /// Tiles are shown and none is loading.
    pub fn settled(&self) -> bool {
        self.loading == 0 && self.resident > 0
    }
}

#[derive(Component)]
pub struct TerrainTile;

#[derive(Resource)]
pub struct TerrainStream {
    source: Arc<TerrainSource>,
    tiles: HashMap<TileId, Slot>,
    /// Tiles without the game's textures: vertex colours, lit like the rest
    /// (made on first use).
    plain_material: Option<Handle<TerrainMaterial>>,
    frame: u64,
}

enum Slot {
    Loading(Task<Result<Option<TileMesh>, String>>),
    Ready {
        /// One entity per quadrant, in Z-order; water surfaces are children.
        quadrants: [Entity; 4],
        /// Water data for this tile, handed down to children without their own.
        water: Option<WaterSource>,
        min_height: f32,
        max_height: f32,
        last_used: u64,
    },
    Missing,
    Failed,
}

impl TerrainStream {
    fn new(source: Arc<TerrainSource>) -> Self {
        Self {
            source,
            tiles: HashMap::new(),
            plain_material: None,
            frame: 0,
        }
    }

    /// Drops every tile (e.g. after changing how meshes are built).
    pub fn clear(&mut self, commands: &mut Commands) {
        for slot in self.tiles.values() {
            if let Slot::Ready { quadrants, .. } = slot {
                for entity in quadrants {
                    commands.entity(*entity).despawn();
                }
            }
        }
        // Dropping the tasks cancels in-flight loads.
        self.tiles.clear();
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_loads(
    mut commands: Commands,
    mut stream: ResMut<TerrainStream>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut terrain_materials: ResMut<Assets<TerrainMaterial>>,
    look: Res<TerrainLook>,
    water_look: Res<WaterLook>,
    clouds: Option<Res<CloudShadows>>,
    look_texture: Option<Res<LookTexture>>,
    ssao_noise: Option<Res<SsaoNoise>>,
    cube_mean: Option<Res<CubeMean>>,
    settings: Res<TerrainSettings>,
    mut stats: ResMut<TerrainStats>,
) {
    let stream = &mut *stream;
    for (tile, slot) in &mut stream.tiles {
        let Slot::Loading(task) = slot else { continue };
        let Some(result) = block_on(poll_once(task)) else {
            continue;
        };
        *slot = match result {
            Ok(Some(built)) => {
                let (x, z) = tile.world_min();
                // With the game's textures each tile gets a material holding its `.mate`.
                let textured = match (&*look, settings.color_mode) {
                    (
                        TerrainLook::Textured {
                            albedo,
                            normals,
                            table,
                        },
                        ColorMode::Natural,
                    ) => {
                        let mate = match &built.mate {
                            Some(material) => images.add(terrain_material::mate_image(material)),
                            None => images.add(terrain_material::blank_mate()),
                        };
                        Some(
                            terrain_materials.add(TerrainMaterial {
                                base: StandardMaterial {
                                    perceptual_roughness: 1.0,
                                    reflectance: 0.1,
                                    ..default()
                                },
                                extension: TerrainExtension {
                                    albedo: albedo.clone(),
                                    mate,
                                    tile: Vec4::new(x, z, tile.world_size(), 0.0),
                                    table: (**table).clone(),
                                    normals: normals.clone(),
                                    cloud_shadow_map: clouds
                                        .as_ref()
                                        .map(|c| c.shadow_map.clone())
                                        .unwrap_or_default(),
                                    clouds: clouds
                                        .as_ref()
                                        .map_or_else(CloudParams::without_shadows, |c| {
                                            c.params.clone()
                                        }),
                                    look: look_texture
                                        .as_ref()
                                        .map(|l| l.0.clone())
                                        .unwrap_or_default(),
                                    ssao_noise: ssao_noise
                                        .as_ref()
                                        .map(|n| n.0.clone())
                                        .unwrap_or_default(),
                                    cube_mean: cube_mean
                                        .as_ref()
                                        .map(|m| m.0.clone())
                                        .unwrap_or_default(),
                                },
                            }),
                        )
                    }
                    _ => None,
                };
                let plain = || {
                    let (albedo, normals, table) = terrain_material::plain_textures(&mut images);
                    terrain_materials.add(TerrainMaterial {
                        base: StandardMaterial {
                            perceptual_roughness: 1.0,
                            reflectance: 0.1,
                            ..default()
                        },
                        extension: TerrainExtension {
                            albedo,
                            mate: images.add(terrain_material::blank_mate()),
                            tile: Vec4::new(0.0, 0.0, 1.0, terrain_material::PLAIN),
                            table,
                            normals,
                            cloud_shadow_map: clouds
                                .as_ref()
                                .map(|c| c.shadow_map.clone())
                                .unwrap_or_default(),
                            clouds: clouds
                                .as_ref()
                                .map_or_else(CloudParams::without_shadows, |c| c.params.clone()),
                            look: look_texture
                                .as_ref()
                                .map(|l| l.0.clone())
                                .unwrap_or_default(),
                            ssao_noise: ssao_noise
                                .as_ref()
                                .map(|n| n.0.clone())
                                .unwrap_or_default(),
                            cube_mean: cube_mean.as_ref().map(|m| m.0.clone()).unwrap_or_default(),
                        },
                    })
                };
                let material = match textured {
                    Some(material) => material,
                    None => stream.plain_material.get_or_insert_with(plain).clone(),
                };
                let [q0, q1, q2, q3] = built.quadrants;
                let [w0, w1, w2, w3] = built.water;
                let quadrants = [(q0, w0), (q1, w1), (q2, w2), (q3, w3)].map(|(patch, water)| {
                    let mut quadrant = commands.spawn((
                        Name::new(format!("terrain {}", tile.file_stem())),
                        TerrainTile,
                        Mesh3d(meshes.add(patch.mesh)),
                        Transform::from_xyz(x, 0.0, z),
                        Visibility::Hidden,
                    ));
                    // The game draws its terrain into the cube map around
                    // the camera (`TeraTerrain` Opaque: `gsys_cube_map` 1),
                    // not the water or the grass (0).
                    quadrant.insert((
                        MeshMaterial3d(material.clone()),
                        crate::cubemap::in_cube_map(),
                    ));
                    // Children share the quadrant's transform and visibility.
                    if let Some(w) = water {
                        if let Some(mesh) = w.water {
                            quadrant.with_child((
                                Mesh3d(meshes.add(mesh)),
                                MeshMaterial3d(water_look.material.clone()),
                            ));
                        }
                        if let Some(mesh) = w.lava {
                            quadrant.with_child((
                                Mesh3d(meshes.add(mesh)),
                                MeshMaterial3d(water_look.lava.clone()),
                            ));
                        }
                        if let Some(light) = w
                            .lava_glow
                            .and_then(|glow| water_material::lava_light(&glow, tile.world_size()))
                        {
                            quadrant.with_child(light);
                        }
                    }
                    quadrant.id()
                });
                Slot::Ready {
                    quadrants,
                    water: built.water_source,
                    min_height: built.min_height,
                    max_height: built.max_height,
                    last_used: stream.frame,
                }
            }
            Ok(None) => Slot::Missing,
            Err(error) => {
                warn!("terrain tile {} failed: {error}", tile.file_stem());
                stats.failures += 1;
                stats.last_error = Some(error);
                Slot::Failed
            }
        };
    }
}

#[allow(clippy::too_many_arguments)]
fn update_tiles(
    mut commands: Commands,
    look: Res<TerrainLook>,
    settings: Res<TerrainSettings>,
    mut stream: ResMut<TerrainStream>,
    mut stats: ResMut<TerrainStats>,
    camera: Query<&GlobalTransform, crate::camera::MainCamera>,
    mut visibility: Query<&mut Visibility, With<TerrainTile>>,
) {
    let Ok(camera) = camera.single() else { return };
    if look.is_loading() {
        // Tiles built now would need rebuilding once the textures arrive.
        return;
    }
    let eye = camera.translation();
    let stream = &mut *stream;
    stream.frame += 1;
    let frame = stream.frame;
    let max_lod = settings.max_lod.min(stream.source.max_lod());

    let source = stream.source.clone();
    let unloaded_distance = |tile: TileId| {
        let (min, max) = source.bounds(tile).unwrap_or((0.0, WORLD_HEIGHT));
        distance_to_tile(eye, tile, min, max)
    };

    // Visible quadrants, as (tile, quadrant index).
    let mut visible: Vec<(TileId, usize)> = Vec::new();
    let mut wanted: Vec<(f32, TileId)> = Vec::new();
    let mut stack = vec![TileId::ROOT];
    while let Some(tile) = stack.pop() {
        let (min_height, max_height) = match stream.tiles.get_mut(&tile) {
            Some(Slot::Ready {
                min_height,
                max_height,
                last_used,
                ..
            }) => {
                *last_used = frame;
                (*min_height, *max_height)
            }
            Some(_) => continue,
            None => {
                if source.may_have(tile) {
                    wanted.push((unloaded_distance(tile), tile));
                }
                continue;
            }
        };

        // SI-WLD-07: terrain LOD split and mesh resolution are ours.
        let close = distance_to_tile(eye, tile, min_height, max_height)
            < tile.world_size() * settings.split_factor;
        let children = tile.children().filter(|_| close && tile.lod() < max_lod);
        for quadrant in 0..4 {
            let child = children
                .map(|c| c[quadrant])
                .filter(|&child| source.may_have(child));
            match child.map(|child| (child, stream.tiles.get(&child))) {
                Some((child, Some(Slot::Ready { .. }))) => {
                    stack.push(child);
                    continue;
                }
                Some((child, None)) => wanted.push((unloaded_distance(child), child)),
                // Loading, missing or failed children, or no split wanted.
                _ => {}
            }
            visible.push((tile, quadrant));
        }
    }

    // Start the nearest loads first.
    let in_flight = stream
        .tiles
        .values()
        .filter(|slot| matches!(slot, Slot::Loading(_)))
        .count();
    wanted.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, tile) in wanted
        .into_iter()
        .take(settings.max_loads_in_flight.saturating_sub(in_flight))
    {
        let source = source.clone();
        let (resolution, mode) = (settings.mesh_resolution, settings.color_mode);
        let inherited = match tile.parent().and_then(|parent| stream.tiles.get(&parent)) {
            Some(Slot::Ready { water, .. }) => water.clone(),
            _ => None,
        };
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let Some(mut data) = source.load(tile)? else {
                return Ok(None);
            };
            let own = data.water.take();
            let water = own.map(|w| (tile, Arc::new(w))).or(inherited);
            Ok(Some(mesh::build(tile, &data, water, resolution, mode)))
        });
        stream.tiles.insert(tile, Slot::Loading(task));
    }

    // Show exactly the selected quadrants.
    let shown: HashSet<(TileId, usize)> = visible.iter().copied().collect();
    for (tile, slot) in &stream.tiles {
        let Slot::Ready { quadrants, .. } = slot else {
            continue;
        };
        for (quadrant, entity) in quadrants.iter().enumerate() {
            let Ok(mut vis) = visibility.get_mut(*entity) else {
                continue;
            };
            let target = if shown.contains(&(*tile, quadrant)) {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            if *vis != target {
                *vis = target;
            }
        }
    }

    evict_unused(&mut commands, stream, settings.cache_budget);

    stats.visible_per_lod = [0; MAX_LOD as usize + 1];
    let visible_tiles: HashSet<TileId> = visible.iter().map(|(tile, _)| *tile).collect();
    for tile in visible_tiles {
        stats.visible_per_lod[tile.lod() as usize] += 1;
    }
    stats.loading = stream
        .tiles
        .values()
        .filter(|s| matches!(s, Slot::Loading(_)))
        .count();
    stats.resident = stream
        .tiles
        .values()
        .filter(|s| matches!(s, Slot::Ready { .. }))
        .count();
}

/// Despawns the least recently used tiles beyond the budget. Tiles used this
/// frame are never evicted.
fn evict_unused(commands: &mut Commands, stream: &mut TerrainStream, budget: usize) {
    let mut ready: Vec<(u64, TileId)> = stream
        .tiles
        .iter()
        .filter_map(|(tile, slot)| match slot {
            Slot::Ready { last_used, .. } if *last_used < stream.frame => Some((*last_used, *tile)),
            _ => None,
        })
        .collect();
    let resident = stream
        .tiles
        .values()
        .filter(|s| matches!(s, Slot::Ready { .. }))
        .count();
    let excess = resident.saturating_sub(budget);
    if excess == 0 {
        return;
    }
    ready.sort_unstable();
    for (_, tile) in ready.into_iter().take(excess) {
        if let Some(Slot::Ready { quadrants, .. }) = stream.tiles.remove(&tile) {
            for entity in quadrants {
                commands.entity(entity).despawn();
            }
        }
    }
}

fn distance_to_tile(eye: Vec3, tile: TileId, min_height: f32, max_height: f32) -> f32 {
    let (x, z) = tile.world_min();
    let size = tile.world_size();
    let min = Vec3::new(x, min_height, z);
    let max = Vec3::new(x + size, max_height, z + size);
    (min - eye).max(eye - max).max(Vec3::ZERO).length()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_is_zero_inside_and_grows_outside() {
        let tile = TileId::ROOT;
        assert_eq!(
            distance_to_tile(Vec3::new(0.0, 100.0, 0.0), tile, 0.0, 800.0),
            0.0
        );
        assert_eq!(
            distance_to_tile(Vec3::new(0.0, 900.0, 0.0), tile, 0.0, 800.0),
            100.0
        );
        assert_eq!(
            distance_to_tile(Vec3::new(8003.0, 800.0, 8004.0), tile, 0.0, 800.0),
            5.0
        );
    }
}
