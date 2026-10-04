//! The renderer's own textures (`Pack/Bootup_Graphics.pack` →
//! `Model/SystemModel.Tex2.sbfres`), which the deferred shading's materials
//! (`DeferredMain` in `SystemModel.sbfres`) sample.

use crate::bfres::{Bfres, bc};
use crate::content::ContentRoots;
use crate::{FormatError, Result};

pub const GRAPHICS_PACK: &str = "Pack/Bootup_Graphics.pack";
/// Inside [`GRAPHICS_PACK`].
pub const SYSTEM_TEXTURES: &str = "Model/SystemModel.Tex2.sbfres";
/// The noise the pre-shading fog shifts heights by: `sampler0` of the
/// `preshading_field*` materials (`uking_tex0` in their shaders).
pub const FOG_NOISE: &str = "cloud_noise";
/// The rotations the screen-space ambient occlusion turns its samples by:
/// `shadow` of the `preshading_shadow_field*` materials (`uking_tex0` in
/// their shaders), 4×4 R8G8.
pub const SSAO_NOISE: &str = "ssao";

/// A single-channel texture: `width × height` bytes, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GreyTexture {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<u8>,
}

/// Level 0 of the fog's noise ([`FOG_NOISE`], BC4); `None` without the pack.
pub fn load_fog_noise(roots: &ContentRoots) -> Result<Option<GreyTexture>> {
    with_system_textures(roots, |bfres| {
        let texture = bfres
            .texture(FOG_NOISE)?
            .ok_or(FormatError::Invalid("SystemModel.Tex2: no cloud_noise"))?;
        let (width, height) = (texture.surface.width, texture.surface.height);
        let blocks = texture.level0_slice(0)?;
        Ok(GreyTexture {
            width,
            height,
            texels: bc::decode_bc4(&blocks, width, height),
        })
    })
}

/// A two-channel texture: `width × height` pairs (red, green), row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgTexture {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<[u8; 2]>,
}

/// Level 0 of the ambient occlusion's rotations ([`SSAO_NOISE`], R8G8: the
/// shader reads red as x and green as w, each `2·v − 1`, a unit vector);
/// `None` without the pack.
pub fn load_ssao_noise(roots: &ContentRoots) -> Result<Option<RgTexture>> {
    with_system_textures(roots, |bfres| {
        let texture = bfres
            .texture(SSAO_NOISE)?
            .ok_or(FormatError::Invalid("SystemModel.Tex2: no ssao"))?;
        let (width, height) = (texture.surface.width, texture.surface.height);
        if texture.surface.format().bits_per_element() != Some(16)
            || texture.surface.format().is_block_compressed()
        {
            return Err(FormatError::Invalid("SystemModel.Tex2: ssao is not R8G8"));
        }
        let bytes = texture.level0_slice(0)?;
        rg_texels(&bytes, width, height).map(|texels| RgTexture {
            width,
            height,
            texels,
        })
    })
}

/// Pairs of bytes of an untiled R8G8 image, checked against its size.
fn rg_texels(bytes: &[u8], width: u32, height: u32) -> Result<Vec<[u8; 2]>> {
    let count = width as usize * height as usize;
    if bytes.len() < count * 2 {
        return Err(FormatError::Invalid(
            "SystemModel.Tex2: R8G8 image shorter than its size",
        ));
    }
    Ok(bytes[..count * 2]
        .chunks_exact(2)
        .map(|p| [p[0], p[1]])
        .collect())
}

/// Runs `read` on `SystemModel.Tex2` of [`GRAPHICS_PACK`]; `None` without
/// the pack or the file in it.
fn with_system_textures<T>(
    roots: &ContentRoots,
    read: impl FnOnce(&Bfres) -> Result<T>,
) -> Result<Option<T>> {
    let Some(path) = roots.find(GRAPHICS_PACK) else {
        return Ok(None);
    };
    let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
    let pack = crate::yaz0::decompress_if(&pack)?;
    let sarc = roead::sarc::Sarc::new(&pack[..])?;
    let Some(data) = sarc.get_data(SYSTEM_TEXTURES) else {
        return Ok(None);
    };
    let data = crate::yaz0::decompress_if(data)?;
    let bfres = Bfres::parse(&data)?;
    read(&bfres).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rg_texels_pair_bytes_row_by_row() {
        let texels = rg_texels(&[1, 2, 3, 4, 5, 6, 7, 8, 9], 2, 2).unwrap();
        assert_eq!(texels, vec![[1, 2], [3, 4], [5, 6], [7, 8]]);
    }

    #[test]
    fn rg_texels_reject_a_short_image() {
        assert!(rg_texels(&[1, 2, 3], 2, 1).is_err());
    }
}
