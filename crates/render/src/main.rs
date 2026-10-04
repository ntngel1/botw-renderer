//! The world of Breath of the Wild, rendered from the baked `assets/`
//! (see `asset-format`), seen through a free camera.
//!
//! Stage 1 is a port of `the original renderer`'s rendering, module by module, with
//! the dump loading replaced by `assets/`. Shading math is the game's as
//! ported there; this crate does not rewrite it.

// Stage 1: modules are ported whole, ahead of the parts that use them all.
#![allow(dead_code)]

mod camera;
mod capture;
mod cast;
mod character;
mod character_material;
mod climate;
mod clouds;
mod cubemap;
mod daynight;
mod deferred_light;
mod diagnostics;
mod effects;
mod face_material;
mod far_trees;
mod fog;
mod grass;
mod heights;
mod lightning;
mod look;
mod mesh;
mod model_water;
mod models;
mod object_material;
mod objects;
mod postfx;
mod precipitation;
mod ready;
mod sky_lut;
mod sky_occlusion;
mod source;
mod terrain;
mod terrain_material;
mod texture;
mod viewer;
mod volume_mask;
mod water_material;

use std::path::PathBuf;
use std::sync::Arc;

use asset_format::Places;
use bevy::camera::{Exposure, Hdr};
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{Atmosphere, AtmosphereEnvironmentMapLight};
use bevy::pbr::AtmosphereSettings;
use bevy::prelude::*;
use bevy::render::occlusion_culling::OcclusionCulling;
use bevy::window::{PresentMode, WindowResolution};
use bevy::winit::WinitSettings;

use camera::{CameraPlugin, FlyCamera, MainView};
use heights::HeightSampler;
use source::TerrainSource;
use terrain::{TerrainPlugin, TerrainSettings};

const USAGE: &str = "\
usage: render [options]
  --assets <dir>   the baked assets (default: assets/ in the workspace)
  --place <name>   where to start (a place in places.ron; default: the
                   baked region)
  --camera x,y,z,yaw,pitch  start camera (degrees; yaw 0 looks north / -Z);
                   overrides --place's view
  --time <HH:MM>   time of day to start at (default 10:00); screenshots
                   hold it still
  --weather <name> keep one weather everywhere: bluesky, cloudy, rain,
                   heavyrain, snow, heavysnow, thunderstorm, thunderrain
  --hidpi          render at the display's full resolution (Retina: 4x
                   the pixels); default renders 1600x900 pixels
  --no-vsync       disable display sync and background update throttling
  --occlusion-culling  skip meshes hidden behind opaque geometry (experimental)
  --profile        log frame/pass timings and scene counts every 10 seconds
  --screenshot <png>  once the world has loaded, save a frame and exit
  --character <name>  stand a baked character (characters/<name>.ron, e.g.
                   link) on the ground where the start camera looks,
                   facing the camera
  --clip <name>    the clip it loops (default Nml_Wait)
  --villagers      preview the unfinished baked UMii villagers (experimental)

Interactive viewer: F2 debug menu, F1 diagnostics, WASD/QE fly, Shift boost,
RMB look, wheel speed, click scene to capture cursor, Esc to release.
";

/// How many world units make a metre of the atmosphere's air: large, so the
/// atmosphere lays no haze of its own over the terrain (see `setup`).
// SI-RUN-04: Bevy atmosphere for the no-dump path.
const AIR_SCALE: f32 = 1000.0;

/// How fast the free camera starts flying (m/s).
const FLY_SPEED: f32 = 60.0;

