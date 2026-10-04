//! Counts the trees in MainField's `<cell>_TeraTree.sblwp` instance lists by
//! model, and how many of them stand where a map unit also places an actor
//! (same spot within 0.5 m), and whether its rotation (degrees here, radians
//! in map units) and scale agree.
//!
//! `cargo run -p botw-formats --example tera_trees -- <content dir>...`

use std::collections::{BTreeMap, HashMap};

use botw_formats::content::ContentRoots;
use botw_formats::map::{MainFieldUnits, PlacedActor};

fn main() {
    let (roots, _) = ContentRoots::resolve(std::env::args().skip(1));
    let title_bg = std::fs::read(roots.find("Pack/TitleBG.pack").unwrap()).unwrap();
    let sarc = roead::sarc::Sarc::new(&title_bg[..]).unwrap();
    let units = MainFieldUnits::new(roots.clone()).unwrap();
    let (mut per_model, mut shared, mut total): (BTreeMap<String, usize>, BTreeMap<String, usize>, usize) = Default::default();
    let mut scales = (f32::MAX, f32::MIN);
    let mut agree = [0usize; 2];
    for column in b'A'..=b'J' {
        for row in 1..=8 {
            let cell = format!("{}-{row}", column as char);
            let path = format!("Map/MainField/{cell}/{cell}_TeraTree.sblwp");
            let bytes = match roots.find(&path) {
                Some(file) => std::fs::read(file).unwrap(),
                None => match sarc.get_data(&path) {
                    Some(data) => data.to_vec(),
                    None => continue,
                },
            };
            let groups = botw_formats::prod::parse(&bytes).unwrap();
            let mut placed: HashMap<(i32, i32), Vec<PlacedActor>> = HashMap::new();
            for actor in units.cell(&cell).unwrap_or_default() {
                let key = ((actor.translate[0] * 2.0).round() as i32, (actor.translate[2] * 2.0).round() as i32);
                placed.entry(key).or_default().push(actor);
            }
            for group in groups {
                for instance in &group.instances {
                    total += 1;
                    scales = (scales.0.min(instance.scale), scales.1.max(instance.scale));
                    *per_model.entry(group.name.clone()).or_default() += 1;
                    let key = ((instance.translate[0] * 2.0).round() as i32, (instance.translate[2] * 2.0).round() as i32);
                    if let Some(actors) = placed.get(&key) {
                        let names: Vec<&str> = actors.iter().map(|a| a.name.as_str()).collect();
                        *shared.entry(format!("{} ~ {}", group.name, names.join("/"))).or_default() += 1;
                        if let Some(same) = actors.iter().find(|a| a.name == group.name) {
                            let rotation = same.rotate.iter().zip(instance.rotate).all(|(r, d)| (r - d.to_radians()).abs() < 1e-3);
                            agree[usize::from(rotation && (same.scale[0] - instance.scale).abs() < 1e-3)] += 1;
                        }
                    }
                }
            }
        }
    }
    println!("{total} TeraTree instances, scales {:.2}..{:.2}", scales.0, scales.1);
    for (name, n) in &per_model {
        println!("  {n:6} {name}");
    }
    println!("at the same spot as a map-unit actor: {}", shared.values().sum::<usize>());
    println!("same actor there: rotation and scale agree {}, differ {}", agree[1], agree[0]);
    for (name, n) in shared.iter().take(40) {
        println!("  {n:6} {name}");
    }
}
