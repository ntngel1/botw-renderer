//! Grass (stage 1e): blades wherever the map's grass data (`.grass.extm`)
//! says, as tall and as coloured as it says, in chunks around the camera.
//! Blades use the game's blade texture (`GrassAlb` in `Terrain.Tex1`), its
//! blade G-buffer and field shading, and bend in the game's grass wind
//! (`wind`, `grass.wgsl`).
//! The blades are the game's own: every 3 m cell holds its buffer's `N²`
//! blades of each of the two types (`buffer`), as long and as wide as its
//! blade shader makes them; how many of a cell's blades are drawn and how
//! far they have turned to the far colour follow the game's distances for
//! the type (`lod`), and tufts (`cards`) carry the grass on beyond.

pub(crate) mod buffer;
mod cards;
mod color_map;
mod hidden;
mod interact;
mod lod;
mod wind;

pub use cards::GrassCards;
pub use color_map::GrassColorMap;
pub use interact::{InteractMaps, MowGrass, PressGrass};
pub use wind::GrassWind;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use asset_format::grass::TeraGrass;
use asset_format::paths;
use asset_format::texture::Texture;

use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::NoAutoAabb;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, Face, ShaderType, TextureDimension, TextureFormat,
};
use bevy::shader::ShaderRef;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::heights::HeightSampler;

const SHADER: Handle<Shader> = uuid_handle!("0b7d2e94-51c3-4f6a-9e28-3a1c7b5d4e60");

/// Chunk edge in metres: five of the game's 3 m cells.
const CHUNK: f32 = 5.0 * lod::CELL;
/// Grass below this (of 255) grows blades too short to see: none are built.
const LEAST_GRASS: f32 = 4.0;
/// The longest blade: `3H` of full grass at the longest random, metres.
const LONGEST_BLADE: f32 = lod::GRASS_SCALE * lod::BLADE_HEIGHT * 1.25;
const MAX_BUILDS_IN_FLIGHT: usize = 6;

pub type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassExtension>;

pub struct GrassPlugin {
    /// The `assets/` folder; without the baked grass the blades use a
    /// generated texture and the recorded parameters.
    pub assets: PathBuf,
    pub sampler: HeightSampler,
    /// See [`Grass::reach`].
    pub reach: f32,
}

impl Plugin for GrassPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "grass.wgsl", Shader::from_wgsl);
        let assets = self.assets.clone();
        let texture = AsyncComputeTaskPool::get().spawn(async move { load_texture(&assets) });
        app.add_plugins((
            MaterialPlugin::<GrassMaterial>::default(),
            cards::GrassCardsPlugin {
                assets: self.assets.clone(),
                sampler: self.sampler.clone(),
            },
        ))
        .init_resource::<GrassColorMap>()
        .init_resource::<InteractMaps>()
        .insert_resource(hidden::HiddenGrass::new(self.assets.clone()))
        .add_message::<MowGrass>()
        .add_message::<PressGrass>()
        .add_systems(Startup, setup_wind)
        .insert_resource(Grass {
            sampler: self.sampler.clone(),
            look: Look::Loading(texture),
            chunks: HashMap::new(),
            enabled: true,
            reach: self.reach,
            far_cards: 0,
            settled: false,
        })
        .add_systems(
            Update,
            (
                finish_texture,
                update_chunks,
                follow_clouds,
                color_map::update_color_map,
                show_color_map,
                interact::update_maps,
                blow,
            )
                .chain(),
        );
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct GrassExtension {
    #[uniform(100)]
    pub settings: GrassSettings,
    /// The cloud shadow's texture (see `clouds.rs`).
    #[texture(101)]
    #[sampler(102)]
    pub cloud_shadow_map: Handle<Image>,
    #[uniform(103)]
    pub clouds: crate::clouds::CloudParams,
    /// The shared look values (`look::LookTexture`).
    #[texture(104)]
    pub look: Handle<Image>,
    /// The game's blade texture (`GrassAlb`, tip at the top).
    #[texture(105)]
    #[sampler(106)]
    pub blade: Handle<Image>,
    /// The game's `grass_color` map (`color_map`).
    #[texture(107)]
    #[sampler(108)]
    pub ground: Handle<Image>,
    /// The game's `grass_wind_swell` (`wind`).
    #[texture(109, visibility(vertex))]
    #[sampler(110, visibility(vertex))]
    pub swell: Handle<Image>,
    /// The game's `grass_mow`, `grass_mow_wide` and `grass_lie` (`interact`).
    #[texture(111, visibility(vertex))]
    #[sampler(112, visibility(vertex))]
    pub mow: Handle<Image>,
    #[texture(113, visibility(vertex))]
    #[sampler(114, visibility(vertex))]
    pub mow_wide: Handle<Image>,
    #[texture(115, visibility(vertex))]
    #[sampler(116, visibility(vertex))]
    pub lie: Handle<Image>,
    /// The environment's mean brightness (`deferred_light::CubeMean`).
    #[texture(117, sample_type = "float", filterable = false)]
    pub cube_mean: Handle<Image>,
}

