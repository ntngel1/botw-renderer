//! Skeletal clips: one GLB per clip, `characters/anims/<set>/<clip>.glb`
//! (`<set>` the game's animation file, e.g. `Player_Animation`).
//!
//! A clip is the game's FSKA sampled at every frame (30 per second, the
//! game's rate) for every bone it animates, by name (the game binds
//! animations to skeletons by bone name), as the original renderer's
//! `character::bake` samples it: a bone's translation, rotation and scale
//! where the animation has them (base values or curves), the bind pose's
//! elsewhere. Euler rotations are turned into quaternions here (the
//! viewer's `euler_rotation`), so every rotation is a quaternion.
//!
//! In glTF: a node per animated bone (named like it, no hierarchy), one
//! animation named like the clip with a LINEAR sampler per animated
//! component over the shared frame times (`frame / 30` s) and `extras`
//! `looping` and `frameCount`. Which bone is the skeleton's root, and what
//! the renderer does with its motion, is the renderer's (the clip keeps it).

use crate::glb::{
    Animation, AnimationExtras, AnimationSampler, Asset, Channel, ChannelTarget, Glb, Gltf, Node,
    Scene, Writer,
};
use crate::{FormatError, Result};

/// The game's animation rate (frames per second).
pub const FPS: f32 = 30.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clip {
    pub name: String,
    /// The clip's length in frames: it has samples for frames `0..=frame_count`.
    pub frame_count: u32,
    pub looping: bool,
    pub tracks: Vec<Track>,
}

/// A bone's pose at every frame of a clip; an empty component is one the
/// clip does not animate.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub bone: String,
    pub translation: Vec<[f32; 3]>,
    /// Quaternions (x, y, z, w).
    pub rotation: Vec<[f32; 4]>,
    pub scale: Vec<[f32; 3]>,
}

impl Clip {
    /// How many samples each animated component has.
    pub fn samples(&self) -> usize {
        self.frame_count as usize + 1
    }

    pub fn to_glb(&self) -> Result<Vec<u8>> {
        let samples = self.samples();
        let mut writer = Writer::default();
        let times: Vec<[f32; 1]> = (0..samples).map(|f| [f as f32 / FPS]).collect();
        let input = writer.floats(&times, "SCALAR", true, None);
        let mut channels = Vec::new();
        let mut samplers = Vec::new();
        for (node, track) in self.tracks.iter().enumerate() {
            let mut channel = |output: u32, path: &str| {
                samplers.push(AnimationSampler {
                    input,
                    output,
                    interpolation: "LINEAR".into(),
                });
                channels.push(Channel {
                    sampler: samplers.len() as u32 - 1,
                    target: ChannelTarget {
                        node: node as u32,
                        path: path.into(),
                    },
                });
            };
            for (len, path) in [
                (track.translation.len(), "translation"),
                (track.rotation.len(), "rotation"),
                (track.scale.len(), "scale"),
            ] {
                if len != 0 && len != samples {
                    return Err(FormatError::Invalid(
                        "clip: a track's samples differ from the clip's frames",
                    ));
                }
                if len == 0 {
                    continue;
                }
                let output = match path {
                    "translation" => writer.floats(&track.translation, "VEC3", false, None),
                    "rotation" => writer.floats(&track.rotation, "VEC4", false, None),
                    _ => writer.floats(&track.scale, "VEC3", false, None),
                };
                channel(output, path);
            }
        }
        let gltf = Gltf {
            asset: Asset::ours(),
            scene: 0,
            scenes: vec![Scene {
                name: self.name.clone(),
                nodes: (0..self.tracks.len() as u32).collect(),
            }],
            nodes: self
                .tracks
                .iter()
                .map(|t| Node {
                    name: t.bone.clone(),
                    ..Default::default()
                })
                .collect(),
            animations: vec![Animation {
                name: self.name.clone(),
                channels,
                samplers,
                extras: AnimationExtras {
                    looping: self.looping,
                    frame_count: self.frame_count,
                },
            }],
            ..Default::default()
        };
        writer.finish(gltf, "clip: cannot write the glTF JSON")
    }

    /// Reads a GLB written by [`Clip::to_glb`].
    pub fn from_glb(bytes: &[u8]) -> Result<Self> {
        let invalid = FormatError::Invalid;
        let glb = Glb::parse(bytes)?;
        let animation = glb
            .gltf
            .animations
            .first()
            .ok_or(invalid("clip: no animation"))?;
        let mut tracks: Vec<Track> = glb
            .gltf
            .nodes
            .iter()
            .map(|n| Track {
                bone: n.name.clone(),
                ..Default::default()
            })
            .collect();
        let samples = animation.extras.frame_count as usize + 1;
        for channel in &animation.channels {
            let sampler = animation
                .samplers
                .get(channel.sampler as usize)
                .ok_or(invalid("clip: sampler out of range"))?;
            let track = tracks
                .get_mut(channel.target.node as usize)
                .ok_or(invalid("clip: node out of range"))?;
            let len = match channel.target.path.as_str() {
                "translation" => {
                    track.translation = glb.floats::<3>(sampler.output)?;
                    track.translation.len()
                }
                "rotation" => {
                    track.rotation = glb.floats::<4>(sampler.output)?;
                    track.rotation.len()
                }
                "scale" => {
                    track.scale = glb.floats::<3>(sampler.output)?;
                    track.scale.len()
                }
                _ => return Err(invalid("clip: unknown channel path")),
            };
            if len != samples {
                return Err(invalid(
                    "clip: a track's samples differ from the clip's frames",
                ));
            }
        }
        Ok(Self {
            name: animation.name.clone(),
            frame_count: animation.extras.frame_count,
            looping: animation.extras.looping,
            tracks,
        })
    }

    pub fn read(path: &std::path::Path) -> Result<Self> {
        Self::from_glb(&crate::read(path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_clip() {
        let clip = Clip {
            name: "Nml_Wait".into(),
            frame_count: 2,
            looping: true,
            tracks: vec![
                Track {
                    bone: "Root".into(),
                    translation: vec![[0.0, 1.0, 0.0], [0.0, 1.1, 0.0], [0.0, 1.0, 0.0]],
                    ..Default::default()
                },
                Track {
                    bone: "Spine_1".into(),
                    rotation: vec![[0.0, 0.0, 0.0, 1.0]; 3],
                    scale: vec![[1.0; 3]; 3],
                    ..Default::default()
                },
            ],
        };
        let glb = clip.to_glb().unwrap();
        assert_eq!(Clip::from_glb(&glb).unwrap(), clip);
        let short = Clip {
            frame_count: 5,
            ..clip
        };
        assert!(short.to_glb().is_err(), "tracks must cover every frame");
    }
}
