//! Lists the files inside a SARC pack (e.g. `Pack/TitleBG.pack`), optionally
//! filtered by a substring, and dumps the top-level keys of a BYML entry.
//! Other entries are written decompressed to `$OUT` (a file path) if set.
//!
//! `cargo run -p botw-formats --example pack_ls -- <pack> [filter] [entry to dump]`

use roead::byml::Byml;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("pack path");
    let filter = args.next().unwrap_or_default();
    let dump = args.next();
    let bytes = std::fs::read(path).unwrap();
    let bytes = botw_formats::yaz0::decompress_if(&bytes).unwrap().into_owned();
    let sarc = roead::sarc::Sarc::new(&bytes[..]).unwrap();
    for file in sarc.files() {
        let Some(name) = file.name() else { continue };
        if name.contains(&filter) {
            println!("{name} ({} bytes)", file.data().len());
        }
        if dump.as_deref() == Some(name) {
            let data = botw_formats::yaz0::decompress_if(file.data()).unwrap();
            if !matches!(data.get(..2), Some(b"BY" | b"YB")) {
                match std::env::var("OUT") {
                    Ok(out) => std::fs::write(&out, &data[..]).map(|_| println!("  written to {out}")).unwrap(),
                    Err(_) => println!("  not BYML; set OUT to extract it"),
                }
                continue;
            }
            let doc = Byml::from_binary(&data[..]).unwrap();
            for (key, value) in doc.as_map().unwrap() {
                match value {
                    Byml::Array(list) => {
                        println!("  {key}: {} items", list.len());
                        for item in list.iter().take(3) {
                            println!("    {}", format!("{item:?}").chars().take(400).collect::<String>());
                        }
                    }
                    other => println!("  {key}: {}", format!("{other:?}").chars().take(300).collect::<String>()),
                }
            }
        }
    }
}
