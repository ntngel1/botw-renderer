//! The region's UMii villagers (`cargo bake --only npcs`): each placed
//! `Npc_*` actor whose pack has a `.bumii` (`Actor/UMii/`) becomes a
//! character `umii/<actor>` (see `asset_format::character`), assembled as
//! the game assembles it (docs/research/umii.md):
//!
//! - its models by [`Umii::parts`] (Hylians; the other races are left out),
//!   or the body its model list names (Sayge, the carpenters);
//! - its colours: the game plays shader-parameter animations at the
//!   parameters' frames ([`Umii::colour_frames`]); the `const_colorN` they
//!   set are folded into each material's albedo by the material's colour
//!   recipe (`uking_colorN_*`, [`albedo_term`]) and baked as the
//!   villager's own albedo textures;
//! - parts follow the body by the bones the game copies every frame
//!   ([`aliases`]);
//! - its idle: the clip its wait sequence loops for its personality
//!   ([`Umii::wait_clip`]) from the body animation file.
//!
//! Output: `characters/umii/<actor>.ron`, `characters/models/umii/<actor>/`
//! (each villager its own folder: the textures are its own),
//! `characters/anims/UMii_*`, and `characters/umii/placed.ron` (where the
//! map puts them).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use asset_format::character::{CharacterDef, JointAdjustment, PlacedCharacter, PlacedCharacters};
use asset_format::paths;
use botw_formats::actor::ActorPacks;
use botw_formats::bfres::model::Material;
use botw_formats::bfres::{Bfres, TextureImage, bc, gx2};
use botw_formats::content::ContentRoots;

use crate::material_anims::shader_params_at;
use crate::models::{FindTexture, LoadOptions, MaterialEdits, bake_folder, read_bfres};
use crate::objects::{Region, parallel, read_cell};
use crate::umii::{ConstructionInfo, Part, Umii};

/// Villagers within this distance (m) of the region's square are baked
/// (the objects' `FAR_RADIUS`: the farthest a placed object shows).
// SI-WLD-03: object draw ranges and LOD choice are ours.
const REACH: f32 = 700.0;

/// Files holding the colour animations every part takes (the skin, hair,
/// beard, glasses and face colours).
const SHARED_ANIMATIONS: [&str; 5] = [
    "UMii_Hylia_Body",
    "UMii_Hylia_Hair",
    "UMii_Hylia_Mustache",
    "UMii_Hylia_Glass",
    "UMii_Hylia_Face_Edit",
];

/// Texture files the parts share (face features, mouths).
const SHARED_TEXTURES: [&str; 2] = ["UMii_Hylia_Face", "UMii_Hylia_Face_Edit"];

/// Bones of a part the game sets from another part's bone every frame
/// besides the `X_Controled` → `X` copies ([`aliases`]): `(bone, joint)`
/// (`FUN_0343b2ec` hair, `FUN_0343b8e4` hat; docs/research/umii.md).
const FOLLOWS: [(&str, &str); 5] = [
    ("Hair_Root", "Neck"),
    ("Hair_Head", "Head"),
    ("Hat_Root", "Neck"),
    ("Hat_Head", "Head"),
    // Native v208 0x0343ce24 copies the face's world matrix every frame.
    ("Nose_D", "Nose_Base"),
];

pub fn bake(roots: &ContentRoots, region: &Region, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let face_trig = Arc::new(crate::texture_srt::TextureSrtTable::load(roots)?);
    let title_bg = roots
        .find("Pack/TitleBG.pack")
        .map(|p| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())))
        .transpose()?
        .map(Arc::new);
    let packs = ActorPacks::new(roots.clone(), title_bg.clone());
    let model_files: Vec<String> = roots
        .list_dir("Model")
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let info = construction_info(roots)?;

    // Where the map puts villagers around the region.
    let cells: Vec<String> = asset_format::objects::all_cells()
        .filter(|c| region.touches(c, REACH))
        .collect();
    let read = parallel(&cells, |cell| read_cell(roots, title_bg.as_deref(), cell));
    let mut placements = Vec::new();
    for (cell, actors) in cells.iter().zip(read) {
        for actor in actors.map_err(|e| format!("cell {cell}: {e}"))? {
            if actor.name.starts_with("Npc_")
                && region.distance(actor.translate[0], actor.translate[2]) < REACH
            {
                placements.push(actor);
            }
        }
    }
    let names: BTreeSet<String> = placements.iter().map(|a| a.name.clone()).collect();
    let names: Vec<String> = names.into_iter().collect();
    let villagers = parallel(&names, |name| villager(&packs, &model_files, &info, name));

    // Only this step writes these.
    for dir in [paths::UMII, paths::UMII_MODELS] {
        let dir = out.join(dir);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }
    let anims = out.join(paths::CHARACTER_ANIMS);
    for set in [
        "UMii_Common_Body_M_Animation",
        "UMii_Common_Body_W_Animation",
    ] {
        let dir = anims.join(set);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
    }

    let mut baked: BTreeMap<String, Villager> = BTreeMap::new();
    for (name, villager) in names.iter().zip(villagers) {
        match villager {
            Ok(Some(villager)) => {
                baked.insert(name.clone(), villager);
            }
            Ok(None) => {}
            Err(error) => println!("warning: npcs: {name}: {error}"),
        }
    }
    let list: Vec<(&String, &Villager)> = baked.iter().collect();
    let results = parallel(&list, |(name, villager)| {
        bake_villager(&packs, &model_files, name, villager, out, &face_trig)
    });
    let mut written = BTreeSet::new();
    let (mut units, mut bytes) = (0, 0);
    for ((name, _), result) in list.iter().zip(results) {
        match result {
            Ok((u, b)) => {
                written.insert((*name).clone());
                units += u;
                bytes += b;
            }
            Err(error) => println!("warning: npcs: {name}: {error}"),
        }
    }

    // Their idles, each clip once.
    let mut clips: BTreeMap<&'static str, BTreeSet<&'static str>> = BTreeMap::new();
    for (name, villager) in &baked {
        if written.contains(name) {
            clips
                .entry(villager.umii.body_animations())
                .or_default()
                .insert(villager.umii.wait_clip());
        }
    }
    let mut clip_bytes = 0;
    for (set, names) in &clips {
        let names: Vec<&str> = names.iter().copied().collect();
        let (_, size) = crate::characters::bake_clips(&packs, set, &names, out)?;
        clip_bytes += size;
    }

    let placed = PlacedCharacters {
        placed: placements
            .iter()
            .filter(|a| written.contains(&a.name))
            .map(|a| PlacedCharacter {
                character: format!("umii/{}", a.name),
                actor: a.name.clone(),
                hash_id: a.hash_id,
                translate: a.translate,
                rotate: a.rotate,
            })
            .collect(),
    };
    asset_format::write_ron(&out.join(paths::UMII_PLACED), &placed).map_err(|e| e.to_string())?;
    println!(
        "npcs: {} of {} placed villagers ({} actors, {units} units, {:.0} MB; clips {:.1} MB) in {:.1} s",
        placed.placed.len(),
        placements.len(),
        written.len(),
        bytes as f64 / 1e6,
        clip_bytes as f64 / 1e6,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}

