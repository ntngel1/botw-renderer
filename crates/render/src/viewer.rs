//! Interactive map tools. The environment editor walks the serialized schema,
//! so new baked parameters automatically get controls without a second list.

use asset_format::{Places, eco::CLIMATES, env::WEATHERS};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::render::occlusion_culling::OcclusionCulling;
use bevy::window::{CursorGrabMode, CursorOptions, PresentMode, PrimaryWindow};
use bevy::winit::{UpdateMode, WinitSettings};
use bevy_egui::{
    EguiContext, EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPreUpdateSet,
    EguiPrimaryContextPass, PrimaryEguiContext, egui,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::camera::{FlyCamera, MainCamera, VIEWPOINTS, Viewpoint};
use crate::climate::{Climate, ClimateOverride, Weather};
use crate::daynight::{Environment, TimeOfDay};
use crate::heights::HeightSampler;
use crate::mesh::ColorMode;
use crate::postfx::ToneChoice;
use crate::terrain::{TerrainSettings, TerrainStream};

pub struct ViewerPlugin {
    pub places: Places,
    pub sampler: HeightSampler,
}

impl Plugin for ViewerPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .insert_resource(EguiGlobalSettings {
                auto_create_primary_context: false,
                ..default()
            })
            .add_systems(PostStartup, attach_context)
            .insert_resource(Viewer {
                open: true,
                places: self.places.clone(),
                sampler: self.sampler.clone(),
                search: String::new(),
                character: "link".into(),
                clip: String::new(),
                clip_fade: 0.15,
            })
            .init_resource::<ViewerInput>()
            .init_resource::<MenuScale>()
            .init_resource::<UiScale>()
            .add_systems(
                PreUpdate,
                update_menu_scale.before(EguiPreUpdateSet::InitContexts),
            )
            .add_systems(
                PreUpdate,
                viewer_input
                    .after(EguiPreUpdateSet::BeginPass)
                    .before(crate::camera::read_look_input),
            )
            .add_systems(EguiPrimaryContextPass, debug_menu);
    }
}

fn attach_context(mut commands: Commands, cameras: Query<Entity, With<crate::camera::MainView>>) {
    for entity in &cameras {
        commands.entity(entity).insert(PrimaryEguiContext);
    }
}

#[derive(Resource)]
struct MenuScale(f32);

impl Default for MenuScale {
    fn default() -> Self {
        Self(1.5)
    }
}

fn update_menu_scale(
    scale: Res<MenuScale>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut contexts: Query<&mut EguiContext, With<PrimaryEguiContext>>,
    mut overlay_scale: ResMut<UiScale>,
) {
    let Ok(window) = windows.single() else { return };
    // The renderer overrides the window scale to 1 to control render pixels.
    // egui receives that effective scale from the camera; compensate for the
    // display's native DPI without applying it twice in --hidpi mode.
    let zoom = scale.0 * window.resolution.base_scale_factor() / window.scale_factor();
    // The F1 diagnostics use Bevy UI rather than egui.
    if overlay_scale.0 != zoom {
        overlay_scale.0 = zoom;
    }
    for mut context in &mut contexts {
        // EguiZoomFactor is a readback component, not a zoom control.
        context.get_mut().set_zoom_factor(zoom);
    }
}

#[derive(Resource)]
struct Viewer {
    open: bool,
    places: Places,
    sampler: HeightSampler,
    search: String,
    character: String,
    clip: String,
    clip_fade: f32,
}

#[derive(Resource, Default)]
pub struct ViewerInput {
    pub pointer: bool,
    pub keyboard: bool,
}

fn viewer_input(
    mut contexts: EguiContexts,
    keys: Res<ButtonInput<KeyCode>>,
    mut viewer: ResMut<Viewer>,
    mut input: ResMut<ViewerInput>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::F2) {
        viewer.open = !viewer.open;
        if viewer.open {
            for mut cursor in &mut cursors {
                cursor.grab_mode = CursorGrabMode::None;
                cursor.visible = true;
            }
        }
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let grabbed = cursors.iter().any(|c| c.grab_mode != CursorGrabMode::None);
    input.pointer = viewer.open && !grabbed && ctx.egui_wants_pointer_input();
    input.keyboard = viewer.open && ctx.egui_wants_keyboard_input();
}

