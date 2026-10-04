//! Prints what each glowing material of BFRES files emits as far as
//! `Material::emission` follows its recipe (texture channels × colour), and
//! the raw recipe of the ones it cannot follow.
//!
//! `cargo run -p botw-formats --example emission_terms -- <file.sbfres>...`

use botw_formats::bfres::Bfres;
use botw_formats::bfres::model::Material;

fn option(m: &Material, key: &str) -> String {
    m.shader_option(key).unwrap_or("?").to_owned()
}

/// The inputs of computed colour `n`, recursively.
fn recipe(m: &Material, value: &str, depth: u32) -> String {
    let Ok(v) = value.parse::<u32>() else { return value.to_owned() };
    match v {
        200..=209 if depth < 4 => {
            let n = v - 200;
            let part = |p: &str| {
                let input = option(m, &format!("uking_color{n}_{p}"));
                format!("{}.{}", recipe(m, &input, depth + 1), option(m, &format!("uking_color{n}_{p}_channel")))
            };
            format!(
                "c{n}[on {} calc {}: {} {} {} {}]",
                option(m, &format!("uking_enable_calc_color{n}")),
                option(m, &format!("uking_color{n}_calc_type")),
                part("A"),
                part("B"),
                part("C"),
                part("D")
            )
        }
        100..=107 => {
            let c = m.shader_param(&format!("const_color{}", v - 100)).unwrap_or(&[]);
            format!("k{}{:?}", v - 100, c.iter().take(3).collect::<Vec<_>>())
        }
        0..=7 => format!("s{v}:{}", m.slot_texture(v as usize).map_or("-", |(_, t)| t)),
        _ => value.to_owned(),
    }
}

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = botw_formats::yaz0::decompress_if(&std::fs::read(&path).unwrap()).unwrap().into_owned();
        let bfres = Bfres::parse(&bytes).unwrap();
        for model in bfres.models().unwrap() {
            for m in &model.materials {
                if m.shader_option("uking_enable_emission") != Some("1") {
                    continue;
                }
                match m.emission() {
                    Some(term) => println!("{} / {}: {:?} × {:?}", model.name, m.name, term.textures().collect::<Vec<_>>(), term.color),
                    None => println!("{} / {}: not followed: {}", model.name, m.name, recipe(m, &option(m, "uking_emission_color"), 0)),
                }
            }
        }
    }
}
