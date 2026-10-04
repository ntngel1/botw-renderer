//! Whether the world around the camera has finished loading: terrain and
//! its collision, the models and the objects placed with them, far models
//! and far trees, grass, camps and characters, the game's environment data,
//! the sky's medium, cloud shadows and cloud caps, the map of what covers
//! the sky above the field, and the renderer's pipelines. [`WorldReady`] holds the answer once it has stayed true for a
//! short run of frames (things spawned from a finished load take a frame or
//! two to reach the screen); screenshots and scripted input wait for it.
//!
//! Enemies start to act once the scene has loaded ([`scene_loaded`]).
//!
//! [`SceneEpoch`] changes when a scene is set up anew in a running app (the
//! next camera of a batch of screenshots): systems that smooth or throttle
//! over time start afresh then, as in a new session.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::render_resource::{CachedPipelineState, PipelineCache};
use bevy::render::{MainWorld, RenderApp};
use bevy::shader::ShaderCacheError;

use crate::cast::Cast;
use crate::character::Characters;
use crate::climate::ClimateMap;
use crate::clouds::{CloudShadows, CloudsNow};
use crate::daynight::{LoadingEnvironment, SkyMedium};
use crate::effects::{EffectLibrary, EffectRuntime};
use crate::far_trees::FarTrees;
use crate::grass::{Grass, GrassCards, GrassColorMap};
use crate::models::ModelLibrary;
use crate::objects::{FarModels, Objects};
use crate::sky_occlusion::SkyCover;
use crate::terrain::TerrainStats;
use crate::terrain_material::TerrainLook;
use crate::water_material::WaterLook;

/// Frames in a row with nothing loading before the world counts as ready.
pub const SETTLE_FRAMES: u32 = 20;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReadySystems;

pub struct ReadyPlugin;

impl Plugin for ReadyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldReady>()
            .init_resource::<SceneEpoch>()
            .add_systems(Last, check_ready.in_set(ReadySystems));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render.add_systems(ExtractSchedule, count_waiting_pipelines);
        }
    }
}

/// Changed (and counted up) when a new scene is set up in the running app.
#[derive(Resource, Default, Debug)]
pub struct SceneEpoch(pub u32);

/// Whether everything the frame needs has loaded.
#[derive(Resource, Default, Debug)]
pub struct WorldReady {
    /// Frames in a row with nothing loading.
    settled: u32,
    /// What was still loading in the last frame checked.
    pending: Vec<&'static str>,
    /// What held the world back most recently.
    last_pending: Vec<&'static str>,
    /// Pipelines the renderer has yet to compile (from the render world).
    waiting_pipelines: usize,
    pipeline_errors: Vec<String>,
    /// The scene has been ready once (it may stream more in since).
    loaded: bool,
}

impl WorldReady {
    /// A permanent shader error must fail capture, rather than hide a material.
    pub fn render_error(&self) -> Option<&str> {
        self.pipeline_errors.first().map(String::as_str)
    }

    /// Nothing has been loading for [`SETTLE_FRAMES`] frames.
    pub fn is_ready(&self) -> bool {
        self.settled >= SETTLE_FRAMES
    }

    /// What is still loading (empty once everything has loaded).
    pub fn pending(&self) -> &[&'static str] {
        &self.pending
    }

    /// What was loading the last time something was, and how many frames
    /// in a row nothing has been.
    pub fn last_pending(&self) -> (&[&'static str], u32) {
        (&self.last_pending, self.settled)
    }

    /// A new scene: count settled frames again, from not loaded.
    pub fn restart(&mut self) {
        self.settled = 0;
        self.loaded = false;
    }
}

/// Run condition: the scene has loaded once (true without [`ReadyPlugin`]).
/// What would otherwise play out while the world loads waits for it: the
/// Bokoblins' minds, so that they stand where the map puts them until then.
pub fn scene_loaded(ready: Option<Res<WorldReady>>) -> bool {
    ready.is_none_or(|ready| ready.loaded)
}

/// Everything that loads in the background, each optional (tests run with
/// only some of the plugins). Stage 1: what is ported so far; collision,
/// the player, characters, camps and cloud caps join with their modules.
#[derive(SystemParam)]
struct Loading<'w> {
    terrain: Option<Res<'w, TerrainStats>>,
    terrain_look: Option<Res<'w, TerrainLook>>,
    water: Option<Res<'w, WaterLook>>,
    models: Option<Res<'w, ModelLibrary>>,
    objects: Option<Res<'w, Objects>>,
    far_models: Option<Res<'w, FarModels>>,
    far_trees: Option<Res<'w, FarTrees>>,
    grass: Option<Res<'w, Grass>>,
    grass_cards: Option<Res<'w, GrassCards>>,
    grass_color: Option<Res<'w, GrassColorMap>>,
    environment: Option<Res<'w, LoadingEnvironment>>,
    climate: Option<Res<'w, ClimateMap>>,
    medium: Option<Res<'w, SkyMedium>>,
    cloud_shadows: Option<Res<'w, CloudShadows>>,
    clouds_now: Option<Res<'w, CloudsNow>>,
    sky_cover: Option<Res<'w, SkyCover>>,
    sky_lut: Option<Res<'w, crate::sky_lut::SkyLutState>>,
    characters: Option<Res<'w, Characters>>,
    cast: Option<Res<'w, Cast>>,
    effects: Option<Res<'w, EffectLibrary>>,
    effect_runtime: Option<Res<'w, EffectRuntime>>,
    elink: Option<Res<'w, crate::effects::elink::Elink>>,
}

