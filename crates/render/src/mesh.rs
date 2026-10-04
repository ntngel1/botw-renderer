//! Turning a tile's samples into a render mesh.
//!
//! Tiles are resampled to `resolution × resolution` quads (the source is
//! 255×255) and split into four quadrant patches. Each patch gets a skirt: a
//! strip hanging down from each edge that hides cracks where neighbouring
//! patches use different levels of detail.

use std::sync::Arc;

use asset_format::terrain::water::{WATER_SAMPLES, kind};
use asset_format::terrain::{MaterialTile, TILE_SAMPLES, TileId, WaterTile};
use bevy::asset::RenderAssetUsages;
use bevy::color::{ColorToComponents, LinearRgba, Mix, Srgba};
use bevy::math::Vec3;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

use crate::source::TileData;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorMode {
    /// Colours from height and slope; looks natural, ignores `.mate` data.
    #[default]
    Natural,
    /// A distinct colour per material index, blended like the game blends them.
    Materials,
}

impl ColorMode {
    pub fn next(self) -> Self {
        match self {
            Self::Natural => Self::Materials,
            Self::Materials => Self::Natural,
        }
    }
}

/// Water data for a tile: its own, or its nearest ancestor's.
pub type WaterSource = (TileId, Arc<WaterTile>);

/// CPU-side meshes for one tile, built off the main thread: one per
/// quadrant, in Z-order like the tile's children, so a quadrant can stand in
/// for a child tile that does not exist or has not loaded yet.
pub struct TileMesh {
    pub quadrants: [Patch; 4],
    /// Water surface per quadrant, where any of it is above the terrain.
    pub water: [Option<WaterPatch>; 4],
    /// The water data used, passed on to children without their own.
    pub water_source: Option<WaterSource>,
    /// The tile's material samples, for textured shading.
    pub mate: Option<MaterialTile>,
    pub min_height: f32,
    pub max_height: f32,
}

/// Water surface meshes for one quadrant, split by how they are drawn.
pub struct WaterPatch {
    pub water: Option<Mesh>,
    pub lava: Option<Mesh>,
    /// Where lava shows above the ground, for a light.
    pub lava_glow: Option<LavaGlow>,
}

/// The visible lava of a water patch.
pub struct LavaGlow {
    /// Mean position of the lava above the ground, relative to the tile's
    /// minimum corner (like the meshes).
    pub center: Vec3,
    /// Square metres of it.
    pub area: f32,
}

// SI-WLD-07: terrain LOD split and mesh resolution are ours.
/// Water surfaces are smooth; this many quads per quadrant edge is plenty.
const WATER_QUADS: usize = 16;

/// The water shader's flow value from `.water.extm`'s raw flow relative to
/// still water (`raw − 32768`): the game samples the flow as a 16-bit
/// normalized channel `c` and uses `2c − 1` (so still water is not quite 0).
pub fn game_flow(relative: f32) -> f32 {
    2.0 * (relative + 32768.0) / 65535.0 - 1.0
}

pub struct Patch {
    pub mesh: Mesh,
    /// Lowest point including the skirts, for bounds.
    pub min_height: f32,
    pub max_height: f32,
    /// Lowest terrain sample, without the skirts.
    pub ground_min: f32,
}

/// `resolution` is the number of quads per tile edge, rounded up to even.
pub fn build(
    tile: TileId,
    data: &TileData,
    water: Option<WaterSource>,
    resolution: u32,
    mode: ColorMode,
) -> TileMesh {
    let res = resolution.max(2).next_multiple_of(2) as usize;
    let half = res / 2;
    let quadrants: [Patch; 4] = std::array::from_fn(|q| {
        let (i0, j0) = ((q & 1) * half, (q >> 1) * half);
        build_patch(tile, data, res, i0, j0, half, mode)
    });
    let last_sample = (TILE_SAMPLES - 1) as f32;
    let to_sample = last_sample / tile.world_size();
    let ground = |x: f32, z: f32| {
        data.height.sample(
            (x * to_sample).min(last_sample),
            (z * to_sample).min(last_sample),
        )
    };
    let water_patches = std::array::from_fn(|q| {
        let (source, surface) = water.as_ref()?;
        build_water_patch(tile, q, *source, surface, quadrants[q].ground_min, &ground)
    });
    let min_height = quadrants
        .iter()
        .map(|p| p.min_height)
        .fold(f32::MAX, f32::min);
    let max_height = quadrants
        .iter()
        .map(|p| p.max_height)
        .fold(f32::MIN, f32::max);
    TileMesh {
        quadrants,
        water: water_patches,
        water_source: water,
        mate: data.material.clone(),
        min_height,
        max_height,
    }
}

