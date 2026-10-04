//! Terrain: the tiles a region needs, and the terrain material textures.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use asset_format::paths;
use asset_format::terrain::{
    self as baked, GrassTile, HeightTile, MaterialTable, MaterialTile, TerrainIndex,
    TerrainMaterial, TileFile, TileInfo, WaterTile,
};
use asset_format::texture::{Format, IDENTITY_SWIZZLE, Texture};
use botw_formats::content::ContentRoots;
use botw_formats::terrain::textures::{TerrainTextures, TextureArray};
use botw_formats::terrain::{self as game, MAINFIELD_TSCB, Tscb};

/// The renderer splits a tile when the camera is closer to it than this
/// many times its size (`TerrainSettings::split_factor` in `render`); a
/// region keeps every tile some camera inside it would split down to.
const SPLIT_FACTOR: f32 = 2.0;

/// A square of the map, world units.
#[derive(Clone, Copy, Debug)]
pub struct Region {
    pub center: [f32; 2],
    pub radius: f32,
}

impl Region {
    /// Horizontal distance from the region to `tile` (0 if they overlap).
    fn distance_to(&self, tile: game::TileId) -> f32 {
        let (x, z) = tile.world_min();
        let size = tile.world_size();
        let dx = (x - (self.center[0] + self.radius))
            .max((self.center[0] - self.radius) - (x + size))
            .max(0.0);
        let dz = (z - (self.center[1] + self.radius))
            .max((self.center[1] - self.radius) - (z + size))
            .max(0.0);
        dx.hypot(dz)
    }

    /// Whether a camera inside the region can split `tile` into its children.
    fn splits(&self, tile: game::TileId) -> bool {
        self.distance_to(tile) < tile.world_size() * SPLIT_FACTOR
    }
}

pub fn read_tscb(roots: &ContentRoots) -> Result<Tscb, String> {
    let path = roots
        .find(MAINFIELD_TSCB)
        .ok_or("Terrain/A/MainField.tscb not found")?;
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Tscb::parse(&bytes).map_err(|e| format!("MainField.tscb: {e}"))
}

