//! BFRES, the Wii U version (FRES 3.x/4.x, big-endian): models, textures
//! and animations. Only what this project uses is read.
//!
//! Every pointer in the file is an `i32` offset relative to where it is
//! stored; 0 means null. Resources are listed in index groups (patricia
//! trees): a header (`u32` byte length, `i32` count) and `count + 1` 16-byte
//! entries, the first being the root: `u32` search value, `u16` left,
//! `u16` right, name pointer, data pointer.
//!
//! Layout notes are in archived FORMATS.md notes; offsets were checked against
//! `Terrain.Tex1.sbfres` from the Wii U dump.

pub mod anim;
pub mod bc;
pub mod gx2;
pub mod maps;
pub mod model;

use crate::{FormatError, Result};
pub use anim::SkeletalAnim;
pub use gx2::Surface;
pub use model::Model;

/// A parsed BFRES file borrowing its bytes.
pub struct Bfres<'a> {
    data: &'a [u8],
    pub name: String,
    pub version: u32,
    models: Vec<(String, usize)>,
    textures: Vec<(String, usize)>,
    skeletal_anims: Vec<(String, usize)>,
}

/// An FTEX: a texture with its GX2 surface and raw (swizzled) data.
#[derive(Clone, Debug)]
pub struct Texture<'a> {
    pub name: String,
    pub surface: Surface,
    /// Swizzled image data of mip level 0 (all array slices).
    pub image: &'a [u8],
    /// Swizzled data of mip levels 1.. (all slices), if any.
    pub mips: &'a [u8],
    /// `(key, value)` pairs from the texture's user data, as text.
    pub user_data: Vec<(String, String)>,
    /// Sources of R, G, B, A (0–3 channel, 4 zero, 5 one).
    pub component_select: [u8; 4],
    /// The data at the image pointer, `mip_size` long: in a `*.Tex2` file
    /// this is where the texture's mips are.
    pub at_image_pointer_mips: &'a [u8],
}

impl Texture<'_> {
    /// Mip level 0 of one array slice (slice 0 for plain 2D textures), with
    /// the tiling undone: elements (pixels or 4×4 blocks) row by row.
    pub fn level0_slice(&self, slice: u32) -> Result<Vec<u8>> {
        if self.image.is_empty() {
            return Err(FormatError::Invalid(
                "bfres: this file does not hold the texture's image data",
            ));
        }
        let level = gx2::level(&self.surface, 0)
            .ok_or(FormatError::Invalid("gx2: unsupported surface format"))?;
        gx2::deswizzle(&self.surface, &level, self.image, slice)
    }

    /// Mip level `level` ≥ 1 of one slice, untiled, if this file holds the mips.
    pub fn mip_slice(&self, level: u32, slice: u32) -> Result<Option<Vec<u8>>> {
        if self.mips.is_empty() || level >= self.surface.mip_count {
            return Ok(None);
        }
        let layout = gx2::level(&self.surface, level)
            .ok_or(FormatError::Invalid("gx2: unsupported surface format"))?;
        let data = self.mips.get(layout.offset..).unwrap_or_default();
        gx2::deswizzle(&self.surface, &layout, data, slice).map(Some)
    }
}

/// A texture ready for the GPU: every level untiled, mip-major (all slices
/// of level 0, then of level 1, …).
#[derive(Clone, Debug)]
pub struct TextureImage {
    pub name: String,
    pub format: gx2::Format,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub mip_levels: u32,
    pub data: Vec<u8>,
    /// GX2 component selection (bytes: source of R, G, B, A; 0–3 = channel,
    /// 4 = zero, 5 = one).
    pub component_select: [u8; 4],
}

