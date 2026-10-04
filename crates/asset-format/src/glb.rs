//! The GLB container (binary glTF 2.0) the model units ([`crate::model`])
//! and the skeletal clips ([`crate::anim`]) share: the glTF JSON they
//! write, the binary chunk's views and accessors, and reading them back.
//! Only what those files use is modelled; fields a file does not use are
//! left out of its JSON.

use serde::{Deserialize, Serialize};

use crate::model::MaterialInfo;
use crate::{FormatError, Result};

const GLB_MAGIC: &[u8; 4] = b"glTF";
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;
pub(crate) const FLOAT: u32 = 5126;
pub(crate) const UNSIGNED_SHORT: u32 = 5123;
pub(crate) const UNSIGNED_INT: u32 = 5125;
pub(crate) const ARRAY_BUFFER: u32 = 34962;
pub(crate) const ELEMENT_ARRAY_BUFFER: u32 = 34963;
pub(crate) const TRIANGLES: u32 = 4;

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Gltf {
    pub asset: Asset,
    #[serde(default)]
    pub scene: u32,
    #[serde(default)]
    pub scenes: Vec<Scene>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub meshes: Vec<Mesh>,
    #[serde(default)]
    pub materials: Vec<GltfMaterial>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skins: Vec<Skin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub accessors: Vec<Accessor>,
    #[serde(default)]
    pub buffer_views: Vec<BufferView>,
    #[serde(default)]
    pub buffers: Vec<Buffer>,
}

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct Asset {
    pub version: String,
    #[serde(default)]
    pub generator: String,
}

