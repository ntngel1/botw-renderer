//! The map's placed objects around the region: the MainField cells a
//! camera inside the region's square would load (the original renderer's
//! `objects::stream_objects`: a 3 × 3 probe at 1.1 × its far radius), the
//! map's `_Far` stand-ins within its horizon of the square, and the models
//! of every actor in reach (`ActorPacks::models`).
//!
//! Static units: the newest loose `<cell>_Static.smubin` (the DLC's), else
//! the one in `Pack/TitleBG.pack` (the viewer reads only TitleBG's; the
//! user's choice, docs/research/world-port-map.md). Dynamic units: the
//! newest copy, as the viewer.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;

use asset_format::objects::{
    ActorModels, FarIndex, ModelRef, ObjectIndex, PlacedActor, PlacedActors, all_cells, cell_origin,
};
use asset_format::paths;
use botw_formats::actor::ActorPacks;
use botw_formats::content::ContentRoots;

/// the original renderer's `Objects::far_radius` (m): the farthest a placed object
/// shows. Only how far around the region objects are baked.
// SI-WLD-03: object draw ranges and LOD choice are ours.
const FAR_RADIUS: f32 = 700.0;
/// The viewer probes cells at this many far radii around the camera.
const CELL_PROBE: f32 = 1.1;
/// the original renderer's `FarModels::horizon` (m).
// SI-WLD-03: object draw ranges and LOD choice are ours.
const HORIZON: f32 = 3500.0;
/// Slack beyond the viewer's distances (m): the renderer re-checks what to
/// spawn only after the camera has moved a little (`render::objects`),
/// spawning a margin ahead of the viewer's radii, and drops `_Far` stand-ins
/// only past 1.05 × the horizon.
const SLACK: f32 = 200.0;

/// Actors the renderer never draws from the map (the viewer's
/// `objects::skipped` prefixes, SI-WLD-06): their models are not baked.
/// `_Far` actors are baked through the `_Far` index.
// SI-WLD-06: actors skipped by name prefix (our heuristic).
const SKIPPED_PREFIXES: [&str; 6] = ["Enemy_", "Npc_", "Animal_", "Weapon_", "Player", "Horse_"];

pub struct Region {
    /// Centre (x, z).
    pub center: [f32; 2],
    /// Half the side of the square.
    pub radius: f32,
}

impl Region {
    /// Distance in the ground plane from `(x, z)` to the square (0 inside).
    pub fn distance(&self, x: f32, z: f32) -> f32 {
        let dx = ((x - self.center[0]).abs() - self.radius).max(0.0);
        let dz = ((z - self.center[1]).abs() - self.radius).max(0.0);
        dx.hypot(dz)
    }

    /// Whether the cell's square comes within `reach` of the region's
    /// square (in each axis: the probe offsets are per axis).
    pub fn touches(&self, cell: &str, reach: f32) -> bool {
        let Some([x0, z0]) = cell_origin(cell) else {
            return false;
        };
        let size = asset_format::objects::CELL_SIZE;
        let (lo_x, hi_x) = (
            self.center[0] - self.radius - reach,
            self.center[0] + self.radius + reach,
        );
        let (lo_z, hi_z) = (
            self.center[1] - self.radius - reach,
            self.center[1] + self.radius + reach,
        );
        x0 < hi_x && x0 + size > lo_x && z0 < hi_z && z0 + size > lo_z
    }
}

