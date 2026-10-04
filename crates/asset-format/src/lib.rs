//! The layout and file formats of `assets/`, the baked world the renderer
//! reads. `bake` writes it from the user's game dump; `render` reads only
//! this. The formats are ours: if the assets are ever replaced, the
//! renderer does not change.
//!
//! ```text
//! assets/
//!   places.ron              named places (the game's location markers)
//!   light/ssao_noise.ron    the ambient occlusion's rotations (RgImage)
//!   sky/env_params.ron      palettes, sun, cloud layers, climates (env)
//!   sky/env_set.ron         the renderer's base objects (envset)
//!   sky/ecosystem.ron       the climate map and areas (eco)
//!   sky/inscatter.bin       the atmosphere table (sky::Inscatter)
//!   sky/cloud_noise.ron     the fog's height noise (light::GreyImage)
//!   sky/clouds.ron          cloud textures (sky::CloudTextures)
//!   sky/clouds/*.ktx2
//!   terrain/index.ron       baked tiles (TerrainIndex)
//!   terrain/materials.ron   terrain materials (MaterialTable)
//!   terrain/albedo.ktx2     material albedo, BC1 sRGB array with mips
//!   terrain/normals.ktx2    material normals + gloss, BC1 array with mips
//!   terrain/tiles/5LIIIIIIII.tile   one per tile (TileFile)
//!   water/table.ron         the water table, WaterAlb (water::WaterTable)
//!   water/tera_water.ron    TeraWater's shader parameters (water::TeraWater)
//!   water/normals.ktx2      WaterNrm: RG8 array, a layer per water kind,
//!                           level 0 as in the game, mips rebuilt
//!   water/foam.ktx2         WaterEmm: R8 array, the same layers
//!   objects/index.ron       what was baked around the region (ObjectIndex)
//!   objects/cells/<cell>.ron  a map cell's placed actors (PlacedActors)
//!   objects/far.ron         `_Far` stand-ins near the region (FarIndex)
//!   objects/actors.ron      actor → models (ActorModels)
//!   models/<folder>/<unit>.glb  a model unit (model::Model)
//!   models/<folder>/<texture>.ktx2  its folder's textures, game and derived
//!   grass/blade.ktx2        GrassAlb as stored (grass)
//!   grass/tuft.ktx2         GrassCrossAlb, RGBA8 with rebuilt mips
//!   grass/tera_grass.ron    TeraGrass's grass parameters (grass::TeraGrass)
//!   grass/hidden/<qx>_<qz>.bin  where the grass is taken away (grass::HiddenQuarter)
//!   trees/index.ron         the far trees (trees::FarTreeIndex)
//!   trees/<atlas>.ktx2      their billboard atlases, as the game stores them
//!   trees/dither_mask.ktx2  TreeDitherMask: R8, every level
//!   effects/index.ron       effect files, who plays what (effects)
//!   effects/files/…         the effect files: emitters, textures
//!   effects/elink.ron       the effect links the renderer plays (elink)
//!   characters/<name>.ron   a character: models, hidden shapes, clips (character)
//!   characters/models/<folder>/<unit>.glb  skinned model units (model::Model)
//!   characters/models/<folder>/<texture>.ktx2  their textures
//!   characters/anims/<set>/<clip>.glb  skeletal clips (anim::Clip)
//! ```

pub mod anim;
pub mod character;
pub mod eco;
pub mod effects;
pub mod elink;
pub mod env;
pub mod envset;
mod glb;
pub mod grass;
pub mod light;
pub mod model;
pub mod objects;
pub mod places;
pub mod sky;
pub mod terrain;
pub mod texture;
pub mod trees;
pub mod water;
pub mod xlu;

use std::path::{Path, PathBuf};

pub use places::{Place, Places};

/// Paths inside `assets/`.
pub mod paths {
    use crate::terrain::TileId;

