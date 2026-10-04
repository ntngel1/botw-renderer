//! Lists placed actors whose name contains a pattern, near a world position.
//!
//! `cargo run -p botw-formats --example actors_near -- x,z radius pattern <content dir>...`

use botw_formats::content::ContentRoots;
use botw_formats::map::{MainFieldUnits, cell_name};

fn main() {
    let mut args = std::env::args().skip(1);
    let (x, z) = args
        .next()
        .and_then(|p| p.split_once(',').map(|(x, z)| (x.parse::<f32>().unwrap(), z.parse::<f32>().unwrap())))
        .expect("x,z");
    let radius: f32 = args.next().and_then(|r| r.parse().ok()).expect("radius");
    let pattern = args.next().expect("pattern");
    let (roots, _) = ContentRoots::resolve(args);
    let units = MainFieldUnits::new(roots).unwrap();

    let mut cells: Vec<String> = [-1.0f32, 0.0, 1.0]
        .iter()
        .flat_map(|dx| [-1.0f32, 0.0, 1.0].map(|dz| cell_name(x + dx * radius, z + dz * radius)))
        .flatten()
        .collect();
    cells.sort();
    cells.dedup();
    let mut found = Vec::new();
    for cell in &cells {
        for actor in units.cell(cell).unwrap() {
            let [ax, _, az] = actor.translate;
            let d = ((ax - x).powi(2) + (az - z).powi(2)).sqrt();
            if d <= radius && actor.name.contains(&pattern) {
                found.push((d, cell.clone(), actor));
            }
        }
    }
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (d, cell, a) in &found {
        let [ax, ay, az] = a.translate;
        println!("{d:7.1} m {cell} {:36} {ax:8.1} {ay:6.1} {az:8.1} rot {:?} scale {:?} {:?}", a.name, a.rotate, a.scale, a.params);
    }
    println!("{} actors in {:?}", found.len(), cells);
}
