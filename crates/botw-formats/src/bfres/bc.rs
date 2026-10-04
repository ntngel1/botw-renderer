//! BC1 (DXT1) blocks: decoding, a simple encoder and mip generation.
//!
//! The dump's large terrain arrays ship without their smaller mip levels,
//! so they are rebuilt here: decode, box-filter, re-encode. The encoder is a
//! plain bounding-box fit, good enough for mips.

/// Decodes one 8-byte BC1 block into 16 RGBA pixels, row-major.
pub fn decode_bc1_block(block: &[u8]) -> [[u8; 4]; 16] {
    let c0 = u16::from_le_bytes([block[0], block[1]]);
    let c1 = u16::from_le_bytes([block[2], block[3]]);
    let (a, b) = (rgb565(c0), rgb565(c1));
    let palette: [[u8; 4]; 4] = if c0 > c1 {
        [a, b, mix(a, b, 2, 1), mix(a, b, 1, 2)]
    } else {
        [a, b, mix(a, b, 1, 1), [0, 0, 0, 0]]
    };
    let bits = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    std::array::from_fn(|i| palette[((bits >> (2 * i)) & 3) as usize])
}

fn rgb565(c: u16) -> [u8; 4] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [
        ((r * 527 + 23) >> 6) as u8,
        ((g * 259 + 33) >> 6) as u8,
        ((b * 527 + 23) >> 6) as u8,
        255,
    ]
}

/// `(wa·a + wb·b) / (wa + wb)` per channel, alpha 255.
fn mix(a: [u8; 4], b: [u8; 4], wa: u32, wb: u32) -> [u8; 4] {
    let m = |i: usize| ((u32::from(a[i]) * wa + u32::from(b[i]) * wb) / (wa + wb)) as u8;
    [m(0), m(1), m(2), 255]
}

fn to565(c: [f32; 3]) -> u16 {
    let r = (c[0].clamp(0.0, 255.0) * 31.0 / 255.0).round() as u16;
    let g = (c[1].clamp(0.0, 255.0) * 63.0 / 255.0).round() as u16;
    let b = (c[2].clamp(0.0, 255.0) * 31.0 / 255.0).round() as u16;
    (r << 11) | (g << 5) | b
}

/// Encodes 16 RGBA pixels (alpha ignored) as a 4-colour BC1 block.
pub fn encode_bc1_block(pixels: &[[u8; 4]; 16]) -> [u8; 8] {
    // Endpoints along the colour bounding box's diagonal, inset a little
    // (as stb_dxt does) so the palette covers the pixels better.
    let mut lo = [255.0f32; 3];
    let mut hi = [0.0f32; 3];
    for p in pixels {
        for c in 0..3 {
            lo[c] = lo[c].min(f32::from(p[c]));
            hi[c] = hi[c].max(f32::from(p[c]));
        }
    }
    // Flip the diagonal on channels that anti-correlate with the channel
    // of widest range.
    let mean: [f32; 3] =
        std::array::from_fn(|c| pixels.iter().map(|p| f32::from(p[c])).sum::<f32>() / 16.0);
    let reference = (0..3)
        .max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b])))
        .unwrap();
    let covariance = |c: usize| {
        pixels
            .iter()
            .map(|p| (f32::from(p[c]) - mean[c]) * (f32::from(p[reference]) - mean[reference]))
            .sum::<f32>()
    };
    for c in (0..3).filter(|&c| c != reference) {
        if covariance(c) < 0.0 {
            std::mem::swap(&mut lo[c], &mut hi[c]);
        }
    }
    let inset: [f32; 3] = std::array::from_fn(|c| (hi[c] - lo[c]) / 16.0);
    let mut c0 = to565(std::array::from_fn(|c| hi[c] - inset[c]));
    let mut c1 = to565(std::array::from_fn(|c| lo[c] + inset[c]));
    if c0 < c1 {
        std::mem::swap(&mut c0, &mut c1);
    }
    let mut block = [0u8; 8];
    block[0..2].copy_from_slice(&c0.to_le_bytes());
    block[2..4].copy_from_slice(&c1.to_le_bytes());
    if c0 == c1 {
        return block; // A flat block: every index 0.
    }
    let palette = {
        let (a, b) = (rgb565(c0), rgb565(c1));
        [a, b, mix(a, b, 2, 1), mix(a, b, 1, 2)]
    };
    let mut bits = 0u32;
    for (i, p) in pixels.iter().enumerate() {
        let distance = |q: &[u8; 4]| {
            (0..3)
                .map(|c| (i32::from(p[c]) - i32::from(q[c])).pow(2))
                .sum::<i32>()
        };
        let best = (0..4).min_by_key(|&k| distance(&palette[k])).unwrap() as u32;
        bits |= best << (2 * i);
    }
    block[4..8].copy_from_slice(&bits.to_le_bytes());
    block
}

