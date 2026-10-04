//! Writes slices of a BC1 texture (level 0) side by side as a PPM image, to
//! check the GX2 untiling by eye.
//!
//! `cargo run -p botw-formats --example dump_texture -- <file.sbfres> <texture> <out.ppm> [first] [count]`

use botw_formats::bfres::{Bfres, bc};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("path");
    let name = args.next().expect("texture");
    let out = args.next().expect("output");
    let first: u32 = args.next().map_or(0, |s| s.parse().unwrap());
    let count: u32 = args.next().map_or(1, |s| s.parse().unwrap());
    let bytes = botw_formats::yaz0::decompress_if(&std::fs::read(&path).unwrap()).unwrap().into_owned();
    let bfres = Bfres::parse(&bytes).unwrap();
    let texture = bfres.texture(&name).unwrap().expect("texture");
    let (w, h) = (texture.surface.width, texture.surface.height);
    let columns = count.min(4);
    let rows = count.div_ceil(columns);
    let (sheet_w, sheet_h) = ((w * columns) as usize, (h * rows) as usize);
    let mut sheet = vec![0u8; sheet_w * sheet_h * 3];
    for i in 0..count {
        let blocks = texture.level0_slice(first + i).unwrap();
        let rgba = bc::decode_bc1(&blocks, w, h);
        let (ox, oy) = (((i % columns) * w) as usize, ((i / columns) * h) as usize);
        for y in 0..h as usize {
            for x in 0..w as usize {
                let p = &rgba[(y * w as usize + x) * 4..][..3];
                sheet[((oy + y) * sheet_w + ox + x) * 3..][..3].copy_from_slice(p);
            }
        }
    }
    let mut file = format!("P6\n{sheet_w} {sheet_h}\n255\n").into_bytes();
    file.extend_from_slice(&sheet);
    std::fs::write(out, file).unwrap();
}