#[derive(SystemParam)]
struct Controls<'w, 's> {
    menu_scale: ResMut<'w, MenuScale>,
    window: Query<'w, 's, &'static mut Window, With<PrimaryWindow>>,
    culling: Query<'w, 's, (Entity, Has<OcclusionCulling>), MainCamera>,
    profile: ResMut<'w, crate::diagnostics::ProfileLog>,
    overlay: Query<'w, 's, &'static mut Visibility, With<crate::diagnostics::Overlay>>,
    updates: ResMut<'w, WinitSettings>,
    time: ResMut<'w, TimeOfDay>,
    weather: ResMut<'w, Weather>,
    camera_effects: ResMut<'w, crate::effects::CameraEffectSettings>,
    effect_render: ResMut<'w, crate::effects::EffectRenderSettings>,
    climate: Res<'w, Climate>,
    climate_override: ResMut<'w, ClimateOverride>,
    environment: ResMut<'w, Environment>,
    terrain: ResMut<'w, TerrainSettings>,
    stream: ResMut<'w, TerrainStream>,
    tone: ResMut<'w, ToneChoice>,
    grass: ResMut<'w, crate::grass::Grass>,
    grass_color: ResMut<'w, crate::grass::GrassColorMap>,
    trees: ResMut<'w, crate::far_trees::TreeSettings>,
    water: Res<'w, crate::water_material::WaterLook>,
    water_materials: ResMut<'w, Assets<crate::water_material::WaterMaterial>>,
    loading: Res<'w, crate::daynight::LoadingEnvironment>,
    objects: ResMut<'w, crate::objects::Objects>,
    far: ResMut<'w, crate::objects::FarModels>,
    camera: Query<
        'w,
        's,
        (
            &'static mut Transform,
            &'static mut FlyCamera,
            &'static mut Projection,
        ),
        MainCamera,
    >,
    cast: ResMut<'w, crate::cast::Cast>,
    rigs: Query<
        'w,
        's,
        (
            Entity,
            &'static crate::character::CharacterRig,
            &'static mut crate::character::Animator,
        ),
    >,
    commands: Commands<'w, 's>,
}

/// Values at viewer startup, including CLI/environment overrides. Runtime
/// streaming state is kept separately and refreshed when settings are restored.
struct InitialSettings {
    menu_scale: f32,
    present_mode: PresentMode,
    unfocused_mode: UpdateMode,
    profile: bool,
    overlay: Visibility,
    occlusion: bool,
    camera: (Transform, FlyCamera, Projection),
    time: TimeOfDay,
    weather: Option<usize>,
    climate: Option<usize>,
    camera_effects: crate::effects::CameraEffectSettings,
    effect_render: crate::effects::EffectRenderSettings,
    terrain: TerrainSettings,
    tone: ToneChoice,
    grass: (bool, f32, bool),
    trees: crate::far_trees::TreeSettings,
    objects: [f32; 5],
    water_aerial: Option<f32>,
}

impl InitialSettings {
    fn capture(c: &Controls) -> Option<Self> {
        let window = c.window.single().ok()?;
        let (transform, fly, projection) = c.camera.single().ok()?;
        Some(Self {
            menu_scale: c.menu_scale.0,
            present_mode: window.present_mode,
            unfocused_mode: c.updates.unfocused_mode,
            profile: c.profile.0,
            overlay: *c.overlay.single().ok()?,
            occlusion: c.culling.single().ok()?.1,
            camera: (*transform, *fly, projection.clone()),
            time: c.time.clone(),
            weather: c.weather.forced(),
            climate: c.climate_override.0,
            camera_effects: *c.camera_effects,
            effect_render: *c.effect_render,
            terrain: c.terrain.clone(),
            tone: *c.tone,
            grass: (
                c.grass.enabled,
                c.grass.reach,
                c.grass_color.use_baked_color,
            ),
            trees: *c.trees,
            objects: [
                c.objects.spawn_radius,
                c.objects.despawn_radius,
                c.objects.far_radius,
                c.objects.large_size,
                c.far.horizon,
            ],
            water_aerial: c
                .water_materials
                .get(&c.water.material)
                .map(|m| m.extension.params.surface.z),
        })
    }

