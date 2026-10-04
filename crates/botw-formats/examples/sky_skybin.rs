//! Untile the three RGBA16F surfaces of `System/KSys/sky.skybin` with a
//! candidate GX2 tile mode and report how smooth each result is (neighbour
//! differences); optionally write the untiled bytes. The file holds the
//! surfaces' tiled images back to back (docs/research/wiiu-sky-resources.md).
//! `cargo run --offline -p botw-formats --example sky_skybin -- FILE [--out DIR]`

use botw_formats::bfres::gx2::{Level, Surface, deswizzle};

/// (width, height, depth) of the surfaces in file order.
const SURFACES: [(&str, u32, u32, u32); 3] = [
    ("transmittance", 256, 64, 1),
    ("inscatter", 256, 32, 16),
    ("irradiance", 64, 64, 1),
];
const RGBA16F: u32 = 0x820;

fn half(bits: u16) -> f32 {
    let sign = if bits >> 15 == 1 { -1.0 } else { 1.0 };
    let exp = (bits >> 10 & 0x1f) as i32;
    let man = (bits & 0x3ff) as f32;
    sign * match exp {
        0 => man * 2f32.powi(-24),
        31 => f32::INFINITY,
        _ => (1.0 + man / 1024.0) * 2f32.powi(exp - 15),
    }
}

fn untile(data: &[u8], width: u32, height: u32, depth: u32, tile_mode: u32) -> Vec<u8> {
    let (pitch_align, height_align) = match tile_mode {
        0 | 1 => (32, 1),
        2 | 3 => (8, 8),
        _ => (32, 16),
    };
    let surface = Surface {
        dim: if depth > 1 { 2 } else { 1 },
        width,
        height,
        depth,
        mip_count: 1,
        format: RGBA16F,
        aa: 0,
        usage: 1,
        image_size: 0,
        mip_size: 0,
        tile_mode,
        swizzle: 0,
        alignment: 0,
        pitch: width.next_multiple_of(pitch_align),
        mip_offsets: [0; 13],
    };
    let level = Level {
        width,
        height,
        pitch: surface.pitch,
        rows: height.next_multiple_of(height_align),
        slices: 1,
        tile_mode,
        offset: 0,
    };
    (0..depth)
        .flat_map(|z| deswizzle(&surface, &level, data, z).expect("untile"))
        .collect()
}

/// Mean absolute difference of the red channel between x and y neighbours.
fn roughness(texels: &[u8], width: u32, height: u32, depth: u32) -> f32 {
    let at = |x: u32, y: u32, z: u32| {
        let i = (((z * height + y) * width + x) * 8) as usize;
        half(u16::from_le_bytes([texels[i], texels[i + 1]]))
    };
    let (mut sum, mut n) = (0.0f64, 0u64);
    for z in 0..depth {
        for y in 0..height - 1 {
            for x in 0..width - 1 {
                let v = at(x, y, z);
                sum += ((at(x + 1, y, z) - v).abs() + (at(x, y + 1, z) - v).abs()) as f64;
                n += 2;
            }
        }
    }
    (sum / n as f64) as f32
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("expected FILE [--out DIR]")?;
    let out = match (args.next().as_deref(), args.next()) {
        (Some("--out"), Some(dir)) => Some(std::path::PathBuf::from(dir)),
        (None, _) => None,
        _ => return Err("expected FILE [--out DIR]".into()),
    };
    let data = std::fs::read(&path)?;
    let mut offset = 0usize;
    for (name, width, height, depth) in SURFACES {
        let size = (width * height * depth * 8) as usize;
        let image = data.get(offset..offset + size).ok_or("file too short")?;
        let mut best = (f32::INFINITY, 0);
        for tile_mode in [1, 2, 3, 4, 7] {
            let texels = untile(image, width, height, depth, tile_mode);
            let r = roughness(&texels, width, height, depth);
            println!("{name} {width}x{height}x{depth} tile_mode={tile_mode} roughness={r:.6}");
            if r < best.0 {
                best = (r, tile_mode);
            }
        }
        println!("{name}: smoothest tile_mode={}", best.1);
        if let Some(dir) = &out {
            std::fs::create_dir_all(dir)?;
            let texels = untile(image, width, height, depth, best.1);
            std::fs::write(dir.join(format!("{name}.rgba16f")), texels)?;
        }
        offset += size;
    }
    Ok(())
}
