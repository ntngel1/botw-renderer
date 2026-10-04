//! Prints the `Actor/ActorInfo.product.sbyml` entries of actors whose name
//! contains a pattern. With `TAG=<name>` only actors with that tag are
//! listed, by name (tags are stored as CRC32 hashes of their names).
//!
//! `cargo run -p botw-formats --example actor_info -- pattern <content dir>...`

use botw_formats::content::ContentRoots;
use roead::byml::Byml;

fn main() {
    let mut args = std::env::args().skip(1);
    let pattern = args.next().expect("pattern");
    let (roots, _) = ContentRoots::resolve(args);
    let path = roots.find("Actor/ActorInfo.product.sbyml").expect("ActorInfo");
    let bytes = std::fs::read(path).unwrap();
    let data = botw_formats::yaz0::decompress_if(&bytes).unwrap();
    let doc = Byml::from_binary(&data[..]).unwrap();
    let root = doc.as_map().unwrap();
    let Some(Byml::Array(actors)) = root.get("Actors") else { panic!("no Actors") };
    let tag = std::env::var("TAG").ok().map(|t| crc32(t.as_bytes()));
    let mut shown = 0;
    for actor in actors {
        let Ok(map) = actor.as_map() else { continue };
        let name = map.get("name").and_then(|n| n.as_string().ok()).map(|s| s.to_string()).unwrap_or_default();
        if !name.contains(&pattern) {
            continue;
        }
        if let Some(tag) = tag {
            let tagged = map.get("tags").and_then(|t| t.as_map().ok()).is_some_and(|tags| {
                tags.values().any(|v| matches!(v, Byml::I32(h) if *h as u32 == tag) || matches!(v, Byml::U32(h) if *h == tag))
            });
            if tagged {
                shown += 1;
                println!("{name}");
            }
            continue;
        }
        shown += 1;
        println!("{name}");
        for (key, value) in map.iter() {
            println!("  {key}: {}", format!("{value:?}").chars().take(300).collect::<String>());
        }
    }
    println!("{shown} actors");
}

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
