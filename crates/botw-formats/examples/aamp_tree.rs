//! Prints the tree of an AAMP parameter file inside a SARC pack (e.g.
//! `Pack/TitleBG.pack` → `WorldMgr/normal.bwinfo`), naming what it can.
//! AAMP stores only CRC32 hashes of names; names come from a small built-in
//! guess list plus any given on the command line (`Name` or `Prefix_%d`).
//!
//! `cargo run -p botw-formats --example aamp_tree -- <pack> <entry> [max children per list] [extra names...]`

use std::collections::HashMap;

use roead::aamp::{Name, Parameter, ParameterIO, ParameterList, ParameterObject};

const GUESSES: &[&str] = &[
    "param_root", "EnvPaletteStatic", "IndoorPalette", "DofMgrParam", "SunParam", "BgDifColor", "BgDifIntencity",
    "FogColor", "FogStart", "FogEnd", "YFogColor", "YFogStart", "SkySunColor", "AmbientIntencity", "Exposure",
    "PaletteSetSelect", "DifUse", "DifXang", "DifYang", "SunSlope", "SunMoonDispDist", "SunScale", "MoonScale",
    "SunDirYStop", "CloudShadowOnOff", "AmplifierForEnvMap", "SkyIsotropicfade",
];

const PATTERNS: &[&str] = &[
    "EnvPalette_%d", "EnvAttribute_%d", "ClimateDefines_%d", "WeatherInfluence_%d", "Remains_%d", "PrCloud_%d",
    "PrCloudV0_%d", "PrCloudV1_%d", "SkyPalette0_%d", "SkyPalette2_%d", "CloudPat0_%d", "CloudPat2_%d",
    "CloudSpd_%d", "EnvPalette_CdanAddFog%d", "PaletteSel%02d",
];

fn main() {
    let mut args = std::env::args().skip(1);
    let pack = args.next().expect("pack path");
    let entry = args.next().expect("entry name");
    let limit: usize = args.next().map_or(3, |s| s.parse().unwrap());
    let extra: Vec<String> = args.collect();

    let mut names: HashMap<u32, String> = HashMap::new();
    let mut add = |name: String| {
        names.insert(Name::from(name.as_str()).hash(), name);
    };
    for name in GUESSES.iter().map(|s| s.to_string()).chain(extra.iter().filter(|s| !s.contains('%')).cloned()) {
        add(name);
    }
    for pattern in PATTERNS.iter().map(|s| s.to_string()).chain(extra.iter().filter(|s| s.contains('%')).cloned()) {
        for i in 0..300 {
            add(pattern.replace("%02d", &format!("{i:02}")).replace("%d", &i.to_string()));
        }
    }

    let bytes = std::fs::read(pack).unwrap();
    let bytes = botw_formats::yaz0::decompress_if(&bytes).unwrap().into_owned();
    let sarc = roead::sarc::Sarc::new(&bytes[..]).unwrap();
    let data = sarc.get_data(&entry).expect("entry not in pack");
    let data = botw_formats::yaz0::decompress_if(data).unwrap();
    let io = ParameterIO::from_binary(&data[..]).unwrap();
    println!("version {} type {:?}", io.version, io.data_type);
    print_list(&io.param_root, 0, limit, &names);
}

fn name(hash: u32, names: &HashMap<u32, String>) -> String {
    names.get(&hash).cloned().unwrap_or_else(|| format!("#{hash:08x}"))
}

fn print_list(list: &ParameterList, depth: usize, limit: usize, names: &HashMap<u32, String>) {
    let pad = "  ".repeat(depth);
    for (i, (key, object)) in list.objects.iter().enumerate() {
        if i >= limit && i + 1 < list.objects.len() {
            if i == limit {
                println!("{pad}... ({} objects)", list.objects.len());
            }
            continue;
        }
        println!("{pad}obj {}", name(key.hash(), names));
        print_object(object, depth + 1, names);
    }
    for (i, (key, child)) in list.lists.iter().enumerate() {
        if i >= limit && i + 1 < list.lists.len() {
            if i == limit {
                println!("{pad}... ({} lists)", list.lists.len());
            }
            continue;
        }
        println!("{pad}list {}", name(key.hash(), names));
        print_list(child, depth + 1, limit, names);
    }
}

fn print_object(object: &ParameterObject, depth: usize, names: &HashMap<u32, String>) {
    let pad = "  ".repeat(depth);
    for (key, value) in object.iter() {
        let text = match value {
            Parameter::StringRef(s) => format!("{s:?}"),
            Parameter::String32(s) => format!("{:?}", s.as_str()),
            Parameter::String64(s) => format!("{:?}", s.as_str()),
            other => format!("{other:?}").chars().take(160).collect(),
        };
        println!("{pad}{} = {text}", name(key.hash(), names));
    }
}
