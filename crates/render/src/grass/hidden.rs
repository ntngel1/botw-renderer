//! Where the game takes grass away for good: the world statistics maps
//! `terrain_embedded_edge`, `terrain_is_in_door` and `terrain_hidden`
//! (`Game/Stats/archive/*.sstats`, a bit a square metre) that the grass
//! cells read when they load (`0x035b0330` states 0xb–0xd, `0x035bd074`).
//! A cell whose bits are set zeroes the mow maps' green there
//! (`interact_cut_array`, `interact.rs`), so neither blades nor tufts grow
//! under houses, inside walls or where objects sink into the ground.
//! docs/research/wiiu-field-shading.md, "Mowing and flattening".

use std::collections::HashMap;
use std::path::PathBuf;

use asset_format::grass::{HiddenQuarter, QUARTER, WORLD_HALF, hidden_path, quarter_of};
use bevy::prelude::*;

/// The masks of the quarter tiles loaded so far, read from the baked
/// quarters (`grass/hidden/`, the three maps ORed) as the maps reach them;
/// without them no grass is hidden.
#[derive(Resource, Default)]
pub struct HiddenGrass {
    assets: Option<PathBuf>,
    quarters: HashMap<[u32; 2], Option<HiddenQuarter>>,
}

impl HiddenGrass {
    pub fn new(assets: PathBuf) -> Self {
        Self {
            assets: Some(assets),
            quarters: HashMap::new(),
        }
    }

    /// Whether the square metre with corner `(x, z)` has no grass.
    pub fn hidden(&mut self, metre: IVec2) -> bool {
        let at = metre.as_vec2() + 0.5;
        let Some(quarter) = quarter_of(at.x, at.y) else {
            return false;
        };
        let assets = &self.assets;
        let grid = self.quarters.entry(quarter).or_insert_with(|| {
            let path = assets.as_ref()?.join(hidden_path(quarter));
            if !path.exists() {
                return None;
            }
            asset_format::read(&path)
                .and_then(|bytes| HiddenQuarter::from_bytes(&bytes))
                .inspect_err(|err| warn!("grass: hidden quarter {quarter:?}: {err}"))
                .ok()
        });
        let Some(grid) = grid else {
            return false;
        };
        let corner = IVec2::new(
            (quarter[0] as f32 * QUARTER - WORLD_HALF[0]) as i32,
            (quarter[1] as f32 * QUARTER - WORLD_HALF[1]) as i32,
        );
        let local = metre - corner;
        grid.get(local.x as usize, local.y as usize)
    }
}
