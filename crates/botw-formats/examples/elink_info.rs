//! Prints what ELink (or SLink) users play: every entry of their asset
//! call tables with its parameters. The database is a file or a pack entry,
//! e.g. `Pack/Bootup.pack:ELink2/ELink2DB.sbelnk`. Without users, lists
//! the asset parameter definitions.
//!
//! `cargo run -p botw-formats --example elink_info -- <database> [user...]`

use botw_formats::xlink::XLink;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("database");
    let bytes = read(&path);
    let db = XLink::parse(&bytes).unwrap();
    let users: Vec<String> = args.collect();
    if users.is_empty() {
        for (i, p) in db.asset_params().iter().enumerate() {
            println!("asset param {i:2}: {} (type {}, default {:#x})", p.name, p.kind, p.default);
        }
    }
    for user in users {
        match db.user(&user).unwrap() {
            None => println!("{user}: not found"),
            Some(calls) => {
                println!("{user}: {} entries", calls.len());
                for (i, call) in calls.iter().enumerate() {
                    let kind = if call.is_container { "container" } else { "asset" };
                    println!(
                        "  [{i}] {kind} \"{}\" id {} parent {} duration {}",
                        call.key, call.asset_id, call.parent_index, call.duration
                    );
                    for (name, value) in &call.params {
                        println!("      {name} = {value:?}");
                    }
                }
            }
        }
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
