//! Prints an AAMP parameter document (`.bgparamlist`, `.bas`, `.baslist`,
//! `.bphysics`, ...) with names resolved from the game's string table. The
//! document is a file, or an entry of a SARC pack (Yaz0 or not).
//!
//! `cargo run -p botw-formats --example aamp_dump -- <file> [entry in pack]`

use roead::aamp::{Name, Parameter, ParameterIO, ParameterList, ParameterObject, get_default_name_table};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("file");
    let bytes = std::fs::read(path).unwrap();
    let mut bytes = botw_formats::yaz0::decompress_if(&bytes).unwrap().into_owned();
    if let Some(entry) = args.next() {
        let sarc = roead::sarc::Sarc::new(&bytes[..]).unwrap();
        let data = sarc.get_data(&entry).unwrap_or_else(|| panic!("no {entry} in the pack"));
        bytes = botw_formats::yaz0::decompress_if(data).unwrap().into_owned();
    }
    let io = ParameterIO::from_binary(&bytes).unwrap();
    println!("type {} version {}", io.data_type, io.version);
    print_list(&io.param_root, Name::from_str("param_root").hash(), 0);
}

fn name(hash: &Name, index: usize, parent: u32) -> String {
    match get_default_name_table().get_name(hash.hash(), index, parent) {
        Some(name) => name.to_string(),
        None => format!("0x{:08x}", hash.hash()),
    }
}

fn print_list(list: &ParameterList, hash: u32, depth: usize) {
    let pad = "  ".repeat(depth);
    for (i, (key, object)) in list.objects.iter().enumerate() {
        println!("{pad}{}:", name(key, i, hash));
        print_object(object, key.hash(), depth + 1);
    }
    for (i, (key, child)) in list.lists.iter().enumerate() {
        println!("{pad}{}/", name(key, i, hash));
        print_list(child, key.hash(), depth + 1);
    }
}

fn print_object(object: &ParameterObject, hash: u32, depth: usize) {
    let pad = "  ".repeat(depth);
    for (i, (key, value)) in object.iter().enumerate() {
        let value = match value {
            Parameter::F32(v) => format!("{v}"),
            Parameter::I32(v) => format!("{v}"),
            Parameter::U32(v) => format!("{v}"),
            Parameter::Bool(v) => format!("{v}"),
            other => match other.as_str() {
                Ok(text) => format!("{text:?}"),
                Err(_) => format!("{other:?}").chars().take(200).collect(),
            },
        };
        println!("{pad}{} = {value}", name(key, i, hash));
    }
}
