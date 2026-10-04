//! The free-flying camera. Look around by holding the right mouse button,
//! or click to capture the cursor (Esc releases it). WASD/QE move, Shift
//! speeds up, the wheel changes the speed, number keys jump to viewpoints.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

/// Marks the camera the frame is seen through (not the cube map's faces).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct MainView;

/// The view the frame is seen through.
pub type MainCamera = (With<Camera3d>, With<MainView>);

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LookInput>()
            .add_systems(PreUpdate, read_look_input.after(bevy::input::InputSystems))
            .add_systems(Update, (grab_cursor, fly, teleport));
    }
}

/// Mouse movement meant for looking around this frame, and wheel notches.
#[derive(Resource, Default)]
pub struct LookInput {
    pub delta: Vec2,
    pub zoom: f32,
    /// The cursor was captured when this frame started.
    pub grabbed: bool,
}

pub(crate) fn read_look_input(
    input: Option<Res<crate::viewer::ViewerInput>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    cursors: Query<&CursorOptions, With<PrimaryWindow>>,
    mut look: ResMut<LookInput>,
) {
    let grabbed = cursors
        .iter()
        .any(|cursor| cursor.grab_mode != CursorGrabMode::None);
    look.grabbed = grabbed;
    let blocked = input.is_some_and(|input| input.pointer);
    look.delta = if !blocked && (grabbed || buttons.pressed(MouseButton::Right)) {
        motion.delta
    } else {
        Vec2::ZERO
    };
    look.zoom = if blocked {
        0.0
    } else {
        match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        }
    };
}

fn grab_cursor(
    input: Option<Res<crate::viewer::ViewerInput>>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    for mut cursor in &mut cursors {
        if buttons.just_pressed(MouseButton::Left)
            && !input.as_ref().is_some_and(|input| input.pointer)
        {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        } else if keys.just_pressed(KeyCode::Escape) {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }
}

#[derive(Component, Clone, Copy, Debug)]
pub struct FlyCamera {
    /// Radians; 0 looks north (−Z).
    pub yaw: f32,
    /// Radians; negative looks down.
    pub pitch: f32,
    /// World units (≈ metres) per second.
    pub speed: f32,
}

impl FlyCamera {
    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
}

/// A named viewpoint. Coordinates use the game's axes: +X east, +Y up, +Z
/// south. Places come from `LocationMarker` entries in the game's
/// `Map/MainField/Static.smubin`; heights were checked against the terrain.
pub struct Viewpoint<'a> {
    pub name: &'a str,
    pub eye: Vec3,
    pub target: Vec3,
}

/// The viewpoints the number keys reach (1–5, in order); the tools window
/// lists them all.
pub const VIEWPOINT_KEYS: [KeyCode; 5] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
];

pub const VIEWPOINTS: [Viewpoint<'static>; 12] = [
    Viewpoint {
        name: "Whole map",
        eye: Vec3::new(0.0, 11_000.0, 7_500.0),
        target: Vec3::new(0.0, 0.0, -300.0),
    },
    Viewpoint {
        // Near where Link starts (StartPoint), looking over the central field at the castle.
        name: "Great Plateau",
        eye: Vec3::new(-1080.0, 330.0, 1820.0),
        target: Vec3::new(-254.0, 250.0, -1063.0),
    },
    Viewpoint {
        name: "Castle",
        eye: Vec3::new(-254.0, 420.0, -150.0),
        target: Vec3::new(-254.0, 260.0, -1063.0),
    },
    Viewpoint {
        // The summit (terrain reaches its 800 m maximum) seen from the south-west.
        name: "Death Mountain",
        eye: Vec3::new(1500.0, 750.0, -1500.0),
        target: Vec3::new(2475.0, 650.0, -2738.0),
    },
    Viewpoint {
        name: "Kakariko",
        eye: Vec3::new(1560.0, 470.0, 1240.0),
        target: Vec3::new(1806.0, 220.0, 985.0),
    },
    // The markers `Hateno`, `WhiteZora`, `Goron`, `Rito`, `Gerudo`,
    // `Cokiri` and `HyralBridge` at their ground height, seen from above.
    Viewpoint::over("Hateno Village", Vec3::new(3592.7, 262.5, 2121.9)),
    Viewpoint::over("Zora's Domain", Vec3::new(3271.9, 214.0, -401.5)),
    Viewpoint::over("Goron City", Vec3::new(1685.2, 490.4, -2467.5)),
    Viewpoint::over("Rito Village", Vec3::new(-3618.1, 289.2, -1807.6)),
    Viewpoint::over("Gerudo Town", Vec3::new(-3835.0, 149.4, 2915.0)),
    Viewpoint::over("Korok Forest", Vec3::new(428.0, 249.5, -2137.5)),
    Viewpoint::over("Lake Hylia", Vec3::new(-40.4, 84.7, 2508.7)),
];

impl<'a> Viewpoint<'a> {
    /// `place` seen from the south, high enough to clear what is around it.
    pub const fn over(name: &'a str, place: Vec3) -> Self {
        Viewpoint {
            name,
            eye: Vec3::new(place.x, place.y + 150.0, place.z + 260.0),
            target: place,
        }
    }

    pub fn camera(&self, speed: f32) -> (Transform, FlyCamera) {
        let fly = FlyCamera::looking_at(self.target - self.eye, speed);
        (
            Transform::from_translation(self.eye).with_rotation(fly.rotation()),
            fly,
        )
    }
}

