//! GX2 surfaces (Wii U textures) and undoing their tiled memory layout.
//!
//! The Wii U GPU (R7xx family) stores textures tiled. This is a port of the
//! relevant parts of AMD's AddrLib (as used by the Wii U SDK) for a GPU with
//! 2 pipes, 4 banks and 256-byte pipe interleave: linear, 1D (micro tiled)
//! and 2D (macro tiled) modes. Block-compressed formats are addressed in 4×4
//! blocks.

use super::Reader;
use crate::{FormatError, Result};

/// `GX2Surface` as stored in an FTEX.
#[derive(Clone, Debug, PartialEq)]
pub struct Surface {
    pub dim: u32,
    pub width: u32,
    pub height: u32,
    /// Depth, or the number of slices of an array texture.
    pub depth: u32,
    pub mip_count: u32,
    pub format: u32,
    pub aa: u32,
    pub usage: u32,
    pub image_size: u32,
    pub mip_size: u32,
    pub tile_mode: u32,
    pub swizzle: u32,
    pub alignment: u32,
    /// Row pitch in elements (pixels, or 4×4 blocks for BCn formats).
    pub pitch: u32,
    pub mip_offsets: [u32; 13],
}

impl Surface {
    pub(crate) fn read(r: &Reader, at: usize) -> Result<Self> {
        let u = |i: usize| r.u32(at + 4 * i);
        let mut mip_offsets = [0; 13];
        for (i, offset) in mip_offsets.iter_mut().enumerate() {
            *offset = u(16 + i)?;
        }
        Ok(Self {
            dim: u(0)?,
            width: u(1)?,
            height: u(2)?,
            depth: u(3)?,
            mip_count: u(4)?,
            format: u(5)?,
            aa: u(6)?,
            usage: u(7)?,
            image_size: u(8)?,
            mip_size: u(10)?,
            tile_mode: u(12)?,
            swizzle: u(13)?,
            alignment: u(14)?,
            pitch: u(15)?,
            mip_offsets,
        })
    }

    pub fn format(&self) -> Format {
        Format::from_gx2(self.format)
    }

    /// A single 2D surface with the layout GX2 computes for it
    /// (`GX2CalcSurfaceSizeAndAlignment`): row pitch, image and mip sizes,
    /// alignment and mip offsets. For files that store only the size,
    /// format and tiling (effect textures). Level 0 keeps the requested tile
    /// mode even when it is smaller than a macro tile; smaller levels fall
    /// back as in [`level`]. Formats this module cannot size are left
    /// without a layout.
    pub fn computed(width: u32, height: u32, mip_count: u32, format: u32, tile_mode: u32, swizzle: u32) -> Self {
        let mut s = Surface {
            dim: 1,
            width,
            height,
            depth: 1,
            mip_count: mip_count.clamp(1, 14),
            format,
            aa: 0,
            usage: 1,
            image_size: 0,
            mip_size: 0,
            tile_mode,
            swizzle,
            alignment: 0,
            pitch: 0,
            mip_offsets: [0; 13],
        };
        let Some(bpp) = s.format().bits_per_element() else { return s };
        let (ew, eh) = if s.format().is_block_compressed() { (width.div_ceil(4), height.div_ceil(4)) } else { (width, height) };
        let (pitch_align, height_align) = alignments(tile_mode, bpp);
        s.pitch = ew.max(1).next_multiple_of(pitch_align);
        s.image_size = s.pitch * eh.max(1).next_multiple_of(height_align) * bpp / 8;
        s.alignment = base_alignment(tile_mode, bpp);
        let mut end = 0u32;
        for l in 1..s.mip_count {
            let Some(layout) = level(&s, l) else { break };
            let align = base_alignment(layout.tile_mode, bpp);
            let start = if l == 1 {
                // Stored relative to the image; the others to the mip data.
                s.mip_offsets[0] = s.image_size.next_multiple_of(align);
                0
            } else {
                let mut start = end.next_multiple_of(align);
                // The first level after the macro-tiled ones is pushed back
                // by 256 bytes per step of pipe and bank swizzle (effect
                // textures in the dump: 0x100 → +256, 0x200 → +512).
                if l == (swizzle >> 16) && layout.tile_mode < 4 {
                    start += ((swizzle >> 8) & 7) << GROUP_BITS;
                }
                s.mip_offsets[l as usize - 1] = start;
                start
            };
            end = start + layout.slice_bytes(bpp) as u32;
        }
        s.mip_size = end;
        s
    }
}

