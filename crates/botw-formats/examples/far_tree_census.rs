//! Counts the placed MainField actors that have a far-tree billboard in
//! `Terrain.Tex1` (by name, or by their `mainModel`), and the tree-tagged
//! ones that have none.
//!
//! `cargo run -p botw-formats --example far_tree_census -- <content dir>...`

use std::collections::{BTreeMap, HashMap};

use botw_formats::bfres::Bfres;
use botw_formats::content::ContentRoots;
use botw_formats::map::MainFieldUnits;
use botw_formats::trees::{ATLASES, billboard_views, read_actor_info};

fn main() {
    let (roots, _) = ContentRoots::resolve(std::env::args().skip(1));
    let tex1 = std::fs::read(roots.find("Model/Terrain.Tex1.sbfres").unwrap()).unwrap();
    let tex1 = botw_formats::yaz0::decompress_if(&tex1).unwrap().into_owned();
    let bfres = Bfres::parse(&tex1).unwrap();
    let mut views = HashMap::new();
    for (atlas, (albedo, _)) in ATLASES.iter().enumerate() {
        let texture = bfres.texture(albedo).unwrap().unwrap();
        let files = texture.user_data.iter().find(|(k, _)| k == "file").map(|(_, v)| v.clone()).unwrap_or_default();
        for (actor, list) in billboard_views(&files) {
            views.insert(actor, (atlas, list.len()));
        }
    }
    let info = read_actor_info(&roots).unwrap();
    let units = MainFieldUnits::new(roots).unwrap();
    let started = std::time::Instant::now();
    let (mut with, mut without): (BTreeMap<String, usize>, BTreeMap<String, usize>) = Default::default();
    let mut total = 0;
    for column in b'A'..=b'J' {
        for row in 1..=8 {
            for actor in units.cell(&format!("{}-{row}", column as char)).unwrap_or_default() {
                total += 1;
                let entry = info.get(&actor.name);
                let model = entry.and_then(|i| i.main_model.clone());
                let billboard = views.contains_key(&actor.name) || model.as_ref().is_some_and(|m| views.contains_key(m));
                if billboard {
                    *with.entry(actor.name).or_default() += 1;
                } else if entry.is_some_and(|i| i.has_tag("Tree") || i.has_tag("RenderingUseTeraTree")) {
                    *without.entry(actor.name).or_default() += 1;
                }
            }
        }
    }
    println!("{total} actors read in {:.1} s", started.elapsed().as_secs_f32());
    println!("with a billboard: {} placements", with.values().sum::<usize>());
    for (name, n) in &with {
        println!("  {n:6} {name}");
    }
    println!("tree-tagged without one: {} placements", without.values().sum::<usize>());
    for (name, n) in &without {
        println!("  {n:6} {name}");
    }
}
