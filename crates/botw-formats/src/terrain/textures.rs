//! The terrain's material textures, ready for a GPU texture array.
//!
//! `Model/Terrain.Tex1.sbfres` holds `MaterialAlb`: 83 layers of 1024×1024
//! BC1 (sRGB) albedo, mip level 0 only (the dump has no smaller levels for
//! it, so they are rebuilt). Its user data lists the 88 terrain materials:
//! `file` (texture name), `array_index` (the layer). `.mate` samples index
//! these materials; `MainField.tscb` gives each material's UV scale.
//!
//! The same file has the grass blades (`GrassAlb`), tree impostors
//! (`Tree0Alb`, `Tree1Alb`), water normals and more; [`load_textures`]
//! reads any of them.

use crate::bfres::model::Material;
use crate::bfres::{Bfres, TextureImage, assemble_texture, bc};
use crate::content::ContentRoots;
use crate::{FormatError, Result};

pub const TERRAIN_TEX1: &str = "Model/Terrain.Tex1.sbfres";
/// The pack that holds `Terrain.Tex2` (and more always-loaded data).
const TITLE_BG_PACK: &str = "Pack/TitleBG.pack";
/// Inside [`TITLE_BG_PACK`].
pub const TERRAIN_TEX2: &str = "Model/Terrain.Tex2.sbfres";
/// Inside [`TITLE_BG_PACK`]: the terrain's models, `TeraWater` among them.
const TERRAIN_MODELS: &str = "Model/Terrain.sbfres";
/// The model whose single material (`Translucent`) draws the water surface.
pub const WATER_MODEL: &str = "TeraWater";
/// The model whose materials draw the grass (`Blade1`, `Blade2`, `Cross1`, …).
pub const GRASS_MODEL: &str = "TeraGrass";

/// A BC1 texture array with a full mip chain.
#[derive(Clone, Debug)]
pub struct TextureArray {
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub mip_levels: u32,
    pub srgb: bool,
    /// Layer-major: every mip level of layer 0, then of layer 1, …
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct TerrainTextures {
    pub albedo: TextureArray,
    /// `MaterialCmb`, the same layers' normals: X and Y in red and green
    /// (blue holds something else). 512×512, not sRGB.
    pub normals: Option<TextureArray>,
    /// Texture layer of each material index.
    pub material_layers: Vec<u32>,
    pub material_names: Vec<String>,
}

/// Named textures from `Terrain.Tex1`, in the order asked (`None` where the
/// file has no such texture). The file is read once.
pub fn load_textures(roots: &ContentRoots, names: &[&str]) -> Result<Vec<Option<TextureImage>>> {
    let path = roots
        .find(TERRAIN_TEX1)
        .ok_or(FormatError::Invalid("Model/Terrain.Tex1.sbfres not found"))?;
    let bytes = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
    let bytes = crate::yaz0::decompress_if(&bytes)?;
    let bfres = Bfres::parse(&bytes)?;
    names
        .iter()
        .map(|name| match bfres.texture(name)? {
            Some(texture) => assemble_texture(&texture, None).map(Some),
            None => Ok(None),
        })
        .collect()
}

impl TerrainTextures {
    pub fn load(roots: &ContentRoots) -> Result<Self> {
        let path = roots
            .find(TERRAIN_TEX1)
            .ok_or(FormatError::Invalid("Model/Terrain.Tex1.sbfres not found"))?;
        let bytes = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
        let bytes = crate::yaz0::decompress_if(&bytes)?;
        let bfres = Bfres::parse(&bytes)?;
        let texture = bfres
            .texture("MaterialAlb")?
            .ok_or(FormatError::Invalid("Terrain.Tex1: no MaterialAlb"))?;

        let list = |key: &str| -> Vec<String> {
            texture
                .user_data
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.split(',').map(str::to_owned).collect())
                .unwrap_or_default()
        };
        let material_layers: Vec<u32> = list("array_index")
            .iter()
            .map(|s| s.parse().unwrap_or(0))
            .collect();
        let material_names = list("file");
        let albedo = build_array(&texture, texture.surface.depth)?;
        let normals = match bfres.texture("MaterialCmb")? {
            Some(normals) => Some(build_array(&normals, normals.surface.depth)?),
            None => None,
        };
        Ok(Self {
            albedo,
            normals,
            material_layers,
            material_names,
        })
    }
}