impl TextureImage {
    /// Size in pixels of mip `level`.
    pub fn level_size(&self, level: u32) -> (u32, u32) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }

    /// Bytes of one layer of mip `level`.
    pub fn level_bytes(&self, level: u32) -> usize {
        let (w, h) = self.level_size(level);
        let bpp = self.format.bits_per_element().unwrap_or(32) as usize;
        if self.format.is_block_compressed() {
            w.div_ceil(4) as usize * h.div_ceil(4) as usize * bpp / 8
        } else {
            w as usize * h as usize * bpp / 8
        }
    }

    /// Mip `level` of layer 0 in the stored format.
    pub fn level_data(&self, level: u32) -> Option<&[u8]> {
        self.layer_data(level, 0)
    }

    /// Mip `level` of one array layer.
    pub fn layer_data(&self, level: u32, layer: u32) -> Option<&[u8]> {
        let size = self.level_bytes(level);
        let start: usize = (0..level)
            .map(|l| self.level_bytes(l) * self.layers as usize)
            .sum::<usize>()
            + size * layer as usize;
        self.data.get(start..start + size)
    }

    /// Mip `level` of layer 0 as RGBA8 (BC4 and R8 go to grey, BC5 and
    /// R8G8 to red and green), for formats this crate decodes.
    pub fn decode_rgba8(&self, level: u32) -> Option<Vec<u8>> {
        self.decode_layer_rgba8(level, 0)
    }

    /// Mip `level` of one array layer as RGBA8, see [`Self::decode_rgba8`].
    pub fn decode_layer_rgba8(&self, level: u32, layer: u32) -> Option<Vec<u8>> {
        let data = self.layer_data(level, layer)?;
        let (w, h) = self.level_size(level);
        Some(match self.format {
            gx2::Format::Rgba8 { .. } => data.to_vec(),
            gx2::Format::Bc1 { .. } => bc::decode_bc1(data, w, h),
            gx2::Format::Bc3 { .. } => bc::decode_bc3(data, w, h),
            // SI-FMT-10: texture mips and BC conversions are ours.
            gx2::Format::Bc4 { .. } => bc::decode_bc4(data, w, h)
                .into_iter()
                .flat_map(|v| [v, v, v, 255])
                .collect(),
            gx2::Format::Bc5 { .. } => bc::decode_bc5(data, w, h),
            gx2::Format::Other(raw) if raw & 0xFF == 0x01 => {
                data.iter().flat_map(|&v| [v, v, v, 255]).collect()
            }
            gx2::Format::Other(raw) if raw & 0xFF == 0x07 => {
                data.chunks(2).flat_map(|p| [p[0], p[1], 0, 255]).collect()
            }
            _ => return None,
        })
    }
}

