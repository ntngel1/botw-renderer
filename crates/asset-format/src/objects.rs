//! The map's placed objects: the actors of the MainField map units around
//! a region, the map's `_Far` stand-ins near it, and which models each
//! actor draws. Written by `bake` from the game's map units
//! (`Map/MainField/<cell>/<cell>_Static.smubin` and `_Dynamic.smubin`,
//! the newest copy of each: base → update → DLC, the static units of
//! `Pack/TitleBG.pack` where no root has a loose one) and actor packs
//! (`Actor/ModelList/*.bmodellist`).
//!
//! Nothing the renderer decides is applied here: every actor of a cell is
//! kept (enemies and `_Far` actors too; the renderer skips them by name, as
//! the original renderer's `objects::skipped`), with the transform the map gives
//! it. `!Parameters` and links are not kept (nothing reads them yet).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Side of a map cell (m). MainField has columns A–J west to east from
/// x = −5000 and rows 1–8 north to south from z = −4000.
pub const CELL_SIZE: f32 = 1000.0;

/// The name of the MainField cell containing world `(x, z)`, e.g. `E-4`,
/// or `None` outside the playable map (`botw_formats::map::cell_name`).
pub fn cell_name(x: f32, z: f32) -> Option<String> {
    let column = ((x + 5000.0) / CELL_SIZE).floor();
    let row = ((z + 4000.0) / CELL_SIZE).floor();
    ((0.0..10.0).contains(&column) && (0.0..8.0).contains(&row))
        .then(|| format!("{}-{}", (b'A' + column as u8) as char, row as u8 + 1))
}

/// Every MainField cell name, `A-1` … `J-8`.
pub fn all_cells() -> impl Iterator<Item = String> {
    (b'A'..=b'J').flat_map(|column| (1..=8).map(move |row| format!("{}-{row}", column as char)))
}

/// The cell's square: minimum corner (x, z).
pub fn cell_origin(cell: &str) -> Option<[f32; 2]> {
    let (column, row) = cell.split_once('-')?;
    let column = column.bytes().next()?.checked_sub(b'A')?;
    let row: u8 = row.parse().ok()?;
    (column < 10 && (1..=8).contains(&row)).then(|| {
        [
            -5000.0 + f32::from(column) * CELL_SIZE,
            -4000.0 + f32::from(row - 1) * CELL_SIZE,
        ]
    })
}

/// `objects/index.ron`: what was baked, and for which region.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectIndex {
    /// The region's centre (x, z) and half side (m), as `cargo bake` took
    /// them.
    pub center: [f32; 2],
    pub radius: f32,
    /// How far around the region's square objects were baked (m): the
    /// cells a camera inside the square would load (the viewer's 3 × 3
    /// probe at its far radius × 1.1), and the models of the objects within
    /// its far radius (+ slack).
    pub reach: f32,
    /// How far around the square `_Far` stand-ins were kept (m).
    pub far_reach: f32,
    /// The cells written to `objects/cells/` (every other cell counts as
    /// having no actors).
    pub cells: Vec<String>,
}

/// A placed actor. The name is an index into its file's name table.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedActor {
    pub name: u32,
    pub hash_id: u32,
    /// World position.
    pub translate: [f32; 3],
    /// Rotation in radians as the map gives it: a single Y angle becomes
    /// `[0, y, 0]`; X, Y, Z are applied in that order (the original renderer's
    /// `objects::placement`: `Quat::from_euler(ZYX, z, y, x)`).
    pub rotate: [f32; 3],
    /// Scale (a uniform scale repeated).
    pub scale: [f32; 3],
}

/// A list of placed actors with their name table: one cell
/// (`objects/cells/<cell>.ron`) or the `_Far` index (`objects/far.ron`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PlacedActors {
    pub names: Vec<String>,
    /// In the units' order (static, then dynamic).
    pub actors: Vec<PlacedActor>,
}

impl PlacedActors {
    pub fn name(&self, actor: &PlacedActor) -> &str {
        self.names
            .get(actor.name as usize)
            .map_or("", String::as_str)
    }

    /// Adds an actor, interning its name.
    pub fn push(
        &mut self,
        lookup: &mut std::collections::HashMap<String, u32>,
        name: &str,
        mut actor: PlacedActor,
    ) {
        let index = *lookup.entry(name.to_owned()).or_insert_with(|| {
            self.names.push(name.to_owned());
            self.names.len() as u32 - 1
        });
        actor.name = index;
        self.actors.push(actor);
    }
}

/// `objects/far.ron`: the map's `_Far` actors within `ObjectIndex::far_reach`
/// of the region's square, and the names of every actor on the map that
/// has a `_Far` stand-in somewhere (the original renderer's `FarModels::stand_ins`,
/// taken over all cells).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FarIndex {
    pub placed: PlacedActors,
    pub stand_ins: Vec<String>,
}

/// A model an actor draws: the BFRES folder (`Model/<folder>.sbfres`) and
/// the model units in it; baked as `models/<folder>/<unit>.glb`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub folder: String,
    pub units: Vec<String>,
}

/// `objects/actors.ron`: the models of every placed actor the renderer may
/// draw (`ActorPacks::models`); actors not listed have none (or were out
/// of reach).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ActorModels {
    pub actors: BTreeMap<String, Vec<ModelRef>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_cells_like_the_game() {
        assert_eq!(cell_name(-4999.0, -3999.0).as_deref(), Some("A-1"));
        assert_eq!(cell_name(-1100.0, 1900.0).as_deref(), Some("D-6"));
        assert_eq!(cell_name(4999.0, 3999.0).as_deref(), Some("J-8"));
        assert_eq!(cell_name(5001.0, 0.0), None);
        assert_eq!(cell_origin("D-6"), Some([-2000.0, 1000.0]));
        assert_eq!(all_cells().count(), 80);
        assert!(all_cells().all(|c| cell_origin(&c).is_some()));
    }

    #[test]
    fn round_trips_a_cell() {
        let mut cell = PlacedActors::default();
        let mut lookup = Default::default();
        let actor = PlacedActor {
            name: 0,
            hash_id: 7,
            translate: [1.5, -2.0, 3.25],
            rotate: [0.0, 1.0e-7, 0.1],
            scale: [1.0; 3],
        };
        cell.push(&mut lookup, "Obj_Tree", actor);
        cell.push(&mut lookup, "Obj_Rock", actor);
        cell.push(&mut lookup, "Obj_Tree", actor);
        assert_eq!(cell.names, ["Obj_Tree", "Obj_Rock"]);
        assert_eq!(cell.name(&cell.actors[2]), "Obj_Tree");
        let text = ron::to_string(&cell).unwrap();
        assert_eq!(ron::from_str::<PlacedActors>(&text).unwrap(), cell);
    }
}
