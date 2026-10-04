//! Actors: which model an actor shows. Each actor has a pack
//! (`Actor/Pack/<name>.sbactorpack`, some inside `Pack/TitleBG.pack`)
//! holding `Actor/ModelList/*.bmodellist`, an AAMP document naming the
//! BFRES file (`Folder`, found at `Model/<Folder>.sbfres`) and the models
//! in it (`UnitName`).

use std::sync::Arc;

use roead::aamp::{Parameter, ParameterIO, ParameterList};

use crate::content::ContentRoots;
use crate::{FormatError, Result};

/// A model an actor draws: BFRES file stem and model names in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRef {
    pub folder: String,
    pub units: Vec<String>,
}

/// Reads the model references from a `.bmodellist`.
pub fn model_list(bytes: &[u8]) -> Result<Vec<ModelRef>> {
    let io = ParameterIO::from_binary(bytes)
        .map_err(|_| FormatError::Invalid("bmodellist: not AAMP"))?;
    let Some(data) = io.param_root.lists.get("ModelData") else {
        return Ok(Vec::new());
    };
    let mut refs = Vec::new();
    for i in 0.. {
        let Some(entry) = data.lists.get(format!("ModelData_{i}").as_str()) else {
            break;
        };
        let folder = entry
            .objects
            .get("Base")
            .and_then(|base| base.get("Folder"))
            .and_then(text)
            .unwrap_or_default();
        let units = entry.lists.get("Unit").map(unit_names).unwrap_or_default();
        if !folder.is_empty() {
            refs.push(ModelRef { folder, units });
        }
    }
    Ok(refs)
}

fn unit_names(list: &ParameterList) -> Vec<String> {
    (0..)
        .map_while(|i| list.objects.get(format!("Unit_{i}").as_str()))
        .filter_map(|unit| unit.get("UnitName").and_then(text))
        .collect()
}

fn text(parameter: &Parameter) -> Option<String> {
    parameter.as_str().ok().map(str::to_owned)
}

/// Finds actor packs across the content roots and `TitleBG.pack`.
#[derive(Clone)]
pub struct ActorPacks {
    roots: ContentRoots,
    title_bg: Option<Arc<Vec<u8>>>,
}

impl ActorPacks {
    pub fn new(roots: ContentRoots, title_bg: Option<Arc<Vec<u8>>>) -> Self {
        Self { roots, title_bg }
    }

    /// The content roots the packs are found in.
    pub fn roots(&self) -> &ContentRoots {
        &self.roots
    }

    /// The decompressed SARC of an actor's pack, if it exists.
    pub fn pack(&self, actor: &str) -> Result<Option<Vec<u8>>> {
        self.file(&format!("Actor/Pack/{actor}.sbactorpack"))
    }

    /// A game file, decompressed: loose in the content roots, or inside
    /// `TitleBG.pack` (which holds what the game always keeps loaded, such
    /// as Link's model and animations).
    pub fn file(&self, relative: &str) -> Result<Option<Vec<u8>>> {
        let compressed = if let Some(path) = self.roots.find(relative) {
            std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?
        } else if let Some(pack) = &self.title_bg {
            let sarc = roead::sarc::Sarc::new(&pack[..])?;
            match sarc.get_data(relative) {
                Some(data) => data.to_vec(),
                None => return Ok(None),
            }
        } else {
            return Ok(None);
        };
        Ok(Some(crate::yaz0::decompress_if(&compressed)?.into_owned()))
    }

    /// An actor's pack as a set of files, if it exists.
    pub fn open(&self, actor: &str) -> Result<Option<Pack>> {
        let Some(bytes) = self.pack(actor)? else {
            return Ok(None);
        };
        roead::sarc::Sarc::new(&bytes[..])?;
        Ok(Some(Pack(bytes)))
    }

    /// The models an actor draws (empty if it has none, e.g. tags).
    pub fn models(&self, actor: &str) -> Result<Vec<ModelRef>> {
        let Some(pack) = self.pack(actor)? else {
            return Ok(Vec::new());
        };
        let sarc = roead::sarc::Sarc::new(&pack[..])?;
        let list = sarc
            .files()
            .find(|f| f.name().is_some_and(|n| n.starts_with("Actor/ModelList/")));
        match list {
            Some(file) => model_list(file.data()),
            None => Ok(Vec::new()),
        }
    }
}

/// The files of an actor pack (a decompressed SARC).
pub struct Pack(Vec<u8>);

impl Pack {
    fn sarc(&self) -> roead::sarc::Sarc<'_> {
        roead::sarc::Sarc::new(&self.0[..]).expect("checked when opened")
    }

    /// The file at `name`, e.g. `Actor/AS/Player_Move.bas`.
    pub fn file(&self, name: &str) -> Option<Vec<u8>> {
        self.sarc().get_data(name).map(<[u8]>::to_vec)
    }

    /// The first file whose name starts with `prefix`, e.g. `Actor/ASList/`.
    pub fn find(&self, prefix: &str) -> Option<Vec<u8>> {
        let sarc = self.sarc();
        let file = sarc
            .files()
            .find(|f| f.name().is_some_and(|n| n.starts_with(prefix)))?;
        Some(file.data().to_vec())
    }
}
