//! Lists what a BFRES holds: textures (size, format, tiling, mips, user
//! data) and models.
//!
//! `cargo run -p botw-formats --example bfres_info -- <file.bfres or .sbfres>`

use botw_formats::bfres::Bfres;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("path");
    let mut bytes = botw_formats::yaz0::decompress_if(&std::fs::read(&path).unwrap()).unwrap().into_owned();
    // `<pack> <entry>`: read the BFRES from inside a SARC pack.
    if let Some(entry) = args.next() {
        let sarc = roead::sarc::Sarc::new(&bytes[..]).unwrap();
        let data = sarc.get_data(&entry).expect("entry in pack").to_vec();
        bytes = botw_formats::yaz0::decompress_if(&data).unwrap().into_owned();
    }
    let bfres = Bfres::parse(&bytes).unwrap();
    println!("{} (version {:#010x})", bfres.name, bfres.version);
    for name in bfres.texture_names().map(str::to_owned).collect::<Vec<_>>() {
        let t = match bfres.texture(&name) {
            Ok(Some(t)) => t,
            other => {
                println!("  tex {name}: {:?}", other.err());
                continue;
            }
        };
        let s = &t.surface;
        println!(
            "  tex {:32} {:4}x{:<4} depth {:3} dim {} mips {:2} fmt {:#06x} {:?} tile {} swz {:#x} pitch {} img {}{} mip {}{} align {:#x} user {:?}",
            t.name, s.width, s.height, s.depth, s.dim, s.mip_count, s.format, s.format(), s.tile_mode, s.swizzle, s.pitch, s.image_size,
            if t.image.is_empty() { " (absent)" } else { "" }, s.mip_size, if t.mips.is_empty() { " (absent)" } else { "" }, s.alignment, t.user_data
        );
        if std::env::var_os("MIPS").is_some() {
            println!("    mip offsets {:?}", &s.mip_offsets[..s.mip_count.saturating_sub(1).min(13) as usize]);
        }
    }
    for m in bfres.models().unwrap() {
        println!("  model {}", m.name);
    }
}
