//! Grass tufts: the game's tuft cards (`GrassCrossAlb` in `Terrain.Tex1`),
//! laid out by its tuft buffer (`buffer::tuft_tile`) in every 10 m tile,
//! each of the game's two tuft types growing in and shrinking away over
//! that type's distances (`lod::TUFT_DISTANCES`, `grass_cards.wgsl`) as the
//! blades thin out, and carrying the grass on beyond them. Stood along the
//! ground's normal, cut away on steep ground, coloured by the game's tuft
//! G-buffer (`uking_grass_cross`), bent by the game's grass wind (`wind`),
//! and lit like the blades.

use std::collections::HashMap;
use std::sync::LazyLock;

use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, Face, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use std::path::{Path, PathBuf};

use asset_format::paths;
use asset_format::texture::Texture;

use super::{GrassSettings, GrassWind, InteractMaps, Rng, buffer, lod, wind};
use crate::heights::HeightSampler;

const SHADER: Handle<Shader> = uuid_handle!("5d1a8c73-0b2e-4f96-8c47-e3a9f1b6d502");

/// Chunk edge in metres: 4 × 4 of the game's tuft tiles.
const CHUNK: f32 = 4.0 * buffer::TUFT_TILE;
/// Grid of height samples a chunk is built from, in metres.
const STEP: f32 = 2.0;
const GRID: usize = (CHUNK / STEP) as usize + 1;
const MAX_BUILDS_IN_FLIGHT: usize = 4;

/// The game's tuft tile (`buffer::tuft_tile`), laid in every tile alike.
static TILE_TUFTS: LazyLock<[Vec<Vec<buffer::TuftVertex>>; 2]> = LazyLock::new(buffer::tuft_tile);

pub type GrassCardMaterial = ExtendedMaterial<StandardMaterial, GrassCardExtension>;

pub struct GrassCardsPlugin {
    /// The `assets/` folder.
    pub assets: PathBuf,
    pub sampler: HeightSampler,
}

impl Plugin for GrassCardsPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "../grass_cards.wgsl", Shader::from_wgsl);
        let assets = self.assets.clone();
        let texture = AsyncComputeTaskPool::get().spawn(async move {
            let swell = super::load_material(&assets)
                .tuft_swell
                .as_ref()
                .and_then(wind::swell_of);
            (load_tuft(&assets), swell)
        });
        app.add_plugins(MaterialPlugin::<GrassCardMaterial>::default())
            .insert_resource(GrassCards {
                sampler: self.sampler.clone(),
                material: Material::Loading(texture),
                chunks: HashMap::new(),
                enabled: true,
                settled: false,
            })
            .add_systems(
                Update,
                (finish_material, update_cards, follow_clouds, blow)
                    .chain()
                    .after(super::blow),
            );
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct GrassCardExtension {
    #[uniform(100)]
    pub settings: GrassSettings,
    /// The clouds, for their shadows (see `clouds.rs`).
    #[texture(101)]
    #[sampler(102)]
    pub cloud_noise: Handle<Image>,
    #[uniform(103)]
    pub clouds: crate::clouds::CloudParams,
    /// The shared look values (`look::LookTexture`).
    #[texture(104)]
    pub look: Handle<Image>,
    /// The game's grass tuft (`GrassCrossAlb`), alpha cut out.
    #[texture(105)]
    #[sampler(106)]
    pub tuft: Handle<Image>,
    /// The game's `grass_wind_swell` (`wind`).
    #[texture(107, visibility(vertex))]
    #[sampler(108, visibility(vertex))]
    pub swell: Handle<Image>,
    /// The game's `grass_mow_wide` and `grass_lie` (`interact`).
    #[texture(109, visibility(vertex))]
    #[sampler(110, visibility(vertex))]
    pub mow_wide: Handle<Image>,
    #[texture(111, visibility(vertex))]
    #[sampler(112, visibility(vertex))]
    pub lie: Handle<Image>,
    /// The environment's mean brightness (`deferred_light::CubeMean`).
    #[texture(113, sample_type = "float", filterable = false)]
    pub cube_mean: Handle<Image>,
}

impl MaterialExtension for GrassCardExtension {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    /// Not in a depth prepass: Bevy's prepass shaders would draw the cards
    /// as whole quads (no tuft cut-out, no growing in), and that depth would
    /// hide the ground behind their empty parts.
    fn enable_prepass() -> bool {
        false
    }

    /// The cards cast no shadows (`NotShadowCaster`); no shadow pipelines.
    fn enable_shadows() -> bool {
        false
    }
}

/// A far-grass chunk entity.
#[derive(Component)]
pub struct GrassCardChunk;

