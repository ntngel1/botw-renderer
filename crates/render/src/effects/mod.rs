//! Particle effects: the game's emitter sets (PTCL), baked by `bake`'s
//! `effects` step, played where the game plays them.
//!
//! An [`EffectSet`] entity plays one emitter set at its transform: spawn
//! one to play an effect (the API scenes use). The plugin spawns them for
//! what plays all the time: placed objects whose ELink user always plays
//! sets (`EffectIndex::actors`, following the object while it is
//! spawned), and the map's effect-only actors (`EffectIndex::effect_actors`,
//! such as the mountain cloud caps) near the camera.

mod draw;
pub mod elink;
mod emit;
mod gpu;
mod library;
mod programs;
mod random;
mod run;
mod sim;
mod upload;

use std::sync::Arc;

use asset_format::effects::EffectActor;
use bevy::prelude::*;

pub use elink::CameraEffectSettings;
pub use library::{EffectData, EffectLibrary};
pub use run::EffectRuntime;

use crate::camera::MainView;
use crate::objects::PlacedObject;

pub struct EffectsPlugin {
    /// The `assets/` folder.
    pub assets: std::path::PathBuf,
    /// The terrain's heights (stream-out particles collide with the ground).
    pub sampler: crate::heights::HeightSampler,
}

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        draw::register(app);
        upload::register(app);
        app.add_plugins(elink::ElinkPlugin {
            assets: self.assets.clone(),
        });
        app.insert_resource(EffectLibrary::new(self.assets.clone()))
            .insert_resource(run::EffectRuntime::new(
                self.assets.clone(),
                EFFECT_SEED,
                self.sampler.clone(),
            ))
            .init_resource::<EffectActors>()
            .init_resource::<EffectRenderSettings>()
            .add_systems(
                PostUpdate,
                run::run_sets.after(bevy::transform::TransformSystems::Propagate),
            )
            .add_systems(
                Update,
                (
                    library::finish_reading,
                    play_object_effects,
                    drop_orphans,
                    stream_effect_actors,
                )
                    .chain(),
            );
    }
}

/// Controls for particle passes not yet supported by the renderer.
#[derive(Resource, Clone, Copy, Default)]
pub struct EffectRenderSettings {
    /// Preview deferred particles through the standard color combiner.
    pub deferred_preview: bool,
}

impl EffectRenderSettings {
    pub fn draws(&self, custom_shader: u32) -> bool {
        // SI-EFX-36: custom 4 writes material data to several G-buffer
        // targets, not a lit color. Keep its incorrect color preview opt-in.
        custom_shader != 4 || self.deferred_preview
    }
}

#[cfg(test)]
mod render_settings_tests {
    use super::*;

    #[test]
    fn deferred_preview_leaves_standard_and_cloud_particles_enabled() {
        let mut settings = EffectRenderSettings::default();
        for shader in [0, 1, 2, 3, 5] {
            assert!(settings.draws(shader));
        }
        assert!(!settings.draws(4));
        settings.deferred_preview = true;
        assert!(settings.draws(4));
    }
}

/// The seed of the game's random the emitters are seeded from.
// SI-EFX-25: a fixed seed, not the game's random state.
const EFFECT_SEED: u32 = 0x5EAD_0001;

/// An emitter set playing at the entity's transform.
#[derive(Component, Clone, Debug)]
pub struct EffectSet {
    /// The effect file (`EffectIndex::files`).
    pub file: Arc<str>,
    pub set: Arc<str>,
    /// The ELink asset's scale, offset and colour.
    pub scale: f32,
    pub offset: Vec3,
    pub color: Vec4,
    /// Live ELink parameters (`xlink2ResourceAccessorELink`), changed
    /// while the set plays: emission ratio (`EmissionRate`, set+0x3c),
    /// alpha (`Alpha`, the set colour's alpha), emission interval scale
    /// (`EmissionInterval`), life scale (`LifeScale`), emission scale
    /// (`EmissionScale`).
    pub emission_rate: f32,
    pub alpha: f32,
    pub emission_interval: f32,
    pub life_scale: f32,
    pub emission_scale: f32,
    /// Stop emitting and let the particles die out (the set is faded);
    /// the entity is despawned once nothing is left.
    pub fading: bool,
}

