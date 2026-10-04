//! Prints statistics of the `.grass.extm` entries covering world positions,
//! next to the terrain heights of the same tile, to pin down the format.
//!
//! `cargo run -p botw-formats --example probe_grass -- <content root>... -- x,z [x,z...]`

use botw_formats::content::ContentRoots;
use botw_formats::terrain::{MAX_LOD, TerrainIndex, TerrainKind, TileId};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|a| a == "--").expect("-- before positions");
    let (roots, _) = ContentRoots::resolve(&args[..split]);
    let index = TerrainIndex::scan(&roots);
    for pos in &args[split + 1..] {
        let (x, z) = pos.split_once(',').map(|(x, z)| (x.parse::<f32>().unwrap(), z.parse::<f32>().unwrap())).unwrap();
        for lod in (5..=MAX_LOD).rev() {
            let Some(tile) = TileId::containing(lod, x, z) else { continue };
            let Some(data) = index.load_raw(tile, TerrainKind::Grass).unwrap() else { continue };
            println!("{x},{z}: lod {lod} tile {} {} bytes", tile.file_stem(), data.len());
            for stride in [4usize, 8] {
                if data.len() % stride != 0 {
                    continue;
                }
                let n = data.len() / stride;
                println!("  stride {stride}: {n} records ({:.1}²)", (n as f32).sqrt());
                for byte in 0..stride {
                    let values: Vec<u8> = data.iter().skip(byte).step_by(stride).copied().collect();
                    let (lo, hi) = values.iter().fold((255u8, 0u8), |(l, h), &v| (l.min(v), h.max(v)));
                    let mean = values.iter().map(|&v| v as f32).sum::<f32>() / values.len() as f32;
                    let zeros = values.iter().filter(|&&v| v == 0).count();
                    println!("    byte {byte}: {lo}..{hi} mean {mean:.1} zeros {zeros}");
                }
            }
            // First rows, as hex, to eyeball the layout.
            for row in data.chunks(32).take(4) {
                println!("  {}", row.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
            }
            if let Some(height) = index.load_height(tile).unwrap() {
                let h = |i: usize, j: usize| height.height(i, j);
                println!("  hght corners {:.1} {:.1} {:.1} {:.1}", h(0, 0), h(255, 0), h(0, 255), h(255, 255));
            }
            break;
        }
    }
}
