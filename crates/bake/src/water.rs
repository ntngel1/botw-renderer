//! The water's shading data, what the original renderer's `water_material.rs`
//! (`load_game_water`) reads from the dump: the water table (`WaterAlb`,
//! `Pack/TitleBG.pack` → `Model/Terrain.Tex2.sbfres`), `TeraWater`'s
//! material parameters (`Pack/TitleBG.pack` → `Model/Terrain.sbfres`) and
//! the water's normal and foam maps (`WaterNrm`, `WaterEmm` in
//! `Model/Terrain.Tex1.sbfres`), converted the way the viewer converts them
//! at load: RG8 and R8 arrays with rebuilt mips.

use std::path::Path;

use asset_format::paths;
use asset_format::texture::{Format, IDENTITY_SWIZZLE, Texture};
use asset_format::water::TeraWater;
use botw_formats::bfres::model::Material;
use botw_formats::bfres::{TextureImage, bc};
use botw_formats::content::ContentRoots;
use botw_formats::terrain::textures::{WaterTable, load_textures, load_water_material};

use crate::sky::write_ron;

pub fn bake(roots: &ContentRoots, out: &Path) -> Result<(), String> {
    let table = WaterTable::load(roots)
        .map_err(|e| format!("water table: {e}"))?
        .ok_or("water table: no WaterAlb in Pack/TitleBG.pack")?;
    write_ron::<_, asset_format::water::WaterTable>(&out.join(paths::WATER_TABLE), &table)?;

    let material = load_water_material(roots)
        .map_err(|e| format!("water material: {e}"))?
        .ok_or("water material: no Model/Terrain.sbfres in Pack/TitleBG.pack")?;
    let material = tera_water(&material)
        .map_err(|name| format!("water material: TeraWater has no usable {name}"))?;
    asset_format::write_ron(&out.join(paths::WATER_MATERIAL), &material)
        .map_err(|e| e.to_string())?;

    let textures =
        load_textures(roots, &["WaterNrm", "WaterEmm"]).map_err(|e| format!("water maps: {e}"))?;
    let mut textures = textures.into_iter();
    let normals = textures
        .next()
        .flatten()
        .ok_or("water maps: no WaterNrm in Terrain.Tex1")?;
    let foam = textures
        .next()
        .flatten()
        .ok_or("water maps: no WaterEmm in Terrain.Tex1")?;
    for (image, channels, path) in [
        (&normals, 2, paths::WATER_NORMALS),
        (&foam, 1, paths::WATER_FOAM),
    ] {
        let bytes = array_with_mips(image, channels)
            .to_ktx2()
            .map_err(|e| format!("{}: {e}", image.name))?;
        asset_format::write(&out.join(path), &bytes).map_err(|e| e.to_string())?;
    }
    println!(
        "water: table, TeraWater, maps {}x{}x{} ({:?}) and {}x{}x{} ({:?})",
        normals.width,
        normals.height,
        normals.layers,
        normals.format,
        foam.width,
        foam.height,
        foam.layers,
        foam.format
    );
    Ok(())
}

/// The parameters from the game's material (the original renderer's
/// `TeraWater::from_material`); the name of the first one missing or
/// malformed otherwise.
fn tera_water(material: &Material) -> Result<TeraWater, String> {
    fn get<const N: usize>(material: &Material, name: &str) -> Result<[f32; N], String> {
        material
            .shader_param(name)
            .and_then(|v| v.try_into().ok())
            .ok_or_else(|| name.to_owned())
    }
    let scalar = |name: &str| get::<1>(material, name).map(|[v]| v);
    let mut tex_srt = [[0.0; 6]; 6];
    for (i, srt) in tex_srt.iter_mut().enumerate() {
        *srt = get(material, &format!("tex_srt{i}"))?;
    }
    Ok(TeraWater {
        tex_srt,
        indirect_scale2: get(material, "indirect_scale2")?,
        indirect_scale4: get(material, "indirect_scale4")?,
        const_color2: get(material, "const_color2")?,
        const_color3: get(material, "const_color3")?,
        const_color5: get(material, "const_color5")?,
        const_vector0: get(material, "const_vector0")?,
        const_vector1: get(material, "const_vector1")?,
        const_value2: scalar("const_value2")?,
        const_value3: scalar("const_value3")?,
        const_value6: scalar("const_value6")?,
    })
}

/// The first `channels` channels of every layer of `texture` (RG8 for 2,
/// R8 for 1), with a full mip chain: the dump has only the first level
/// (the original renderer's `array_with_mips`).
// SI-FMT-10: texture mips and BC conversions are ours.
fn array_with_mips(texture: &TextureImage, channels: usize) -> Texture {
    let (width, height) = (texture.width, texture.height);
    let levels = 32 - width.max(height).leading_zeros();
    // Per layer, every level; KTX2 wants every layer of a level together.
    let mut layers: Vec<Vec<Vec<u8>>> = Vec::new();
    for layer in 0..texture.layers {
        let mut rgba = texture
            .decode_layer_rgba8(0, layer)
            .unwrap_or_else(|| vec![128; (width * height * 4) as usize]);
        let (mut w, mut h) = (width, height);
        let mut chain = Vec::with_capacity(levels as usize);
        for level in 0..levels {
            if level > 0 {
                (rgba, w, h) = bc::downsample(&rgba, w, h);
            }
            chain.push(
                rgba.chunks(4)
                    .flat_map(|p| p[..channels].to_vec())
                    .collect(),
            );
        }
        layers.push(chain);
    }
    let data = (0..levels as usize)
        .flat_map(|level| {
            layers
                .iter()
                .flat_map(move |chain| chain[level].iter().copied())
        })
        .collect();
    Texture {
        format: if channels == 2 {
            Format::Rg8
        } else {
            Format::R8
        },
        width,
        height,
        layers: texture.layers,
        mip_levels: levels,
        data,
        swizzle: IDENTITY_SWIZZLE,
    }
}
