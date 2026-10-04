//! PTCL effect files, the Wii U version ("EFTB", `Effect/*.sesetlist`, and
//! `Effect/GameResident.sesetlist` in `Pack/Bootup.pack`): emitter sets,
//! their emitters, textures and primitive meshes.
//!
//! The file is a tree of nodes. Every node starts with a 32-byte header:
//! magic, `u32` size, then offsets relative to the node's start: first
//! child, next sibling, first attribute (0xFFFFFFFF = none), data; `u32`
//! padding, `u16` child count, `u16` 1. After the 0x30-byte file header the
//! top-level nodes are siblings: `ESTA` (emitter sets), `TEXA` (textures),
//! `PRMA` (primitives) and `SHDA` (GX2 shaders, not decoded).
//!
//! Layout notes are in archived FORMATS.md notes; offsets were checked against
//! `MountainCloud.sesetlist` and `GameResident.sesetlist` from the dump.

pub mod emitter;
pub mod files;

use crate::bfres::{self, gx2};
use crate::{FormatError, Result};
pub use emitter::Emitter;

/// The only version this reader knows: BotW on Wii U.
pub const VERSION: u32 = 20;

/// A parsed effect file.
#[derive(Clone, Debug)]
pub struct Ptcl<'a> {
    pub version: u32,
    pub name: String,
    pub emitter_sets: Vec<EmitterSet>,
    pub textures: Vec<Texture<'a>>,
    pub primitives: Vec<Primitive>,
    /// Size of the GX2 shader binary (`SHDB`), which is not decoded.
    pub shader_bytes: usize,
}

/// A named group of emitters: what the game spawns as one effect.
#[derive(Clone, Debug)]
pub struct EmitterSet {
    pub name: String,
    pub emitters: Vec<Emitter>,
}

impl EmitterSet {
    /// Every emitter of the set, children included, depth first.
    pub fn all_emitters(&self) -> Vec<&Emitter> {
        fn walk<'e>(list: &'e [Emitter], out: &mut Vec<&'e Emitter>) {
            for e in list {
                out.push(e);
                walk(&e.children, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.emitters, &mut out);
        out
    }
}

/// A texture (`TEXR`) with its GX2 image data (`GX2B`): level 0, then the
/// other mip levels.
#[derive(Clone, Debug)]
pub struct Texture<'a> {
    /// What emitters' samplers refer to.
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    /// GX2 surface format.
    pub format: u32,
    pub tile_mode: u32,
    /// GX2 `swizzle` as computed for the surface: bits 8–10 pipe and bank
    /// swizzle, bits 16+ the number of macro-tiled mip levels.
    pub swizzle: u32,
    /// GX2 component selection: the source of R, G, B, A (0–3 a channel,
    /// 4 zero, 5 one).
    pub component_select: [u8; 4],
    pub data: &'a [u8],
}

impl Texture<'_> {
    /// The GX2 surface, with the layout GX2 computes for it (effect files
    /// store only size, format and tiling).
    pub fn surface(&self) -> gx2::Surface {
        gx2::Surface::computed(self.width, self.height, self.mip_count, self.format, self.tile_mode, self.swizzle)
    }

    /// The texture untiled and ready for the GPU, every mip level included.
    pub fn image(&self) -> Result<bfres::TextureImage> {
        let surface = self.surface();
        if surface.format().bits_per_element().is_none() {
            return Err(FormatError::Invalid("gx2: unsupported surface format"));
        }
        let image_size = surface.image_size as usize;
        if self.data.len() < image_size {
            return Err(FormatError::Invalid("ptcl: texture data shorter than its image"));
        }
        let mips = self.data.get(surface.mip_offsets[0] as usize..).unwrap_or_default();
        let texture = bfres::Texture {
            name: format!("{:08x}", self.id),
            surface,
            image: &self.data[..image_size],
            mips,
            user_data: Vec::new(),
            component_select: self.component_select,
            // `assemble_texture` reads mips from here (the Tex2 quirk).
            at_image_pointer_mips: mips,
        };
        let with_mips = (self.mip_count > 1).then_some(&texture);
        bfres::assemble_texture(&texture, with_mips)
    }
}

