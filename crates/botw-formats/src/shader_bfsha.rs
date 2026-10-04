//! Bounded Wii U FSHA 4.5.0.4 metadata, variant keys and GX2 code reader.
//! Switch BFSHA and other versions are explicitly unsupported. No GPU semantics
//! or runtime material selection is implied by parsing these records.

use crate::{FormatError, Result};
use std::ops::Range;

fn error(message: &'static str) -> FormatError {
    FormatError::Invalid(message)
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn bytes(&self, start: usize, size: usize) -> Result<&'a [u8]> {
        let end = start
            .checked_add(size)
            .ok_or_else(|| error("BFSHA range overflow"))?;
        self.0
            .get(start..end)
            .ok_or_else(|| error("BFSHA range outside file"))
    }
    fn u16(&self, p: usize) -> Result<u16> {
        Ok(u16::from_be_bytes(self.bytes(p, 2)?.try_into().unwrap()))
    }
    fn u32(&self, p: usize) -> Result<u32> {
        Ok(u32::from_be_bytes(self.bytes(p, 4)?.try_into().unwrap()))
    }
    fn byte(&self, p: usize) -> Result<u8> {
        Ok(self.bytes(p, 1)?[0])
    }
    fn pointer(&self, p: usize) -> Result<Option<usize>> {
        let displacement = self.u32(p)? as i32;
        if displacement == 0 {
            return Ok(None);
        }
        let target = p
            .checked_add_signed(displacement as isize)
            .ok_or_else(|| error("BFSHA relative pointer overflow"))?;
        self.bytes(target, 1)?;
        Ok(Some(target))
    }
    fn required(&self, p: usize) -> Result<usize> {
        self.pointer(p)?
            .ok_or_else(|| error("BFSHA required pointer is null"))
    }
    fn name(&self, p: usize) -> Result<&'a str> {
        let start = self.required(p)?;
        let tail = self
            .0
            .get(start..)
            .ok_or_else(|| error("BFSHA string outside file"))?;
        let end = tail
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(|| error("BFSHA unterminated string"))?;
        std::str::from_utf8(&tail[..end]).map_err(|_| error("BFSHA non-UTF8 string"))
    }
    fn dictionary(&self, p: usize) -> Result<Vec<(&'a str, Option<usize>)>> {
        let Some(start) = self.pointer(p)? else {
            return Ok(Vec::new());
        };
        let size = self.u32(start)? as usize;
        let count = self.u32(start + 4)? as usize;
        let expected = count
            .checked_add(1)
            .and_then(|n| n.checked_mul(16))
            .and_then(|n| n.checked_add(8))
            .ok_or_else(|| error("BFSHA dictionary count overflow"))?;
        if expected != size {
            return Err(error("BFSHA dictionary size/count mismatch"));
        }
        self.bytes(start, size)?;
        let mut entries = Vec::new();
        for i in 0..count {
            let node = start + 24 + i * 16;
            entries.push((self.name(node + 8)?, self.pointer(node + 12)?));
        }
        Ok(entries)
    }
}