fn construction_info(roots: &ContentRoots) -> Result<ConstructionInfo, String> {
    let path = roots
        .find("Pack/Bootup.pack")
        .ok_or("Pack/Bootup.pack not found")?;
    let pack = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let sarc = roead::sarc::Sarc::new(&pack[..]).map_err(|e| format!("Bootup.pack: {e}"))?;
    let data = sarc
        .get_data("Mii/UMiiConstructionInfo.byml")
        .ok_or("Bootup.pack: no Mii/UMiiConstructionInfo.byml")?;
    let data = botw_formats::yaz0::decompress_if(data).map_err(|e| e.to_string())?;
    let mut info = ConstructionInfo::parse(&data)?;
    let angles = sarc
        .get_data("Mii/umii.bnetfp")
        .ok_or("Bootup.pack: no Mii/umii.bnetfp")?;
    let angles = botw_formats::yaz0::decompress_if(angles).map_err(|e| e.to_string())?;
    info.body_angles = crate::umii::BodyAngles::parse(&angles)?;
    Ok(info)
}

/// A villager to bake: its parameters and models.
struct Villager {
    umii: Umii,
    parts: Vec<(Part, String)>,
}

/// The villager actor `name` is, if it is a UMii this step can build.
fn villager(
    packs: &ActorPacks,
    model_files: &[String],
    info: &ConstructionInfo,
    name: &str,
) -> Result<Option<Villager>, String> {
    let Some(pack) = packs.open(name).map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let Some(bytes) = pack.find("Actor/UMii/") else {
        return Ok(None);
    };
    let bytes = botw_formats::yaz0::decompress_if(&bytes).map_err(|e| e.to_string())?;
    let umii = Umii::parse(&bytes)?;
    if umii.ffsd_type == 1 {
        return Err("built from a Mii (ffsd type 1), not supported".into());
    }
    let Some(mut parts) = umii.parts(info) else {
        return Err(format!("race {} is not built yet", umii.race));
    };
    // A model list naming a real model gives the body (Sayge's dyer's
    // clothes, the carpenters); the others name `Umii_Dummy`.
    let models = packs.models(name).map_err(|e| e.to_string())?;
    if let Some(body) = models.iter().find(|m| m.folder != "Umii_Dummy") {
        parts.retain(|(p, _)| *p != Part::Body);
        for unit in body.units.iter().rev() {
            parts.insert(0, (Part::Body, format!("{}/{unit}", body.folder)));
        }
    }
    let exists = |model: &str| {
        let folder = model.split('/').next().unwrap_or(model);
        model_files.iter().any(|f| f == &format!("{folder}.sbfres"))
    };
    let missing: Vec<String> = parts
        .iter()
        .filter(|(_, m)| !exists(m))
        .map(|(_, m)| m.clone())
        .collect();
    if parts
        .iter()
        .any(|(p, m)| matches!(p, Part::Body | Part::Face) && missing.contains(m))
    {
        return Err(format!("no model {}", missing.join(", ")));
    }
    for model in &missing {
        println!("warning: npcs: {name}: no model {model}, left out");
    }
    parts.retain(|(_, m)| !missing.contains(m));
    Ok(Some(Villager { umii, parts }))
}

/// `(folder, unit)` of a part: `folder/unit` or a model whose folder is
/// its name.
fn folder_unit(model: &str) -> (&str, &str) {
    model.split_once('/').unwrap_or((model, model))
}

