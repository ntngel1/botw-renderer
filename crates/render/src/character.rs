//! Characters drawn with the game's skinned models and skeletal animations
//! (Link; NPCs and enemies later). A character is a body model plus outfit
//! pieces: their skeletons merge by bone name into one set of joint
//! entities, each piece skins to the joints of its own bones, and clips
//! pose the joints. Clips are baked per frame on load; playback
//! interpolates between frames, mixes clips the way the game's blenders do
//! (a main clip with weighted companions kept in step) and cross-fades
//! between clips.
//!
//! Ported from the original renderer's `character.rs`: what its `load` read from
//! the dump (the spec, the folders' skinned models, the FSKA clips) comes
//! from `assets/characters/` (`asset_format::character`, `model`, `anim`);
//! the skeleton merge, the per-joint bake of the clips (root in bind pose),
//! playback, mixing, cross-fades, morphs and the face layer are as there.
//! The bone offsets the viewer's player code adds (the ears under a hood)
//! come with the character ([`CharacterAsset::offsets`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use asset_format::anim::Clip as BakedClip;
use asset_format::character::CharacterDef;
use asset_format::paths;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::character_material::{CharacterMaterial, CharacterMaterialPlugin, CharacterShared};
use crate::models::{CpuSkinnedFolder, SkinnedPart, load_skinned_folder};

/// The game's animation rate.
const FPS: f32 = asset_format::anim::FPS;

pub struct CharacterPlugin {
    /// The `assets/` folder.
    pub assets: PathBuf,
}

impl Plugin for CharacterPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(CharacterMaterialPlugin)
            .init_resource::<crate::models::DynamicEmission>()
            .insert_resource(Characters {
                assets: self.assets.clone(),
                loads: HashMap::new(),
            })
            .add_systems(
                Update,
                (finish_loads, animate).chain().in_set(CharacterSystems),
            );
    }
}

/// Loading characters and posing their joints.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct CharacterSystems;

/// The characters baked into `assets/characters/`, loaded on first use.
#[derive(Resource)]
pub struct Characters {
    assets: PathBuf,
    loads: HashMap<String, Load>,
}

enum Load {
    Loading(Task<Result<CpuCharacter, String>>),
    Ready(Arc<CharacterAsset>),
    Failed,
}

pub enum CharacterState {
    Loading,
    Ready(Arc<CharacterAsset>),
    /// Not baked, or loading failed.
    Unavailable,
}

impl Characters {
    /// Some character is still loading.
    pub fn is_loading(&self) -> bool {
        self.loads
            .values()
            .any(|load| matches!(load, Load::Loading(_)))
    }

    /// The character `name` (`characters/<name>.ron`); the first call
    /// starts loading it.
    pub fn get(&mut self, name: &str) -> CharacterState {
        match self.loads.get(name) {
            Some(Load::Ready(asset)) => CharacterState::Ready(asset.clone()),
            Some(Load::Loading(_)) => CharacterState::Loading,
            Some(Load::Failed) => CharacterState::Unavailable,
            None => {
                let assets = self.assets.clone();
                let key = name.to_owned();
                let task = AsyncComputeTaskPool::get().spawn(async move { load(&assets, &key) });
                self.loads.insert(name.to_owned(), Load::Loading(task));
                CharacterState::Loading
            }
        }
    }
}

/// A joint of the merged skeleton.
#[derive(Clone, Debug)]
pub struct Joint {
    pub name: String,
    pub parent: Option<usize>,
    pub bind: Transform,
}

pub struct CharacterAsset {
    pub joints: Vec<Joint>,
    pieces: Vec<Piece>,
    clips: HashMap<String, Arc<Clip>>,
    /// Rotations added on top of the animation to some joints (the ears
    /// under a hood), see [`JointOffsets`].
    pub offsets: Vec<(usize, Quat)>,
    pub adjustments: Vec<(usize, Vec3, Vec3, Option<Vec3>)>,
    pub rt_copies: Vec<(usize, usize)>,
    /// The clip it loops standing about (`CharacterDef::idle`; may be
    /// empty).
    pub idle: String,
}

impl CharacterAsset {
    /// Baked animation names, in stable order for the viewer's clip picker.
    pub fn clip_names(&self) -> Vec<&str> {
        let mut names: Vec<_> = self.clips.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    pub fn joint(&self, name: &str) -> Option<usize> {
        self.joints.iter().position(|j| j.name == name)
    }

    /// Whether clip `name` loops (`None`: the character lacks it).
    pub fn looping(&self, name: &str) -> Option<bool> {
        self.clips.get(name).map(|clip| clip.looping)
    }

    /// Clip `name`'s length in frames (its last frame).
    pub fn frames(&self, name: &str) -> Option<f32> {
        self.clips.get(name).map(|clip| clip.duration() * FPS)
    }

    /// Which joints are `root` or below it.
    fn below(&self, root: usize) -> Vec<bool> {
        let mut mask = vec![false; self.joints.len()];
        // Parents come before their children.
        for (i, joint) in self.joints.iter().enumerate() {
            mask[i] = i == root || joint.parent.is_some_and(|p| mask[p]);
        }
        mask
    }
}

struct Piece {
    parts: Vec<SkinnedPart>,
    /// Joint of each of the piece's bones (the mesh's joint indices).
    joints: Vec<usize>,
    inverse_bindposes: Handle<SkinnedMeshInverseBindposes>,
}

/// An animation baked to joint transforms at every frame.
pub struct Clip {
    /// `[frame][joint]`; looping clips repeat their first frame at the end.
    frames: Vec<Vec<Transform>>,
    pub looping: bool,
    /// How fast the animation itself moves the character (its root
    /// motion, e.g. forward for a run, up for a climb), in metres per
    /// second: playback speed follows real speed.
    pub root_speed: f32,
}

impl Clip {
    pub fn duration(&self) -> f32 {
        (self.frames.len().saturating_sub(1)) as f32 / FPS
    }