/// Untiles every slice of a BC1 array texture's level 0 and rebuilds the
/// mips, spreading slices over threads.
pub fn build_array(texture: &crate::bfres::Texture, layers: u32) -> Result<TextureArray> {
    let s = &texture.surface;
    let srgb = match s.format() {
        crate::bfres::gx2::Format::Bc1 { srgb } => srgb,
        _ => return Err(FormatError::Invalid("terrain textures: expected BC1")),
    };
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(layers.max(1) as usize);
    let per_thread = (layers as usize).div_ceil(threads);
    let chunks: Vec<Result<Vec<Vec<u8>>>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                scope.spawn(move || {
                    let range =
                        (t * per_thread) as u32..(((t + 1) * per_thread) as u32).min(layers);
                    range
                        .map(|layer| {
                            let level0 = texture.level0_slice(layer)?;
                            let mut all = level0.clone();
                            // SI-FMT-10: texture mips and BC conversions are ours.
                            for level in bc::bc1_mips(&level0, s.width, s.height) {
                                all.extend_from_slice(&level);
                            }
                            Ok(all)
                        })
                        .collect::<Result<Vec<_>>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("texture thread panicked"))
            .collect()
    });
    let mut data = Vec::new();
    for chunk in chunks {
        for layer in chunk? {
            data.extend_from_slice(&layer);
        }
    }
    let mip_levels = 32 - s.width.max(s.height).leading_zeros();
    Ok(TextureArray {
        width: s.width,
        height: s.height,
        layers,
        mip_levels,
        srgb,
        data,
    })
}

impl TextureArray {
    /// Each layer's mean colour over its level 0, linear 0-1 (sRGB texels
    /// decoded first). The game reads a terrain material's mean colour back
    /// from the GPU for the grass (`grass_color`, see
    /// `docs/research/wiiu-field-shading.md`); this stands in for the
    /// 1×1 mip it most likely samples.
    pub fn layer_means(&self) -> Vec<[f32; 3]> {
        let blocks = (self.width.div_ceil(4) * self.height.div_ceil(4)) as usize;
        let stride = self.data.len() / self.layers.max(1) as usize;
        let decode: [f32; 256] = std::array::from_fn(|v| {
            let c = v as f32 / 255.0;
            if !self.srgb {
                c
            } else if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        });
        (0..self.layers as usize)
            .map(|layer| {
                let level0 = &self.data[layer * stride..][..blocks * 8];
                let mut sum = [0.0f64; 3];
                for block in level0.as_chunks::<8>().0 {
                    for texel in bc::decode_bc1_block(block) {
                        for c in 0..3 {
                            sum[c] += decode[texel[c] as usize] as f64;
                        }
                    }
                }
                let texels = (blocks * 16) as f64;
                sum.map(|s| (s / texels) as f32)
            })
            .collect()
    }
}

/// Texels per water kind in `WaterAlb`.
pub const WATER_TABLE_TEXELS: usize = 7;
/// Water kinds (layers) in `WaterAlb`, `WaterNrm` and `WaterEmm`.
pub const WATER_KINDS: usize = 8;

/// `WaterAlb`: a 7×1 RGBA16F texture array, one layer per water kind (the
/// kinds of `.water.extm`). On Wii U the game reads it back on the CPU and
/// hands the RGB of each kind's seven texels to the water shader
/// (`uking_terrain_water`) as uniforms; what each texel does there is in
/// `docs/research/wiiu-water-variants.md` (e.g. texel 1 the deep colour,
/// texels 2 and 3 the opacity's rate and offset per metre of water, texel
/// 6 the vertex waves). The texels match the material's constants per kind
/// (the Lava layer equals `TeraWater`'s own `const_color3/4/5` and
/// `const_value0/1`, the Sea layer those of the sea horizon model).
///
/// Values are linear and as stored.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct WaterTable {
    pub kinds: [[[f32; 4]; WATER_TABLE_TEXELS]; WATER_KINDS],
}

impl WaterTable {
    /// Reads `WaterAlb` from `Pack/TitleBG.pack` → `Model/Terrain.Tex2.sbfres`
    /// (the only file that holds its image); `None` without it.
    pub fn load(roots: &ContentRoots) -> Result<Option<Self>> {
        let Some(bytes) = title_bg_file(roots, TERRAIN_TEX2)? else {
            return Ok(None);
        };
        let bfres = Bfres::parse(&bytes)?;
        let Some(texture) = bfres.texture("WaterAlb")? else {
            return Ok(None);
        };
        Self::from_texture(&texture).map(Some)
    }

    /// Decodes the table from its FTEX.
    pub fn from_texture(texture: &crate::bfres::Texture) -> Result<Self> {
        let s = &texture.surface;
        if s.format().bits_per_element() != Some(64)
            || (s.width as usize) < WATER_TABLE_TEXELS
            || (s.depth as usize) < WATER_KINDS
        {
            return Err(FormatError::Invalid("WaterAlb: expected 7×1×8 RGBA16F"));
        }
        let rows = (0..WATER_KINDS as u32)
            .map(|layer| texture.level0_slice(layer))
            .collect::<Result<Vec<_>>>()?;
        Self::from_rows(&rows)
    }