/// Bakes a villager's parts into its folder and writes its character;
/// returns the units written and their size.
fn bake_villager(
    packs: &ActorPacks,
    model_files: &[String],
    name: &str,
    villager: &Villager,
    out: &Path,
    face_trig: &Arc<crate::texture_srt::TextureSrtTable>,
) -> Result<(usize, u64), String> {
    let umii = &villager.umii;
    let body = villager
        .parts
        .iter()
        .find(|(p, _)| *p == Part::Body)
        .map(|(_, m)| folder_unit(m).1)
        .ok_or("no body")?;
    let backpack = villager
        .parts
        .iter()
        .find(|(p, _)| *p == Part::BackPack)
        .map(|(_, m)| m.as_str());

    // The colours: each animation at its frame, from the part's own file
    // (that part only) or a shared one (every part).
    let shared: Vec<(String, Vec<u8>)> = SHARED_ANIMATIONS
        .iter()
        .filter_map(|f| {
            Some((
                f.to_string(),
                read_bfres(packs, &format!("Model/{f}.sbfres"))?,
            ))
        })
        .collect();
    let own: Vec<(String, Vec<u8>)> = villager
        .parts
        .iter()
        .filter_map(|(_, m)| {
            let folder = folder_unit(m).0;
            Some((
                folder.to_owned(),
                read_bfres(packs, &format!("Model/{folder}.sbfres"))?,
            ))
        })
        .collect();
    // (folder or "" for every part, material, parameter) → value.
    let mut colours: BTreeMap<(String, String, String), [f32; 4]> = BTreeMap::new();
    for (animation, frame) in umii.colour_frames(body, backpack) {
        let found = own
            .iter()
            .find_map(|(f, b)| Some((f.clone(), shader_params_at(b, &animation, frame as f32)?)))
            .or_else(|| {
                shared.iter().find_map(|(_, b)| {
                    Some((
                        String::new(),
                        shader_params_at(b, &animation, frame as f32)?,
                    ))
                })
            });
        let Some((scope, values)) = found else {
            continue;
        };
        for (material, param, components) in values {
            let value = colours
                .entry((scope.clone(), material, param))
                .or_insert([f32::NAN; 4]);
            for (k, v) in components {
                if let Some(slot) = value.get_mut(k) {
                    *slot = v;
                }
            }
        }
    }

    let dir = out.join(paths::UMII_MODELS).join(name);
    let recipes: Arc<Mutex<HashMap<String, Material>>> = Arc::default();
    let (mut units, mut bytes) = (0, 0);
    for (_, model) in &villager.parts {
        let (folder, unit) = folder_unit(model);
        let colours: Vec<((String, String), [f32; 4])> = colours
            .iter()
            .filter(|((scope, _, _), _)| scope.is_empty() || scope == folder)
            .map(|((_, m, p), v)| ((m.clone(), p.clone()), *v))
            .collect();
        let edit_recipes = recipes.clone();
        let texture_recipes = recipes.clone();
        let options = LoadOptions {
            skinned: true,
            face_trig: Some(face_trig.clone()),
            shared_textures: SHARED_TEXTURES.iter().map(|s| s.to_string()).collect(),
            edits: Some(MaterialEdits {
                material: Arc::new(move |model: &str, material: &mut Material| {
                    for ((m, p), value) in &colours {
                        if *m == material.name {
                            set_param(material, p, value);
                        }
                    }
                    tint(model, material, &edit_recipes);
                }),
                texture: Arc::new(move |name: &str, find: FindTexture| {
                    let recipe = texture_recipes.lock().ok()?.get(name).cloned()?;
                    albedo_image(&recipe, find)
                }),
            }),
        };
        let stats = bake_folder(
            packs,
            model_files,
            folder,
            &BTreeSet::from([unit.to_owned()]),
            &dir,
            &options,
        )
        .map_err(|e| format!("{folder}: {e}"))?;
        if let Some(unit) = stats.missing_units.first() {
            return Err(format!("{folder}: no model {unit}"));
        }
        units += stats.units;
        bytes += stats.bytes;
    }

    let folder = format!("umii/{name}");
    let parts: Vec<(String, String)> = villager
        .parts
        .iter()
        .map(|(_, m)| (folder.clone(), folder_unit(m).1.to_owned()))
        .collect();
    let skeletons: Vec<Vec<String>> = villager
        .parts
        .iter()
        .map(|(_, m)| bone_names(packs, model_files, m))
        .collect();
    let mut def = CharacterDef {
        parts,
        animations: umii.body_animations().into(),
        clips: vec![umii.wait_clip().into()],
        aliases: aliases(&skeletons),
        // Native 0x0343b104 uses the RT-only setter for the face head.
        joint_rt_copies: vec![("Head_Controled".into(), "Head".into())],
        joint_adjustments: feature_adjustments(
            &shared
                .iter()
                .find(|(name, _)| name == "UMii_Hylia_Face_Edit")
                .ok_or("UMii_Hylia_Face_Edit missing")?
                .1,
            umii,
        )?,
        idle: umii.wait_clip().into(),
        ..Default::default()
    };
    setup_nose(&mut def, villager, &own)?;
    asset_format::write_ron(&out.join(paths::character(&folder)), &def)
        .map_err(|e| e.to_string())?;
    Ok((units, bytes))
}

