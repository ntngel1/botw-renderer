//! Small tables of the game's lighting.

use serde::{Deserialize, Serialize};

/// A small two-channel 8-bit image, row-major.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgImage {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<[u8; 2]>,
}

/// A small single-channel 8-bit image, row-major.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GreyImage {
    pub width: u32,
    pub height: u32,
    pub texels: Vec<u8>,
}
