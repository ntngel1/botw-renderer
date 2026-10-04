//! Prints an actor's animation sequences (AS) from its pack: the AS list
//! with cross-fade rules, or the element tree of the named AS (selectors
//! with their ranges or keys, clips with morph frames and rates).
//!
//! `cargo run -p botw-formats --example as_tree -- <actor pack> [AS name ...]`

use botw_formats::anim_seq::{AnimSeq, AsList};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("actor pack");
    let bytes = botw_formats::yaz0::decompress_if(&std::fs::read(path).unwrap())
        .unwrap()
        .into_owned();
    let sarc = roead::sarc::Sarc::new(&bytes[..]).unwrap();
    let list_file = sarc
        .files()
        .find(|f| f.name().is_some_and(|n| n.starts_with("Actor/ASList/")))
        .expect("no AS list");
    let list = AsList::parse(list_file.data()).unwrap();
    let names: Vec<String> = args.collect();
    if names.is_empty() {
        println!("animations: {}", list.anim_files.join(", "));
        for (name, file) in &list.defines {
            println!("{name} -> {file}");
        }
        for fade in &list.cross_fades {
            let posts: Vec<String> = fade
                .posts
                .iter()
                .map(|(n, f)| format!("{}:{f}", if n.is_empty() { "*" } else { n }))
                .collect();
            let excepts = if fade.excepts.is_empty() {
                String::new()
            } else {
                format!(" except {}", fade.excepts.join(" "))
            };
            println!("fade {} -> {}{excepts}", fade.pre, posts.join(" "));
        }
        return;
    }
    for name in names {
        let file = list.file_of(&name).unwrap_or(&name);
        let data = sarc
            .get_data(&format!("Actor/AS/{file}.bas"))
            .unwrap_or_else(|| panic!("no {file}.bas"));
        let seq = AnimSeq::parse(data).unwrap();
        println!("{name} ({file}.bas, {} elements)", seq.elements.len());
        print(&seq, 0, 1, &mut vec![false; seq.elements.len()]);
    }
}

fn print(seq: &AnimSeq, index: usize, depth: usize, seen: &mut [bool]) {
    let element = &seq.elements[index];
    let pad = "  ".repeat(depth);
    if std::mem::replace(&mut seen[index], true) && !element.children.is_empty() {
        println!("{pad}#{index} (see above)");
        return;
    }
    let mut line = format!("{pad}#{index} {}", element.type_name());
    if let Some(file) = &element.file_name {
        line += &format!(" {file} morph {} rate {}", element.morph, element.rate);
    }
    if let Some(limit) = element.input_limit {
        line += &format!(" input limit {limit}/frame");
    }
    if let Some(bit) = element.bit_index {
        line += &format!(" bit {bit}");
    }
    if element.sequence_loop {
        line += " loop";
    }
    if !element.floats.is_empty() {
        line += &format!(" floats {:?}", element.floats);
    }
    if !element.ints.is_empty() {
        line += &format!(" ints {:?}", element.ints);
    }
    println!("{line}");
    for (i, &child) in element.children.iter().enumerate() {
        let key = match (element.ranges.get(i), element.strings.get(i)) {
            _ if element.type_name() == "BoolSelector" => format!("{}", i == 1),
            (Some((start, end)), _) => format!("[{start}, {end}]"),
            (_, Some(key)) => format!("{key:?}"),
            _ => String::new(),
        };
        if !key.is_empty() {
            println!("{pad}  when {key}:");
        }
        print(seq, child, depth + 2, seen);
    }
}
