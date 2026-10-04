//! Textures as KTX2: the game's texel data as it is (no re-encoding),
//! every level untiled, plus the game's component selection as the
//! standard `KTXswizzle` key. Without supercompression, or with the
//! standard's Zstandard scheme (lossless, per level; the models'
//! textures, which only this crate reads).

use crate::{FormatError, Result};

/// Texel formats the game's textures come in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rgba8 { srgb: bool },
    Bc1 { srgb: bool },
    Bc2 { srgb: bool },
    Bc3 { srgb: bool },
    Bc4 { signed: bool },
    Bc5 { signed: bool },
    R8,
    Rg8,
    Rgba16Float,
}

impl Format {
    pub fn is_block_compressed(self) -> bool {
        matches!(
            self,
            Self::Bc1 { .. }
                | Self::Bc2 { .. }
                | Self::Bc3 { .. }
                | Self::Bc4 { .. }
                | Self::Bc5 { .. }
        )
    }

    /// Bytes per element: a pixel, or a 4×4 block.
    pub fn element_bytes(self) -> usize {
        match self {
            Self::R8 => 1,
            Self::Rg8 => 2,
            Self::Rgba8 { .. } => 4,
            Self::Rgba16Float => 8,
            Self::Bc1 { .. } | Self::Bc4 { .. } => 8,
            Self::Bc2 { .. } | Self::Bc3 { .. } | Self::Bc5 { .. } => 16,
        }
    }

    fn vk_format(self) -> u32 {
        match self {
            Self::R8 => 9,
            Self::Rg8 => 16,
            Self::Rgba8 { srgb: false } => 37,
            Self::Rgba8 { srgb: true } => 43,
            Self::Rgba16Float => 97,
            Self::Bc1 { srgb: false } => 133,
            Self::Bc1 { srgb: true } => 134,
            Self::Bc2 { srgb: false } => 135,
            Self::Bc2 { srgb: true } => 136,
            Self::Bc3 { srgb: false } => 137,
            Self::Bc3 { srgb: true } => 138,
            Self::Bc4 { signed: false } => 139,
            Self::Bc4 { signed: true } => 140,
            Self::Bc5 { signed: false } => 141,
            Self::Bc5 { signed: true } => 142,
        }
    }

    fn from_vk_format(vk: u32) -> Option<Self> {
        Some(match vk {
            9 => Self::R8,
            16 => Self::Rg8,
            37 => Self::Rgba8 { srgb: false },
            43 => Self::Rgba8 { srgb: true },
            97 => Self::Rgba16Float,
            133 => Self::Bc1 { srgb: false },
            134 => Self::Bc1 { srgb: true },
            135 => Self::Bc2 { srgb: false },
            136 => Self::Bc2 { srgb: true },
            137 => Self::Bc3 { srgb: false },
            138 => Self::Bc3 { srgb: true },
            139 => Self::Bc4 { signed: false },
            140 => Self::Bc4 { signed: true },
            141 => Self::Bc5 { signed: false },
            142 => Self::Bc5 { signed: true },
            _ => return None,
        })
    }

    fn srgb(self) -> bool {
        matches!(
            self,
            Self::Rgba8 { srgb: true }
                | Self::Bc1 { srgb: true }
                | Self::Bc2 { srgb: true }
                | Self::Bc3 { srgb: true }
        )
    }

    /// The Data Format Descriptor's colour model (KHR_DF_MODEL_*).
    fn color_model(self) -> u8 {
        match self {
            Self::Bc1 { .. } => 128,
            Self::Bc2 { .. } => 129,
            Self::Bc3 { .. } => 130,
            Self::Bc4 { .. } => 131,
            Self::Bc5 { .. } => 132,
            _ => 1, // RGBSDA
        }
    }
}

/// The game's component selection (GX2 `compSel`): per output channel R,
/// G, B, A its source, 0–3 a channel, 4 zero, 5 one.
pub type Swizzle = [u8; 4];