struct Args {
    assets: PathBuf,
    place: Option<String>,
    /// Start camera: x, y, z, yaw, pitch (degrees).
    camera: Option<[f32; 5]>,
    /// Time of day to start at, in hours.
    time: Option<f32>,
    /// Weather to keep everywhere (index into `env::WEATHERS`).
    weather: Option<usize>,
    hidpi: bool,
    no_vsync: bool,
    occlusion_culling: bool,
    profile: bool,
    screenshot: Option<PathBuf>,
    /// A character to stand where the camera looks, and its clip.
    character: Option<String>,
    clip: String,
    villagers: bool,
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut args = Args {
        assets: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets"),
        place: None,
        camera: None,
        time: None,
        weather: None,
        hidpi: false,
        no_vsync: false,
        occlusion_culling: false,
        profile: false,
        screenshot: None,
        character: None,
        clip: "Nml_Wait".into(),
        villagers: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--assets" => args.assets = value()?.into(),
            "--place" => args.place = Some(value()?),
            "--camera" => {
                let text = value()?;
                let parts: Vec<f32> = text
                    .split(',')
                    .map(parse_number)
                    .collect::<Result<_, _>>()?;
                args.camera = Some(
                    parts
                        .try_into()
                        .map_err(|_| "--camera needs x,y,z,yaw,pitch".to_string())?,
                );
            }
            "--time" => args.time = Some(parse_time(&value()?)?),
            "--weather" => {
                let name = value()?;
                let found = asset_format::env::WEATHERS
                    .iter()
                    .position(|w| w.eq_ignore_ascii_case(&name));
                args.weather = Some(found.ok_or_else(|| format!("unknown weather {name}"))?);
            }
            "--hidpi" => args.hidpi = true,
            "--no-vsync" => args.no_vsync = true,
            "--occlusion-culling" => args.occlusion_culling = true,
            "--profile" => args.profile = true,
            "--screenshot" => args.screenshot = Some(value()?.into()),
            "--character" => args.character = Some(value()?),
            "--clip" => args.clip = value()?,
            "--villagers" => args.villagers = true,
            "-h" | "--help" => return Ok(None),
            _ => return Err(format!("unknown option {arg}")),
        }
    }
    args.assets = args
        .assets
        .canonicalize()
        .map_err(|e| format!("{}: {e} (run `cargo bake` first)", args.assets.display()))?;
    Ok(Some(args))
}

/// `HH:MM` (or `HH`) as hours.
fn parse_time(text: &str) -> Result<f32, String> {
    let (hours, minutes) = text.split_once(':').unwrap_or((text, "0"));
    let (hours, minutes): (u32, u32) = (parse_number(hours)?, parse_number(minutes)?);
    if hours > 24 || minutes > 59 {
        return Err(format!("--time needs HH:MM, got {text}"));
    }
    Ok(hours as f32 + minutes as f32 / 60.0)
}

fn parse_number<T: std::str::FromStr>(text: &str) -> Result<T, String> {
    text.trim()
        .parse()
        .map_err(|_| format!("not a number: {text}"))
}

