//! The game's grass distances (docs/research/wiiu-field-shading.md, "Grass blade
//! fade to distant color"). Every one of them scales with `1/k`, where
//! `k = max(tan 10°, tan(fovy/2))` is the camera's current half-angle
//! (`0x03701f2c`): narrowing the view draws the grass farther out. The
//! shaders take `k` from the projection themselves; these are for the CPU's
//! chunk rings and the materials' tables.
//!
//! `reach` (the viewer's grass draw distance setting, not the game's; 1 is
//! the game) moves every one of these distances `reach` times as far out.

use bevy::prelude::*;

/// `tan(20°/2)`, the least `k` (`DAT_1047bf1c` = 20°, `0x03701efc`).
pub const MIN_K: f32 = 0.176_326_98;
/// `tan 25°`: the half-angle the tables are made for (the tuft and cover
/// writers `0x03704e10`, `0x03708118` multiply by it in code).
pub const REFERENCE_K: f32 = 0.466_31;

/// Per blade type (`0x1047bf54`, fields 3 and 4): the apparent size
/// `1/(d·k)` where a cell stops drawing all its blades, and where it draws
/// none. 4.1 m and 12.5 m (type 0), 13.2 m and 25 m (type 1) at `REFERENCE_K`.
pub const BLADE_SIZES: [(f32, f32); 2] = [(0.523_05, 0.171_56), (0.162_50, 0.085_78)];
/// `uking_grass_lod_color.w` of `Blade1` and `Blade2` (`TeraGrass`), for
/// when there is no dump: how far each type turns to the far colour
/// (`sat(w − r)²`).
pub const BLADE_FAR_SHARES: [f32; 2] = [0.7, 0.9];

/// The grass's own scale, `tera+0x10fc`: 1.0, written once by the terrain
/// constructor `0x036fba18` (@`0x036fbdd4`); every blade, tuft and cover size carries it.
pub const GRASS_SCALE: f32 = 1.0;
/// Blade table field 2 (`0x1047bf54`), the same for both types: a blade is
/// `0.4·S·w·(0.75…1.25)` long per third (`gsys_shape[2].y` = `S·T.2/3`).
pub const BLADE_HEIGHT: f32 = 1.2;
/// Blade table field 1 × the blade class's `p[8]` = 0.16 (`0x037026c4`): the
/// width per unit of lean, `gsys_shape[2].x` (0.112 and 0.144).
pub const BLADE_WIDTHS: [f32; 2] = [0.16 * 0.7, 0.16 * 0.9];
/// Blades per cell side (@`0x036df3e4`): a 3 m cell holds 30² type 0 blades
/// and 16² type 1 blades.
pub const BLADES_PER_SIDE: [u32; 2] = [30, 16];
/// Edge of the cell the blade buffer fills, metres (`0x03703140`).
pub const CELL: f32 = 3.0;
/// Tuft table field 1 (`0x1047bf90`): a tuft of full grass is this tall
/// (× `S`; `gsys_shape[2].y` of `uking_grass_cross`).
pub const TUFT_HEIGHTS: [f32; 2] = [1.56, 1.44];

/// Per tuft type (`0x1047bf90`, fields 2–5), metres at `REFERENCE_K`: where
/// tufts start growing in and over how far, where the last shrinks away
/// and over how far.
pub const TUFT_DISTANCES: [Vec4; 2] = [
    Vec4::new(8.0, 7.51, 50.8, 20.0),
    Vec4::new(14.0, 8.5, 120.0, 24.0),
];

/// The game's `k` for a camera.
// SI-GRS-05: grass LOD k from the viewer's 45 degree FOV.
pub fn lod_k(projection: &Projection) -> f32 {
    match projection {
        Projection::Perspective(p) => (p.fov * 0.5).tan().max(MIN_K),
        // The game keeps an extent there; the viewer's camera is perspective.
        _ => MIN_K,
    }
}

/// A blade type's apparent sizes ([`BLADE_SIZES`]) at `reach`.
pub fn blade_sizes(kind: usize, reach: f32) -> (f32, f32) {
    let (full, none) = BLADE_SIZES[kind];
    (full / reach, none / reach)
}

/// The tuft types' distances ([`TUFT_DISTANCES`]) at `reach`.
pub fn tuft_distances(reach: f32) -> [Vec4; 2] {
    TUFT_DISTANCES.map(|t| t * reach)
}

/// Distance beyond which no blade of a type is drawn.
pub fn type_reach(k: f32, kind: usize, reach: f32) -> f32 {
    1.0 / (k * blade_sizes(kind, reach).1)
}

/// Distance beyond which no blade is drawn.
pub fn blade_reach(k: f32, reach: f32) -> f32 {
    (0..BLADE_SIZES.len())
        .map(|kind| type_reach(k, kind, reach))
        .fold(0.0, f32::max)
}

/// Nearest and farthest distance where tufts show.
pub fn tuft_span(k: f32, reach: f32) -> (f32, f32) {
    let scale = REFERENCE_K / k;
    let tufts = tuft_distances(reach);
    let near = tufts.iter().map(|t| t.x).fold(f32::MAX, f32::min);
    let far = tufts.iter().map(|t| t.z).fold(0.0, f32::max);
    (near * scale, far * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blade_tables_meet_their_design_distances_at_fifty_degrees() {
        let [(_, none0), (full1, none1)] = BLADE_SIZES;
        let at = |size: f32| 1.0 / (REFERENCE_K * size);
        assert!((at(none0) - 12.5).abs() < 0.01);
        assert!((at(none1) - 25.0).abs() < 0.01);
        assert!((at(full1) - 13.2).abs() < 0.01);
        assert!((blade_reach(REFERENCE_K, 1.0) - 25.0).abs() < 0.01);
        assert_eq!(tuft_span(REFERENCE_K, 1.0), (8.0, 120.0));
    }

    #[test]
    fn reach_moves_every_distance_as_far_out() {
        let k = REFERENCE_K;
        assert!((blade_reach(k, 3.0) - 3.0 * blade_reach(k, 1.0)).abs() < 0.01);
        for kind in 0..2 {
            assert!((type_reach(k, kind, 2.0) - 2.0 * type_reach(k, kind, 1.0)).abs() < 0.01);
        }
        let (near, far) = tuft_span(k, 4.0);
        assert!((near - 32.0).abs() < 0.01 && (far - 480.0).abs() < 0.01);
    }

    #[test]
    fn a_narrower_view_draws_grass_farther() {
        let wide = lod_k(&Projection::Perspective(PerspectiveProjection {
            fov: 70f32.to_radians(),
            ..default()
        }));
        let narrow = lod_k(&Projection::Perspective(PerspectiveProjection {
            fov: 30f32.to_radians(),
            ..default()
        }));
        assert!(blade_reach(narrow, 1.0) > blade_reach(wide, 1.0));
        assert!(tuft_span(narrow, 1.0).1 > tuft_span(wide, 1.0).1);
        // Below 20° the distances stop growing.
        let zoomed = lod_k(&Projection::Perspective(PerspectiveProjection {
            fov: 5f32.to_radians(),
            ..default()
        }));
        assert_eq!(zoomed, MIN_K);
    }
}