impl MaterialExtension for GrassExtension {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    /// Not in the camera's depth prepass: the prepass would draw the blades
    /// unbent (Bevy's vertex shader), and that depth would hide the swaying
    /// ones.
    fn enable_prepass() -> bool {
        false
    }
}

/// See `grass.wgsl` and `grass_cards.wgsl`.
#[derive(ShaderType, Clone, Debug)]
pub struct GrassSettings {
    /// The game's grass wind: `gsys_environment` 32, 33, 57, 58, 59 and 60
    /// (`wind::GrassWind::environment`).
    pub wind: [Vec4; 6],
    /// The material's swell (`uking_grass_wind_swell_*`): frequency and
    /// per-blade dispersion, both over the world coefficient, the swell's
    /// scale, the world coefficient.
    pub swell: Vec4,
    /// `gsys_environment` 34, 35, 36: `uv = xz·scale + offset` of
    /// `grass_lie`, `grass_mow` and `grass_mow_wide` (`interact`).
    pub maps: [Vec4; 3],
    /// The game's distances for type 0 and type 1 (`lod`). Blades: the
    /// sizes where the full count ends and where the last blade goes, the
    /// far colour's share, unused. Tufts: [`lod::TUFT_DISTANCES`].
    pub lod: [Vec4; 2],
    /// Blades: 1 once the `grass_color` map is filled (until then blades use
    /// the map's grass colour); the blade height `tera+0x10fc` × `T.2`; the
    /// blade type; its blades per cell. Tufts: see `grass_cards.wgsl`.
    pub flags: Vec4,
}

/// A grass chunk entity.
#[derive(Component)]
pub struct GrassChunk;

#[derive(Resource)]
pub struct Grass {
    sampler: HeightSampler,
    look: Look,
    chunks: HashMap<IVec2, Chunk>,
    /// Toggled with G.
    pub enabled: bool,
    /// How many times as far out as the game the grass is drawn (`lod`):
    /// the viewer's draw distance setting (`--grass-reach`), 1 by default,
    /// the game's distances.
    pub reach: f32,
    /// Far grass cards drawn (`cards`).
    pub far_cards: usize,
    /// Every chunk in range was built by the last update.
    settled: bool,
}

enum Look {
    Loading(Task<Textures>),
    /// One material per blade type.
    Ready([Handle<GrassMaterial>; 2]),
}

/// A chunk's blades of each type (mesh and bounds; none where no grass
/// grows) and how many there are.
type Built = ([Option<(Mesh, Aabb)>; 2], usize);

enum Chunk {
    Building(Task<Option<Built>>),
    Ready {
        entities: [Option<Entity>; 2],
        blades: usize,
    },
    /// No grass here.
    Empty,
}

