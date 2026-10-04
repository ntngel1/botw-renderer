//! Decodes `.water.extm` entries both ways and compares their heights with
//! the water range `MainField.tscb` gives for the same tile.
//!
//! `cargo run -p botw-formats --example probe_water -- <MainField.tscb> <archive.water.extm.sstera>...`

use botw_formats::stera;
use botw_formats::terrain::{TileId, Tscb};

fn main() {
    let mut args = std::env::args().skip(1);
    let tscb = Tscb::parse(&std::fs::read(args.next().expect("tscb")).unwrap()).unwrap();
    for path in args {
        for entry in stera::read(&std::fs::read(&path).unwrap()).unwrap() {
            let (tile, _) = TileId::parse_name(&entry.name).unwrap();
            let area = tscb.areas.iter().find(|a| a.tile == tile);
            println!("{} ({} bytes) tscb water {:?}", entry.name, entry.data.len(), area.and_then(|a| a.water));
            if entry.data.len() % 8 != 0 {
                println!("  not a multiple of 8");
                continue;
            }
            let records = entry.data.as_chunks::<8>().0;
            println!("  {} records ({}²)", records.len(), (records.len() as f32).sqrt());
            for (label, big) in [("LE", false), ("BE", true)] {
                let u16_at = |r: &[u8; 8], i: usize| {
                    let b = [r[i], r[i + 1]];
                    if big { u16::from_be_bytes(b) } else { u16::from_le_bytes(b) }
                };
                let heights: Vec<f32> = records.iter().map(|r| f32::from(u16_at(r, 0)) / 65535.0 * 800.0).collect();
                let (lo, hi) = heights.iter().fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
                let flows: Vec<u16> = records.iter().flat_map(|r| [u16_at(r, 2), u16_at(r, 4)]).collect();
                let (flo, fhi) = flows.iter().fold((u16::MAX, 0), |(l, h), &v| (l.min(v), h.max(v)));
                println!("  {label}: height {lo:.2}..{hi:.2} m, flow raw {flo}..{fhi}");
            }
            let mut bytes6: Vec<(u8, u8)> = records.iter().map(|r| (r[6], r[7])).collect();
            bytes6.sort();
            bytes6.dedup();
            println!("  bytes 6,7 distinct: {:?}", &bytes6[..bytes6.len().min(12)]);
        }
    }
}
