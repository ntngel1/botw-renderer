//! The environment's cube map, drawn around the camera like the game's
//! `gsys_cube_map` (docs/research/wiiu-deferred-shading.md, "Cube map
//! gsys"; Wii U v208): the terrain and the objects whose materials ask for
//! it (render info `gsys_cube_map`), then the sky and its clouds (the
//! callback `FUN_0340b6cc`, `0x0340b6cc`), seen from where the camera
//! stands, 0.1 to 300 m out, one face a frame (KSys sets `+9 = 1`,
//! `FUN_03405f48`).
//!
//! Six cameras draw the faces into images of their own; the render world
//! copies each into the cube the frame after it was drawn, and once all
//! six have come in Bevy's filter (`GeneratedEnvironmentMapLight`) makes
//! its blurred levels from the whole set, in place of
//! the game's Gaussian chain (`FUN_03a8e7f4`: level `i` blurred with
//! `2 + (0.7·i)²` taps). The faces' size is not the game's (not found), nor
//! is the filter; see RENDER-004 in archived GAPS.md. notes The faces are lit by
//! shadowless copies of the sun and the moon ([`mirror_lights`]).
//!
//! The faces are drawn with the view's exposure, so the shared look
//! (`look.rs`, exposed like the view) lights them right; the environment
//! light's strength undoes it (`SKY_LIGHT / exposure`), so that its readers
//! get the radiance times `SKY_LIGHT`, as from the atmosphere's map.
//! Without the game's data the atmosphere's sky stays the environment
//! (`main.rs`).

use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Exposure, Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{
    AtmosphereEnvironmentMapLight, EnvironmentMapLight, GeneratedEnvironmentMapLight,
};
use bevy::pbr::generate::{RenderEnvironmentMap, extract_generated_environment_map_entities};
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::render::renderer::RenderContext;
use bevy::render::sync_world::RenderEntity;
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

use crate::camera::MainCamera;
use crate::daynight::SKY_LIGHT;

/// The render layer of what the game draws into its cube map; everything
/// else stays on layer 0 only, out of the faces' sight.
pub const CUBE_LAYER: usize = 1;
/// Side of a face in texels (the game's is set at run time and was not
/// found). The shaders read the levels counted from the smallest
/// (`CUBE_LOD_FROM_TOP` in `deferred_light.wgsl`), so the size changes only
/// the sharpest levels.
// SI-LGT-13: cube map size, Bevy GGX filter, range and clear are ours.
const FACE_SIZE: u32 = 256;
/// The faces' near and far planes (KSys, `FUN_03405f48`: `+0xc = 0.1`,
/// `+0x10 = 300.0` at `0x1046f474`). Bevy's projection has no far plane:
/// only what lies wholly beyond 300 m is left out.
const NEAR: f32 = 0.1;
// SI-LGT-13: cube map size, Bevy GGX filter, range and clear are ours.
const FAR: f32 = 300.0;

/// The layers of something drawn both in the view and in the cube map.
pub fn in_cube_map() -> RenderLayers {
    RenderLayers::from_layers(&[0, CUBE_LAYER])
}

/// A camera drawing one face of the cube map (0–5: +X, −X, +Y, −Y, +Z, −Z
/// of the cube, whose z is the world's −z, see [`face_axes`]).
#[derive(Component, Clone, Copy, Debug)]
pub struct CubeFace(pub u8);

pub struct CubeMapPlugin {
    /// Only with the game's data: without it the atmosphere's sky stays.
    pub enabled: bool,
}

impl Plugin for CubeMapPlugin {
    fn build(&self, app: &mut App) {
        if !self.enabled {
            return;
        }
        app.add_plugins((
            ExtractResourcePlugin::<CubeTargets>::default(),
            ExtractResourcePlugin::<DrawnFace>::default(),
        ))
        .init_resource::<DrawnFace>()
        .add_systems(PostStartup, setup)
        .add_systems(Update, spawn_faces.run_if(resource_added::<CubeTargets>))
        // Before the transforms settle: the shadows' cascades are laid
        // out afterwards for the cameras active then.
        .add_systems(
            PostUpdate,
            (follow_view, mirror_lights).before(TransformSystems::Propagate),
        );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<CubeFrame>()
            .add_systems(
                ExtractSchedule,
                plan_cube_frame.after(extract_generated_environment_map_entities),
            )
            .add_systems(
                Render,
                copy_faces
                    .after(RenderSystems::PrepareBindGroups)
                    .before(bevy::pbr::generate::downsampling_system),
            );
    }
}

/// The face whose camera draws this frame.
#[derive(Resource, Clone, Copy, Default, ExtractResource)]
struct DrawnFace(Option<u8>);

/// What the render world does with the cube this frame: the face to copy
/// in (drawn the frame before), and whether the set is complete, so that
/// Bevy filters the cube (it does every frame it finds the view's
/// `RenderEnvironmentMap`).
#[derive(Resource, Default)]
pub struct CubeFrame {
    copy: Option<u8>,
    pub filter: bool,
}