/// The top-left `wanted` elements of a `stored`-sized element grid.
fn crop(data: &[u8], stored: (u32, u32), wanted: (u32, u32), bytes: usize) -> Vec<u8> {
    if stored == wanted {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(wanted.0 as usize * wanted.1 as usize * bytes);
    for y in 0..wanted.1 as usize {
        let start = y * stored.0 as usize * bytes;
        let row = data.get(start..start + wanted.0 as usize * bytes);
        out.extend_from_slice(row.unwrap_or(&vec![0; wanted.0 as usize * bytes]));
    }
    out
}

/// Assembles a texture from the files that hold its parts. Wii U BotW
/// keeps mip level 0 in `*.Tex1` (base game) and the smaller levels in
/// `*.Tex2` (update). In `Tex1` the images are packed back to back and the
/// mip pointer is just the next image; in `Tex2` the *image* pointer is
/// where the mips are (packed back to back), and the mip pointer leads
/// nowhere. BC1 textures without a `Tex2` part get rebuilt mips; others keep
/// a single level.
pub fn assemble_texture(level0: &Texture, mips: Option<&Texture>) -> Result<TextureImage> {
    let base = level0;
    if base.image.is_empty() {
        return Err(FormatError::Invalid("bfres: no image data for level 0"));
    }
    let s = &base.surface;
    let layers = if matches!(s.dim, 4 | 5 | 7) {
        s.depth.max(1)
    } else {
        1
    };
    let mut data = Vec::new();
    let mut level0 = Vec::with_capacity(layers as usize);
    for layer in 0..layers {
        level0.push(base.level0_slice(layer)?);
    }
    for slice in &level0 {
        data.extend_from_slice(slice);
    }
    let mut mip_levels = 1;
    let with_mips =
        mips.filter(|t| !t.at_image_pointer_mips.is_empty() && t.surface.mip_count == s.mip_count);
    let bpp = s.format().bits_per_element().unwrap_or(32);
    let block = if s.format().is_block_compressed() {
        4
    } else {
        1
    };
    if let Some(source) = with_mips {
        for level in 1..s.mip_count {
            let layout = gx2::level(s, level)
                .ok_or(FormatError::Invalid("gx2: unsupported surface format"))?;
            // GX2 pads mips to powers of two; GPUs expect the true size.
            let stored = (layout.width.div_ceil(block), layout.height.div_ceil(block));
            let wanted = (
                (s.width >> level).max(1).div_ceil(block),
                (s.height >> level).max(1).div_ceil(block),
            );
            let level_data = source
                .at_image_pointer_mips
                .get(layout.offset..)
                .unwrap_or_default();
            for layer in 0..layers {
                let slice = gx2::deswizzle(s, &layout, level_data, layer)?;
                data.extend_from_slice(&crop(&slice, stored, wanted, (bpp / 8) as usize));
            }
            mip_levels += 1;
        }
    // SI-FMT-10: texture mips and BC conversions are ours.
    } else if matches!(s.format(), gx2::Format::Bc1 { .. }) {
        let rebuilt: Vec<Vec<Vec<u8>>> = level0
            .iter()
            .map(|l0| bc::bc1_mips(l0, s.width, s.height))
            .collect();
        let levels = rebuilt.first().map_or(0, Vec::len);
        for level in 0..levels {
            for layer in &rebuilt {
                data.extend_from_slice(&layer[level]);
            }
        }
        mip_levels += levels as u32;
    }
    Ok(TextureImage {
        name: base.name.clone(),
        format: s.format(),
        width: s.width,
        height: s.height,
        layers,
        mip_levels,
        data,
        component_select: base.component_select,
    })
}

/// Assembles a texture whose file holds all its levels (the environment's
/// `collect.genvres`, unlike the models' `Tex1`/`Tex2` pairs): level 0 at
/// the image pointer, the smaller levels at the mip pointer.
pub fn assemble_whole_texture(texture: &Texture) -> Result<TextureImage> {
    let mut image = assemble_texture(texture, None)?;
    let s = &texture.surface;
    if image.mip_levels > 1 || texture.mips.is_empty() {
        return Ok(image);
    }
    let bpp = s.format().bits_per_element().unwrap_or(32);
    let block = if s.format().is_block_compressed() {
        4
    } else {
        1
    };
    for level in 1..s.mip_count {
        let layout =
            gx2::level(s, level).ok_or(FormatError::Invalid("gx2: unsupported surface format"))?;
        let stored = (layout.width.div_ceil(block), layout.height.div_ceil(block));
        let wanted = (
            (s.width >> level).max(1).div_ceil(block),
            (s.height >> level).max(1).div_ceil(block),
        );
        let mut slices = Vec::new();
        for layer in 0..image.layers {
            let Some(slice) = texture.mip_slice(level, layer)? else {
                return Ok(image);
            };
            slices.extend_from_slice(&crop(&slice, stored, wanted, (bpp / 8) as usize));
        }
        image.data.extend_from_slice(&slices);
        image.mip_levels += 1;
    }
    Ok(image)
}

impl<'a> Bfres<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.get(..4) != Some(b"FRES") {
            return Err(FormatError::Invalid("bfres: missing FRES magic"));
        }
        let r = Reader(data);
        if r.u16(0x08)? != 0xFEFF {
            return Err(FormatError::Invalid(
                "bfres: only big-endian (Wii U) files are supported",
            ));
        }
        let version = r.u32(0x04)?;
        let name = r.string_at(0x14)?.unwrap_or_default();
        let models = r.index_group(0x20)?;
        let textures = r.index_group(0x24)?;
        let skeletal_anims = r.index_group(0x28)?;
        Ok(Self {
            data,
            name,
            version,
            models,
            textures,
            skeletal_anims,
        })
    }

    pub fn texture_names(&self) -> impl Iterator<Item = &str> {
        self.textures.iter().map(|(name, _)| name.as_str())
    }

    pub fn textures(&self) -> Result<Vec<Texture<'a>>> {
        self.textures
            .iter()
            .map(|(_, at)| self.texture_at(*at))
            .collect()
    }

    pub fn texture(&self, name: &str) -> Result<Option<Texture<'a>>> {
        self.textures
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, at)| self.texture_at(*at))
            .transpose()
    }

    pub fn models(&self) -> Result<Vec<Model>> {
        self.models
            .iter()
            .map(|(_, at)| model::read_model(&Reader(self.data), *at))
            .collect()
    }

    pub fn skeletal_anim_names(&self) -> impl Iterator<Item = &str> {
        self.skeletal_anims.iter().map(|(name, _)| name.as_str())
    }

    pub fn skeletal_anim(&self, name: &str) -> Result<Option<SkeletalAnim>> {
        self.skeletal_anims
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, at)| anim::read_skeletal_anim(&Reader(self.data), *at))
            .transpose()
    }

    fn texture_at(&self, at: usize) -> Result<Texture<'a>> {
        let r = Reader(self.data);
        if self.data.get(at..at + 4) != Some(b"FTEX") {
            return Err(FormatError::Invalid("bfres: texture without FTEX magic"));
        }
        let surface = Surface::read(&r, at + 4)?;
        let name = r.string_at(at + 0xA8)?.unwrap_or_default();
        // Wii U BotW splits textures across files (see `assemble_texture`);
        // pointers into the other file's part may point past this file's end.
        let image = match r.pointer(at + 0xB0)? {
            Some(start) => r
                .slice(start, surface.image_size as usize)
                .unwrap_or_default(),
            None => &[],
        };
        let mips = match r.pointer(at + 0xB4)? {
            Some(start) if surface.mip_size > 0 => r
                .slice(start, surface.mip_size as usize)
                .unwrap_or_default(),
            _ => &[],
        };
        let user_data = match r.pointer(at + 0xB8)? {
            Some(group) => r.user_data(group)?,
            None => Vec::new(),
        };
        let component_select = r.u32(at + 0x88)?.to_be_bytes();
        let at_image_pointer_mips = match r.pointer(at + 0xB0)? {
            Some(start) if surface.mip_size > 0 => r
                .slice(start, surface.mip_size as usize)
                .unwrap_or_default(),
            _ => &[],
        };
        Ok(Texture {
            name,
            surface,
            image,
            mips,
            user_data,
            component_select,
            at_image_pointer_mips,
        })
    }
}