/// Row pitch and height alignment, in elements, of a tile mode.
fn alignments(tile_mode: u32, bpp: u32) -> (u32, u32) {
    match tile_mode {
        0 | 1 => ((256 * 8 / bpp).max(64), 1),
        2 | 3 => ((256 / bpp).max(8), 8),
        _ => ((8 * BANKS).max(8 * BANKS * (256 / bpp / 8)), 8 * PIPES),
    }
}

/// Where a level of a tile mode may start: a whole macro tile per pipe and
/// bank for macro tiling, the pipe interleave otherwise.
fn base_alignment(tile_mode: u32, bpp: u32) -> u32 {
    match tile_mode {
        0..=3 => 1 << GROUP_BITS,
        _ => PIPES * BANKS * MICRO_TILE_PIXELS * thickness(tile_mode) * bpp / 8,
    }
}

/// The surface formats this project decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rgba8 { srgb: bool },
    Bc1 { srgb: bool },
    Bc2 { srgb: bool },
    Bc3 { srgb: bool },
    Bc4 { signed: bool },
    Bc5 { signed: bool },
    Other(u32),
}

impl Format {
    pub fn from_gx2(raw: u32) -> Self {
        let srgb = raw & 0x400 != 0;
        let signed = raw & 0x200 != 0;
        match raw & 0xFF {
            0x1A => Format::Rgba8 { srgb },
            0x31 => Format::Bc1 { srgb },
            0x32 => Format::Bc2 { srgb },
            0x33 => Format::Bc3 { srgb },
            0x34 => Format::Bc4 { signed },
            0x35 => Format::Bc5 { signed },
            _ => Format::Other(raw),
        }
    }

    pub fn is_block_compressed(self) -> bool {
        matches!(self, Format::Bc1 { .. } | Format::Bc2 { .. } | Format::Bc3 { .. } | Format::Bc4 { .. } | Format::Bc5 { .. })
    }

    /// Bits per element (a pixel, or a 4×4 block).
    pub fn bits_per_element(self) -> Option<u32> {
        match self {
            Format::Rgba8 { .. } => Some(32),
            Format::Bc1 { .. } | Format::Bc4 { .. } => Some(64),
            Format::Bc2 { .. } | Format::Bc3 { .. } | Format::Bc5 { .. } => Some(128),
            // R8 and R8G8 (effect textures), decoded to grey and red-green.
            Format::Other(raw) if raw & 0xFF == 0x01 => Some(8),
            Format::Other(raw) if raw & 0xFF == 0x07 => Some(16),
            // R16G16B16A16 float (the water colour table `WaterAlb`).
            Format::Other(raw) if raw & 0xFF == 0x20 => Some(64),
            Format::Other(_) => None,
        }
    }
}

const PIPES: u32 = 2;
const BANKS: u32 = 4;
const GROUP_BITS: u32 = 8; // 256-byte pipe interleave
const PIPE_BITS: u32 = 1;
const BANK_BITS: u32 = 2;
const MICRO_TILE_PIXELS: u32 = 64;

/// Where and how one mip level of a surface is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Level {
    /// Size in pixels (mips are padded up to powers of two).
    pub width: u32,
    pub height: u32,
    /// Row pitch and padded height, in elements.
    pub pitch: u32,
    pub rows: u32,
    /// Array slices stored (mips of arrays pad the count to a power of two).
    pub slices: u32,
    pub tile_mode: u32,
    /// Byte offset: level 0 within the image data, others within the mip data.
    pub offset: usize,
}

