//! Prints the structure of a PTCL effect file (`*.sesetlist`): emitter
//! sets and their emitters, textures, primitives. A file inside a pack is
//! given as `<pack>:<entry>`, e.g. `Pack/Bootup.pack:Effect/GameResident.sesetlist`.
//! `filter` limits the emitter sets listed to names containing it.
//! Resident textures are looked up in `$RESIDENT` (given the same way). With
//! `$OUT` set to a directory, the textures the listed emitters use (and,
//! without a filter, all of the file's own) are written there as PNG;
//! `$MIP` picks the mip level (default 0); `$BG=rrggbb` blends them over that
//! colour by their alpha instead of keeping it, to see what a particle cuts out.
//!
//! `cargo run -p botw-formats --example ptcl_info -- <file> [filter]`

use botw_formats::ptcl::{Emitter, Ptcl};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("effect file");
    let filter = args.next().unwrap_or_default();
    let bytes = read(&path);
    let ptcl = Ptcl::parse(&bytes).unwrap();
    println!("EFTB version {} \"{}\"", ptcl.version, ptcl.name);

    let sets: Vec<_> = ptcl.emitter_sets.iter().filter(|s| s.name.contains(&filter)).collect();
    println!("{} emitter sets ({} listed)", ptcl.emitter_sets.len(), sets.len());
    for set in sets {
        println!("  set \"{}\"", set.name);
        for emitter in &set.emitters {
            print_emitter(emitter, 2);
        }
    }

    println!("{} textures", ptcl.textures.len());
    for t in &ptcl.textures {
        let s = t.surface();
        let layout = s.mip_offsets[0] + s.mip_size;
        let check = match (s.mip_count, s.image_size) {
            (_, 0) => "size unknown".to_string(),
            (1, size) if size as usize <= t.data.len() => "fits".to_string(),
            (_, _) if layout as usize == t.data.len() => "layout matches".to_string(),
            _ => format!("layout ends at {layout}"),
        };
        println!(
            "  {:08x} {}x{} format {:#x} mips {} tile {} swizzle {:#x} select {:?} ({} bytes, {check})",
            t.id,
            t.width,
            t.height,
            t.format,
            t.mip_count,
            t.tile_mode,
            t.swizzle,
            t.component_select,
            t.data.len()
        );
    }
    println!("{} primitives", ptcl.primitives.len());
    for p in &ptcl.primitives {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in &p.positions {
            for i in 0..3 {
                lo[i] = lo[i].min(v[i]);
                hi[i] = hi[i].max(v[i]);
            }
        }
        println!(
            "  {:08x}: {} vertices ({} normals, {} tangents, {} colours, {} uvs), {} triangles, bounds {lo:?}..{hi:?}",
            p.id,
            p.positions.len(),
            p.normals.len(),
            p.tangents.len(),
            p.colors.len(),
            p.uvs.len(),
            p.indices.len() / 3
        );
    }
    println!("{} bytes of shaders", ptcl.shader_bytes);

    // Samplers must point at textures here or, when marked resident, in
    // `$RESIDENT` (GameResident.sesetlist) if given.
    let resident_bytes = std::env::var("RESIDENT").ok().map(|p| read(&p));
    let resident = resident_bytes.as_ref().map(|b| Ptcl::parse(b).unwrap());
    let (mut found, mut missing) = (0, Vec::new());
    let mut used = Vec::new();
    for e in ptcl.emitter_sets.iter().flat_map(|s| s.all_emitters()) {
        for s in e.samplers.iter().flatten() {
            let file = if s.resident { resident.as_ref() } else { Some(&ptcl) };
            match file.map(|f| f.texture(s.texture)) {
                Some(Some(t)) => {
                    found += 1;
                    if sets_listed(&ptcl, &filter).any(|set| set.all_emitters().iter().any(|x| std::ptr::eq(*x, e))) {
                        used.push(t.clone());
                    }
                }
                Some(None) => missing.push(format!("{}:{:08x}", e.name, s.texture)),
                None => {}
            }
        }
    }
    println!("sampler textures found: {found}, missing: {} {:?}", missing.len(), &missing[..missing.len().min(10)]);
    let meshes: Vec<u64> = ptcl.emitter_sets.iter().flat_map(|s| s.all_emitters()).filter_map(|e| e.particle.primitive).collect();
    let local = meshes.iter().filter(|&&id| ptcl.primitive(id).is_some()).count();
    let in_resident = meshes.iter().filter(|&&id| resident.as_ref().is_some_and(|r| r.primitive(id).is_some())).count();
    println!("particle meshes: {} ({local} here, {in_resident} in the resident file)", meshes.len());

    if let Ok(out) = std::env::var("OUT") {
        if filter.is_empty() {
            used.extend(ptcl.textures.iter().cloned());
        }
        used.sort_by_key(|t| t.id);
        used.dedup_by_key(|t| t.id);
        std::fs::create_dir_all(&out).unwrap();
        let mip: u32 = std::env::var("MIP").ok().and_then(|m| m.parse().ok()).unwrap_or(0);
        for t in &used {
            let path = if mip == 0 { format!("{out}/{:08x}.png", t.id) } else { format!("{out}/{:08x}_mip{mip}.png", t.id) };
            match t.image() {
                Ok(image) => match image.decode_rgba8(mip) {
                    Some(rgba) => {
                        let rgba = apply_component_select(&rgba, image.component_select);
                        let rgba = match std::env::var("BG") { Ok(bg) => over_background(&rgba, &bg), Err(_) => rgba };
                        let (w, h) = image.level_size(mip);
                        std::fs::write(&path, png(w, h, &rgba)).unwrap();
                        println!("wrote {path} ({w}x{h}, {} mips)", image.mip_levels);
                    }
                    None => println!("{:08x}: format {:?} not decoded", t.id, image.format),
                },
                Err(e) => println!("{:08x}: {e}", t.id),
            }
        }
    }
}

