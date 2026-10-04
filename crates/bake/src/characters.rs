//! Character assets: skinned models with
//! their skeletons, and skeletal clips, under `characters/` (see
//! `asset_format::character`).
//!
//! Link: his body `Link` and the pieces of the outfit the viewer starts him
//! in (`tunic`: `Armor_Default_Head`, `Armor_001_Upper`, `Armor_001_Lower`),
//! dressed as the original renderer's `player/outfit.rs` `dress` does (pieces'
//! models, the Sheikah Slate pouch, body shapes the pieces cover, the ears'
//! turn under the head piece), baked with the viewer's skinned loads
//! (`LoadOptions { skinned, shared_textures: [Link] }`), and the clips of
//! `Player_Animation` the viewer's `player/visuals.rs` `link_spec` loads,
//! sampled per frame (the viewer's `character::bake`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use asset_format::character::CharacterDef;
use asset_format::paths;
use botw_formats::actor::ActorPacks;
use botw_formats::armor::{Armor, Slot};
use botw_formats::bfres::Bfres;
use botw_formats::content::ContentRoots;
use glam::{EulerRot, Quat};

use crate::clips::sample_clip;
use crate::models::{LoadOptions, bake_folder, read_bfres};
use crate::objects::parallel;

/// The outfit Link starts out in (the viewer's `OUTFITS[0]`, `tunic`).
const LINK_OUTFIT: [&str; 3] = ["Armor_Default_Head", "Armor_001_Upper", "Armor_001_Lower"];

/// Link's animation file.
const LINK_ANIMATIONS: &str = "Player_Animation";

