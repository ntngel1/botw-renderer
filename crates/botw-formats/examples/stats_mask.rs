//! Prints the grass-hiding masks of the world statistics maps around a
//! point, a character a metre: `h` `terrain_hidden`, `e`
//! `terrain_embedded_edge`, `i` `terrain_is_in_door` (the first that has
//! it), `.` none.
//!
//! `cargo run -p botw-formats --example stats_mask -- <content dir>... <x> <z> [half]`

use botw_formats::content::ContentRoots;
use botw_formats::stats;

const MASKS: [(&str, char); 3] = [
    ("terrain_hidden", 'h'),
    ("terrain_embedded_edge", 'e'),
    ("terrain_is_in_door", 'i'),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let numbers: Vec<f32> = args.iter().rev().map_while(|a| a.parse().ok()).collect();
    let (half, z, x) = match numbers[..] {
        [half, z, x, ..] => (half as i32, z, x),
        [z, x] => (20, z, x),
        _ => panic!("usage: stats_mask <content dir>... <x> <z> [half]"),
    };
    let dirs = &args[..args.len() - numbers.len().min(3)];
    let (roots, _) = ContentRoots::resolve(dirs);
    let names: Vec<&str> = MASKS.iter().map(|m| m.0).collect();
    let grids = stats::load_quarter(&roots, x, z, &names)
        .unwrap()
        .expect("no archive for that point");
    println!(
        "{} around ({x}, {z}), one quarter only",
        stats::quarter_name(x, z).unwrap()
    );
    let (x, z) = (x.floor() as i32, z.floor() as i32);
    let mut counts = [0; 3];
    for dz in -half..half {
        let line: String = (-half..half)
            .map(|dx| {
                let hit = MASKS.iter().position(|(name, _)| {
                    grids
                        .iter()
                        .any(|(n, g)| n == name && g.metre_bit(x + dx, z + dz))
                });
                if let Some(i) = hit {
                    counts[i] += 1;
                }
                hit.map_or('.', |i| MASKS[i].1)
            })
            .collect();
        println!("{line}");
    }
    println!(
        "hidden {} embedded {} indoor {}",
        counts[0], counts[1], counts[2]
    );
}