#[derive(Resource)]
pub struct GrassCards {
    sampler: HeightSampler,
    material: Material,
    chunks: HashMap<IVec2, Chunk>,
    enabled: bool,
    /// Every chunk in the ring was built by the last update.
    settled: bool,
}

enum Material {
    /// The tuft texture and `Cross1`'s wind swell.
    Loading(Task<(Option<Texture>, Option<Vec4>)>),
    Ready(Handle<GrassCardMaterial>),
}

enum Chunk {
    Building(Task<Option<(Mesh, usize)>>),
    Ready {
        entity: Entity,
        cards: usize,
    },
    /// No grass here.
    Empty,
}

impl GrassCards {
    /// The tuft texture is in and every chunk in the ring is built.
    pub fn is_settled(&self) -> bool {
        self.settled
    }

    /// Chunks drawn and cards in them.
    pub fn stats(&self) -> (usize, usize) {
        self.chunks
            .values()
            .filter_map(|chunk| match chunk {
                Chunk::Ready { cards, .. } => Some(*cards),
                _ => None,
            })
            .fold((0, 0), |(chunks, total), cards| (chunks + 1, total + cards))
    }
}

/// The tuft as baked: sRGB RGBA, its mips rebuilt keeping the alpha
/// coverage (`bake`, SI-GRS-02).
fn load_tuft(assets: &Path) -> Option<Texture> {
    Texture::read(&assets.join(paths::GRASS_TUFT))
        .inspect_err(|error| warn!("grass tuft texture unavailable: {error}"))
        .ok()
}

/// The tuft's texture: clamped, filtered linearly between its mips.
fn tuft_image(texture: &Texture) -> Option<Image> {
    let mut image = crate::texture::gpu_image(texture, true)?;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });
    Some(image)
}

