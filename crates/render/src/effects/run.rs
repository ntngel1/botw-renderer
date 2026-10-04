//! Playing [`EffectSet`]s: each gets the library's simulation of its
//! emitter set ([`EmitterSetSim`]) once its effect file is read, steps it
//! every frame (30 steps a second, as the game's frame-rate scale) and
//! draws each visible emitter with live particles as a child mesh entity
//! (`draw.rs`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use asset_format::effects::{self as fx, EffectTables};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::storage::ShaderBuffer;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use super::draw::Extra;
use super::draw::{
    DYNAMIC_BYTES, DrawParams, ParticleKey, ParticleMaterial, dynamic_bytes, particle_bytes,
    particles_mesh, sampler,
};
use super::gpu::{EmitterDynamic, ParticleAttr};
use super::random::SeadRandom;
use super::sim::{EmitterSetSim, SetParams, StreamOutEnv};
use super::{EffectData, EffectLibrary, EffectSet};
use crate::camera::MainView;
use crate::deferred_light::CubeMean;
use crate::look::LookTexture;

/// What every set shares: the library's tables and the game's random.
#[derive(Resource)]
pub struct EffectRuntime {
    tables: Option<Arc<EffectTables>>,
    task: Option<Task<Option<EffectTables>>>,
    /// The game's random for emitter seeds (`DAT_10597bf4`).
    random: SeadRandom,
    /// Per baked emitter: its static block and draw switches on the GPU.
    gpu: HashMap<usize, Arc<EmitterGpu>>,
    /// Images by (file, texture id, sampler).
    images: HashMap<(usize, u32, [u8; 3]), Handle<Image>>,
    white: Option<Handle<Image>>,
    /// Running emitters and live particles (the overlay).
    pub emitters: usize,
    pub particles: usize,
    heights: crate::heights::HeightSampler,
    /// The particle manager's smoothed wind speed (`PtclMgr+0x6f7c`) and
    /// its sheltered one (`+0x6f78`).
    wind: [f32; 2],
}

impl EffectRuntime {
    pub fn new(assets: PathBuf, seed: u32, heights: crate::heights::HeightSampler) -> Self {
        let path = assets.join(fx::TABLES);
        let task = path.exists().then(|| {
            AsyncComputeTaskPool::get().spawn(async move {
                asset_format::read_ron::<EffectTables>(&path)
                    .inspect_err(|e| warn!("effect tables: {e}"))
                    .ok()
            })
        });
        Self {
            tables: None,
            task,
            random: SeadRandom::new(seed),
            gpu: HashMap::new(),
            images: HashMap::new(),
            white: None,
            emitters: 0,
            particles: 0,
            heights,
            wind: [0.0; 2],
        }
    }

    pub fn is_loading(&self) -> bool {
        self.task.is_some()
    }
}

struct EmitterGpu {
    statics: Handle<ShaderBuffer>,
    textures: [Handle<Image>; 3],
    draw: DrawParams,
    key: ParticleKey,
    /// The particle's shape (the primitive), if not the quad.
    shape: Option<fx::Primitive>,
}

/// A set's simulation and its emitters' draw entities (children).
#[derive(Component)]
pub struct SetRun {
    sim: EmitterSetSim,
    draws: Vec<Entity>,
    faded: bool,
}

/// One emitter's draw entity.
#[derive(Component)]
pub struct EmitterDraw {
    particles: Handle<ShaderBuffer>,
    dynamic: Handle<ShaderBuffer>,
    /// The mesh holds this many particles of this emitter's shape.
    capacity: usize,
    emitter: usize,
}