/// Normal UMii model constructor 0x034313bc attaches part 2 to Nose_U
/// (table 0x10554990, initialized at 0x03447280). Setup 0x03437e54
/// left-multiplies the nose root RT by Euler (-pi/2, -pi/2, 0) and
/// multiplies its scale by Head_Controled's local scale (face class W: 0.93).
fn setup_nose(
    def: &mut CharacterDef,
    villager: &Villager,
    own: &[(String, Vec<u8>)],
) -> Result<(), String> {
    let bone = |part, name: &str| {
        let model = villager
            .parts
            .iter()
            .find(|(p, _)| *p == part)
            .ok_or_else(|| format!("UMii part {part:?} missing"))?;
        let (folder, unit) = folder_unit(&model.1);
        let bytes = &own
            .iter()
            .find(|(f, _)| f == folder)
            .ok_or_else(|| format!("UMii folder {folder} missing"))?
            .1;
        let bfres = Bfres::parse(bytes).map_err(|e| e.to_string())?;
        let model = bfres
            .models()
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|m| m.name == unit)
            .ok_or_else(|| format!("UMii model {unit} missing"))?;
        model
            .bones
            .into_iter()
            .find(|b| b.name == name)
            .ok_or_else(|| format!("UMii bone {name} missing in {unit}"))
    };
    let root = bone(Part::Nose, "Nose_Root")?;
    let head = bone(Part::Face, "Head_Controled")?;
    // All source Hylian nose roots have identity rotation/zero translation.
    // The runtime's post-animation offsets are equivalent to the native
    // left product only for these roots; reject a different source layout.
    let identity_rotation = if root.euler {
        root.rotation[..3] == [0.0; 3]
    } else {
        root.rotation == [0.0, 0.0, 0.0, 1.0]
    };
    if !identity_rotation || root.translation != [0.0; 3] {
        return Err("UMii nose root is not an identity RT".into());
    }
    let rotation = crate::models::euler_rotation(
        -std::f32::consts::FRAC_PI_2,
        -std::f32::consts::FRAC_PI_2,
        0.0,
    );
    def.joint_offsets
        .push(("Nose_Root".into(), rotation.to_array()));
    def.joint_attachments
        .push(("Nose_Root".into(), "Nose_U".into()));
    let class_scale = if villager.umii.face_class() == 2 {
        0.93
    } else {
        1.0
    };
    // Preserve native multiplication order: first the face-class factor,
    // then the face head scale; runtime applies the resulting difference.
    let scale =
        std::array::from_fn(|i| (root.scale[i] * class_scale) * head.scale[i] - root.scale[i]);
    if scale.iter().any(|v| !v.is_finite()) {
        return Err("non-finite UMii nose root scale".into());
    }
    if scale != [0.0; 3] {
        def.joint_adjustments.push(JointAdjustment {
            joint: "Nose_Root".into(),
            scale,
            ..Default::default()
        });
    }
    Ok(())
}

/// Native 0x03436204 samples each feature twice and subtracts scale and
/// translation. Callers 0x034362d8 / 0x0343677c / 0x03436c34 add these differences to
/// the current local SRT; 0x03c700b0 adds translation without rotating it.
fn feature_adjustments(bytes: &[u8], umii: &Umii) -> Result<Vec<JointAdjustment>, String> {
    let bfres = Bfres::parse(bytes).map_err(|e| e.to_string())?;
    // Native table 0x10554e78; +0x6d4/6c4 and +0x730/720/740 of UMii.
    let face = ["B", "M", "W"][umii.face_class()];
    // Reference frames: v208 0x102c5f94/98/a8/ac.
    let tracks = [
        ("Nose_Scale", umii.nose.scale, 4.0),
        ("Nose_Trans_V", umii.nose.trans_v, 9.0),
        ("Mouth_Scale", umii.mouth.scale, 4.0),
        ("Mouth_Trans_V", umii.mouth.trans_v, 13.0),
        ("Mouth_Scale_V", umii.mouth.aspect, 3.0),
        // Native jaw caller 0x03436c34: selected integer at +0x40c,
        // reference 0.0 at 0x102c59a0, using the same difference helper.
        ("Jaw_Pattern", umii.jaw as f32, 0.0),
    ];
    let mut adjustments: BTreeMap<String, JointAdjustment> = BTreeMap::new();
    for (suffix, frame, reference) in tracks {
        if !frame.is_finite() {
            return Err(format!("non-finite UMii {suffix} frame"));
        }
        let name = format!("Face_{face}_{suffix}");
        let animation = bfres
            .skeletal_anim(&name)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("UMii feature animation {name} missing"))?;
        for bone in &animation.bones {
            let delta = feature_difference(bone, frame, reference);
            if delta.translation == [0.0; 3] && delta.scale == [0.0; 3] {
                continue;
            }
            if delta
                .translation
                .iter()
                .chain(&delta.scale)
                .any(|v| !v.is_finite())
            {
                return Err(format!(
                    "non-finite UMii feature difference {name}/{}",
                    bone.name
                ));
            }
            let entry = adjustments
                .entry(bone.name.clone())
                .or_insert_with(|| JointAdjustment {
                    joint: bone.name.clone(),
                    ..Default::default()
                });
            for i in 0..3 {
                entry.translation[i] += delta.translation[i];
                entry.scale[i] += delta.scale[i];
            }
        }
    }
    Ok(adjustments.into_values().collect())
}

fn feature_difference(
    bone: &botw_formats::bfres::anim::BoneAnim,
    frame: f32,
    reference: f32,
) -> JointAdjustment {
    let current = bone.sample(frame);
    let base = bone.sample(reference);
    let difference = |a: Option<[f32; 3]>, b: Option<[f32; 3]>| {
        match (a, b) {
            (Some(a), Some(b)) => std::array::from_fn(|i| a[i] - b[i]),
            // Absent channels keep the same bind component in both samples.
            _ => [0.0; 3],
        }
    };
    JointAdjustment {
        joint: bone.name.clone(),
        translation: difference(current.translation, base.translation),
        scale: difference(current.scale, base.scale),
        scale_override: None,
    }
}

/// The bone names of a model.
fn bone_names(packs: &ActorPacks, model_files: &[String], model: &str) -> Vec<String> {
    let (folder, unit) = folder_unit(model);
    let _ = model_files;
    let Some(bytes) = read_bfres(packs, &format!("Model/{folder}.sbfres")) else {
        return Vec::new();
    };
    let Ok(bfres) = Bfres::parse(&bytes) else {
        return Vec::new();
    };
    bfres
        .models()
        .unwrap_or_default()
        .into_iter()
        .find(|m| m.name == unit)
        .map(|m| m.bones.into_iter().map(|b| b.name).collect())
        .unwrap_or_default()
}

