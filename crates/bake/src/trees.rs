//! The far trees, what the original renderer's `far_trees.rs` (`load`) reads from
//! the dump: the billboard atlases and `TreeDitherMask` (`Terrain.Tex1`
//! with the smaller levels of `Terrain.Tex2`), the atlas materials'
//! alpha tests (`TeraTree`), `ActorInfo`, and every map cell's
//! `_TeraTree` list. The atlases stay as the game stores them; the mask is
//! decoded to its red channel at every level, as the viewer does at load.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use asset_format::paths;
use asset_format::texture::{Format, IDENTITY_SWIZZLE, Texture};
use asset_format::trees::{Atlas, FarTreeIndex, Species, Tree, TreeCell, atlas_path};
use botw_formats::actor::ActorPacks;
use botw_formats::bfres::{Bfres, TextureImage, assemble_texture};
use botw_formats::content::ContentRoots;
use botw_formats::prod;
use botw_formats::terrain::textures::{TERRAIN_TEX1, TERRAIN_TEX2};
use botw_formats::trees::{
    ATLASES, ActorInfo, BillboardFrame, BillboardView, DITHER_MASK, billboard_views,
    parse_actor_info,
};

pub fn bake(roots: &ContentRoots, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let title_bg = roots
        .find("Pack/TitleBG.pack")
        .map(|p| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())))
        .transpose()?
        .map(Arc::new);
    let packs = ActorPacks::new(roots.clone(), title_bg);
    let file = |name: &str| -> Result<Vec<u8>, String> {
        packs
            .file(name)
            .map_err(|e| format!("{name}: {e}"))?
            .ok_or_else(|| format!("{name} not found"))
    };
    let tex1 = file(TERRAIN_TEX1)?;
    let tex1 = Bfres::parse(&tex1).map_err(|e| e.to_string())?;
    // Level 0 is in `Terrain.Tex1`, the smaller levels in `Terrain.Tex2`.
    let tex2 = file(TERRAIN_TEX2)?;
    let tex2 = Bfres::parse(&tex2).map_err(|e| e.to_string())?;
    let info = parse_actor_info(&file("Actor/ActorInfo.product.sbyml")?)
        .map_err(|e| format!("ActorInfo: {e}"))?;
    let array = |name: &str| -> Result<(TextureImage, Vec<(String, String)>), String> {
        let texture = tex1
            .texture(name)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("{name} not found"))?;
        let mips = tex2.texture(name).map_err(|e| e.to_string())?;
        let image =
            assemble_texture(&texture, mips.as_ref()).map_err(|e| format!("{name}: {e}"))?;
        Ok((image, texture.user_data.clone()))
    };

    let refs = alpha_refs(&packs)?;
    let mut atlases = Vec::new();
    let mut views = HashMap::new();
    for (atlas, (albedo_name, normals_name)) in ATLASES.iter().enumerate() {
        let (albedo, user_data) = array(albedo_name)?;
        let (normals, _) = array(normals_name)?;
        for image in [&albedo, &normals] {
            write_as_is(image, &out.join(atlas_path(&image.name)))?;
        }
        let files = user_data
            .iter()
            .find(|(k, _)| k == "file")
            .map_or("", |(_, v)| v.as_str());
        for (actor, list) in billboard_views(files) {
            views.insert(actor, (atlas as u32, list));
        }
        atlases.push(Atlas {
            albedo: albedo_name.to_string(),
            normals: normals_name.to_string(),
            alpha_ref: refs[atlas],
        });
        println!(
            "trees: {albedo_name} {}x{}x{} ({:?}, {} levels)",
            albedo.width, albedo.height, albedo.layers, albedo.format, albedo.mip_levels
        );
    }

    // The trees the terrain system draws: one instance list per map cell
    // (the newest copy is the DLC's, loose; the base game's are in
    // `TitleBG.pack`).
    let mut species: Vec<Species> = Vec::new();
    let mut known: HashMap<String, Option<u32>> = HashMap::new();
    let mut cells = Vec::new();
    let mut count = 0;
    for cell in asset_format::objects::all_cells() {
        let Some(bytes) = packs
            .file(&format!("Map/MainField/{cell}/{cell}_TeraTree.sblwp"))
            .map_err(|e| e.to_string())?
        else {
            continue;
        };
        let groups = prod::parse(&bytes).map_err(|e| format!("{cell}_TeraTree: {e}"))?;
        let mut trees = Vec::new();
        for group in groups {
            let index = *known.entry(group.name.clone()).or_insert_with(|| {
                let found = species_of(&group.name, &info, &views)?;
                species.push(found);
                Some(species.len() as u32 - 1)
            });
            let Some(index) = index else { continue };
            for instance in &group.instances {
                trees.push(Tree {
                    species: index,
                    translate: instance.translate,
                    rotate: instance.rotate.map(f32::to_radians),
                    scale: instance.scale,
                });
            }
        }
        count += trees.len();
        if !trees.is_empty() {
            cells.push(TreeCell { cell, trees });
        }
    }
    let index = FarTreeIndex {
        atlases,
        species,
        cells,
    };
    crate::sky::write_ron::<_, FarTreeIndex>(&out.join(paths::TREE_INDEX), &index)?;

    let mask = dither_mask(&tex1, &tex2).map_err(|e| format!("{DITHER_MASK}: {e}"))?;
    let bytes = mask.to_ktx2().map_err(|e| e.to_string())?;
    asset_format::write(&out.join(paths::TREE_DITHER_MASK), &bytes).map_err(|e| e.to_string())?;
    println!(
        "trees: {count} far trees of {} species in {} cells ({:.1} s)",
        index.species.len(),
        index.cells.len(),
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// A game texture as KTX2, texel data as it is (Zstandard).
fn write_as_is(image: &TextureImage, path: &Path) -> Result<(), String> {
    let texture = crate::sky::texture(image)
        .ok_or_else(|| format!("{}: format {:?} not supported", image.name, image.format))?;
    if texture.swizzle != IDENTITY_SWIZZLE {
        // the original renderer decodes the atlases without the component selection.
        return Err(format!("{}: unexpected swizzle", image.name));
    }
    let bytes = texture
        .to_ktx2_zstd(19)
        .map_err(|e| format!("{}: {e}", image.name))?;
    asset_format::write(path, &bytes).map_err(|e| e.to_string())
}

/// The terrain's tree materials' alpha-test references (`Tree0`, `Tree1` in
/// `TeraTree`, `Model/Terrain.sbfres`), per atlas.
fn alpha_refs(packs: &ActorPacks) -> Result<[f32; 2], String> {
    let bytes = packs
        .file("Model/Terrain.sbfres")
        .map_err(|e| e.to_string())?
        .ok_or("Model/Terrain.sbfres not found")?;
    let bfres = Bfres::parse(&bytes).map_err(|e| e.to_string())?;
    let model = bfres
        .models()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| m.name == "TeraTree")
        .ok_or("TeraTree not found")?;
    let reference = |name: &str| {
        model
            .materials
            .iter()
            .find(|m| m.name == name)
            .and_then(|m| m.render_state.alpha_test)
            .ok_or_else(|| format!("TeraTree {name}: no alpha test"))
    };
    Ok([reference("Tree0")?, reference("Tree1")?])
}