/// Decodes one 8-byte BC4 block into 16 unsigned values, row-major.
pub fn decode_bc4_block(block: &[u8]) -> [u8; 16] {
    let (a, b) = (u32::from(block[0]), u32::from(block[1]));
    let palette: [u8; 8] = if a > b {
        std::array::from_fn(|i| match i {
            0 => a as u8,
            1 => b as u8,
            i => (((8 - i as u32) * a + (i as u32 - 1) * b) / 7) as u8,
        })
    } else {
        std::array::from_fn(|i| match i {
            0 => a as u8,
            1 => b as u8,
            6 => 0,
            7 => 255,
            i => (((6 - i as u32) * a + (i as u32 - 1) * b) / 5) as u8,
        })
    };
    let bits = u64::from_le_bytes([
        block[2], block[3], block[4], block[5], block[6], block[7], 0, 0,
    ]);
    std::array::from_fn(|i| palette[((bits >> (3 * i)) & 7) as usize])
}

/// Decodes a BC3 image (16-byte blocks: BC4-style alpha, then BC1 colour)
/// into RGBA8.
pub fn decode_bc3(blocks: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (bw, bh) = (width.div_ceil(4) as usize, height.div_ceil(4) as usize);
    let (w, h) = (width as usize, height as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let offset = (by * bw + bx) * 16;
            let Some(block) = blocks.get(offset..offset + 16) else {
                continue;
            };
            let alpha = decode_bc4_block(&block[..8]);
            // BC3 colour blocks always use the four-colour mode.
            for (i, p) in decode_bc1_four_colour(&block[8..16]).iter().enumerate() {
                write_pixel(
                    &mut rgba,
                    w,
                    h,
                    bx * 4 + i % 4,
                    by * 4 + i / 4,
                    [p[0], p[1], p[2], alpha[i]],
                );
            }
        }
    }
    rgba
}

fn decode_bc1_four_colour(block: &[u8]) -> [[u8; 4]; 16] {
    let (a, b) = (
        rgb565(u16::from_le_bytes([block[0], block[1]])),
        rgb565(u16::from_le_bytes([block[2], block[3]])),
    );
    let palette = [a, b, mix(a, b, 2, 1), mix(a, b, 1, 2)];
    let bits = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
    std::array::from_fn(|i| palette[((bits >> (2 * i)) & 3) as usize])
}

fn write_pixel(rgba: &mut [u8], w: usize, h: usize, x: usize, y: usize, p: [u8; 4]) {
    if x < w && y < h {
        rgba[(y * w + x) * 4..][..4].copy_from_slice(&p);
    }
}

/// Decodes a BC4 image into one byte per pixel.
pub fn decode_bc4(blocks: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (bw, bh) = (width.div_ceil(4) as usize, height.div_ceil(4) as usize);
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; w * h];
    for by in 0..bh {
        for bx in 0..bw {
            let offset = (by * bw + bx) * 8;
            let Some(block) = blocks.get(offset..offset + 8) else {
                continue;
            };
            for (i, v) in decode_bc4_block(block).into_iter().enumerate() {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < w && y < h {
                    out[y * w + x] = v;
                }
            }
        }
    }
    out
}

/// Decodes a BC1 image (blocks in row-major order) into RGBA8.
pub fn decode_bc1(blocks: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (bw, bh) = (width.div_ceil(4) as usize, height.div_ceil(4) as usize);
    let (w, h) = (width as usize, height as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let offset = (by * bw + bx) * 8;
            let Some(block) = blocks.get(offset..offset + 8) else {
                continue;
            };
            let pixels = decode_bc1_block(block);
            for (i, p) in pixels.iter().enumerate() {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < w && y < h {
                    rgba[(y * w + x) * 4..][..4].copy_from_slice(p);
                }
            }
        }
    }
    rgba
}

