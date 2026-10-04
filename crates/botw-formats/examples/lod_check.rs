//! For each shape of a model, prints per level of detail the index count,
//! the vertex range used and the longest triangle edge: a broken level
//! shows as edges far longer than the full-detail mesh's.
//!
//! `cargo run -p botw-formats --example lod_check -- <file.sbfres> [model]`

use botw_formats::bfres::Bfres;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("path");
    let wanted = args.next();
    let bytes = botw_formats::yaz0::decompress_if(&std::fs::read(&path).unwrap()).unwrap().into_owned();
    let bfres = Bfres::parse(&bytes).unwrap();
    for model in bfres.models().unwrap() {
        if wanted.as_ref().is_some_and(|w| *w != model.name) {
            continue;
        }
        for shape in &model.shapes {
            let buffer = &model.vertex_buffers[shape.vertex_buffer as usize];
            let Some(positions) = buffer.attribute("_p0") else { continue };
            let p = |i: u32| positions.values.get(i as usize).map(|v| [v[0], v[1], v[2]]);
            print!("{} {} ({} verts):", model.name, shape.name, positions.values.len());
            for lod in &shape.lods {
                let (lo, hi) = lod.indices.iter().fold((u32::MAX, 0), |(l, h), &i| (l.min(i), h.max(i)));
                let longest = lod
                    .indices
                    .chunks(3)
                    .filter_map(|t| {
                        let (a, b, c) = (p(t[0])?, p(t[1])?, p(t[2])?);
                        let d = |x: [f32; 3], y: [f32; 3]| ((x[0] - y[0]).powi(2) + (x[1] - y[1]).powi(2) + (x[2] - y[2]).powi(2)).sqrt();
                        Some(d(a, b).max(d(b, c)).max(d(a, c)))
                    })
                    .fold(0.0f32, f32::max);
                print!("  [{} idx, verts {lo}..={hi}, longest edge {longest:.1}]", lod.indices.len());
            }
            println!();
        }
    }
}
