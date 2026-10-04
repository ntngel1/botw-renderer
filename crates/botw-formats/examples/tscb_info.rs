//! Summarises a terrain scene: header, material table and tiles per level.
//!
//! `cargo run -p botw-formats --example tscb_info -- <MainField.tscb>`

use botw_formats::terrain::{MAX_LOD, Tscb};

fn main() {
    let path = std::env::args().nth(1).expect("path to a .tscb file");
    let tscb = Tscb::parse(&std::fs::read(path).expect("read")).expect("parse");
    println!(
        "world_scale {} max_height {} tile_size {} materials {} areas {}",
        tscb.world_scale,
        tscb.max_height,
        tscb.tile_size,
        tscb.materials.len(),
        tscb.areas.len()
    );
    for lod in 0..=MAX_LOD {
        let areas: Vec<_> = tscb.areas.iter().filter(|a| a.tile.lod() == lod).collect();
        let water = areas.iter().filter(|a| a.has_water_data).count();
        let grass = areas.iter().filter(|a| a.has_grass_data).count();
        let size = areas.first().map_or(0.0, |a| a.size);
        println!("  lod {lod}: {:5} tiles of {size:7.1} m, {water:4} with water, {grass:4} with grass", areas.len());
    }
    for m in tscb.materials.iter().take(4) {
        println!("  material {m:?}");
    }
}
