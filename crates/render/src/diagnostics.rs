//! Frame diagnostics from the first day: frame time on the CPU, GPU time
//! per render pass, the terrain stream and the objects, in a corner overlay (F1 hides
//! it) and in the log every few seconds.

use std::time::Duration;

use bevy::diagnostic::{
    DiagnosticPath, DiagnosticsStore, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin,
};
use bevy::prelude::*;
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use bevy::time::common_conditions::on_timer;

use crate::terrain::TerrainStats;

pub struct DiagnosticsPlugin;

#[derive(Resource, Default)]
pub struct ProfileLog(pub bool);

impl Plugin for DiagnosticsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProfileLog>()
            .add_plugins((
                FrameTimeDiagnosticsPlugin::default(),
                RenderDiagnosticsPlugin,
                LogDiagnosticsPlugin {
                    wait_duration: Duration::from_secs(10),
                    filter: Some(
                        [
                            FrameTimeDiagnosticsPlugin::FPS,
                            FrameTimeDiagnosticsPlugin::FRAME_TIME,
                        ]
                        .into_iter()
                        .collect(),
                    ),
                    ..default()
                },
            ))
            .add_systems(Startup, spawn_overlay)
            .add_systems(
                Update,
                (
                    toggle_overlay,
                    update_overlay.run_if(on_timer(Duration::from_millis(500))),
                    log_profile
                        .after(update_overlay)
                        .run_if(on_timer(Duration::from_secs(10))),
                ),
            );
    }
}

fn log_profile(
    enabled: Res<ProfileLog>,
    overlay: Query<&Text, With<Overlay>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    updates: Res<bevy::winit::WinitSettings>,
    cameras: Query<
        Has<bevy::render::occlusion_culling::OcclusionCulling>,
        crate::camera::MainCamera,
    >,
) {
    if enabled.0
        && let Ok(text) = overlay.single()
    {
        if let Ok(window) = windows.single() {
            info!(
                "profile: {}x{}, {:?}, focused {}, background {:?}, occlusion {}",
                window.physical_width(),
                window.physical_height(),
                window.present_mode,
                window.focused,
                updates.unfocused_mode,
                cameras.single().unwrap_or(false)
            );
        }
        info!("profile:\n{}", text.0);
    }
}

#[derive(Component)]
pub(crate) struct Overlay;

fn spawn_overlay(mut commands: Commands) {
    commands.spawn((
        Overlay,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::WHITE),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(6.0),
            left: Val::Px(6.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
    ));
}

fn toggle_overlay(
    keys: Res<ButtonInput<KeyCode>>,
    mut overlay: Query<&mut Visibility, With<Overlay>>,
) {
    if keys.just_pressed(KeyCode::F1) {
        for mut visibility in &mut overlay {
            visibility.toggle_visible_hidden();
        }
    }
}

/// Text is rebuilt twice a second, not every frame.
fn update_overlay(
    store: Res<DiagnosticsStore>,
    stats: Res<TerrainStats>,
    objects: Option<Res<crate::objects::Objects>>,
    far: Option<Res<crate::objects::FarModels>>,
    library: Option<Res<crate::models::ModelLibrary>>,
    far_trees: Option<Res<crate::far_trees::FarTrees>>,
    effects: Option<Res<crate::effects::EffectRuntime>>,
    sets: Query<(), With<crate::effects::EffectSet>>,
    camera: Query<&GlobalTransform, crate::camera::MainCamera>,
    mut overlay: Query<&mut Text, With<Overlay>>,
) {
    let Ok(mut text) = overlay.single_mut() else {
        return;
    };
    let smoothed = |path: &DiagnosticPath| store.get(path).and_then(|d| d.smoothed());
    let mut lines = vec![format!(
        "{:.0} fps  {:.2} ms",
        smoothed(&FrameTimeDiagnosticsPlugin::FPS).unwrap_or(0.0),
        smoothed(&FrameTimeDiagnosticsPlugin::FRAME_TIME).unwrap_or(0.0),
    )];
    // Time per render pass (RenderDiagnosticsPlugin): on the GPU where the
    // device has timestamps inside passes (Apple GPUs do not), else the
    // CPU's encoding time. The slowest few.
    let passes = |suffix: &str| -> Vec<(String, f64)> {
        let mut passes: Vec<(String, f64)> = store
            .iter()
            .filter_map(|d| {
                let path = d.path().as_str();
                let pass = path.strip_prefix("render/")?.strip_suffix(suffix)?;
                Some((pass.to_owned(), d.smoothed()?))
            })
            .collect();
        passes.sort_by(|a, b| b.1.total_cmp(&a.1));
        passes
    };
    let (unit, passes) = match passes("/elapsed_gpu") {
        gpu if gpu.iter().any(|(_, ms)| *ms > 0.0) => ("gpu", gpu),
        _ => ("cpu", passes("/elapsed_cpu")),
    };
    let total: f64 = passes
        .iter()
        .filter(|(pass, _)| !pass.contains('/'))
        .map(|(_, ms)| ms)
        .sum();
    if !passes.is_empty() {
        lines.push(format!("passes ({unit}) {total:.2} ms"));
        for (pass, ms) in passes.iter().take(6) {
            lines.push(format!("  {ms:5.2} ms  {pass}"));
        }
    }
    lines.push(format!(
        "terrain: {} resident, {} loading, per lod {:?}",
        stats.resident, stats.loading, stats.visible_per_lod
    ));
    if let (Some(objects), Some(far), Some(library)) = (objects, far, library) {
        lines.push(format!(
            "objects: {} spawned, {} far, {} model folders",
            objects.spawned(),
            far.spawned(),
            library.loaded_folders()
        ));
    }
    if let Some(trees) = far_trees {
        lines.push(format!("far trees: {}", trees.count()));
    }
    if let Some(effects) = effects {
        lines.push(format!(
            "effects: {} sets, {} emitters, {} particles",
            sets.iter().count(),
            effects.emitters,
            effects.particles
        ));
    }
    if let Ok(camera) = camera.single() {
        let p = camera.translation();
        lines.push(format!("camera {:.1} {:.1} {:.1}", p.x, p.y, p.z));
    }
    text.0 = lines.join("\n");
}
