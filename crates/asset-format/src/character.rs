//! Characters: what one is made of, `characters/<name>.ron`
//! ([`CharacterDef`]), with its skinned model units under
//! `characters/models/<folder>/<unit>.glb` (see [`crate::model`]: skeleton,
//! joints, weights) and their textures next to them, and its clips under
//! `characters/anims/<set>/<clip>.glb` (see [`crate::anim`]).
//!
//! the original renderer's `character::CharacterSpec` with the dump resolved: the
//! body first (its skeleton is the base the others' bones merge into by
//! name), then outfit pieces; the body shapes the pieces cover; the clip
//! set and which clips of it were baked.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CharacterDef {
    /// `(folder, unit)` of each model, the body first.
    pub parts: Vec<(String, String)>,
    /// Shapes not to draw, by name: body skin under clothes.
    pub hidden: Vec<String>,
    /// Rotations added on top of the animation to some bones, by name, as
    /// quaternions (x, y, z, w): the ears bent back under a hood
    /// (`EarRotate` of the head piece).
    pub joint_offsets: Vec<(String, [f32; 4])>,
    /// Additive scale/translation differences from authored UMii feature
    /// tracks, applied after the normal animation without changing inverse binds.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub joint_adjustments: Vec<JointAdjustment>,
    /// `(destination, source)` local rotation/translation copies after animation.
    /// Destination scale stays independent (UMii Head_Controled, Wii U 0x0343b104).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub joint_rt_copies: Vec<(String, String)>,
    /// Roots of separate model units attached to another part's joint.
    /// `(root, parent)`; the original inverse bind matrices stay unchanged.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub joint_attachments: Vec<(String, String)>,
    /// The game's animation file the clips come from, e.g.
    /// `Player_Animation`: `characters/anims/<animations>/`.
    pub animations: String,
    /// The clips baked for it (in that folder).
    pub clips: Vec<String>,
    /// Bones of later parts that are an earlier part's bone, by name:
    /// `(bone, joint)`. The game copies the joint's world matrix onto the
    /// bone every frame, so the bone is that joint. Local RT-only copies
    /// belong in `joint_rt_copies`, preserving the destination scale.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<(String, String)>,
    /// The clip it loops when standing about (empty: none chosen).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub idle: String,
}

/// Characters standing where the map places them:
/// `characters/umii/placed.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlacedCharacters {
    pub placed: Vec<PlacedCharacter>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlacedCharacter {
    /// `characters/<character>.ron`.
    pub character: String,
    /// The map actor it stands for, and its placement's hash.
    pub actor: String,
    pub hash_id: u32,
    pub translate: [f32; 3],
    /// Euler angles (radians, X, Y, Z) as in the map.
    pub rotate: [f32; 3],
}

/// Native UMii feature differences (v208 0x03436204), with an optional
/// absolute body proportion scale (0x0343dcc0).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JointAdjustment {
    pub joint: String,
    pub translation: [f32; 3],
    pub scale: [f32; 3],
    /// Native body proportion tracks replace local scale before additive features.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale_override: Option<[f32; 3]>,
}