/// The water surface over quadrant `q` of `tile`, sampled from `surface`
/// (the data of tile `source`, which covers `tile`). `None` if all of it is
/// below `terrain_min`, i.e. hidden under the ground. `ground` is the
/// terrain's height at a tile-relative position.
fn build_water_patch(
    tile: TileId,
    q: usize,
    source: TileId,
    surface: &WaterTile,
    terrain_min: f32,
    ground: &dyn Fn(f32, f32) -> f32,
) -> Option<WaterPatch> {
    let (tile_x, tile_z) = tile.world_min();
    let half = tile.world_size() / 2.0;
    let (x0, z0) = (
        tile_x + (q & 1) as f32 * half,
        tile_z + (q >> 1) as f32 * half,
    );
    let (source_x, source_z) = source.world_min();
    let to_grid = (WATER_SAMPLES - 1) as f32 / source.world_size();
    let n = WATER_QUADS;
    let step = half / n as f32;
    let uv = |i: usize, j: usize| {
        let (x, z) = (x0 + i as f32 * step, z0 + j as f32 * step);
        ((x - source_x) * to_grid, (z - source_z) * to_grid)
    };

    let mut positions = Vec::with_capacity((n + 1) * (n + 1));
    // The water kind and depth over the terrain ride in UV x and y for the
    // water shader, the flow (the game's value along x and z, see
    // `game_flow`) in the second UV set.
    let mut kinds = Vec::with_capacity(positions.capacity());
    let mut flows = Vec::with_capacity(positions.capacity());
    let mut highest = f32::MIN;
    for j in 0..=n {
        for i in 0..=n {
            let (u, v) = uv(i, j);
            let height = surface.height(u, v);
            highest = highest.max(height);
            let (x, z) = (x0 - tile_x + i as f32 * step, z0 - tile_z + j as f32 * step);
            positions.push([x, height, z]);
            // The game's vertex shader's kind (filtered, `packed_kind`).
            let water_kind = surface.packed_kind(u, v);
            kinds.push([f32::from(water_kind), height - ground(x, z)]);
            flows.push(surface.flow(u, v).map(game_flow));
        }
    }
    // SI-WLD-07: terrain LOD split and mesh resolution are ours.
    if highest < terrain_min {
        return None;
    }

    let (mut water, mut lava) = (Vec::new(), Vec::new());
    // Where the lava shows above the ground: its centre and how much.
    let (mut glow_sum, mut glow_cells) = (Vec3::ZERO, 0usize);
    let vertex = |i: usize, j: usize| (j * (n + 1) + i) as u32;
    for j in 0..n {
        for i in 0..n {
            let (u, v) = uv(i, j);
            let cell_kind = surface.kind(u + step * to_grid / 2.0, v + step * to_grid / 2.0);
            if cell_kind == kind::LAVA {
                let (x, z) = (
                    x0 - tile_x + (i as f32 + 0.5) * step,
                    z0 - tile_z + (j as f32 + 0.5) * step,
                );
                let height = surface.height(u + step * to_grid / 2.0, v + step * to_grid / 2.0);
                if height > ground(x, z) {
                    glow_sum += Vec3::new(x, height, z);
                    glow_cells += 1;
                }
            }
            let target = if cell_kind == kind::LAVA {
                &mut lava
            } else {
                &mut water
            };
            let (a, b, c, d) = (
                vertex(i, j),
                vertex(i + 1, j),
                vertex(i, j + 1),
                vertex(i + 1, j + 1),
            );
            target.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mesh = |indices: Vec<u32>| {
        (!indices.is_empty()).then(|| {
            Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_NORMAL,
                vec![[0.0, 1.0, 0.0]; positions.len()],
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, kinds.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, flows.clone())
            .with_inserted_indices(Indices::U32(indices))
        })
    };
    let lava_glow = (glow_cells > 0).then(|| LavaGlow {
        center: glow_sum / glow_cells as f32,
        area: glow_cells as f32 * step * step,
    });
    Some(WaterPatch {
        water: mesh(water),
        lava: mesh(lava),
        lava_glow,
    })
}

/// Builds `count × count` quads of the tile's `res × res` grid starting at
/// grid vertex `(i0, j0)`, plus skirts. Positions are relative to the tile's
/// minimum corner, so every quadrant of a tile shares one transform.
fn build_patch(
    tile: TileId,
    data: &TileData,
    res: usize,
    i0: usize,
    j0: usize,
    count: usize,
    mode: ColorMode,
) -> Patch {
    let verts_per_edge = count + 1;
    let size = tile.world_size();
    let last_sample = (TILE_SAMPLES - 1) as f32;
    let to_sample = |i: usize| i as f32 / res as f32 * last_sample;
    // World distance between two source samples, for slope-correct normals.
    let sample_spacing = size / last_sample;

    let mut positions = Vec::with_capacity(verts_per_edge * verts_per_edge + 4 * verts_per_edge);
    let mut normals = Vec::with_capacity(positions.capacity());
    let mut colors = Vec::with_capacity(positions.capacity());
    let (mut min_height, mut max_height) = (f32::MAX, f32::MIN);

    for j in j0..=j0 + count {
        for i in i0..=i0 + count {
            let (u, v) = (to_sample(i), to_sample(j));
            let h = data.height.sample(u, v);
            min_height = min_height.min(h);
            max_height = max_height.max(h);
            // Central differences, one-sided at tile edges (divided by the
            // actual span so edge normals match the neighbour's).
            let (u0, u1) = ((u - 1.0).max(0.0), (u + 1.0).min(last_sample));
            let (v0, v1) = ((v - 1.0).max(0.0), (v + 1.0).min(last_sample));
            let dhdx = (data.height.sample(u1, v) - data.height.sample(u0, v))
                / ((u1 - u0) * sample_spacing);
            let dhdz = (data.height.sample(u, v1) - data.height.sample(u, v0))
                / ((v1 - v0) * sample_spacing);
            let normal = Vec3::new(-dhdx, 1.0, -dhdz).normalize();

            positions.push([
                i as f32 / res as f32 * size,
                h,
                j as f32 / res as f32 * size,
            ]);
            normals.push(normal.to_array());
            colors.push(vertex_color(mode, h, normal, data.material.as_ref(), u, v));
        }
    }

    let grid = |i: usize, j: usize| (j * verts_per_edge + i) as u32;
    let mut indices = Vec::with_capacity(count * count * 6 + 4 * count * 6);
    for j in 0..count {
        for i in 0..count {
            let (a, b, c, d) = (
                grid(i, j),
                grid(i + 1, j),
                grid(i, j + 1),
                grid(i + 1, j + 1),
            );
            // Counter-clockwise seen from above (+Y).
            indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }

    // SI-WLD-07: terrain LOD split and mesh resolution are ours.
    // Skirts hide cracks next to patches of other levels of detail; deep
    // enough to cover the error between adjacent levels.
    let skirt_depth = (size / res as f32) * 2.0 + 2.0;
    let edges: [Vec<usize>; 4] = [
        (0..verts_per_edge).map(|i| grid(i, 0) as usize).collect(),
        (0..verts_per_edge)
            .map(|j| grid(count, j) as usize)
            .collect(),
        (0..verts_per_edge)
            .rev()
            .map(|i| grid(i, count) as usize)
            .collect(),
        (0..verts_per_edge)
            .rev()
            .map(|j| grid(0, j) as usize)
            .collect(),
    ];
    for edge in edges {
        let first_skirt = positions.len() as u32;
        for &top in &edge {
            let [x, y, z] = positions[top];
            positions.push([x, y - skirt_depth, z]);
            normals.push(normals[top]);
            colors.push(colors[top]);
        }
        for k in 0..edge.len() - 1 {
            let (t0, t1) = (edge[k] as u32, edge[k + 1] as u32);
            let (s0, s1) = (first_skirt + k as u32, first_skirt + k as u32 + 1);
            // Edges run clockwise around the patch, so this faces outwards.
            indices.extend_from_slice(&[t0, t1, s0, s0, t1, s1]);
        }
    }
    let ground_min = min_height;
    min_height -= skirt_depth;

    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices));
    Patch {
        mesh,
        min_height,
        max_height,
        ground_min,
    }
}

