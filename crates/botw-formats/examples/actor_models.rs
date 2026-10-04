//! Prints which BFRES models actors draw (from their packs' model lists).
//!
//! `cargo run -p botw-formats --example actor_models -- <actor>[,<actor>...] <content dir>...`

use std::sync::Arc;

use botw_formats::actor::ActorPacks;
use botw_formats::content::ContentRoots;

fn main() {
    let mut args = std::env::args().skip(1);
    let actors = args.next().expect("actor names");
    let (roots, _) = ContentRoots::resolve(args);
    let title_bg = roots.find("Pack/TitleBG.pack").and_then(|p| std::fs::read(p).ok()).map(Arc::new);
    let packs = ActorPacks::new(roots, title_bg);
    for actor in actors.split(',') {
        println!("{actor}: {:?}", packs.models(actor));
    }
}