/// Shared joints for controlled/followed bones. Nose_D is an exact world
/// copy (0x0343ce24). SI-CHR-03: the native body-to-face helper 0x0343aa4c
/// instead copies local RT/scale; full part attachments and the remaining
/// controlled local copies are still needed. The face head uses separate RT copying.
fn aliases(skeletons: &[Vec<String>]) -> Vec<(String, String)> {
    let mut known: BTreeSet<&str> = BTreeSet::new();
    let mut aliases = Vec::new();
    for bones in skeletons {
        for bone in bones {
            // Head keeps its face-local scale; native 0x0343b104 copies only RT.
            if bone == "Head_Controled" {
                continue;
            }
            let target = bone
                .strip_suffix("_Controled")
                .filter(|t| known.contains(t))
                .or_else(|| {
                    FOLLOWS
                        .iter()
                        .find(|(b, j)| b == bone && known.contains(j))
                        .map(|(_, j)| *j)
                });
            if let Some(target) = target {
                aliases.push((bone.clone(), target.to_owned()));
            }
        }
        known.extend(bones.iter().map(String::as_str));
    }
    aliases
}

/// Sets the components a colour animation gave (NaN: left as it was).
fn set_param(material: &mut Material, name: &str, value: &[f32; 4]) {
    if let Some(param) = material.shader_params.iter_mut().find(|p| p.name == name) {
        for (slot, v) in param.values.iter_mut().zip(value) {
            if !v.is_nan() {
                *slot = *v;
            }
        }
    }
}

/// Points a material whose albedo is a colour recipe at a texture made up
/// for it (`<albedo>+<model>+<material>`, made by [`albedo_image`] from
/// the recipe kept in `recipes`).
fn tint(model: &str, material: &mut Material, recipes: &Mutex<HashMap<String, Material>>) {
    // SI-CHR-03: layered UMii faces need their distinct vertex UV sets and
    // texture SRTs. Do not evaluate this recovered recipe at one common UV.
    if (material.name == "Mt_Face" && material.sampler_texture("_a4").is_some())
        || crate::models::is_layered_lip(material)
    {
        return;
    }
    let Some(albedo) = material.sampler_texture("_a0").map(str::to_owned) else {
        return;
    };
    let Some(term) = albedo_term(material) else {
        return;
    };
    if term == Term::Slot(0, 0) {
        return;
    }
    let name = format!("{albedo}+{model}+{}", material.name);
    if let Ok(mut recipes) = recipes.lock() {
        recipes.insert(name.clone(), material.clone());
    }
    for (sampler, texture) in &mut material.textures {
        if sampler == "_a0" || sampler == "a0" {
            *texture = name.clone();
        }
    }
}

/// A colour input of `uking_mat`'s colour recipes (`uking_colorN_*`,
/// `uking_albedo_color`): a shader slot's texture and channel, a
/// `const_colorN`, a computed colour, or a constant.
#[derive(Clone, Debug, PartialEq)]
pub enum Term {
    /// Slot (0–7, `_a0 _s0 _n0 _e0 _t0 _a1 …`) and channel option (0 RGB,
    /// 10 R, 20 G, 30 B, 40 A).
    Slot(usize, i32),
    Constant([f32; 3]),
    /// `uking_colorN_calc_type` over its inputs A–D.
    Calc(i32, Vec<Term>),
    /// Component selection and inverse variants on computed RGB colours.
    Channel(Box<Term>, i32),
    Clamp(Box<Term>),
}

/// The material's albedo as a recipe of its inputs, `None` where it uses
/// what is not known (docs/research/umii.md: 4xx inputs, calc types).
pub fn albedo_term(material: &Material) -> Option<Term> {
    let value = material
        .shader_option("uking_albedo_color")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let channel = material
        .shader_option("uking_albedo_channel")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    input(material, value, channel, 0)
}

fn input(material: &Material, value: i32, channel: i32, depth: u32) -> Option<Term> {
    let inverse = channel % 10;
    if !matches!(inverse, 0 | 1) || !(0..=4).contains(&(channel / 10)) {
        return None;
    }
    let constant = |values: &[f32]| -> Option<Term> {
        let rgb = match channel / 10 {
            0 => [*values.first()?, *values.get(1)?, *values.get(2)?],
            c => [*values.get((c - 1) as usize)?; 3],
        };
        Some(Term::Constant(if inverse == 1 {
            rgb.map(|v| 1.0 - v)
        } else {
            rgb
        }))
    };
    match value {
        0..=7 => Some(Term::Slot(value as usize, channel)),
        100..=103 => constant(material.shader_param(&format!("const_color{}", value - 100))?),
        // PS 21d9f489394a0f84 (4881): these are scalar values, not colours.
        104..=111 => {
            let v = *material
                .shader_param(&format!("const_value{}", value - 104))?
                .first()?;
            Some(Term::Constant([if inverse == 1 { 1.0 - v } else { v }; 3]))
        }
        // PS e81702a641e3ed4f (11187): input 112 reads const_color4.
        // SI-EMI-01: the remaining 113..115 mapping is not independently verified.
        112..=115 => constant(material.shader_param(&format!("const_color{}", value - 108))?),
        // BFSHA has colour registers 0..13; reject cyclic/deeper recipes.
        200..=213 if depth < 14 => {
            if channel / 10 == 4 {
                return None;
            } // RGB recipes do not carry computed alpha.
            let term = computed(material, value - 200, depth + 1)?;
            Some(if channel == 0 {
                term
            } else {
                Term::Channel(Box::new(term), channel)
            })
        }
        // White in the body's PS c0efc5578962538f.
        402 => Some(Term::Constant([if inverse == 1 { 0.0 } else { 1.0 }; 3])),
        _ => None,
    }
}

