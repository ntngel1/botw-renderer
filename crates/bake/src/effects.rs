//! `effects/`: the effect files something in the baked world plays. Reads
//! what the `objects` step wrote (the cells in reach).
//!
//! Who plays what: an actor pack's `ActorLink` names its ELink user
//! (`ptcl::files::elink_user`, the actor's own name without one); the
//! ELink database says which emitter sets the user plays all the time
//! (`ptcl::files::always_effects`); the sets live in the effect file named
//! after the user. Kept: the actors of the baked cells, and the map's
//! effect-only actors (no model) everywhere, as the original renderer's
//! `effects::load` gathers its cloud caps from every cell. Weather sets
//! are added by name (`WEATHER_FILES`). The resident file
//! (`GameResident`, textures and primitives shared by all) always.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use asset_format::effects::{self as fx, EffectActor, EffectFile, EffectIndex, PlayedSet};
use asset_format::objects::{ObjectIndex, PlacedActors, all_cells};
use asset_format::paths;
use botw_formats::actor::{ActorPacks, model_list};
use botw_formats::content::ContentRoots;
use botw_formats::ptcl::files::{self, Bootup};
use botw_formats::ptcl::{Emitter, Node, Ptcl, Reader};
use botw_formats::xlink::XLink;

use crate::eft;
use crate::objects::{parallel, read_cell};

#[path = "eft_tables.rs"]
pub(crate) mod eft_tables;

/// Effect files the weather plays (not through an actor's ELink).
const WEATHER_FILES: &[&str] = crate::elink::EFFECT_FILES;

/// What an actor pack says about effects.
struct ActorEffects {
    user: String,
    has_models: bool,
    played: Vec<files::AlwaysEffect>,
}

pub fn bake(roots: &ContentRoots, out: &Path) -> Result<(), String> {
    let started = std::time::Instant::now();
    let index: ObjectIndex = asset_format::read_ron(&out.join(paths::OBJECT_INDEX))
        .map_err(|e| format!("effects need the objects step first: {e}"))?;
    let title_bg = roots
        .find("Pack/TitleBG.pack")
        .map(|p| std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display())))
        .transpose()?
        .map(Arc::new);
    let packs = ActorPacks::new(roots.clone(), title_bg.clone());
    let bootup = Bootup::read(roots)
        .map_err(|e| e.to_string())?
        .ok_or("Pack/Bootup.pack not found")?;
    let elink = bootup
        .entry(files::ELINK_DB)
        .map_err(|e| e.to_string())?
        .ok_or("ELink database not found")?;
    let elink = XLink::parse(&elink).map_err(|e| format!("ELink database: {e}"))?;
    let resident = bootup
        .entry(files::RESIDENT_FILE)
        .map_err(|e| e.to_string())?
        .ok_or("GameResident effect file not found")?;

    // Every actor on the map: the baked cells' in full, the rest for
    // effect-only actors.
    let cells: Vec<String> = all_cells().collect();
    let units = parallel(&cells, |cell| read_cell(roots, title_bg.as_deref(), cell));
    let mut map_actors = Vec::new();
    for (cell, actors) in cells.iter().zip(units) {
        map_actors.extend(actors.map_err(|e| format!("cell {cell}: {e}"))?);
    }
    let mut in_reach = BTreeSet::new();
    for cell in &index.cells {
        let placed: PlacedActors = asset_format::read_ron(&out.join(paths::object_cell(cell)))
            .map_err(|e| e.to_string())?;
        in_reach.extend(placed.names);
    }
    let names: Vec<String> = map_actors
        .iter()
        .map(|a| a.name.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let looked_up = parallel(&names, |name| actor_effects(&packs, &elink, name));
    let mut actors = BTreeMap::new();
    for (name, effects) in names.iter().zip(looked_up) {
        let effects = effects.map_err(|e| format!("actor {name}: {e}"))?;
        if !effects.played.is_empty() {
            actors.insert(name.clone(), effects);
        }
    }

    let mut out_index = EffectIndex::default();
    let mut wanted: BTreeSet<String> = WEATHER_FILES.iter().map(|s| s.to_string()).collect();
    for (name, effects) in &actors {
        let effect_only = !effects.has_models;
        if !in_reach.contains(name) && !effect_only {
            continue;
        }
        wanted.insert(effects.user.clone());
        out_index.actors.insert(
            name.clone(),
            effects
                .played
                .iter()
                .map(|p| PlayedSet {
                    file: effects.user.clone(),
                    set: p.set.clone(),
                    scale: p.scale,
                    offset: p.offset,
                    color: p.color,
                })
                .collect(),
        );
    }
    for actor in &map_actors {
        if actors.get(&actor.name).is_some_and(|e| !e.has_models) {
            out_index.effect_actors.push(EffectActor {
                name: actor.name.clone(),
                hash_id: actor.hash_id,
                translate: actor.translate,
                rotate: actor.rotate,
                scale: actor.scale,
            });
        }
    }

    let dir = out.join(paths::EFFECT_FILES);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut files_bytes = vec![(fx::RESIDENT.to_owned(), resident)];
    for user in &wanted {
        match files::read_effect_file(roots, user).map_err(|e| e.to_string())? {
            Some(bytes) => files_bytes.push((user.clone(), bytes)),
            None => eprintln!("warning: effect file {user} not found"),
        }
    }
    let written = parallel(&files_bytes, |(name, bytes)| write_file(name, bytes, out));
    let mut emitters = 0;
    for ((name, _), result) in files_bytes.iter().zip(written) {
        emitters += result.map_err(|e| format!("effect file {name}: {e}"))?;
        out_index.files.push(name.clone());
    }
    // Played sets whose file was not found play nothing.
    let present: BTreeSet<&String> = out_index.files.iter().collect();
    for sets in out_index.actors.values_mut() {
        sets.retain(|s| present.contains(&s.file));
    }
    out_index.actors.retain(|_, sets| !sets.is_empty());
    let actors_left: BTreeSet<&String> = out_index.actors.keys().collect();
    out_index
        .effect_actors
        .retain(|a| actors_left.contains(&a.name));
    crate::sky::write_ron::<_, EffectIndex>(&out.join(paths::EFFECT_INDEX), &out_index)?;
    println!(
        "effects: {} files ({} emitters), {} actors play sets, {} effect-only placed ({:.1} s)",
        out_index.files.len(),
        emitters,
        out_index.actors.len(),
        out_index.effect_actors.len(),
        started.elapsed().as_secs_f32()
    );
    eft_tables::bake(roots, out)
}