    // SI-ANM-07: baked whole frames (our model).
    fn sample(&self, joint: usize, time: f32) -> Transform {
        let last = self.frames.len() - 1;
        let frame = time * FPS;
        let frame = if self.looping && last > 0 {
            frame.rem_euclid(last as f32)
        } else {
            frame.clamp(0.0, last as f32)
        };
        let i = (frame.floor() as usize).min(last);
        let (a, b) = (self.frames[i][joint], self.frames[(i + 1).min(last)][joint]);
        let t = frame - i as f32;
        Transform {
            translation: a.translation.lerp(b.translation, t),
            rotation: a.rotation.slerp(b.rotation, t),
            scale: a.scale.lerp(b.scale, t),
        }
    }
}

struct CpuCharacter {
    joints: Vec<Joint>,
    folders: Vec<CpuSkinnedFolder>,
    /// (folder index, unit name, joint per bone, inverse bind poses).
    pieces: Vec<(usize, String, Vec<usize>, Vec<Mat4>)>,
    hidden: Vec<String>,
    clips: HashMap<String, Clip>,
    offsets: Vec<(usize, Quat)>,
    adjustments: Vec<(usize, Vec3, Vec3, Option<Vec3>)>,
    rt_copies: Vec<(usize, usize)>,
    idle: String,
}

fn load(assets: &Path, name: &str) -> Result<CpuCharacter, String> {
    let started = std::time::Instant::now();
    let def: CharacterDef =
        asset_format::read_ron(&assets.join(paths::character(name))).map_err(|e| e.to_string())?;
    // Each folder once, with every unit the character takes from it.
    let mut folder_names: Vec<String> = Vec::new();
    let mut folder_units: Vec<Vec<String>> = Vec::new();
    for (folder, unit) in &def.parts {
        match folder_names.iter().position(|f| f == folder) {
            Some(i) => folder_units[i].push(unit.clone()),
            None => {
                folder_names.push(folder.clone());
                folder_units.push(vec![unit.clone()]);
            }
        }
    }
    let folders = folder_names
        .iter()
        .zip(&folder_units)
        .map(|(folder, units)| {
            load_skinned_folder(&assets.join(paths::CHARACTER_MODELS).join(folder), units)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut joints: Vec<Joint> = Vec::new();
    let mut pieces = Vec::new();
    for (folder, unit) in &def.parts {
        let index = folder_names
            .iter()
            .position(|f| f == folder)
            .expect("listed above");
        let bones = folders[index]
            .skeletons
            .get(unit)
            .ok_or_else(|| format!("{folder}: no model {unit}"))?;
        // Bones the skeleton has already are shared (by name, or the joint
        // the game copies onto them: `CharacterDef::aliases`); new ones
        // (hair, cloth) join under their parent.
        let mut map = Vec::with_capacity(bones.len());
        for bone in bones {
            let name = def
                .aliases
                .iter()
                .find(|(b, _)| *b == bone.name)
                .map_or(bone.name.as_str(), |(_, joint)| joint.as_str());
            let joint = match joints.iter().position(|j| j.name == name) {
                Some(joint) => joint,
                None => {
                    joints.push(Joint {
                        name: bone.name.clone(),
                        parent: bone.parent.map(|p| map[p]),
                        bind: bone.local,
                    });
                    joints.len() - 1
                }
            };
            map.push(joint);
        }
        let inverse_binds = bones.iter().map(|b| b.inverse_bind).collect();
        pieces.push((index, unit.clone(), map, inverse_binds));
    }

    attach_joints(&mut joints, &def.joint_attachments)?;
    let rt_copies = resolve_rt_copies(&joints, &def.joint_rt_copies)?;
    let mut clips = HashMap::new();
    for clip in &def.clips {
        let path = assets.join(paths::character_clip(&def.animations, clip));
        match BakedClip::read(&path) {
            Ok(baked) => {
                clips.insert(clip.clone(), bake(&baked, &joints));
            }
            Err(error) => warn!("{}: animation {clip}: {error}", def.animations),
        }
    }
    // Native feature callers skip animation targets absent from this face model.
    let adjustments = def
        .joint_adjustments
        .iter()
        .filter_map(|a| {
            let joint = joints.iter().position(|j| j.name == a.joint)?;
            Some((
                joint,
                Vec3::from(a.translation),
                Vec3::from(a.scale),
                a.scale_override.map(Vec3::from),
            ))
        })
        .collect();
    let offsets = def
        .joint_offsets
        .iter()
        .filter_map(|(joint, q)| {
            let index = joints.iter().position(|j| j.name == *joint)?;
            Some((index, Quat::from_array(*q)))
        })
        .collect();
    info!(
        "character {name} ready in {:.2} s ({} joints, {} clips)",
        started.elapsed().as_secs_f32(),
        joints.len(),
        clips.len()
    );
    Ok(CpuCharacter {
        joints,
        folders,
        pieces,
        hidden: def.hidden,
        clips,
        offsets,
        adjustments,
        rt_copies,
        idle: def.idle,
    })
}

/// The body precedes its face, so a copied source is already posed this frame.
fn resolve_rt_copies(
    joints: &[Joint],
    copies: &[(String, String)],
) -> Result<Vec<(usize, usize)>, String> {
    let mut resolved = Vec::new();
    for (destination, source) in copies {
        let index = |name: &str| joints.iter().position(|j| j.name == name);
        let destination_index = index(destination)
            .ok_or_else(|| format!("RT copy destination {destination} missing"))?;
        let source_index =
            index(source).ok_or_else(|| format!("RT copy source {source} missing"))?;
        if source_index >= destination_index {
            return Err(format!(
                "RT copy {source} -> {destination} requires source first"
            ));
        }
        if resolved.iter().any(|(d, _)| *d == destination_index) {
            return Err(format!("duplicate RT copy destination {destination}"));
        }
        resolved.push((destination_index, source_index));
    }
    Ok(resolved)
}

/// Native 0x0343b104..1c copies local RT using the +0x54 setter, not scale.
fn copy_local_rt(mut destination: Transform, source: Transform) -> Transform {
    destination.translation = source.translation;
    destination.rotation = source.rotation;
    destination
}

fn attach_joints(joints: &mut [Joint], attachments: &[(String, String)]) -> Result<(), String> {
    for (root, parent) in attachments {
        let root_index = joints
            .iter()
            .position(|j| j.name == *root)
            .ok_or_else(|| format!("attachment root {root} missing"))?;
        let parent_index = joints
            .iter()
            .position(|j| j.name == *parent)
            .ok_or_else(|| format!("attachment parent {parent} missing"))?;
        // Joint traversal and spawning require parents before children.
        if parent_index >= root_index {
            return Err(format!(
                "attachment {root} -> {parent} is not topologically ordered"
            ));
        }
        if joints[root_index].parent.is_some() {
            return Err(format!("attachment {root} is not a model root"));
        }
        joints[root_index].parent = Some(parent_index);
    }
    Ok(())
}

/// A baked clip as transforms of every joint at every frame; joints it
/// does not animate keep their bind pose. The root's motion (moving and
/// turning) is dropped: whoever moves the character moves and turns it
/// instead (the viewer's `bake`, over the clip's tracks).
fn bake(clip: &BakedClip, joints: &[Joint]) -> Clip {
    let tracks: Vec<_> = joints
        .iter()
        .map(|j| clip.tracks.iter().find(|t| t.bone == j.name))
        .collect();
    let frames = (0..clip.samples())
        .map(|frame| {
            joints
                .iter()
                .zip(&tracks)
                .map(|(joint, track)| {
                    let Some(track) = track else {
                        return joint.bind;
                    };
                    // SI-ANM-07: root in bind pose (our model).
                    let rotation = match (joint.parent, track.rotation.get(frame)) {
                        (Some(_), Some(&q)) => Quat::from_array(q),
                        _ => joint.bind.rotation,
                    };
                    let translation = match (joint.parent, track.translation.get(frame)) {
                        (Some(_), Some(&t)) => Vec3::from(t),
                        _ => joint.bind.translation,
                    };
                    Transform {
                        translation,
                        rotation,
                        scale: track
                            .scale
                            .get(frame)
                            .map_or(joint.bind.scale, |&s| s.into()),
                    }
                })
                .collect()
        })
        .collect();
    // SI-ANM-05: clip speed from root chord speed, not the AS Rate.
    let root = joints
        .first()
        .and_then(|root| clip.tracks.iter().find(|t| t.bone == root.name));
    let root_speed = match root {
        Some(root) if clip.frame_count > 0 => {
            let at =
                |frame: usize| Vec3::from(root.translation.get(frame).copied().unwrap_or_default());
            at(clip.frame_count as usize).distance(at(0)) / (clip.frame_count as f32 / FPS)
        }
        _ => 0.0,
    };
    Clip {
        frames,
        looping: clip.looping,
        root_speed,
    }
}

fn finish_loads(
    mut characters: ResMut<Characters>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
    mut faces: ResMut<Assets<crate::face_material::FaceMaterial>>,
    mut bindposes: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    shared: CharacterShared,
    mut dynamic: ResMut<crate::models::DynamicEmission>,
) {
    let shading = shared.template();
    for (key, load) in characters.loads.iter_mut() {
        let Load::Loading(task) = load else { continue };
        let Some(result) = block_on(poll_once(task)) else {
            continue;
        };
        *load = match result {
            Ok(cpu) => {
                let units: Vec<_> = cpu
                    .folders
                    .into_iter()
                    .map(|f| {
                        f.upload(
                            &mut meshes,
                            &mut images,
                            &mut materials,
                            &mut faces,
                            &shading,
                            &mut dynamic,
                        )
                    })
                    .collect();
                let pieces = cpu
                    .pieces
                    .into_iter()
                    .map(|(folder, unit, joints, inverse_binds)| Piece {
                        parts: units[folder]
                            .get(&unit)
                            .map(|parts| {
                                parts
                                    .iter()
                                    .filter(|p| !cpu.hidden.iter().any(|h| **h == *p.shape))
                                    .cloned()
                                    .collect()
                            })
                            .unwrap_or_default(),
                        joints,
                        inverse_bindposes: bindposes
                            .add(SkinnedMeshInverseBindposes::from(inverse_binds)),
                    })
                    .collect();
                let clips = cpu
                    .clips
                    .into_iter()
                    .map(|(name, clip)| (name, Arc::new(clip)))
                    .collect();
                Load::Ready(Arc::new(CharacterAsset {
                    joints: cpu.joints,
                    pieces,
                    clips,
                    offsets: cpu.offsets,
                    adjustments: cpu.adjustments,
                    rt_copies: cpu.rt_copies,
                    idle: cpu.idle,
                }))
            }
            Err(error) => {
                warn!("character {key}: {error}");
                Load::Failed
            }
        };
    }
}

/// Rotations added on top of the animation to some joints (e.g. ears bent
/// back under a hood): `(joint, rotation)`.
#[derive(Component, Clone, Debug, Default)]
pub struct JointOffsets(pub Vec<(usize, Quat)>);

#[derive(Component)]
pub struct JointAdjustments(pub Vec<(usize, Vec3, Vec3, Option<Vec3>)>);

fn adjust_pose(
    mut pose: Transform,
    translation: Vec3,
    scale: Vec3,
    scale_override: Option<Vec3>,
) -> Transform {
    pose.translation += translation;
    pose.scale = scale_override.unwrap_or(pose.scale) + scale;
    pose
}

/// A spawned character's joint entities.
#[derive(Component)]
pub struct CharacterRig {
    pub joints: Vec<Entity>,
    pub asset: Arc<CharacterAsset>,
}

/// Plays a character's clips.
#[derive(Component, Default)]
pub struct Animator {
    current: Option<Playing>,
    /// The clip being faded out and how far the fade is (0 → 1).
    previous: Option<(Playing, f32)>,
    fade_time: f32,
    /// Playback rate of the current clip.
    pub speed: f32,
    /// A clip for the face over whatever the body plays (the game's face
    /// animations move the bones under `Face_Root`: eyelids, brows, lips).
    face: Option<FaceLayer>,
    /// A morph in progress from the pose put out before ([`Self::morph`]).
    morph: Option<Morph>,
    /// The body's pose as last put out, by joint: what a morph starts from.
    output: Vec<Transform>,
}

/// The game's morph into a new AS (`0x0370be98` starts it, `0x0370bf1c`
/// steps it, `0x03160aac` poses the bones; the actor's frame steps each
/// slot through `0x03711140` and `0x03718544` by the game's dt × the AS's
/// rate × RateAll, which is 1 for Link): `t` goes from 0 to 1 over
/// `frames` game frames at the AS's rate, eased to `e` = 2t² below ½ and
/// 1 − 2(1 − t)² above; each frame the pose put out moves from the last
/// one towards what plays by `share` = 1 − (1 − e) / (1 − e before), so
/// the pose Link had when it started weighs 1 − e.
struct Morph {
    t: f32,
    eased: f32,
    frames: f32,
    rate: f32,
    share: f32,
}

impl Morph {
    /// One step of `frames` game frames; `false` once the morph is over.
    fn step(&mut self, frames: f32) -> bool {
        self.t += frames * self.rate / self.frames;
        if self.t >= 1.0 {
            return false;
        }
        let before = self.eased;
        self.eased = if self.t < 0.5 {
            2.0 * self.t * self.t
        } else {
            1.0 - 2.0 * (1.0 - self.t) * (1.0 - self.t)
        };
        self.share = 1.0 - (1.0 - self.eased) / (1.0 - before);
        true
    }
}

struct FaceLayer {
    posed: bool,
    name: String,
    clip: Arc<Clip>,
    time: f32,
    /// The joints it poses.
    joints: Arc<[bool]>,
    /// The face clip before, faded out over `fade_time`.
    previous: Option<(Arc<Clip>, f32)>,
    fade: f32,
    fade_time: f32,
}

#[derive(Clone)]
struct Playing {
    name: String,
    clip: Arc<Clip>,
    time: f32,
    /// Where a one-off clip starts and stops (seconds; `None`: its end) when
    /// only part of it plays; it holds the end pose after.
    start: f32,
    end: Option<f32>,
    /// Clips mixed in, kept at the main clip's phase, with their weights;
    /// the main clip weighs what is left of 1.
    mix: Vec<(String, Arc<Clip>, f32)>,
    /// Set from outside ([`Animator::pose`]): the times of the main clip
    /// and of each mixed-in one (seconds, in `mix`'s order), which playback
    /// does not move on.
    posed: Option<Vec<f32>>,
}

impl Playing {
    fn new(name: &str, clip: Arc<Clip>, time: f32) -> Self {
        Self {
            name: name.to_owned(),
            clip,
            time,
            start: 0.0,
            end: None,
            mix: Vec::new(),
            posed: None,
        }
    }

    /// When the part of the clip that plays ends.
    fn end(&self) -> f32 {
        self.end
            .map_or(self.clip.duration(), |end| end.min(self.clip.duration()))
    }

    /// Where in the clip the pose is: one-off clips hold their end.
    fn clip_time(&self) -> f32 {
        if self.clip.looping {
            self.time
        } else {
            self.time.min(self.end())
        }
    }

    /// Where a mixed-in clip is: at the same phase of its own loop, or the
    /// same time for one-off clips.
    // SI-ANM-07: blend children in the lead clip's phase (our model).
    fn time_in(&self, other: &Clip) -> f32 {
        let duration = self.clip.duration();
        if self.clip.looping && other.looping && duration > 0.0 {
            (self.time / duration).rem_euclid(1.0) * other.duration()
        } else {
            self.time
        }
    }

    fn sample(&self, joint: usize) -> Transform {
        let main = 1.0 - self.mix.iter().map(|(_, _, w)| w).sum::<f32>();
        let mut pose = self.clip.sample(joint, self.clip_time());
        let mut total = main.max(0.0);
        for (i, (_, clip, weight)) in self.mix.iter().enumerate() {
            let time = match &self.posed {
                Some(times) => times.get(i).copied().unwrap_or(0.0),
                None => self.time_in(clip),
            };
            let other = clip.sample(joint, time);
            total += weight;
            if total <= 1e-6 {
                continue;
            }
            pose = mix(pose, other, weight / total);
        }
        pose
    }
}

fn mix(from: Transform, to: Transform, w: f32) -> Transform {
    Transform {
        translation: from.translation.lerp(to.translation, w),
        rotation: from.rotation.slerp(to.rotation, w),
        scale: from.scale.lerp(to.scale, w),
    }
}

impl Animator {
    /// Switches to clip `name`, cross-fading over `fade` seconds. Looping
    /// clips keep their phase, so feet stay in step between walk and run; a
    /// loop following a one-off clip (a start, a landing) starts where its
    /// pose is closest to the one-off's current pose.
    pub fn play(&mut self, asset: &CharacterAsset, name: &str, fade: f32) {
        if self.current.as_ref().is_some_and(|c| c.name == name) {
            return;
        }
        let Some(clip) = asset.clips.get(name) else {
            return;
        };
        let time = match &self.current {
            Some(c) if c.clip.looping && clip.looping && c.clip.duration() > 0.0 => {
                (c.time / c.clip.duration()).fract() * clip.duration()
            }
            Some(c) if clip.looping => closest_pose(c, clip),
            _ => 0.0,
        };
        self.previous = self.current.take().map(|c| (c, 0.0));
        self.fade_time = fade.max(1e-3);
        self.current = Some(Playing::new(name, clip.clone(), time));
    }

    /// Plays clip `name` from its start even if it is already playing.
    pub fn restart(&mut self, asset: &CharacterAsset, name: &str, fade: f32) {
        self.play_frames(asset, name, fade, 0.0, None);
    }

    /// Plays frames `from` to `to` of one-off clip `name` (`None`: to its
    /// end) even if it is already playing, then holds frame `to`: the part
    /// of a clip an AS plays (`FrameCtrl` `StartFrame`, `EndFrame`).
    /// [`Self::progress`] and [`Self::finished`] follow that part. A loop
    /// starts its first pass at `from` and wraps to its start (`to` is
    /// ignored). A negative `from` starts at the clip's end, as the game
    /// reads a negative `StartFrame` (`0x03860d08`).
    pub fn play_frames(
        &mut self,
        asset: &CharacterAsset,
        name: &str,
        fade: f32,
        from: f32,
        to: Option<f32>,
    ) {
        let Some(clip) = asset.clips.get(name) else {
            return;
        };
        let start = if from < 0.0 {
            clip.duration()
        } else {
            (from / FPS).min(clip.duration())
        };
        self.previous = self.current.take().map(|c| (c, 0.0));
        self.fade_time = fade.max(1e-3);
        self.current = Some(Playing {
            start,
            end: to.filter(|_| !clip.looping).map(|to| (to / FPS).max(start)),
            ..Playing::new(name, clip.clone(), start)
        });
    }

    /// Mixes clips into the current one: `(clip, weight)` with weights
    /// summing to at most 1; the current clip weighs the rest (listing it
    /// sets its own share). Clips the character lacks are left out.
    pub fn blend(&mut self, asset: &CharacterAsset, weights: &[(&str, f32)]) {
        let Some(current) = &mut self.current else {
            return;
        };
        current.posed = None;
        let known: Vec<(&str, f32)> = weights
            .iter()
            .copied()
            .filter(|(name, w)| {
                *w > 1e-4 && (*name == current.name || asset.clips.contains_key(*name))
            })
            .collect();
        let total: f32 = known.iter().map(|(_, w)| w).sum();
        if total <= 1e-4 {
            current.mix.clear();
            return;
        }
        current.mix = known
            .iter()
            .filter(|(name, _)| *name != current.name)
            .map(|(name, w)| (name.to_string(), asset.clips[*name].clone(), w / total))
            .collect();
    }

    /// Poses clips whose frames the caller keeps (AS `Move`'s loop, which
    /// the movement code steps): `(clip, frame, weight)`, the heaviest as
    /// the current clip and the others mixed in at their own frames;
    /// playback does not move them on. A clip fading out fades on. Clips
    /// the character lacks are left out.
    pub fn pose(&mut self, asset: &CharacterAsset, clips: &[(&str, f32, f32)]) {
        let known: Vec<(&str, f32, f32)> = clips
            .iter()
            .copied()
            .filter(|(name, _, w)| *w > 1e-4 && asset.clips.contains_key(*name))
            .collect();
        let total: f32 = known.iter().map(|(_, _, w)| w).sum();
        let Some((main, &(name, frame, _))) = known
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.2.total_cmp(&b.1.2))
        else {
            return;
        };
        // A clip may play twice (on two branches of a tree), each at its
        // own frame and weight.
        let others: Vec<&(&str, f32, f32)> = known
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != main)
            .map(|(_, c)| c)
            .collect();
        self.current = Some(Playing {
            mix: others
                .iter()
                .map(|(n, _, w)| (n.to_string(), asset.clips[*n].clone(), w / total))
                .collect(),
            posed: Some(others.iter().map(|(_, f, _)| f / FPS).collect()),
            ..Playing::new(name, asset.clips[name].clone(), frame / FPS)
        });
    }

    /// Morphs from the pose last put out into what plays, over `frames`
    /// game frames stepped at `rate` (the AS's): the game's blend when a
    /// new AS starts without a cross-fade between the two. Under 0.01
    /// frames it cancels a morph in progress (`0x0370be98`). The cross-fade
    /// from the clip before is dropped: the new AS alone plays.
    pub fn morph(&mut self, frames: f32, rate: f32) {
        if frames < 0.01 || self.output.is_empty() {
            self.morph = None;
            return;
        }
        self.previous = None;
        self.morph = Some(Morph {
            t: 0.0,
            eased: 0.0,
            frames,
            rate,
            share: 0.0,
        });
    }

    /// Starts the cross-fade [`Self::play`] or [`Self::play_frames`] just
    /// began `share` (0 → 1) of the way through, the rest over what is
    /// left of its time.
    pub fn fade_from(&mut self, share: f32) {
        if let Some((_, fade)) = &mut self.previous {
            *fade = share.clamp(0.0, 1.0);
        }
    }

    /// The current clip's frame (a one-off clip holds its end).
    pub fn frame(&self) -> Option<f32> {
        self.current.as_ref().map(|c| c.clip_time() * FPS)
    }

    /// The clips mixed into the current one and their weights.
    pub fn mixed(&self) -> Vec<(&str, f32)> {
        self.current.as_ref().map_or_else(Vec::new, |c| {
            c.mix.iter().map(|(n, _, w)| (n.as_str(), *w)).collect()
        })
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_ref().map(|c| c.name.as_str())
    }

    /// The current clip's own speed, see [`Clip::root_speed`]: of the mix,
    /// by weight.
    pub fn root_speed(&self) -> f32 {
        self.current.as_ref().map_or(0.0, |c| {
            let main = 1.0 - c.mix.iter().map(|(_, _, w)| w).sum::<f32>();
            c.clip.root_speed * main.max(0.0)
                + c.mix
                    .iter()
                    .map(|(_, clip, w)| clip.root_speed * w)
                    .sum::<f32>()
        })
    }

    /// How far through the current clip playback is (0 → 1; loops wrap;
    /// one-off clips over the part that plays).
    pub fn progress(&self) -> f32 {
        self.current.as_ref().map_or(0.0, |c| {
            if c.clip.looping {
                (c.time / c.clip.duration().max(1e-3)).fract()
            } else {
                ((c.time - c.start) / (c.end() - c.start).max(1e-3)).clamp(0.0, 1.0)
            }
        })
    }

    /// Plays clip `name` on the face (from its start), fading in over
    /// `fade` seconds; needs a `Face_Root` joint.
    pub fn play_face(&mut self, asset: &CharacterAsset, name: &str, fade: f32) {
        let Some(clip) = asset.clips.get(name) else {
            return;
        };
        let joints = match &self.face {
            Some(face) => face.joints.clone(),
            None => match asset.joint("Face_Root") {
                Some(root) => asset.below(root).into(),
                None => return,
            },
        };
        let previous = self.face.take().map(|f| (f.clip.clone(), f.time));
        self.face = Some(FaceLayer {
            posed: false,
            name: name.to_owned(),
            clip: clip.clone(),
            time: 0.0,
            joints,
            previous,
            fade: 0.0,
            fade_time: fade.max(1e-3),
        });
    }

    /// Absolute facial cue/crossfade; never advances with preview/readback dt.
    pub fn pose_face(&mut self, asset: &CharacterAsset, clips: &[(&str, f32, f32)]) {
        let Some(&(name, frame, weight)) = clips.last() else {
            return;
        };
        let Some(clip) = asset.clips.get(name) else {
            return;
        };
        let Some(root) = asset.joint("Face_Root") else {
            return;
        };
        let joints = self
            .face
            .as_ref()
            .map_or_else(|| asset.below(root).into(), |f| f.joints.clone());
        let previous = clips
            .first()
            .filter(|_| clips.len() > 1)
            .and_then(|&(name, frame, _)| {
                asset
                    .clips
                    .get(name)
                    .map(|clip| (clip.clone(), frame / FPS))
            });
        self.face = Some(FaceLayer {
            posed: true,
            name: name.into(),
            clip: clip.clone(),
            time: frame / FPS,
            joints,
            previous,
            fade: weight,
            fade_time: 1.0,
        });
    }

    /// The face clip playing and whether a one-off one has ended.
    pub fn face(&self) -> Option<(&str, bool)> {
        self.face.as_ref().map(|f| {
            (
                f.name.as_str(),
                !f.clip.looping && f.time >= f.clip.duration(),
            )
        })
    }

    /// Whether a non-looping clip has played to its end (or the end of the
    /// part that plays).
    pub fn finished(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|c| !c.clip.looping && c.time >= c.end())
    }
}

