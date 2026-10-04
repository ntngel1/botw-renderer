//! The far trees: the forests the game's terrain system draws as
//! billboards beyond the tree models' reach. Written by `bake` from
//! `Map/MainField/<cell>/<cell>_TeraTree.sblwp` (the newest copy of each:
//! the DLC's loose one, else `Pack/TitleBG.pack`'s), `ActorInfo`, the
//! atlases `Tree0Alb`/`Tree0NrmTrs`/`Tree1Alb`/`Tree1NrmTrs` and
//! `TreeDitherMask` (`Model/Terrain.Tex1.sbfres` with the smaller levels
//! from `Terrain.Tex2`), and the atlas materials' alpha tests (`TeraTree`
//! in `Model/Terrain.sbfres`).
//!
//! The whole map's trees: the billboards reach the horizon. Nothing the
//! renderer decides is applied here (hand-off distances, the actors' own
//! models).
//!
//! ```text
//! trees/index.ron          FarTreeIndex
//! trees/<atlas>.ktx2       Tree0Alb, Tree0NrmTrs, Tree1Alb, Tree1NrmTrs
//!                          as the game stores them, with its levels
//! trees/dither_mask.ktx2   TreeDitherMask's red channel, R8, every level
//! ```

use serde::{Deserialize, Serialize};

/// `trees/index.ron`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FarTreeIndex {
    /// Per atlas (`Tree0`, `Tree1`): the texture names and the atlas
    /// material's alpha-test reference.
    pub atlases: Vec<Atlas>,
    /// Every actor with a billboard that the map lists trees of.
    pub species: Vec<Species>,
    /// Per map cell with trees (`A-1` … `J-8`): its trees.
    pub cells: Vec<TreeCell>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Atlas {
    /// The albedo array (`Tree0Alb`): its file is `trees/<name>.ktx2`.
    pub albedo: String,
    /// The normals + translucency array (`Tree0NrmTrs`).
    pub normals: String,
    /// `TeraTree`'s `Tree0`/`Tree1` render state alpha-test reference.
    pub alpha_ref: f32,
}

/// How an actor's billboard is framed in its atlas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Species {
    /// The actor (as named in the `_TeraTree` lists).
    pub name: String,
    pub atlas: u32,
    /// The layer of its first view.
    pub first_layer: u32,
    /// Views at even angles from 0° in consecutive layers (1 when they are
    /// not, only the first is used then).
    pub views: u32,
    /// The picture: its bottom (model space Y), height and width (m), from
    /// the actor's bounding box (`ActorInfo` `aabb`, its main model's when
    /// it has none).
    // SI-TRE-02: billboard frame from the AABB is ours.
    pub bottom: f32,
    pub height: f32,
    pub width: f32,
    /// `ActorInfo` `traverseDist`, if above 0.
    pub traverse_dist: Option<f32>,
}

/// A map cell's far trees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TreeCell {
    pub cell: String,
    pub trees: Vec<Tree>,
}

/// A tree as the `_TeraTree` list places it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tree {
    /// Index into [`FarTreeIndex::species`].
    pub species: u32,
    pub translate: [f32; 3],
    /// Radians about X, Y, Z.
    pub rotate: [f32; 3],
    pub scale: f32,
}

pub fn atlas_path(name: &str) -> String {
    format!("{}/{name}.ktx2", crate::paths::TREES)
}