#[derive(Debug)]
pub struct BfshaArchive<'a> {
    pub name: &'a str,
    pub models: Vec<ShaderModel<'a>>,
}
#[derive(Debug)]
pub struct ShaderModel<'a> {
    pub offset: usize,
    pub name: &'a str,
    pub static_key_words: u8,
    pub dynamic_key_words: u8,
    pub static_options: Vec<ShaderOption<'a>>,
    pub dynamic_options: Vec<ShaderOption<'a>>,
    pub attributes: Vec<NamedRecord<'a>>,
    pub samplers: Vec<NamedRecord<'a>>,
    pub uniform_blocks: Vec<UniformBlock<'a>>,
    pub programs: Vec<BfshaProgram<'a>>,
}
#[derive(Debug)]
pub struct ShaderOption<'a> {
    pub name: &'a str,
    pub default_choice: u8,
    pub flags: u8,
    pub key_offset: u8,
    pub word_index: u8,
    pub shift: u8,
    pub mask: u32,
    pub choices: Vec<&'a str>,
}
impl<'a> ShaderOption<'a> {
    /// Decode the resource's choice index from a complete static+dynamic key.
    pub fn choice(&self, key: &[u32]) -> Result<&'a str> {
        let word = key
            .get(self.word_index as usize)
            .ok_or_else(|| error("BFSHA option word outside variant key"))?;
        let index = (word & self.mask)
            .checked_shr(self.shift as u32)
            .ok_or_else(|| error("BFSHA option shift exceeds word width"))?
            as usize;
        self.choices
            .get(index)
            .copied()
            .ok_or_else(|| error("BFSHA key selects nonexistent option choice"))
    }
}
/// Named native reflection record, retained without assuming type enum semantics.
#[derive(Debug)]
pub struct NamedRecord<'a> {
    pub name: &'a str,
    pub offset: usize,
    pub data: &'a [u8],
}
#[derive(Debug)]
pub struct UniformBlock<'a> {
    pub record: NamedRecord<'a>,
    pub uniforms: Vec<NamedRecord<'a>>,
}
#[derive(Debug)]
pub struct BfshaProgram<'a> {
    pub offset: usize,
    pub flags: u16,
    pub key: Vec<u32>,
    /// VS, GS, geometry-copy, PS, compute. Only VS/PS code is decoded.
    pub stage_offsets: [Option<usize>; 5],
    pub vertex: Option<BfshaCode<'a>>,
    pub pixel: Option<BfshaCode<'a>>,
    /// Four signed byte locations per reflected binding: VS/GS/PS/compute.
    pub sampler_locations: &'a [u8],
    pub block_locations: &'a [u8],
}
#[derive(Debug)]
pub struct BfshaCode<'a> {
    pub header_offset: usize,
    pub range: Range<usize>,
    pub bytes: &'a [u8],
}

fn named_records<'a>(
    r: &Reader<'a>,
    field: usize,
    size: usize,
    count: usize,
) -> Result<Vec<NamedRecord<'a>>> {
    let entries = r.dictionary(field)?;
    if entries.len() != count {
        return Err(error("BFSHA reflected count mismatch"));
    }
    entries
        .into_iter()
        .map(|(name, offset)| {
            let offset = offset.ok_or_else(|| error("BFSHA null reflection record"))?;
            Ok(NamedRecord {
                name,
                offset,
                data: r.bytes(offset, size)?,
            })
        })
        .collect()
}
fn options<'a>(r: &Reader<'a>, field: usize, count: usize) -> Result<Vec<ShaderOption<'a>>> {
    let records = named_records(r, field, 24, count)?;
    records
        .into_iter()
        .map(|record| {
            let p = record.offset;
            let choices = r.dictionary(p + 16)?;
            if choices.len() != record.data[0] as usize
                || record.data[1] as usize >= choices.len()
                || record.data[7] >= 32
            {
                return Err(error("BFSHA invalid option choices/default/shift"));
            }
            Ok(ShaderOption {
                name: record.name,
                default_choice: record.data[1],
                flags: record.data[4],
                key_offset: record.data[5],
                word_index: record.data[6],
                shift: record.data[7],
                mask: r.u32(p + 8)?,
                choices: choices.into_iter().map(|(name, _)| name).collect(),
            })
        })
        .collect()
}
fn code<'a>(r: &Reader<'a>, start: Option<usize>, vertex: bool) -> Result<Option<BfshaCode<'a>>> {
    let Some(start) = start else {
        return Ok(None);
    };
    let (header_size, size_field) = if vertex { (0x134, 0xd0) } else { (0xe8, 0xa4) };
    r.bytes(start, header_size)?;
    let size = r.u32(start + size_field)? as usize;
    let offset = r.required(start + size_field + 4)?;
    if size == 0
        || size % 8 != 0
        || (offset < start + header_size && offset.checked_add(size).is_some_and(|end| end > start))
    {
        return Err(error("BFSHA invalid GX2 program range"));
    }
    let bytes = r.bytes(offset, size)?;
    Ok(Some(BfshaCode {
        header_offset: start,
        range: offset..offset + size,
        bytes,
    }))
}