/// Encodes RGBA8 into BC1 blocks in row-major order.
pub fn encode_bc1(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (bw, bh) = (width.div_ceil(4) as usize, height.div_ceil(4) as usize);
    let (w, h) = (width as usize, height as usize);
    let mut blocks = Vec::with_capacity(bw * bh * 8);
    for by in 0..bh {
        for bx in 0..bw {
            let pixels: [[u8; 4]; 16] = std::array::from_fn(|i| {
                // Edge blocks of tiny levels repeat the last row/column.
                let x = (bx * 4 + i % 4).min(w - 1);
                let y = (by * 4 + i / 4).min(h - 1);
                rgba[(y * w + x) * 4..][..4].try_into().unwrap()
            });
            blocks.extend_from_slice(&encode_bc1_block(&pixels));
        }
    }
    blocks
}

/// Halves an RGBA8 image with a 2×2 box filter (odd edges clamp).
/// Decodes BC5 (two BC4 channels, e.g. a normal map's X and Y) to RGBA8
/// with blue 0 and alpha 255.
// SI-FMT-10: texture mips and BC conversions are ours.
pub fn decode_bc5(blocks: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (bw, bh) = (width.div_ceil(4) as usize, height.div_ceil(4) as usize);
    let (w, h) = (width as usize, height as usize);
    let mut rgba = vec![0u8; w * h * 4];
    for by in 0..bh {
        for bx in 0..bw {
            let offset = (by * bw + bx) * 16;
            let Some(block) = blocks.get(offset..offset + 16) else {
                continue;
            };
            let (red, green) = (decode_bc4_block(&block[..8]), decode_bc4_block(&block[8..]));
            for i in 0..16 {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < w && y < h {
                    rgba[(y * w + x) * 4..(y * w + x) * 4 + 4]
                        .copy_from_slice(&[red[i], green[i], 0, 255]);
                }
            }
        }
    }
    rgba
}

/// Encodes one BC4 block from 16 values (row-major 4×4), using the
/// 8-value mode between the block's extremes.
pub fn encode_bc4_block(values: &[u8; 16]) -> [u8; 8] {
    let (lo, hi) = values
        .iter()
        .fold((255u8, 0u8), |(l, h), &v| (l.min(v), h.max(v)));
    let mut block = [0u8; 8];
    if hi == lo {
        block[0] = hi;
        block[1] = lo;
        return block; // All indices 0 = the first endpoint.
    }
    block[0] = hi;
    block[1] = lo;
    // Palette index for each of the 8 steps from hi (0) to lo (1), in the
    // format's order: 0 = hi, 1 = lo, 2..7 in between from hi towards lo.
    const ORDER: [u64; 8] = [0, 2, 3, 4, 5, 6, 7, 1];
    let range = f32::from(hi - lo);
    let mut bits = 0u64;
    for (i, &v) in values.iter().enumerate() {
        let step = ((f32::from(hi - v) / range) * 7.0).round() as usize;
        bits |= ORDER[step.min(7)] << (3 * i);
    }
    block[2..8].copy_from_slice(&bits.to_le_bytes()[..6]);
    block
}

/// Re-encodes a BC1 image's red and green channels as BC5: normal maps
/// stored as BC1 keep X and Y there (blue is something else).
pub fn bc1_to_bc5(blocks: &[u8], width: u32, height: u32) -> Vec<u8> {
    bc1_channels_to_bc4(blocks, width, height, &[0, 1])
}

/// Re-encodes a BC1 image's blue channel as BC4: the gloss the game keeps
/// in its normal maps' blue.
pub fn bc1_blue_to_bc4(blocks: &[u8], width: u32, height: u32) -> Vec<u8> {
    bc1_channels_to_bc4(blocks, width, height, &[2])
}

/// A BC1 image's `channels` (0 red … 2 blue), each re-encoded as BC4, one
/// block after another per 4×4 block.
fn bc1_channels_to_bc4(blocks: &[u8], width: u32, height: u32, channels: &[usize]) -> Vec<u8> {
    let rgba = decode_bc1(blocks, width, height);
    let (w, h) = (width as usize, height as usize);
    let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
    let mut out = Vec::with_capacity(bw * bh * 8 * channels.len());
    for by in 0..bh {
        for bx in 0..bw {
            for &channel in channels {
                let values: [u8; 16] = std::array::from_fn(|i| {
                    let (x, y) = ((bx * 4 + i % 4).min(w - 1), (by * 4 + i / 4).min(h - 1));
                    rgba[(y * w + x) * 4 + channel]
                });
                out.extend_from_slice(&encode_bc4_block(&values));
            }
        }
    }
    out
}