impl Asset {
    pub fn ours() -> Self {
        Self {
            version: "2.0".into(),
            generator: "botw рендерер bake".into(),
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct Scene {
    #[serde(default)]
    pub name: String,
    pub nodes: Vec<u32>,
}

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct Node {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skin: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub translation: Option<[f32; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f32; 4]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<[f32; 3]>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Mesh {
    #[serde(default)]
    pub name: String,
    pub primitives: Vec<Primitive>,
    pub extras: MeshExtras,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct MeshExtras {
    pub radius: f32,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Primitive {
    pub attributes: Attributes,
    pub indices: u32,
    pub material: u32,
    pub mode: u32,
    pub extras: PrimitiveExtras,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct PrimitiveExtras {
    pub lod: u32,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub(crate) struct Attributes {
    #[serde(rename = "POSITION")]
    pub position: u32,
    #[serde(rename = "NORMAL")]
    pub normal: u32,
    #[serde(rename = "TEXCOORD_0")]
    pub texcoord0: u32,
    #[serde(rename = "TANGENT")]
    pub tangent: u32,
    #[serde(rename = "COLOR_0", default, skip_serializing_if = "Option::is_none")]
    pub color0: Option<u32>,
    #[serde(
        rename = "TEXCOORD_1",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub texcoord1: Option<u32>,
    #[serde(
        rename = "TEXCOORD_2",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub texcoord2: Option<u32>,
    #[serde(
        rename = "TEXCOORD_3",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub texcoord3: Option<u32>,
    #[serde(rename = "JOINTS_0", default, skip_serializing_if = "Option::is_none")]
    pub joints0: Option<u32>,
    #[serde(rename = "WEIGHTS_0", default, skip_serializing_if = "Option::is_none")]
    pub weights0: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GltfMaterial {
    #[serde(default)]
    pub name: String,
    pub alpha_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha_cutoff: Option<f32>,
    pub double_sided: bool,
    pub extras: MaterialInfo,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Skin {
    pub inverse_bind_matrices: u32,
    pub joints: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Animation {
    #[serde(default)]
    pub name: String,
    pub channels: Vec<Channel>,
    pub samplers: Vec<AnimationSampler>,
    pub extras: AnimationExtras,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Channel {
    pub sampler: u32,
    pub target: ChannelTarget,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct ChannelTarget {
    pub node: u32,
    /// `translation`, `rotation` or `scale`.
    pub path: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct AnimationSampler {
    pub input: u32,
    pub output: u32,
    pub interpolation: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AnimationExtras {
    pub looping: bool,
    pub frame_count: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Accessor {
    pub buffer_view: u32,
    pub component_type: u32,
    pub count: u32,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<Vec<f32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<Vec<f32>>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BufferView {
    pub buffer: u32,
    pub byte_offset: u32,
    pub byte_length: u32,
    /// Vertex and index data say what they are; matrices and animation
    /// samples have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<u32>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Buffer {
    pub byte_length: u32,
}

/// Builds the binary chunk and its views and accessors.
#[derive(Default)]
pub(crate) struct Writer {
    bin: Vec<u8>,
    views: Vec<BufferView>,
    accessors: Vec<Accessor>,
}

impl Writer {
    fn add(&mut self, bytes: &[u8], target: Option<u32>, accessor: Accessor) -> u32 {
        self.bin.resize(self.bin.len().next_multiple_of(4), 0);
        self.views.push(BufferView {
            buffer: 0,
            byte_offset: self.bin.len() as u32,
            byte_length: bytes.len() as u32,
            target,
        });
        self.bin.extend_from_slice(bytes);
        self.accessors.push(Accessor {
            buffer_view: self.views.len() as u32 - 1,
            ..accessor
        });
        self.accessors.len() as u32 - 1
    }

    /// Floats of glTF type `kind` (`SCALAR`, `VEC3`, `MAT4`…), with their
    /// bounds if `bounds`.
    pub fn floats<const N: usize>(
        &mut self,
        values: &[[f32; N]],
        kind: &str,
        bounds: bool,
        target: Option<u32>,
    ) -> u32 {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let (min, max) = if bounds && !values.is_empty() {
            let mut min = [f32::MAX; N];
            let mut max = [f32::MIN; N];
            for v in values {
                for i in 0..N {
                    min[i] = min[i].min(v[i]);
                    max[i] = max[i].max(v[i]);
                }
            }
            (Some(min.to_vec()), Some(max.to_vec()))
        } else {
            (None, None)
        };
        self.add(
            &bytes,
            target,
            Accessor {
                buffer_view: 0,
                component_type: FLOAT,
                count: values.len() as u32,
                kind: kind.into(),
                min,
                max,
            },
        )
    }

    /// Vertex attributes of unsigned shorts (joint indices).
    pub fn u16s<const N: usize>(&mut self, values: &[[u16; N]], kind: &str) -> u32 {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        self.add(
            &bytes,
            Some(ARRAY_BUFFER),
            Accessor {
                buffer_view: 0,
                component_type: UNSIGNED_SHORT,
                count: values.len() as u32,
                kind: kind.into(),
                min: None,
                max: None,
            },
        )
    }

    pub fn indices(&mut self, values: &[u32]) -> u32 {
        let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.add(
            &bytes,
            Some(ELEMENT_ARRAY_BUFFER),
            Accessor {
                buffer_view: 0,
                component_type: UNSIGNED_INT,
                count: values.len() as u32,
                kind: "SCALAR".into(),
                min: None,
                max: None,
            },
        )
    }

    /// The GLB of `gltf` with this binary chunk (its accessors, views and
    /// buffer filled in).
    pub fn finish(mut self, mut gltf: Gltf, what: &'static str) -> Result<Vec<u8>> {
        self.bin.resize(self.bin.len().next_multiple_of(4), 0);
        gltf.accessors = self.accessors;
        gltf.buffer_views = self.views;
        gltf.buffers = vec![Buffer {
            byte_length: self.bin.len() as u32,
        }];
        let mut json = serde_json::to_vec(&gltf).map_err(|_| FormatError::Invalid(what))?;
        json.resize(json.len().next_multiple_of(4), b' ');
        let total = 12 + 8 + json.len() + 8 + self.bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(GLB_MAGIC);
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
        out.extend_from_slice(&json);
        out.extend_from_slice(&(self.bin.len() as u32).to_le_bytes());
        out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
        out.extend_from_slice(&self.bin);
        Ok(out)
    }
}

/// A GLB read back: its glTF and binary chunk.
pub(crate) struct Glb<'a> {
    pub gltf: Gltf,
    bin: &'a [u8],
}

impl<'a> Glb<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let invalid = FormatError::Invalid;
        let u32_at = |at: usize| -> Result<u32> {
            Ok(u32::from_le_bytes(
                bytes
                    .get(at..at + 4)
                    .ok_or(invalid("glb: truncated"))?
                    .try_into()
                    .expect("four bytes"),
            ))
        };
        if bytes.get(..4) != Some(&GLB_MAGIC[..]) || u32_at(4)? != 2 {
            return Err(invalid("glb: not a glTF 2.0 binary"));
        }
        let json_len = u32_at(12)? as usize;
        if u32_at(16)? != CHUNK_JSON {
            return Err(invalid("glb: the first chunk is not JSON"));
        }
        let json = bytes
            .get(20..20 + json_len)
            .ok_or(invalid("glb: truncated"))?;
        let bin_at = 20 + json_len;
        let bin_len = u32_at(bin_at)? as usize;
        if u32_at(bin_at + 4)? != CHUNK_BIN {
            return Err(invalid("glb: no binary chunk"));
        }
        let bin = bytes
            .get(bin_at + 8..bin_at + 8 + bin_len)
            .ok_or(invalid("glb: truncated"))?;
        let gltf =
            serde_json::from_slice(json).map_err(|_| invalid("glb: unreadable glTF JSON"))?;
        Ok(Self { gltf, bin })
    }

    /// The bytes of `accessor`, checked to be `component`s, `width` per
    /// element of `size` bytes each.
    fn data(&self, accessor: u32, component: u32, width: usize, size: usize) -> Result<&'a [u8]> {
        let invalid = FormatError::Invalid;
        let a = self
            .gltf
            .accessors
            .get(accessor as usize)
            .ok_or(invalid("glb: accessor out of range"))?;
        let view = self
            .gltf
            .buffer_views
            .get(a.buffer_view as usize)
            .ok_or(invalid("glb: buffer view out of range"))?;
        if a.component_type != component {
            return Err(invalid("glb: unexpected component type"));
        }
        let start = view.byte_offset as usize;
        let len = a.count as usize * width * size;
        self.bin
            .get(start..start + len)
            .filter(|_| len <= view.byte_length as usize)
            .ok_or(invalid("glb: accessor outside its buffer"))
    }

    pub fn floats<const N: usize>(&self, accessor: u32) -> Result<Vec<[f32; N]>> {
        let bytes = self.data(accessor, FLOAT, N, 4)?;
        Ok(bytes
            .chunks_exact(4 * N)
            .map(|element| {
                std::array::from_fn(|i| {
                    f32::from_le_bytes(element[4 * i..4 * i + 4].try_into().expect("four bytes"))
                })
            })
            .collect())
    }

    pub fn u16s<const N: usize>(&self, accessor: u32) -> Result<Vec<[u16; N]>> {
        let bytes = self.data(accessor, UNSIGNED_SHORT, N, 2)?;
        Ok(bytes
            .chunks_exact(2 * N)
            .map(|element| {
                std::array::from_fn(|i| {
                    u16::from_le_bytes(element[2 * i..2 * i + 2].try_into().expect("two bytes"))
                })
            })
            .collect())
    }

    pub fn u32s(&self, accessor: u32) -> Result<Vec<u32>> {
        let bytes = self.data(accessor, UNSIGNED_INT, 1, 4)?;
        Ok(bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().expect("four bytes")))
            .collect())
    }
}