    fn restore(&self, c: &mut Controls, viewer: &mut Viewer, environment: &Environment) {
        c.menu_scale.0 = self.menu_scale;
        if let Ok(mut window) = c.window.single_mut() {
            window.present_mode = self.present_mode;
        }
        c.updates.unfocused_mode = self.unfocused_mode;
        c.profile.0 = self.profile;
        if let Ok(mut visibility) = c.overlay.single_mut() {
            *visibility = self.overlay;
        }
        if let Ok((camera, _)) = c.culling.single() {
            if self.occlusion {
                c.commands.entity(camera).insert(OcclusionCulling);
            } else {
                c.commands.entity(camera).remove::<OcclusionCulling>();
            }
        }
        if let Ok((mut transform, mut fly, mut projection)) = c.camera.single_mut() {
            *transform = self.camera.0;
            *fly = self.camera.1;
            *projection = self.camera.2.clone();
        }
        *c.time = self.time.clone();
        c.weather.force(self.weather);
        c.climate_override.0 = self.climate;
        *c.camera_effects = self.camera_effects;
        *c.effect_render = self.effect_render;
        *c.environment = environment.clone();
        *c.terrain = self.terrain.clone();
        c.stream.clear(&mut c.commands);
        *c.tone = self.tone;
        (c.grass.enabled, c.grass.reach) = (self.grass.0, self.grass.1);
        c.grass_color.set_baked_color(self.grass.2);
        *c.trees = self.trees;
        [
            c.objects.spawn_radius,
            c.objects.despawn_radius,
            c.objects.far_radius,
            c.objects.large_size,
            c.far.horizon,
        ] = self.objects;
        c.objects.refresh(&mut c.commands);
        c.far.refresh(&mut c.commands);
        if let Some(value) = self.water_aerial
            && let Some(mut material) = c.water_materials.get_mut(&c.water.material)
        {
            material.extension.params.surface.z = value;
        }
        viewer.search.clear();
        viewer.character = "link".into();
        viewer.clip.clear();
        viewer.clip_fade = 0.15;
        for (_, _, mut animator) in &mut c.rigs {
            animator.speed = 1.0;
        }
    }
}