pub fn downsample(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((width / 2).max(1), (height / 2).max(1));
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; nw as usize * nh as usize * 4];
    for y in 0..nh as usize {
        for x in 0..nw as usize {
            for c in 0..4 {
                let at = |xx: usize, yy: usize| {
                    u32::from(rgba[(yy.min(h - 1) * w + xx.min(w - 1)) * 4 + c])
                };
                let sum = at(2 * x, 2 * y)
                    + at(2 * x + 1, 2 * y)
                    + at(2 * x, 2 * y + 1)
                    + at(2 * x + 1, 2 * y + 1);
                out[(y * nw as usize + x) * 4 + c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    (out, nw, nh)
}

/// Every mip level below `level0` (BC1 blocks of a `width`×`height`
/// image), down to 1×1, as BC1 blocks.
pub fn bc1_mips(level0: &[u8], width: u32, height: u32) -> Vec<Vec<u8>> {
    let mut rgba = decode_bc1(level0, width, height);
    let (mut w, mut h) = (width, height);
    let mut levels = Vec::new();
    while w > 1 || h > 1 {
        let (next, nw, nh) = downsample(&rgba, w, h);
        levels.push(encode_bc1(&next, nw, nh));
        (rgba, w, h) = (next, nw, nh);
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc4_round_trips_closely() {
        let values: [u8; 16] = std::array::from_fn(|i| (i * 13 + 20) as u8);
        let decoded = decode_bc4_block(&encode_bc4_block(&values));
        for (a, b) in values.iter().zip(decoded) {
            assert!(a.abs_diff(b) <= 14, "{a} vs {b}");
        }
        let flat = decode_bc4_block(&encode_bc4_block(&[77; 16]));
        assert!(flat.iter().all(|&v| v == 77));
    }

    #[test]
    fn decodes_bc5_channels() {
        // Red: flat 200 (both endpoints 200, all indices 0); green: flat 50.
        let mut block = [0u8; 16];
        block[..2].copy_from_slice(&[200, 200]);
        block[8..10].copy_from_slice(&[50, 50]);
        let rgba = decode_bc5(&block, 4, 4);
        assert!(rgba.chunks(4).all(|p| p == [200, 50, 0, 255]));
    }

    #[test]
    fn splits_bc1_channels_into_bc4() {
        // A normal map's texel: X and Y in red and green, gloss in blue.
        let pixels: [[u8; 4]; 16] = std::array::from_fn(|i| [100, 160, (i * 8 + 40) as u8, 255]);
        let bc1 = encode_bc1(&pixels.concat(), 4, 4);
        let reference = decode_bc1(&bc1, 4, 4);
        let blue = decode_bc4(&bc1_blue_to_bc4(&bc1, 4, 4), 4, 4);
        let xy = decode_bc5(&bc1_to_bc5(&bc1, 4, 4), 4, 4);
        for (i, p) in reference.chunks(4).enumerate() {
            assert!(p[2].abs_diff(blue[i]) <= 12, "blue {} vs {}", p[2], blue[i]);
            assert!(p[0].abs_diff(xy[i * 4]) <= 2 && p[1].abs_diff(xy[i * 4 + 1]) <= 2);
        }
    }

    #[test]
    fn round_trips_a_gradient_closely() {
        // Red rises while blue falls: the fit must pick the anti-diagonal.
        // Four palette colours over a 60-step range leave ~10 of error.
        let pixels: [[u8; 4]; 16] =
            std::array::from_fn(|i| [(i * 4) as u8, 100, 200 - (i * 4) as u8, 255]);
        let decoded = decode_bc1_block(&encode_bc1_block(&pixels));
        for (a, b) in pixels.iter().zip(decoded.iter()) {
            for c in 0..3 {
                assert!(
                    (i32::from(a[c]) - i32::from(b[c])).abs() <= 16,
                    "{a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn flat_blocks_stay_flat() {
        let pixels = [[40, 120, 200, 255]; 16];
        let decoded = decode_bc1_block(&encode_bc1_block(&pixels));
        for p in decoded {
            assert!((i32::from(p[1]) - 120).abs() <= 4);
        }
    }

    #[test]
    fn builds_the_whole_mip_chain() {
        let level0 = encode_bc1(&vec![128u8; 16 * 8 * 4], 16, 8);
        let mips = bc1_mips(&level0, 16, 8);
        // 8×4, 4×2, 2×1, 1×1: each at least one block.
        assert_eq!(
            mips.iter().map(Vec::len).collect::<Vec<_>>(),
            vec![16, 8, 8, 8]
        );
    }
}