/// The clips the original renderer loads for Link (`player/visuals.rs`,
/// `link_spec`, with its run and walk starts and `ATTACK_CLIP`).
const LINK_CLIPS: &[&str] = &[
    "Nml_Wait",
    "Random_Nml_Wait_RelaxUpper",
    "Random_Nml_Wait_Kinnikujiman",
    "Random_Nml_Wait_Nemui",
    "Nml_Move_Walk_Ed_L",
    "Nml_Move_Walk_Ed_R",
    "Nml_Move_Run_Ed_L",
    "Nml_Move_Run_Ed_R",
    "Nml_Move_Walk",
    "Nml_Move_Run",
    "Nml_Move_Dash",
    "Nml_Move_Walk_Curve_L180",
    "Nml_Move_Walk_Curve_R180",
    "Nml_Move_Run_Curve_L180",
    "Nml_Move_Run_Curve_R180",
    "Nml_Move_Dash_Curve_L_180",
    "Nml_Move_Dash_Curve_R_180",
    "Nml_Move_Hard_Curve_L",
    "Nml_Move_Hard_Curve_R",
    "Nml_Move_Walk_Slope_U45",
    "Nml_Move_Walk_Slope_D45",
    "Nml_Move_Run_Slope_U45",
    "Nml_Move_Run_Slope_D45",
    "Nml_Move_Dash_U45",
    "Nml_Move_Dash_D45",
    "Nml_Move_Run_Brake_FootL",
    "Nml_Move_Run_Brake_FootR",
    "Nml_Move_Dash_Brake_FootL",
    "Nml_Move_Dash_Brake_FootR",
    "Nml_Tired_Wait_St_L",
    "Nml_Tired_Wait_St_R",
    "Nml_Tired_Wait",
    "Nml_Tired_Wait_Ed",
    "Nml_Move_Walk_Hard",
    "Nml_Move_Run_Hard",
    "Nml_Wait_Jump",
    "Nml_Move_Run_Jump_FootL",
    "Nml_Move_Run_Jump_FootR",
    "Nml_Move_Run_Jump_FootL_SwitchL",
    "Nml_Move_Run_Jump_FootL_SwitchR",
    "Nml_Move_Run_Jump_FootR_SwitchL",
    "Nml_Move_Run_Jump_FootR_SwitchR",
    "Fall",
    "Nml_Wait_Land",
    "Nml_Move_Run_Land_FootL",
    "Nml_Move_Run_Land_FootR",
    "Nml_Move_Run_Land_ToRun_FootL",
    "Nml_Move_Run_Land_ToRun_FootR",
    "Nml_Damage_Land",
    "Nml_Damage_Land_Stand",
    "Equip_Float_On",
    "Equip_Float_On_Fall",
    "Equip_Float_Off",
    "Float_Float",
    "Climb_Wall_Wait_FootL",
    "Climb_Wall_Wait_FootR",
    "Climb_AirToWall",
    "Climb_Wall_DownToWall",
    "Wall_Dash_FootL",
    "Wall_Dash_FootR",
    "Climb_WallOff_ToWait_FootL",
    "Climb_WallOff_ToWait_FootR",
    "Climb_Wall_Off_Tired_FootL",
    "Climb_WallOff_ToSquat_FootL",
    "Climb_WallOff_ToSquat_FootR",
    "Climb_Jump_Off_FootL",
    "Climb_Wall_Ed_Edge_L",
    "Climb_Wall_Ed_Edge_R",
    "Climb_Wall_Move_U",
    "Climb_Wall_Move_D",
    "Climb_Wall_Move_L",
    "Climb_Wall_Move_R",
    "Climb_Wall_Move_UL",
    "Climb_Wall_Move_UR",
    "Climb_Wall_Move_DL",
    "Climb_Wall_Move_DR",
    "Climb_Wall_Move_N_FootL",
    "Climb_Wall_Move_N_FootR",
    "Climb_Wall_Move_U_Add45",
    "Climb_Wall_Move_D_Add45",
    "Climb_Wall_Move_L_Add45",
    "Climb_Wall_Move_R_Add45",
    "Climb_Wall_Move_UL_Add45",
    "Climb_Wall_Move_UR_Add45",
    "Climb_Wall_Move_DL_Add45",
    "Climb_Wall_Move_DR_Add45",
    "Climb_Wall_Jump_R_FootR",
    "Climb_Wall_Jump_UR_FootL",
    "Climb_Wall_Jump_UR_FootR",
    "Climb_Wall_Jump_U_FootL",
    "Climb_Wall_Jump_U_FootR",
    "Climb_Wall_Jump_UL_FootL",
    "Climb_Wall_Jump_UL_FootR",
    "Climb_Wall_Jump_L_FootL",
    "Float_Float_F",
    "Float_Float_L",
    "Float_Float_R",
    "Float_Float_B",
    "Swim_Move_Fast_F_Curve_L",
    "Swim_Move_Fast_F_Curve_R",
    "Swim_Wait",
    "Swim_Move_Fast_F",
    "Swim_Move_Slow_F",
    "Swim_Move_Slow_F_Curve_L",
    "Swim_Move_Slow_F_Curve_R",
    "Swim_Move_Dash",
    "Swim_Ed_Edge_0cm",
    "Swim_Ed_Edge_50cm",
    "Sword_Attack_S1",
    "Face_Default",
    "Face_Default_Blink",
    "Face_Tired",
    "Face_Tired_Blink",
    "Voice_Serious_A",
    "Face_Random_Nml_Wait_RelaxUpper",
    "Face_Random_Nml_Wait_Kinnikujiman",
    "Face_Random_Nml_Wait_Nemui",
    "Nml_Move_Run_St_R180",
    "Nml_Move_Run_St_R090",
    "Nml_Move_Run_St_R020",
    "Nml_Move_Run_St_F_R020",
    "Nml_Move_Run_St_F_L020",
    "Nml_Move_Run_St_L020",
    "Nml_Move_Run_St_L090",
    "Nml_Move_Run_St_L180",
    "Nml_Move_Walk_St_R180",
    "Nml_Move_Walk_St_R090",
    "Nml_Move_Walk_St_R020",
    "Nml_Move_Walk_St_F_R020",
    "Nml_Move_Walk_St_F_L020",
    "Nml_Move_Walk_St_L020",
    "Nml_Move_Walk_St_L090",
    "Nml_Move_Walk_St_L180",
];

