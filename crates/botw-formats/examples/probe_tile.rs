//! Prints what a terrain archive holds: entry names, sizes and height
//! statistics decoded both ways, to check byte order against a real dump.
//!
//! `cargo run -p botw-formats --example probe_tile -- <archive.sstera>...`

use botw_formats::stera;

fn main() {
    for path in std::env::args().skip(1) {
        let bytes = std::fs::read(&path).expect("read archive");
        let sarc = botw_formats::yaz0::decompress_if(&bytes).expect("yaz0");
        println!("{path}: {} bytes, SARC BOM {:02X?}", sarc.len(), &sarc[6..8]);
        for entry in stera::read(&bytes).expect("sarc") {
            println!("  {} ({} bytes)", entry.name, entry.data.len());
            if entry.name.ends_with(".hght") {
                for (label, decode) in [("LE", u16::from_le_bytes as fn([u8; 2]) -> u16), ("BE", u16::from_be_bytes)] {
                    let values: Vec<u16> = entry.data.as_chunks::<2>().0.iter().map(|&p| decode(p)).collect();
                    let (min, max) = values.iter().fold((u16::MAX, 0), |(lo, hi), &v| (lo.min(v), hi.max(v)));
                    // Mean absolute step between horizontal neighbours: smooth data has small steps.
                    let roughness: f64 = values
                        .chunks(256)
                        .flat_map(|row| row.windows(2).map(|w| (f64::from(w[0]) - f64::from(w[1])).abs()))
                        .sum::<f64>()
                        / (256.0 * 255.0);
                    println!("    {label}: min {min} max {max} mean step {roughness:.1}");
                }
            }
        }
    }
}