impl Level {
    pub fn slice_bytes(&self, bits_per_element: u32) -> usize {
        self.pitch as usize * self.rows as usize * (bits_per_element / 8) as usize
    }
}

/// The layout of mip level `level` of `surface`, following AddrLib's rules
/// for GX2 (checked against the dump's mip offsets, arrays included).
pub fn level(surface: &Surface, level: u32) -> Option<Level> {
    let format = surface.format();
    let bpp = format.bits_per_element()?;
    let bc = format.is_block_compressed();
    let array = matches!(surface.dim, 4 | 5 | 7);
    if level == 0 {
        let slices = if array { surface.depth.max(1) } else { 1 };
        let rows = surface.image_size / (slices * surface.pitch.max(1) * bpp / 8).max(1);
        return Some(Level {
            width: surface.width,
            height: surface.height,
            pitch: surface.pitch,
            rows,
            slices,
            tile_mode: surface.tile_mode,
            offset: 0,
        });
    }
    let width = (surface.width >> level).max(1).next_power_of_two();
    let height = (surface.height >> level).max(1).next_power_of_two();
    let (ew, eh) = if bc { (width.div_ceil(4), height.div_ceil(4)) } else { (width, height) };
    let tile_mode = mip_tile_mode(surface.tile_mode, bpp, ew, eh);
    let (pitch_align, height_align) = alignments(tile_mode, bpp);
    let offset = if level == 1 { 0 } else { *surface.mip_offsets.get(level as usize - 1)? as usize };
    Some(Level {
        width,
        height,
        pitch: ew.next_multiple_of(pitch_align),
        rows: eh.next_multiple_of(height_align),
        slices: if array { surface.depth.max(1).next_power_of_two() } else { 1 },
        tile_mode,
        offset,
    })
}

/// AddrLib's `ComputeSurfaceMipLevelTileMode` for thin modes: macro tiling
/// falls back to micro tiling once a level is smaller than a macro tile.
fn mip_tile_mode(base: u32, bpp: u32, width: u32, height: u32) -> u32 {
    let micro_tile_bytes = bpp * 64 / 8;
    let width_align_factor = (256 / micro_tile_bytes).max(1);
    let (macro_width, macro_height) = match base {
        5 => (16, 32),
        6 => (8, 64),
        _ => (32, 16),
    };
    match base {
        4 | 5 | 6 | 12 if width < width_align_factor * macro_width || height < macro_height => 2,
        7 | 13 if width < width_align_factor * macro_width || height < macro_height => 3,
        mode => mode,
    }
}

/// Undoes the tiling of one level (and array slice) of `surface`,
/// returning its elements (pixels or 4×4 blocks) row by row. `data` starts
/// at the level.
pub fn deswizzle(surface: &Surface, level: &Level, data: &[u8], slice: u32) -> Result<Vec<u8>> {
    let format = surface.format();
    let bpp = format.bits_per_element().ok_or(FormatError::Invalid("gx2: unsupported surface format"))?;
    let bytes = (bpp / 8) as usize;
    let (w, h) = if format.is_block_compressed() {
        (level.width.div_ceil(4), level.height.div_ceil(4))
    } else {
        (level.width, level.height)
    };
    let pipe_swizzle = (surface.swizzle >> 8) & 1;
    let bank_swizzle = (surface.swizzle >> 9) & 3;
    let is_depth = surface.usage & 4 != 0;
    let (pitch, rows, tile_mode) = (level.pitch, level.rows, level.tile_mode);

    let mut out = vec![0u8; w as usize * h as usize * bytes];
    for y in 0..h {
        for x in 0..w {
            let address = match tile_mode {
                0 | 1 => ((slice * rows + y) * pitch + x) as usize * bytes,
                2 | 3 => micro_tiled_address(x, y, slice, bpp, pitch, rows, tile_mode, is_depth),
                4..=15 => macro_tiled_address(x, y, slice, bpp, pitch, rows, tile_mode, is_depth, pipe_swizzle, bank_swizzle)?,
                _ => return Err(FormatError::Invalid("gx2: unsupported tile mode")),
            };
            let target = (y * w + x) as usize * bytes;
            if let Some(source) = data.get(address..address + bytes) {
                out[target..target + bytes].copy_from_slice(source);
            }
        }
    }
    Ok(out)
}

