//! Characters a run places in the world (`--character <name>`): each
//! stands where it is put, facing where it is told, playing a looping clip
//! on its body and the resting face with a blink now and then. Behaviour
//! uses the game's clips and skeletons through [`crate::character`].

use bevy::prelude::*;

use crate::character::{Animator, CharacterRig, CharacterState, Characters, spawn_character};

/// The resting face and its blink (the viewer's `Player_FaceDefault.bas`
/// pair, `REST_FACES`).
const REST_FACE: &str = "Face_Default";
const BLINK: &str = "Face_Default_Blink";

/// Seconds between blinks: ours (the viewer reads the AS's loop counts).
const BLINK_EVERY: [f32; 3] = [3.1, 4.4, 2.3];

pub struct CastPlugin {
    pub cast: Vec<CastMember>,
}

/// A character to place.
#[derive(Clone, Debug)]
pub struct CastMember {
    /// The baked character (`characters/<name>.ron`).
    pub name: String,
    /// Where its feet stand.
    pub position: Vec3,
    /// Its turn about the vertical (radians; 0 faces +Z, the models'
    /// forward).
    pub yaw: f32,
    /// The clip its body loops (empty: the character's idle).
    pub clip: String,
    /// Where in its loop it starts (seconds), so a crowd does not move in
    /// step.
    pub phase: f32,
}

impl Plugin for CastPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Cast {
            waiting: self.cast.clone(),
        })
        .add_systems(
            Update,
            (spawn_cast, start_clips, blink)
                .chain()
                .after(crate::character::CharacterSystems),
        );
    }
}

/// The cast not yet spawned (their characters still loading).
#[derive(Resource)]
pub struct Cast {
    waiting: Vec<CastMember>,
}

impl Cast {
    /// Queue a character preview using the same loading path as the CLI.
    pub fn place(&mut self, member: CastMember) {
        self.waiting.push(member);
    }

    /// Some character still waits to be spawned.
    pub fn is_pending(&self) -> bool {
        !self.waiting.is_empty()
    }
}

/// The clip a freshly spawned character starts with.
#[derive(Component)]
struct StartClip(String, f32);

/// The face's blink loop.
#[derive(Component, Default)]
struct Face {
    resting: f32,
    blinks: usize,
}

fn spawn_cast(mut commands: Commands, mut cast: ResMut<Cast>, mut characters: ResMut<Characters>) {
    cast.waiting
        .retain(|member| match characters.get(&member.name) {
            CharacterState::Loading => true,
            CharacterState::Unavailable => {
                warn!(
                    "character {} is not baked (run `cargo bake --only characters`)",
                    member.name
                );
                false
            }
            CharacterState::Ready(asset) => {
                let clip = if member.clip.is_empty() {
                    &asset.idle
                } else {
                    &member.clip
                };
                let place = commands
                    .spawn((
                        Name::new(member.name.clone()),
                        Transform::from_translation(member.position)
                            .with_rotation(Quat::from_rotation_y(member.yaw)),
                        Visibility::default(),
                    ))
                    .id();
                let rig = spawn_character(&mut commands, place, &asset, Transform::IDENTITY);
                commands
                    .entity(rig)
                    .insert((StartClip(clip.clone(), member.phase), Face::default()));
                debug!(
                    "{} at ({:.1}, {:.1}, {:.1}) plays {clip}",
                    member.name, member.position.x, member.position.y, member.position.z,
                );
                false
            }
        });
}

fn start_clips(
    mut commands: Commands,
    mut rigs: Query<(Entity, &CharacterRig, &mut Animator, &StartClip)>,
) {
    for (entity, rig, mut animator, start) in &mut rigs {
        if rig.asset.looping(&start.0).is_none() {
            warn!("the character has no clip {}", start.0);
        }
        if start.1 > 0.0 {
            animator.play_frames(&rig.asset, &start.0, 0.0, start.1 * 30.0, None);
        } else {
            animator.play(&rig.asset, &start.0, 0.0);
        }
        animator.play_face(&rig.asset, REST_FACE, 0.0);
        commands.entity(entity).remove::<StartClip>();
    }
}

/// The resting face, a blink every few seconds, back to rest.
fn blink(time: Res<Time>, mut rigs: Query<(&CharacterRig, &mut Animator, &mut Face)>) {
    for (rig, mut animator, mut face) in &mut rigs {
        match animator.face() {
            Some((REST_FACE, _)) => {
                face.resting += time.delta_secs();
                if face.resting >= BLINK_EVERY[face.blinks % BLINK_EVERY.len()] {
                    animator.play_face(&rig.asset, BLINK, 0.05);
                    face.blinks += 1;
                }
            }
            Some((BLINK, true)) => {
                animator.play_face(&rig.asset, REST_FACE, 0.05);
                face.resting = 0.0;
            }
            _ => {}
        }
    }
}
