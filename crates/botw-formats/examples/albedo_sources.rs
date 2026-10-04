//! Surveys where the model materials without an albedo texture take their
//! colour from (`uking_albedo_color`), with an example material and its
//! constant colours for each source.
//!
//! `cargo run --release -p botw-formats --example albedo_sources -- <content dir>...`

use std::collections::BTreeMap;

use botw_formats::bfres::Bfres;
use botw_formats::content::ContentRoots;

fn main() {
    let (roots, _) = ContentRoots::resolve(std::env::args().skip(1));
    let mut sources: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for (name, path) in roots.list_dir("Model") {
        if !name.ends_with(".sbfres") || name.contains(".Tex") || name.starts_with("Terrain") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(bytes) = botw_formats::yaz0::decompress_if(&bytes) else { continue };
        let Ok(bfres) = Bfres::parse(&bytes) else { continue };
        let Ok(models) = bfres.models() else { continue };
        for model in models {
            for m in &model.materials {
                if m.sampler_texture("_a0").is_some() || m.texture("tma").is_some() {
                    continue;
                }
                let source = m.shader_option("uking_albedo_color").unwrap_or("-").to_owned();
                let entry = sources.entry(source).or_insert((0, String::new()));
                entry.0 += 1;
                if entry.1.is_empty() {
                    let colors: Vec<String> = (0..8)
                        .filter_map(|k| m.shader_param(&format!("const_color{k}")).map(|c| format!("c{k}={:?}", &c[..3.min(c.len())])))
                        .collect();
                    entry.1 = format!("{} / {} / {} {}", name, model.name, m.name, colors.join(" "));
                }
            }
        }
    }
    for (source, (n, example)) in &sources {
        println!("{n:6} albedo {source}: {example}");
    }
}
