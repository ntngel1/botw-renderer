//! Stitches one level of detail of MainField heights into a grayscale PGM
//! (north up, east right), to eyeball axis orientation against the game map.
//!
//! `cargo run -p botw-formats --example height_map -- <lod> <out.pgm> <content dir>...`

use botw_formats::content::ContentRoots;
use botw_formats::terrain::{TILE_SAMPLES, TerrainIndex, TileId};

fn main() {
    let mut args = std::env::args().skip(1);
    let lod: u8 = args.next().and_then(|s| s.parse().ok()).expect("lod");
    let out = args.next().expect("output path");
    let (roots, _) = ContentRoots::resolve(args);
    let index = TerrainIndex::scan(&roots);

    // One pixel per sample interval; tiles share edge samples.
    let per_axis = 1usize << lod;
    let step = TILE_SAMPLES - 1;
    let side = per_axis * step;
    let mut pixels = vec![0u8; side * side];
    let mut missing = 0;
    for gz in 0..per_axis {
        for gx in 0..per_axis {
            let tile = TileId::from_grid(lod, gx as u8, gz as u8).unwrap();
            let Ok(Some(height)) = index.load_height(tile) else {
                missing += 1;
                continue;
            };
            for z in 0..step {
                for x in 0..step {
                    let raw = height.raw(x, z);
                    pixels[(gz * step + z) * side + gx * step + x] = (raw >> 8) as u8;
                }
            }
        }
    }
    let mut file = format!("P5\n{side} {side}\n255\n").into_bytes();
    file.extend_from_slice(&pixels);
    std::fs::write(&out, file).expect("write");
    println!("{side}×{side}, {missing} tiles missing");
}