/// Bounds-checked big-endian reads over the whole file.
#[derive(Clone, Copy)]
pub(crate) struct Reader<'a>(pub &'a [u8]);

impl<'a> Reader<'a> {
    pub fn slice(&self, at: usize, len: usize) -> Result<&'a [u8]> {
        self.0
            .get(at..at + len)
            .ok_or(FormatError::Invalid("bfres: read past the end of the file"))
    }

    pub fn u8(&self, at: usize) -> Result<u8> {
        Ok(self.slice(at, 1)?[0])
    }

    pub fn u16(&self, at: usize) -> Result<u16> {
        Ok(u16::from_be_bytes(self.slice(at, 2)?.try_into().unwrap()))
    }

    pub fn u32(&self, at: usize) -> Result<u32> {
        Ok(u32::from_be_bytes(self.slice(at, 4)?.try_into().unwrap()))
    }

    pub fn i32(&self, at: usize) -> Result<i32> {
        Ok(self.u32(at)? as i32)
    }

    pub fn f32(&self, at: usize) -> Result<f32> {
        Ok(f32::from_bits(self.u32(at)?))
    }

    /// The target of the self-relative pointer stored at `at`, or `None`.
    pub fn pointer(&self, at: usize) -> Result<Option<usize>> {
        let offset = self.i32(at)?;
        if offset == 0 {
            return Ok(None);
        }
        let target = at as i64 + i64::from(offset);
        usize::try_from(target)
            .map(Some)
            .map_err(|_| FormatError::Invalid("bfres: pointer before the file start"))
    }

    /// A NUL-terminated string at `at`.
    pub fn string(&self, at: usize) -> Result<String> {
        let tail = self
            .0
            .get(at..)
            .ok_or(FormatError::Invalid("bfres: string past the end"))?;
        let end = tail
            .iter()
            .position(|&b| b == 0)
            .ok_or(FormatError::Invalid("bfres: unterminated string"))?;
        Ok(String::from_utf8_lossy(&tail[..end]).into_owned())
    }

    /// The string a pointer at `at` points to.
    pub fn string_at(&self, at: usize) -> Result<Option<String>> {
        self.pointer(at)?.map(|p| self.string(p)).transpose()
    }

    /// Entries of the index group the pointer at `at` points to, as
    /// `(name, data offset)`, in file order.
    pub fn index_group(&self, at: usize) -> Result<Vec<(String, usize)>> {
        let Some(group) = self.pointer(at)? else {
            return Ok(Vec::new());
        };
        let count = self.i32(group + 4)?.max(0) as usize;
        (1..=count)
            .map(|i| {
                let entry = group + 8 + i * 16;
                let name = self.string_at(entry + 8)?.unwrap_or_default();
                let data = self.pointer(entry + 12)?.ok_or(FormatError::Invalid(
                    "bfres: index group entry without data",
                ))?;
                Ok((name, data))
            })
            .collect()
    }

    /// User data (FRES `_USD` entries) as text: name, type, values.
    fn user_data(&self, group: usize) -> Result<Vec<(String, String)>> {
        let count = self.i32(group + 4)?.max(0) as usize;
        (1..=count)
            .map(|i| {
                let entry = group + 8 + i * 16;
                let name = self.string_at(entry + 8)?.unwrap_or_default();
                let Some(data) = self.pointer(entry + 12)? else {
                    return Ok((name, String::new()));
                };
                // name ptr, u16 count, u8 type, pad, values
                let n = self.u16(data + 4)? as usize;
                let kind = self.u8(data + 6)?;
                let values: Vec<String> = (0..n)
                    .map(|k| {
                        let v = data + 8 + k * 4;
                        Ok(match kind {
                            0 => self.i32(v)?.to_string(),
                            1 => self.f32(v)?.to_string(),
                            2 | 3 => self.string_at(v)?.unwrap_or_default(),
                            _ => format!("{:#x}", self.u32(v)?),
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok((name, values.join(",")))
            })
            .collect()
    }
}
