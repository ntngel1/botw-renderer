//! The precomputed atmosphere tables of the sky (`Pack/Bootup_Graphics.pack`
//! → `System/KSys/sky.skybin`, Wii U v208).
//!
//! The file is three GX2 RGBA16F surface images back to back with no header
//! (`SKY_LoadSkybinSurfaces`, `0x033f3558`, copies them into the surfaces
//! `SKY_ConstructSkyObject` creates): transmittance 256×64, inscatter
//! 256×32×16 and irradiance 64×64. The images are tiled. Only the inscatter
//! table is decoded here: it is the one the per-frame sky LUT bake
//! (`sky_bake_inscatter`, PS 413) samples, and its tile mode (7, 2D tiled
//! thick) is the one the untiled result supports; the tile modes of the two
//! 2D tables are not settled. See docs/research/wiiu-sky-resources.md.

use crate::bfres::gx2::{Level, Surface, deswizzle};
use crate::bfres::model::half_to_f32;
use crate::content::ContentRoots;
use crate::system_model::GRAPHICS_PACK;
use crate::{FormatError, Result};

/// Inside [`GRAPHICS_PACK`].
pub const SKYBIN: &str = "System/KSys/sky.skybin";

/// Bruneton's table resolutions as the game sizes the surfaces
/// (`FUN_03405f48` arguments): the inscatter table is `RES_NU` slices of
/// `RES_MU_S` texels across x, `RES_MU` rows and `RES_R` depth slices.
pub const RES_NU: u32 = 8;
pub const RES_MU_S: u32 = 32;
pub const RES_MU: u32 = 32;
pub const RES_R: u32 = 16;

const RGBA16F: u32 = 0x820;
const TEXEL_BYTES: usize = 8;
const TRANSMITTANCE_BYTES: usize = 256 * 64 * TEXEL_BYTES;
const INSCATTER_BYTES: usize = (RES_NU * RES_MU_S * RES_MU * RES_R) as usize * TEXEL_BYTES;
const IRRADIANCE_BYTES: usize = 64 * 64 * TEXEL_BYTES;
/// Observed: the untiled table is smooth only with this mode.
// SI-FMT-08: inscatter tile mode picked by smoothness.
const INSCATTER_TILE_MODE: u32 = 7;

/// The inscatter table: RGB Rayleigh inscatter and, in alpha, the red
/// channel of the Mie inscatter (Bruneton's packing).
#[derive(Clone, Debug, PartialEq)]
pub struct Inscatter {
    /// `width × height × depth` texels, x fastest, then y, then z.
    pub texels: Vec<[f32; 4]>,
}

impl Inscatter {
    pub const WIDTH: u32 = RES_NU * RES_MU_S;
    pub const HEIGHT: u32 = RES_MU;
    pub const DEPTH: u32 = RES_R;

    pub fn texel(&self, x: u32, y: u32, z: u32) -> [f32; 4] {
        self.texels[((z * Self::HEIGHT + y) * Self::WIDTH + x) as usize]
    }
}

/// Decodes the inscatter table of a whole `sky.skybin`.
pub fn parse_inscatter(skybin: &[u8]) -> Result<Inscatter> {
    let expected = TRANSMITTANCE_BYTES + INSCATTER_BYTES + IRRADIANCE_BYTES;
    if skybin.len() != expected {
        return Err(FormatError::WrongSize {
            what: "sky.skybin",
            expected,
            actual: skybin.len(),
        });
    }
    let image = &skybin[TRANSMITTANCE_BYTES..TRANSMITTANCE_BYTES + INSCATTER_BYTES];
    let (width, height, depth) = (Inscatter::WIDTH, Inscatter::HEIGHT, Inscatter::DEPTH);
    let surface = Surface {
        dim: 2,
        width,
        height,
        depth,
        mip_count: 1,
        format: RGBA16F,
        aa: 0,
        usage: 1,
        image_size: INSCATTER_BYTES as u32,
        mip_size: 0,
        tile_mode: INSCATTER_TILE_MODE,
        swizzle: 0,
        alignment: 0,
        pitch: width,
        mip_offsets: [0; 13],
    };
    // 256 × 32 × 16 fills whole macro tiles (32 × 16 × 4): no padding.
    let level = Level {
        width,
        height,
        pitch: width,
        rows: height,
        slices: 1,
        tile_mode: INSCATTER_TILE_MODE,
        offset: 0,
    };
    let mut texels = Vec::with_capacity((width * height * depth) as usize);
    for z in 0..depth {
        for texel in deswizzle(&surface, &level, image, z)?.chunks_exact(TEXEL_BYTES) {
            let c = |i: usize| half_to_f32(u16::from_le_bytes([texel[2 * i], texel[2 * i + 1]]));
            let value = [c(0), c(1), c(2), c(3)];
            if !value.iter().all(|v| v.is_finite()) {
                return Err(FormatError::Invalid(
                    "sky.skybin: non-finite inscatter texel",
                ));
            }
            texels.push(value);
        }
    }
    Ok(Inscatter { texels })
}

