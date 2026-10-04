//! FMDL: a model's skeleton, vertex buffers, shapes (meshes with LODs) and
//! materials, Wii U layout (FRES 4.5, checked against `Obj_TreeConiferous`).
//!
//! ```text
//! FMDL  0x04 name, 0x08 path, 0x0C FSKL, 0x10 FVTX array, 0x14 FSHP group,
//!       0x18 FMAT group, 0x1C user data, 0x20 u16 counts (FVTX, FSHP, FMAT, user)
//! FSKL  0x04 flags, 0x08 u16 bone count, smooth/rigid matrix counts,
//!       0x10 bone group, 0x14 bone array (0x40 each), 0x18 matrix-to-bone list
//! bone  name, u16 index, u16 parent, i16 smooth/rigid matrix, i16 billboard,
//!       u16 user count, u32 flags (0x1000: Euler XYZ, else quaternion),
//!       scale, rotation (4 floats), translation
//! FVTX  0x04 u8 attribute count, u8 buffer count, 0x08 vertex count,
//!       0x0C u8 skin count, 0x14 attribute group, 0x18 buffer array (0x18 each)
//! attr  name, u8 buffer, u16 offset, u32 GX2 attribute format
//! buffer  u32 -, u32 size, u32 -, u16 stride, u16 -, u32 -, data pointer
//! FSHP  0x04 name, 0x0C u16 index, u16 material, u16 bone, u16 vertex buffer,
//!       u16 skin bone count, u8 skin count, u8 LOD count, …, 0x24 LOD array
//!       (0x1C each: u32 primitive, u32 index format, u32 index count,
//!       u16 sub-mesh count, sub-meshes, index buffer, u32 first vertex),
//!       0x28 skin bone indices
//! FMAT  0x04 name, 0x0E u16 render info count, 0x10 u8 samplers, u8 textures,
//!       0x1C render info group, 0x24 shader assign, 0x28 texture refs
//!       (name, FTEX), 0x30 sampler group (names like `_a0`, in texture order)
//! ```
//!
//! Vertex and index data are big-endian.

use super::Reader;
use crate::{FormatError, Result};

#[derive(Clone, Debug)]
pub struct Model {
    pub name: String,
    pub bones: Vec<Bone>,
    /// Bone index for each matrix index used by skinned vertices.
    pub matrix_to_bone: Vec<u16>,
    pub vertex_buffers: Vec<VertexBuffer>,
    pub shapes: Vec<Shape>,
    pub materials: Vec<Material>,
}

#[derive(Clone, Debug)]
pub struct Bone {
    pub name: String,
    pub parent: Option<u16>,
    pub scale: [f32; 3],
    /// A quaternion (x, y, z, w), or Euler XYZ radians in the first three.
    pub rotation: [f32; 4],
    pub euler: bool,
    pub translation: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct VertexBuffer {
    pub vertex_count: u32,
    /// How many bones each vertex is bound to (0 = rigid to the shape's bone).
    pub skin_count: u8,
    pub attributes: Vec<Attribute>,
}

/// A decoded vertex attribute: `_p0` position, `_n0` normal, `_t0` tangent,
/// `_u0`… UV sets, `_c0` colour, `_i0` bone indices, `_w0` weights.
#[derive(Clone, Debug)]
pub struct Attribute {
    pub name: String,
    pub format: u32,
    pub values: Vec<[f32; 4]>,
}

impl VertexBuffer {
    pub fn attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.iter().find(|a| a.name == name)
    }
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub name: String,
    pub material: u16,
    pub bone: u16,
    pub vertex_buffer: u16,
    pub skin_count: u8,
    /// Levels of detail, finest first.
    pub lods: Vec<Mesh>,
    pub skin_bones: Vec<u16>,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    /// GX2 primitive type (4 = triangle list).
    pub primitive: u32,
    /// Indices, already offset by the mesh's first vertex.
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    /// `(sampler, texture)`, e.g. `("_a0", "Tree_TreeConiferousLeaf_A_Alb")`.
    pub textures: Vec<(String, String)>,
    /// Render info as text, e.g. `gsys_render_state_mode = mask`.
    pub render_info: Vec<(String, String)>,
    pub shader_archive: String,
    pub shading_model: String,
    /// Shader options, e.g. `enable_alpha_test = 1`.
    pub shader_options: Vec<(String, String)>,
    /// Sampler assignments from the shader's names to the material's.
    pub sampler_assign: Vec<(String, String)>,
    /// The material's samplers with their GX2 sampler words
    /// (`SQ_TEX_SAMPLER_WORD0..2`: wrap, filters, LOD).
    pub samplers: Vec<(String, [u32; 3])>,
    pub render_state: RenderState,
    /// Shader parameters: name, GX2 type, values (as floats or unsigned ints).
    pub shader_params: Vec<ShaderParam>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShaderParam {
    pub name: String,
    /// 0–3 bool, 4–7 int, 8–11 uint, 12–15 float (1–4 components), 16+ matrices/SRTs.
    pub kind: u8,
    pub values: Vec<f32>,
}

impl Material {
    pub fn shader_param(&self, name: &str) -> Option<&[f32]> {
        self.shader_params
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.values.as_slice())
    }
}

