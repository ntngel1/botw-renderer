//! What covers the sky above the field: the game's sky occlusion map
//! (KSys's, `gsys_depth_shadow_quarter` with `gsys_environment[39]`), which
//! the field's pre-shading turns into `vis` (`sky_visibility` in
//! `deferred_light.wgsl`; docs/research/wiiu-field-shading.md, "Top-down sky
//! map").
//!
//! The game draws, from above, a square of [`SIDE`] metres around the
//! camera at a metre a texel: the terrain's heights, then the models whose
//! material has `uking_edit_sky_occlusion` (depth only, the highest wins),
//! then halves the map and blurs it. Here the same map is built on the CPU
//! whenever its centre or the set of such models changes: the heights of
//! the terrain ([`HeightSampler`]) and the triangles of the placed objects'
//! [`SkyOccluder`]s. It reaches the shaders through the look texture
//! ([`LookCover`], [`Look::sky_occlusion`]).

use std::sync::Arc;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::heights::HeightSampler;
use crate::look::{LOOK_COVER, Look, LookCover, LookSystems};

/// Side of the map (m), the game's `+0x780`.
pub const SIDE: f32 = 192.0;
/// Texels of the full map a side (the game's `+0x778`): a metre each.
const TEXELS: usize = 192;
/// How far the centre moves at a time (m): the game's `+0x784`, 4 texels.
const STEP: f32 = 4.0;
/// Heights the map holds, from 0 up (the game's `+0x788`, `e39.w`): its
/// camera looks down from `1 + RANGE` with its far plane at 0.
const RANGE: f32 = 1000.0;
/// The blur of the halved map, once across and once down. The game blurs
/// with agl's Gaussian filter, variant 5 (`+0x794`), whose weights were not
/// read: a stand-in of five binomial taps.
// SI-LGT-14: sky-occlusion blur kernel, LOD, centre and heights are ours.
const BLUR: [f32; 5] = [1.0 / 16.0, 4.0 / 16.0, 6.0 / 16.0, 4.0 / 16.0, 1.0 / 16.0];

pub struct SkyOcclusionPlugin {
    pub sampler: HeightSampler,
}

impl Plugin for SkyOcclusionPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(SkyCover {
            sampler: self.sampler.clone(),
            centre: None,
            dirty: true,
            building: None,
        })
        .add_systems(PostUpdate, update_cover.before(LookSystems::Upload));
    }
}

/// A model's shapes that cover the sky (their materials'
/// `uking_edit_sky_occlusion` is on; not blended), in model space: the
/// coarsest level of detail, as the game's camera sees them from 1 km up.
// SI-LGT-14: sky-occlusion blur kernel, LOD, centre and heights are ours.
#[derive(Debug, Default)]
pub struct SkyShape {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    /// Distance of the farthest vertex from the model's origin.
    pub radius: f32,
}

impl SkyShape {
    pub fn new(vertices: Vec<Vec3>, triangles: Vec<[u32; 3]>) -> Self {
        let radius = vertices.iter().map(|v| v.length()).fold(0.0, f32::max);
        Self {
            vertices,
            triangles,
            radius,
        }
    }
}

/// On a placed object: its models' shapes that cover the sky.
#[derive(Component, Clone, Debug)]
pub struct SkyOccluder(pub Arc<[Arc<SkyShape>]>);

/// The map being kept around the camera.
#[derive(Resource)]
pub struct SkyCover {
    sampler: HeightSampler,
    /// Centre of the map in the look texture.
    centre: Option<Vec2>,
    /// The covering models changed since the map was started.
    dirty: bool,
    building: Option<Task<(Vec2, Vec<f32>)>>,
}

impl SkyCover {
    /// The map in the look texture is the one for the camera's place and
    /// the models there.
    pub fn is_settled(&self) -> bool {
        self.centre.is_some() && !self.dirty && self.building.is_none()
    }
}

/// The map's centre for a camera at `at` (x, z): the middle of the
/// [`STEP`]-metre cell it is in (the game's `FUN_033fa9d0`).
// SI-LGT-14: sky-occlusion blur kernel, LOD, centre and heights are ours.
fn centre_for(at: Vec2) -> Vec2 {
    ((at / STEP).floor() + 0.5) * STEP
}

/// The look texels for a map around `centre` (see [`Look::sky_occlusion`]):
/// the corner split into a whole part (a multiple of 4 m, exact in half
/// precision) and the rest.
fn cover_texels(centre: Vec2) -> [Vec4; 4] {
    let corner = centre - SIDE * 0.5;
    let whole = (corner / 4.0).floor() * 4.0;
    let rest = corner - whole;
    [
        Vec4::new(whole.x, rest.x, whole.y, rest.y),
        Vec4::new(SIDE, RANGE, 0.0, 1.0),
        Vec4::ZERO,
        Vec4::ZERO,
    ]
}

