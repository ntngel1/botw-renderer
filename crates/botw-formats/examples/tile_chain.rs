//! For world positions, lists the tile chain from the root down with TSCB
//! flags: which levels have terrain, water and grass data.
//!
//! `cargo run -p botw-formats --example tile_chain -- <MainField.tscb> x,z [x,z...]`

use botw_formats::terrain::{MAX_LOD, TileId, Tscb};

fn main() {
    let mut args = std::env::args().skip(1);
    let tscb = Tscb::parse(&std::fs::read(args.next().expect("tscb")).unwrap()).unwrap();
    for pos in args {
        let (x, z) = pos.split_once(',').map(|(x, z)| (x.parse::<f32>().unwrap(), z.parse::<f32>().unwrap())).unwrap();
        let chain: Vec<String> = (0..=MAX_LOD)
            .filter_map(|lod| {
                let tile = TileId::containing(lod, x, z)?;
                let area = tscb.areas.iter().find(|a| a.tile == tile)?;
                Some(format!(
                    "{lod}{}{}",
                    if area.has_water_data { "w" } else { "" },
                    if area.has_grass_data { "g" } else { "" }
                ))
            })
            .collect();
        println!("{x},{z}: {}", chain.join(" "));
    }
}