impl Grass {
    /// The blades' texture is in and every chunk in range is built.
    pub fn is_settled(&self) -> bool {
        self.settled
    }

    /// Chunks drawn and blades in them.
    pub fn stats(&self) -> (usize, usize) {
        self.chunks
            .values()
            .filter_map(|chunk| match chunk {
                Chunk::Ready { blades, .. } => Some(*blades),
                _ => None,
            })
            .fold((0, 0), |(chunks, total), blades| {
                (chunks + 1, total + blades)
            })
    }
}

/// The game's blade texture (`GrassAlb`; its blade G-buffer does not read
/// `GrassSpm`, programs 7 and 11 of `uking_grass_blade`) and the blade
/// types' far colour shares (`uking_grass_lod_color.w`).
#[derive(Default)]
struct Textures {
    blade: Option<Texture>,
    far_shares: Option<[f32; 2]>,
    swell: Option<Vec4>,
}

fn load_texture(assets: &Path) -> Textures {
    let blade = Texture::read(&assets.join(paths::GRASS_BLADE))
        .inspect_err(|error| warn!("grass textures unavailable: {error}"))
        .ok();
    let material = load_material(assets);
    if material.far_shares.is_none() {
        warn!("TeraGrass: no uking_grass_lod_color on Blade1/Blade2; using the recorded values");
    }
    Textures {
        blade,
        far_shares: material.far_shares,
        swell: material.blade_swell.as_ref().and_then(wind::swell_of),
    }
}

/// `TeraGrass`'s baked parameters (none without them).
pub(crate) fn load_material(assets: &Path) -> TeraGrass {
    let path = assets.join(paths::GRASS_MATERIAL);
    if !path.exists() {
        return TeraGrass::default();
    }
    asset_format::read_ron(&path)
        .inspect_err(|error| warn!("grass materials unavailable: {error}"))
        .unwrap_or_default()
}