fn computed(material: &Material, n: i32, depth: u32) -> Option<Term> {
    let option = |key: &str| -> Option<i32> {
        material
            .shader_option(&format!("uking_color{n}_{key}"))
            .and_then(|v| v.parse().ok())
    };
    if material.shader_option(&format!("uking_enable_calc_color{n}")) != Some("1") {
        return None;
    }
    let calc = option("calc_type").unwrap_or(0);
    let part = |p: &str| {
        input(
            material,
            option(p).unwrap_or(0),
            option(&format!("{p}_channel")).unwrap_or(0),
            depth,
        )
    };
    // Types 1, 17 and 27 are recovered from Wii U v208 uking_mat 11187,
    // byte-identical to Cemu PS e81702a641e3ed4f. See research/umii.md.
    let term = match calc {
        0 => part("A")?,
        1 | 2 => Term::Calc(calc, vec![part("A")?, part("B")?]),
        4 | 9 | 17 => Term::Calc(calc, vec![part("A")?, part("B")?, part("C")?]),
        11 | 27 => Term::Calc(calc, vec![part("A")?, part("B")?, part("C")?, part("D")?]),
        _ => return None,
    };
    Some(if option("clamp01") == Some(1) {
        Term::Clamp(Box::new(term))
    } else {
        term
    })
}

fn channel_rgb(rgb: [f32; 3], channel: i32) -> [f32; 3] {
    let picked = match channel / 10 {
        1..=3 => [rgb[(channel / 10 - 1) as usize]; 3],
        _ => rgb,
    };
    if channel % 10 == 1 {
        picked.map(|v| 1.0 - v)
    } else {
        picked
    }
}

/// Evaluates a term at one texel, `sample(slot)` giving linear RGBA.
fn evaluate(term: &Term, sample: &dyn Fn(usize) -> [f32; 4]) -> [f32; 3] {
    match term {
        Term::Constant(c) => *c,
        Term::Channel(term, channel) => channel_rgb(evaluate(term, sample), *channel),
        Term::Clamp(term) => evaluate(term, sample).map(|v| v.clamp(0.0, 1.0)),
        Term::Slot(slot, channel) => {
            let t = sample(*slot);
            if channel / 10 == 4 {
                [if channel % 10 == 1 { 1.0 - t[3] } else { t[3] }; 3]
            } else {
                channel_rgb([t[0], t[1], t[2]], *channel)
            }
        }
        Term::Calc(calc, inputs) => {
            let v: Vec<[f32; 3]> = inputs.iter().map(|t| evaluate(t, sample)).collect();
            std::array::from_fn(|c| match calc {
                1 | 17 => v.iter().map(|x| x[c]).sum(),
                4 => v[0][c] + (v[1][c] - v[0][c]) * v[2][c],
                27 => {
                    let a = v[0][c] * v[3][c];
                    a + (v[1][c] - a) * v[2][c]
                }
                _ => v.iter().map(|x| x[c]).product(),
            })
        }
    }
}

