//! The sky: the precomputed atmosphere table (`sky/inscatter.bin`) and the
//! layout of `sky/`. The parameters themselves are `env`, `envset` and
//! `eco` (RON).

use std::io::Read;

use crate::{FormatError, Result};

/// Bruneton's table resolutions as the game sizes its surfaces
/// (botw-formats `sky.rs`): `RES_NU` slices of `RES_MU_S` texels across x,
/// `RES_MU` rows and `RES_R` depth slices.
pub const RES_NU: u32 = 8;
pub const RES_MU_S: u32 = 32;
pub const RES_MU: u32 = 32;
pub const RES_R: u32 = 16;

/// The inscatter table of `sky.skybin`, untiled and decoded: RGB Rayleigh
/// inscatter and, in alpha, the red channel of the Mie inscatter.
#[derive(Clone, Debug, PartialEq)]
pub struct Inscatter {
    /// `width × height × depth` texels, x fastest, then y, then z.
    pub texels: Vec<[f32; 4]>,
}

const MAGIC: &[u8; 4] = b"HINS";

impl Inscatter {
    pub const WIDTH: u32 = RES_NU * RES_MU_S;
    pub const HEIGHT: u32 = RES_MU;
    pub const DEPTH: u32 = RES_R;
    const TEXELS: usize = (Self::WIDTH * Self::HEIGHT * Self::DEPTH) as usize;

    pub fn texel(&self, x: u32, y: u32, z: u32) -> [f32; 4] {
        self.texels[((z * Self::HEIGHT + y) * Self::WIDTH + x) as usize]
    }

    /// A zstd frame of `HINS` and the texels as little-endian `f32`s.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut raw = MAGIC.to_vec();
        for texel in &self.texels {
            for value in texel {
                raw.extend_from_slice(&value.to_le_bytes());
            }
        }
        zstd::encode_all(&raw[..], 9).map_err(|e| FormatError::Compression(e.to_string()))
    }

    pub fn parse(compressed: &[u8]) -> Result<Self> {
        let mut raw = Vec::new();
        zstd::Decoder::new(compressed)
            .and_then(|mut d| d.read_to_end(&mut raw))
            .map_err(|e| FormatError::Compression(e.to_string()))?;
        let body = raw
            .strip_prefix(MAGIC)
            .ok_or(FormatError::Invalid("inscatter: missing HINS magic"))?;
        if body.len() != Self::TEXELS * 16 {
            return Err(FormatError::WrongSize {
                what: "inscatter",
                expected: Self::TEXELS * 16,
                actual: body.len(),
            });
        }
        let texels = body
            .as_chunks::<16>()
            .0
            .iter()
            .map(|t| {
                std::array::from_fn(|i| f32::from_le_bytes(t[i * 4..i * 4 + 4].try_into().unwrap()))
            })
            .collect();
        Ok(Self { texels })
    }

    pub fn read(path: &std::path::Path) -> Result<Self> {
        Self::parse(&crate::read(path)?)
    }
}

/// The cloud textures (`sky/clouds.ron`): the game's list in its order
/// (`collect.genvres`; a layer's texture numbers index it), each a KTX2
/// file under `sky/clouds/`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CloudTextures {
    /// File names under `sky/clouds/`, in the game's order.
    pub textures: Vec<String>,
    /// The cloud shadow's texture (`env.bgenv`'s `prjshd` of
    /// `Master_Field`), a file under `sky/clouds/`.
    pub shadow: Option<String>,
}
