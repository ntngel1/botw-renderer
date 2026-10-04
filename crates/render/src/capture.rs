// SI-RUN-07: screenshot capture uses the renderer's readiness checks.
//! `--screenshot`: once everything around the camera has loaded
//! (`ready::WorldReady`: terrain, the environment, the sky's medium, cloud
//! shadows, the map of what covers the sky, pipelines) and another
//! [`MEASURED_FRAMES`] frames have been drawn (their mean frame time is
//! logged), save one frame of the main view and exit. The view draws into
//! an image of the window's size rather than the window: macOS stops
//! presenting to a covered window, and its shot came out black. A
//! screenshot waits for streaming and pipeline readiness.

use std::path::PathBuf;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::ui::IsDefaultUiCamera;
use bevy::window::PrimaryWindow;

use crate::camera::MainView;
use crate::ready::WorldReady;
use crate::terrain::TerrainStats;
use crate::water_material::WaterFollow;

pub struct CapturePlugin {
    pub path: PathBuf,
}

impl Plugin for CapturePlugin {
    fn build(&self, app: &mut App) {
        // The game's draws differ every session; shots repeat.
        app.insert_resource(crate::clouds::GlobalRandom::fixed());
        // The shot waits for the water to have the sky's clouds: with the
        // world wind they move every frame, so while waiting the water
        // takes them every frame; the
        // measured frames keep its usual steps.
        app.insert_resource(WaterFollow { interval: 0.0 });
        app.insert_resource(Capture {
            path: self.path.clone(),
            settled_frames: 0,
            state: State::Waiting,
            target: Handle::default(),
        })
        .add_systems(PostStartup, render_offscreen)
        .add_systems(Last, capture);
    }
}

/// Frames everything must stay loaded before the shot, so streaming
/// finishes splitting down and the GPU has every texture.
const SETTLED_FRAMES: u32 = 30;
/// Never before this many seconds: pipelines are only queued once the
/// things that need them are first drawn.
const MIN_SECS: f32 = 3.0;
/// Frames drawn after loading before the shot; their mean frame time is
/// logged (the overlay's FPS is a smoothed value of the same frames).
const MEASURED_FRAMES: u32 = 300;
/// Give up waiting after this many seconds.
const TIMEOUT_SECS: f32 = 120.0;

#[derive(Resource)]
struct Capture {
    path: PathBuf,
    settled_frames: u32,
    state: State,
    /// What the main view draws into.
    target: Handle<Image>,
}

/// Points the main view (and the overlay) at an image of the window's size.
fn render_offscreen(
    mut commands: Commands,
    mut capture: ResMut<Capture>,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    views: Query<Entity, With<MainView>>,
) {
    let size = windows
        .single()
        .map_or(UVec2::new(1600, 900), |w| w.physical_size());
    capture.target = images.add(Image::new_target_texture(
        size.x,
        size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    for view in &views {
        commands.entity(view).insert((
            RenderTarget::Image(capture.target.clone().into()),
            IsDefaultUiCamera,
        ));
    }
}

#[derive(PartialEq)]
enum State {
    Waiting,
    /// Loaded; frames drawn since and the real time they started at.
    Measuring(u32, f64),
    /// Measured; the water catching up with the clouds again.
    Settling,
    Requested,
    Saved(u32),
}

fn capture(
    mut commands: Commands,
    mut capture: ResMut<Capture>,
    stats: Res<TerrainStats>,
    world: Res<WorldReady>,
    time: Res<Time>,
    real: Res<Time<Real>>,
    mut follow: ResMut<WaterFollow>,
    mut exit: MessageWriter<AppExit>,
) {
    if let Some(error) = world.render_error() {
        error!("capture: shader compilation failed: {error}");
        exit.write(AppExit::error());
        return;
    }
    match capture.state {
        State::Waiting => {
            let settled = world.pending().is_empty();
            capture.settled_frames = if settled {
                capture.settled_frames + 1
            } else {
                0
            };
            let timed_out = time.elapsed_secs() > TIMEOUT_SECS;
            let ready = capture.settled_frames >= SETTLED_FRAMES && time.elapsed_secs() > MIN_SECS;
            if ready || timed_out {
                if timed_out {
                    warn!(
                        "still loading {:?}; taking the shot anyway",
                        world.pending()
                    );
                }
                capture.state = State::Measuring(0, real.elapsed_secs_f64());
                follow.interval = WaterFollow::default().interval;
            }
        }
        State::Measuring(frames, start) if frames < MEASURED_FRAMES => {
            capture.state = State::Measuring(frames + 1, start);
        }
        State::Measuring(frames, start) => {
            let mean = (real.elapsed_secs_f64() - start) / f64::from(frames);
            info!(
                "frames after loading: {:.2} ms mean ({:.1} fps) over {frames}",
                mean * 1000.0,
                1.0 / mean
            );
            info!(
                "shot: {} tiles resident, {} loading, {:.1} s",
                stats.resident,
                stats.loading,
                time.elapsed_secs()
            );
            follow.interval = 0.0;
            capture.state = State::Settling;
        }
        State::Settling => {
            if !world.pending().is_empty() {
                return;
            }
            capture.state = State::Requested;
            commands
                .spawn(Screenshot::image(capture.target.clone()))
                .observe(save_to_disk(capture.path.clone()))
                .observe(|_: On<ScreenshotCaptured>, mut capture: ResMut<Capture>| {
                    capture.state = State::Saved(0);
                });
        }
        State::Requested => {}
        // `save_to_disk` writes on its own; give it a few frames.
        State::Saved(frames) if frames < 5 => capture.state = State::Saved(frames + 1),
        State::Saved(_) => {
            info!("saved {}", capture.path.display());
            exit.write(AppExit::Success);
        }
    }
}