fn sets_listed<'p>(ptcl: &'p Ptcl, filter: &'p str) -> impl Iterator<Item = &'p botw_formats::ptcl::EmitterSet> {
    ptcl.emitter_sets.iter().filter(move |s| s.name.contains(filter))
}

/// Applies GX2 component selection (0–3 a channel, 4 zero, 5 one), so the
/// PNG shows what the shader samples.
fn apply_component_select(rgba: &[u8], select: [u8; 4]) -> Vec<u8> {
    rgba.chunks(4)
        .flat_map(|p| {
            select.map(|c| match c {
                0..=3 => p[c as usize],
                4 => 0,
                _ => 255,
            })
        })
        .collect()
}

/// Blends RGBA pixels over an opaque `rrggbb` colour by their alpha.
fn over_background(rgba: &[u8], bg: &str) -> Vec<u8> {
    let bg = u32::from_str_radix(bg, 16).expect("BG as rrggbb").to_be_bytes();
    rgba.chunks(4)
        .flat_map(|p| {
            let a = p[3] as u32;
            let mix = |i: usize| ((p[i] as u32 * a + bg[i + 1] as u32 * (255 - a)) / 255) as u8;
            [mix(0), mix(1), mix(2), 255]
        })
        .collect()
}

/// A minimal PNG encoder: RGBA8, stored (uncompressed) deflate blocks.
fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in bytes {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(&out[start..]);
        out.extend_from_slice(&crc.to_be_bytes());
    }
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in rgba.chunks(width as usize * 4) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (i, block) in blocks.iter().enumerate() {
        zlib.push((i + 1 == blocks.len()) as u8);
        zlib.extend_from_slice(&(block.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}

fn print_emitter(e: &Emitter, depth: usize) {
    let pad = "  ".repeat(depth);
    println!("{pad}emitter \"{}\" at {:#x} attributes {:?}", e.name, e.data_offset, e.attributes);
    let pad = "  ".repeat(depth + 1);
    let (i, em, s, r, p, v) = (&e.info, &e.emission, &e.shape, &e.render, &e.particle, &e.velocity);
    println!(
        "{pad}emitter: translate {:?} (± {:?}) rotate {:?} scale {:?} follow {} calc {} fade in/out {}/{} frames",
        i.translate, i.translate_random, i.rotate, i.scale, i.follow_type, i.calc_type, i.fade_in_time, i.alpha_fade_time
    );
    println!(
        "{pad}emission: {} × {} (+{}) every {} (+{}) frames from {}{}; shape {} radius {:?} sweep {} caliber {}",
        if em.one_time { "once" } else { "loop" },
        em.rate,
        em.rate_random,
        em.interval,
        em.interval_random,
        em.start,
        if em.one_time { format!(" for {}", em.duration) } else { String::new() },
        s.volume_type,
        s.radius,
        s.sweep_longitude,
        s.caliber_ratio
    );
    println!(
        "{pad}particle: life {} (-{}%) billboard {} rotation {} primitive {:?} size {:?} (± {:?}%) velocity {} + {} × {:?} (spread {}, random {})",
        p.life,
        p.life_random,
        p.billboard,
        p.rotation_type,
        p.primitive.map(|id| format!("{id:08x}")),
        e.scale,
        e.scale_random,
        v.all_direction,
        v.directional,
        v.direction,
        v.diffusion_angle,
        v.random
    );
    println!(
        "{pad}render: blend {} type {} depth test {} write {} alpha test {} ({}) side {}; combiner {:?}",
        r.blend,
        r.blend_type,
        r.depth_test,
        r.depth_write,
        r.alpha_test,
        r.alpha_threshold,
        r.display_side,
        e.combiner
    );
    println!("{pad}colour 0 {:?}", e.color0);
    println!("{pad}alpha 0 {:?}", e.alpha0);
    println!("{pad}colour 1 {:?}", e.color1);
    println!("{pad}alpha 1 {:?}", e.alpha1);
    println!("{pad}colour scale {} scale keys {:?} param keys {:?}", e.color_scale, e.scale_keys, e.param_keys);
    println!(
        "{pad}rotation {:?} (± {:?}) add {:?} (± {:?}); camera alpha near {:?} far {:?}; soft {:?}; gravity {:?} air {}",
        e.rotation.initial,
        e.rotation.initial_random,
        e.rotation.add,
        e.rotation.add_random,
        e.near_alpha,
        e.far_alpha,
        e.soft_particle,
        e.gravity,
        e.air_resistance
    );
    for (slot, (sampler, anim)) in e.samplers.iter().zip(&e.texture_anims).enumerate() {
        let Some(t) = sampler else { continue };
        println!(
            "{pad}texture {slot}: {:08x}{} wrap {:?} filter {}; grid {:?} scroll {:?}+{:?}/frame scale {:?}+{:?} rotate {}+{} pattern type {} ({} cells, {} frames each, {} random)",
            t.texture,
            if t.resident { " (resident)" } else { "" },
            t.wrap,
            t.filter,
            anim.uv_divisions,
            anim.scroll_initial,
            anim.scroll_add,
            anim.scale_initial,
            anim.scale_add,
            anim.rotate_initial,
            anim.rotate_add,
            anim.pattern_type,
            anim.pattern_count,
            anim.pattern_frequency,
            anim.pattern_random
        );
    }
    for child in &e.children {
        print_emitter(child, depth + 1);
    }
}

/// Reads a file, or an entry of a SARC pack given as `pack:entry`, and
/// undoes Yaz0 compression.
fn read(path: &str) -> Vec<u8> {
    let decompress = |b: &[u8]| botw_formats::yaz0::decompress_if(b).unwrap().into_owned();
    match path.split_once(".pack:") {
        Some((pack, entry)) => {
            let pack = decompress(&std::fs::read(format!("{pack}.pack")).unwrap());
            let sarc = roead::sarc::Sarc::new(&pack[..]).unwrap();
            decompress(sarc.get_data(entry).expect("entry in the pack"))
        }
        None => decompress(&std::fs::read(path).unwrap()),
    }
}
