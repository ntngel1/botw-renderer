//! The baked effect files (`assets/effects`), read in the background on
//! first use and kept: the decoded emitters, their resource blocks and
//! textures (as GPU images once read).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use asset_format::effects::{self as fx, EffectFile, EffectIndex};
use asset_format::paths;
use asset_format::texture::Texture;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

/// An effect file as read from `assets/`.
pub struct EffectData {
    pub file: EffectFile,
    /// The emitters' resource and attribute blocks (`fx::Block` ranges).
    pub res: Vec<u8>,
    /// GPU images by texture id, filled on the main thread.
    pub images: HashMap<u32, Handle<Image>>,
    /// Each texture's component selection (GX2 `compSel`: source of R, G,
    /// B, A; 0–3 a channel, 4 zero, 5 one).
    pub swizzles: HashMap<u32, [u8; 4]>,
}

impl EffectData {
    pub fn block(&self, block: fx::Block) -> &[u8] {
        block.get(&self.res).unwrap_or_default()
    }
}

struct ReadFile {
    file: EffectFile,
    res: Vec<u8>,
    textures: Vec<(u32, Texture)>,
}

enum Slot {
    Loading(Task<Result<ReadFile, String>>),
    Ready(Arc<EffectData>),
    Failed,
}

#[derive(Resource)]
pub struct EffectLibrary {
    assets: PathBuf,
    index: Option<Arc<EffectIndex>>,
    index_task: Option<Task<Option<EffectIndex>>>,
    files: HashMap<String, Slot>,
}

impl EffectLibrary {
    pub fn new(assets: PathBuf) -> Self {
        let path = assets.join(paths::EFFECT_INDEX);
        let index_task = path.exists().then(|| {
            AsyncComputeTaskPool::get().spawn(async move {
                asset_format::read_ron::<EffectIndex>(&path)
                    .inspect_err(|e| warn!("effects: {e}"))
                    .ok()
            })
        });
        if index_task.is_none() {
            warn!("no effects; run `cargo bake --only effects`");
        }
        Self {
            assets,
            index: None,
            index_task,
            files: HashMap::new(),
        }
    }

    pub fn index(&self) -> Option<&Arc<EffectIndex>> {
        self.index.as_ref()
    }

    /// Whether the index or a requested file is still being read.
    pub fn is_loading(&self) -> bool {
        self.index_task.is_some() || self.files.values().any(|s| matches!(s, Slot::Loading(_)))
    }

    /// The file, if read; starts reading it otherwise (when it exists).
    pub fn get(&mut self, name: &str) -> Option<Arc<EffectData>> {
        match self.files.get(name) {
            Some(Slot::Ready(data)) => return Some(data.clone()),
            Some(_) => return None,
            None => {}
        }
        let listed = self
            .index
            .as_ref()
            .is_some_and(|i| i.files.iter().any(|f| f == name));
        if !listed {
            self.files.insert(name.to_owned(), Slot::Failed);
            return None;
        }
        let assets = self.assets.clone();
        let file = name.to_owned();
        let task = AsyncComputeTaskPool::get().spawn(async move { read(&assets, &file) });
        self.files.insert(name.to_owned(), Slot::Loading(task));
        None
    }

    /// The resident file (shared textures and primitives), if read.
    pub fn resident(&mut self) -> Option<Arc<EffectData>> {
        self.get(fx::RESIDENT)
    }
}

fn read(assets: &std::path::Path, name: &str) -> Result<ReadFile, String> {
    let file: EffectFile =
        asset_format::read_ron(&assets.join(fx::file_path(name))).map_err(|e| e.to_string())?;
    let res =
        asset_format::read(&assets.join(fx::resource_path(name))).map_err(|e| e.to_string())?;
    let textures = file
        .textures
        .iter()
        .filter_map(|&id| {
            Texture::read(&assets.join(fx::texture_path(name, id)))
                .inspect_err(|e| warn!("effect {name}: texture {id:08x}: {e}"))
                .ok()
                .map(|t| (id, t))
        })
        .collect();
    Ok(ReadFile {
        file,
        res,
        textures,
    })
}

pub fn finish_reading(mut library: ResMut<EffectLibrary>, mut images: ResMut<Assets<Image>>) {
    if let Some(task) = &mut library.index_task
        && let Some(index) = block_on(poll_once(task))
    {
        library.index_task = None;
        library.index = index.map(Arc::new);
        if let Some(index) = &library.index {
            info!(
                "effects: {} files, {} actors play sets, {} effect actors",
                index.files.len(),
                index.actors.len(),
                index.effect_actors.len()
            );
        }
    }
    for (name, slot) in library.files.iter_mut() {
        let Slot::Loading(task) = slot else { continue };
        let Some(result) = block_on(poll_once(task)) else {
            continue;
        };
        *slot = match result {
            Ok(read) => {
                let images = read
                    .textures
                    .iter()
                    .filter_map(|(id, texture)| {
                        let mut image = crate::texture::gpu_image(texture, true)?;
                        // Kept in the main world too: emitters copy it with
                        // their own samplers.
                        image.asset_usage = bevy::asset::RenderAssetUsages::default();
                        Some((*id, images.add(image)))
                    })
                    .collect();
                let swizzles = read
                    .textures
                    .iter()
                    .map(|(id, t)| (*id, t.swizzle))
                    .collect();
                Slot::Ready(Arc::new(EffectData {
                    file: read.file,
                    res: read.res,
                    images,
                    swizzles,
                }))
            }
            Err(error) => {
                warn!("effect file {name}: {error}");
                Slot::Failed
            }
        };
    }
}