/// Bakes `objects/`: cells, the `_Far` index and the actors' models.
pub fn bake(roots: &ContentRoots, region: &Region, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let title_bg = roots
        .find("Pack/TitleBG.pack")
        .map(|p| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())))
        .transpose()?
        .map(Arc::new);

    // Every cell: the `_Far` stand-ins come from all of them.
    let cells: Vec<String> = all_cells().collect();
    let read: Vec<Result<Vec<botw_formats::map::PlacedActor>, String>> =
        parallel(&cells, |cell| read_cell(roots, title_bg.as_deref(), cell));
    let mut units = Vec::with_capacity(cells.len());
    for (cell, actors) in cells.iter().zip(read) {
        units.push((
            cell.clone(),
            actors.map_err(|e| format!("cell {cell}: {e}"))?,
        ));
    }

    let cells_dir = out.join(paths::OBJECT_CELLS);
    if cells_dir.exists() {
        std::fs::remove_dir_all(&cells_dir).map_err(|e| format!("{}: {e}", cells_dir.display()))?;
    }
    let cell_reach = FAR_RADIUS * CELL_PROBE;
    let mut index = ObjectIndex {
        center: region.center,
        radius: region.radius,
        reach: cell_reach,
        far_reach: HORIZON + SLACK,
        cells: Vec::new(),
    };
    let mut far = FarIndex::default();
    let mut far_names = HashMap::new();
    let mut stand_ins = BTreeSet::new();
    // Actor names whose models the renderer may need.
    let mut wanted: BTreeSet<String> = BTreeSet::new();
    let mut placed = 0;
    for (cell, actors) in &units {
        for actor in actors {
            if let Some(real) = actor.name.strip_suffix("_Far") {
                stand_ins.insert(real.to_owned());
                if region.distance(actor.translate[0], actor.translate[2]) < index.far_reach {
                    far.placed
                        .push(&mut far_names, &actor.name, placed_actor(actor));
                    wanted.insert(actor.name.clone());
                }
            }
        }
        if !region.touches(cell, cell_reach) {
            continue;
        }
        let mut file = PlacedActors::default();
        let mut names = HashMap::new();
        for actor in actors {
            file.push(&mut names, &actor.name, placed_actor(actor));
            let drawn = !SKIPPED_PREFIXES.iter().any(|p| actor.name.starts_with(p))
                && !actor.name.ends_with("_Far");
            if drawn && region.distance(actor.translate[0], actor.translate[2]) < FAR_RADIUS + SLACK
            {
                wanted.insert(actor.name.clone());
            }
        }
        placed += file.actors.len();
        asset_format::write_ron(&out.join(paths::object_cell(cell)), &file)
            .map_err(|e| e.to_string())?;
        index.cells.push(cell.clone());
    }
    far.stand_ins = stand_ins.into_iter().collect();
    asset_format::write_ron(&out.join(paths::FAR_INDEX), &far).map_err(|e| e.to_string())?;
    asset_format::write_ron(&out.join(paths::OBJECT_INDEX), &index).map_err(|e| e.to_string())?;
    println!(
        "objects: {} cells ({} actors), {} far stand-ins, {} names in reach ({:.1} s)",
        index.cells.len(),
        placed,
        far.placed.actors.len(),
        wanted.len(),
        started.elapsed().as_secs_f32()
    );

    // Which models they draw.
    let packs = ActorPacks::new(roots.clone(), title_bg);
    let names: Vec<String> = wanted.into_iter().collect();
    let refs = parallel(&names, |name| packs.models(name).map_err(|e| e.to_string()));
    let mut models = ActorModels {
        actors: BTreeMap::new(),
    };
    for (name, refs) in names.iter().zip(refs) {
        let refs = refs.map_err(|e| format!("actor {name}: {e}"))?;
        if !refs.is_empty() {
            models.actors.insert(
                name.clone(),
                refs.into_iter()
                    .map(|r| ModelRef {
                        folder: r.folder,
                        units: r.units,
                    })
                    .collect(),
            );
        }
    }
    asset_format::write_ron(&out.join(paths::ACTOR_MODELS), &models).map_err(|e| e.to_string())?;
    println!(
        "objects: {} of {} actors have models ({:.1} s)",
        models.actors.len(),
        names.len(),
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

fn placed_actor(actor: &botw_formats::map::PlacedActor) -> PlacedActor {
    PlacedActor {
        name: 0,
        hash_id: actor.hash_id,
        translate: actor.translate,
        rotate: actor.rotate,
        scale: actor.scale,
    }
}

/// A cell's static and dynamic units.
pub fn read_cell(
    roots: &ContentRoots,
    title_bg: Option<&Vec<u8>>,
    cell: &str,
) -> Result<Vec<botw_formats::map::PlacedActor>, String> {
    let read = |path: std::path::PathBuf| {
        std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let mut actors = Vec::new();
    let static_unit = format!("Map/MainField/{cell}/{cell}_Static.smubin");
    let bytes = match roots.find(&static_unit) {
        Some(path) => Some(read(path)?),
        None => match title_bg {
            Some(pack) => roead::sarc::Sarc::new(&pack[..])
                .map_err(|e| format!("TitleBG.pack: {e}"))?
                .get_data(&static_unit)
                .map(<[u8]>::to_vec),
            None => None,
        },
    };
    if let Some(bytes) = bytes {
        actors.extend(botw_formats::map::parse_unit(&bytes).map_err(|e| e.to_string())?);
    }
    if let Some(path) = roots.find(format!("Map/MainField/{cell}/{cell}_Dynamic.smubin")) {
        actors.extend(botw_formats::map::parse_unit(&read(path)?).map_err(|e| e.to_string())?);
    }
    Ok(actors)
}

/// `f` over `items` on every core, results in order.
pub fn parallel<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut results: Vec<(usize, R)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some(item) = items.get(i) else { break };
                        done.push((i, f(item)));
                    }
                    done
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().expect("worker thread panicked"))
            .collect()
    });
    results.sort_by_key(|(i, _)| *i);
    results.into_iter().map(|(_, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_cells_a_region_camera_loads() {
        // Hateno: x 2592.7..4592.7, z 1121.9..3121.9, plus 770 m.
        let region = Region {
            center: [3592.7, 2121.9],
            radius: 1000.0,
        };
        let cells: Vec<String> = all_cells()
            .filter(|c| region.touches(c, FAR_RADIUS * CELL_PROBE))
            .collect();
        assert_eq!(
            cells,
            [
                "G-5", "G-6", "G-7", "G-8", "H-5", "H-6", "H-7", "H-8", "I-5", "I-6", "I-7", "I-8",
                "J-5", "J-6", "J-7", "J-8"
            ]
        );
        assert_eq!(region.distance(3592.7, 2121.9), 0.0);
        assert!((region.distance(4592.7 + 3.0, 3121.9 + 4.0) - 5.0).abs() < 1e-3);
    }
}