/// The time in `to` whose pose (joint rotations) is closest to `from`'s now.
// SI-ANM-06: enemies start loops at the closest pose; Connect not applied.
fn closest_pose(from: &Playing, to: &Clip) -> f32 {
    let joints = from.clip.frames.first().map_or(0, Vec::len);
    let pose: Vec<Quat> = (0..joints)
        .map(|j| from.clip.sample(j, from.clip_time()).rotation)
        .collect();
    let distance = |frame: &Vec<Transform>| -> f32 {
        pose.iter()
            .zip(frame)
            .map(|(a, b)| 1.0 - a.dot(b.rotation).abs())
            .sum()
    };
    let best = to
        .frames
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| distance(a).total_cmp(&distance(b)));
    best.map_or(0.0, |(frame, _)| frame as f32 / FPS)
}

/// Spawns a character under `parent`; returns the character's root entity.
pub fn spawn_character(
    commands: &mut Commands,
    parent: Entity,
    asset: &Arc<CharacterAsset>,
    transform: Transform,
) -> Entity {
    let root = commands
        .spawn((
            Name::new("character"),
            transform,
            Visibility::default(),
            ChildOf(parent),
        ))
        .id();
    let mut joints: Vec<Entity> = Vec::with_capacity(asset.joints.len());
    let mut poses = Vec::with_capacity(asset.joints.len());
    for (i, joint) in asset.joints.iter().enumerate() {
        let parent = joint.parent.map_or(root, |p| joints[p]);
        let mut pose = joint.bind;
        if let Some((_, source)) = asset.rt_copies.iter().find(|(d, _)| *d == i) {
            pose = copy_local_rt(pose, poses[*source]);
        }
        if let Some((_, translation, scale, scale_override)) =
            asset.adjustments.iter().find(|(j, _, _, _)| *j == i)
        {
            pose = adjust_pose(pose, *translation, *scale, *scale_override);
        }
        poses.push(pose);
        // Visible so things held by a joint (a sword in a hand) inherit visibility.
        joints.push(
            commands
                .spawn((
                    Name::new(joint.name.clone()),
                    pose,
                    Visibility::default(),
                    ChildOf(parent),
                ))
                .id(),
        );
    }
    for piece in &asset.pieces {
        let piece_joints: Vec<Entity> = piece.joints.iter().map(|&j| joints[j]).collect();
        for part in &piece.parts {
            let mut entity = commands.spawn((
                Name::new(part.shape.to_string()),
                Mesh3d(part.mesh.clone()),
                SkinnedMesh {
                    inverse_bindposes: piece.inverse_bindposes.clone(),
                    joints: piece_joints.clone(),
                },
                // Bounds of a skinned mesh move with its joints.
                NoFrustumCulling,
                Transform::default(),
                ChildOf(root),
            ));
            part.material.insert(&mut entity);
        }
    }
    commands.entity(root).insert((
        CharacterRig {
            joints,
            asset: asset.clone(),
        },
        Animator {
            speed: 1.0,
            ..default()
        },
        JointOffsets(asset.offsets.clone()),
        JointAdjustments(asset.adjustments.clone()),
    ));
    root
}

