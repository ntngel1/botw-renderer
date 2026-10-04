//! Prints the field's climate map from `Ecosystem/FieldMapArea.sbeco` and
//! `AreaData.sbyml` (`Pack/Bootup.pack`): a coarse map with one letter per
//! climate, the areas, and the area and climate at given points.
//!
//! `cargo run -p botw-formats --example eco_info -- <content dir>... [x,z]...`

use botw_formats::content::ContentRoots;
use botw_formats::eco::{CLIMATES, Ecosystem};

fn main() {
    let (dirs, points): (Vec<String>, Vec<String>) = std::env::args().skip(1).partition(|a| !a.contains(','));
    let (roots, _) = ContentRoots::resolve(&dirs);
    let eco = Ecosystem::load(&roots).unwrap().expect("no Pack/Bootup.pack");
    println!("{} areas", eco.areas.len());
    let letter = |climate: usize| (b'A' + climate as u8) as char;
    for (i, climate) in CLIMATES.iter().enumerate() {
        println!("  {} {climate}", letter(i));
    }
    // North is up: rows go from z −4000 to 4000.
    for row in 0..40 {
        let z = -4000.0 + (row as f32 + 0.5) * 200.0;
        let line: String = (0..100).map(|col| letter(eco.climate_at(-5000.0 + (col as f32 + 0.5) * 100.0, z))).collect();
        println!("{line}");
    }
    for point in points {
        let mut parts = point.split(',').map(|p| p.parse::<f32>().expect("x,z"));
        let (x, z) = (parts.next().unwrap(), parts.next().unwrap());
        let area = eco.area_at(x, z);
        println!("({x}, {z}): area {:?} {:?}, climate {}", eco.map.at(x, z), area.map(|a| &a.name), CLIMATES[eco.climate_at(x, z)]);
    }
}
