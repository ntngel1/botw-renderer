//! The grass's shading data, what the original renderer's `grass.rs`,
//! `grass/cards.rs`, `grass/wind.rs` and `grass/hidden.rs` read from the
//! dump: the blade texture (`GrassAlb`, as stored), the tuft texture
//! (`GrassCrossAlb`, decoded with its mips rebuilt as the viewer does at
//! load), `TeraGrass`'s far-colour shares and wind swells, and where the
//! world's statistics maps take the grass away (every quarter tile of the
//! map, ORed into one bit a square metre).

use std::path::Path;

use asset_format::grass::{HiddenQuarter, QUARTER, QUARTER_METRES, Swell, TeraGrass, hidden_path};
use asset_format::paths;
use asset_format::texture::{Format, IDENTITY_SWIZZLE, Texture};
use botw_formats::bfres::bc;
use botw_formats::bfres::model::Material;
use botw_formats::content::ContentRoots;
use botw_formats::stats::{self, StatGrid};
use botw_formats::terrain::textures::{GRASS_MODEL, load_terrain_model_materials, load_textures};

/// The grids a grass cell ORs into its 9-bit mask (`FUN_035af354` handles
/// `grass+0x33f1c…0x33f24`), the original renderer's `hidden::MASKS`.
const MASKS: [&str; 3] = [
    "terrain_embedded_edge",
    "terrain_is_in_door",
    "terrain_hidden",
];