fn srgb_to_linear(v: u8) -> f32 {
    let c = f32::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// The albedo the material's recipe makes, at the size of its albedo
/// texture, with a mip chain (SI-FMT-10's box filter), alpha the albedo's.
fn albedo_image(material: &Material, find: FindTexture) -> Option<TextureImage> {
    let term = albedo_term(material)?;
    let albedo_name = material.sampler_texture("_a0")?;
    let albedo = find(albedo_name)?;
    let (w, h) = (albedo.width as usize, albedo.height as usize);
    // Each slot's texture decoded once: (width, height, sRGB, RGBA8 with
    // the texture's component selection).
    let mut slots: Vec<Option<(usize, usize, bool, Vec<u8>)>> = vec![None; 8];
    for (slot, decoded) in slots.iter_mut().enumerate() {
        let Some((_, name)) = material.slot_texture(slot) else {
            continue;
        };
        let Some(image) = find(name) else { continue };
        let Some(rgba) = image.decode_rgba8(0) else {
            continue;
        };
        let select = image.component_select;
        let rgba: Vec<u8> = rgba
            .chunks(4)
            .flat_map(|p| {
                select.map(|s| match s {
                    0..=3 => p[s as usize],
                    4 => 0,
                    _ => 255,
                })
            })
            .collect();
        let srgb = matches!(
            image.format,
            gx2::Format::Bc1 { srgb: true }
                | gx2::Format::Bc2 { srgb: true }
                | gx2::Format::Bc3 { srgb: true }
                | gx2::Format::Rgba8 { srgb: true }
        );
        *decoded = Some((image.width as usize, image.height as usize, srgb, rgba));
    }
    let base = slots[0].clone()?;
    let mut level0 = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let sample = |slot: usize| -> [f32; 4] {
                let Some((sw, sh, srgb, data)) = &slots[slot] else {
                    return [1.0; 4];
                };
                // The same UV set: the nearest texel of a smaller texture.
                let (sx, sy) = (x * sw / w, y * sh / h);
                let p = &data[(sy * sw + sx) * 4..][..4];
                let c = |v: u8| {
                    if *srgb {
                        srgb_to_linear(v)
                    } else {
                        f32::from(v) / 255.0
                    }
                };
                [c(p[0]), c(p[1]), c(p[2]), f32::from(p[3]) / 255.0]
            };
            let rgb = evaluate(&term, &sample);
            let alpha = base.3[(y * w + x) * 4 + 3];
            level0.extend_from_slice(&[
                linear_to_srgb(rgb[0]),
                linear_to_srgb(rgb[1]),
                linear_to_srgb(rgb[2]),
                alpha,
            ]);
        }
    }
    // SI-FMT-10: texture mips and BC conversions are ours.
    let (mut level, mut lw, mut lh) = (level0, w as u32, h as u32);
    let mut data = level.clone();
    let mut mip_levels = 1;
    while lw > 1 || lh > 1 {
        (level, lw, lh) = bc::downsample(&level, lw, lh);
        data.extend_from_slice(&level);
        mip_levels += 1;
    }
    Some(TextureImage {
        name: albedo_name.to_owned(),
        format: gx2::Format::Rgba8 { srgb: true },
        width: w as u32,
        height: h as u32,
        layers: 1,
        mip_levels,
        data,
        component_select: [0, 1, 2, 3],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use botw_formats::bfres::model::ShaderParam;

    fn body_material() -> Material {
        let pairs = |list: &[(&str, &str)]| {
            list.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        Material {
            name: "Mt_Body".into(),
            textures: pairs(&[("_a0", "Alb"), ("_cm0", "Msk0")]),
            render_info: Vec::new(),
            shader_archive: "uking_mat".into(),
            shading_model: "uking_mat".into(),
            shader_options: pairs(&[
                ("uking_enable_calc_color0", "1"),
                ("uking_color0_calc_type", "4"),
                ("uking_color0_A", "402"),
                ("uking_color0_B", "101"),
                ("uking_color0_C", "3"),
                ("uking_color0_D", "402"),
                ("uking_enable_calc_color3", "1"),
                ("uking_color3_calc_type", "2"),
                ("uking_color3_A", "200"),
                ("uking_color3_B", "300"),
                ("uking_albedo_color", "203"),
            ]),
            sampler_assign: pairs(&[("_a0", "_a0"), ("_e0", "_cm0")]),
            samplers: Vec::new(),
            render_state: Default::default(),
            shader_params: vec![ShaderParam {
                name: "const_color1".into(),
                kind: 15,
                values: vec![0.0, 0.52, 0.0, 1.0],
            }],
        }
    }

    #[test]
    fn an_unknown_input_leaves_the_recipe_unknown() {
        let mut material = body_material();
        let (_, value) = material
            .shader_options
            .iter_mut()
            .find(|(k, _)| k == "uking_color3_B")
            .unwrap();
        *value = "599".into();
        assert_eq!(albedo_term(&material), None);
    }

    #[test]
    fn colour_masks_blend_white_to_the_colour() {
        let mut material = body_material();
        for (k, v) in &mut material.shader_options {
            if k == "uking_color3_B" {
                *v = "0".into();
            }
        }
        set_param(
            &mut material,
            "const_color1",
            &[0.5, f32::NAN, 0.25, f32::NAN],
        );
        let term = albedo_term(&material).unwrap();
        // Albedo 0.8 grey, mask 0.5: 0.8 · mix(1, c, 0.5) per channel.
        let rgb = evaluate(&term, &|slot| match slot {
            0 => [0.8; 4],
            3 => [0.5; 4],
            _ => [1.0; 4],
        });
        let expect = |c: f32| 0.8 * (1.0 + (c - 1.0) * 0.5);
        for (got, want) in rgb.iter().zip([expect(0.5), expect(0.52), expect(0.25)]) {
            assert!((got - want).abs() < 1e-6, "{rgb:?}");
        }
    }

    /// Static graph of Wii U uking_mat 11187, with synthetic colours.
    fn face_material() -> Material {
        let mut m = body_material();
        m.shader_options = vec![("uking_albedo_color".into(), "207".into())];
        let graph = [
            (17, [(4, 40), (3, 40), (4, 21), (400, 0)]),
            (2, [(3, 20), (5, 40), (400, 0), (400, 0)]),
            (1, [(3, 0), (4, 21), (400, 0), (400, 0)]),
            (4, [(103, 0), (402, 10), (202, 0), (400, 0)]),
            (9, [(101, 0), (0, 0), (203, 0), (400, 0)]),
            (27, [(5, 0), (204, 0), (201, 11), (100, 0)]),
            (27, [(112, 0), (205, 0), (4, 41), (4, 10)]),
            (2, [(206, 0), (1, 10), (4, 41), (400, 0)]),
        ];
        for (n, (calc, inputs)) in graph.iter().enumerate() {
            m.shader_options.extend([
                (format!("uking_enable_calc_color{n}"), "1".into()),
                (format!("uking_color{n}_calc_type"), calc.to_string()),
                (format!("uking_color{n}_clamp01"), "1".into()),
            ]);
            for (p, (value, channel)) in ["A", "B", "C", "D"].iter().zip(inputs) {
                m.shader_options.extend([
                    (format!("uking_color{n}_{p}"), value.to_string()),
                    (format!("uking_color{n}_{p}_channel"), channel.to_string()),
                ]);
            }
        }
        m.shader_params = [
            ("const_color0", vec![0.2, 0.3, 0.4, 1.0]),
            ("const_color1", vec![0.5, 0.3, 0.2, 1.0]),
            ("const_color3", vec![0.25, 0.5, 0.75, 1.0]),
            ("const_color4", vec![0.1, 0.2, 0.3, 0.8]),
            ("const_value0", vec![1.5]),
        ]
        .into_iter()
        .map(|(name, values)| ShaderParam {
            name: name.into(),
            kind: 15,
            values,
        })
        .collect();
        m
    }

    #[test]
    fn face_layers_preserve_skin_and_cover_it_with_feature_colours() {
        let m = face_material();
        let term = albedo_term(&m).expect("the eight-register face graph is supported");
        let pixels = [
            [0.8; 4],
            [1.0; 4],
            [1.0; 4],
            [1.0, 1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0, 0.0],
            [0.5, 0.2, 0.1, 0.0],
        ];
        let check = |pixels: [[f32; 4]; 6], expected: [f32; 3]| {
            let actual = evaluate(&term, &|i| pixels[i]);
            for (a, b) in actual.into_iter().zip(expected) {
                assert!((a - b).abs() < 1e-6, "{actual:?} != {expected:?}");
            }
        };
        check(pixels, [0.4, 0.24, 0.16]); // No feature coverage: skin.
        let mut makeup = pixels;
        makeup[5][3] = 1.0;
        check(makeup, [0.1, 0.06, 0.04]); // Full makeup coverage.
        let mut brow = makeup;
        brow[4] = [0.25, 1.0, 0.0, 1.0];
        brow[1][0] = 0.5;
        check(brow, [0.0125, 0.025, 0.0375]); // Brow over makeup, line mask last.
        let alpha = input(&m, 200, 0, 0).unwrap();
        let mut covered = pixels;
        covered[3][3] = 0.3;
        covered[4] = [0.25, 0.8, 0.0, 0.4];
        let partial = evaluate(&alpha, &|i| covered[i]);
        assert!((partial[0] - 0.9).abs() < 1e-6);
        covered[4][1] = 0.2;
        assert_eq!(evaluate(&alpha, &|i| covered[i]), [1.0; 3]);
    }

    #[test]
    fn scalar_inputs_and_constant_alpha_do_not_alias_colour_slots() {
        let m = face_material();
        let no_texture = |_: usize| [0.0; 4];
        assert_eq!(
            evaluate(&input(&m, 104, 0, 0).unwrap(), &no_texture),
            [1.5; 3]
        );
        assert_eq!(
            evaluate(&input(&m, 104, 1, 0).unwrap(), &no_texture),
            [-0.5; 3]
        );
        assert_eq!(
            evaluate(&input(&m, 112, 40, 0).unwrap(), &no_texture),
            [0.8; 3]
        );
        assert_eq!(
            evaluate(&input(&m, 112, 41, 0).unwrap(), &no_texture),
            [1.0 - 0.8; 3]
        );
    }

    #[test]
    fn cyclic_colour_graphs_are_rejected() {
        let mut m = face_material();
        let (_, value) = m
            .shader_options
            .iter_mut()
            .find(|(k, _)| k == "uking_color2_A")
            .unwrap();
        *value = "207".into();
        assert_eq!(albedo_term(&m), None);
    }

    #[test]
    fn feature_differences_use_authored_reference_frames_and_additive_scale() {
        use botw_formats::bfres::anim::{BoneAnim, Curve, CurveKind, Target};
        let bone = BoneAnim {
            name: "Nose".into(),
            flags: 0,
            base_scale: Some([2.0, 3.0, 4.0]),
            base_translation: Some([10.0, 20.0, 30.0]),
            base_rotation: None,
            curves: vec![
                Curve {
                    target: Target::Scale(0),
                    kind: CurveKind::Linear,
                    frames: vec![0.0, 4.0, 8.0],
                    keys: vec![
                        [1.0, 1.0, 0.0, 0.0],
                        [2.0, 2.0, 0.0, 0.0],
                        [4.0, 0.0, 0.0, 0.0],
                    ],
                },
                Curve {
                    target: Target::Translation(1),
                    kind: CurveKind::Linear,
                    frames: vec![0.0, 4.0, 8.0],
                    keys: vec![
                        [10.0, 10.0, 0.0, 0.0],
                        [20.0, 8.0, 0.0, 0.0],
                        [28.0, 0.0, 0.0, 0.0],
                    ],
                },
            ],
        };
        let delta = feature_difference(&bone, 6.0, 4.0);
        assert_eq!(delta.scale, [1.0, 0.0, 0.0]);
        assert_eq!(delta.translation, [0.0, 4.0, 0.0]);
        let neutral = feature_difference(&bone, 4.0, 4.0);
        assert_eq!(neutral.scale, [0.0; 3]);
        assert_eq!(neutral.translation, [0.0; 3]);
    }

    #[test]
    fn controlled_bones_are_the_joints_they_copy() {
        let body = vec!["Root".to_owned(), "Neck".into(), "Head".into()];
        let face = vec![
            "Neck_Root".to_owned(),
            "Neck_Controled".into(),
            "Head_Controled".into(),
            "Chin".into(),
        ];
        let hair = vec!["Hair_Root".to_owned(), "Hair_Head".into()];
        let beard = vec!["Beard_Root".to_owned(), "Chin_Controled".into()];
        let aliases = aliases(&[body, face, hair, beard]);
        let to = |b: &str| {
            aliases
                .iter()
                .find(|(x, _)| x == b)
                .map(|(_, j)| j.as_str())
        };
        assert_eq!(to("Head_Controled"), None);
        assert_eq!(to("Hair_Head"), Some("Head"));
        assert_eq!(to("Hair_Root"), Some("Neck"));
        assert_eq!(to("Chin_Controled"), Some("Chin"));
        assert_eq!(to("Neck_Root"), None);
    }
}