#[allow(clippy::too_many_arguments)]
fn update_cover(
    mut cover: ResMut<SkyCover>,
    mut map: ResMut<LookCover>,
    mut look: ResMut<Look>,
    cameras: Query<&GlobalTransform, crate::camera::MainCamera>,
    occluders: Query<(&SkyOccluder, &Transform)>,
    added: Query<(), Added<SkyOccluder>>,
    mut removed: RemovedComponents<SkyOccluder>,
) {
    if !added.is_empty() || removed.read().count() > 0 {
        cover.dirty = true;
    }
    if let Some(task) = cover.building.as_mut() {
        let Some((centre, heights)) = block_on(poll_once(task)) else {
            return;
        };
        cover.building = None;
        cover.centre = Some(centre);
        map.0 = Some(Arc::new(heights));
        look.sky_occlusion = cover_texels(centre);
    }
    let Ok(camera) = cameras.single() else { return };
    let centre = centre_for(camera.translation().xz());
    if cover.centre == Some(centre) && !cover.dirty {
        return;
    }
    cover.dirty = false;
    // The models whose reach touches the map.
    let reach = SIDE * 0.5 * std::f32::consts::SQRT_2;
    let shapes: Vec<(Mat4, Arc<SkyShape>)> = occluders
        .iter()
        .flat_map(|(occluder, transform)| {
            let size = transform.scale.max_element();
            let distance = transform.translation.xz().distance(centre);
            let matrix = transform.to_matrix();
            occluder
                .0
                .iter()
                .filter(move |s| distance < reach + s.radius * size)
                .map(move |s| (matrix, s.clone()))
        })
        .collect();
    debug!("sky cover around {centre}: {} shapes", shapes.len());
    let sampler = cover.sampler.clone();
    cover.building = Some(AsyncComputeTaskPool::get().spawn(async move {
        let heights = build_cover(centre, &|x, z| sampler.height_at(x, z), &shapes);
        (centre, heights)
    }));
}

/// The map around `centre`: [`LOOK_COVER`]² heights (m) of what covers the
/// sky, row by row from the north-west corner. The full map holds, per
/// metre, the terrain's height (0 where there is none: the game clears its
/// depth to the far plane) or the highest point of the shapes above it,
/// within 0…[`RANGE`] (the camera's near and far planes clip the rest);
/// halved as the game's copy does (the mean of 2 × 2), then blurred.
fn build_cover(
    centre: Vec2,
    terrain: &dyn Fn(f32, f32) -> Option<f32>,
    shapes: &[(Mat4, Arc<SkyShape>)],
) -> Vec<f32> {
    let corner = centre - SIDE * 0.5;
    let texel = SIDE / TEXELS as f32;
    let mut full = vec![0.0f32; TEXELS * TEXELS];
    for (i, h) in full.iter_mut().enumerate() {
        let (x, z) = (i % TEXELS, i / TEXELS);
        let at = corner + (Vec2::new(x as f32, z as f32) + 0.5) * texel;
        // SI-LGT-14: sky-occlusion blur kernel, LOD, centre and heights are ours.
        *h = terrain(at.x, at.y).unwrap_or(0.0).clamp(0.0, RANGE);
    }
    for (matrix, shape) in shapes {
        let world: Vec<Vec3> = shape
            .vertices
            .iter()
            .map(|&v| matrix.transform_point3(v))
            .collect();
        for t in &shape.triangles {
            let [a, b, c] = t.map(|i| world[i as usize]);
            raster_top(&mut full, corner, texel, a, b, c);
        }
    }
    let half = TEXELS / 2;
    debug_assert_eq!(half, LOOK_COVER);
    let mut halved = vec![0.0f32; half * half];
    for (i, h) in halved.iter_mut().enumerate() {
        let (x, z) = (2 * (i % half), 2 * (i / half));
        let at = |dx: usize, dz: usize| full[(z + dz) * TEXELS + x + dx];
        *h = 0.25 * (at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1));
    }
    blur(&blur(&halved, half, (1, 0)), half, (0, 1))
}

/// Raises the texels whose centres the triangle `a b c` covers (seen from
/// above) to its height there, if it lies within the map's heights.
fn raster_top(full: &mut [f32], corner: Vec2, texel: f32, a: Vec3, b: Vec3, c: Vec3) {
    // Texel coordinates, texel `i` centred on `i`.
    let to = |p: Vec3| (p.xz() - corner) / texel - 0.5;
    let (pa, pb, pc) = (to(a), to(b), to(c));
    let area = (pb - pa).perp_dot(pc - pa);
    if area.abs() < 1e-9 {
        return; // Seen edge-on from above: covers nothing.
    }
    let lo = pa.min(pb).min(pc).ceil().max(Vec2::ZERO);
    let hi = pa
        .max(pb)
        .max(pc)
        .floor()
        .min(Vec2::splat((TEXELS - 1) as f32));
    if lo.x > hi.x || lo.y > hi.y {
        return;
    }
    for z in lo.y as usize..=hi.y as usize {
        for x in lo.x as usize..=hi.x as usize {
            let p = Vec2::new(x as f32, z as f32);
            let wa = (pc - pb).perp_dot(p - pb) / area;
            let wb = (pa - pc).perp_dot(p - pc) / area;
            let wc = 1.0 - wa - wb;
            if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                continue;
            }
            let y = wa * a.y + wb * b.y + wc * c.y;
            let h = &mut full[z * TEXELS + x];
            if (0.0..=RANGE).contains(&y) && y > *h {
                *h = y;
            }
        }
    }
}

