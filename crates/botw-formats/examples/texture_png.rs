//! Assembles a model texture from its Tex1/Tex2 files and writes one mip
//! level as PPM (and its alpha as `<out>.alpha.pgm`), to check untiling and
//! decoding by eye. `LAYER=n` picks an array layer (default 0).
//!
//! `cargo run -p botw-formats --example texture_png -- <out.ppm> <texture> <level> <X.Tex1.sbfres> [X.Tex2.sbfres]`

use botw_formats::bfres::{Bfres, assemble_texture};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("output");
    let name = args.next().expect("texture");
    let level: u32 = args.next().expect("level").parse().unwrap();
    let files: Vec<Vec<u8>> =
        args.map(|p| botw_formats::yaz0::decompress_if(&std::fs::read(p).unwrap()).unwrap().into_owned()).collect();
    let bfres: Vec<Bfres> = files.iter().map(|b| Bfres::parse(b).unwrap()).collect();
    let level0 = bfres[0].texture(&name).unwrap().expect("texture in Tex1");
    let mips = bfres.get(1).and_then(|b| b.texture(&name).unwrap());
    let image = assemble_texture(&level0, mips.as_ref()).unwrap();
    println!("{:?} {}x{} layers {} mips {} select {:?}", image.format, image.width, image.height, image.layers, image.mip_levels, image.component_select);
    let layer = std::env::var("LAYER").ok().and_then(|l| l.parse().ok()).unwrap_or(0);
    let rgba = image.decode_layer_rgba8(level, layer).expect("decodable format");
    let (w, h) = image.level_size(level);
    let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
    for p in rgba.chunks(4) {
        ppm.extend_from_slice(&p[..3]);
    }
    let mut pgm = format!("P5\n{w} {h}\n255\n").into_bytes();
    pgm.extend(rgba.chunks(4).map(|p| p[3]));
    std::fs::write(format!("{out}.alpha.pgm"), pgm).unwrap();
    std::fs::write(out, ppm).unwrap();
}