impl FlyCamera {
    /// Yaw and pitch that look along `direction`.
    pub fn looking_at(direction: Vec3, speed: f32) -> Self {
        let d = direction.normalize_or(Vec3::NEG_Z);
        FlyCamera {
            yaw: (-d.x).atan2(-d.z),
            pitch: d.y.asin(),
            speed,
        }
    }
}

fn fly(
    input: Option<Res<crate::viewer::ViewerInput>>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    look: Res<LookInput>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
) {
    if input.is_some_and(|input| input.keyboard) {
        return;
    }
    for (mut transform, mut fly) in &mut cameras {
        fly.yaw -= look.delta.x * 0.003;
        fly.pitch = (fly.pitch - look.delta.y * 0.003).clamp(-1.55, 1.55);
        if look.zoom != 0.0 {
            fly.speed = (fly.speed * 1.25f32.powf(look.zoom)).clamp(2.0, 20_000.0);
        }

        let rotation = fly.rotation();
        let axis = |positive: KeyCode, negative: KeyCode| {
            f32::from(u8::from(keys.pressed(positive)))
                - f32::from(u8::from(keys.pressed(negative)))
        };
        let direction = rotation * Vec3::NEG_Z * axis(KeyCode::KeyW, KeyCode::KeyS)
            + rotation * Vec3::X * axis(KeyCode::KeyD, KeyCode::KeyA)
            + Vec3::Y * axis(KeyCode::KeyE, KeyCode::KeyQ);
        let boost = if keys.pressed(KeyCode::ShiftLeft) {
            5.0
        } else {
            1.0
        };

        transform.translation +=
            direction.normalize_or_zero() * fly.speed * boost * time.delta_secs();
        transform.rotation = rotation;
    }
}

/// The viewpoint whose number key was pressed this frame.
pub fn pressed_viewpoint(keys: &ButtonInput<KeyCode>) -> Option<&'static Viewpoint<'static>> {
    VIEWPOINT_KEYS
        .iter()
        .position(|key| keys.just_pressed(*key))
        .and_then(|i| VIEWPOINTS.get(i))
}

fn teleport(
    input: Option<Res<crate::viewer::ViewerInput>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
) {
    if input.is_some_and(|input| input.keyboard) {
        return;
    }
    let Some(viewpoint) = pressed_viewpoint(&keys) else {
        return;
    };
    for (mut transform, mut fly) in &mut cameras {
        (*transform, *fly) = viewpoint.camera(fly.speed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looking_at_matches_the_yaw_convention() {
        let north = FlyCamera::looking_at(Vec3::NEG_Z, 1.0);
        assert!(north.yaw.abs() < 1e-6 && north.pitch.abs() < 1e-6);
        // Positive yaw turns from north towards west (-X).
        let west = FlyCamera::looking_at(Vec3::NEG_X, 1.0);
        assert!((west.yaw - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        let down = FlyCamera::looking_at(Vec3::new(0.0, -1.0, -1.0), 1.0);
        assert!((down.pitch + std::f32::consts::FRAC_PI_4).abs() < 1e-6);
        let forward = down.rotation() * Vec3::NEG_Z;
        assert!(forward.abs_diff_eq(Vec3::new(0.0, -1.0, -1.0).normalize(), 1e-5));
    }

    #[test]
    fn editing_debug_fields_blocks_flight_and_viewpoint_hotkeys() {
        let mut app = App::new();
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs(1));
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::Digit1);
        app.insert_resource(time)
            .insert_resource(keys)
            .insert_resource(LookInput::default())
            .insert_resource(crate::viewer::ViewerInput {
                pointer: false,
                keyboard: true,
            })
            .add_systems(Update, (fly, teleport).chain());
        let entity = app
            .world_mut()
            .spawn((
                Transform::IDENTITY,
                FlyCamera::looking_at(Vec3::NEG_Z, 60.0),
            ))
            .id();
        app.world_mut().run_schedule(Update);
        assert_eq!(
            *app.world().get::<Transform>(entity).unwrap(),
            Transform::IDENTITY
        );
        app.world_mut()
            .resource_mut::<crate::viewer::ViewerInput>()
            .keyboard = false;
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Digit1);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut().run_schedule(Update);
        assert_eq!(
            app.world().get::<Transform>(entity).unwrap().translation,
            Vec3::new(0.0, 0.0, -60.0)
        );
    }

    #[test]
    fn pointer_over_debug_menu_blocks_mouse_look_and_speed_wheel() {
        let mut app = App::new();
        let mut buttons = ButtonInput::<MouseButton>::default();
        buttons.press(MouseButton::Right);
        app.insert_resource(buttons)
            .insert_resource(AccumulatedMouseMotion {
                delta: Vec2::new(10.0, 20.0),
            })
            .insert_resource(AccumulatedMouseScroll {
                unit: MouseScrollUnit::Line,
                delta: Vec2::Y,
            })
            .insert_resource(LookInput::default())
            .insert_resource(crate::viewer::ViewerInput {
                pointer: true,
                keyboard: false,
            })
            .add_systems(Update, read_look_input);
        app.world_mut().run_schedule(Update);
        let look = app.world().resource::<LookInput>();
        assert_eq!(look.delta, Vec2::ZERO);
        assert_eq!(look.zoom, 0.0);
        app.world_mut()
            .resource_mut::<crate::viewer::ViewerInput>()
            .pointer = false;
        app.world_mut().run_schedule(Update);
        let look = app.world().resource::<LookInput>();
        assert_eq!(look.delta, Vec2::new(10.0, 20.0));
        assert_eq!(look.zoom, 1.0);
    }
}