impl Loading<'_> {
    /// The names of what is still loading.
    fn pending(&self) -> Vec<&'static str> {
        let busy = |loading: bool, name: &'static str| loading.then_some(name);
        let clouds_behind = match (&self.cloud_shadows, &self.clouds_now) {
            (Some(shadows), Some(now)) => !shadows.follows(now),
            _ => false,
        };
        [
            busy(
                self.terrain_look.as_ref().is_some_and(|l| l.is_loading()),
                "terrain textures",
            ),
            busy(
                self.terrain.as_ref().is_some_and(|s| !s.settled()),
                "terrain tiles",
            ),
            busy(
                self.water.as_ref().is_some_and(|w| !w.is_settled()),
                "water",
            ),
            busy(
                self.models.as_ref().is_some_and(|m| m.is_loading()),
                "models",
            ),
            busy(
                self.objects.as_ref().is_some_and(|o| !o.is_settled()),
                "objects",
            ),
            busy(
                self.far_models.as_ref().is_some_and(|f| !f.is_settled()),
                "far models",
            ),
            busy(
                self.far_trees.as_ref().is_some_and(|t| t.is_loading()),
                "far trees",
            ),
            busy(
                self.grass.as_ref().is_some_and(|g| !g.is_settled()),
                "grass",
            ),
            busy(
                self.grass_cards.as_ref().is_some_and(|g| !g.is_settled()),
                "far grass",
            ),
            busy(
                self.grass_color.as_ref().is_some_and(|g| !g.is_settled()),
                "grass colour",
            ),
            busy(
                self.environment.as_ref().is_some_and(|e| e.is_loading()),
                "environment",
            ),
            busy(
                self.climate.as_ref().is_some_and(|c| !c.is_settled()),
                "climate",
            ),
            busy(
                self.medium.as_ref().is_some_and(|m| m.is_pending()),
                "sky medium",
            ),
            busy(clouds_behind, "cloud shadows"),
            busy(
                self.sky_lut.as_ref().is_some_and(|s| s.is_loading()),
                "sky table",
            ),
            busy(
                self.sky_cover.as_ref().is_some_and(|c| !c.is_settled()),
                "sky cover",
            ),
            busy(
                self.characters.as_ref().is_some_and(|c| c.is_loading())
                    || self.cast.as_ref().is_some_and(|c| c.is_pending()),
                "characters",
            ),
            busy(
                self.effects.as_ref().is_some_and(|e| e.is_loading())
                    || self.effect_runtime.as_ref().is_some_and(|e| e.is_loading())
                    || self.elink.as_ref().is_some_and(|e| e.is_loading()),
                "effects",
            ),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

fn check_ready(loading: Loading, mut ready: ResMut<WorldReady>) {
    let mut pending = loading.pending();
    if ready.waiting_pipelines > 0 {
        pending.push("pipelines");
    }
    if ready.render_error().is_some() {
        pending.push("failed pipelines");
    }
    let ready = ready.bypass_change_detection();
    ready.settled = if pending.is_empty() {
        ready.settled.saturating_add(1)
    } else {
        0
    };
    ready.loaded |= ready.settled >= SETTLE_FRAMES;
    if !pending.is_empty() {
        ready.last_pending.clone_from(&pending);
    }
    ready.pending = pending;
}

/// Hands the number of pipelines still compiling to the main world.
fn count_waiting_pipelines(mut main: ResMut<MainWorld>, cache: Res<PipelineCache>) {
    if let Some(mut ready) = main.get_resource_mut::<WorldReady>() {
        let ready = ready.bypass_change_detection();
        ready.waiting_pipelines = cache.waiting_pipelines().count();
        ready.pipeline_errors = cache
            .pipelines()
            .filter_map(|p| match &p.state {
                CachedPipelineState::Err(error) => permanent_shader_error(error),
                _ => None,
            })
            .collect();
    }
}

fn permanent_shader_error(error: &ShaderCacheError) -> Option<String> {
    match error {
        ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable => {
            None
        }
        _ => Some(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_shader_prevents_a_complete_scene() {
        let mut app = App::new();
        app.add_plugins(ReadyPlugin);
        app.world_mut()
            .resource_mut::<WorldReady>()
            .pipeline_errors
            .push("invalid shader".into());
        for _ in 0..SETTLE_FRAMES + 1 {
            app.update();
        }
        let ready = app.world().resource::<WorldReady>();
        assert!(!ready.is_ready());
        assert_eq!(ready.render_error(), Some("invalid shader"));
        assert_eq!(ready.pending(), &["failed pipelines"]);
    }

    #[test]
    fn missing_import_can_retry_but_invalid_module_fails() {
        assert!(permanent_shader_error(&ShaderCacheError::ShaderImportNotYetAvailable).is_none());
        assert!(
            permanent_shader_error(&ShaderCacheError::CreateShaderModule("bad module".into()))
                .unwrap()
                .contains("bad module")
        );
    }
}