fn actor_effects(packs: &ActorPacks, elink: &XLink, name: &str) -> Result<ActorEffects, String> {
    let pack = packs.open(name).map_err(|e| e.to_string())?;
    let user = pack
        .as_ref()
        .and_then(|p| p.find("Actor/ActorLink/"))
        .map(|link| files::elink_user(&link))
        .transpose()
        .map_err(|e| e.to_string())?
        .flatten()
        .unwrap_or_else(|| name.to_owned());
    let has_models = match pack.as_ref().and_then(|p| p.find("Actor/ModelList/")) {
        Some(list) => !model_list(&list).map_err(|e| e.to_string())?.is_empty(),
        None => false,
    };
    let played =
        files::always_effects(elink, &user).map_err(|e| format!("ELink user {user}: {e}"))?;
    Ok(ActorEffects {
        user,
        has_models,
        played,
    })
}

/// Writes one effect file; returns its emitter count.
fn write_file(name: &str, bytes: &[u8], out: &Path) -> Result<usize, String> {
    let ptcl = Ptcl::parse(bytes).map_err(|e| e.to_string())?;
    let r = Reader(bytes);
    let programs = shader_programs(&r, bytes);
    let mut res = Vec::new();
    let mut count = 0;
    let mut file = EffectFile {
        name: name.to_owned(),
        ..Default::default()
    };
    for set in &ptcl.emitter_sets {
        let emitters = set
            .emitters
            .iter()
            .map(|e| emitter(&r, &ptcl, &programs, e, &mut res, &mut count))
            .collect::<Result<_, _>>()?;
        file.sets.push(fx::EmitterSet {
            name: set.name.clone(),
            emitters,
        });
    }
    for texture in &ptcl.textures {
        let image = texture
            .image()
            .map_err(|e| format!("texture {:08x}: {e}", texture.id))?;
        crate::sky::write_texture(&image, &out.join(fx::texture_path(name, texture.id)))?;
        file.textures.push(texture.id);
    }
    file.primitives = ptcl
        .primitives
        .iter()
        .map(|p| fx::Primitive {
            id: p.id,
            positions: p.positions.clone(),
            normals: p.normals.clone(),
            tangents: p.tangents.clone(),
            colors: p.colors.clone(),
            uvs: p.uvs.clone(),
            indices: p.indices.clone(),
        })
        .collect();
    asset_format::write(&out.join(fx::resource_path(name)), &res).map_err(|e| e.to_string())?;
    crate::sky::write_ron::<_, EffectFile>(&out.join(fx::file_path(name)), &file)?;
    Ok(count)
}

/// Appends `bytes` to the resource blob.
fn block(res: &mut Vec<u8>, bytes: &[u8]) -> fx::Block {
    let block = fx::Block {
        offset: res.len() as u32,
        size: bytes.len() as u32,
    };
    res.extend_from_slice(bytes);
    block
}