/// A mesh particles can be drawn with (`PRIM`). Attributes the primitive
/// lacks are empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Primitive {
    /// What emitters refer to.
    pub id: u64,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    /// Tangents with the bitangent sign in `w`.
    pub tangents: Vec<[f32; 4]>,
    pub colors: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// A node header.
#[derive(Clone, Copy, Debug)]
pub struct Node {
    pub magic: [u8; 4],
    /// Where the node starts in the file.
    pub offset: usize,
    pub size: u32,
    pub child: Option<usize>,
    pub next: Option<usize>,
    pub attribute: Option<usize>,
    /// Where the node's data starts in the file.
    pub data: usize,
    pub child_count: u16,
}

impl Node {
    pub fn read(r: &Reader, at: usize) -> Result<Self> {
        // Links only point forward; one to the node itself would loop.
        let link = |i: usize| -> Result<Option<usize>> {
            match r.u32(at + i)? {
                u32::MAX => Ok(None),
                0 => Err(FormatError::Invalid("ptcl: node links to itself")),
                v => Ok(Some(at + v as usize)),
            }
        };
        Ok(Self {
            magic: r.slice(at, 4)?.try_into().unwrap(),
            offset: at,
            size: r.u32(at + 4)?,
            child: link(8)?,
            next: link(12)?,
            attribute: link(16)?,
            data: at + r.u32(at + 20)? as usize,
            child_count: r.u16(at + 28)?,
        })
    }

    pub fn magic_str(&self) -> String {
        String::from_utf8_lossy(&self.magic).into_owned()
    }

    /// The node at `first` and the siblings after it.
    pub fn siblings(r: &Reader, first: Option<usize>) -> Result<Vec<Node>> {
        let mut out: Vec<Node> = Vec::new();
        let mut at = first;
        while let Some(offset) = at {
            // Siblings always follow each other; this also stops cycles.
            if out.last().is_some_and(|prev| offset <= prev.offset) {
                return Err(FormatError::Invalid("ptcl: sibling offsets go backwards"));
            }
            let node = Node::read(r, offset)?;
            at = node.next;
            out.push(node);
        }
        Ok(out)
    }

    pub fn children(&self, r: &Reader) -> Result<Vec<Node>> {
        Node::siblings(r, self.child)
    }
}

impl<'a> Ptcl<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let r = Reader(data);
        if r.slice(0, 4)? != b"EFTB" {
            return Err(FormatError::Invalid("ptcl: not an EFTB effect file"));
        }
        let version = r.u32(4)?;
        if version != VERSION {
            return Err(FormatError::Invalid("ptcl: only EFTB version 20 (Wii U BotW) is supported"));
        }
        let mut ptcl = Ptcl {
            version,
            name: r.string(8, 0x20)?,
            emitter_sets: Vec::new(),
            textures: Vec::new(),
            primitives: Vec::new(),
            shader_bytes: 0,
        };
        for top in Node::siblings(&r, Some(0x30))? {
            match &top.magic {
                b"ESTA" => {
                    for set in top.children(&r)? {
                        let emitters = set.children(&r)?.iter().map(|n| Emitter::read(&r, n)).collect::<Result<_>>()?;
                        ptcl.emitter_sets.push(EmitterSet { name: r.string(set.data + 0x10, 0x40)?, emitters });
                    }
                }
                b"TEXA" => {
                    for tex in top.children(&r)? {
                        ptcl.textures.push(read_texture(&r, &tex)?);
                    }
                }
                b"PRMA" => {
                    for prim in top.children(&r)? {
                        ptcl.primitives.push(read_primitive(&r, &prim)?);
                    }
                }
                b"SHDA" => ptcl.shader_bytes = top.children(&r)?.iter().map(|n| n.size as usize).sum(),
                _ => {}
            }
        }
        Ok(ptcl)
    }

    pub fn emitter_set(&self, name: &str) -> Option<&EmitterSet> {
        self.emitter_sets.iter().find(|s| s.name == name)
    }

    pub fn texture(&self, id: u32) -> Option<&Texture<'a>> {
        self.textures.iter().find(|t| t.id == id)
    }

    pub fn primitive(&self, id: u64) -> Option<&Primitive> {
        self.primitives.iter().find(|p| p.id == id)
    }
}