impl EffectSet {
    pub fn new(file: &str, set: &str) -> Self {
        Self {
            file: file.into(),
            set: set.into(),
            scale: 1.0,
            offset: Vec3::ZERO,
            color: Vec4::ONE,
            emission_rate: 1.0,
            alpha: 1.0,
            emission_interval: 1.0,
            life_scale: 1.0,
            emission_scale: 1.0,
            fading: false,
        }
    }
}

/// The entity an effect belongs to: it goes when its owner does.
#[derive(Component)]
pub struct EffectOwner(pub Entity);

/// Effect sets of placed objects, spawned when the object is.
fn play_object_effects(
    mut commands: Commands,
    library: Res<EffectLibrary>,
    objects: Query<(Entity, &PlacedObject, &Transform), Added<PlacedObject>>,
) {
    let Some(index) = library.index() else {
        return;
    };
    for (entity, object, transform) in &objects {
        let Some(sets) = index.actors.get(&*object.0) else {
            continue;
        };
        for played in sets {
            commands.spawn((
                Name::new(format!("effect {}", played.set)),
                EffectSet {
                    scale: played.scale,
                    offset: Vec3::from(played.offset),
                    color: Vec4::from(played.color),
                    ..EffectSet::new(&played.file, &played.set)
                },
                EffectOwner(entity),
                *transform,
                Visibility::default(),
            ));
        }
    }
}

/// Effects whose owner is gone stop.
fn drop_orphans(mut commands: Commands, effects: Query<(Entity, &EffectOwner)>, owners: Query<()>) {
    for (entity, owner) in &effects {
        if owners.get(owner.0).is_err() {
            commands.entity(entity).despawn();
        }
    }
}

/// How near the camera an effect-only actor is played (m).
// SI-EFX-07: effect actors play within a fixed radius of the camera.
const EFFECT_ACTOR_RADIUS: f32 = 4000.0;
/// Re-check after the camera moved this far (m).
const EFFECT_ACTOR_RECHECK: f32 = 100.0;

#[derive(Resource, Default)]
struct EffectActors {
    /// Where the set was last checked from.
    checked_at: Option<Vec3>,
    /// Spawned, by index into `EffectIndex::effect_actors`.
    spawned: std::collections::HashMap<usize, Vec<Entity>>,
}

fn stream_effect_actors(
    mut commands: Commands,
    library: Res<EffectLibrary>,
    mut state: ResMut<EffectActors>,
    camera: Query<&GlobalTransform, With<MainView>>,
) {
    let (Some(index), Ok(camera)) = (library.index(), camera.single()) else {
        return;
    };
    let eye = camera.translation();
    if state
        .checked_at
        .is_some_and(|at| at.distance(eye) < EFFECT_ACTOR_RECHECK)
    {
        return;
    }
    state.checked_at = Some(eye);
    let near = |actor: &EffectActor, radius: f32| {
        Vec2::new(actor.translate[0], actor.translate[2]).distance(eye.xz()) < radius
    };
    let index = index.clone();
    state.spawned.retain(|&i, entities| {
        let keep = near(
            &index.effect_actors[i],
            EFFECT_ACTOR_RADIUS + EFFECT_ACTOR_RECHECK,
        );
        if !keep {
            for &entity in entities.iter() {
                commands.entity(entity).despawn();
            }
        }
        keep
    });
    for (i, actor) in index.effect_actors.iter().enumerate() {
        if state.spawned.contains_key(&i) || !near(actor, EFFECT_ACTOR_RADIUS) {
            continue;
        }
        let Some(sets) = index.actors.get(&actor.name) else {
            continue;
        };
        let [x, y, z] = actor.rotate;
        let transform = Transform::from_translation(Vec3::from(actor.translate))
            .with_rotation(Quat::from_euler(EulerRot::ZYX, z, y, x))
            .with_scale(Vec3::from(actor.scale));
        let entities = sets
            .iter()
            .map(|played| {
                commands
                    .spawn((
                        Name::new(format!("effect {} ({})", played.set, actor.name)),
                        EffectSet {
                            scale: played.scale,
                            offset: Vec3::from(played.offset),
                            color: Vec4::from(played.color),
                            ..EffectSet::new(&played.file, &played.set)
                        },
                        transform,
                        Visibility::default(),
                    ))
                    .id()
            })
            .collect();
        state.spawned.insert(i, entities);
    }
}