    /// The table from each layer's untiled first row: RGBA halves,
    /// little-endian like all GX2 surface data (unlike the big-endian rest
    /// of the file).
    pub fn from_rows(rows: &[Vec<u8>]) -> Result<Self> {
        if rows.len() < WATER_KINDS {
            return Err(FormatError::Invalid("WaterAlb: fewer than 8 layers"));
        }
        let mut kinds = [[[0.0; 4]; WATER_TABLE_TEXELS]; WATER_KINDS];
        for (kind, row) in kinds.iter_mut().zip(rows) {
            let halves = row
                .get(..WATER_TABLE_TEXELS * 8)
                .ok_or(FormatError::Invalid("WaterAlb: short row"))?;
            for (texel, bytes) in kind.iter_mut().zip(halves.as_chunks::<8>().0) {
                for (value, half) in texel.iter_mut().zip(bytes.as_chunks::<2>().0) {
                    *value = crate::bfres::model::half_to_f32(u16::from_le_bytes(*half));
                }
            }
        }
        Ok(Self { kinds })
    }
}

/// `TeraWater`'s material from `Pack/TitleBG.pack` → `Model/Terrain.sbfres`
/// (its texture matrices and constants feed the water shader); `None`
/// without the pack or the file.
pub fn load_water_material(roots: &ContentRoots) -> Result<Option<Material>> {
    let Some(materials) = load_terrain_model_materials(roots, WATER_MODEL)? else {
        return Ok(None);
    };
    materials
        .into_iter()
        .next()
        .ok_or(FormatError::Invalid("TeraWater: no material"))
        .map(Some)
}

/// The materials of `model` in `Pack/TitleBG.pack` → `Model/Terrain.sbfres`
/// ([`WATER_MODEL`], [`GRASS_MODEL`]); `None` without the pack or the file.
pub fn load_terrain_model_materials(
    roots: &ContentRoots,
    model: &str,
) -> Result<Option<Vec<Material>>> {
    let Some(bytes) = title_bg_file(roots, TERRAIN_MODELS)? else {
        return Ok(None);
    };
    let models = Bfres::parse(&bytes)?.models()?;
    let model = models
        .into_iter()
        .find(|m| m.name == model)
        .ok_or(FormatError::Invalid("Terrain.sbfres: no such model"))?;
    Ok(Some(model.materials))
}

/// File `name` from `Pack/TitleBG.pack`, decompressed; `None` if either
/// is missing.
fn title_bg_file(roots: &ContentRoots, name: &str) -> Result<Option<Vec<u8>>> {
    let Some(path) = roots.find(TITLE_BG_PACK) else {
        return Ok(None);
    };
    let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
    let pack = crate::yaz0::decompress_if(&pack)?;
    let sarc = roead::sarc::Sarc::new(&pack[..])?;
    let Some(data) = sarc.get_data(name) else {
        return Ok(None);
    };
    Ok(Some(crate::yaz0::decompress_if(data)?.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_means_average_level_zero_in_linear_light() {
        let white = [0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0];
        let black = [0u8; 8];
        let mip = [0xab; 8];
        // Two 8×4 layers of two blocks each, every layer followed by a
        // (garbage) smaller level that must not count.
        let data = [&white[..], &black, &mip, &white, &white, &mip].concat();
        let array = TextureArray {
            width: 8,
            height: 4,
            layers: 2,
            mip_levels: 2,
            srgb: true,
            data,
        };
        let means = array.layer_means();
        assert_eq!(means.len(), 2);
        for c in 0..3 {
            assert!(
                (means[0][c] - 0.5).abs() < 1e-6,
                "half white, half black: {:?}",
                means[0]
            );
            assert!((means[1][c] - 1.0).abs() < 1e-6);
        }
        // A mid-grey sRGB texel (130 of 255) is about 0.22 in linear light.
        let grey = [0x10, 0x84, 0x10, 0x84, 0, 0, 0, 0];
        let array = TextureArray {
            width: 4,
            height: 4,
            layers: 1,
            mip_levels: 1,
            srgb: true,
            data: grey.to_vec(),
        };
        assert!(
            (array.layer_means()[0][1] - 0.223).abs() < 0.005,
            "{:?}",
            array.layer_means()
        );
    }

    #[test]
    fn water_table_rows_are_little_endian_halves() {
        let half = |v: usize| -> [u8; 2] { [[0x00, 0x00], [0x00, 0x3c], [0x00, 0x40]][v] };
        // Layer k, texel x = (x % 3, k % 3, 1, 0) as halves, padded to the
        // tile's row of 8 texels.
        let rows: Vec<Vec<u8>> = (0..WATER_KINDS)
            .map(|k| {
                let mut row: Vec<u8> = (0..WATER_TABLE_TEXELS)
                    .flat_map(|x| [half(x % 3), half(k % 3), half(1), half(0)].concat())
                    .collect();
                row.resize(8 * 8, 0xee);
                row
            })
            .collect();
        let table = WaterTable::from_rows(&rows).unwrap();
        assert_eq!(table.kinds[0][1], [1.0, 0.0, 1.0, 0.0]);
        assert_eq!(table.kinds[5][6], [0.0, 2.0, 1.0, 0.0]);
        assert!(WaterTable::from_rows(&rows[..3]).is_err());
        assert!(WaterTable::from_rows(&vec![vec![0; 8]; WATER_KINDS]).is_err());
    }
}