fn vertex_color(
    mode: ColorMode,
    height: f32,
    normal: Vec3,
    material: Option<&MaterialTile>,
    u: f32,
    v: f32,
) -> [f32; 4] {
    let color = match (mode, material) {
        (ColorMode::Materials, Some(material)) => {
            let sample = material.get(u.round() as usize, v.round() as usize);
            let (a, b) = (
                material_color(sample.material0),
                material_color(sample.material1),
            );
            a.mix(&b, f32::from(sample.blend) / 255.0)
        }
        _ => natural_color(height, normal),
    };
    color.to_f32_array()
}

fn natural_color(height: f32, normal: Vec3) -> LinearRgba {
    let steepness = 1.0 - normal.y;
    let grass = srgb(0.36, 0.50, 0.22);
    let dry = srgb(0.58, 0.56, 0.33);
    let sand = srgb(0.78, 0.72, 0.52);
    let rock = srgb(0.47, 0.45, 0.42);
    let snow = srgb(0.93, 0.95, 0.98);

    let lowland = sand.mix(&grass, smoothstep(20.0, 60.0, height));
    let upland = lowland.mix(&dry, smoothstep(250.0, 450.0, height));
    let with_rock = upland.mix(&rock, smoothstep(0.12, 0.3, steepness));
    with_rock.mix(
        &snow,
        smoothstep(480.0, 560.0, height) * (1.0 - smoothstep(0.25, 0.45, steepness)),
    )
}