fn debug_menu(
    mut contexts: EguiContexts,
    mut viewer: ResMut<Viewer>,
    mut c: Controls,
    mut original_environment: Local<Option<Environment>>,
    mut initial_settings: Local<Option<InitialSettings>>,
) -> Result {
    if initial_settings.is_none() {
        *initial_settings = InitialSettings::capture(&c);
    }
    if original_environment.is_none() && !c.loading.is_loading() {
        *original_environment = Some(c.environment.clone());
    }
    if !viewer.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let mut open = viewer.open;
    let screen = ctx.content_rect();
    egui::Window::new("Map viewer · Debug")
        .open(&mut open)
        .default_pos([(screen.right() - 442.0).max(screen.left()), 12.0])
        .default_width(430.0)
        .default_height((screen.height() - 40.0).clamp(120.0, 760.0))
        .vscroll(true)
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(
                    initial_settings.is_some() && original_environment.is_some(),
                    egui::Button::new("Reset settings"),
                ).on_hover_text("Restore all settings and the camera to their startup values.").clicked() {
                    if let (Some(initial), Some(environment)) =
                        (initial_settings.as_ref(), original_environment.as_ref()) {
                        initial.restore(&mut c, &mut viewer, environment);
                    }
                }
                if let Ok(mut visibility) = c.overlay.single_mut() {
                    let visible = *visibility != Visibility::Hidden;
                    if ui.button(if visible { "Hide FPS / stats" } else { "Show FPS / stats" }).clicked() {
                        *visibility = if visible { Visibility::Hidden } else { Visibility::Visible };
                    }
                }
            });
            ui.add(
                egui::Slider::new(&mut c.menu_scale.0, 1.0..=2.5)
                    .text("Menu scale")
                    .fixed_decimals(2),
            );
            ui.label("F2 menu · F1 stats · Esc releases cursor");
            ui.label("WASD / QE fly · Shift boost · RMB look · Wheel speed");
            ui.separator();
            ui.collapsing("Performance", |ui| {
                if let Ok(mut window) = c.window.single_mut() {
                    let mut vsync = !matches!(window.present_mode, PresentMode::AutoNoVsync | PresentMode::Immediate | PresentMode::Mailbox);
                    if ui.checkbox(&mut vsync, "VSync").changed() {
                        window.present_mode = if vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
                        c.updates.unfocused_mode = if vsync {
                            WinitSettings::game().unfocused_mode
                        } else {
                            UpdateMode::Continuous
                        };
                    }
                    ui.label(format!("Rendering {} × {} pixels", window.physical_width(), window.physical_height()));
                }
                if let Ok((camera, mut enabled)) = c.culling.single() {
                    if ui.checkbox(&mut enabled, "Experimental GPU occlusion culling").changed() {
                        if enabled {
                            c.commands.entity(camera).insert(OcclusionCulling);
                        } else {
                            c.commands.entity(camera).remove::<OcclusionCulling>();
                        }
                    }
                }
                let mut throttle = !matches!(c.updates.unfocused_mode, UpdateMode::Continuous);
                if ui.checkbox(&mut throttle, "Throttle when window is unfocused").changed() {
                    c.updates.unfocused_mode = if throttle {
                        WinitSettings::game().unfocused_mode
                    } else {
                        UpdateMode::Continuous
                    };
                }
                ui.checkbox(&mut c.profile.0, "Log detailed profile every 10 seconds");
                ui.label("F1 shows timings. On Metal, pass timings measure CPU encoding, not GPU execution.");
            });
            ui.collapsing("Camera & places", |ui| {
                if let Ok((mut transform, mut fly, mut projection)) = c.camera.single_mut() {
                    ui.horizontal(|ui| {
                        ui.label("Position");
                        let position = &mut transform.translation;
                        for v in [&mut position.x, &mut position.y, &mut position.z] {
                            ui.add(egui::DragValue::new(v).speed(1.0));
                        }
                    });
                    let mut yaw = (fly.yaw.to_degrees() + 180.0).rem_euclid(360.0) - 180.0;
                    let mut pitch = fly.pitch.to_degrees();
                    scalar(ui, "Yaw (degrees)", &mut yaw, -180.0..=180.0, 0.5);
                    scalar(ui, "Pitch (degrees)", &mut pitch, -88.8..=88.8, 0.5);
                    fly.yaw = yaw.to_radians();
                    fly.pitch = pitch.to_radians();
                    transform.rotation = fly.rotation();
                    scalar(ui, "Speed (m/s)", &mut fly.speed, 2.0..=20_000.0, 1.0);
                    if let Projection::Perspective(p) = &mut *projection {
                        let mut fov = p.fov.to_degrees();
                        scalar(ui, "FOV (degrees)", &mut fov, 10.0..=150.0, 0.5);
                        p.fov = fov.to_radians();
                        scalar(ui, "Near plane", &mut p.near, 0.01..=100.0, 0.01);
                        let near = p.near + 1.0;
                        scalar(ui, "Far plane", &mut p.far, near..=100_000.0, 100.0);
                    }
                    egui::ComboBox::from_id_salt("jump")
                        .selected_text("Jump to place…")
                        .show_ui(ui, |ui| {
                            for place in &viewer.places.places {
                                if ui.button(&place.marker).clicked() {
                                    let [x, y, z] = place.position;
                                    let ground = viewer.sampler.height_at(x, z).unwrap_or(y);
                                    (*transform, *fly) =
                                        Viewpoint::over(&place.marker, Vec3::new(x, ground, z))
                                            .camera(fly.speed);
                                }
                            }
                            ui.separator();
                            for viewpoint in &VIEWPOINTS {
                                if ui.button(viewpoint.name).clicked() {
                                    (*transform, *fly) = viewpoint.camera(fly.speed);
                                }
                            }
                        });
                }
            });
            egui::CollapsingHeader::new("Time & weather")
                .default_open(true)
                .show(ui, |ui| {
                    ui.add(egui::Slider::new(&mut c.time.hours, 0.0..=23.999).text("Hour"));
                    ui.label(format!(
                        "{:02}:{:02}",
                        c.time.hours as u32,
                        ((c.time.hours.fract() * 60.0) as u32).min(59)
                    ));
                    let mut flowing = c.time.speed != 0.0;
                    if ui.checkbox(&mut flowing, "Time flows").changed() {
                        c.time.speed = if flowing { TimeOfDay::GAME_SPEED } else { 0.0 };
                    }
                    scalar(
                        ui,
                        "Game hours / second",
                        &mut c.time.speed,
                        0.0..=24.0,
                        0.001,
                    );
                    ui.horizontal(|ui| {
                        ui.label("Day / moon phase");
                        ui.add(egui::DragValue::new(&mut c.time.day).range(0..=1_000_000));
                    });
                    let mut forced = c.weather.forced();
                    choice(ui, "Weather", &mut forced, &WEATHERS);
                    if forced != c.weather.forced() {
                        c.weather.force(forced);
                    }
                    choice(ui, "Climate", &mut c.climate_override.0, &CLIMATES);
                    ui.label(format!(
                        "Current: {} · {}",
                        c.climate.name(),
                        c.weather.name()
                    ));
                    if let Some(t) = c.weather.temperature {
                        ui.label(format!("Temperature: {t:.1} °C"));
                    }
                });
            ui.collapsing("Particle effects", |ui| {
                ui.checkbox(
                    &mut c.camera_effects.field_haze,
                    "Experimental camera haze (FieldEnvEffect)",
                );
                ui.checkbox(&mut c.camera_effects.volume_dust, "Experimental volume dust");
                ui.checkbox(&mut c.camera_effects.volume_fog, "Experimental volume fog");
                ui.checkbox(&mut c.camera_effects.volume_add, "Experimental volume add");
                ui.checkbox(
                    &mut c.effect_render.deferred_preview,
                    "Experimental deferred particles (volcano plumes)",
                ).on_hover_text("Preview only: these game shaders write G-buffer material data. Their deferred rendering is not ported yet.");
                ui.label("Off by default: camera haze activation and volume particle rendering are incomplete.");
            });
            ui.collapsing("Terrain streaming", |ui| {
                let mut rebuild = false;
                ui.horizontal(|ui| {
                    ui.label("Mesh resolution");
                    rebuild |= ui
                        .add(egui::DragValue::new(&mut c.terrain.mesh_resolution).range(2..=255))
                        .changed();
                });
                scalar(
                    ui,
                    "Split factor",
                    &mut c.terrain.split_factor,
                    0.1..=10.0,
                    0.1,
                );
                ui.horizontal(|ui| {
                    ui.label("Max LOD");
                    ui.add(
                        egui::DragValue::new(&mut c.terrain.max_lod)
                            .range(0..=asset_format::terrain::MAX_LOD),
                    );
                });
                ui.horizontal(|ui| {
                    ui.label("Concurrent loads");
                    ui.add(egui::DragValue::new(&mut c.terrain.max_loads_in_flight).range(1..=128));
                });
                ui.horizontal(|ui| {
                    ui.label("Cache (tiles)");
                    ui.add(egui::DragValue::new(&mut c.terrain.cache_budget).range(16..=20_000));
                });
                let mut materials = matches!(c.terrain.color_mode, ColorMode::Materials);
                if ui
                    .checkbox(&mut materials, "Material debug colours")
                    .changed()
                {
                    c.terrain.color_mode = if materials {
                        ColorMode::Materials
                    } else {
                        ColorMode::Natural
                    };
                    rebuild = true;
                }
                if rebuild || ui.button("Reload terrain tiles").clicked() {
                    c.stream.clear(&mut c.commands);
                }
            });
            ui.collapsing("Objects & grass", |ui| {
                let before = (
                    c.objects.spawn_radius,
                    c.objects.despawn_radius,
                    c.objects.far_radius,
                    c.objects.large_size,
                    c.far.horizon,
                );
                scalar(
                    ui,
                    "Object spawn radius",
                    &mut c.objects.spawn_radius,
                    1.0..=20_000.0,
                    10.0,
                );
                let spawn = c.objects.spawn_radius;
                scalar(
                    ui,
                    "Object despawn radius",
                    &mut c.objects.despawn_radius,
                    spawn..=30_000.0,
                    10.0,
                );
                scalar(
                    ui,
                    "Large object radius",
                    &mut c.objects.far_radius,
                    1.0..=30_000.0,
                    10.0,
                );
                scalar(
                    ui,
                    "Large object size",
                    &mut c.objects.large_size,
                    0.1..=1000.0,
                    1.0,
                );
                scalar(
                    ui,
                    "Far models horizon",
                    &mut c.far.horizon,
                    1.0..=50_000.0,
                    100.0,
                );
                ui.checkbox(&mut c.grass.enabled, "Grass enabled");
                scalar(
                    ui,
                    "Grass reach multiplier",
                    &mut c.grass.reach,
                    0.1..=10.0,
                    0.1,
                );
                if before
                    != (
                        c.objects.spawn_radius,
                        c.objects.despawn_radius,
                        c.objects.far_radius,
                        c.objects.large_size,
                        c.far.horizon,
                    )
                {
                    c.objects.refresh(&mut c.commands);
                    c.far.refresh(&mut c.commands);
                }
                let mut baked = c.grass_color.use_baked_color;
                if ui.checkbox(&mut baked, "Use baked grass colour").changed() {
                    c.grass_color.set_baked_color(baked);
                }
                let mut trees = *c.trees;
                ui.checkbox(&mut trees.enabled, "Far trees enabled");
                ui.checkbox(&mut trees.shadows, "Far tree shadows");
                ui.checkbox(&mut trees.billboards_only, "Trees as billboards only");
                if trees != *c.trees {
                    *c.trees = trees;
                }
            });
            ui.collapsing("Characters & animation", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Baked character");
                    ui.text_edit_singleline(&mut viewer.character);
                });
                ui.horizontal(|ui| {
                    ui.label("Starting clip (empty = idle)");
                    ui.text_edit_singleline(&mut viewer.clip);
                });
                if ui
                    .add_enabled(
                        !viewer.character.trim().is_empty(),
                        egui::Button::new("Place on ground ahead"),
                    )
                    .clicked()
                {
                    if let Ok((transform, _, _)) = c.camera.single() {
                        let position = crate::ground_ahead(&viewer.sampler, transform);
                        let to_camera = transform.translation - position;
                        c.cast.place(crate::cast::CastMember {
                            name: viewer.character.trim().into(),
                            position,
                            yaw: to_camera.x.atan2(to_camera.z),
                            clip: viewer.clip.trim().into(),
                            phase: 0.0,
                        });
                    }
                }
                if c.cast.is_pending() {
                    ui.label("Loading characters…");
                }
                scalar(
                    ui,
                    "Clip crossfade (seconds)",
                    &mut viewer.clip_fade,
                    0.0..=10.0,
                    0.01,
                );
                for (entity, rig, mut animator) in &mut c.rigs {
                    ui.push_id(entity, |ui| {
                        ui.collapsing(format!("Character {entity}"), |ui| {
                            scalar(
                                ui,
                                "Playback speed (0 = pause)",
                                &mut animator.speed,
                                0.0..=10.0,
                                0.05,
                            );
                            let mut selected = animator.current().unwrap_or("").to_owned();
                            let before = selected.clone();
                            egui::ComboBox::from_label("Body clip")
                                .selected_text(&selected)
                                .show_ui(ui, |ui| {
                                    for name in rig.asset.clip_names() {
                                        ui.selectable_value(&mut selected, name.to_owned(), name);
                                    }
                                });
                            if before != selected {
                                animator.play(&rig.asset, &selected, viewer.clip_fade);
                            }
                            if !selected.is_empty() {
                                let mut frame = animator.frame().unwrap_or(0.0);
                                let max = rig.asset.frames(&selected).unwrap_or(0.0);
                                if ui
                                    .add(egui::Slider::new(&mut frame, 0.0..=max).text("Frame"))
                                    .changed()
                                {
                                    animator.play_frames(&rig.asset, &selected, 0.0, frame, None);
                                }
                                if ui.button("Restart clip").clicked() {
                                    animator.restart(&rig.asset, &selected, viewer.clip_fade);
                                }
                            }
                            let mut face = animator.face().map_or("", |(name, _)| name).to_owned();
                            let before = face.clone();
                            egui::ComboBox::from_label("Face clip")
                                .selected_text(&face)
                                .show_ui(ui, |ui| {
                                    for name in rig.asset.clip_names() {
                                        ui.selectable_value(&mut face, name.to_owned(), name);
                                    }
                                });
                            if before != face {
                                animator.play_face(&rig.asset, &face, viewer.clip_fade);
                            }
                        });
                    });
                }
            });
            ui.collapsing("Water", |ui| {
                if let Some(mut material) = c.water_materials.get_mut(&c.water.material) {
                    scalar(
                        ui,
                        "Aerial perspective strength",
                        &mut material.extension.params.surface.z,
                        0.0..=10.0,
                        0.01,
                    );
                }
            });
            ui.collapsing("Post processing", |ui| {
                ui.checkbox(&mut c.tone.color_table, "Colour table tone curve");
                ui.checkbox(&mut c.tone.no_bloom, "Disable bloom");
                scalar(ui, "HDR scale", &mut c.tone.hdr_scale, 0.001..=100.0, 0.01);
            });
            ui.separator();
            ui.label("Environment parameters (live)");
            ui.label("All baked fields; changes apply for this session.");
            ui.horizontal(|ui| {
                ui.label("Filter");
                ui.text_edit_singleline(&mut viewer.search);
            });
            let filter = viewer.search.to_lowercase();
            // Each section is serialized only while open. Edits are committed only
            // on a changed widget, preserving Bevy resource change detection.
            if ui
                .add_enabled(
                    original_environment.is_some(),
                    egui::Button::new("Reset environment to loaded values"),
                )
                .clicked()
            {
                if let Some(original) = original_environment.as_ref() {
                    *c.environment = original.clone();
                }
            }
            if c.loading.is_loading() {
                ui.label("Loading environment parameters…");
                return;
            }
            let env = c.environment.bypass_change_detection();
            let mut changed = false;
            changed |= section(ui, "Sun", &mut env.sun, &filter);
            changed |= section(ui, "Clouds", &mut env.clouds, &filter);
            changed |= section(ui, "Climate definitions", &mut env.climates, &filter);
            changed |= section(
                ui,
                "Weather influences",
                &mut env.weather_influences,
                &filter,
            );
            changed |= section(ui, "Palette sets", &mut env.sets, &filter);
            changed |= section(ui, "Base palettes / fallback", &mut env.palettes, &filter);
            changed |= section(
                ui,
                "Shared palette parameters",
                &mut env.palette_static,
                &filter,
            );
            changed |= section(
                ui,
                "Renderer (light, fog, water, bloom, sky)",
                &mut env.renderer,
                &filter,
            );
            if changed {
                c.environment.set_changed();
            }
        });
    viewer.open = open;
    Ok(())
}