/// One pass of [`BLUR`] along `step` over a `side`² map, the edges
/// repeated.
fn blur(map: &[f32], side: usize, step: (usize, usize)) -> Vec<f32> {
    let reach = BLUR.len() as i32 / 2;
    (0..side * side)
        .map(|i| {
            let (x, z) = ((i % side) as i32, (i / side) as i32);
            BLUR.iter()
                .enumerate()
                .map(|(k, w)| {
                    let d = k as i32 - reach;
                    let sx = (x + d * step.0 as i32).clamp(0, side as i32 - 1);
                    let sz = (z + d * step.1 as i32).clamp(0, side as i32 - 1);
                    w * map[sz as usize * side + sx as usize]
                })
                .sum()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(y: f32) -> impl Fn(f32, f32) -> Option<f32> {
        move |_, _| Some(y)
    }

    #[test]
    fn centre_keeps_to_four_metre_cells() {
        assert_eq!(centre_for(Vec2::new(0.1, -0.1)), Vec2::new(2.0, -2.0));
        assert_eq!(
            centre_for(Vec2::new(-901.0, 1803.9)),
            Vec2::new(-902.0, 1802.0)
        );
    }

    #[test]
    fn corner_splits_exactly_for_half_floats() {
        let [corner, size, ..] = cover_texels(Vec2::new(-4502.0, 3002.0));
        // Corner -4598, 2906: multiples of 4 and the rest.
        assert_eq!(corner, Vec4::new(-4600.0, 2.0, 2904.0, 2.0));
        assert_eq!(size, Vec4::new(SIDE, RANGE, 0.0, 1.0));
    }

    #[test]
    fn open_ground_is_the_terrain_itself() {
        let map = build_cover(Vec2::ZERO, &flat(120.0), &[]);
        assert_eq!(map.len(), LOOK_COVER * LOOK_COVER);
        assert!(map.iter().all(|&h| (h - 120.0).abs() < 1e-3));
    }

    #[test]
    fn heights_stay_within_the_cameras_range() {
        let below = build_cover(Vec2::ZERO, &flat(-30.0), &[]);
        let above = build_cover(Vec2::ZERO, &flat(1500.0), &[]);
        assert!(below.iter().all(|&h| h == 0.0));
        assert!(above.iter().all(|&h| h == RANGE));
        let none = build_cover(Vec2::ZERO, &|_, _| None, &[]);
        assert!(none.iter().all(|&h| h == 0.0));
    }

    #[test]
    fn a_roof_raises_the_map_under_it_only() {
        // A 20 m square roof 8 m above ground 100, centred on the map.
        let v = vec![
            Vec3::new(-10.0, 108.0, -10.0),
            Vec3::new(10.0, 108.0, -10.0),
            Vec3::new(10.0, 108.0, 10.0),
            Vec3::new(-10.0, 108.0, 10.0),
        ];
        let roof = Arc::new(SkyShape::new(v, vec![[0, 1, 2], [0, 2, 3]]));
        let map = build_cover(Vec2::ZERO, &flat(100.0), &[(Mat4::IDENTITY, roof)]);
        let at = |x: f32, z: f32| {
            let i = ((x + SIDE * 0.5) / 2.0) as usize;
            let j = ((z + SIDE * 0.5) / 2.0) as usize;
            map[j * LOOK_COVER + i]
        };
        assert!((at(0.0, 0.0) - 108.0).abs() < 1e-3, "{}", at(0.0, 0.0));
        // The blur softens the edge over a few metres; far off, the ground.
        assert!(at(12.0, 0.0) > 100.0 && at(12.0, 0.0) < 108.0);
        assert!((at(40.0, 40.0) - 100.0).abs() < 1e-3);
    }

    #[test]
    fn a_transformed_shape_lands_where_it_is_placed() {
        let v = vec![
            Vec3::new(-2.0, 0.0, -2.0),
            Vec3::new(2.0, 0.0, -2.0),
            Vec3::new(2.0, 0.0, 2.0),
            Vec3::new(-2.0, 0.0, 2.0),
        ];
        let slab = Arc::new(SkyShape::new(v, vec![[0, 1, 2], [0, 2, 3]]));
        let placed = Mat4::from_scale_rotation_translation(
            Vec3::splat(3.0),
            Quat::IDENTITY,
            Vec3::new(50.0, 20.0, -30.0),
        );
        let map = build_cover(Vec2::ZERO, &flat(0.0), &[(placed, slab)]);
        let i = ((50.0 + SIDE * 0.5) / 2.0) as usize;
        let j = ((-30.0 + SIDE * 0.5) / 2.0) as usize;
        assert!((map[j * LOOK_COVER + i] - 20.0).abs() < 1e-3);
        assert_eq!(map[0], 0.0);
    }
}
