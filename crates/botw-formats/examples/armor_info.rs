//! Prints armour pieces: slot, models, set, effect and what they change on
//! Link (ears, Sheikah Slate pouch).
//!
//! `cargo run -p botw-formats --example armor_info -- <content dir>... -- Armor_001_Head [more actors]`

use std::sync::Arc;

use botw_formats::actor::ActorPacks;
use botw_formats::armor::Armor;
use botw_formats::content::ContentRoots;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|a| a == "--").expect("dirs -- actors");
    let (roots, _) = ContentRoots::resolve(&args[..split]);
    let title_bg = roots.find("Pack/TitleBG.pack").and_then(|p| std::fs::read(p).ok()).map(Arc::new);
    let packs = ActorPacks::new(roots, title_bg);
    for actor in &args[split + 1..] {
        match Armor::read(&packs, actor) {
            Ok(Some(armor)) => {
                let models: Vec<String> = armor.models.iter().map(|m| format!("{}: {}", m.folder, m.units.join(" "))).collect();
                println!(
                    "{actor}: {:?}, models [{}], series {}, effect {} {}{}, ears {:?}{}",
                    armor.slot,
                    models.join("; "),
                    armor.series,
                    armor.effect,
                    armor.effect_level,
                    if armor.set_bonus { ", set bonus" } else { "" },
                    armor.ear_rotate,
                    if armor.hides_pouch { ", hides the pouch" } else { "" }
                );
            }
            Ok(None) => println!("{actor}: not an armour actor in this dump"),
            Err(error) => println!("{actor}: {error}"),
        }
    }
}