fn main() -> AppExit {
    let args = match parse_args() {
        Ok(Some(args)) => args,
        Ok(None) => {
            print!("{USAGE}");
            return AppExit::Success;
        }
        Err(error) => {
            eprintln!("error: {error}\n\n{USAGE}");
            return AppExit::error();
        }
    };
    let source = match TerrainSource::open(args.assets.clone()) {
        Ok(source) => Arc::new(source),
        Err(error) => {
            eprintln!("error: {error} (run `cargo bake` first)");
            return AppExit::error();
        }
    };
    println!("terrain: {}", source.describe());
    let places: Places =
        asset_format::read_ron(&args.assets.join(asset_format::paths::PLACES)).unwrap_or_default();
    let place_name = args
        .place
        .clone()
        .unwrap_or_else(|| source.region().to_owned());
    let Some(place) = places.find(&place_name) else {
        eprintln!("error: no place named {place_name} in places.ron");
        return AppExit::error();
    };
    // The marker's own height is not the ground's; look at the ground.
    let sampler = HeightSampler::new(source.clone());
    let [x, _, z] = place.position;
    let ground = sampler.height_at(x, z).unwrap_or(place.position[1]);
    let start = match args.camera {
        Some([x, y, z, yaw, pitch]) => {
            let fly = FlyCamera {
                yaw: yaw.to_radians(),
                pitch: pitch.to_radians(),
                speed: FLY_SPEED,
            };
            (
                Transform::from_xyz(x, y, z).with_rotation(fly.rotation()),
                fly,
            )
        }
        None => {
            println!("start: {} at ({x:.1}, {ground:.1}, {z:.1})", place.marker);
            camera::Viewpoint::over(&place.marker, Vec3::new(x, ground, z)).camera(FLY_SPEED)
        }
    };

    // A character stands on the ground where the camera looks, facing it.
    let cast: Vec<cast::CastMember> = args
        .character
        .iter()
        .map(|name| {
            let (camera, _) = &start;
            let position = ground_ahead(&sampler, camera);
            let to_camera = camera.translation - position;
            cast::CastMember {
                name: name.clone(),
                position,
                yaw: to_camera.x.atan2(to_camera.z),
                clip: args.clip.clone(),
                phase: 0.0,
            }
        })
        // SI-CHR-03: UMii assembly still needs proportions and face setup.
        .chain(if args.villagers {
            villagers(&args.assets)
        } else {
            Vec::new()
        })
        .collect();

    // SI-RUN-01: window 1600x900 (the game renders 1280x720).
    let mut resolution = WindowResolution::new(1600, 900);
    if !args.hidpi {
        resolution = resolution.with_scale_factor_override(1.0);
    }
    // Screenshots show the time they were asked for.
    let time_flows = args.screenshot.is_none();
    // The game's sky (dome, cube map) only with the baked sky.
    let baked_sky = args.assets.join(asset_format::paths::ENV_PARAMS).exists();
    let terrain_look = (
        look::LookPlugin,
        deferred_light::DeferredLightPlugin {
            assets: args.assets.clone(),
        },
        sky_occlusion::SkyOcclusionPlugin {
            sampler: sampler.clone(),
        },
        terrain_material::TerrainMaterialPlugin {
            assets: args.assets.clone(),
        },
        water_material::WaterMaterialPlugin {
            assets: args.assets.clone(),
        },
        cubemap::CubeMapPlugin { enabled: baked_sky },
    );
    let world_objects = (
        object_material::ObjectMaterialPlugin,
        model_water::ModelWaterPlugin,
        models::ModelsPlugin {
            assets: args.assets.clone(),
        },
        objects::ObjectsPlugin {
            assets: args.assets.clone(),
        },
        effects::EffectsPlugin {
            assets: args.assets.clone(),
            sampler: sampler.clone(),
        },
        far_trees::FarTreesPlugin {
            assets: args.assets.clone(),
        },
        grass::GrassPlugin {
            assets: args.assets.clone(),
            sampler: sampler.clone(),
            // The game's distances.
            reach: 1.0,
        },
        character::CharacterPlugin {
            assets: args.assets.clone(),
        },
        cast::CastPlugin { cast },
    );
    let sky = (
        clouds::CloudsPlugin,
        fog::FogPlugin {
            assets: args.assets.clone(),
        },
        sky_lut::SkyLutPlugin {
            assets: args.assets.clone(),
        },
        postfx::PostFxPlugin,
    );
    let mut app = App::new();
    app.insert_resource(diagnostics::ProfileLog(args.profile));
    // A hidden/unfocused window otherwise sleeps between updates, including
    // offscreen captures. Disable that throttle for screenshots and no-vsync runs.
    if args.screenshot.is_some() || args.no_vsync {
        app.insert_resource(WinitSettings::continuous());
    }
    if let Some(path) = args.screenshot.clone() {
        app.add_plugins(capture::CapturePlugin { path });
    }
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "botw рендерер".into(),
                resolution,
                present_mode: if args.no_vsync {
                    PresentMode::AutoNoVsync
                } else {
                    PresentMode::AutoVsync
                },
                // Screenshots draw off screen (`capture.rs`) and log
                // the frame time: no window to hold them to the
                // display's refresh.
                visible: args.screenshot.is_none(),
                ..default()
            }),
            ..default()
        })
        .set(AssetPlugin {
            file_path: args.assets.to_string_lossy().into_owned(),
            ..default()
        });
    app.add_plugins(plugins)
        .add_plugins(precipitation::PrecipitationPlugin)
        .add_plugins(volume_mask::VolumeMaskPlugin)
        .add_plugins(lightning::LightningPlugin {
            sampler: sampler.clone(),
        })
        .add_plugins(ready::ReadyPlugin)
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight::NONE)
        .add_plugins((
            terrain_look,
            world_objects,
            sky,
            TerrainPlugin {
                source,
                settings: TerrainSettings::default(),
            },
        ))
        .add_plugins(climate::ClimatePlugin {
            assets: args.assets.clone(),
            weather: args.weather,
        })
        .add_plugins(daynight::DayNightPlugin {
            start: args.time.unwrap_or(daynight::DEFAULT_START),
            flowing: time_flows,
            assets: args.assets.clone(),
        })
        .add_systems(
            Startup,
            move |commands: Commands, media: ResMut<Assets<ScatteringMedium>>| {
                setup_scene(commands, media, start, args.occlusion_culling)
            },
        );
    app.add_plugins((CameraPlugin, diagnostics::DiagnosticsPlugin));
    if args.screenshot.is_none() {
        app.add_plugins(viewer::ViewerPlugin { places, sampler });
    }
    app.run()
}