/// The generated tuft (no baked one) as a texture without mips.
fn generated_tuft_image() -> Image {
    let (data, width, height) = generated_tuft();
    let mut image = Image::new(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

/// A tuft of spiky blades for when there is no dump: darker at the root,
/// top row the tips, like `GrassCrossAlb`.
fn generated_tuft() -> (Vec<u8>, u32, u32) {
    const W: u32 = 128;
    const H: u32 = 64;
    let mut rng = Rng(0x5eed_1234_abcd_0001);
    // Blades: (root x, lean, height fraction, half width at the root).
    let blades: Vec<(f32, f32, f32, f32)> = (0..28)
        .map(|_| {
            (
                rng.range(0.05, 0.95),
                rng.range(-0.15, 0.15),
                rng.range(0.45, 1.0),
                rng.range(0.012, 0.025),
            )
        })
        .collect();
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H {
        // 0 at the root (bottom row), 1 at the top.
        let t = 1.0 - y as f32 / (H - 1) as f32;
        let c = Vec3::new(0.2, 0.33, 0.08).lerp(Vec3::new(0.42, 0.56, 0.16), t);
        for x in 0..W {
            let u = x as f32 / (W - 1) as f32;
            let inside = blades.iter().any(|&(root, lean, top, half)| {
                let along = t / top;
                along <= 1.0 && (u - (root + lean * along)).abs() <= half * (1.0 - along)
            });
            data.extend([
                (c.x * 255.0) as u8,
                (c.y * 255.0) as u8,
                (c.z * 255.0) as u8,
                if inside { 255 } else { 0 },
            ]);
        }
    }
    (data, W, H)
}

fn finish_material(
    mut cards: ResMut<GrassCards>,
    grass: Res<super::Grass>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<GrassCardMaterial>>,
    clouds: Option<Res<crate::clouds::CloudShadows>>,
    look: Option<Res<crate::look::LookTexture>>,
    wind: Res<GrassWind>,
    maps: Res<InteractMaps>,
    cube_mean: Option<Res<crate::deferred_light::CubeMean>>,
) {
    let Material::Loading(task) = &mut cards.material else {
        return;
    };
    let Some((texture, swell)) = block_on(poll_once(task)) else {
        return;
    };
    let reach = grass.reach;
    let tuft = texture
        .as_ref()
        .and_then(tuft_image)
        .unwrap_or_else(generated_tuft_image);
    let material = materials.add(GrassCardMaterial {
        base: StandardMaterial {
            cull_mode: None::<Face>,
            double_sided: false,
            ..default()
        },
        extension: GrassCardExtension {
            settings: GrassSettings {
                wind: wind.environment(),
                swell: swell.unwrap_or(wind::TUFT_SWELL),
                maps: maps.transforms(),
                lod: lod::tuft_distances(reach),
                flags: Vec4::ZERO,
            },
            cloud_noise: clouds.as_ref().map(|c| c.noise.clone()).unwrap_or_default(),
            clouds: clouds
                .as_ref()
                .map_or_else(crate::clouds::CloudParams::without_shadows, |c| {
                    c.params.clone()
                }),
            look: look.map(|l| l.0.clone()).unwrap_or_default(),
            tuft: images.add(tuft),
            swell: wind.swell.clone(),
            mow_wide: maps.wide_image.clone(),
            lie: maps.lie_image.clone(),
            cube_mean: cube_mean.as_ref().map(|m| m.0.clone()).unwrap_or_default(),
        },
    });
    cards.material = Material::Ready(material);
}

/// Hands the grass wind, the maps' transforms and the draw distance
/// (`Grass::reach`) to the cards.
fn blow(
    cards: Res<GrassCards>,
    grass: Res<super::Grass>,
    wind: Res<GrassWind>,
    maps: Res<InteractMaps>,
    mut materials: ResMut<Assets<GrassCardMaterial>>,
) {
    if let Material::Ready(material) = &cards.material
        && let Some(mut material) = materials.get_mut(material)
    {
        material.extension.settings.wind = wind.environment();
        material.extension.settings.maps = maps.transforms();
        material.extension.settings.lod = lod::tuft_distances(grass.reach);
    }
}

/// Keeps the cards' cloud shadows on the moving sun.
fn follow_clouds(
    cards: Res<GrassCards>,
    clouds: Option<Res<crate::clouds::CloudShadows>>,
    mut materials: ResMut<Assets<GrassCardMaterial>>,
) {
    let (Material::Ready(material), Some(clouds)) = (&cards.material, clouds) else {
        return;
    };
    if clouds.is_changed()
        && let Some(mut material) = materials.get_mut(material)
    {
        material.extension.clouds = clouds.params.clone();
    }
}

fn update_cards(
    mut commands: Commands,
    mut cards: ResMut<GrassCards>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut grass: ResMut<super::Grass>,
    cameras: Query<(&GlobalTransform, &Projection), crate::camera::MainCamera>,
    mut visibility: Query<&mut Visibility, With<GrassCardChunk>>,
) {
    let cards = &mut *cards;
    // Shown and hidden with the blades (G).
    if cards.enabled != grass.enabled {
        cards.enabled = grass.enabled;
        for mut v in &mut visibility {
            *v = if cards.enabled {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
    }
    grass.far_cards = cards.stats().1;
    cards.settled = false;
    let Material::Ready(material) = &cards.material else {
        return;
    };
    let Ok((camera, projection)) = cameras.single() else {
        return;
    };
    let eye = camera.translation().xz();
    let (nearest, farthest) = lod::tuft_span(lod::lod_k(projection), grass.reach);

    for chunk in cards.chunks.values_mut() {
        let Chunk::Building(task) = chunk else {
            continue;
        };
        let Some(result) = block_on(poll_once(task)) else {
            continue;
        };
        *chunk = match result {
            Some((mesh, count)) => {
                let entity = commands
                    .spawn((
                        Name::new("far grass"),
                        GrassCardChunk,
                        Mesh3d(meshes.add(mesh)),
                        MeshMaterial3d(material.clone()),
                        Transform::IDENTITY,
                        if cards.enabled {
                            Visibility::Inherited
                        } else {
                            Visibility::Hidden
                        },
                        NotShadowCaster,
                    ))
                    .id();
                Chunk::Ready {
                    entity,
                    cards: count,
                }
            }
            None => Chunk::Empty,
        };
    }

    // Only the ring where cards show.
    let wanted = |key: IVec2| {
        let min = key.as_vec2() * CHUNK;
        let near = eye.clamp(min, min + CHUNK).distance(eye);
        let far = [
            min,
            min + Vec2::X * CHUNK,
            min + Vec2::Y * CHUNK,
            min + CHUNK,
        ]
        .iter()
        .map(|c| c.distance(eye))
        .fold(0.0, f32::max);
        near < farthest && far > nearest
    };
    cards.chunks.retain(|key, chunk| {
        if wanted(*key) {
            return true;
        }
        if let Chunk::Ready { entity, .. } = chunk {
            commands.entity(*entity).despawn();
        }
        false
    });

    let in_flight = cards
        .chunks
        .values()
        .filter(|c| matches!(c, Chunk::Building(_)))
        .count();
    if in_flight >= MAX_BUILDS_IN_FLIGHT {
        return;
    }
    let center = (eye / CHUNK).floor().as_ivec2();
    let span = (farthest / CHUNK).ceil() as i32 + 1;
    let mut missing: Vec<(f32, IVec2)> = (-span..=span)
        .flat_map(|dz| (-span..=span).map(move |dx| center + IVec2::new(dx, dz)))
        .filter(|key| !cards.chunks.contains_key(key) && wanted(*key))
        .map(|key| ((key.as_vec2() + 0.5) * CHUNK - eye, key))
        .map(|(offset, key)| (offset.length(), key))
        .collect();
    cards.settled = in_flight == 0 && missing.is_empty();
    missing.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, key) in missing.into_iter().take(MAX_BUILDS_IN_FLIGHT - in_flight) {
        let sampler = cards.sampler.clone();
        let task = AsyncComputeTaskPool::get().spawn(async move { build_chunk(&sampler, key) });
        cards.chunks.insert(key, Chunk::Building(task));
    }
}

/// The tufts of a chunk (world-space mesh) and how many there are: every
/// 10 m tile holds the game's tile of tufts (`buffer::tuft_tile`), each
/// vertex on the ground at its own place, with the ground's normal and the
/// grass there, as the game's vertex shader samples `grass_summary0/1` per
/// vertex. A tuft whose vertices all have no grass stays out: the shader
/// would lay it flat on one line.
fn build_chunk(sampler: &HeightSampler, key: IVec2) -> Option<(Mesh, usize)> {
    let origin = key.as_vec2() * CHUNK;
    let mut heights = Vec::with_capacity(GRID * GRID);
    for j in 0..GRID {
        for i in 0..GRID {
            let (x, z) = (origin.x + i as f32 * STEP, origin.y + j as f32 * STEP);
            heights.push(sampler.height_at(x, z)?);
        }
    }
    let cell = |x: f32, z: f32| {
        let (gx, gz) = (
            (x / STEP).clamp(0.0, (GRID - 1) as f32 - 1e-3),
            (z / STEP).clamp(0.0, (GRID - 1) as f32 - 1e-3),
        );
        let (i, j) = (gx.floor() as usize, gz.floor() as usize);
        (i, j, gx - i as f32, gz - j as f32)
    };
    let height = |x: f32, z: f32| {
        let (i, j, fx, fz) = cell(x, z);
        let at = |a: usize, b: usize| heights[b * GRID + a];
        let top = at(i, j) + (at(i + 1, j) - at(i, j)) * fx;
        let bottom = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * fx;
        top + (bottom - top) * fz
    };

    let (mut positions, mut normals, mut uvs, mut uvs_b, mut colors, mut indices) = (
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    let mut count = 0;
    let tiles = (CHUNK / buffer::TUFT_TILE) as i32;
    for (tz, tx) in (0..tiles).flat_map(|tz| (0..tiles).map(move |tx| (tz, tx))) {
        let corner = Vec2::new(tx as f32, tz as f32) * buffer::TUFT_TILE;
        for (kind, tufts) in TILE_TUFTS.iter().enumerate() {
            // The type's full height (`tera+0x10fc` × `T.1`); the shader
            // scales it with the grass, sinking the tuft.
            let tall = lod::GRASS_SCALE * lod::TUFT_HEIGHTS[kind];
            for tuft in tufts {
                let vertices: Vec<_> = tuft
                    .iter()
                    .map(|vertex| {
                        let [px, pz] = vertex.place().map(|p| p * buffer::TUFT_TILE);
                        let (x, z) = (corner.x + px, corner.y + pz);
                        let (wx, wz) = (origin.x + x, origin.y + z);
                        let grass = sampler
                            .grass_at(wx, wz)
                            .map_or((0.0, [0.0; 3]), |g| (g.height, g.color));
                        (x, z, vertex.uv(), grass)
                    })
                    .collect();
                if vertices.iter().all(|(.., (amount, _))| *amount <= 0.0) {
                    continue;
                }
                count += 1;
                let first = positions.len() as u32;
                for (x, z, [u, v], (amount, color)) in vertices {
                    let dx = height(x + 0.5, z) - height(x - 0.5, z);
                    let dz = height(x, z + 0.5) - height(x, z - 0.5);
                    positions.push([origin.x + x, height(x, z), origin.y + z]);
                    normals.push(Vec3::new(-dx, 1.0, -dz).normalize().to_array());
                    uvs.push([u, v]);
                    // The type's full height; the grass amount
                    // (`summary0.w`), which scales the tuft and its gust.
                    uvs_b.push([tall, amount / 255.0]);
                    let color = Vec3::from(color) / 255.0;
                    colors.push([color.x, color.y, color.z, kind as f32]);
                }
                indices.extend(
                    buffer::TUFT_TRIANGLES[kind]
                        .iter()
                        .flatten()
                        .map(|i| first + i),
                );
            }
        }
    }
    if count == 0 {
        return None;
    }
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uvs_b)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices));
    Some((mesh, count))
}
