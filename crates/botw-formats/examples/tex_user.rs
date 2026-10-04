//! Prints one texture's user data in full, one value per line.
//!
//! `cargo run -p botw-formats --example tex_user -- <file.sbfres> <texture>`

use botw_formats::bfres::Bfres;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("path");
    let name = args.next().expect("texture name");
    let bytes = botw_formats::yaz0::decompress_if(&std::fs::read(&path).unwrap()).unwrap().into_owned();
    let bfres = Bfres::parse(&bytes).unwrap();
    let texture = bfres.texture(&name).unwrap().expect("texture");
    let columns: Vec<(String, Vec<String>)> =
        texture.user_data.iter().map(|(k, v)| (k.clone(), v.split(',').map(str::to_owned).collect())).collect();
    let rows = columns.iter().map(|(_, v)| v.len()).max().unwrap_or(0);
    println!("{}", columns.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join("\t"));
    for i in 0..rows {
        println!("{}", columns.iter().map(|(_, v)| v.get(i).map_or("", |s| s.as_str())).collect::<Vec<_>>().join("\t"));
    }
}