/// Where the camera's view meets the terrain (marching along it a metre at
/// a time, then halving), or the ground 10 m ahead if it does not within
/// 3 km.
fn ground_ahead(sampler: &HeightSampler, camera: &Transform) -> Vec3 {
    let (from, dir) = (camera.translation, *camera.forward());
    let below = |t: f32| {
        let p = from + dir * t;
        sampler.height_at(p.x, p.z).is_some_and(|h| p.y <= h)
    };
    let hit = (1..3000).map(|t| t as f32).find(|&t| below(t));
    let t = match hit {
        Some(t) => {
            let (mut lo, mut hi) = (t - 1.0, t);
            for _ in 0..20 {
                let mid = 0.5 * (lo + hi);
                if below(mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            hi
        }
        None => 10.0,
    };
    let p = from + dir * t;
    Vec3::new(p.x, sampler.height_at(p.x, p.z).unwrap_or(p.y), p.z)
}

fn setup_scene(
    mut commands: Commands,
    mut media: ResMut<Assets<ScatteringMedium>>,
    (transform, fly): (Transform, FlyCamera),
    occlusion_culling: bool,
) {
    // A physically based sky: the atmosphere draws the sky and lights the
    // scene through an environment map. Its own haze over the terrain (the
    // aerial perspective) would lie under the game's (`fog.rs`) and wash the
    // distance out twice, so the planet is scaled up until the world spans a
    // few metres of its air: the sky barely changes (the camera sits
    // `AIR_SCALE` times lower in it), the terrain gets no haze from it.
    // SI-RUN-04: Bevy atmosphere for the no-dump path.
    let atmosphere = Atmosphere::earth(media.add(ScatteringMedium::earth(256, 256)));
    let planet = Transform::from_translation(-Vec3::Y * atmosphere.inner_radius * AIR_SCALE)
        .with_scale(Vec3::splat(AIR_SCALE));
    commands.spawn((atmosphere, GlobalTransform::from(planet)));
    let mut camera = commands.spawn((
        Camera3d::default(),
        MainView,
        // SI-RUN-02: camera near, far and FOV are not recovered.
        // SI-CAM-02: Bevy default FOV and near plane.
        Projection::Perspective(PerspectiveProjection {
            near: 0.3,
            far: 30_000.0,
            ..default()
        }),
        transform,
        fly,
        Msaa::Off,
        AtmosphereSettings::default(),
        AtmosphereEnvironmentMapLight {
            intensity: daynight::SKY_LIGHT,
            ..default()
        },
        Exposure {
            ev100: daynight::DAY_EV100,
        },
        // The game's bloom and tone curve replace Bevy's (`postfx.rs`).
        Hdr,
        Tonemapping::None,
        // The field's ambient occlusion and the clouds' reduced buffer read
        // the depth around a pixel; water reads the depth of what lies
        // under it.
        DepthPrepass,
    ));
    if occlusion_culling {
        camera.insert(OcclusionCulling);
    }
}

/// The villagers the map places (`characters/umii/placed.ron`, `cargo bake
/// --only npcs`), each looping its idle from its own point in the loop.
fn villagers(assets: &std::path::Path) -> Vec<cast::CastMember> {
    let path = assets.join(asset_format::paths::UMII_PLACED);
    let Ok(placed) = asset_format::read_ron::<asset_format::character::PlacedCharacters>(&path)
    else {
        return Vec::new();
    };
    placed
        .placed
        .into_iter()
        .map(|p| cast::CastMember {
            name: p.character,
            position: Vec3::from(p.translate),
            // The map turns villagers about the vertical only.
            yaw: p.rotate[1],
            clip: String::new(),
            // Ours: a phase from the placement's hash.
            phase: (p.hash_id % 997) as f32 / 997.0 * 4.0,
        })
        .collect()
}
