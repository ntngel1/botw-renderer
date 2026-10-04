//! Compares a texture's image and mip data between two BFRES files (e.g.
//! Tex1 and Tex2), to see which file holds what.
//!
//! `cargo run -p botw-formats --example tex_compare -- <texture> <a.sbfres> <b.sbfres>`

use botw_formats::bfres::Bfres;

fn checksum(data: &[u8]) -> u64 {
    data.iter().fold(0xcbf29ce484222325u64, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
}

fn main() {
    let mut args = std::env::args().skip(1);
    let name = args.next().expect("texture");
    for path in args {
        let bytes = botw_formats::yaz0::decompress_if(&std::fs::read(&path).unwrap()).unwrap().into_owned();
        let bfres = Bfres::parse(&bytes).unwrap();
        let t = bfres.texture(&name).unwrap().expect("texture");
        let zero = |d: &[u8]| d.iter().filter(|&&b| b == 0).count() as f32 / d.len().max(1) as f32;
        println!(
            "{path}\n  image {} bytes sum {:016x} zeros {:.2}\n  mips {} bytes sum {:016x} zeros {:.2}\n  first mip bytes {:02x?}",
            t.image.len(),
            checksum(t.image),
            zero(t.image),
            t.mips.len(),
            checksum(t.mips),
            zero(t.mips),
            &t.mips[..t.mips.len().min(24)]
        );
    }
}