fn thickness(tile_mode: u32) -> u32 {
    match tile_mode {
        3 | 7 | 11 | 13 | 15 => 4,
        16 | 17 => 8,
        _ => 1,
    }
}

fn pixel_index_within_micro_tile(x: u32, y: u32, z: u32, bpp: u32, tile_mode: u32, is_depth: bool) -> u32 {
    let bit = |v: u32, n: u32| (v >> n) & 1;
    let [b0, b1, b2, b3, b4, b5] = if is_depth {
        [bit(x, 0), bit(y, 0), bit(x, 1), bit(y, 1), bit(x, 2), bit(y, 2)]
    } else {
        match bpp {
            8 => [bit(x, 0), bit(x, 1), bit(x, 2), bit(y, 1), bit(y, 0), bit(y, 2)],
            16 => [bit(x, 0), bit(x, 1), bit(x, 2), bit(y, 0), bit(y, 1), bit(y, 2)],
            64 => [bit(x, 0), bit(y, 0), bit(x, 1), bit(x, 2), bit(y, 1), bit(y, 2)],
            128 => [bit(y, 0), bit(x, 0), bit(x, 1), bit(x, 2), bit(y, 1), bit(y, 2)],
            // 32 and 96 bits, and anything else.
            _ => [bit(x, 0), bit(x, 1), bit(y, 0), bit(x, 2), bit(y, 1), bit(y, 2)],
        }
    };
    let t = thickness(tile_mode);
    let (b6, b7, b8) = (
        if t > 1 { bit(z, 0) } else { 0 },
        if t > 1 { bit(z, 1) } else { 0 },
        if t == 8 { bit(z, 2) } else { 0 },
    );
    b0 | b1 << 1 | b2 << 2 | b3 << 3 | b4 << 4 | b5 << 5 | b6 << 6 | b7 << 7 | b8 << 8
}

#[allow(clippy::too_many_arguments)]
fn micro_tiled_address(x: u32, y: u32, slice: u32, bpp: u32, pitch: u32, height: u32, tile_mode: u32, is_depth: bool) -> usize {
    let micro_thickness = thickness(tile_mode);
    let micro_tile_bytes = (MICRO_TILE_PIXELS * micro_thickness * bpp).div_ceil(8);
    let micro_tiles_per_row = pitch >> 3;
    let tile_offset = micro_tile_bytes * ((x >> 3) + (y >> 3) * micro_tiles_per_row);
    let slice_bytes = (pitch as u64 * height as u64 * micro_thickness as u64 * bpp as u64).div_ceil(8);
    let slice_offset = (slice / micro_thickness) as u64 * slice_bytes;
    let pixel_offset = (bpp * pixel_index_within_micro_tile(x, y, slice, bpp, tile_mode, is_depth)) >> 3;
    (pixel_offset as u64 + tile_offset as u64 + slice_offset) as usize
}

