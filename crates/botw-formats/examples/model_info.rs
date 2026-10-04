//! Prints a BFRES model's structure: bones, vertex attributes, shapes and
//! materials (textures, render info, shader options).
//!
//! `cargo run -p botw-formats --example model_info -- <file.sbfres> [model]`
//! or `... -- <pack> <entry.sbfres> [model]` for a BFRES inside a SARC pack.

use botw_formats::bfres::Bfres;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("path");
    let mut wanted = args.next();
    let mut bytes = botw_formats::yaz0::decompress_if(&std::fs::read(&path).unwrap())
        .unwrap()
        .into_owned();
    if let Some(entry) = wanted.clone().filter(|w| w.ends_with("bfres")) {
        let sarc = roead::sarc::Sarc::new(&bytes[..]).unwrap();
        let data = sarc.get_data(&entry).expect("entry in pack").to_vec();
        bytes = botw_formats::yaz0::decompress_if(&data)
            .unwrap()
            .into_owned();
        wanted = args.next();
    }
    let bfres = Bfres::parse(&bytes).unwrap();
    for model in bfres.models().unwrap() {
        if wanted.as_ref().is_some_and(|w| *w != model.name) {
            println!("model {} (skipped)", model.name);
            continue;
        }
        println!(
            "model {}: {} bones, {} vertex buffers, {} shapes, {} materials",
            model.name,
            model.bones.len(),
            model.vertex_buffers.len(),
            model.shapes.len(),
            model.materials.len()
        );
        for b in &model.bones {
            println!(
                "  bone {} parent {:?} s {:?} r {:?} euler {} t {:?}",
                b.name, b.parent, b.scale, b.rotation, b.euler, b.translation
            );
        }
        for (i, vb) in model.vertex_buffers.iter().enumerate() {
            let attrs: Vec<String> = vb
                .attributes
                .iter()
                .map(|a| {
                    format!(
                        "{}:{:#x}={:?}",
                        a.name,
                        a.format,
                        a.values
                            .first()
                            .map(|v| v.map(|x| (x * 1000.0).round() / 1000.0))
                    )
                })
                .collect();
            println!(
                "  vb{i}: {} verts skin {} | {}",
                vb.vertex_count,
                vb.skin_count,
                attrs.join(" ")
            );
        }
        for s in &model.shapes {
            let lods: Vec<String> = s
                .lods
                .iter()
                .map(|m| format!("{}", m.indices.len()))
                .collect();
            println!(
                "  shape {} mat {} bone {} vb {} skin {} lods [{}]",
                s.name,
                s.material,
                s.bone,
                s.vertex_buffer,
                s.skin_count,
                lods.join(",")
            );
        }
        for m in &model.materials {
            println!(
                "  material {} ({} / {})",
                m.name, m.shader_archive, m.shading_model
            );
            println!("    textures {:?}", m.textures);
            println!("    render state {:?}", m.render_state);
            let params: Vec<String> = m
                .shader_params
                .iter()
                .map(|p| format!("{}({})={:?}", p.name, p.kind, p.values))
                .collect();
            println!("    params {}", params.join(" "));
            println!("    render info {:?}", m.render_info);
            // `ALL_OPTIONS=1` lists the options left at 0 or -1 too.
            let all = std::env::var_os("ALL_OPTIONS").is_some();
            let options: Vec<&(String, String)> = m
                .shader_options
                .iter()
                .filter(|(_, v)| all || (v != "0" && v != "-1"))
                .collect();
            println!("    options (non-zero) {:?}", options);
            println!("    sampler assign {:?}", m.sampler_assign);
            for (name, [word0, _, _]) in &m.samplers {
                // SQ_TEX_SAMPLER_WORD0: clamp x/y (0 wrap, 1 mirror, 2 clamp,
                // …), xy mag/min filter (0 point, 1 bilinear), mip filter
                // (0 none, 1 point, 2 linear).
                println!(
                    "    sampler {name}: clamp {}/{} mag {} min {} mip {}",
                    word0 & 7,
                    word0 >> 3 & 7,
                    word0 >> 9 & 3,
                    word0 >> 12 & 3,
                    word0 >> 17 & 3
                );
            }
        }
    }
}
