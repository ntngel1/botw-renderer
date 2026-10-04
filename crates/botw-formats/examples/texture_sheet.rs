//! Writes every layer of an array texture side by side into one PPM, alpha
//! composited over a grey checkerboard (or, with `ALPHA=1`, the alpha
//! channel as grey), to look at billboard atlases and the like by eye.
//!
//! `cargo run -p botw-formats --example texture_sheet -- <out.ppm> <texture> <columns> <X.Tex1.sbfres> [X.Tex2.sbfres]`

use botw_formats::bfres::{Bfres, assemble_texture};

fn main() {
    let mut args = std::env::args().skip(1);
    let out = args.next().expect("output");
    let name = args.next().expect("texture");
    let columns: u32 = args.next().expect("columns").parse().unwrap();
    let files: Vec<Vec<u8>> =
        args.map(|p| botw_formats::yaz0::decompress_if(&std::fs::read(p).unwrap()).unwrap().into_owned()).collect();
    let bfres: Vec<Bfres> = files.iter().map(|b| Bfres::parse(b).unwrap()).collect();
    let level0 = bfres[0].texture(&name).unwrap().expect("texture in Tex1");
    let mips = bfres.get(1).and_then(|b| b.texture(&name).unwrap());
    let image = assemble_texture(&level0, mips.as_ref()).unwrap();
    let alpha_only = std::env::var("ALPHA").is_ok();
    let (w, h) = (image.width, image.height);
    let rows = image.layers.div_ceil(columns);
    let (sheet_w, sheet_h) = (w * columns, h * rows);
    let mut sheet = vec![0u8; (sheet_w * sheet_h * 3) as usize];
    for layer in 0..image.layers {
        let rgba = image.decode_layer_rgba8(0, layer).expect("decodable format");
        let (ox, oy) = ((layer % columns) * w, (layer / columns) * h);
        for y in 0..h {
            for x in 0..w {
                let p = &rgba[((y * w + x) * 4) as usize..][..4];
                let checker = if ((x / 8) + (y / 8)) % 2 == 0 { 90.0 } else { 140.0 };
                let a = p[3] as f32 / 255.0;
                let rgb: [u8; 3] = if alpha_only {
                    [p[3]; 3]
                } else {
                    std::array::from_fn(|c| (p[c] as f32 * a + checker * (1.0 - a)) as u8)
                };
                let i = (((oy + y) * sheet_w + ox + x) * 3) as usize;
                sheet[i..i + 3].copy_from_slice(&rgb);
            }
        }
    }
    let mut ppm = format!("P6\n{sheet_w} {sheet_h}\n255\n").into_bytes();
    ppm.extend(sheet);
    std::fs::write(out, ppm).unwrap();
}
