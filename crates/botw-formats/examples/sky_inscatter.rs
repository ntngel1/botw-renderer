//! Loads the sky's inscatter table (`sky.skybin` in `Bootup_Graphics.pack`)
//! through the content roots and prints its channel ranges; with
//! `--compare FILE` also the largest difference from an untiled
//! `inscatter.rgba16f` (the `sky_skybin` example's output).
//! `cargo run --offline -p botw-formats --example sky_inscatter -- <content dir>... [--compare FILE]`

use botw_formats::content::ContentRoots;
use botw_formats::sky::{Inscatter, load_inscatter};

/// Binary16 to f32 (the table holds no infinities or NaNs).
fn half(bits: u16) -> f32 {
    let sign = if bits >> 15 == 1 { -1.0 } else { 1.0 };
    let exp = (bits >> 10 & 0x1f) as i32;
    let man = (bits & 0x3ff) as f32;
    sign * if exp == 0 {
        man * 2f32.powi(-24)
    } else {
        (1.0 + man / 1024.0) * 2f32.powi(exp - 15)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut dirs = Vec::new();
    let mut compare = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compare" => compare = Some(args.next().ok_or("--compare FILE")?),
            _ => dirs.push(arg),
        }
    }
    let (roots, _) = ContentRoots::resolve(&dirs);
    let table = load_inscatter(&roots)?.ok_or("no Pack/Bootup_Graphics.pack or sky.skybin")?;
    println!(
        "inscatter {}x{}x{}",
        Inscatter::WIDTH,
        Inscatter::HEIGHT,
        Inscatter::DEPTH
    );
    for c in 0..4 {
        let (lo, hi) = table
            .texels
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), t| {
                (lo.min(t[c]), hi.max(t[c]))
            });
        println!("channel {c}: {lo} … {hi}");
    }
    if let Some(path) = compare {
        let bytes = std::fs::read(path)?;
        let other: Vec<f32> = bytes
            .chunks_exact(2)
            .map(|h| half(u16::from_le_bytes([h[0], h[1]])))
            .collect();
        if other.len() != table.texels.len() * 4 {
            return Err("comparison file has a different size".into());
        }
        let max = table
            .texels
            .iter()
            .flatten()
            .zip(&other)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        println!("max difference from the comparison file: {max}");
    }
    Ok(())
}
