//! Readers for the Breath of the Wild data this project uses.
//!
//! Nothing here ships game data: every reader works on bytes the user
//! extracted from their own copy of the game. See `archived FORMATS.md notes` for the
//! format notes these readers are based on.

pub mod actor;
pub mod aidef;
pub mod anim_seq;
pub mod armor;
pub mod bfres;
pub mod content;
pub mod eco;
pub mod env;
pub mod envset;
pub mod map;
pub mod params;
pub mod prod;
pub mod ptcl;
pub mod shader;
pub mod shader_bfsha;
pub mod sky;
pub mod stats;
pub mod stera;
pub mod system_model;
pub mod terrain;
pub mod trees;
pub mod xlink;
pub mod yaz0;

/// Error returned by every reader in this crate.
#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("{what}: expected {expected} bytes, got {actual}")]
    WrongSize {
        what: &'static str,
        expected: usize,
        actual: usize,
    },
    #[error("archive error: {0}")]
    Archive(#[from] roead::Error),
    #[error("{0}")]
    Invalid(&'static str),
    #[error("Yaz0: {0}")]
    Yaz0(&'static str),
    #[error("archive entry without a name at index {0}")]
    UnnamedEntry(usize),
    #[error("I/O error on {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub type Result<T> = std::result::Result<T, FormatError>;
