//! Loads the terrain texture array as the viewer does and reports timing.
//!
//! `cargo run --release -p botw-formats --example terrain_textures -- <content dir>...`

use botw_formats::content::ContentRoots;
use botw_formats::terrain::textures::TerrainTextures;

fn main() {
    let (roots, _) = ContentRoots::resolve(std::env::args().skip(1));
    let start = std::time::Instant::now();
    let textures = TerrainTextures::load(&roots).unwrap();
    let a = &textures.albedo;
    println!(
        "{}x{} × {} layers, {} mips, {:.1} MB, {} materials, in {:.2} s",
        a.width,
        a.height,
        a.layers,
        a.mip_levels,
        a.data.len() as f64 / 1e6,
        textures.material_layers.len(),
        start.elapsed().as_secs_f64()
    );
}