pub fn bake(roots: &ContentRoots, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let title_bg = roots
        .find("Pack/TitleBG.pack")
        .map(|p| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())))
        .transpose()?
        .map(Arc::new);
    let packs = ActorPacks::new(roots.clone(), title_bg);
    let model_files: Vec<String> = roots
        .list_dir("Model")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    // What this step baked before goes (the villagers under `umii/` and
    // their clips are the `npcs` step's).
    remove_outputs(out, &["umii"], &["UMii_"])?;

    let pieces = LINK_OUTFIT
        .iter()
        .map(|actor| match Armor::read(&packs, actor) {
            Ok(Some(armor)) if !armor.models.is_empty() => Ok(armor),
            Ok(_) => Err(format!("{actor}: no armour actor with models")),
            Err(error) => Err(format!("{actor}: {error}")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let (parts, hidden) = dress(&pieces);
    let joint_offsets = ear_rotation(&pieces).map_or_else(Vec::new, |turn| {
        ["Ear_L", "Ear_R"]
            .iter()
            .map(|ear| (ear.to_string(), turn.to_array()))
            .collect()
    });
    let (units, textures, model_bytes) = bake_parts(&packs, &model_files, &parts, out)?;
    let (clips, clip_bytes) = bake_clips(&packs, LINK_ANIMATIONS, LINK_CLIPS, out)?;
    let def = CharacterDef {
        parts,
        hidden,
        joint_offsets,
        animations: LINK_ANIMATIONS.into(),
        clips,
        ..Default::default()
    };
    asset_format::write_ron(&out.join(paths::character("link")), &def)
        .map_err(|e| e.to_string())?;
    println!(
        "characters: link ({} parts, {units} units, {textures} textures, {:.0} MB; {} clips, {:.0} MB) in {:.1} s",
        def.parts.len(),
        model_bytes as f64 / 1e6,
        def.clips.len(),
        clip_bytes as f64 / 1e6,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

/// Removes `characters/` but for the entries (in it and in `models/`)
/// named in `keep` and the clip sets in `anims/` starting with one of
/// `keep_anims`.
pub(crate) fn remove_outputs(out: &Path, keep: &[&str], keep_anims: &[&str]) -> Result<(), String> {
    let dir = out.join(paths::CHARACTERS);
    let remove = |path: &Path| -> Result<(), String> {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_file(path)
        };
        result.map_err(|e| format!("{}: {e}", path.display()))
    };
    let entries = |path: &Path| -> Vec<(String, std::path::PathBuf)> {
        std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .collect()
    };
    for (name, path) in entries(&dir) {
        match name.as_str() {
            "models" => {
                for (name, path) in entries(&path) {
                    if !keep.contains(&name.as_str()) {
                        remove(&path)?;
                    }
                }
            }
            "anims" => {
                for (name, path) in entries(&path) {
                    if !keep_anims.iter().any(|k| name.starts_with(k)) {
                        remove(&path)?;
                    }
                }
            }
            _ if keep.contains(&name.as_str()) => {}
            _ => remove(&path)?,
        }
    }
    Ok(())
}

/// Bakes a character's model units, skinned, each folder taking textures it
/// lacks from the body's (the first part's) folder. Returns the units and
/// textures written and their size.
fn bake_parts(
    packs: &ActorPacks,
    model_files: &[String],
    parts: &[(String, String)],
    out: &Path,
) -> Result<(usize, usize, u64), String> {
    let mut folders: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (folder, unit) in parts {
        folders
            .entry(folder.clone())
            .or_default()
            .insert(unit.clone());
    }
    let folders: Vec<(String, BTreeSet<String>)> = folders.into_iter().collect();
    let options = LoadOptions {
        skinned: true,
        shared_textures: parts.first().map(|(f, _)| f.clone()).into_iter().collect(),
        edits: None,
        face_trig: None,
    };
    let dir = out.join(paths::CHARACTER_MODELS);
    let results = parallel(&folders, |(folder, units)| {
        bake_folder(
            packs,
            model_files,
            folder,
            units,
            &dir.join(folder),
            &options,
        )
    });
    let (mut units, mut textures, mut bytes) = (0, 0, 0);
    for ((folder, _), result) in folders.iter().zip(results) {
        let stats = result.map_err(|e| format!("{folder}: {e}"))?;
        if let Some(unit) = stats.missing_units.first() {
            return Err(format!("{folder}: no model {unit}"));
        }
        units += stats.units;
        textures += stats.textures;
        bytes += stats.bytes;
    }
    Ok((units, textures, bytes))
}

/// Bakes clips `names` of the game's animation file `set`; returns the
/// names baked and their size. Clips the file lacks are left out, as the
/// viewer leaves them out.
pub(crate) fn bake_clips(
    packs: &ActorPacks,
    set: &str,
    names: &[&str],
    out: &Path,
) -> Result<(Vec<String>, u64), String> {
    let bytes = read_bfres(packs, &format!("Model/{set}.sbfres"))
        .ok_or_else(|| format!("Model/{set}.sbfres not found"))?;
    let bfres = Bfres::parse(&bytes).map_err(|e| format!("{set}: {e}"))?;
    let (mut baked, mut size) = (Vec::new(), 0);
    for &name in names {
        match bfres.skeletal_anim(name) {
            Ok(Some(anim)) => {
                let glb = sample_clip(&anim)
                    .to_glb()
                    .map_err(|e| format!("{set}/{name}: {e}"))?;
                asset_format::write(&out.join(paths::character_clip(set, name)), &glb)
                    .map_err(|e| e.to_string())?;
                size += glb.len() as u64;
                baked.push(name.to_owned());
            }
            Ok(None) => println!("warning: {set}: no animation {name}"),
            Err(error) => println!("warning: {set}: animation {name}: {error}"),
        }
    }
    Ok((baked, size))
}

/// Link's body shapes a worn upper or lower piece covers: it brings its
/// own skin (every upper has an `Mt_Upper_Skin` shape for the arms and
/// hands that show) and belts, the legs cover the body's legs and shorts
/// (the viewer's `outfit.rs`).
// SI-EQP-04: armour speed per level and covered body parts are ours.
const UPPER_BODY: [&str; 3] = [
    "Skin__Mt_Upper_Skin",
    "Belt_A_Buckle__Mt_Belt_A",
    "Belt_C_Buckle__Mt_Belt_C",
];
const LOWER_BODY: [&str; 2] = ["Skin__Mt_Lower_Skin", "Skin__Mt_Underwear"];
/// The Sheikah Slate pouch on the belt, worn unless an upper piece hides it.
const POUCH: [(&str, &str); 2] = [
    ("Armor_Default", "Armor_Default_Extra_00"),
    ("Armor_Default", "Armor_Default_Extra_01"),
];

/// Link dressed in `pieces`: the models to put on his skeleton (the body
/// first) and the body shapes to hide (the viewer's `outfit::dress`).
fn dress(pieces: &[Armor]) -> (Vec<(String, String)>, Vec<String>) {
    let mut parts = vec![("Link".to_owned(), "Link".to_owned())];
    let mut hidden = Vec::new();
    for piece in pieces {
        for model in &piece.models {
            parts.extend(
                model
                    .units
                    .iter()
                    .map(|unit| (model.folder.clone(), unit.clone())),
            );
        }
        if !piece.is_default() {
            match piece.slot {
                Slot::Upper => hidden.extend(UPPER_BODY.map(String::from)),
                Slot::Lower => hidden.extend(LOWER_BODY.map(String::from)),
                Slot::Head => {}
            }
        }
    }
    if !pieces
        .iter()
        .any(|p| p.slot == Slot::Upper && p.hides_pouch)
    {
        parts.extend(POUCH.map(|(folder, unit)| (folder.to_owned(), unit.to_owned())));
    }
    (parts, hidden)
}

/// How the ears turn under the head piece, as a rotation of each ear bone
/// (the viewer's `outfit::ear_rotation`).
fn ear_rotation(pieces: &[Armor]) -> Option<Quat> {
    let [x, y, z] = pieces.iter().find(|p| p.slot == Slot::Head)?.ear_rotate;
    // SI-EQP-05: EarRotate read as Euler XYZ degrees.
    (x != 0.0 || y != 0.0 || z != 0.0).then(|| {
        Quat::from_euler(
            EulerRot::XYZ,
            x.to_radians(),
            y.to_radians(),
            z.to_radians(),
        )
    })
}

#[cfg(test)]
mod tests {
    use botw_formats::actor::ModelRef;

    use super::*;

    fn piece(actor: &str) -> Armor {
        Armor {
            actor: actor.into(),
            slot: Slot::of(actor).unwrap(),
            models: vec![ModelRef {
                folder: actor[..9].into(),
                units: vec![actor.into()],
            }],
            series: String::new(),
            effect: "None".into(),
            effect_level: 0,
            set_bonus: false,
            ear_rotate: [0.0; 3],
            hides_pouch: false,
        }
    }

    #[test]
    fn dressing_hides_the_body_under_the_clothes() {
        let pieces = LINK_OUTFIT.map(piece);
        let (parts, hidden) = dress(&pieces);
        assert_eq!(parts[0], ("Link".into(), "Link".into()));
        assert!(parts.contains(&("Armor_001".into(), "Armor_001_Upper".into())));
        assert!(parts.iter().any(|(_, u)| u == "Armor_Default_Extra_00"));
        assert!(hidden.contains(&"Skin__Mt_Upper_Skin".to_owned()));
        assert!(ear_rotation(&pieces).is_none());
    }
}
