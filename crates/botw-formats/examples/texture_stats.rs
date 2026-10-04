//! Prints textures' formats and per-channel statistics (level 0, layer 0),
//! to tell what each channel of a material map holds.
//!
//! `cargo run -p botw-formats --example texture_stats -- <X.Tex1.sbfres> <texture>...`

use botw_formats::bfres::{Bfres, assemble_texture};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("Tex1 file");
    let bytes = botw_formats::yaz0::decompress_if(&std::fs::read(&path).unwrap()).unwrap().into_owned();
    let bfres = Bfres::parse(&bytes).unwrap();
    let names: Vec<String> = args.collect();
    let names = if names.is_empty() { bfres.texture_names().map(str::to_owned).collect() } else { names };
    for name in names {
        let Some(texture) = bfres.texture(&name).unwrap() else {
            println!("{name}: not found");
            continue;
        };
        let image = assemble_texture(&texture, None).unwrap();
        let Some(rgba) = image.decode_rgba8(0) else {
            println!("{name}: {:?} not decodable", image.format);
            continue;
        };
        let n = (rgba.len() / 4) as f64;
        let stats: Vec<String> = (0..4)
            .map(|c| {
                let values = rgba.iter().skip(c).step_by(4).map(|&v| f64::from(v));
                let (sum, min, max) = values.fold((0.0, 255.0f64, 0.0f64), |(s, lo, hi), v| (s + v, lo.min(v), hi.max(v)));
                format!("{}: {:.0} ({min:.0}-{max:.0})", ["R", "G", "B", "A"][c], sum / n)
            })
            .collect();
        println!("{name} {}x{} {:?} select {:?} | {}", image.width, image.height, image.format, image.component_select, stats.join("  "));
    }
}