#[allow(clippy::too_many_arguments)]
fn macro_tiled_address(
    x: u32,
    y: u32,
    slice: u32,
    bpp: u32,
    pitch: u32,
    height: u32,
    tile_mode: u32,
    is_depth: bool,
    pipe_swizzle: u32,
    bank_swizzle: u32,
) -> Result<usize> {
    let micro_thickness = thickness(tile_mode);
    let micro_tile_bits = bpp * micro_thickness * 64;
    let element_offset = (bpp * pixel_index_within_micro_tile(x, y, slice, bpp, tile_mode, is_depth)).div_ceil(8) as u64;
    let _ = micro_tile_bits;

    let mut pipe = ((y >> 3) ^ (x >> 3)) & 1;
    let mut bank = (((y >> 5) ^ (x >> 3)) & 1) | (2 * (((y >> 4) ^ (x >> 4)) & 1));
    let rotation = match tile_mode {
        4..=11 | 14 | 15 => PIPES * ((BANKS >> 1) - 1),
        12 | 13 => 1,
        _ => 0,
    };
    let swizzle = pipe_swizzle + 2 * bank_swizzle;
    let slice_in = if matches!(tile_mode, 7 | 11 | 13 | 15) { slice >> 2 } else { slice };
    let bank_pipe = ((pipe + 2 * bank) ^ (swizzle + slice_in * rotation)) % 8;
    pipe = bank_pipe % 2;
    bank = bank_pipe / 2;

    let slice_bytes = (height as u64 * pitch as u64 * micro_thickness as u64 * bpp as u64).div_ceil(8);
    let slice_offset = slice_bytes * (slice / micro_thickness) as u64;

    let (mut macro_pitch, mut macro_height) = (8 * BANKS, 8 * PIPES);
    match tile_mode {
        5 | 9 => {
            macro_pitch >>= 1;
            macro_height *= 2;
        }
        6 | 10 => {
            macro_pitch >>= 2;
            macro_height *= 4;
        }
        _ => {}
    }
    let macro_tiles_per_row = pitch / macro_pitch;
    let macro_tile_bytes = (micro_thickness as u64 * bpp as u64 * macro_height as u64 * macro_pitch as u64).div_ceil(8);
    let macro_x = x / macro_pitch;
    let macro_y = y / macro_height;
    let macro_offset = (macro_x as u64 + macro_tiles_per_row as u64 * macro_y as u64) * macro_tile_bytes;

    if matches!(tile_mode, 8..=11 | 14 | 15) {
        // Bank-swapped modes are not used by the textures seen so far.
        return Err(FormatError::Invalid("gx2: bank-swapped tile modes are not supported"));
    }

    let group_mask = (1u64 << GROUP_BITS) - 1;
    let swizzle_bits = BANK_BITS + PIPE_BITS;
    let total = element_offset + ((macro_offset + slice_offset) >> swizzle_bits);
    let high = (total & !group_mask) << swizzle_bits;
    let low = total & group_mask;
    let pipe_part = (pipe as u64) << GROUP_BITS;
    let bank_part = (bank as u64) << (PIPE_BITS + GROUP_BITS);
    Ok((bank_part | pipe_part | low | high) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surface(format: u32, tile_mode: u32) -> Surface {
        Surface {
            dim: 1,
            width: 64,
            height: 64,
            depth: 1,
            mip_count: 1,
            format,
            aa: 0,
            usage: 1,
            image_size: 0,
            mip_size: 0,
            tile_mode,
            swizzle: 0,
            alignment: 0,
            pitch: 64,
            mip_offsets: [0; 13],
        }
    }

    fn level0(s: &Surface) -> Level {
        Level { width: s.width, height: s.height, pitch: s.pitch, rows: s.height, slices: 1, tile_mode: s.tile_mode, offset: 0 }
    }

    #[test]
    fn linear_is_row_major() {
        let s = surface(0x1A, 1);
        let data: Vec<u8> = (0..64 * 64 * 4).map(|i| (i / 4) as u8).collect();
        let out = deswizzle(&s, &level0(&s), &data, 0).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn tiled_layouts_are_permutations() {
        // Every source element lands somewhere and nothing is duplicated.
        for tile_mode in [2, 4] {
            let s = surface(0x1A, tile_mode);
            let data: Vec<u8> = (0..64u32 * 64).flat_map(|i| i.to_le_bytes()).collect();
            let out = deswizzle(&s, &level0(&s), &data, 0).unwrap();
            let mut ids: Vec<u32> = out.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), 64 * 64, "tile mode {tile_mode}");
        }
    }

    /// Level sizes must reproduce the mip offsets stored in the dump.
    #[allow(clippy::too_many_arguments)]
    fn check_offsets(width: u32, height: u32, depth: u32, dim: u32, format: u32, mips: u32, offsets: &[u32], mip_size: u32) {
        let mut s = surface(format, 4);
        (s.width, s.height, s.depth, s.dim, s.mip_count) = (width, height, depth, dim, mips);
        s.mip_offsets[..offsets.len()].copy_from_slice(offsets);
        let bpp = s.format().bits_per_element().unwrap();
        let mut end = 0;
        for l in 1..mips {
            let level = level(&s, l).unwrap();
            if l >= 2 {
                assert!(level.offset >= end, "level {l} starts at {} before the previous ends at {end}", level.offset);
            }
            end = level.offset + level.slice_bytes(bpp) * level.slices as usize;
        }
        assert!(end <= mip_size as usize && mip_size as usize - end <= 1024, "levels end at {end}, mips are {mip_size}");
    }

    #[test]
    fn mip_layouts_match_the_dump() {
        // Tree_TreeConiferousLeaf_A_Alb: 512² BC1, no bank swizzle.
        check_offsets(512, 512, 1, 1, 0x431, 10, &[131072, 32768, 40960, 43008, 43520, 44032, 44544, 45056, 45568], 46080);
        let mut s = surface(0x431, 4);
        (s.width, s.height, s.mip_count) = (512, 512, 10);
        s.mip_offsets[..9].copy_from_slice(&[131072, 32768, 40960, 43008, 43520, 44032, 44544, 45056, 45568]);
        assert_eq!(level(&s, 2).unwrap().offset, 32768);
        assert_eq!(level(&s, 3).unwrap().tile_mode, 2, "16 blocks wide falls back to 1D");
        // MaterialAlb: 1024² BC1 array of 83, mips pad to 128 slices.
        check_offsets(
            1024,
            1024,
            83,
            5,
            0x431,
            11,
            &[43515904, 16777216, 20971520, 22020096, 22282240, 22347776, 22413312, 22478848, 22544384, 22609920],
            22675456,
        );
        // Tree1Alb: 64×128 BC3 array of 49.
        check_offsets(64, 128, 49, 5, 0x433, 8, &[802816, 131072, 196608, 262144, 327680, 393216, 458752], 524288);
    }

    #[test]
    fn computed_layouts_match_stored_ones() {
        // Tree_TreeConiferousLeaf_A_Alb from a BFRES: 512² BC1, 10 mips.
        let s = Surface::computed(512, 512, 10, 0x431, 4, 0x30000);
        assert_eq!((s.pitch, s.image_size), (128, 131072));
        assert_eq!(s.mip_offsets[..9], [131072, 32768, 40960, 43008, 43520, 44032, 44544, 45056, 45568]);
        assert_eq!(s.mip_size, 46080);
        // Effect textures, whose files give only the total size: a
        // non-power-of-two BC5, a small BC4 whose level 0 stays macro
        // tiled, and pipe/bank swizzled ones with padding before level 2.
        let total = |s: Surface| s.mip_offsets[0] + s.mip_size;
        assert_eq!(total(Surface::computed(512, 148, 10, 0x35, 4, 0x30000)), 0x24000);
        assert_eq!(total(Surface::computed(64, 64, 7, 0x34, 4, 0x10000)), 0x1C00);
        assert_eq!(total(Surface::computed(256, 256, 9, 0x433, 4, 0x20100)), 92416);
        assert_eq!(total(Surface::computed(256, 256, 9, 0x35, 4, 0x20200)), 92672);
    }

    #[test]
    fn micro_tile_order_for_32_bit_pixels() {
        // Within an 8×8 micro tile, 32-bit pixels go x0, x1, y0, x2, y1, y2.
        assert_eq!(pixel_index_within_micro_tile(1, 0, 0, 32, 2, false), 1);
        assert_eq!(pixel_index_within_micro_tile(0, 1, 0, 32, 2, false), 4);
        assert_eq!(pixel_index_within_micro_tile(2, 0, 0, 32, 2, false), 2);
        assert_eq!(pixel_index_within_micro_tile(4, 0, 0, 32, 2, false), 8);
    }
}