/// Stable, well-spread colour per material index (golden-ratio hues).
fn material_color(index: u8) -> LinearRgba {
    let hue = (f32::from(index) * 0.618_034).fract() * 360.0;
    let lightness = 0.45 + 0.15 * f32::from(index % 3) / 2.0;
    bevy::color::Hsla::new(hue, 0.55, lightness, 1.0).into()
}

fn srgb(r: f32, g: f32, b: f32) -> LinearRgba {
    Srgba::new(r, g, b, 1.0).into()
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use asset_format::terrain::hght::world_to_raw;
    use asset_format::terrain::{HeightTile, WaterSample};
    use bevy::mesh::VertexAttributeValues;

    /// Rolling hills between 100 and 300 m.
    fn hills() -> HeightTile {
        let n = TILE_SAMPLES;
        HeightTile::from_raw(
            (0..n * n)
                .map(|i| {
                    let (x, z) = ((i % n) as f32 * 0.05, (i / n) as f32 * 0.07);
                    world_to_raw(200.0 + 100.0 * (x.sin() * z.cos()))
                })
                .collect(),
        )
    }

    fn data(material: bool) -> TileData {
        TileData {
            height: hills(),
            material: material.then(|| {
                MaterialTile::from_samples(vec![Default::default(); TILE_SAMPLES * TILE_SAMPLES])
            }),
            water: None,
            grass: None,
        }
    }

    fn flat_water(height: f32) -> WaterTile {
        WaterTile::from_samples(vec![
            WaterSample {
                height,
                flow: [32768; 2],
                kind: kind::FRESH,
                unknown: 0,
            };
            WATER_SAMPLES * WATER_SAMPLES
        ])
    }

    #[test]
    fn game_flow_spans_the_normalized_channel() {
        assert_eq!(game_flow(-32768.0), -1.0);
        assert_eq!(game_flow(32767.0), 1.0);
        // Still water (raw 32768) is just above zero, as on the GPU.
        assert!(game_flow(0.0) > 0.0 && game_flow(0.0) < 2e-5);
    }

    #[test]
    fn builds_quadrants_with_skirts() {
        let tile = TileId::from_grid(4, 5, 9).unwrap();
        let data = data(true);
        for mode in [ColorMode::Natural, ColorMode::Materials] {
            let built = build(tile, &data, None, 16, mode);
            for (q, patch) in built.quadrants.iter().enumerate() {
                let Some(VertexAttributeValues::Float32x3(positions)) =
                    patch.mesh.attribute(Mesh::ATTRIBUTE_POSITION)
                else {
                    panic!("positions missing");
                };
                assert_eq!(positions.len(), 9 * 9 + 4 * 9);
                assert_eq!(patch.mesh.indices().unwrap().len(), 8 * 8 * 6 + 4 * 8 * 6);
                // The first vertex sits at the quadrant's corner, the last grid
                // vertex at the opposite one.
                let half = tile.world_size() / 2.0;
                let (qx, qz) = ((q & 1) as f32 * half, (q >> 1) as f32 * half);
                assert!((positions[0][0] - qx).abs() < 1e-3 && (positions[0][2] - qz).abs() < 1e-3);
                let far = positions[9 * 9 - 1];
                assert!((far[0] - qx - half).abs() < 1e-3 && (far[2] - qz - half).abs() < 1e-3);
                assert!(patch.min_height < patch.max_height);
            }
            assert!(built.min_height < built.max_height);
        }
    }

    #[test]
    fn water_patches_skip_water_under_the_ground() {
        let tile = TileId::from_grid(4, 5, 9).unwrap();
        let data = data(false);
        let root = TileId::ROOT;
        let low = (root, Arc::new(flat_water(0.0)));
        assert!(
            build(tile, &data, Some(low), 8, ColorMode::Natural)
                .water
                .iter()
                .all(Option::is_none)
        );
        let high = (root, Arc::new(flat_water(900.0)));
        let built = build(tile, &data, Some(high), 8, ColorMode::Natural);
        for patch in &built.water {
            let patch = patch.as_ref().expect("water above the terrain everywhere");
            assert!(patch.water.is_some() && patch.lava.is_none());
        }
    }

    #[test]
    fn quadrant_edges_agree() {
        let tile = TileId::from_grid(3, 2, 2).unwrap();
        let data = data(false);
        let built = build(tile, &data, None, 8, ColorMode::Natural);
        let positions = |q: usize| match built.quadrants[q].mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        {
            Some(VertexAttributeValues::Float32x3(p)) => p.clone(),
            _ => panic!("positions missing"),
        };
        let (left, right) = (positions(0), positions(1));
        for j in 0..5 {
            assert_eq!(left[j * 5 + 4], right[j * 5], "shared edge row {j}");
        }
    }
}
