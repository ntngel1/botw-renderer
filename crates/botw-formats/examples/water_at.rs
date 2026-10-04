//! Prints the water surface (height, kind, flow) and the terrain height at
//! world positions, from the finest tiles that have them.
//!
//! `cargo run -p botw-formats --example water_at -- x,z [x,z...] -- <content dir>...`

use botw_formats::content::ContentRoots;
use botw_formats::terrain::water::WATER_SAMPLES;
use botw_formats::terrain::{MAX_LOD, TILE_SAMPLES, TerrainIndex, TileId};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|a| a == "--").expect("positions -- dirs");
    let (roots, _) = ContentRoots::resolve(&args[split + 1..]);
    let index = TerrainIndex::scan(&roots);
    for pos in &args[..split] {
        let (x, z) = pos.split_once(',').map(|(x, z)| (x.parse::<f32>().unwrap(), z.parse::<f32>().unwrap())).unwrap();
        let ground = (0..=MAX_LOD).rev().find_map(|lod| {
            let tile = TileId::containing(lod, x, z)?;
            let h = index.load_height(tile).ok()??;
            let (x0, z0) = tile.world_min();
            let s = (TILE_SAMPLES - 1) as f32 / tile.world_size();
            Some(h.sample((x - x0) * s, (z - z0) * s))
        });
        let water = (0..=MAX_LOD).rev().find_map(|lod| {
            let tile = TileId::containing(lod, x, z)?;
            let w = index.load_water(tile).ok()??;
            let (x0, z0) = tile.world_min();
            let s = (WATER_SAMPLES - 1) as f32 / tile.world_size();
            let (u, v) = ((x - x0) * s, (z - z0) * s);
            let nearest = w.get(u.round() as usize, v.round() as usize);
            Some((lod, w.height(u, v), nearest))
        });
        match water {
            Some((lod, h, s)) => println!(
                "{x},{z}: ground {:.1}, water {h:.1} (lod {lod}, kind {}, unknown {}, flow {:?})",
                ground.unwrap_or(f32::NAN),
                s.kind,
                s.unknown,
                s.flow
            ),
            None => println!("{x},{z}: ground {:.1}, no water", ground.unwrap_or(f32::NAN)),
        }
    }
}