pub fn bake(roots: &ContentRoots, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let textures = load_textures(roots, &["GrassAlb", "GrassCrossAlb"])
        .map_err(|e| format!("grass textures: {e}"))?;
    let mut textures = textures.into_iter();
    let blade = textures
        .next()
        .flatten()
        .ok_or("grass: no GrassAlb in Terrain.Tex1")?;
    let tuft = textures
        .next()
        .flatten()
        .ok_or("grass: no GrassCrossAlb in Terrain.Tex1")?;
    crate::sky::write_texture(&blade, &out.join(paths::GRASS_BLADE))?;
    let rgba = tuft
        .decode_layer_rgba8(0, 0)
        .ok_or("grass: cannot decode GrassCrossAlb")?;
    let bytes = tuft_texture(rgba, tuft.width, tuft.height)
        .to_ktx2()
        .map_err(|e| e.to_string())?;
    asset_format::write(&out.join(paths::GRASS_TUFT), &bytes).map_err(|e| e.to_string())?;

    let materials = load_terrain_model_materials(roots, GRASS_MODEL)
        .map_err(|e| format!("{GRASS_MODEL}: {e}"))?
        .ok_or("grass: no Model/Terrain.sbfres in Pack/TitleBG.pack")?;
    let material = TeraGrass {
        far_shares: far_shares(&materials),
        blade_swell: swell(&materials, "Blade1"),
        tuft_swell: swell(&materials, "Cross1"),
    };
    for (what, missing) in [
        ("uking_grass_lod_color", material.far_shares.is_none()),
        ("Blade1 swell", material.blade_swell.is_none()),
        ("Cross1 swell", material.tuft_swell.is_none()),
    ] {
        if missing {
            eprintln!("warning: {GRASS_MODEL}: no {what}; the renderer keeps its recorded values");
        }
    }
    crate::sky::write_ron::<_, TeraGrass>(&out.join(paths::GRASS_MATERIAL), &material)?;

    // Every quarter tile with a statistics archive.
    let mut quarters = 0;
    let mut hidden_metres = 0;
    let dir = out.join(paths::GRASS_HIDDEN);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    for qz in 0..16u32 {
        for qx in 0..20u32 {
            let corner = [-5000.0 + qx as f32 * QUARTER, -4000.0 + qz as f32 * QUARTER];
            let at = (corner[0] + 1.0, corner[1] + 1.0);
            let grids: Vec<StatGrid> = stats::load_quarter(roots, at.0, at.1, &MASKS)
                .map_err(|e| format!("statistics maps at {at:?}: {e}"))?
                .unwrap_or_default()
                .into_iter()
                .map(|(_, grid)| grid)
                .collect();
            if grids.is_empty() {
                continue;
            }
            let mut quarter = HiddenQuarter::new();
            for j in 0..QUARTER_METRES {
                for i in 0..QUARTER_METRES {
                    let metre = (corner[0] as i32 + i as i32, corner[1] as i32 + j as i32);
                    if grids.iter().any(|g| g.metre_bit(metre.0, metre.1)) {
                        quarter.set(i, j);
                        hidden_metres += 1;
                    }
                }
            }
            if quarter.is_empty() {
                continue;
            }
            let bytes = quarter.to_bytes().map_err(|e| e.to_string())?;
            asset_format::write(&out.join(hidden_path([qx, qz])), &bytes)
                .map_err(|e| e.to_string())?;
            quarters += 1;
        }
    }
    println!(
        "grass: GrassAlb {}x{} ({:?}), GrassCrossAlb {}x{}, {} hidden m² in {quarters} quarters ({:.1} s)",
        blade.width,
        blade.height,
        blade.format,
        tuft.width,
        tuft.height,
        hidden_metres,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// `uking_grass_lod_color.w` of `TeraGrass`'s `Blade1` and `Blade2` (types
/// 0 and 1, matched by the blade table's same numbers).
fn far_shares(materials: &[Material]) -> Option<[f32; 2]> {
    let share = |name: &str| {
        materials
            .iter()
            .find(|m| m.name == name)?
            .shader_param("uking_grass_lod_color")?
            .get(3)
            .copied()
    };
    share("Blade1").zip(share("Blade2")).map(|(a, b)| [a, b])
}

/// The swell of `TeraGrass`'s material `name` (the original renderer's
/// `wind::load_swell`, before packing).
fn swell(materials: &[Material], name: &str) -> Option<Swell> {
    let material = materials.iter().find(|m| m.name == name)?;
    let param = |p: &str| {
        material
            .shader_param(&format!("uking_grass_wind_swell_{p}"))?
            .first()
            .copied()
    };
    let swell = Swell {
        freq_scale: param("freq_scale")?,
        dispersion_scale: param("dispersion_scale")?,
        scale: param("scale")?,
        world_transform_coef: param("world_transform_coef")?,
    };
    (swell.world_transform_coef > 0.0).then_some(swell)
}

/// The tuft as sRGB RGBA with every mip level; each level's alpha is scaled
/// so the same share of it passes the cut-out as at level 0, or far cards
/// would thin away with distance (the original renderer's `cards::tuft_image`).
// SI-GRS-02: tuft mips rebuilt keeping alpha coverage (our model).
fn tuft_texture(rgba: Vec<u8>, width: u32, height: u32) -> Texture {
    let covered = |data: &[u8], scale: f32| {
        data.chunks(4)
            .filter(|p| f32::from(p[3]) * scale >= 127.5)
            .count() as f32
            / (data.len() / 4) as f32
    };
    let target = covered(&rgba, 1.0);
    let (mut level, mut w, mut h) = (rgba, width, height);
    let mut data = level.clone();
    let mut levels = 1;
    while w > 1 || h > 1 {
        (level, w, h) = bc::downsample(&level, w, h);
        // The alpha scale that keeps the coverage, by bisection.
        let (mut lo, mut hi) = (0.5f32, 8.0f32);
        for _ in 0..16 {
            let mid = 0.5 * (lo + hi);
            if covered(&level, mid) < target {
                lo = mid
            } else {
                hi = mid
            }
        }
        let mut scaled = level.clone();
        for p in scaled.chunks_mut(4) {
            p[3] = (f32::from(p[3]) * hi).min(255.0) as u8;
        }
        data.extend_from_slice(&scaled);
        levels += 1;
    }
    Texture {
        format: Format::Rgba8 { srgb: true },
        width,
        height,
        layers: 1,
        mip_levels: levels,
        data,
        swizzle: IDENTITY_SWIZZLE,
    }
}
