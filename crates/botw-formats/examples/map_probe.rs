//! Cross-checks terrain against map placement data: prints the named
//! location markers and, for every placed object, how far above the terrain
//! it sits. If axes, world size and height scale are right, most objects rest
//! within a metre of the ground.
//!
//! `cargo run -p botw-formats --example map_probe -- <content dir>...`

use std::collections::HashMap;

use botw_formats::content::ContentRoots;
use botw_formats::terrain::{HeightTile, MAX_LOD, TILE_SAMPLES, TerrainIndex, TileId};
use roead::byml::Byml;

fn main() {
    let (roots, _) = ContentRoots::resolve(std::env::args().skip(1));
    let index = TerrainIndex::scan(&roots);
    let mut cache: HashMap<TileId, Option<HeightTile>> = HashMap::new();
    // Terrain height at a world position from the finest tile that exists.
    let mut ground = |x: f32, z: f32| -> Option<(f32, u8)> {
        for lod in (0..=MAX_LOD).rev() {
            let tile = TileId::containing(lod, x, z)?;
            let height = cache.entry(tile).or_insert_with(|| index.load_height(tile).ok().flatten());
            if let Some(height) = height {
                let (min_x, min_z) = tile.world_min();
                let scale = (TILE_SAMPLES - 1) as f32 / tile.world_size();
                return Some((height.sample((x - min_x) * scale, (z - min_z) * scale), lod));
            }
        }
        None
    };

    let pack_path = roots.find("Pack/TitleBG.pack").expect("TitleBG.pack");
    let pack_bytes = std::fs::read(pack_path).unwrap();
    let pack = roead::sarc::Sarc::new(&pack_bytes[..]).unwrap();
    let mut units: Vec<(String, Vec<u8>)> = pack
        .files()
        .filter_map(|f| Some((f.name()?.to_owned(), f.data().to_vec())))
        .filter(|(name, _)| name.starts_with("Map/MainField/") && name.ends_with("_Static.smubin"))
        .collect();
    // Dynamic units live loose in Map/MainField/<cell>/.
    for root in roots.roots() {
        let Ok(cells) = std::fs::read_dir(root.join("Map/MainField")) else { continue };
        for cell in cells.flatten() {
            let Ok(files) = std::fs::read_dir(cell.path()) else { continue };
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                if name.ends_with("_Dynamic.smubin") {
                    units.retain(|(n, _)| !n.ends_with(&name));
                    units.push((format!("Map/MainField/{name}"), std::fs::read(file.path()).unwrap()));
                }
            }
        }
    }
    // Named places live in the field-wide static unit in Bootup.pack.
    let bootup = std::fs::read(roots.find("Pack/Bootup.pack").expect("Bootup.pack")).unwrap();
    let bootup = roead::sarc::Sarc::new(&bootup[..]).unwrap();
    let field_static = bootup.get_data("Map/MainField/Static.smubin").expect("MainField/Static.smubin");
    units.push(("Map/MainField/Static.smubin".into(), field_static.to_vec()));
    println!("{} map units", units.len());

    let mut offsets = Vec::new();
    let mut markers = Vec::new();
    for (name, bytes) in &units {
        let data = botw_formats::yaz0::decompress_if(bytes).unwrap();
        let doc = Byml::from_binary(&data[..]).unwrap_or_else(|e| panic!("{name}: {e}"));
        let Ok(map) = doc.as_map() else { continue };
        for key in ["LocationMarker", "LocationPointer"] {
            let Some(Byml::Array(list)) = map.get(key) else { continue };
            for marker in list {
                let m = marker.as_map().unwrap();
                let id = m.get("MessageID").and_then(|v| v.as_string().ok()).cloned().unwrap_or_default();
                if !id.is_empty() {
                    markers.push((id, translate(m.get("Translate"))));
                }
            }
        }
        if let Some(Byml::Array(objs)) = map.get("Objs") {
            for obj in objs {
                let m = obj.as_map().unwrap();
                let [x, y, z] = translate(m.get("Translate"));
                let actor = m.get("UnitConfigName").and_then(|v| v.as_string().ok()).cloned().unwrap_or_default();
                if let Some((h, _)) = ground(x, z) {
                    offsets.push((y - h, actor));
                }
            }
        }
    }

    markers.sort_by(|a, b| a.0.cmp(&b.0));
    println!("{} location markers:", markers.len());
    for (id, [x, y, z]) in &markers {
        let g = ground(*x, *z).map(|(h, _)| h).unwrap_or(f32::NAN);
        println!("  {id:40} {x:8.1} {y:7.1} {z:8.1}  ground {g:7.1}");
    }

    println!("{} objects compared with terrain", offsets.len());
    let mut histogram = [0usize; 12];
    let edges = [-50.0, -10.0, -3.0, -1.0, -0.3, 0.3, 1.0, 3.0, 10.0, 50.0, 200.0];
    for (d, ..) in &offsets {
        let bucket = edges.iter().position(|&e| *d < e).unwrap_or(edges.len());
        histogram[bucket] += 1;
    }
    let mut label = String::from("<-50");
    for (i, count) in histogram.iter().enumerate() {
        if i > 0 {
            label = if i < edges.len() { format!("{}..{}", edges[i - 1], edges[i]) } else { format!(">{}", edges[i - 1]) };
        }
        println!("  {label:>12}: {count}");
    }
    // Ground-hugging actors (trees, grass) should sit right on the terrain.
    for prefix in ["Obj_Tree", "Weapon_", "Enemy_Bokoblin"] {
        let mut ds: Vec<f32> = offsets.iter().filter(|o| o.1.starts_with(prefix)).map(|o| o.0).collect();
        ds.sort_by(f32::total_cmp);
        if !ds.is_empty() {
            println!("  {prefix}: n={} median {:.2} p10 {:.2} p90 {:.2}", ds.len(), ds[ds.len() / 2], ds[ds.len() / 10], ds[ds.len() * 9 / 10]);
        }
    }
}

fn translate(value: Option<&Byml>) -> [f32; 3] {
    match value {
        Some(Byml::Array(v)) if v.len() == 3 => std::array::from_fn(|i| v[i].as_float().unwrap_or(0.0)),
        Some(Byml::Map(m)) => ["X", "Y", "Z"].map(|k| m.get(k).and_then(|v| v.as_float().ok()).unwrap_or(0.0)),
        _ => [0.0; 3],
    }
}