/// FMAT render state (0x20): mode, face culling, alpha test.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RenderState {
    /// 0 custom, 1 opaque, 2 alpha mask, 3 translucent.
    pub mode: u32,
    pub cull_front: bool,
    pub cull_back: bool,
    /// Alpha-test reference when alpha testing is enabled.
    pub alpha_test: Option<f32>,
    /// GX2 `CB_COLOR_CONTROL` (+0x14: ROP and per-target blend enable in
    /// bits 8–15), the blend target (+0x18) and its `CB_BLEND_CONTROL`
    /// (+0x1C), as stored.
    pub blend: [u32; 3],
}

impl Material {
    /// The texture bound to `sampler` (`_a0` albedo, `_n0` normal, …).
    pub fn texture(&self, sampler: &str) -> Option<&str> {
        self.textures
            .iter()
            .find(|(s, _)| s == sampler)
            .map(|(_, t)| t.as_str())
    }

    // SI-FMT-12: sampler name fallback is our reading.
    /// The texture bound to `sampler`, or to the same name without its
    /// leading underscore, which a few materials use (`a0`, `n0`: the far
    /// model of Death Mountain's summit).
    pub fn sampler_texture(&self, sampler: &str) -> Option<&str> {
        self.texture(sampler)
            .or_else(|| self.texture(sampler.strip_prefix('_')?))
    }