fn scalar(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    speed: f64,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::DragValue::new(value).range(range).speed(speed));
    });
}

fn choice(ui: &mut egui::Ui, label: &str, value: &mut Option<usize>, names: &[&str]) {
    egui::ComboBox::from_label(label)
        .selected_text(
            value
                .and_then(|i| names.get(i))
                .copied()
                .unwrap_or("Automatic"),
        )
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, "Automatic");
            for (i, name) in names.iter().enumerate() {
                ui.selectable_value(value, Some(i), *name);
            }
        });
}

fn section<T: Serialize + DeserializeOwned>(
    ui: &mut egui::Ui,
    label: &str,
    target: &mut T,
    filter: &str,
) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new(label)
        .open(if filter.is_empty() { None } else { Some(true) })
        .show(ui, |ui| {
            let Ok(mut value) = serde_json::to_value(&*target) else {
                ui.label("Cannot encode parameters");
                return;
            };
            if edit_value(ui, &mut value, filter) {
                match serde_json::from_value(value) {
                    Ok(next) => {
                        *target = next;
                        changed = true;
                    }
                    Err(error) => {
                        ui.colored_label(egui::Color32::RED, error.to_string());
                    }
                }
            }
        });
    changed
}

fn matches_filter(label: &str, value: &Value, filter: &str) -> bool {
    filter.is_empty()
        || label.to_lowercase().contains(filter)
        || match value {
            Value::Object(fields) => fields.iter().any(|(k, v)| matches_filter(k, v, filter)),
            Value::Array(values) => values.iter().any(|v| matches_filter("", v, filter)),
            _ => false,
        }
}

