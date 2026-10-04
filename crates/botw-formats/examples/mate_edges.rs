//! How `.mate` and `.hght` samples line up between neighbouring tiles and
//! between a tile and its parent: for tiles around world positions, prints
//! how often the east neighbour's first column has the same dominant
//! material as this tile's columns near the edge, the samples on both sides
//! of the south edge, the mean height step across that edge against the
//! step inside the tile (about equal: the edge samples are not shared), and
//! how often a child's samples match its parent's under two layouts:
//! samples on the corners of 255 intervals ("shared edges") or in 256 cells.
//!
//! `cargo run -p botw-formats --example mate_edges -- <content dir>... -- x,z[,lod] [x,z[,lod]...]`

use botw_formats::content::ContentRoots;
use botw_formats::terrain::{MAX_LOD, MaterialSample, MaterialTile, TerrainIndex, TileId};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let split = args.iter().position(|a| a == "--").expect("-- before positions");
    let (roots, _) = ContentRoots::resolve(&args[..split]);
    let index = TerrainIndex::scan(&roots);
    for pos in &args[split + 1..] {
        let parts: Vec<f32> = pos.split(',').map(|v| v.parse().unwrap()).collect();
        let (x, z) = (parts[0], parts[1]);
        let lods: Vec<u8> = match parts.get(2) {
            Some(lod) => vec![*lod as u8],
            None => (4..=MAX_LOD).collect(),
        };
        for lod in lods {
            let Some(tile) = TileId::containing(lod, x, z) else { continue };
            let Ok(Some(here)) = index.load_material(tile) else { continue };
            let (gx, gz) = tile.grid();
            print!("{x},{z} lod {lod}:");
            if let Some(east) = TileId::from_grid(lod, gx.wrapping_add(1), gz)
                && let Ok(Some(east)) = index.load_material(east)
            {
                // Share of rows where the east tile's column 0 equals our column c.
                let same = |c: usize, e: usize| (0..256).filter(|&row| dominant(here.get(c, row)) == dominant(east.get(e, row))).count() as f32 / 256.0;
                print!("  east col0 = ours col 253/254/255: {:.2}/{:.2}/{:.2}", same(253, 0), same(254, 0), same(255, 0));
                print!("  east col1 = ours col 255: {:.2}", same(255, 1));
            }
            if let Some(south) = TileId::from_grid(lod, gx, gz.wrapping_add(1))
                && let Ok(Some(south)) = index.load_material(south)
            {
                // The samples on both sides of the south edge, at a few columns.
                let show = |s: MaterialSample| format!("{}/{}@{}", s.material0, s.material1, s.blend);
                let columns = [0usize, 64, 128, 192, 255];
                print!("\n  south edge rows 254|255 || 0|1:");
                for c in columns {
                    print!(
                        "  [{} {} || {} {}]",
                        show(here.get(c, 254)),
                        show(here.get(c, 255)),
                        show(south.get(c, 0)),
                        show(south.get(c, 1))
                    );
                }
                print!("\n ");
            }
            if let Some(south) = TileId::from_grid(lod, gx, gz.wrapping_add(1))
                && let (Ok(Some(here)), Ok(Some(south))) = (index.load_height(tile), index.load_height(south))
            {
                // Mean height step across the edge against the step inside.
                let step = |a: &dyn Fn(usize) -> f32, b: &dyn Fn(usize) -> f32| (0..256).map(|c| (a(c) - b(c)).abs()).sum::<f32>() / 256.0;
                let across = step(&|c| here.height(c, 255), &|c| south.height(c, 0));
                let inside = step(&|c| here.height(c, 254), &|c| here.height(c, 255));
                print!("  heights: |row255 - south row0| {across:.3} m, |row254 - row255| {inside:.3} m");
            }
            if let Some(parent) = tile.parent()
                && let Ok(Some(parent_mate)) = index.load_material(parent)
            {
                let (px, pz) = parent.grid();
                let (qx, qz) = ((gx - px * 2) as f32, (gz - pz * 2) as f32);
                print!("  parent: shared edges {:.2}, cells {:.2}", nesting(&here, &parent_mate, qx, qz, true), nesting(&here, &parent_mate, qx, qz, false));
            }
            println!();
        }
    }
}

/// Share of the child's samples that match the parent sample at the same
/// place, for child quadrant (`qx`, `qz`): with shared edges only the child
/// samples that fall exactly on a parent sample, with cells the parent cell
/// holding the child cell.
fn nesting(child: &MaterialTile, parent: &MaterialTile, qx: f32, qz: f32, shared_edges: bool) -> f32 {
    let (mut hits, mut total) = (0, 0);
    for cz in 0..256usize {
        for cx in 0..256usize {
            // Parent sample coordinate of this child sample.
            let to_parent = |c: usize, q: f32| {
                if shared_edges { (q * 255.0 + c as f32) / 2.0 } else { ((q * 256.0 + c as f32) / 2.0).floor() }
            };
            let (px, pz) = (to_parent(cx, qx), to_parent(cz, qz));
            if px.fract() != 0.0 || pz.fract() != 0.0 {
                continue;
            }
            total += 1;
            if dominant(child.get(cx, cz)) == dominant(parent.get(px as usize, pz as usize)) {
                hits += 1;
            }
        }
    }
    if total == 0 { f32::NAN } else { hits as f32 / total as f32 }
}

/// The material that dominates a sample.
fn dominant(sample: MaterialSample) -> u8 {
    if sample.blend < 128 { sample.material0 } else { sample.material1 }
}