    pub const PLACES: &str = "places.ron";
    pub const SSAO_NOISE: &str = "light/ssao_noise.ron";
    pub const ENV_PARAMS: &str = "sky/env_params.ron";
    pub const ENV_SET: &str = "sky/env_set.ron";
    pub const ECOSYSTEM: &str = "sky/ecosystem.ron";
    pub const INSCATTER: &str = "sky/inscatter.bin";
    pub const CLOUD_NOISE: &str = "sky/cloud_noise.ron";
    pub const CLOUD_TEXTURES: &str = "sky/clouds.ron";
    pub const CLOUD_TEXTURE_DIR: &str = "sky/clouds";
    pub const TERRAIN_INDEX: &str = "terrain/index.ron";
    pub const TERRAIN_MATERIALS: &str = "terrain/materials.ron";
    pub const TERRAIN_ALBEDO: &str = "terrain/albedo.ktx2";
    pub const TERRAIN_NORMALS: &str = "terrain/normals.ktx2";
    pub const TERRAIN_TILES: &str = "terrain/tiles";
    pub const WATER_TABLE: &str = "water/table.ron";
    pub const WATER_MATERIAL: &str = "water/tera_water.ron";
    pub const WATER_NORMALS: &str = "water/normals.ktx2";
    pub const WATER_FOAM: &str = "water/foam.ktx2";
    pub const OBJECT_INDEX: &str = "objects/index.ron";
    pub const OBJECT_CELLS: &str = "objects/cells";
    pub const FAR_INDEX: &str = "objects/far.ron";
    pub const ACTOR_MODELS: &str = "objects/actors.ron";
    pub const MODELS: &str = "models";
    pub const GRASS_BLADE: &str = "grass/blade.ktx2";
    pub const GRASS_TUFT: &str = "grass/tuft.ktx2";
    pub const GRASS_MATERIAL: &str = "grass/tera_grass.ron";
    pub const GRASS_HIDDEN: &str = "grass/hidden";
    pub const TREES: &str = "trees";
    pub const TREE_INDEX: &str = "trees/index.ron";
    pub const TREE_DITHER_MASK: &str = "trees/dither_mask.ktx2";
    pub const EFFECT_INDEX: &str = "effects/index.ron";
    pub const EFFECT_FILES: &str = "effects/files";
    pub const ELINK: &str = crate::elink::ELINK;
    pub const CHARACTERS: &str = "characters";
    pub const CHARACTER_MODELS: &str = "characters/models";
    pub const CHARACTER_ANIMS: &str = "characters/anims";
    /// The UMii villagers (`bake --only npcs`): their characters are
    /// `umii/<actor>`, their models `characters/models/umii/<actor>/`.
    pub const UMII: &str = "characters/umii";
    pub const UMII_MODELS: &str = "characters/models/umii";
    pub const UMII_PLACED: &str = "characters/umii/placed.ron";

    pub fn character(name: &str) -> String {
        format!("{CHARACTERS}/{name}.ron")
    }

    pub fn character_unit(folder: &str, unit: &str) -> String {
        format!("{CHARACTER_MODELS}/{folder}/{unit}.glb")
    }

    pub fn character_clip(set: &str, clip: &str) -> String {
        format!("{CHARACTER_ANIMS}/{set}/{clip}.glb")
    }

    pub fn object_cell(cell: &str) -> String {
        format!("{OBJECT_CELLS}/{cell}.ron")
    }

    pub fn model_unit(folder: &str, unit: &str) -> String {
        format!("{MODELS}/{folder}/{unit}.glb")
    }

    pub fn model_texture(folder: &str, texture: &str) -> String {
        format!("{MODELS}/{folder}/{texture}.ktx2")
    }

    pub fn terrain_tile(tile: TileId) -> String {
        format!("{TERRAIN_TILES}/{}.tile", tile.file_stem())
    }
}

/// Error returned by every reader and writer in this crate.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("{what}: expected {expected} bytes, got {actual}")]
    WrongSize {
        what: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("{0}")]
    Invalid(&'static str),
    #[error("compression: {0}")]
    Compression(String),
    #[error("{path}: {message}")]
    Ron { path: PathBuf, message: String },
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, FormatError>;

pub fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| FormatError::Io {
        path: path.to_owned(),
        source,
    })
}

/// Writes `bytes` to `path`, creating its folders.
pub fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    let io = |source| FormatError::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io)?;
    }
    std::fs::write(path, bytes).map_err(io)
}

pub fn read_ron<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let text = String::from_utf8_lossy(&read(path)?).into_owned();
    ron::from_str(&text).map_err(|e| FormatError::Ron {
        path: path.to_owned(),
        message: e.to_string(),
    })
}

pub fn write_ron<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let config = ron::ser::PrettyConfig::new().depth_limit(3);
    let text = ron::ser::to_string_pretty(value, config).map_err(|e| FormatError::Ron {
        path: path.to_owned(),
        message: e.to_string(),
    })?;
    write(path, text.as_bytes())
}