fn animate(
    time: Res<Time>,
    mut rigs: Query<(
        &CharacterRig,
        &mut Animator,
        Option<&JointOffsets>,
        Option<&JointAdjustments>,
    )>,
    mut transforms: Query<&mut Transform>,
) {
    let dt = time.delta_secs();
    for (rig, mut animator, offsets, adjustments) in &mut rigs {
        let animator = &mut *animator;
        let speed = animator.speed;
        let Some(current) = &mut animator.current else {
            continue;
        };
        if current.posed.is_none() {
            current.time += dt * speed;
        }
        // SI-ANM-07: linear cross-fade at the new clip's speed (our model).
        if let Some((previous, fade)) = &mut animator.previous {
            // A posed clip fades out as it was left.
            if previous.posed.is_none() {
                previous.time += dt * speed;
            }
            *fade += dt / animator.fade_time;
        }
        if animator
            .previous
            .as_ref()
            .is_some_and(|(_, fade)| *fade >= 1.0)
        {
            animator.previous = None;
        }
        if let Some(face) = animator.face.as_mut().filter(|face| !face.posed) {
            face.time += dt;
            face.fade += dt / face.fade_time;
            if face.fade >= 1.0 {
                face.previous = None;
            }
            if let Some((_, time)) = &mut face.previous {
                *time += dt;
            }
        }
        if animator
            .morph
            .as_mut()
            .is_some_and(|morph| !morph.step(dt * FPS))
        {
            animator.morph = None;
        }
        let morph = animator.morph.as_ref().map(|m| m.share);
        animator
            .output
            .resize(rig.joints.len(), Transform::IDENTITY);
        let face = animator.face.as_ref();
        for (joint, &entity) in rig.joints.iter().enumerate() {
            let mut pose = current.sample(joint);
            if let Some((previous, fade)) = &animator.previous {
                pose = mix(previous.sample(joint), pose, fade.clamp(0.0, 1.0));
            }
            // SI-ANM-15: morph blends every bone, root included.
            if let Some(share) = morph {
                pose = mix(animator.output[joint], pose, share);
            }
            animator.output[joint] = pose;
            if let Some(face) = face.filter(|f| f.joints.get(joint).copied().unwrap_or(false)) {
                pose = face.clip.sample(joint, face.time);
                if let Some((clip, time)) = &face.previous {
                    pose = mix(clip.sample(joint, *time), pose, face.fade.clamp(0.0, 1.0));
                }
            }
            if let Some((_, source)) = rig.asset.rt_copies.iter().find(|(d, _)| *d == joint) {
                if let Ok(source_pose) = transforms.get(rig.joints[*source]) {
                    pose = copy_local_rt(pose, *source_pose);
                }
            }
            if let Some((_, translation, scale, scale_override)) =
                adjustments.and_then(|a| a.0.iter().find(|(j, _, _, _)| *j == joint))
            {
                pose = adjust_pose(pose, *translation, *scale, *scale_override);
            }
            // SI-EQP-05: EarRotate read as Euler XYZ degrees.
            if let Some((_, offset)) = offsets.and_then(|o| o.0.iter().find(|(j, _)| *j == joint)) {
                pose.rotation *= *offset;
            }
            if let Ok(mut transform) = transforms.get_mut(entity) {
                *transform = pose;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_head_rt_copy_preserves_face_scale_at_spawn_and_during_animation() {
        let source = Transform {
            translation: Vec3::new(0.16, 0.02, 0.0),
            rotation: Quat::from_rotation_z(0.4),
            scale: Vec3::splat(1.4),
        };
        let destination = Transform::from_scale(Vec3::new(0.93, 0.95, 0.97));
        let joints = vec![
            Joint {
                name: "Head".into(),
                parent: None,
                bind: source,
            },
            Joint {
                name: "Head_Controled".into(),
                parent: None,
                bind: destination,
            },
        ];
        let names = vec![("Head_Controled".into(), "Head".into())];
        let copies = resolve_rt_copies(&joints, &names).unwrap();
        assert!(resolve_rt_copies(&joints, &[("Head".into(), "Head_Controled".into())]).is_err());
        assert!(resolve_rt_copies(&joints, &[("Absent".into(), "Head".into())]).is_err());
        assert!(resolve_rt_copies(&joints, &[names[0].clone(), names[0].clone()]).is_err());
        let asset = Arc::new(CharacterAsset {
            joints,
            pieces: Vec::new(),
            clips: HashMap::from([(
                "idle".into(),
                Arc::new(Clip {
                    frames: vec![vec![source, destination]; 2],
                    looping: true,
                    root_speed: 0.0,
                }),
            )]),
            offsets: Vec::new(),
            adjustments: vec![(1, Vec3::Y * 0.01, Vec3::ZERO, None)],
            rt_copies: copies,
            idle: String::new(),
        });
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_systems(Update, animate);
        let parent = app.world_mut().spawn(Transform::IDENTITY).id();
        let root = spawn_character(
            &mut app.world_mut().commands(),
            parent,
            &asset,
            Transform::IDENTITY,
        );
        app.world_mut().flush();
        let entities = app
            .world()
            .get::<CharacterRig>(root)
            .unwrap()
            .joints
            .clone();
        let assert_pose = |world: &World| {
            let head = world.get::<Transform>(entities[1]).unwrap();
            assert!(
                head.translation
                    .abs_diff_eq(source.translation + Vec3::Y * 0.01, 1e-6)
            );
            assert!(head.rotation.abs_diff_eq(source.rotation, 1e-6));
            assert!(head.scale.abs_diff_eq(destination.scale, 1e-6));
            assert!(
                world
                    .get::<Transform>(entities[0])
                    .unwrap()
                    .scale
                    .abs_diff_eq(source.scale, 1e-6)
            );
        };
        assert_pose(app.world());
        app.world_mut()
            .get_mut::<Animator>(root)
            .unwrap()
            .play(&asset, "idle", 0.0);
        for _ in 0..3 {
            app.update();
            assert_pose(app.world());
        }
    }

    #[test]
    fn absolute_body_scale_replaces_animation_scale_before_features() {
        // Native body application replaces scale, unlike face feature deltas.
        let authored_scale = Vec3::new(0.8, 1.15, 0.9);
        let feature_scale = Vec3::new(0.0, 0.1, 0.0);
        for animation_scale in [Vec3::ONE, Vec3::splat(2.0)] {
            let pose = Transform {
                translation: Vec3::new(1.0, 2.0, 3.0),
                rotation: Quat::from_rotation_y(0.4),
                scale: animation_scale,
            };
            let adjusted = adjust_pose(pose, Vec3::Y, feature_scale, Some(authored_scale));
            assert_eq!(adjusted.scale, authored_scale + feature_scale);
            assert_eq!(adjusted.translation, Vec3::new(1.0, 3.0, 3.0));
            assert_eq!(adjusted.rotation, pose.rotation);
        }
    }

    #[test]
    fn a_separate_model_root_keeps_its_bind_when_attached() {
        let bind = Transform::from_xyz(0.0, 0.0, 0.2);
        let mut joints = vec![
            Joint {
                name: "Nose_U".into(),
                parent: None,
                bind: Transform::IDENTITY,
            },
            Joint {
                name: "Nose_Root".into(),
                parent: None,
                bind,
            },
        ];
        attach_joints(&mut joints, &[("Nose_Root".into(), "Nose_U".into())]).unwrap();
        assert_eq!(joints[1].parent, Some(0));
        assert_eq!(joints[1].bind, bind);
        assert!(attach_joints(&mut joints, &[("Nose_U".into(), "Nose_Root".into())]).is_err());
        assert!(attach_joints(&mut joints, &[("Nose_Root".into(), "Nose_U".into())]).is_err());
    }

    fn clip(looping: bool) -> Clip {
        let at = |x: f32| vec![Transform::from_xyz(x, 0.0, 0.0)];
        Clip {
            frames: vec![at(0.0), at(3.0), at(0.0)],
            looping,
            root_speed: 0.0,
        }
    }

    #[test]
    fn samples_between_frames_and_wraps_loops() {
        let looping = clip(true);
        assert!((looping.duration() - 2.0 / FPS).abs() < 1e-6);
        assert!((looping.sample(0, 0.5 / FPS).translation.x - 1.5).abs() < 1e-5);
        // One and a half loops in: halfway from frame 1 back to frame 0.
        assert!((looping.sample(0, 3.5 / FPS).translation.x - 1.5).abs() < 1e-5);
        let once = clip(false);
        assert_eq!(once.sample(0, 10.0).translation.x, 0.0, "clamps at the end");
    }

    #[test]
    fn keeps_phase_when_switching_loops() {
        let asset = CharacterAsset {
            joints: vec![Joint {
                name: "Root".into(),
                parent: None,
                bind: Transform::IDENTITY,
            }],
            pieces: Vec::new(),
            offsets: Vec::new(),
            adjustments: Vec::new(),
            rt_copies: Vec::new(),
            idle: String::new(),
            clips: HashMap::from([
                ("walk".to_owned(), Arc::new(clip(true))),
                ("run".to_owned(), Arc::new(clip(true))),
            ]),
        };
        let mut animator = Animator {
            speed: 1.0,
            ..default()
        };
        animator.play(&asset, "walk", 0.2);
        animator.current.as_mut().unwrap().time = 1.0 / FPS; // Half-way.
        animator.play(&asset, "run", 0.2);
        assert_eq!(animator.current(), Some("run"));
        assert!((animator.current.as_ref().unwrap().time - 1.0 / FPS).abs() < 1e-6);
        assert!(animator.previous.is_some());
    }

    #[test]
    fn mixes_clips_in_step() {
        let at = |x: f32| vec![Transform::from_xyz(x, 0.0, 0.0)];
        // A loop twice as long as the main one, mixed in at its own phase.
        let long = Clip {
            frames: vec![at(10.0), at(10.0), at(16.0), at(10.0), at(10.0)],
            looping: true,
            root_speed: 4.0,
        };
        let asset = CharacterAsset {
            joints: vec![Joint {
                name: "Root".into(),
                parent: None,
                bind: Transform::IDENTITY,
            }],
            pieces: Vec::new(),
            offsets: Vec::new(),
            adjustments: Vec::new(),
            rt_copies: Vec::new(),
            idle: String::new(),
            clips: HashMap::from([
                (
                    "run".to_owned(),
                    Arc::new(Clip {
                        root_speed: 2.0,
                        ..clip(true)
                    }),
                ),
                ("lean".to_owned(), Arc::new(long)),
            ]),
        };
        let mut animator = Animator {
            speed: 1.0,
            ..default()
        };
        animator.play(&asset, "run", 0.0);
        animator.blend(&asset, &[("lean", 0.25), ("missing", 0.5)]);
        assert_eq!(
            animator.mixed(),
            vec![("lean", 1.0)],
            "weights of known clips scale to 1"
        );
        animator.blend(&asset, &[("run", 0.75), ("lean", 0.25)]);
        let current = animator.current.as_ref().unwrap();
        // Half-way through the run (x = 3) is half-way through the lean (x = 16).
        let time = 1.0 / FPS;
        let current = Playing {
            time,
            ..current.clone()
        };
        assert!((current.sample(0).translation.x - (3.0 * 0.75 + 16.0 * 0.25)).abs() < 1e-4);
        assert!((animator.root_speed() - (2.0 * 0.75 + 4.0 * 0.25)).abs() < 1e-5);
    }

    #[test]
    fn a_morph_eases_the_old_pose_out() {
        let mut morph = Morph {
            t: 0.0,
            eased: 0.0,
            frames: 4.0,
            rate: 1.0,
            share: 0.0,
        };
        // The pose put out follows what plays by each frame's share; the
        // pose at the start weighs 1 − e, eased in and out.
        let mut old_weight = 1.0;
        for eased in [0.125, 0.5, 0.875] {
            assert!(morph.step(1.0));
            old_weight *= 1.0 - morph.share;
            assert!((old_weight - (1.0 - eased)).abs() < 1e-6);
        }
        assert!(!morph.step(1.0), "over after its frames");
        // At the AS's rate: slower out of breath.
        let mut slow = Morph {
            rate: 0.85,
            t: 0.0,
            eased: 0.0,
            frames: 4.0,
            share: 0.0,
        };
        assert!((0..4).all(|_| slow.step(1.0)));
        assert!(!slow.step(1.0));
    }

    #[test]
    fn a_morph_starts_from_the_pose_put_out() {
        let mut animator = Animator::default();
        animator.morph(5.0, 1.0);
        assert!(animator.morph.is_none(), "nothing put out yet");
        animator.output = vec![Transform::IDENTITY];
        animator.morph(5.0, 1.0);
        assert!(animator.morph.is_some());
        animator.morph(0.0, 1.0);
        assert!(animator.morph.is_none(), "0 frames cancels it");
    }

    #[test]
    fn the_face_layer_poses_only_the_face() {
        let pose = |x: f32| vec![Transform::from_xyz(x, 0.0, 0.0); 3];
        let body = Clip {
            frames: vec![pose(1.0), pose(1.0)],
            looping: true,
            root_speed: 0.0,
        };
        let blink = Clip {
            frames: vec![pose(5.0), pose(8.0)],
            looping: false,
            root_speed: 0.0,
        };
        let joint = |name: &str, parent| Joint {
            name: name.into(),
            parent,
            bind: Transform::IDENTITY,
        };
        let asset = CharacterAsset {
            joints: vec![
                joint("Root", None),
                joint("Face_Root", Some(0)),
                joint("Eyelid", Some(1)),
            ],
            pieces: Vec::new(),
            offsets: Vec::new(),
            adjustments: Vec::new(),
            rt_copies: Vec::new(),
            idle: String::new(),
            clips: HashMap::from([
                ("run".to_owned(), Arc::new(body)),
                ("blink".to_owned(), Arc::new(blink)),
            ]),
        };
        assert_eq!(asset.below(1), vec![false, true, true]);
        let mut animator = Animator {
            speed: 1.0,
            ..default()
        };
        animator.play(&asset, "run", 0.0);
        animator.play_face(&asset, "blink", 0.0);
        assert_eq!(animator.face(), Some(("blink", false)));

        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_systems(Update, animate);
        let root = app.world_mut().spawn(Transform::default()).id();
        let face = app.world_mut().spawn(Transform::default()).id();
        let eyelid = app.world_mut().spawn(Transform::default()).id();
        let rig_entity = app
            .world_mut()
            .spawn((
                CharacterRig {
                    joints: vec![root, face, eyelid],
                    asset: Arc::new(asset),
                },
                animator,
                JointAdjustments(vec![
                    (0, Vec3::ZERO, Vec3::ZERO, Some(Vec3::new(0.8, 1.15, 0.9))),
                    (2, Vec3::new(0.0, 2.0, 0.0), Vec3::splat(0.25), None),
                ]),
            ))
            .id();
        app.update();
        let x = |e: Entity| app.world().get::<Transform>(e).unwrap().translation.x;
        assert_eq!((x(root), x(face), x(eyelid)), (1.0, 5.0, 5.0));
        assert_eq!(
            app.world().get::<Transform>(eyelid).unwrap().scale,
            Vec3::splat(1.25)
        );
        assert_eq!(
            app.world().get::<Transform>(eyelid).unwrap().translation.y,
            2.0
        );
        let asset = app
            .world()
            .get::<CharacterRig>(rig_entity)
            .unwrap()
            .asset
            .clone();
        app.world_mut()
            .get_mut::<Animator>(rig_entity)
            .unwrap()
            .pose_face(&asset, &[("blink", 0.5, 1.0)]);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(10),
        ));
        for _ in 0..3 {
            app.update();
            assert_eq!(
                app.world().get::<Transform>(root).unwrap().scale,
                Vec3::new(0.8, 1.15, 0.9),
                "body proportions survive clip evaluation and face overrides"
            );
            let adjusted = app.world().get::<Transform>(eyelid).unwrap();
            assert_eq!(
                adjusted.scale,
                Vec3::splat(1.25),
                "feature scale is additive and does not accumulate"
            );
            assert_eq!(
                adjusted.translation.y, 2.0,
                "feature placement survives face animation overrides"
            );
        }
        let x = |e: Entity| app.world().get::<Transform>(e).unwrap().translation.x;
        assert_eq!(
            (x(root), x(face), x(eyelid)),
            (1.0, 6.5, 6.5),
            "absolute face poses don't drift during held capture frames"
        );
    }

    #[test]
    fn plays_part_of_a_one_off_clip() {
        let at = |x: f32| vec![Transform::from_xyz(x, 0.0, 0.0)];
        let clip = Clip {
            frames: (0..=6).map(|f| at(f as f32)).collect(),
            looping: false,
            root_speed: 0.0,
        };
        let asset = CharacterAsset {
            joints: vec![Joint {
                name: "Root".into(),
                parent: None,
                bind: Transform::IDENTITY,
            }],
            pieces: Vec::new(),
            offsets: Vec::new(),
            adjustments: Vec::new(),
            rt_copies: Vec::new(),
            idle: String::new(),
            clips: HashMap::from([("jump".to_owned(), Arc::new(clip))]),
        };
        let pose = |animator: &Animator| animator.current.as_ref().unwrap().sample(0).translation.x;
        let mut animator = Animator {
            speed: 1.0,
            ..default()
        };
        // Frames 0 to 4, holding frame 4 after.
        animator.play_frames(&asset, "jump", 0.1, 0.0, Some(4.0));
        animator.current.as_mut().unwrap().time = 2.0 / FPS;
        assert!((animator.progress() - 0.5).abs() < 1e-5);
        assert!(!animator.finished());
        animator.current.as_mut().unwrap().time = 5.0 / FPS;
        assert!((pose(&animator) - 4.0).abs() < 1e-5);
        assert!(animator.finished());
        assert_eq!(animator.progress(), 1.0);

        // The same clip again, from frame 5 to its end.
        animator.play_frames(&asset, "jump", 0.0, 5.0, None);
        assert!(animator.previous.is_some(), "restarts the playing clip");
        assert!((pose(&animator) - 5.0).abs() < 1e-5);
        assert_eq!(animator.progress(), 0.0);
        animator.current.as_mut().unwrap().time = 6.0 / FPS;
        assert!(animator.finished());
    }

    #[test]
    fn a_negative_start_frame_starts_at_the_end() {
        let asset = CharacterAsset {
            joints: vec![Joint {
                name: "Root".into(),
                parent: None,
                bind: Transform::IDENTITY,
            }],
            pieces: Vec::new(),
            offsets: Vec::new(),
            adjustments: Vec::new(),
            rt_copies: Vec::new(),
            idle: String::new(),
            clips: HashMap::from([
                ("pose".to_owned(), Arc::new(clip(false))),
                ("cycle".to_owned(), Arc::new(clip(true))),
            ]),
        };
        let mut animator = Animator {
            speed: 1.0,
            ..default()
        };
        animator.play_frames(&asset, "pose", 0.0, -1.0, None);
        assert!(animator.finished(), "holds the last frame");
        // A loop's first pass starts at the frame given, ignoring an end.
        animator.play_frames(&asset, "cycle", 0.0, 1.0, Some(1.0));
        let playing = animator.current.as_ref().unwrap();
        assert!((playing.time - 1.0 / FPS).abs() < 1e-6 && playing.end.is_none());
    }

    #[test]
    fn a_loop_after_a_one_off_starts_at_the_closest_pose() {
        let turned = |angle: f32| vec![Transform::from_rotation(Quat::from_rotation_z(angle))];
        let one_off = Clip {
            frames: vec![turned(0.0), turned(1.0)],
            looping: false,
            root_speed: 0.0,
        };
        let cycle = Clip {
            frames: vec![
                turned(0.0),
                turned(0.5),
                turned(1.0),
                turned(0.5),
                turned(0.0),
            ],
            looping: true,
            root_speed: 0.0,
        };
        let asset = CharacterAsset {
            joints: vec![Joint {
                name: "Root".into(),
                parent: None,
                bind: Transform::IDENTITY,
            }],
            pieces: Vec::new(),
            offsets: Vec::new(),
            adjustments: Vec::new(),
            rt_copies: Vec::new(),
            idle: String::new(),
            clips: HashMap::from([
                ("start".to_owned(), Arc::new(one_off)),
                ("run".to_owned(), Arc::new(cycle)),
            ]),
        };
        let mut animator = Animator {
            speed: 1.0,
            ..default()
        };
        animator.play(&asset, "start", 0.1);
        animator.current.as_mut().unwrap().time = 1.0 / FPS; // At its end: turned by 1.
        animator.play(&asset, "run", 0.1);
        assert!((animator.current.as_ref().unwrap().time - 2.0 / FPS).abs() < 1e-6);
    }

    #[test]
    fn bakes_tracks_by_bone_name_with_the_root_in_bind_pose() {
        let joint = |name: &str, parent| Joint {
            name: name.into(),
            parent,
            bind: Transform::from_xyz(0.0, 9.0, 0.0),
        };
        let joints = [
            joint("Root", None),
            joint("Spine", Some(0)),
            joint("Arm", Some(1)),
        ];
        let clip = BakedClip {
            name: "walk".into(),
            frame_count: 1,
            looping: true,
            tracks: vec![
                asset_format::anim::Track {
                    bone: "Root".into(),
                    translation: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.1]],
                    ..Default::default()
                },
                asset_format::anim::Track {
                    bone: "Spine".into(),
                    translation: vec![[1.0, 0.0, 0.0], [2.0, 0.0, 0.0]],
                    ..Default::default()
                },
            ],
        };
        let baked = bake(&clip, &joints);
        assert_eq!(baked.frames.len(), 2);
        assert_eq!(
            baked.frames[1][0].translation.y, 9.0,
            "root keeps its bind pose"
        );
        assert_eq!(baked.frames[1][1].translation.x, 2.0);
        assert_eq!(baked.frames[1][2], joints[2].bind, "unanimated joints too");
        assert!((baked.root_speed - 0.1 * FPS).abs() < 1e-4);
    }

    /// Loads the baked Link (`cargo test -p render baked_link -- --ignored`,
    /// after `cargo bake --only characters`).
    #[test]
    #[ignore]
    fn baked_link_loads() {
        let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let link = load(&assets, "link").unwrap();
        assert!(link.joints.len() > 100 && link.clips.contains_key("Nml_Wait"));
        assert_eq!(link.joints[0].parent, None);
        // Every piece's bones are in the merged skeleton.
        for (_, unit, joints, binds) in &link.pieces {
            assert_eq!(joints.len(), binds.len(), "{unit}");
        }
        println!(
            "{} joints, {} pieces, {} clips",
            link.joints.len(),
            link.pieces.len(),
            link.clips.len()
        );
    }
}
