//! Prints the game's water colour table (`WaterAlb` in `Terrain.Tex2`,
//! inside `Pack/TitleBG.pack`): seven RGBA texels per water kind, then
//! the `TeraWater` material parameters the viewer's water shader reads.
//!
//! `cargo run -p botw-formats --example water_table -- <content dir>...`

use botw_formats::content::ContentRoots;
use botw_formats::terrain::textures::{WaterTable, load_water_material};

const NAMES: [&str; 8] = [
    "Water", "HotWater", "Poison", "Lava", "IceWater", "Mud", "Clear01", "Sea",
];

fn main() {
    let dirs: Vec<String> = std::env::args().skip(1).collect();
    let (roots, _) = ContentRoots::resolve(&dirs);
    let table = WaterTable::load(&roots)
        .expect("TitleBG.pack")
        .expect("no WaterAlb");
    for (name, texels) in NAMES.iter().zip(&table.kinds) {
        println!("{name}");
        for (x, t) in texels.iter().enumerate() {
            println!("  {x}: {:8.4} {:8.4} {:8.4} {:8.4}", t[0], t[1], t[2], t[3]);
        }
    }
    let material = load_water_material(&roots)
        .expect("TitleBG.pack")
        .expect("no Terrain.sbfres");
    println!("TeraWater material {}", material.name);
    for param in material.shader_params.iter().filter(|p| {
        ["tex_srt", "const_", "indirect_scale"]
            .iter()
            .any(|k| p.name.starts_with(k))
    }) {
        println!("  {} = {:?}", param.name, param.values);
    }
}