/// Bakes every tile of the terrain scene a camera inside `region` can
/// show: the root, and the children of every kept tile it can split.
pub fn bake_tiles(
    roots: &ContentRoots,
    tscb: &Tscb,
    region_name: &str,
    region: Region,
    out: &Path,
) -> Result<(), String> {
    let areas: HashMap<game::TileId, &game::tscb::Area> =
        tscb.areas.iter().map(|area| (area.tile, area)).collect();
    let mut wanted = Vec::new();
    let mut stack = vec![game::TileId::ROOT];
    while let Some(tile) = stack.pop() {
        let Some(area) = areas.get(&tile) else {
            continue;
        };
        wanted.push(*area);
        if region.splits(tile) {
            stack.extend(tile.children().into_iter().flatten());
        }
    }
    wanted.sort_by_key(|area| (area.tile.lod(), area.tile.index()));
    let mut per_lod = [0usize; game::MAX_LOD as usize + 1];
    for area in &wanted {
        per_lod[area.tile.lod() as usize] += 1;
    }
    println!("terrain: {} tiles per level {per_lod:?}", wanted.len());

    let index = game::TerrainIndex::scan(roots);
    let done = AtomicUsize::new(0);
    let bytes_written = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = wanted.len().div_ceil(threads).max(1);
    let results: Vec<Result<Vec<TileInfo>, String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = wanted
            .chunks(chunk)
            .map(|areas| {
                let (index, done, bytes_written) = (&index, &done, &bytes_written);
                scope.spawn(move || {
                    let mut infos = Vec::with_capacity(areas.len());
                    for area in areas {
                        let (info, len) = bake_tile(index, area, out)?;
                        infos.push(info);
                        bytes_written.fetch_add(len, Ordering::Relaxed);
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        if n % 250 == 0 {
                            println!("  {n} tiles");
                        }
                    }
                    Ok(infos)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("tile thread panicked"))
            .collect()
    });
    let mut tiles = Vec::with_capacity(wanted.len());
    for result in results {
        tiles.extend(result?);
    }
    println!(
        "terrain: wrote {} tiles, {:.1} MB",
        tiles.len(),
        bytes_written.load(Ordering::Relaxed) as f64 / 1e6
    );
    let index = TerrainIndex {
        region: region_name.to_owned(),
        tiles,
    };
    asset_format::write_ron(&out.join(paths::TERRAIN_INDEX), &index).map_err(|e| e.to_string())
}

/// Converts one tile; returns its index entry and the file's size.
fn bake_tile(
    index: &game::TerrainIndex,
    area: &game::tscb::Area,
    out: &Path,
) -> Result<(TileInfo, usize), String> {
    let tile = area.tile;
    let name = tile.file_stem();
    let height = index
        .load_raw(tile, game::TerrainKind::Height)
        .map_err(|e| format!("{name} heights: {e}"))?
        .ok_or_else(|| format!("{name}: listed in MainField.tscb but has no heights"))?;
    let material = index
        .load_raw(tile, game::TerrainKind::Material)
        .map_err(|e| format!("{name} materials: {e}"))?;
    let water = match area.has_water_data {
        true => index
            .load_raw(tile, game::TerrainKind::Water)
            .map_err(|e| format!("{name} water: {e}"))?,
        false => None,
    };
    let grass = match area.has_grass_data {
        true => index
            .load_raw(tile, game::TerrainKind::Grass)
            .map_err(|e| format!("{name} grass: {e}"))?,
        false => None,
    };
    // The game's records are our sections' layouts: parse to check, as is.
    let file = TileFile {
        height: HeightTile::parse(&height).map_err(|e| format!("{name}: {e}"))?,
        material: material
            .map(|b| MaterialTile::parse(&b))
            .transpose()
            .map_err(|e| format!("{name}: {e}"))?,
        water: water
            .map(|b| WaterTile::parse(&b))
            .transpose()
            .map_err(|e| format!("{name}: {e}"))?,
        grass: grass
            .map(|b| GrassTile::parse(&b))
            .transpose()
            .map_err(|e| format!("{name}: {e}"))?,
    };
    let info = TileInfo {
        tile: baked::TileId::new(tile.lod(), tile.index()).expect("same addressing"),
        min_height: area.min_height,
        max_height: area.max_height,
        water_range: area.water,
        water: file.water.is_some(),
        grass: file.grass.is_some(),
    };
    let bytes = file.to_bytes().map_err(|e| format!("{name}: {e}"))?;
    asset_format::write(&out.join(paths::terrain_tile(info.tile)), &bytes)
        .map_err(|e| e.to_string())?;
    Ok((info, bytes.len()))
}

/// The material albedo and normal arrays (`Model/Terrain.Tex1.sbfres`) as
/// KTX2, and the material table.
pub fn bake_textures(roots: &ContentRoots, tscb: &Tscb, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let textures = TerrainTextures::load(roots).map_err(|e| format!("terrain textures: {e}"))?;
    let means = textures.albedo.layer_means();
    let materials = (0..textures.material_layers.len().max(tscb.materials.len()))
        .map(|m| {
            let info = tscb.materials.iter().find(|info| info.index as usize == m);
            let layer = textures.material_layers.get(m).copied().unwrap_or(0);
            TerrainMaterial {
                name: textures.material_names.get(m).cloned().unwrap_or_default(),
                layer,
                // The TSCB lists each material's UV scale; its default is 0.1 (10 m).
                uv_scale: info.map_or([0.1, 0.1], |i| [i.u_scale, i.v_scale]),
                unknown: info.map_or([0.0; 2], |i| i.unknown),
                mean_albedo: means.get(layer as usize).copied().unwrap_or([0.5; 3]),
            }
        })
        .collect();
    let table = MaterialTable {
        albedo_size: textures.albedo.width,
        normals_size: textures.normals.as_ref().map_or(0, |n| n.width),
        materials,
    };
    write_array(&textures.albedo, &out.join(paths::TERRAIN_ALBEDO))?;
    if let Some(normals) = &textures.normals {
        write_array(normals, &out.join(paths::TERRAIN_NORMALS))?;
    }
    asset_format::write_ron(&out.join(paths::TERRAIN_MATERIALS), &table)
        .map_err(|e| e.to_string())?;
    println!(
        "terrain textures: {} materials, {} albedo layers in {:.1} s",
        table.materials.len(),
        textures.albedo.layers,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// A layer-major BC1 array (every level of layer 0, then layer 1, …) as a
/// KTX2 file (every layer of level 0, then of level 1, …).
fn write_array(array: &TextureArray, path: &Path) -> Result<(), String> {
    let mut texture = Texture {
        format: Format::Bc1 { srgb: array.srgb },
        width: array.width,
        height: array.height,
        layers: array.layers,
        mip_levels: array.mip_levels,
        data: Vec::with_capacity(array.data.len()),
        swizzle: IDENTITY_SWIZZLE,
    };
    let sizes: Vec<usize> = (0..array.mip_levels)
        .map(|l| texture.level_bytes(l))
        .collect();
    let per_layer: usize = sizes.iter().sum();
    if per_layer * array.layers as usize != array.data.len() {
        return Err(format!("{}: unexpected texture data size", path.display()));
    }
    for (level, &size) in sizes.iter().enumerate() {
        let start: usize = sizes[..level].iter().sum();
        for layer in 0..array.layers as usize {
            texture
                .data
                .extend_from_slice(&array.data[layer * per_layer + start..][..size]);
        }
    }
    let bytes = texture
        .to_ktx2()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    asset_format::write(path, &bytes).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_splits_tiles_near_it() {
        let region = Region {
            center: [3592.7, 2121.9],
            radius: 1000.0,
        };
        assert!(region.splits(game::TileId::ROOT));
        let inside = game::TileId::containing(8, 3600.0, 2100.0).unwrap();
        assert_eq!(region.distance_to(inside), 0.0);
        assert!(region.splits(inside));
        let far = game::TileId::containing(8, -3000.0, -3000.0).unwrap();
        assert!(!region.splits(far));
    }
}