fn set_params(set: &EffectSet) -> SetParams {
    SetParams {
        scale: set.scale,
        offset: set.offset,
        color: Vec4::new(
            set.color.x,
            set.color.y,
            set.color.z,
            set.color.w * set.alpha,
        ),
        emission_ratio: set.emission_rate,
        interval_scale: set.emission_interval,
        life_scale: set.life_scale,
        emission_scale: Vec3::splat(set.emission_scale),
        ..default()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn run_sets(
    mut commands: Commands,
    real: Res<Time>,
    settings: Res<super::EffectRenderSettings>,
    mut library: ResMut<EffectLibrary>,
    mut runtime: ResMut<EffectRuntime>,
    mut sets: Query<(Entity, &EffectSet, &GlobalTransform, Option<&mut SetRun>)>,
    draws: Query<&EmitterDraw>,
    mut uploads: ResMut<super::upload::EffectUploads>,
    camera: Query<&GlobalTransform, With<MainView>>,
    look: Res<LookTexture>,
    means: Res<CubeMean>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut images: ResMut<Assets<Image>>,
    grass_wind: Option<Res<crate::grass::GrassWind>>,
) {
    if let Some(task) = &mut runtime.task
        && let Some(tables) = block_on(poll_once(task))
    {
        runtime.task = None;
        runtime.tables = tables.map(Arc::new);
    }
    let Some(tables) = runtime.tables.clone() else {
        return;
    };
    let Ok(camera) = camera.single() else {
        return;
    };
    let eye = camera.translation();
    let camera_rotation = camera.rotation();
    let resident = library.resident();
    let frames = real.delta_secs() * 30.0;
    uploads.0.clear();
    let env = stream_out_env(&mut runtime, grass_wind.as_deref(), frames);
    let (mut emitter_count, mut particle_count) = (0, 0);
    for (entity, set, transform, run) in &mut sets {
        let matrix = transform.affine();
        let Some(mut run) = run else {
            // Start once the file (and the resident one) is read.
            let (Some(file), Some(_)) = (library.get(&set.file), resident.as_ref()) else {
                continue;
            };
            let Some(index) = file.file.sets.iter().position(|s| *s.name == *set.set) else {
                warn_once!("effect {}: no emitter set {}", set.file, set.set);
                commands.entity(entity).despawn();
                continue;
            };
            let set_random = runtime.random.next();
            let sim = EmitterSetSim::new(
                file,
                index,
                matrix,
                set_params(set),
                tables.clone(),
                &mut runtime.random,
                set_random,
            );
            commands.entity(entity).insert(SetRun {
                sim,
                draws: Vec::new(),
                faded: false,
            });
            continue;
        };
        let run = &mut *run;
        let params = set_params(set);
        if run.sim.params() != &params {
            run.sim.update_params(|p| *p = params);
        }
        run.sim.set_matrix(matrix);
        run.sim.set_stream_out_env(env.clone());
        if set.fading && !run.faded {
            run.sim.fade();
            run.faded = true;
        }
        run.sim.step(frames, eye);
        if !run.sim.alive() {
            commands.entity(entity).despawn();
            continue;
        }

        // Draw entities for the visible emitters, in draw order.
        let views: Vec<_> = run
            .sim
            .views()
            .into_iter()
            .filter(|v| {
                v.visible
                    && !v.particles.is_empty()
                    && settings.draws(v.emitter.params.custom_shader)
            })
            .collect();
        emitter_count += views.len();
        let Some(file) = library.get(&set.file) else {
            continue;
        };
        while run.draws.len() < views.len() {
            let child = commands
                .spawn((
                    Name::new("particles"),
                    Transform::IDENTITY,
                    Visibility::Hidden,
                    NoFrustumCulling,
                    NotShadowCaster,
                    NotShadowReceiver,
                    ChildOf(entity),
                ))
                .id();
            run.draws.push(child);
            // Filled next frame, once the entity exists.
        }
        for (i, &child) in run.draws.iter().enumerate() {
            let Some(view) = views.get(i) else {
                commands.entity(child).insert(Visibility::Hidden);
                continue;
            };
            particle_count += view.particles.len();
            let key = view.emitter as *const fx::Emitter as usize;
            let gpu = gpu_for(
                &mut runtime,
                key,
                view.emitter,
                &file,
                resident.as_deref(),
                &mut buffers,
                &mut images,
            );
            let count = view.particles.len();
            let p = &view.emitter.params;
            // The area loop draws the emitter several times (the copies
            // share one mesh: copy k holds particles k·capacity…).
            let copies = p
                .area_loop
                .as_ref()
                .map_or(1, |a| ((a.extra_draws + 1.0).trunc() as usize).max(1));
            let reuse = draws
                .get(child)
                .is_ok_and(|d| d.emitter == key && d.capacity >= count);
            let (particles, dynamic) = if reuse {
                let draw = draws.get(child).unwrap();
                (draw.particles.id(), draw.dynamic.id())
            } else {
                // A new emitter here, or more particles than the buffers
                // hold: new buffers, mesh and material.
                let capacity = count.next_power_of_two().max(16);
                let (mesh, vertices) = particles_mesh(gpu.shape.as_ref(), capacity * copies);
                let mut draw_params = gpu.draw;
                draw_params.shape.x = vertices;
                let usage = RenderAssetUsages::default();
                let particles = buffers.add(ShaderBuffer::new(
                    &vec![0; capacity * std::mem::size_of::<ParticleAttr>()],
                    usage,
                ));
                let dynamic = buffers.add(ShaderBuffer::new(&[0; DYNAMIC_BYTES], usage));
                let material = materials.add(ParticleMaterial {
                    statics: gpu.statics.clone(),
                    particles: particles.clone(),
                    dynamic: dynamic.clone(),
                    draw: draw_params,
                    tex0: gpu.textures[0].clone(),
                    tex1: gpu.textures[1].clone(),
                    tex2: gpu.textures[2].clone(),
                    look: look.0.clone(),
                    means: means.0.clone(),
                    key: gpu.key,
                });
                let ids = (particles.id(), dynamic.id());
                commands.entity(child).insert((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(material),
                    EmitterDraw {
                        particles,
                        dynamic,
                        capacity,
                        emitter: key,
                    },
                ));
                ids
            };
            uploads
                .0
                .push((particles, particle_bytes(view.particles).into()));
            let capacity = draws
                .get(child)
                .map_or(count.next_power_of_two().max(16), |d| d.capacity);
            let extra = extra_for(p, &view.dynamic, eye, camera_rotation);
            let counts = [
                count as u32,
                capacity as u32,
                copies as u32,
                p.custom_switches[1],
            ];
            uploads
                .0
                .push((dynamic, dynamic_bytes(&view.dynamic, counts, &extra).into()));
            // Emitters of a set draw in creation order (the game sorts sets,
            // not emitters): each later one sits a centimetre nearer the
            // camera for Bevy's transparent sort.
            let towards_eye =
                (eye - transform.translation()).normalize_or_zero() * (0.01 * i as f32);
            let local = matrix.inverse().transform_vector3(towards_eye);
            commands
                .entity(child)
                .insert((Visibility::Inherited, Transform::from_translation(local)));
        }
    }
    runtime.emitters = emitter_count;
    runtime.particles = particle_count;
}

fn gpu_for(
    runtime: &mut EffectRuntime,
    key: usize,
    emitter: &fx::Emitter,
    file: &EffectData,
    resident: Option<&EffectData>,
    buffers: &mut Assets<ShaderBuffer>,
    images: &mut Assets<Image>,
) -> Arc<EmitterGpu> {
    if let Some(gpu) = runtime.gpu.get(&key) {
        return gpu.clone();
    }
    let p = &emitter.params;
    let statics = buffers.add(ShaderBuffer::new(
        file.block(emitter.static_block),
        RenderAssetUsages::default(),
    ));
    let white = runtime
        .white
        .get_or_insert_with(|| images.add(Image::default()))
        .clone();
    let mut present = [false; 3];
    let textures = std::array::from_fn(|s| {
        let Some(smp) = &emitter.samplers[s] else {
            return white.clone();
        };
        let source = if smp.resident { resident } else { Some(file) };
        let Some(source) = source else {
            return white.clone();
        };
        let Some(base) = source.images.get(&smp.texture) else {
            return white.clone();
        };
        present[s] = true;
        let cache = (
            source as *const EffectData as usize,
            smp.texture,
            [smp.wrap[0], smp.wrap[1], smp.filter],
        );
        runtime
            .images
            .entry(cache)
            .or_insert_with(|| {
                let mut image = images.get(base).cloned().unwrap_or_default();
                image.sampler = sampler(smp);
                images.add(image)
            })
            .clone()
    });
    let shape = p
        .primitive
        .and_then(|id| {
            file.file
                .primitive(id)
                .or_else(|| resident?.file.primitive(id))
        })
        .cloned();
    let mut draw = DrawParams::new(p, present, 4);
    draw.shape.w = program_family(&p.pixel_program);
    draw.textures.y = super::programs::distance_scale(&p.vertex_program);
    draw.swizzles.w = light_family(&p.vertex_program);
    for (s, smp) in emitter.samplers.iter().enumerate() {
        let Some(smp) = smp else { continue };
        let source = if smp.resident { resident } else { Some(file) };
        if let Some(sw) = source.and_then(|f| f.swizzles.get(&smp.texture)) {
            draw.swizzles[s] = u32::from_le_bytes(*sw);
        }
    }
    let gpu = Arc::new(EmitterGpu {
        statics,
        textures,
        draw,
        key: ParticleKey::new(p),
        shape,
    });
    runtime.gpu.insert(key, gpu.clone());
    gpu
}

/// The area loop's box and the custom parameters
/// (docs/research/eft-custom-blocks.md §2.3, §3).
fn extra_for(p: &fx::EmitterParams, dynamic: &EmitterDynamic, eye: Vec3, camera: Quat) -> Extra {
    let mut extra = Extra::default();
    for (k, v) in p.custom_params.iter().take(32).enumerate() {
        extra.reserved[k / 4][k % 4] = *v;
    }
    let Some(a) = &p.area_loop else {
        return extra;
    };
    // `0x03b5ee80`: the box's Euler rotation.
    let [rx, ry, rz] = a.rotation;
    let rotation = Mat4::from_quat(Quat::from_euler(EulerRot::ZYX, rz, ry, rx));
    let area = if a.follows_camera {
        // Centred `centre` in camera space in front of the eye.
        Mat4::from_translation(eye + camera * Vec3::from(a.centre)) * rotation
    } else {
        let srt = &dynamic.srt;
        let emitter = Mat4::from_cols(
            Vec4::new(srt[0].x, srt[1].x, srt[2].x, 0.0),
            Vec4::new(srt[0].y, srt[1].y, srt[2].y, 0.0),
            Vec4::new(srt[0].z, srt[1].z, srt[2].z, 0.0),
            Vec4::new(srt[0].w, srt[1].w, srt[2].w, 1.0),
        );
        emitter * Mat4::from_translation(Vec3::from(a.centre)) * rotation
    };
    extra.area = area;
    extra.area_inverse = area.inverse();
    extra.step = Vec3::from(a.step).extend(0.0);
    extra.fade = Vec3::from(a.fade).extend(0.0);
    extra.half_size = Vec3::from(a.half_size).extend(a.cut_mode as f32);
    extra.cut = Vec4::new(a.cut_height, 0.0, 0.0, 0.0);
    extra
}

/// Pixel programs ported on their own (BotW's custom shaders compile a
/// different program per use; the standard combiner path covers the rest):
/// 0 the standard path, 1 the cloud program (`ps_991ea4f8496e`:
/// MountainCloud, the distant weather clouds, the volcano's).
fn program_family(pixel: &str) -> u32 {
    match pixel.get(..12) {
        Some("991ea4f8496e") => 1,
        _ => 0,
    }
}

/// How a custom shader 3 vertex program reads the analyzer's table: 0 by
/// the height on screen between texels 2 and 3 (clouds), 1 by the param
/// track between the effect texels 8 and 9 (`vs_e0a9f75d496f`: rain).
fn light_family(vertex: &str) -> u32 {
    match vertex.get(..12) {
        Some("e0a9f75d496f") => 1,
        _ => 0,
    }
}

/// `T` (`0x03786f34`): the particle manager's wind table at
/// `PtclMgr+0x1febc` (`0x03782ce4`), piecewise linear over 0…15.
const WIND_TABLE: [f32; 16] = [
    0.0, 1.15, 1.5, 1.85, 2.05, 2.25, 2.4, 2.55, 2.7, 2.85, 2.96, 3.075, 3.19, 3.3, 3.4, 3.5,
];

fn wind_table(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 15.0 {
        return WIND_TABLE[15];
    }
    let i = x.floor() as usize;
    let f = x - i as f32;
    WIND_TABLE[i] + (WIND_TABLE[(i + 1).min(15)] - WIND_TABLE[i]) * f
}

/// The values the stream-out fields read (`cus1`, docs/research/
/// eft-custom-blocks.md §4.4): the world's wind (our `WindMgr` port)
/// smoothed as `0x0378655c` does, through `T`.
fn stream_out_env(
    runtime: &mut EffectRuntime,
    wind: Option<&crate::grass::GrassWind>,
    frames: f32,
) -> StreamOutEnv {
    let (speed, direction) = wind.map_or((0.0, Vec2::Y), |w| w.world());
    if frames > 0.0 {
        let [s, s2] = &mut runtime.wind;
        *s += (speed - *s) * (1.0 - 0.01f32.powf(frames));
        // SI-EFX-34: nothing shelters the camera (c = 1).
        *s2 += (*s - *s2) * (1.0 - 0.93f32.powf(frames));
    }
    let heights = runtime.heights.clone();
    StreamOutEnv {
        wind_speed: wind_table(runtime.wind[0]),
        sheltered_wind_speed: wind_table(runtime.wind[1]),
        wind_direction: Vec3::new(direction.x, 0.0, direction.y),
        // SI-EFX-34: the drift (`PtclMgr+0x1fefc`, `+0x1ff04`) has no
        // traced writer: 0.
        drift: [0.0, 0.0],
        ground: Some(Arc::new(move |x, z| heights.height_at(x, z))),
    }
}