/// The game's dissolve mask (BC4, one channel) decoded, every level.
fn dither_mask(tex1: &Bfres, tex2: &Bfres) -> Result<Texture, String> {
    let texture = tex1
        .texture(DITHER_MASK)
        .map_err(|e| e.to_string())?
        .ok_or("not found")?;
    let mips = tex2.texture(DITHER_MASK).map_err(|e| e.to_string())?;
    let image = assemble_texture(&texture, mips.as_ref()).map_err(|e| e.to_string())?;
    let red = |rgba: Vec<u8>| rgba.chunks(4).map(|p| p[0]).collect::<Vec<u8>>();
    let levels = (0..image.mip_levels)
        .map(|level| image.decode_rgba8(level).map(red))
        .collect::<Option<Vec<_>>>()
        .ok_or("cannot decode")?;
    Ok(Texture {
        format: Format::R8,
        width: image.width,
        height: image.height,
        layers: 1,
        mip_levels: image.mip_levels,
        data: levels.concat(),
        swizzle: IDENTITY_SWIZZLE,
    })
}

/// The pictures and framing of an actor's billboard, if it has one: by its
/// own name, or its main model's (`_Set_01` variants share their tree's).
/// the original renderer's `species_of`, without the hand-off (the renderer's).
fn species_of(
    name: &str,
    info: &HashMap<String, ActorInfo>,
    views: &HashMap<String, (u32, Vec<BillboardView>)>,
) -> Option<Species> {
    let entry = info.get(name)?;
    // Actors with a `_Far` model (the Zonai towers) are drawn by that.
    if info.contains_key(&format!("{name}_Far")) {
        return None;
    }
    let (atlas, list) = views
        .get(name)
        .or_else(|| views.get(entry.main_model.as_deref()?))?;
    let aabb = entry
        .aabb
        .or_else(|| info.get(entry.main_model.as_deref()?)?.aabb)?;
    // The shader expects the views in consecutive layers, evenly spaced
    // from 0°; otherwise only the first is used.
    let n = list.len();
    let even = list.iter().enumerate().all(|(k, v)| {
        v.layer == list[0].layer + k as u32 && (v.angle - k as f32 * 360.0 / n as f32).abs() < 0.5
    });
    // SI-TRE-02: billboard frame from the AABB is ours.
    let frame = BillboardFrame::from_aabb(aabb);
    Some(Species {
        name: name.to_owned(),
        atlas: *atlas,
        first_layer: list.first()?.layer,
        // (Packed with the layer in 4 bits.)
        views: if even && n < 16 { n as u32 } else { 1 },
        bottom: frame.bottom,
        height: frame.height,
        width: frame.width,
        traverse_dist: entry.traverse_dist,
    })
}