impl<'a> BfshaArchive<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let r = Reader(bytes);
        if r.bytes(0, 4)? != b"FSHA"
            || r.u32(4)? != 0x04050004
            || r.u16(8)? != 0xfeff
            || r.u16(10)? != 0x10
        {
            return Err(error("BFSHA: only Wii U FSHA 4.5.0.4 BE supported"));
        }
        if r.u32(12)? as usize != bytes.len() {
            return Err(error("BFSHA file size mismatch"));
        }
        let name = r.name(0x14)?;
        let entries = r.dictionary(0x2c)?;
        if entries.len() != r.u16(0x24)? as usize {
            return Err(error("BFSHA model count mismatch"));
        }
        let mut models = Vec::new();
        for (name, offset) in entries {
            let p = offset.ok_or_else(|| error("BFSHA null model"))?;
            r.bytes(p, 0x70)?;
            if r.name(p + 0x24)? != name {
                return Err(error("BFSHA model name mismatch"));
            }
            let static_key_words = r.byte(p)?;
            let dynamic_key_words = r.byte(p + 1)?;
            let key_words = static_key_words as usize + dynamic_key_words as usize;
            let count = r.u16(p + 6)? as usize;
            let static_options = options(&r, p + 0x30, r.u16(p + 2)? as usize)?;
            let dynamic_options = options(&r, p + 0x38, r.u16(p + 4)? as usize)?;
            let attributes = named_records(&r, p + 0x40, 4, r.byte(p + 8)? as usize)?;
            let samplers = named_records(&r, p + 0x48, 8, r.byte(p + 9)? as usize)?;
            let mut uniform_blocks = Vec::new();
            for record in named_records(&r, p + 0x50, 16, r.byte(p + 10)? as usize)? {
                let uniforms = named_records(
                    &r,
                    record.offset + 8,
                    16,
                    r.u16(record.offset + 4)? as usize,
                )?;
                uniform_blocks.push(UniformBlock { record, uniforms });
            }
            let program_start = if count == 0 { 0 } else { r.required(p + 0x58)? };
            r.bytes(program_start, count * 0x4c)?;
            let key_start = if count * key_words == 0 {
                0
            } else {
                r.required(p + 0x5c)?
            };
            r.bytes(key_start, count * key_words * 4)?;
            let mut programs = Vec::new();
            for i in 0..count {
                let q = program_start + i * 0x4c;
                if r.required(q + 0x48)? != p {
                    return Err(error("BFSHA program parent mismatch"));
                }
                let key = (0..key_words)
                    .map(|j| r.u32(key_start + (i * key_words + j) * 4))
                    .collect::<Result<Vec<_>>>()?;
                for option in static_options.iter().chain(&dynamic_options) {
                    option.choice(&key)?;
                }
                let mut stage_offsets = [None; 5];
                for (j, slot) in stage_offsets.iter_mut().enumerate() {
                    *slot = r.pointer(q + 0x34 + j * 4)?;
                }
                let locations = |field, expected| -> Result<&'a [u8]> {
                    if expected == 0 {
                        return Ok(&[]);
                    }
                    r.bytes(r.required(field)?, expected * 4)
                };
                if r.byte(q + 2)? as usize != samplers.len()
                    || r.byte(q + 3)? as usize != uniform_blocks.len()
                {
                    return Err(error("BFSHA program reflection count mismatch"));
                }
                programs.push(BfshaProgram {
                    offset: q,
                    flags: r.u16(q)?,
                    key,
                    vertex: code(&r, stage_offsets[0], true)?,
                    pixel: code(&r, stage_offsets[3], false)?,
                    stage_offsets,
                    sampler_locations: locations(q + 0x2c, samplers.len())?,
                    block_locations: locations(q + 0x30, uniform_blocks.len())?,
                });
            }
            models.push(ShaderModel {
                offset: p,
                name,
                static_key_words,
                dynamic_key_words,
                static_options,
                dynamic_options,
                attributes,
                samplers,
                uniform_blocks,
                programs,
            });
        }
        Ok(Self { name, models })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set(b: &mut [u8], p: usize, v: u32) {
        b[p..p + 4].copy_from_slice(&v.to_be_bytes());
    }
    fn pointer(b: &mut [u8], p: usize, target: usize) {
        set(b, p, (target as i32 - p as i32) as u32);
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 0x2a8];
        b[..4].copy_from_slice(b"FSHA");
        set(&mut b, 4, 0x04050004);
        set(&mut b, 8, 0xfeff0010);
        set(&mut b, 12, 0x2a8);
        pointer(&mut b, 0x14, 0x70);
        b[0x70..0x75].copy_from_slice(b"test\0");
        b[0x78..0x7e].copy_from_slice(b"model\0");
        b[0x25] = 1;
        pointer(&mut b, 0x2c, 0x40);
        set(&mut b, 0x40, 40);
        set(&mut b, 0x44, 1);
        pointer(&mut b, 0x60, 0x78);
        pointer(&mut b, 0x64, 0x90);
        b[0x90] = 1;
        b[0x97] = 1;
        pointer(&mut b, 0xb4, 0x78);
        pointer(&mut b, 0xe8, 0x100);
        pointer(&mut b, 0xec, 0x150);
        pointer(&mut b, 0x134, 0x160);
        pointer(&mut b, 0x148, 0x90);
        set(&mut b, 0x230, 8);
        pointer(&mut b, 0x234, 0x2a0);
        b[0x2a0..].copy_from_slice(b"own-code");
        b
    }
    #[test]
    fn extracts_program_keys_and_original_gpu_bytes() {
        let b = fixture();
        let a = BfshaArchive::parse(&b).unwrap();
        assert_eq!(a.name, "test");
        assert_eq!(a.models[0].name, "model");
        let p = &a.models[0].programs[0];
        assert_eq!(p.key, [0]);
        let code = p.vertex.as_ref().unwrap();
        assert_eq!(code.bytes, b"own-code");
        assert_eq!(code.range, 0x2a0..0x2a8);
        assert!(p.pixel.is_none());
    }
    #[test]
    fn rejects_truncation_bad_dictionary_parent_and_code_ranges() {
        let b = fixture();
        for size in 0..b.len() {
            assert!(BfshaArchive::parse(&b[..size]).is_err());
        }
        for (offset, value) in [
            (0x44, u32::MAX),
            (0x40, 24),
            (0x134, 0),
            (0x148, 0),
            (0x148, 4),
            (0x234, 0),
            (0x234, i32::MAX as u32),
            (0x230, 9),
            (0x230, u32::MAX),
        ] {
            let mut bad = b.clone();
            set(&mut bad, offset, value);
            // A null optional VS is a supported missing stage, not corruption.
            if offset == 0x134 {
                assert!(
                    BfshaArchive::parse(&bad).unwrap().models[0].programs[0]
                        .vertex
                        .is_none()
                );
            } else {
                assert!(BfshaArchive::parse(&bad).is_err(), "offset {offset:#x}");
            }
        }
    }

    #[test]
    fn relative_offsets_support_backward_references_but_not_null_or_overflow() {
        let r = Reader(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xfc, 0x7f, 0xff, 0xff, 0xff]);
        assert_eq!(r.pointer(4).unwrap(), Some(0));
        assert!(r.required(0).is_err());
        assert!(r.pointer(8).is_err());
    }
    #[test]
    fn option_choice_uses_mask_shift_and_global_key_index() {
        let option = ShaderOption {
            name: "mode",
            default_choice: 0,
            flags: 0,
            key_offset: 1,
            word_index: 1,
            shift: 4,
            mask: 0x30,
            choices: vec!["off", "on"],
        };
        assert_eq!(option.choice(&[0xffff, 0x110]).unwrap(), "on");
        assert!(option.choice(&[0]).is_err());
        assert!(option.choice(&[0, 0x20]).is_err());
    }
    #[test]
    fn rejects_wrong_platform_version_and_size() {
        for n in 0..64 {
            assert!(BfshaArchive::parse(&vec![0; n]).is_err());
        }
        let mut b = vec![0; 64];
        b[..4].copy_from_slice(b"FSHA");
        b[4..8].copy_from_slice(&0x04050004u32.to_be_bytes());
        b[8..12].copy_from_slice(&[0xfe, 0xff, 0, 0x10]);
        assert!(BfshaArchive::parse(&b).is_err());
    }
}
