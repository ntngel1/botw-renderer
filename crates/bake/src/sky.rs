//! The sky block: time-of-day palettes and climates (`WorldMgr/normal.bwinfo`),
//! the renderer's base objects (`Env/env.sgenvb`), the climate map
//! (`Ecosystem/`), the atmosphere table (`sky.skybin`), the fog's noise and
//! the cloud textures.

use std::path::Path;

use asset_format::paths;
use asset_format::texture::{Format, Texture};
use botw_formats::bfres::TextureImage;
use botw_formats::bfres::gx2;
use botw_formats::content::ContentRoots;
use botw_formats::env::EnvParams;
use botw_formats::envset::EnvSet;

pub fn bake(roots: &ContentRoots, out: &Path) -> Result<(), String> {
    let params = EnvParams::load(roots)
        .map_err(|e| format!("env params: {e}"))?
        .ok_or("env params: no Pack/TitleBG.pack")?;
    write_ron::<_, asset_format::env::EnvParams>(&out.join(paths::ENV_PARAMS), &params)?;
    println!(
        "sky: {} palettes, {} palette sets, {} climates",
        params.palettes.len(),
        params.palette_sets.len(),
        params.climates.len()
    );

    let set = EnvSet::load(roots)
        .map_err(|e| format!("env set: {e}"))?
        .ok_or("env set: no Pack/Bootup.pack")?;
    write_ron::<_, asset_format::envset::EnvSet>(&out.join(paths::ENV_SET), &set)?;

    let ecosystem = botw_formats::eco::Ecosystem::load(roots)
        .map_err(|e| format!("ecosystem: {e}"))?
        .ok_or("ecosystem: no Pack/Bootup.pack")?;
    write_ron::<_, asset_format::eco::Ecosystem>(&out.join(paths::ECOSYSTEM), &ecosystem)?;
    println!("sky: ecosystem with {} areas", ecosystem.areas.len());

    let inscatter = botw_formats::sky::load_inscatter(roots)
        .map_err(|e| format!("sky table: {e}"))?
        .ok_or("sky table: no Pack/Bootup_Graphics.pack")?;
    let inscatter = asset_format::sky::Inscatter {
        texels: inscatter.texels,
    };
    let bytes = inscatter.to_bytes().map_err(|e| e.to_string())?;
    asset_format::write(&out.join(paths::INSCATTER), &bytes).map_err(|e| e.to_string())?;

    let noise = botw_formats::system_model::load_fog_noise(roots)
        .map_err(|e| format!("cloud noise: {e}"))?
        .ok_or("cloud noise: no Pack/Bootup_Graphics.pack")?;
    let noise = asset_format::light::GreyImage {
        width: noise.width,
        height: noise.height,
        texels: noise.texels,
    };
    asset_format::write_ron(&out.join(paths::CLOUD_NOISE), &noise).map_err(|e| e.to_string())?;

    // The cloud list in the game's order, and the shadow's texture.
    let dir = out.join(paths::CLOUD_TEXTURE_DIR);
    let list = EnvSet::load_cloud_textures(roots).map_err(|e| format!("cloud textures: {e}"))?;
    let mut textures = Vec::with_capacity(list.len());
    for (i, image) in list.iter().enumerate() {
        let name = format!("{i:02}_{}.ktx2", image.name);
        write_texture(image, &dir.join(&name))?;
        textures.push(name);
    }
    let shadow = match &set.cloud_shadow_texture {
        Some(reference) => match EnvSet::load_texture(roots, reference)
            .map_err(|e| format!("cloud shadow: {e}"))?
        {
            Some(image) => {
                let name = format!("shadow_{}.ktx2", image.name);
                write_texture(&image, &dir.join(&name))?;
                Some(name)
            }
            None => None,
        },
        None => None,
    };
    println!("sky: {} cloud textures, shadow {shadow:?}", textures.len());
    let clouds = asset_format::sky::CloudTextures { textures, shadow };
    asset_format::write_ron(&out.join(paths::CLOUD_TEXTURES), &clouds).map_err(|e| e.to_string())
}

/// Writes `value` as RON and checks that the runtime's mirror type `M`
/// reads it back.
pub fn write_ron<T: serde::Serialize, M: serde::de::DeserializeOwned>(
    path: &Path,
    value: &T,
) -> Result<(), String> {
    asset_format::write_ron(path, value).map_err(|e| e.to_string())?;
    asset_format::read_ron::<M>(path)
        .map(|_| ())
        .map_err(|e| format!("the runtime cannot read what was written: {e}"))
}

/// A game texture as KTX2 (texel data as it is).
pub fn write_texture(image: &TextureImage, path: &Path) -> Result<(), String> {
    let texture = texture(image).ok_or_else(|| {
        format!(
            "{}: texture format {:?} is not supported",
            image.name, image.format
        )
    })?;
    let bytes = texture
        .to_ktx2()
        .map_err(|e| format!("{}: {e}", image.name))?;
    asset_format::write(path, &bytes).map_err(|e| e.to_string())
}

pub fn texture(image: &TextureImage) -> Option<Texture> {
    let format = match image.format {
        gx2::Format::Rgba8 { srgb } => Format::Rgba8 { srgb },
        gx2::Format::Bc1 { srgb } => Format::Bc1 { srgb },
        gx2::Format::Bc2 { srgb } => Format::Bc2 { srgb },
        gx2::Format::Bc3 { srgb } => Format::Bc3 { srgb },
        gx2::Format::Bc4 { signed } => Format::Bc4 { signed },
        gx2::Format::Bc5 { signed } => Format::Bc5 { signed },
        gx2::Format::Other(raw) => match raw & 0xFF {
            0x01 => Format::R8,
            0x07 => Format::Rg8,
            0x20 => Format::Rgba16Float,
            _ => return None,
        },
    };
    Some(Texture {
        format,
        width: image.width,
        height: image.height,
        layers: image.layers.max(1),
        mip_levels: image.mip_levels.max(1),
        data: image.data.clone(),
        swizzle: image.component_select,
    })
}