/// A blade-like gradient for when there is no dump.
fn generated_texture() -> Image {
    const HEIGHT: u32 = 32;
    let data = (0..HEIGHT)
        .flat_map(|y| {
            // Row 0 is the tip.
            let t = 1.0 - y as f32 / (HEIGHT - 1) as f32;
            let c = Vec3::new(0.16, 0.30, 0.07).lerp(Vec3::new(0.52, 0.66, 0.24), t);
            let px = [
                (c.x * 255.0) as u8,
                (c.y * 255.0) as u8,
                (c.z * 255.0) as u8,
                255,
            ];
            [px; 4].concat()
        })
        .collect();
    Image::new(
        Extent3d {
            width: 4,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn finish_texture(
    mut grass: ResMut<Grass>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
    clouds: Option<Res<crate::clouds::CloudShadows>>,
    look: Option<Res<crate::look::LookTexture>>,
    color_map: Res<GrassColorMap>,
    wind: Res<GrassWind>,
    maps: Res<InteractMaps>,
    cube_mean: Option<Res<crate::deferred_light::CubeMean>>,
) {
    let Look::Loading(task) = &mut grass.look else {
        return;
    };
    let Some(textures) = block_on(poll_once(task)) else {
        return;
    };
    let layer = |t: Option<Texture>| t.and_then(|t| crate::texture::texture_layer_image(&t, 0));
    let image = layer(textures.blade).unwrap_or_else(generated_texture);
    let far_shares = textures.far_shares.unwrap_or(lod::BLADE_FAR_SHARES);
    let reach = grass.reach;
    let blade_lod = |t: usize| {
        let (full, none) = lod::blade_sizes(t, reach);
        Vec4::new(
            full,
            none,
            far_shares[t],
            lod::GRASS_SCALE * lod::BLADE_WIDTHS[t],
        )
    };
    let per_type = lod::BLADES_PER_SIDE.map(|n| (n * n) as f32);
    let first = GrassMaterial {
        base: StandardMaterial {
            // Both sides lit alike: blades carry the ground's normal, and
            // flipping it for back faces would turn half the grass dark.
            cull_mode: None::<Face>,
            double_sided: false,
            ..default()
        },
        extension: GrassExtension {
            settings: GrassSettings {
                wind: wind.environment(),
                swell: textures.swell.unwrap_or(wind::BLADE_SWELL),
                maps: maps.transforms(),
                lod: [blade_lod(0), blade_lod(1)],
                flags: Vec4::new(0.0, lod::GRASS_SCALE * lod::BLADE_HEIGHT, 0.0, per_type[0]),
            },
            cloud_shadow_map: clouds
                .as_ref()
                .map(|c| c.shadow_map.clone())
                .unwrap_or_default(),
            clouds: clouds
                .as_ref()
                .map_or_else(crate::clouds::CloudParams::without_shadows, |c| {
                    c.params.clone()
                }),
            look: look.map(|l| l.0.clone()).unwrap_or_default(),
            blade: images.add(image),
            ground: color_map.image.clone(),
            swell: wind.swell.clone(),
            mow: maps.mow_image.clone(),
            mow_wide: maps.wide_image.clone(),
            lie: maps.lie_image.clone(),
            cube_mean: cube_mean.as_ref().map(|m| m.0.clone()).unwrap_or_default(),
        },
    };
    let mut second = first.clone();
    second.extension.settings.flags.z = 1.0;
    second.extension.settings.flags.w = per_type[1];
    grass.look = Look::Ready([materials.add(first), materials.add(second)]);
}

/// The grass wind, settled on the first climate's wind power.
fn setup_wind(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    environment: Option<Res<crate::daynight::Environment>>,
) {
    let table = wind::SwellTexture::new();
    let swell = images.add(table.image());
    let power = environment
        .and_then(|e| e.climates.first().map(|c| c.wind_power))
        .unwrap_or(wind::DEFAULT_POWER);
    commands.insert_resource(GrassWind::new(power, swell, table));
}

/// Steps the grass wind with game time and hands it to the blades: the
/// camera's climate's wind power, rerolled every game hour. Also hands them
/// the draw distance (`Grass::reach`).
fn blow(
    time: Res<Time>,
    day: Option<Res<crate::daynight::TimeOfDay>>,
    climate: Option<Res<crate::climate::Climate>>,
    environment: Option<Res<crate::daynight::Environment>>,
    mut wind: ResMut<GrassWind>,
    maps: Res<InteractMaps>,
    grass: Res<Grass>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    if let (Some(climate), Some(environment)) = (&climate, &environment)
        && let Some(defines) = environment.climates.get(climate.current)
    {
        wind.power = defines.wind_power;
    }
    if let Some(day) = &day {
        wind.set_hour((day.day as f32 * 24.0 + day.hours).floor() as i64);
    }
    wind.advance(time.delta_secs());
    let Look::Ready(handles) = &grass.look else {
        return;
    };
    for handle in handles {
        if let Some(mut material) = materials.get_mut(handle) {
            let settings = &mut material.extension.settings;
            settings.wind = wind.environment();
            settings.maps = maps.transforms();
            for (kind, table) in settings.lod.iter_mut().enumerate() {
                (table.x, table.y) = lod::blade_sizes(kind, grass.reach);
            }
        }
    }
}

/// Keeps the blades' cloud shadows on the moving sun.
fn follow_clouds(
    grass: Res<Grass>,
    clouds: Option<Res<crate::clouds::CloudShadows>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    let (Look::Ready(handles), Some(clouds)) = (&grass.look, clouds) else {
        return;
    };
    if !clouds.is_changed() {
        return;
    }
    for handle in handles {
        if let Some(mut material) = materials.get_mut(handle) {
            material.extension.clouds = clouds.params.clone();
        }
    }
}

/// Switches the blades to the `grass_color` map once it is filled.
fn show_color_map(
    grass: Res<Grass>,
    color_map: Res<GrassColorMap>,
    mut materials: ResMut<Assets<GrassMaterial>>,
) {
    let Look::Ready(handles) = &grass.look else {
        return;
    };
    let ready = if color_map.ready { 1.0 } else { 0.0 };
    for handle in handles {
        if materials
            .get(handle)
            .is_some_and(|m| m.extension.settings.flags.x != ready)
            && let Some(mut material) = materials.get_mut(handle)
        {
            material.extension.settings.flags.x = ready;
        }
    }
}

fn update_chunks(
    input: Option<Res<crate::viewer::ViewerInput>>,
    mut commands: Commands,
    mut grass: ResMut<Grass>,
    mut meshes: ResMut<Assets<Mesh>>,
    keys: Res<ButtonInput<KeyCode>>,
    cameras: Query<(&GlobalTransform, &Projection), crate::camera::MainCamera>,
    mut visibility: Query<&mut Visibility, With<GrassChunk>>,
) {
    let grass = &mut *grass;
    if keys.just_pressed(KeyCode::KeyG) && !input.is_some_and(|input| input.keyboard) {
        grass.enabled = !grass.enabled;
    }
    grass.settled = false;
    let Look::Ready(materials) = &grass.look else {
        return;
    };
    let Ok((camera, projection)) = cameras.single() else {
        return;
    };
    let eye = camera.translation().xz();
    let k = lod::lod_k(projection);
    let reach = lod::blade_reach(k, grass.reach);

    // Finish builds.
    for (key, chunk) in grass.chunks.iter_mut() {
        let Chunk::Building(task) = chunk else {
            continue;
        };
        let Some(result) = block_on(poll_once(task)) else {
            continue;
        };
        *chunk = match result {
            Some((types, blades)) => {
                let mut kind = 0;
                let entities = types.map(|built| {
                    let material = materials[kind].clone();
                    kind += 1;
                    let (mesh, bounds) = built?;
                    let entity = commands.spawn((
                        Name::new("grass"),
                        GrassChunk,
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(material),
                        Transform::from_xyz(key.x as f32 * CHUNK, 0.0, key.y as f32 * CHUNK),
                        // The blades grow out of their roots in the shader.
                        bounds,
                        NoAutoAabb,
                        Visibility::Hidden,
                        NotShadowCaster,
                    ));
                    Some(entity.id())
                });
                Chunk::Ready { entities, blades }
            }
            None => Chunk::Empty,
        };
    }

    // Drop chunks out of range.
    grass.chunks.retain(|key, chunk| {
        if chunk_distance(*key, eye) > reach + CHUNK {
            if let Chunk::Ready { entities, .. } = chunk {
                for entity in entities.iter().flatten() {
                    commands.entity(*entity).despawn();
                }
            }
            return false; // A build in flight is simply dropped.
        }
        true
    });

    // Show each type's blades where its cells may draw any.
    for (key, chunk) in &grass.chunks {
        let Chunk::Ready { entities, .. } = chunk else {
            continue;
        };
        for (kind, entity) in entities.iter().enumerate() {
            let Some(mut shown) = entity.and_then(|e| visibility.get_mut(e).ok()) else {
                continue;
            };
            let near = chunk_distance(*key, eye) < lod::type_reach(k, kind, grass.reach);
            shown.set_if_neq(if grass.enabled && near {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        }
    }

    // Start builds for the nearest missing chunks.
    let in_flight = grass
        .chunks
        .values()
        .filter(|c| matches!(c, Chunk::Building(_)))
        .count();
    if in_flight >= MAX_BUILDS_IN_FLIGHT {
        return;
    }
    let center = (eye / CHUNK).floor().as_ivec2();
    let span = (reach / CHUNK).ceil() as i32 + 1;
    let mut missing: Vec<(f32, IVec2)> = (-span..=span)
        .flat_map(|dz| (-span..=span).map(move |dx| center + IVec2::new(dx, dz)))
        .filter(|key| !grass.chunks.contains_key(key))
        .map(|key| (chunk_distance(key, eye), key))
        .filter(|(distance, _)| *distance < reach)
        .collect();
    grass.settled = in_flight == 0 && missing.is_empty();
    missing.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, key) in missing.into_iter().take(MAX_BUILDS_IN_FLIGHT - in_flight) {
        let sampler = grass.sampler.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { build_chunk(&sampler, key) });
        grass.chunks.insert(key, Chunk::Building(task));
    }
}

/// Distance on X/Z from `eye` to the nearest point of a chunk.
fn chunk_distance(key: IVec2, eye: Vec2) -> f32 {
    let min = key.as_vec2() * CHUNK;
    eye.clamp(min, min + CHUNK).distance(eye)
}

/// Grid of terrain and grass samples over a chunk, 1 m apart.
// SI-GRS-03: grass heights and normals from our own 1 m grid.
struct ChunkGrid {
    heights: Vec<f32>,
    grass: Vec<(f32, [f32; 3])>,
}

const GRID: usize = CHUNK as usize + 1;

impl ChunkGrid {
    fn bilinear<T: Copy>(values: &[T], x: f32, z: f32, lerp: impl Fn(T, T, f32) -> T) -> T {
        let (x0, z0) = (
            (x.floor() as usize).min(GRID - 2),
            (z.floor() as usize).min(GRID - 2),
        );
        let (fx, fz) = (x - x0 as f32, z - z0 as f32);
        let at = |i: usize, j: usize| values[j * GRID + i];
        let top = lerp(at(x0, z0), at(x0 + 1, z0), fx);
        let bottom = lerp(at(x0, z0 + 1), at(x0 + 1, z0 + 1), fx);
        lerp(top, bottom, fz)
    }

    fn height(&self, x: f32, z: f32) -> f32 {
        Self::bilinear(&self.heights, x, z, |a, b, t| a + (b - a) * t)
    }

    fn normal(&self, x: f32, z: f32) -> Vec3 {
        let dx = self.height(x + 0.5, z) - self.height(x - 0.5, z);
        let dz = self.height(x, z + 0.5) - self.height(x, z - 0.5);
        Vec3::new(-dx, 1.0, -dz).normalize()
    }

    fn grass(&self, x: f32, z: f32) -> (f32, [f32; 3]) {
        Self::bilinear(&self.grass, x, z, |a, b, t| {
            (
                a.0 + (b.0 - a.0) * t,
                std::array::from_fn(|c| a.1[c] + (b.1[c] - a.1[c]) * t),
            )
        })
    }
}

/// Small deterministic generator for the no-dump tuft texture.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

/// The game's blade buffer of a cell, per type (`buffer`): the same in
/// every cell.
static CELL_BLADES: LazyLock<[Vec<buffer::Blade>; 2]> =
    LazyLock::new(|| lod::BLADES_PER_SIDE.map(buffer::cell));

/// A chunk's blades of both types (offsets from its corner), cell by cell.
fn build_chunk(sampler: &HeightSampler, key: IVec2) -> Option<Built> {
    let origin = key.as_vec2() * CHUNK;
    let mut grid = ChunkGrid {
        heights: Vec::with_capacity(GRID * GRID),
        grass: Vec::with_capacity(GRID * GRID),
    };
    for j in 0..GRID {
        for i in 0..GRID {
            let (x, z) = (origin.x + i as f32, origin.y + j as f32);
            grid.heights.push(sampler.height_at(x, z)?);
            grid.grass.push(
                sampler
                    .grass_at(x, z)
                    .map_or((0.0, [0.0; 3]), |g| (g.height, g.color)),
            );
        }
    }
    if grid.grass.iter().all(|(height, _)| *height < LEAST_GRASS) {
        return None;
    }

    let cells = (CHUNK / lod::CELL).round() as usize;
    let mut total = 0;
    let types = std::array::from_fn(|kind| {
        let blades = &CELL_BLADES[kind];
        let mut mesh = BladeMesh::default();
        for cell in (0..cells * cells).map(|c| UVec2::new((c % cells) as u32, (c / cells) as u32)) {
            let corner = cell.as_vec2() * lod::CELL;
            for (index, blade) in blades.iter().enumerate() {
                let place = corner + Vec2::from(blade.place.map(|b| b as f32 / 255.0)) * lod::CELL;
                let (amount, color) = grid.grass(place.x, place.y);
                if amount < LEAST_GRASS {
                    continue;
                }
                mesh.push(
                    kind,
                    blade,
                    BladeRoot {
                        // SI-GRS-03: grass heights and normals from our own 1 m grid.
                        position: Vec3::new(place.x, grid.height(place.x, place.y) - 0.04, place.y),
                        normal: grid.normal(place.x, place.y),
                        cell: corner + lod::CELL / 2.0,
                        grass: amount / 255.0,
                        // The data's colour as the game's grass shaders read it
                        // (0-1, the usual green (24, 52, 8) their reference).
                        color: Vec3::from(color) / 255.0,
                        rank: buffer::thinning_rank(index, blades.len()),
                    },
                );
            }
        }
        total += mesh.blades;
        mesh.finish()
    });
    (total > 0).then_some((types, total))
}

/// Where a blade of the buffer grows in a chunk.
struct BladeRoot {
    position: Vec3,
    /// The ground's normal there.
    normal: Vec3,
    /// The middle of its 3 m cell (X, Z).
    cell: Vec2,
    /// The map's grass there (`.grass.extm` height / 255: `summary0.w`).
    grass: f32,
    /// The map's grass colour there (`.grass.extm`, 0-1).
    color: Vec3,
    /// Its place in the order a thinned cell draws (`buffer::thinning_rank`).
    rank: usize,
}

/// A type's blades as `grass.wgsl` reads them: every vertex at the blade's
/// root, the shader builds the blade.
#[derive(Default)]
struct BladeMesh {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    tangents: Vec<[f32; 4]>,
    uvs: Vec<[f32; 2]>,
    uvs_b: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    blades: usize,
    bounds: Option<(Vec3, Vec3)>,
}

impl BladeMesh {
    fn push(&mut self, kind: usize, blade: &buffer::Blade, root: BladeRoot) {
        let first = self.positions.len() as u32;
        let [lean_x, lean_z] = blade.lean();
        // The two random bytes in one float (`Sem8`: phase + 256 × length).
        let random = blade.random[0] as f32 + 256.0 * blade.random[1] as f32;
        for &(u, row) in buffer::VERTICES[kind] {
            self.positions.push(root.position.to_array());
            self.normals.push(root.normal.to_array());
            self.tangents
                .push([lean_x, root.cell.x, lean_z, root.cell.y]);
            // The game's texture coordinates, (u/2, 1 − row/3).
            self.uvs.push([u as f32 / 2.0, 1.0 - row as f32 / 3.0]);
            self.uvs_b.push([root.rank as f32, random]);
            self.colors
                .push([root.color.x, root.color.y, root.color.z, root.grass]);
        }
        self.indices
            .extend(buffer::TRIANGLES[kind].iter().flatten().map(|i| first + i));
        self.blades += 1;
        let (min, max) = self.bounds.get_or_insert((root.position, root.position));
        (*min, *max) = (min.min(root.position), max.max(root.position));
    }

    fn finish(self) -> Option<(Mesh, Aabb)> {
        let (min, max) = self.bounds?;
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, self.tangents)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, self.uvs_b)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices));
        // A blade reaches its length from the root, whichever way it leans.
        let reach = Vec3::splat(LONGEST_BLADE);
        Some((mesh, Aabb::from_min_max(min - reach, max + reach)))
    }
}