/// `TEXR` data: `u16` width, height; `u32` depth, component selection, mip
/// count, GX2 format, tile mode, requested swizzle, computed swizzle, —,
/// id; `u8` the effect library's own format code; padding. The image is
/// the `GX2B` child. `TEXA` lists all `TEXR`s first, then their `GX2B`s.
fn read_texture<'a>(r: &Reader<'a>, node: &Node) -> Result<Texture<'a>> {
    let d = node.data;
    let data = match node.child.map(|at| Node::read(r, at)).transpose()? {
        Some(b) if &b.magic == b"GX2B" => r.slice(b.data, b.size as usize)?,
        _ => &[],
    };
    Ok(Texture {
        id: r.u32(d + 0x24)?,
        width: r.u16(d)?.into(),
        height: r.u16(d + 2)?.into(),
        mip_count: r.u32(d + 0x0C)?,
        format: r.u32(d + 0x10)?,
        tile_mode: r.u32(d + 0x14)?,
        swizzle: r.u32(d + 0x1C)?,
        component_select: r.u32(d + 8)?.to_be_bytes(),
        data,
    })
}

/// `PRIM` data: `u64` id; six `(u32 count, u32 components)` pairs for
/// position, normal, tangent, colour, uv and a second uv (counted but never
/// stored); `u32` index count; offsets (from the data start) of the
/// position, normal, tangent, colour, uv and index arrays (0 when absent).
/// Vertex attributes are stored as four floats each, indices as `u32`.
fn read_primitive(r: &Reader, node: &Node) -> Result<Primitive> {
    let d = node.data;
    let index_count = r.u32(d + 0x38)? as usize;
    let offset = |i: usize| r.u32(d + 0x3C + 4 * i).map(|o| d + o as usize);
    let vec4s = |attribute: usize| -> Result<Vec<[f32; 4]>> {
        let n = r.u32(d + 8 + 8 * attribute)? as usize;
        let at = offset(attribute)?;
        r.slice(at, 16 * n)?; // bounds-check once
        (0..n).map(|v| r.f32s(at + 16 * v)).collect()
    };
    let xyz = |v: Vec<[f32; 4]>| v.into_iter().map(|[x, y, z, _]| [x, y, z]).collect();
    let index_at = offset(5)?;
    r.slice(index_at, 4 * index_count)?;
    Ok(Primitive {
        id: r.u64(d)?,
        positions: xyz(vec4s(0)?),
        normals: xyz(vec4s(1)?),
        tangents: vec4s(2)?,
        colors: vec4s(3)?,
        uvs: vec4s(4)?.into_iter().map(|[u, v, _, _]| [u, v]).collect(),
        indices: (0..index_count).map(|i| r.u32(index_at + 4 * i)).collect::<Result<_>>()?,
    })
}

/// Bounds-checked big-endian reads over the whole file.
#[derive(Clone, Copy)]
pub struct Reader<'a>(pub &'a [u8]);