/// The inscatter table from the dump; `None` without the pack or the file.
pub fn load_inscatter(roots: &ContentRoots) -> Result<Option<Inscatter>> {
    let Some(path) = roots.find(GRAPHICS_PACK) else {
        return Ok(None);
    };
    let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
    let pack = crate::yaz0::decompress_if(&pack)?;
    let sarc = roead::sarc::Sarc::new(&pack[..])?;
    let Some(data) = sarc.get_data(SKYBIN) else {
        return Ok(None);
    };
    parse_inscatter(&crate::yaz0::decompress_if(data)?).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Binary16 bits of a small non-negative integer (exact below 2048).
    fn half_int(n: u32) -> u16 {
        if n == 0 {
            return 0;
        }
        let e = 31 - n.leading_zeros();
        ((e + 15) << 10 | ((n << 10 >> e) & 0x3ff)) as u16
    }

    /// A skybin whose inscatter image numbers its elements in storage
    /// order: red = index % 1024, green = index / 1024.
    fn numbered_skybin() -> Vec<u8> {
        let mut data = vec![0u8; TRANSMITTANCE_BYTES];
        for i in 0..(INSCATTER_BYTES / TEXEL_BYTES) as u32 {
            for channel in [half_int(i % 1024), half_int(i / 1024), 0, 0] {
                data.extend_from_slice(&channel.to_le_bytes());
            }
        }
        data.resize(data.len() + IRRADIANCE_BYTES, 0);
        data
    }

    #[test]
    fn untiling_uses_every_stored_texel_once() {
        let table = parse_inscatter(&numbered_skybin()).unwrap();
        let mut ids: Vec<u32> = table
            .texels
            .iter()
            .map(|t| t[0] as u32 + 1024 * t[1] as u32)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), INSCATTER_BYTES / TEXEL_BYTES);
        assert_eq!(
            *ids.last().unwrap() as usize,
            INSCATTER_BYTES / TEXEL_BYTES - 1
        );
    }

    #[test]
    fn other_tables_are_not_read_as_inscatter() {
        let mut data = numbered_skybin();
        let before = parse_inscatter(&data).unwrap();
        data[..TRANSMITTANCE_BYTES].fill(0x3c);
        let end = data.len();
        data[end - IRRADIANCE_BYTES..].fill(0x3c);
        assert_eq!(parse_inscatter(&data).unwrap(), before);
    }

    #[test]
    fn rejects_wrong_sizes_and_non_finite_texels() {
        let mut data = numbered_skybin();
        assert!(matches!(
            parse_inscatter(&data[1..]),
            Err(FormatError::WrongSize { .. })
        ));
        data.push(0);
        assert!(matches!(
            parse_inscatter(&data),
            Err(FormatError::WrongSize { .. })
        ));
        data.pop();
        let at = TRANSMITTANCE_BYTES + 6;
        data[at..at + 2].copy_from_slice(&0x7c00u16.to_le_bytes());
        assert!(matches!(
            parse_inscatter(&data),
            Err(FormatError::Invalid(_))
        ));
    }
}