/// Plans [`CubeFrame`]; on the frames that do not filter, takes the
/// view's `RenderEnvironmentMap` away again, so that Bevy keeps the levels
/// it filtered last.
fn plan_cube_frame(
    drawn: Extract<Res<DrawnFace>>,
    views: Extract<Query<RenderEntity, With<GeneratedEnvironmentMapLight>>>,
    mut last: Local<Option<u8>>,
    mut copied: Local<u8>,
    mut frame: ResMut<CubeFrame>,
    mut commands: Commands,
) {
    frame.copy = last.take();
    *last = drawn.0;
    if let Some(face) = frame.copy {
        *copied |= 1 << face;
    }
    frame.filter = *copied == 0b11_1111;
    if frame.filter {
        *copied = 0;
    } else {
        for view in &views {
            commands.entity(view).remove::<RenderEnvironmentMap>();
        }
    }
}

/// The cube and the images its faces are drawn into.
#[derive(Resource, Clone, ExtractResource)]
struct CubeTargets {
    cube: Handle<Image>,
    faces: [Handle<Image>; 6],
}

/// The world direction a face looks along and its up, so that the face's
/// texels match Bevy's cube layout (`sample_cube_dir` in `utils.wgsl`, read
/// with the direction's z negated, as the shaders do).
fn face_axes(face: u8) -> (Vec3, Vec3) {
    match face {
        0 => (Vec3::X, Vec3::Y),
        1 => (Vec3::NEG_X, Vec3::Y),
        2 => (Vec3::Y, Vec3::Z),
        3 => (Vec3::NEG_Y, Vec3::NEG_Z),
        4 => (Vec3::NEG_Z, Vec3::Y),
        _ => (Vec3::Z, Vec3::Y),
    }
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    views: Query<Entity, MainCamera>,
) {
    let mut cube = Image::new_fill(
        Extent3d {
            width: FACE_SIZE,
            height: FACE_SIZE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        &[0; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::all(),
    );
    cube.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    cube.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    let cube = images.add(cube);
    let faces = std::array::from_fn(|_| {
        let mut face =
            Image::new_target_texture(FACE_SIZE, FACE_SIZE, TextureFormat::Rgba16Float, None);
        face.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        images.add(face)
    });
    for view in &views {
        commands
            .entity(view)
            .remove::<AtmosphereEnvironmentMapLight>()
            // SI-LGT-13: cube map size, Bevy GGX filter, range and clear are ours.
            .insert(GeneratedEnvironmentMapLight {
                environment_map: cube.clone(),
                intensity: SKY_LIGHT,
                ..default()
            });
    }
    commands.insert_resource(CubeTargets { cube, faces });
}

/// The face cameras, a frame after the view's: the debug tools' UI goes to
/// the first camera it sees (`bevy_egui`).
fn spawn_faces(
    mut commands: Commands,
    targets: Res<CubeTargets>,
    views: Query<&Exposure, MainCamera>,
) {
    let exposure = views.iter().next().copied().unwrap_or_default();
    for (index, target) in targets.faces.iter().enumerate() {
        commands.spawn((
            Name::new(format!("cube map face {index}")),
            CubeFace(index as u8),
            Camera3d::default(),
            Camera {
                order: -1,
                is_active: false,
                // SI-LGT-13: cube map size, Bevy GGX filter, range and clear are ours.
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            RenderTarget::Image(target.clone().into()),
            Projection::Perspective(PerspectiveProjection {
                fov: FRAC_PI_2,
                aspect_ratio: 1.0,
                near: NEAR,
                far: FAR,
                ..default()
            }),
            exposure,
            Hdr,
            Tonemapping::None,
            Msaa::Off,
            RenderLayers::layer(CUBE_LAYER),
        ));
    }
}

/// Puts the faces where the view was last frame, draws the next one this
/// frame, and keeps the faces' exposure and the environment light's
/// strength with the view's.
fn follow_view(
    mut frame: Local<u32>,
    mut drawn_face: ResMut<DrawnFace>,
    mut commands: Commands,
    mut views: Query<
        (
            &GlobalTransform,
            Option<&Exposure>,
            &mut GeneratedEnvironmentMapLight,
            Option<&mut EnvironmentMapLight>,
        ),
        MainCamera,
    >,
    mut faces: Query<
        (
            Entity,
            &CubeFace,
            &mut Camera,
            &mut Transform,
            &mut Exposure,
            Option<&mut EnvironmentMapLight>,
        ),
        Without<GeneratedEnvironmentMapLight>,
    >,
) {
    let Ok((view, exposure, mut generated, mut light)) = views.single_mut() else {
        return;
    };
    let exposure = exposure.copied().unwrap_or_default();
    let strength = SKY_LIGHT / exposure.exposure();
    generated.intensity = strength;
    if let Some(light) = light.as_mut() {
        light.intensity = strength;
    }
    let drawn = (*frame % 6) as u8;
    *frame = frame.wrapping_add(1);
    drawn_face.0 = Some(drawn);
    for (entity, face, mut camera, mut transform, mut face_exposure, face_light) in &mut faces {
        let (forward, up) = face_axes(face.0);
        *transform = Transform::from_translation(view.translation()).looking_to(forward, up);
        camera.is_active = face.0 == drawn;
        *face_exposure = exposure;
        // The faces are lit by the cube map drawn so far, like the view.
        // SI-LGT-13: cube map size, Bevy GGX filter, range and clear are ours.
        match (face_light, light.as_deref()) {
            (Some(mut face_light), Some(light)) => face_light.intensity = light.intensity,
            (None, Some(light)) => {
                commands.entity(entity).insert(light.clone());
            }
            _ => {}
        }
    }
}

/// A copy of a directional light (the sun, the moon) that lights the faces
/// only, without shadows: Bevy would lay out the light's shadow cascades for
/// every face drawn, doubling their cost (whether the game's cube map pass
/// is shadowed was not traced).
#[derive(Component)]
struct FaceLightOf(Entity);

/// Keeps a shadowless copy of each directional light on the cube map's
/// layer, with its colour, strength and direction.
// SI-LGT-13: cube map size, Bevy GGX filter, range and clear are ours.
fn mirror_lights(
    mut commands: Commands,
    lights: Query<(Entity, &DirectionalLight, &Transform), Without<FaceLightOf>>,
    mut copies: Query<(Entity, &FaceLightOf, &mut DirectionalLight, &mut Transform)>,
) {
    let mut copied = Vec::new();
    for (entity, of, mut light, mut transform) in &mut copies {
        let Ok((_, source, source_transform)) = lights.get(of.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        *light = DirectionalLight {
            shadow_maps_enabled: false,
            ..source.clone()
        };
        *transform = *source_transform;
        copied.push(of.0);
    }
    for (entity, source, transform) in &lights {
        if !copied.contains(&entity) {
            commands.spawn((
                FaceLightOf(entity),
                DirectionalLight {
                    shadow_maps_enabled: false,
                    ..source.clone()
                },
                *transform,
                RenderLayers::layer(CUBE_LAYER),
            ));
        }
    }
}

/// Copies the face drawn last frame into the cube, before Bevy filters it.
fn copy_faces(
    targets: Option<Res<CubeTargets>>,
    frame: Res<CubeFrame>,
    images: Res<RenderAssets<GpuImage>>,
    mut ctx: RenderContext,
) {
    let (Some(targets), Some(layer)) = (targets, frame.copy) else {
        return;
    };
    let Some(cube) = images.get(&targets.cube) else {
        return;
    };
    let Some(face) = images.get(&targets.faces[usize::from(layer)]) else {
        return;
    };
    let mut to = cube.texture.as_image_copy();
    to.origin.z = u32::from(layer);
    ctx.command_encoder().copy_texture_to_texture(
        face.texture.as_image_copy(),
        to,
        Extent3d {
            width: FACE_SIZE,
            height: FACE_SIZE,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bevy's cube layout (`sample_cube_dir`): the direction of a face's
    /// texel at `uv` (0..1, y down), in the cube's frame.
    fn cube_dir(face: u8, u: f32, v: f32) -> Vec3 {
        let (x, y) = (2.0 * u - 1.0, 2.0 * v - 1.0);
        match face {
            0 => Vec3::new(1.0, -y, -x),
            1 => Vec3::new(-1.0, -y, x),
            2 => Vec3::new(x, 1.0, y),
            3 => Vec3::new(x, -1.0, -y),
            4 => Vec3::new(x, -y, 1.0),
            _ => Vec3::new(-x, -y, -1.0),
        }
        .normalize()
    }

    #[test]
    fn shaders_take_the_same_sky_light() {
        let shader = include_str!("deferred_light.wgsl");
        assert!(shader.contains(&format!("const SKY_LIGHT: f32 = {SKY_LIGHT:?};")));
    }

    #[test]
    fn faces_see_what_the_cube_holds() {
        for face in 0..6 {
            let (forward, up) = face_axes(face);
            let camera = Transform::default().looking_to(forward, up);
            for (u, v) in [(0.5, 0.5), (0.1, 0.2), (0.9, 0.3), (0.25, 0.8)] {
                // The camera's ray through the image's texel (x right, y up).
                let ray =
                    camera.rotation * Vec3::new(2.0 * u - 1.0, 1.0 - 2.0 * v, -1.0).normalize();
                // The shaders sample the cube with the world direction's z negated.
                let expected = cube_dir(face, u, v) * Vec3::new(1.0, 1.0, -1.0);
                assert!(
                    ray.distance(expected) < 1e-5,
                    "face {face} at ({u}, {v}): {ray} vs {expected}"
                );
            }
        }
    }
}