fn edit_value(ui: &mut egui::Ui, value: &mut Value, filter: &str) -> bool {
    let mut changed = false;
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                if !matches_filter(name, value, filter) {
                    continue;
                }
                ui.push_id(name, |ui| {
                    if matches!(name.as_str(), "clamped_luminance" | "distance_attenuation") {
                        ui.horizontal(|ui| {
                            let mut enabled = !value.is_null();
                            if ui.checkbox(&mut enabled, name).changed() {
                                *value = if enabled {
                                    Value::from(1.0)
                                } else {
                                    Value::Null
                                };
                                changed = true;
                            }
                            if enabled {
                                changed |= edit_value(ui, value, "");
                            }
                        });
                    } else if name == "kind"
                        && serde_json::from_value::<asset_format::envset::CurveKind>(value.clone())
                            .is_ok()
                    {
                        let mut kind: asset_format::envset::CurveKind =
                            serde_json::from_value(value.clone()).unwrap();
                        let before = kind;
                        use asset_format::envset::CurveKind;
                        egui::ComboBox::from_label(name)
                            .selected_text(format!("{kind:?}"))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut kind, CurveKind::Linear2D, "Linear2D");
                                ui.selectable_value(&mut kind, CurveKind::Hermit2D, "Hermit2D");
                                ui.selectable_value(&mut kind, CurveKind::Step2D, "Step2D");
                                ui.selectable_value(&mut kind, CurveKind::Other(0), "Other");
                            });
                        if let CurveKind::Other(n) = &mut kind {
                            ui.add(egui::DragValue::new(n));
                        }
                        if before != kind {
                            *value = serde_json::to_value(kind).unwrap();
                            changed = true;
                        }
                    } else if value.is_object() || value.is_array() {
                        egui::CollapsingHeader::new(name)
                            .open(if filter.is_empty() { None } else { Some(true) })
                            .show(ui, |ui| {
                                changed |= edit_value(
                                    ui,
                                    value,
                                    if name.to_lowercase().contains(filter) {
                                        ""
                                    } else {
                                        filter
                                    },
                                );
                            });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(name);
                            changed |= edit_value(ui, value, "");
                        });
                    }
                });
            }
        }
        Value::Array(values) => {
            for (i, value) in values.iter_mut().enumerate() {
                if !matches_filter("", value, filter) {
                    continue;
                }
                ui.push_id(i, |ui| {
                    if value.is_object() || value.is_array() {
                        egui::CollapsingHeader::new(format!("[{i}]"))
                            .open(if filter.is_empty() { None } else { Some(true) })
                            .show(ui, |ui| {
                                changed |= edit_value(ui, value, filter);
                            });
                    } else {
                        ui.horizontal(|ui| {
                            ui.label(format!("[{i}]"));
                            changed |= edit_value(ui, value, "");
                        });
                    }
                });
            }
            if values.is_empty() {
                ui.label("No baked entries");
            }
        }
        Value::Bool(v) => {
            changed = ui.checkbox(v, "").changed();
        }
        Value::Number(v) => {
            if v.is_f64() {
                let mut n = v.as_f64().unwrap_or_default();
                if ui.add(egui::DragValue::new(&mut n).speed(0.01)).changed() && n.is_finite() {
                    *value = Value::from(n);
                    changed = true;
                }
            } else if v.is_u64() {
                let mut n = v.as_u64().unwrap_or_default();
                if ui.add(egui::DragValue::new(&mut n)).changed() {
                    *value = Value::from(n);
                    changed = true;
                }
            } else {
                let mut n = v.as_i64().unwrap_or_default();
                if ui.add(egui::DragValue::new(&mut n)).changed() {
                    *value = Value::from(n);
                    changed = true;
                }
            }
        }
        Value::String(v) => {
            changed = ui.text_edit_singleline(v).changed();
        }
        Value::Null => {
            ui.label("None (not baked)");
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retina_zoom_reaches_egui_and_tracks_resolution_and_display_changes() {
        let mut app = App::new();
        app.init_resource::<MenuScale>()
            .init_resource::<UiScale>()
            .add_systems(PreUpdate, update_menu_scale);
        let mut resolution =
            bevy::window::WindowResolution::new(1600, 900).with_scale_factor_override(1.0);
        resolution.set_scale_factor(2.0);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution,
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        let context = app.world_mut().spawn(PrimaryEguiContext).id();

        let pixels_per_point = |app: &mut App| {
            app.update();
            let native = app.world().get::<Window>(window).unwrap().scale_factor();
            let mut input = egui::RawInput::default();
            input
                .viewports
                .get_mut(&egui::ViewportId::ROOT)
                .unwrap()
                .native_pixels_per_point = Some(native);
            let mut context = app.world_mut().get_mut::<EguiContext>(context).unwrap();
            let ctx = context.get_mut();
            ctx.begin_pass(input);
            ctx.end_pass().pixels_per_point
        };

        // Retina at a forced 1x render scale still renders 150% UI at 3 px/pt.
        assert_eq!(pixels_per_point(&mut app), 3.0);
        assert_eq!(app.world().resource::<UiScale>().0, 3.0);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(3456, 2234);
        assert_eq!(pixels_per_point(&mut app), 3.0);
        // Native HiDPI rendering must not double the Retina compensation.
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_scale_factor_override(None);
        assert_eq!(pixels_per_point(&mut app), 3.0);
        app.world_mut().resource_mut::<MenuScale>().0 = 2.0;
        assert_eq!(pixels_per_point(&mut app), 4.0);
        // Moving to a 1x monitor keeps the user's 200% preference.
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_scale_factor(1.0);
        assert_eq!(pixels_per_point(&mut app), 2.0);
    }

    #[test]
    fn environment_schema_round_trips_every_section() {
        let env = Environment::fallback();
        let value = serde_json::to_value(&env).unwrap();
        let decoded: Environment = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), value);
        assert!(value["renderer"].is_object());
        assert!(value["clouds"].is_object());
    }

    #[test]
    fn parameter_filter_finds_nested_array_fields() {
        let value = serde_json::json!({"layers": [{"wind_power": 2.0}], "bloom": false});
        assert!(matches_filter("clouds", &value, "wind"));
        assert!(matches_filter("clouds", &value, "cloud"));
        assert!(!matches_filter("clouds", &value, "unknown"));
    }
}