impl<'a> Reader<'a> {
    pub fn slice(&self, at: usize, len: usize) -> Result<&'a [u8]> {
        at.checked_add(len)
            .and_then(|end| self.0.get(at..end))
            .ok_or(FormatError::Invalid("ptcl: read past the end of the file"))
    }

    pub fn u8(&self, at: usize) -> Result<u8> {
        Ok(self.slice(at, 1)?[0])
    }

    pub fn u16(&self, at: usize) -> Result<u16> {
        Ok(u16::from_be_bytes(self.slice(at, 2)?.try_into().unwrap()))
    }

    pub fn u32(&self, at: usize) -> Result<u32> {
        Ok(u32::from_be_bytes(self.slice(at, 4)?.try_into().unwrap()))
    }

    pub fn i32(&self, at: usize) -> Result<i32> {
        Ok(self.u32(at)? as i32)
    }

    pub fn u64(&self, at: usize) -> Result<u64> {
        Ok(u64::from_be_bytes(self.slice(at, 8)?.try_into().unwrap()))
    }

    pub fn f32(&self, at: usize) -> Result<f32> {
        Ok(f32::from_bits(self.u32(at)?))
    }

    pub fn f32s<const N: usize>(&self, at: usize) -> Result<[f32; N]> {
        let mut out = [0.0; N];
        for (i, v) in out.iter_mut().enumerate() {
            *v = self.f32(at + 4 * i)?;
        }
        Ok(out)
    }

    /// A string in a fixed `len`-byte field, up to the first NUL.
    pub fn string(&self, at: usize, len: usize) -> Result<String> {
        let field = self.slice(at, len)?;
        let end = field.iter().position(|&b| b == 0).unwrap_or(len);
        Ok(String::from_utf8_lossy(&field[..end]).into_owned())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Builds synthetic effect files node by node.
    #[derive(Default)]
    pub struct Builder {
        pub bytes: Vec<u8>,
    }

    impl Builder {
        pub fn new() -> Self {
            let mut bytes = vec![0; 0x30];
            bytes[..4].copy_from_slice(b"EFTB");
            bytes[4..8].copy_from_slice(&VERSION.to_be_bytes());
            bytes[8..12].copy_from_slice(b"Game");
            Self { bytes }
        }

        /// Appends a node with `data` (aligned to `align` bytes in the
        /// file) and returns its offset. Links are patched in later.
        pub fn node(&mut self, magic: &[u8; 4], data: &[u8], align: usize) -> usize {
            let at = self.bytes.len();
            let data_at = (at + 0x20).next_multiple_of(align);
            let mut header = [0xFFu8; 0x20];
            header[..4].copy_from_slice(magic);
            header[4..8].copy_from_slice(&(data.len() as u32).to_be_bytes());
            header[20..24].copy_from_slice(&((data_at - at) as u32).to_be_bytes());
            header[24..28].fill(0);
            header[28..32].copy_from_slice(&[0, 0, 0, 1]);
            self.bytes.extend_from_slice(&header);
            self.bytes.resize(data_at, 0);
            self.bytes.extend_from_slice(data);
            self.bytes.resize(self.bytes.len().next_multiple_of(4), 0);
            at
        }

        /// Sets the link at header byte `field` (8 child, 12 next, 16 attribute).
        pub fn link(&mut self, from: usize, field: usize, to: usize) {
            self.bytes[from + field..from + field + 4].copy_from_slice(&((to - from) as u32).to_be_bytes());
            if field == 8 {
                let count = u16::from_be_bytes([self.bytes[from + 28], self.bytes[from + 29]]) + 1;
                self.bytes[from + 28..from + 30].copy_from_slice(&count.to_be_bytes());
            }
        }
    }

    /// A 64-byte name field preceded by `before` bytes of zeros.
    pub fn named(before: usize, name: &str, total: usize) -> Vec<u8> {
        let mut data = vec![0; total];
        data[before..before + name.len()].copy_from_slice(name.as_bytes());
        data
    }

    fn texr(id: u32, width: u16, height: u16, format: u32, mips: u32) -> Vec<u8> {
        let mut d = vec![0u8; 0x30];
        d[0..2].copy_from_slice(&width.to_be_bytes());
        d[2..4].copy_from_slice(&height.to_be_bytes());
        d[4..8].copy_from_slice(&1u32.to_be_bytes());
        d[8..12].copy_from_slice(&[0, 0, 0, 1]);
        d[0x0C..0x10].copy_from_slice(&mips.to_be_bytes());
        d[0x10..0x14].copy_from_slice(&format.to_be_bytes());
        d[0x14..0x18].copy_from_slice(&4u32.to_be_bytes());
        d[0x1C..0x20].copy_from_slice(&0x10000u32.to_be_bytes());
        d[0x24..0x28].copy_from_slice(&id.to_be_bytes());
        d
    }

    /// Two sets (the first with a parent and a child emitter and an
    /// attribute) and one texture.
    pub fn sample_file(emitter_data: impl Fn(&str) -> Vec<u8>, gx2: &[u8]) -> Vec<u8> {
        let mut b = Builder::new();
        let esta = b.node(b"ESTA", &[], 4);
        let set_a = b.node(b"ESET", &named(0x10, "Cloud_Set", 0x60), 4);
        let parent = b.node(b"EMTR", &emitter_data("Parent"), 0x100);
        let attribute = b.node(b"CSDP", &[0; 0x10], 4);
        let child = b.node(b"EMTR", &emitter_data("Child"), 0x100);
        let set_b = b.node(b"ESET", &named(0x10, "Other", 0x60), 4);
        let lone = b.node(b"EMTR", &emitter_data("Lone"), 0x100);
        let texa = b.node(b"TEXA", &[], 4);
        let texr = b.node(b"TEXR", &texr(0xC10D, 64, 64, 0x34, 1), 4);
        let gx2b = b.node(b"GX2B", gx2, 0x100);
        b.link(0x30, 12, texa);
        b.link(esta, 8, set_a);
        b.link(set_a, 12, set_b);
        b.link(set_a, 8, parent);
        b.link(parent, 16, attribute);
        b.link(parent, 8, child);
        b.link(set_b, 8, lone);
        b.link(texa, 8, texr);
        b.link(texr, 8, gx2b);
        b.bytes
    }

    #[test]
    fn reads_the_node_tree() {
        let bytes = sample_file(emitter::tests::sample, &[0xAB; 64]);
        let ptcl = Ptcl::parse(&bytes).unwrap();
        assert_eq!(ptcl.name, "Game");
        let names: Vec<_> = ptcl.emitter_sets.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Cloud_Set", "Other"]);
        let set = ptcl.emitter_set("Cloud_Set").unwrap();
        let all: Vec<_> = set.all_emitters().iter().map(|e| e.name.as_str()).collect();
        assert_eq!(all, ["Parent", "Child"]);
        assert_eq!(set.emitters[0].attributes, ["CSDP"]);
        assert_eq!(set.emitters[0].data_offset % 0x100, 0);
        let texture = ptcl.texture(0xC10D).unwrap();
        assert_eq!((texture.width, texture.height, texture.format, texture.component_select), (64, 64, 0x34, [0, 0, 0, 1]));
        assert_eq!(texture.data, &[0xAB; 64]);
    }

    #[test]
    fn decodes_texture_mips() {
        // Linear RGBA8, 64² with one mip: level 0 at pitch 64, level 1
        // (32²) at the next 256-byte boundary, also at pitch 64.
        let mut gx2 = Vec::new();
        for y in 0..64u32 {
            for x in 0..64u32 {
                gx2.extend_from_slice(&[x as u8, y as u8, 7, 255]);
            }
        }
        for y in 0..32u32 {
            for x in 0..64u32 {
                gx2.extend_from_slice(&[x as u8, y as u8, 9, 255]);
            }
        }
        let mut bytes = sample_file(emitter::tests::sample, &gx2);
        // Make the sample texture linear RGBA8 with two levels.
        let texr = bytes.windows(4).position(|w| w == b"TEXR").unwrap() + 0x20;
        bytes[texr + 0x0C..texr + 0x10].copy_from_slice(&2u32.to_be_bytes());
        bytes[texr + 0x10..texr + 0x14].copy_from_slice(&0x1Au32.to_be_bytes());
        bytes[texr + 0x14..texr + 0x18].copy_from_slice(&1u32.to_be_bytes());
        bytes[texr + 0x1C..texr + 0x20].copy_from_slice(&0u32.to_be_bytes());
        let ptcl = Ptcl::parse(&bytes).unwrap();
        let image = ptcl.texture(0xC10D).unwrap().image().unwrap();
        assert_eq!((image.width, image.height, image.mip_levels), (64, 64, 2));
        let level0 = image.level_data(0).unwrap();
        assert_eq!(&level0[(5 * 64 + 3) * 4..][..4], &[3, 5, 7, 255]);
        let level1 = image.level_data(1).unwrap();
        assert_eq!(level1.len(), 32 * 32 * 4);
        assert_eq!(&level1[(31 * 32 + 31) * 4..][..4], &[31, 31, 9, 255]);
    }

    #[test]
    fn reads_primitives() {
        // A quad with positions, normals and uvs (no tangents or colours).
        let quad: [[f32; 4]; 4] = [[-0.5, -0.5, 0.0, 1.0], [0.5, -0.5, 0.0, 1.0], [0.5, 0.5, 0.0, 1.0], [-0.5, 0.5, 0.0, 1.0]];
        let mut d = vec![0u8; 0x60];
        d[..8].copy_from_slice(&0x9CC7_0E4Fu64.to_be_bytes());
        for (slot, components) in [(0, 3u32), (1, 3), (4, 2)] {
            d[8 + 8 * slot..12 + 8 * slot].copy_from_slice(&4u32.to_be_bytes());
            d[12 + 8 * slot..16 + 8 * slot].copy_from_slice(&components.to_be_bytes());
        }
        d[0x38..0x3C].copy_from_slice(&6u32.to_be_bytes());
        for (slot, at) in [(0, 0x60u32), (1, 0xA0), (4, 0xE0), (5, 0x120)] {
            d[0x3C + 4 * slot..0x40 + 4 * slot].copy_from_slice(&at.to_be_bytes());
        }
        let floats = |vs: &[[f32; 4]]| vs.iter().flatten().flat_map(|v| v.to_be_bytes()).collect::<Vec<u8>>();
        d.extend(floats(&quad));
        d.extend(floats(&[[0.0, 0.0, 1.0, 0.0]; 4]));
        d.extend(floats(&[[0.0, 1.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]]));
        d.extend([0u32, 1, 2, 0, 2, 3].iter().flat_map(|i| i.to_be_bytes()));

        let mut b = Builder::new();
        let prma = b.node(b"PRMA", &[], 4);
        let prim = b.node(b"PRIM", &d, 4);
        b.link(prma, 8, prim);
        let ptcl = Ptcl::parse(&b.bytes).unwrap();
        let p = ptcl.primitive(0x9CC7_0E4F).unwrap();
        assert_eq!(p.positions[2], [0.5, 0.5, 0.0]);
        assert_eq!(p.normals, [[0.0, 0.0, 1.0]; 4]);
        assert_eq!(p.uvs[1], [1.0, 1.0]);
        assert!(p.tangents.is_empty() && p.colors.is_empty());
        assert_eq!(p.indices, [0, 1, 2, 0, 2, 3]);
    }

    #[test]
    fn rejects_other_files() {
        assert!(Ptcl::parse(b"VFXB").is_err());
        let mut bytes = Builder::new().bytes;
        bytes[7] = 21;
        assert!(Ptcl::parse(&bytes).is_err());
        // A sibling link pointing back must not loop forever.
        let mut b = Builder::new();
        let esta = b.node(b"ESTA", &[], 4);
        b.bytes[esta + 12..esta + 16].copy_from_slice(&0u32.to_be_bytes());
        assert!(Ptcl::parse(&b.bytes).is_err());
        // Nor may an emitter that is its own child.
        let mut bytes = sample_file(emitter::tests::sample, &[]);
        let emtr = bytes.windows(4).position(|w| w == b"EMTR").unwrap();
        bytes[emtr + 8..emtr + 12].copy_from_slice(&0u32.to_be_bytes());
        assert!(Ptcl::parse(&bytes).is_err());
    }
}
