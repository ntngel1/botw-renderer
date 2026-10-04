//! Surveys every model material in `Model/*.sbfres`: which samplers they
//! use, and for materials with a given sampler, which shader options and
//! parameters (names matching a filter) come with it and how often.
//!
//! `cargo run --release -p botw-formats --example material_census -- <sampler> <name filter> <content dir>...`
//! (e.g. `_e0 emission`; `-` as the sampler just counts samplers). Examples
//! are listed from files starting with `PREFIX`. `COMBO=a,b,c` also counts
//! the combinations of those options' values (`uking_` may be left out;
//! `$emission_color` follows the option's value to the colour it names, see
//! archived FORMATS.md notes). Render info entries matching the filter are counted
//! too; `RINFO=key=value` lists the materials that carry that entry.

use std::collections::BTreeMap;

use botw_formats::bfres::Bfres;
use botw_formats::content::ContentRoots;

fn main() {
    let mut args = std::env::args().skip(1);
    let sampler = args.next().expect("sampler");
    let filter = args.next().expect("filter").to_lowercase();
    let (roots, _) = ContentRoots::resolve(args);
    type Counts = BTreeMap<String, usize>;
    let (mut samplers, mut options, mut params, mut examples): (
        Counts,
        Counts,
        Counts,
        Vec<String>,
    ) = Default::default();
    let mut with = 0;
    // Materials whose emission, metal and specular maps can be followed.
    let mut followed = [0usize; 3];
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut combos: BTreeMap<String, usize> = BTreeMap::new();
    let (mut render_info, mut carriers): (Counts, Vec<String>) = Default::default();
    let rinfo = std::env::var("RINFO")
        .ok()
        .and_then(|r| r.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())));
    let combo: Vec<String> = std::env::var("COMBO")
        .map(|c| c.split(',').map(str::to_owned).collect())
        .unwrap_or_default();
    for (name, path) in roots.list_dir("Model") {
        if !name.ends_with(".sbfres") || name.contains(".Tex") || name.starts_with("Terrain") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(bytes) = botw_formats::yaz0::decompress_if(&bytes) else {
            continue;
        };
        let Ok(bfres) = Bfres::parse(&bytes) else {
            continue;
        };
        let Ok(models) = bfres.models() else { continue };
        for model in models {
            for m in &model.materials {
                for (s, _) in &m.textures {
                    *samplers.entry(s.clone()).or_default() += 1;
                }
                let Some((_, texture)) = m.textures.iter().find(|(s, _)| *s == sampler) else {
                    continue;
                };
                with += 1;
                followed[0] += usize::from(m.emission().is_some());
                followed[1] += usize::from(m.metalness().is_some());
                followed[2] += usize::from(m.specular_mask().is_some());
                *kinds
                    .entry(name.split('_').next().unwrap_or("").to_owned())
                    .or_default() += 1;
                if !combo.is_empty() {
                    let option = |key: &str| {
                        let key = if key.starts_with("uking_") || key.starts_with("gsys_") {
                            key.to_owned()
                        } else {
                            format!("uking_{key}")
                        };
                        m.shader_options
                            .iter()
                            .find(|(k, _)| *k == key)
                            .map_or("?".to_owned(), |(_, v)| v.clone())
                    };
                    let values: Vec<String> = combo
                        .iter()
                        .map(|key| match key.strip_prefix('$') {
                            // A computed colour `2xx`: its calculation and inputs.
                            Some(key) => {
                                let value = option(key);
                                match value.parse::<u32>() {
                                    Ok(v @ 200..=299) => {
                                        let n = v - 200;
                                        let part = |p: &str| {
                                            format!(
                                                "{}.{}",
                                                option(&format!("color{n}_{p}")),
                                                option(&format!("color{n}_{p}_channel"))
                                            )
                                        };
                                        format!(
                                            "{key}={value}[on {} calc {} A {} B {} C {} D {}]",
                                            option(&format!("enable_calc_color{n}")),
                                            option(&format!("color{n}_calc_type")),
                                            part("A"),
                                            part("B"),
                                            part("C"),
                                            part("D")
                                        )
                                    }
                                    _ => format!("{key}={value}"),
                                }
                            }
                            None => format!("{key}={}", option(key)),
                        })
                        .collect();
                    let assign: Vec<String> = m
                        .sampler_assign
                        .iter()
                        .map(|(k, v)| format!("{k}<{v}"))
                        .collect();
                    *combos
                        .entry(format!("{} | {}", values.join(" "), assign.join(" ")))
                        .or_default() += 1;
                }
                for (k, v) in &m.shader_options {
                    if k.to_lowercase().contains(&filter) {
                        *options.entry(format!("{k} = {v}")).or_default() += 1;
                    }
                }
                for (k, v) in &m.render_info {
                    if k.to_lowercase().contains(&filter) {
                        *render_info.entry(format!("{k} = {v}")).or_default() += 1;
                    }
                    if rinfo.as_ref().is_some_and(|(rk, rv)| rk == k && rv == v) {
                        carriers.push(format!("{name} {} / {}", model.name, m.name));
                    }
                }
                for p in &m.shader_params {
                    if p.name.to_lowercase().contains(&filter) {
                        let values: Vec<String> =
                            p.values.iter().map(|v| format!("{v:.2}")).collect();
                        *params
                            .entry(format!("{} = [{}]", p.name, values.join(", ")))
                            .or_default() += 1;
                    }
                }
                if examples.len() < 40
                    && std::env::var("PREFIX").is_ok_and(|p| name.starts_with(&p))
                {
                    let all: Vec<String> =
                        m.textures.iter().map(|(s, t)| format!("{s}={t}")).collect();
                    examples.push(format!(
                        "{name} {} / {}: {texture} | {}",
                        model.name,
                        m.name,
                        all.join(" ")
                    ));
                }
            }
        }
    }
    println!("samplers:");
    for (s, n) in &samplers {
        println!("  {n:7} {s}");
    }
    if sampler == "-" {
        return;
    }
    println!(
        "{with} materials with {sampler}; emission / metal / specular followed in {followed:?}; by file prefix:"
    );
    for (k, n) in &kinds {
        println!("  {n:7} {k}");
    }
    if !combo.is_empty() {
        println!("combinations:");
        let mut by_count: Vec<_> = combos.iter().collect();
        by_count.sort_by(|a, b| b.1.cmp(a.1));
        for (c, n) in by_count.iter().take(40) {
            println!("  {n:7} {c}");
        }
    }
    println!("options matching {filter:?}:");
    for (o, n) in &options {
        println!("  {n:7} {o}");
    }
    println!("render info matching {filter:?}:");
    for (r, n) in &render_info {
        println!("  {n:7} {r}");
    }
    if let Some((k, v)) = &rinfo {
        println!("materials with {k} = {v}:");
        for c in &carriers {
            println!("  {c}");
        }
    }
    println!("parameters matching {filter:?} (by value):");
    let mut by_count: Vec<_> = params.into_iter().collect();
    by_count.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (p, n) in by_count.iter().take(60) {
        println!("  {n:7} {p}");
    }
    println!("examples:");
    for e in &examples {
        println!("  {e}");
    }
}
