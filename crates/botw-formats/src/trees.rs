//! Far trees: the game draws distant trees as billboards from two texture
//! arrays in `Terrain.Tex1` (`Tree0Alb` 128×128, `Tree1Alb` 64×128, BC3,
//! with `Tree0NrmTrs`/`Tree1NrmTrs` beside them). Their user data names
//! every layer `<actor>_R<degrees>`: the actor's model turned by that many
//! degrees about Y and seen from +Z, orthographically. Which trees get
//! billboards and how big they are comes from `Actor/ActorInfo.product.sbyml`
//! (tags, `mainModel`, bounding box). See archived FORMATS.md. notes

use std::collections::HashMap;

use roead::byml::Byml;

use crate::bfres::TextureImage;
use crate::content::ContentRoots;
use crate::{FormatError, Result};

/// The two billboard atlases, in the order the renderer indexes them.
pub const ATLASES: [(&str, &str); 2] = [("Tree0Alb", "Tree0NrmTrs"), ("Tree1Alb", "Tree1NrmTrs")];

/// One view of an actor in a billboard atlas.
#[derive(Clone, Debug, PartialEq)]
pub struct BillboardView {
    /// Yaw of the model in the picture, degrees.
    pub angle: f32,
    pub layer: u32,
}

/// Billboard views per actor name, from an atlas's `file` user data
/// (comma-separated `<actor>_R<ddd>` in layer order).
pub fn billboard_views(files: &str) -> HashMap<String, Vec<BillboardView>> {
    let mut views: HashMap<String, Vec<BillboardView>> = HashMap::new();
    for (layer, file) in files.split(',').enumerate() {
        let Some((actor, angle)) = file.rsplit_once("_R") else {
            continue;
        };
        let Ok(angle) = angle.parse::<f32>() else {
            continue;
        };
        views
            .entry(actor.to_owned())
            .or_default()
            .push(BillboardView {
                angle,
                layer: layer as u32,
            });
    }
    for list in views.values_mut() {
        list.sort_by(|a, b| a.angle.total_cmp(&b.angle));
    }
    views
}

/// What `ActorInfo` says about an actor, as far as far trees need it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ActorInfo {
    pub main_model: Option<String>,
    /// Model-space bounding box.
    pub aabb: Option<([f32; 3], [f32; 3])>,
    /// CRC32 hashes of the actor's tags.
    pub tags: Vec<u32>,
    // SI-TRE-01: far-tree hand-off distance is our heuristic.
    /// How far from the camera the actor exists (0 or absent: the game's
    /// default of 100 m).
    pub traverse_dist: Option<f32>,
}

impl ActorInfo {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.contains(&crc32(tag.as_bytes()))
    }
}

/// `Actor/ActorInfo.product.sbyml`: every actor's summary.
pub fn read_actor_info(roots: &ContentRoots) -> Result<HashMap<String, ActorInfo>> {
    let path = roots
        .find("Actor/ActorInfo.product.sbyml")
        .ok_or(FormatError::Invalid("ActorInfo.product.sbyml not found"))?;
    let bytes = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
    parse_actor_info(&bytes)
}