fn emitter(
    r: &Reader,
    ptcl: &Ptcl,
    programs: &(Vec<String>, Vec<String>),
    e: &Emitter,
    res: &mut Vec<u8>,
    count: &mut usize,
) -> Result<fx::Emitter, String> {
    *count += 1;
    let data = r
        .slice(e.data_offset, eft::SIZE)
        .map_err(|e| e.to_string())?;
    let resource = block(res, data);
    // The attribute nodes hang off the emitter node; find it again from the
    // data offset.
    let mut raw_attributes: Vec<(String, &[u8])> = Vec::new();
    if let Some(node) = emitter_node(r, e.data_offset) {
        for attribute in Node::siblings(r, node.attribute).map_err(|e| e.to_string())? {
            let bytes = r
                .slice(
                    attribute.data,
                    attribute.size as usize - (attribute.data - attribute.offset),
                )
                .map_err(|e| e.to_string())?;
            raw_attributes.push((attribute.magic_str(), bytes));
        }
    }
    let attributes = raw_attributes
        .iter()
        .map(|(magic, bytes)| (magic.clone(), block(res, bytes)))
        .collect();
    let animations = raw_attributes
        .iter()
        .filter(|(magic, _)| magic.starts_with("EA"))
        .filter_map(|(magic, bytes)| Some((magic.clone(), eft::emitter_anim(bytes)?)))
        .collect();
    let fields = eft::Fields::from_attributes(&raw_attributes);
    let raw = eft::Res(data);
    let vertices = (raw.u64(0x878) != u64::MAX)
        .then(|| ptcl.primitive(raw.u64(0x878)))
        .flatten()
        .map(|p| p.positions.len() as u32);
    let patched = eft::setup(data, &fields, vertices);
    let static_block = block(res, &eft::static_block(&patched));
    let field_block = eft::field_block(&fields).map(|b| block(res, &b));
    let children = e
        .children
        .iter()
        .map(|c| emitter(r, ptcl, programs, c, res, count))
        .collect::<Result<_, _>>()?;
    let samplers = [0, 1, 2].map(|s| {
        let at = 0x9f8 + 0x20 * s;
        let id = raw.u64(at);
        (id != u64::MAX).then(|| fx::Sampler {
            texture: id as u32,
            resident: raw.flag(at + 0x18),
            wrap: [raw.u8(at + 8), raw.u8(at + 9)],
            filter: raw.u8(at + 10),
            max_lod: raw.f32(at + 0xc),
            lod_bias: raw.f32(at + 0x10),
        })
    });
    Ok(fx::Emitter {
        name: e.name.clone(),
        resource,
        static_block,
        field_block,
        attributes,
        animations,
        children,
        params: {
            let mut params = eft::emitter_params(&eft::Res(&patched));
            let pick = |list: &Vec<String>, at: usize| {
                list.get(raw.u32(at) as usize).cloned().unwrap_or_default()
            };
            params.vertex_program = pick(&programs.0, 0x914);
            params.pixel_program = pick(&programs.1, 0x918);
            for (magic, bytes) in &raw_attributes {
                match magic.as_str() {
                    "EP04" => params.area_loop = eft::area_loop(bytes),
                    "CSDP" => params.custom_params = eft::custom_params(bytes),
                    _ => {}
                }
            }
            params
        },
        samplers,
    })
}

/// The `EMTR` node whose data starts at `data`: walk the tree.
fn emitter_node(r: &Reader, data: usize) -> Option<Node> {
    fn walk(r: &Reader, nodes: Vec<Node>, data: usize) -> Option<Node> {
        for n in nodes {
            if &n.magic == b"EMTR" && n.data == data {
                return Some(n);
            }
            if let Ok(children) = n.children(r)
                && let Some(found) = walk(r, children, data)
            {
                return Some(found);
            }
        }
        None
    }
    let top = Node::siblings(r, Some(0x30)).ok()?;
    let esta = top.into_iter().find(|n| &n.magic == b"ESTA")?;
    walk(r, esta.children(r).ok()?, data)
}

/// The file's shader programs' hashes (`SHDA` → `SHDB`'s GFX2 file).
fn shader_programs(r: &Reader, bytes: &[u8]) -> (Vec<String>, Vec<String>) {
    let Ok(top) = Node::siblings(r, Some(0x30)) else {
        return Default::default();
    };
    for node in top.iter().filter(|n| &n.magic == b"SHDA") {
        let Ok(children) = node.children(r) else {
            continue;
        };
        for shdb in children.iter().filter(|n| &n.magic == b"SHDB") {
            let end = (shdb.data + shdb.size as usize).min(bytes.len());
            if let Some(gfx2) = bytes.get(shdb.data..end) {
                return eft::program_hashes(gfx2);
            }
        }
    }
    Default::default()
}