    pub fn render_info(&self, key: &str) -> Option<&str> {
        self.render_info
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn shader_option(&self, key: &str) -> Option<&str> {
        self.shader_options
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

pub(crate) fn read_model(r: &Reader, at: usize) -> Result<Model> {
    if r.slice(at, 4)? != b"FMDL" {
        return Err(FormatError::Invalid("bfres: model without FMDL magic"));
    }
    let name = r.string_at(at + 0x04)?.unwrap_or_default();
    let (bones, matrix_to_bone) = match r.pointer(at + 0x0C)? {
        Some(skeleton) => read_skeleton(r, skeleton)?,
        None => (Vec::new(), Vec::new()),
    };
    let vertex_buffer_count = r.u16(at + 0x20)? as usize;
    let vertex_buffers = match r.pointer(at + 0x10)? {
        Some(array) => (0..vertex_buffer_count)
            .map(|i| read_vertex_buffer(r, array + i * 0x20))
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    let shapes = r
        .index_group(at + 0x14)?
        .into_iter()
        .map(|(name, s)| read_shape(r, s, name))
        .collect::<Result<_>>()?;
    let materials = r
        .index_group(at + 0x18)?
        .into_iter()
        .map(|(name, m)| read_material(r, m, name))
        .collect::<Result<_>>()?;
    Ok(Model {
        name,
        bones,
        matrix_to_bone,
        vertex_buffers,
        shapes,
        materials,
    })
}

fn read_skeleton(r: &Reader, at: usize) -> Result<(Vec<Bone>, Vec<u16>)> {
    let count = r.u16(at + 0x08)? as usize;
    let smooth = r.u16(at + 0x0A)? as usize;
    let rigid = r.u16(at + 0x0C)? as usize;
    let array = r
        .pointer(at + 0x14)?
        .ok_or(FormatError::Invalid("bfres: skeleton without bones"))?;
    let bones = (0..count)
        .map(|i| {
            let b = array + i * 0x40;
            let f = |o: usize| r.f32(b + o);
            let parent = r.u16(b + 0x06)?;
            Ok(Bone {
                name: r.string_at(b)?.unwrap_or_default(),
                parent: (parent != 0xFFFF).then_some(parent),
                euler: r.u32(b + 0x10)? & 0x1000 != 0,
                scale: [f(0x14)?, f(0x18)?, f(0x1C)?],
                rotation: [f(0x20)?, f(0x24)?, f(0x28)?, f(0x2C)?],
                translation: [f(0x30)?, f(0x34)?, f(0x38)?],
            })
        })
        .collect::<Result<_>>()?;
    let matrix_to_bone = match r.pointer(at + 0x18)? {
        Some(list) => (0..smooth + rigid)
            .map(|i| r.u16(list + i * 2))
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    Ok((bones, matrix_to_bone))
}

fn read_vertex_buffer(r: &Reader, at: usize) -> Result<VertexBuffer> {
    if r.slice(at, 4)? != b"FVTX" {
        return Err(FormatError::Invalid(
            "bfres: vertex buffer without FVTX magic",
        ));
    }
    let buffer_count = r.u8(at + 0x05)? as usize;
    let vertex_count = r.u32(at + 0x08)?;
    let skin_count = r.u8(at + 0x0C)?;
    let buffer_array = r
        .pointer(at + 0x18)?
        .ok_or(FormatError::Invalid("bfres: vertex buffer without buffers"))?;
    let buffers: Vec<(usize, usize)> = (0..buffer_count)
        .map(|i| {
            let b = buffer_array + i * 0x18;
            let stride = r.u16(b + 0x0C)? as usize;
            let data = r
                .pointer(b + 0x14)?
                .ok_or(FormatError::Invalid("bfres: buffer without data"))?;
            Ok((data, stride))
        })
        .collect::<Result<_>>()?;
    let attributes = r
        .index_group(at + 0x14)?
        .into_iter()
        .map(|(name, a)| {
            let buffer = r.u8(a + 0x04)? as usize;
            let offset = r.u16(a + 0x06)? as usize;
            let format = r.u32(a + 0x08)?;
            let &(data, stride) = buffers
                .get(buffer)
                .ok_or(FormatError::Invalid("bfres: attribute buffer out of range"))?;
            let values = (0..vertex_count as usize)
                .map(|v| decode_attribute(r, data + v * stride + offset, format))
                .collect::<Result<_>>()?;
            Ok(Attribute {
                name,
                format,
                values,
            })
        })
        .collect::<Result<_>>()?;
    Ok(VertexBuffer {
        vertex_count,
        skin_count,
        attributes,
    })
}

/// Decodes one big-endian GX2 attribute value to four floats (missing
/// components are 0, alpha 1).
pub(crate) fn decode_attribute(r: &Reader, at: usize, format: u32) -> Result<[f32; 4]> {
    let u8n = |i: usize| -> Result<f32> { Ok(f32::from(r.u8(at + i)?) / 255.0) };
    let s8n = |i: usize| -> Result<f32> { Ok((f32::from(r.u8(at + i)? as i8) / 127.0).max(-1.0)) };
    let u8i = |i: usize| -> Result<f32> { Ok(f32::from(r.u8(at + i)?)) };
    let u16n = |i: usize| -> Result<f32> { Ok(f32::from(r.u16(at + 2 * i)?) / 65535.0) };
    let s16n = |i: usize| -> Result<f32> {
        Ok((f32::from(r.u16(at + 2 * i)? as i16) / 32767.0).max(-1.0))
    };
    let u16i = |i: usize| -> Result<f32> { Ok(f32::from(r.u16(at + 2 * i)?)) };
    let f16 = |i: usize| -> Result<f32> { Ok(half_to_f32(r.u16(at + 2 * i)?)) };
    let f32_ = |i: usize| r.f32(at + 4 * i);
    Ok(match format {
        0x000 => [u8n(0)?, 0.0, 0.0, 1.0],
        0x004 => [u8n(0)?, u8n(1)?, 0.0, 1.0],
        0x00A => [u8n(0)?, u8n(1)?, u8n(2)?, u8n(3)?],
        0x007 => [u16n(0)?, u16n(1)?, 0.0, 1.0],
        0x00E => [u16n(0)?, u16n(1)?, u16n(2)?, u16n(3)?],
        0x100 => [u8i(0)?, 0.0, 0.0, 0.0],
        0x104 => [u8i(0)?, u8i(1)?, 0.0, 0.0],
        0x10A => [u8i(0)?, u8i(1)?, u8i(2)?, u8i(3)?],
        0x107 => [u16i(0)?, u16i(1)?, 0.0, 0.0],
        0x10E => [u16i(0)?, u16i(1)?, u16i(2)?, u16i(3)?],
        0x204 => [s8n(0)?, s8n(1)?, 0.0, 1.0],
        0x20A => [s8n(0)?, s8n(1)?, s8n(2)?, s8n(3)?],
        0x207 => [s16n(0)?, s16n(1)?, 0.0, 1.0],
        0x20E => [s16n(0)?, s16n(1)?, s16n(2)?, s16n(3)?],
        0x00B | 0x20B => {
            let v = r.u32(at)?;
            let signed = format == 0x20B;
            let c = |shift: u32| -> f32 {
                let raw = (v >> shift) & 0x3FF;
                if signed {
                    ((((raw << 22) as i32) >> 22) as f32 / 511.0).max(-1.0)
                } else {
                    raw as f32 / 1023.0
                }
            };
            let w = (v >> 30) & 3;
            let w = if signed {
                ((((w << 30) as i32) >> 30) as f32).max(-1.0)
            } else {
                w as f32 / 3.0
            };
            [c(0), c(10), c(20), w]
        }
        0x806 => [f32_(0)?, 0.0, 0.0, 1.0],
        0x808 => [f16(0)?, f16(1)?, 0.0, 1.0],
        0x80D => [f32_(0)?, f32_(1)?, 0.0, 1.0],
        0x80F => [f16(0)?, f16(1)?, f16(2)?, f16(3)?],
        0x811 => [f32_(0)?, f32_(1)?, f32_(2)?, 1.0],
        0x813 => [f32_(0)?, f32_(1)?, f32_(2)?, f32_(3)?],
        _ => {
            return Err(FormatError::Invalid(
                "bfres: unsupported vertex attribute format",
            ));
        }
    })
}

pub(crate) fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = ((h >> 10) & 0x1F) as i32;
    let mantissa = f32::from(h & 0x3FF);
    sign * match exponent {
        0 => mantissa / 1024.0 * 2f32.powi(-14),
        31 => {
            if mantissa == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => (1.0 + mantissa / 1024.0) * 2f32.powi(e - 15),
    }
}

fn read_shape(r: &Reader, at: usize, name: String) -> Result<Shape> {
    let material = r.u16(at + 0x0E)?;
    let bone = r.u16(at + 0x10)?;
    let vertex_buffer = r.u16(at + 0x12)?;
    let skin_bone_count = r.u16(at + 0x14)? as usize;
    let skin_count = r.u8(at + 0x16)?;
    let lod_count = r.u8(at + 0x17)? as usize;
    let lods = match r.pointer(at + 0x24)? {
        Some(array) => (0..lod_count)
            .map(|i| read_mesh(r, array + i * 0x1C))
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    let skin_bones = match r.pointer(at + 0x28)? {
        Some(list) => (0..skin_bone_count)
            .map(|i| r.u16(list + 2 * i))
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    Ok(Shape {
        name,
        material,
        bone,
        vertex_buffer,
        skin_count,
        lods,
        skin_bones,
    })
}

fn read_mesh(r: &Reader, at: usize) -> Result<Mesh> {
    let primitive = r.u32(at)?;
    let index_format = r.u32(at + 0x04)?;
    let index_count = r.u32(at + 0x08)? as usize;
    let buffer = r
        .pointer(at + 0x14)?
        .ok_or(FormatError::Invalid("bfres: mesh without index buffer"))?;
    let data = r
        .pointer(buffer + 0x14)?
        .ok_or(FormatError::Invalid("bfres: index buffer without data"))?;
    let first_vertex = r.u32(at + 0x18)?;
    let indices = (0..index_count)
        .map(|i| {
            Ok(first_vertex
                + match index_format {
                    // GX2 index formats: 0/4 16-bit (little/big endian), 1/9 32-bit.
                    4 => u32::from(r.u16(data + 2 * i)?),
                    9 => r.u32(data + 4 * i)?,
                    0 => u32::from(u16::from_le_bytes(
                        r.slice(data + 2 * i, 2)?.try_into().unwrap(),
                    )),
                    1 => u32::from_le_bytes(r.slice(data + 4 * i, 4)?.try_into().unwrap()),
                    _ => return Err(FormatError::Invalid("bfres: unsupported index format")),
                })
        })
        .collect::<Result<_>>()?;
    Ok(Mesh { primitive, indices })
}

fn read_material(r: &Reader, at: usize, name: String) -> Result<Material> {
    let texture_count = r.u8(at + 0x11)? as usize;
    let texture_names: Vec<String> = match r.pointer(at + 0x28)? {
        Some(refs) => (0..texture_count)
            .map(|i| Ok(r.string_at(refs + i * 8)?.unwrap_or_default()))
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    // Sampler names in the group come in texture order; each entry starts
    // with its GX2 sampler.
    let samplers: Vec<(String, [u32; 3])> = r
        .index_group(at + 0x30)?
        .into_iter()
        .map(|(name, sampler)| {
            Ok((
                name,
                [r.u32(sampler)?, r.u32(sampler + 4)?, r.u32(sampler + 8)?],
            ))
        })
        .collect::<Result<_>>()?;
    let textures = samplers
        .iter()
        .map(|(name, _)| name.clone())
        .zip(texture_names)
        .collect();

    let render_info = r
        .index_group(at + 0x1C)?
        .into_iter()
        .map(|(key, info)| {
            let count = r.u16(info)? as usize;
            let kind = r.u8(info + 2)?;
            let values: Vec<String> = (0..count)
                .map(|k| {
                    let v = info + 8 + 4 * k;
                    Ok(match kind {
                        0 => r.i32(v)?.to_string(),
                        1 => r.f32(v)?.to_string(),
                        _ => r.string_at(v)?.unwrap_or_default(),
                    })
                })
                .collect::<Result<_>>()?;
            Ok((key, values.join(",")))
        })
        .collect::<Result<_>>()?;

    let (mut shader_archive, mut shading_model) = (String::new(), String::new());
    let (mut shader_options, mut sampler_assign) = (Vec::new(), Vec::new());
    if let Some(assign) = r.pointer(at + 0x24)? {
        shader_archive = r.string_at(assign)?.unwrap_or_default();
        shading_model = r.string_at(assign + 4)?.unwrap_or_default();
        let pairs = |group: usize| -> Result<Vec<(String, String)>> {
            r.index_group(group)?
                .into_iter()
                .map(|(k, v)| Ok((k, r.string(v)?)))
                .collect()
        };
        sampler_assign = pairs(assign + 0x14)?;
        shader_options = pairs(assign + 0x18)?;
    }
    let render_state = match r.pointer(at + 0x20)? {
        Some(state) => {
            let polygon = r.u32(state + 4)?;
            let alpha = r.u32(state + 0x0C)?;
            RenderState {
                mode: r.u32(state)? & 3,
                cull_front: polygon & 1 != 0,
                cull_back: polygon & 2 != 0,
                alpha_test: (alpha & 8 != 0).then_some(r.f32(state + 0x10)?),
                blend: [
                    r.u32(state + 0x14)?,
                    r.u32(state + 0x18)?,
                    r.u32(state + 0x1C)?,
                ],
            }
        }
        None => RenderState::default(),
    };
    let param_data = r.pointer(at + 0x3C)?;
    let shader_params = match param_data {
        Some(data) => r
            .index_group(at + 0x38)?
            .into_iter()
            .map(|(name, param)| {
                let kind = r.u8(param)?;
                let size = r.u8(param + 1)? as usize;
                let offset = r.u16(param + 2)? as usize;
                let values = (0..size / 4)
                    .map(|k| {
                        let word = data + offset + 4 * k;
                        Ok(match kind {
                            12..=15 | 16.. => r.f32(word)?,
                            _ => r.u32(word)? as f32,
                        })
                    })
                    .collect::<Result<_>>()?;
                Ok(ShaderParam { name, kind, values })
            })
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    Ok(Material {
        name,
        textures,
        render_info,
        shader_archive,
        shading_model,
        shader_options,
        sampler_assign,
        samplers,
        render_state,
        shader_params,
    })
}