pub fn parse_actor_info(bytes: &[u8]) -> Result<HashMap<String, ActorInfo>> {
    let data = crate::yaz0::decompress_if(bytes)?;
    let doc =
        Byml::from_binary(&data[..]).map_err(|_| FormatError::Invalid("ActorInfo: not BYML"))?;
    let Ok(root) = doc.as_map() else {
        return Err(FormatError::Invalid("ActorInfo: root is not a map"));
    };
    let Some(Byml::Array(actors)) = root.get("Actors") else {
        return Err(FormatError::Invalid("ActorInfo: no Actors"));
    };
    let vec3 = |value: Option<&Byml>| -> Option<[f32; 3]> {
        let map = value?.as_map().ok()?;
        Some([
            map.get("X")?.as_float().ok()?,
            map.get("Y")?.as_float().ok()?,
            map.get("Z")?.as_float().ok()?,
        ])
    };
    let mut info = HashMap::with_capacity(actors.len());
    for actor in actors {
        let Ok(map) = actor.as_map() else { continue };
        let Some(name) = map.get("name").and_then(|n| n.as_string().ok()) else {
            continue;
        };
        let tags = match map.get("tags") {
            Some(Byml::Map(tags)) => tags
                .values()
                .filter_map(|v| match v {
                    Byml::I32(h) => Some(*h as u32),
                    Byml::U32(h) => Some(*h),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let aabb = vec3(map.get("aabbMin")).zip(vec3(map.get("aabbMax")));
        let main_model = map
            .get("mainModel")
            .and_then(|m| m.as_string().ok())
            .map(|m| m.to_string());
        // SI-TRE-01: far-tree hand-off distance is our heuristic.
        let traverse_dist = map
            .get("traverseDist")
            .and_then(|d| d.as_float().ok())
            .filter(|d| *d > 0.0);
        info.insert(
            name.to_string(),
            ActorInfo {
                main_model,
                aabb,
                tags,
                traverse_dist,
            },
        );
    }
    Ok(info)
}

/// Standard CRC32 (the hash the game's tags and parameter names use).
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// How a billboard frames its tree, from the actor's bounding box: the
/// picture is `height` tall from `bottom` (model space Y) and `width` wide,
/// centred on the model's origin, wide enough for the box at any yaw.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BillboardFrame {
    pub bottom: f32,
    pub height: f32,
    pub width: f32,
}

// SI-TRE-02: billboard frame from the AABB is ours.
impl BillboardFrame {
    pub fn from_aabb((min, max): ([f32; 3], [f32; 3])) -> Self {
        let reach_x = min[0].abs().max(max[0].abs());
        let reach_z = min[2].abs().max(max[2].abs());
        Self {
            bottom: min[1],
            height: max[1] - min[1],
            width: 2.0 * reach_x.hypot(reach_z),
        }
    }
}

/// `Terrain.Tex1`'s mask for dissolving a tree's model into its billboard.
pub const DITHER_MASK: &str = "TreeDitherMask";

/// A dissolve mask's values replaced by their ranks, spread evenly over
/// 0–255: hiding the texels below a threshold `t` then hides the share `t`
/// of them, whatever the mask's own histogram (`TreeDitherMask` is mostly
/// dark). Equal values are ordered by a hash of their position, so a flat
/// area does not dissolve row by row.
pub fn dither_ranks(values: &[u8]) -> Vec<u8> {
    let scramble = |i: usize| {
        (i as u32)
            .wrapping_mul(0x9E37_79B9)
            .rotate_left(13)
            .wrapping_mul(0x85EB_CA6B)
    };
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by_key(|&i| (values[i], scramble(i)));
    let mut ranks = vec![0; values.len()];
    for (rank, &i) in order.iter().enumerate() {
        ranks[i] = (rank * 256 / values.len().max(1)) as u8;
    }
    ranks
}

/// An uncompressed RGBA8 texture array with its mip chain.
#[derive(Clone, Debug, PartialEq)]
pub struct RgbaArray {
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    /// Per mip level, every layer back to back.
    pub levels: Vec<Vec<u8>>,
}

/// Decodes a billboard atlas (or its normals) with the game's own mip chain
/// (`Terrain.Tex2`: 8 levels), which the far-tree pixel shader's alpha test
/// reads as it is.
pub fn decode_atlas(texture: &TextureImage) -> Option<RgbaArray> {
    let levels = (0..texture.mip_levels)
        .map(|level| {
            (0..texture.layers)
                .map(|layer| texture.decode_layer_rgba8(level, layer))
                .collect::<Option<Vec<_>>>()
                .map(|layers| layers.concat())
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RgbaArray {
        width: texture.width,
        height: texture.height,
        layers: texture.layers,
        levels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_known_tag_hashes() {
        // Hashes as stored in ActorInfo for the `Tree` tag.
        assert_eq!(crc32(b"Tree"), 0x170C_F1E2);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn views_are_grouped_by_actor_and_sorted() {
        let views =
            billboard_views("Tree_A_L_R000,Tree_A_L_R045,Other_R120,Other_R000,Tree_A_L_R090,junk");
        assert_eq!(
            views["Tree_A_L"]
                .iter()
                .map(|v| (v.angle, v.layer))
                .collect::<Vec<_>>(),
            [(0.0, 0), (45.0, 1), (90.0, 4)]
        );
        assert_eq!(
            views["Other"]
                .iter()
                .map(|v| (v.angle, v.layer))
                .collect::<Vec<_>>(),
            [(0.0, 3), (120.0, 2)]
        );
        assert_eq!(views.len(), 2);
    }

    #[test]
    fn dither_ranks_spread_evenly() {
        // Mostly zeros, like the game's mask: ranks still cover 0-255 evenly.
        let values: Vec<u8> = (0..1024)
            .map(|i| if i % 3 == 0 { (i % 200) as u8 } else { 0 })
            .collect();
        let ranks = dither_ranks(&values);
        for threshold in [16u8, 64, 128, 200] {
            let below = ranks.iter().filter(|&&r| r < threshold).count();
            assert_eq!(below, threshold as usize * 4, "threshold {threshold}");
        }
        // Order is kept where values differ.
        let (a, b) = (
            values.iter().position(|&v| v == 150).unwrap(),
            values.iter().position(|&v| v == 30).unwrap(),
        );
        assert!(ranks[a] > ranks[b]);
    }

    #[test]
    fn frame_covers_the_box_at_any_yaw() {
        let frame = BillboardFrame::from_aabb(([-3.0, -1.0, -2.0], [4.0, 9.0, 1.0]));
        assert_eq!(frame.bottom, -1.0);
        assert_eq!(frame.height, 10.0);
        assert!((frame.width - 2.0 * 20f32.sqrt()).abs() < 1e-5);
    }
}