pub const IDENTITY_SWIZZLE: Swizzle = [0, 1, 2, 3];

/// A 2D texture or texture array, every level untiled, mip-major (all
/// layers of level 0, then of level 1, …).
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub format: Format,
    pub width: u32,
    pub height: u32,
    /// At least 1.
    pub layers: u32,
    pub mip_levels: u32,
    pub data: Vec<u8>,
    pub swizzle: Swizzle,
}

const IDENTIFIER: [u8; 12] = [
    0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A,
];
const SWIZZLE_KEY: &[u8] = b"KTXswizzle\0";
/// KTX2 supercompression scheme: Zstandard.
const ZSTD_SCHEME: u32 = 2;

impl Texture {
    /// Size in pixels of mip `level`.
    pub fn level_size(&self, level: u32) -> (u32, u32) {
        ((self.width >> level).max(1), (self.height >> level).max(1))
    }

    /// Bytes of one layer of mip `level`.
    pub fn level_bytes(&self, level: u32) -> usize {
        let (w, h) = self.level_size(level);
        let element = self.format.element_bytes();
        if self.format.is_block_compressed() {
            w.div_ceil(4) as usize * h.div_ceil(4) as usize * element
        } else {
            w as usize * h as usize * element
        }
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

    fn level_range(&self, level: u32) -> std::ops::Range<usize> {
        let start: usize = (0..level)
            .map(|l| self.level_bytes(l) * self.layers as usize)
            .sum();
        start..start + self.level_bytes(level) * self.layers as usize
    }

    pub fn to_ktx2(&self) -> Result<Vec<u8>> {
        self.encode(None)
    }

    /// KTX2 with each level compressed by Zstandard at `level`
    /// (supercompression scheme 2).
    pub fn to_ktx2_zstd(&self, level: i32) -> Result<Vec<u8>> {
        self.encode(Some(level))
    }

    fn encode(&self, zstd: Option<i32>) -> Result<Vec<u8>> {
        let expected = self.level_range(self.mip_levels.saturating_sub(1)).end;
        if self.data.len() != expected || self.layers == 0 || self.mip_levels == 0 {
            return Err(FormatError::Invalid(
                "texture: data does not match its size",
            ));
        }
        let levels = self.mip_levels as usize;
        let stored: Vec<std::borrow::Cow<[u8]>> = (0..levels)
            .map(|level| {
                let raw = &self.data[self.level_range(level as u32)];
                match zstd {
                    Some(z) => zstd::bulk::compress(raw, z)
                        .map(std::borrow::Cow::Owned)
                        .map_err(|e| FormatError::Compression(e.to_string())),
                    None => Ok(std::borrow::Cow::Borrowed(raw)),
                }
            })
            .collect::<Result<_>>()?;
        let header_len = 12 + 9 * 4 + 4 * 4 + 2 * 8;
        let index_len = levels * 24;
        let dfd = self.dfd(zstd.is_some());
        let dfd_offset = header_len + index_len;
        let kvd = self.key_values();
        let kvd_offset = dfd_offset + dfd.len();
        let mut end = kvd_offset + kvd.len();

        // Levels go in smallest first, as the spec asks, each aligned to
        // the element size (and 4); supercompressed levels need no
        // alignment.
        let align = if zstd.is_some() {
            1
        } else {
            self.format.element_bytes().max(4)
        };
        let mut offsets = vec![0usize; levels];
        for level in (0..levels).rev() {
            end = end.next_multiple_of(align);
            offsets[level] = end;
            end += stored[level].len();
        }

        let mut out = Vec::with_capacity(end);
        out.extend_from_slice(&IDENTIFIER);
        let layer_count = if self.layers > 1 { self.layers } else { 0 };
        for value in [
            self.format.vk_format(),
            1, // typeSize
            self.width,
            self.height,
            0, // pixelDepth
            layer_count,
            1, // faceCount
            levels as u32,
            if zstd.is_some() { ZSTD_SCHEME } else { 0 },
        ] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        for value in [dfd_offset, dfd.len(), kvd_offset, kvd.len()] {
            out.extend_from_slice(&(value as u32).to_le_bytes());
        }
        out.extend_from_slice(&[0; 16]); // no supercompression global data
        for (level, offset) in offsets.iter().enumerate() {
            let raw = self.level_range(level as u32).len();
            for value in [*offset, stored[level].len(), raw] {
                out.extend_from_slice(&(value as u64).to_le_bytes());
            }
        }
        out.extend_from_slice(&dfd);
        out.extend_from_slice(&kvd);
        for level in (0..levels).rev() {
            out.resize(offsets[level], 0);
            out.extend_from_slice(&stored[level]);
        }
        Ok(out)
    }

    /// One basic descriptor block with one sample over the whole element.
    fn dfd(&self, supercompressed: bool) -> Vec<u8> {
        let mut block = Vec::new();
        block.extend_from_slice(&0u32.to_le_bytes()); // Khronos, basic
        block.extend_from_slice(&2u16.to_le_bytes()); // version
        block.extend_from_slice(&40u16.to_le_bytes()); // 24 + 16 per sample
        let transfer = if self.format.srgb() { 2 } else { 1 };
        block.extend_from_slice(&[self.format.color_model(), 1, transfer, 0]);
        let dim = if self.format.is_block_compressed() {
            3
        } else {
            0
        };
        block.extend_from_slice(&[dim, dim, 0, 0]);
        let bytes = self.format.element_bytes() as u8;
        // bytesPlane0 is 0 for supercompressed data.
        let plane = if supercompressed { 0 } else { bytes };
        block.extend_from_slice(&[plane, 0, 0, 0, 0, 0, 0, 0]);
        block.extend_from_slice(&0u16.to_le_bytes());
        block.extend_from_slice(&[bytes * 8 - 1, 0]);
        block.extend_from_slice(&[0, 0, 0, 0]);
        block.extend_from_slice(&0u32.to_le_bytes());
        block.extend_from_slice(&u32::MAX.to_le_bytes());
        let mut dfd = ((block.len() + 4) as u32).to_le_bytes().to_vec();
        dfd.extend_from_slice(&block);
        dfd
    }

    fn key_values(&self) -> Vec<u8> {
        if self.swizzle == IDENTITY_SWIZZLE {
            return Vec::new();
        }
        let mut entry = SWIZZLE_KEY.to_vec();
        entry.extend(self.swizzle.map(|s| match s {
            0 => b'r',
            1 => b'g',
            2 => b'b',
            3 => b'a',
            4 => b'0',
            _ => b'1',
        }));
        entry.push(0);
        let mut kvd = (entry.len() as u32).to_le_bytes().to_vec();
        kvd.extend_from_slice(&entry);
        kvd.resize(kvd.len().next_multiple_of(4), 0);
        kvd
    }

    pub fn from_ktx2(bytes: &[u8]) -> Result<Self> {
        let invalid = |what| FormatError::Invalid(what);
        if bytes.get(..12) != Some(&IDENTIFIER[..]) {
            return Err(invalid("ktx2: missing identifier"));
        }
        let u32_at = |at: usize| -> Result<u32> {
            Ok(u32::from_le_bytes(
                bytes
                    .get(at..at + 4)
                    .ok_or(invalid("ktx2: truncated"))?
                    .try_into()
                    .expect("four bytes"),
            ))
        };
        let u64_at = |at: usize| -> Result<usize> {
            Ok(u64::from_le_bytes(
                bytes
                    .get(at..at + 8)
                    .ok_or(invalid("ktx2: truncated"))?
                    .try_into()
                    .expect("eight bytes"),
            ) as usize)
        };
        let format =
            Format::from_vk_format(u32_at(12)?).ok_or(invalid("ktx2: unsupported format"))?;
        let (width, height) = (u32_at(20)?, u32_at(24)?);
        let layers = u32_at(32)?.max(1);
        if u32_at(36)? > 1 || u32_at(28)? > 0 {
            return Err(invalid("ktx2: cube maps and 3D textures are not supported"));
        }
        let mip_levels = u32_at(40)?.max(1);
        let scheme = u32_at(44)?;
        if scheme != 0 && scheme != ZSTD_SCHEME {
            return Err(invalid("ktx2: unsupported supercompression"));
        }
        let (kvd_offset, kvd_len) = (u32_at(56)? as usize, u32_at(60)? as usize);
        let mut texture = Self {
            format,
            width,
            height,
            layers,
            mip_levels,
            data: Vec::new(),
            swizzle: IDENTITY_SWIZZLE,
        };
        for level in 0..mip_levels as usize {
            let at = 80 + level * 24;
            let (offset, len, raw) = (u64_at(at)?, u64_at(at + 8)?, u64_at(at + 16)?);
            let expected = texture.level_range(level as u32).len();
            if raw != expected || (scheme == 0 && len != expected) {
                return Err(invalid("ktx2: level size does not match the texture"));
            }
            let data = bytes
                .get(offset..offset + len)
                .ok_or(invalid("ktx2: truncated"))?;
            if scheme == ZSTD_SCHEME {
                let level = zstd::bulk::decompress(data, expected)
                    .map_err(|e| FormatError::Compression(e.to_string()))?;
                if level.len() != expected {
                    return Err(invalid("ktx2: level size does not match the texture"));
                }
                texture.data.extend_from_slice(&level);
            } else {
                texture.data.extend_from_slice(data);
            }
        }
        // Key/value pairs: u32 length, key\0value, padded to 4.
        let mut at = kvd_offset;
        let kvd_end = kvd_offset + kvd_len;
        while at + 4 <= kvd_end {
            let len = u32_at(at)? as usize;
            let entry = bytes
                .get(at + 4..at + 4 + len)
                .ok_or(invalid("ktx2: truncated"))?;
            if let Some(value) = entry.strip_prefix(SWIZZLE_KEY)
                && value.len() >= 4
            {
                for (out, c) in texture.swizzle.iter_mut().zip(value) {
                    *out = match c {
                        b'r' => 0,
                        b'g' => 1,
                        b'b' => 2,
                        b'a' => 3,
                        b'0' => 4,
                        _ => 5,
                    };
                }
            }
            at = (at + 4 + len).next_multiple_of(4);
        }
        Ok(texture)
    }

    pub fn read(path: &std::path::Path) -> Result<Self> {
        Self::from_ktx2(&crate::read(path)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_swizzled_array() {
        let mut texture = Texture {
            format: Format::Bc4 { signed: false },
            width: 8,
            height: 8,
            layers: 3,
            mip_levels: 4,
            data: Vec::new(),
            swizzle: [0, 0, 0, 5],
        };
        texture.data = (0..texture.level_range(3).end).map(|i| i as u8).collect();
        let bytes = texture.to_ktx2().unwrap();
        assert_eq!(Texture::from_ktx2(&bytes).unwrap(), texture);
        assert_eq!(texture.layer_data(1, 2).unwrap().len(), 8);
        let packed = texture.to_ktx2_zstd(9).unwrap();
        assert_eq!(Texture::from_ktx2(&packed).unwrap(), texture);
    }

    #[test]
    fn plain_textures_have_no_layer_count() {
        let texture = Texture {
            format: Format::Rgba8 { srgb: true },
            width: 2,
            height: 2,
            layers: 1,
            mip_levels: 2,
            data: vec![7; 16 + 4],
            swizzle: IDENTITY_SWIZZLE,
        };
        let bytes = texture.to_ktx2().unwrap();
        assert_eq!(u32::from_le_bytes(bytes[32..36].try_into().unwrap()), 0);
        assert_eq!(Texture::from_ktx2(&bytes).unwrap(), texture);
    }
}
