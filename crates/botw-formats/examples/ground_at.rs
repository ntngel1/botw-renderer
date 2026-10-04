//! Prints the terrain height from every level of detail at world positions.
//!
//! `cargo run -p botw-formats --example ground_at -- x,z [x,z...] -- <content dir>...`

use botw_formats::content::ContentRoots;
use botw_formats::terrain::{MAX_LOD, TILE_SAMPLES, TerrainIndex, TileId};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|a| a == "--").expect("positions -- dirs");
    let (roots, _) = ContentRoots::resolve(&args[split + 1..]);
    let index = TerrainIndex::scan(&roots);
    for pos in &args[..split] {
        let (x, z) = pos.split_once(',').map(|(x, z)| (x.parse::<f32>().unwrap(), z.parse::<f32>().unwrap())).unwrap();
        let heights: Vec<String> = (0..=MAX_LOD)
            .map(|lod| {
                let tile = TileId::containing(lod, x, z).unwrap();
                match index.load_height(tile) {
                    Ok(Some(h)) => {
                        let (x0, z0) = tile.world_min();
                        let s = (TILE_SAMPLES - 1) as f32 / tile.world_size();
                        format!("{lod}:{:.1}", h.sample((x - x0) * s, (z - z0) * s))
                    }
                    _ => format!("{lod}:-"),
                }
            })
            .collect();
        println!("{x},{z}: {}", heights.join(" "));
    }
}
