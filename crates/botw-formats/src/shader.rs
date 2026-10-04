//! Wii U SHARCB v9 archive metadata and unmodified GX2 program extraction.
//!
//! This is deliberately not a shader decompiler. The v9 layout is evidenced
//! by local Wii U archives; v8 and Switch containers are explicitly rejected.
//! See `docs/research/wiiu-shader-containers.md` for sources and limitations.

use std::ops::Range;

use crate::{FormatError, Result};

fn invalid(message: &'static str) -> FormatError {
    FormatError::Invalid(message)
}
fn slice(bytes: &[u8], offset: usize, size: usize) -> Result<&[u8]> {
    let end = offset
        .checked_add(size)
        .ok_or_else(|| invalid("SHARCB range overflow"))?;
    bytes
        .get(offset..end)
        .ok_or_else(|| invalid("SHARCB range outside enclosing record"))
}
fn word(bytes: &[u8], offset: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        slice(bytes, offset, 4)?.try_into().unwrap(),
    ))
}
fn string(bytes: &[u8]) -> Result<&str> {
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| invalid("SHARCB unterminated string"))?;
    std::str::from_utf8(&bytes[..end]).map_err(|_| invalid("SHARCB non-UTF8 string"))
}

/// Archive ranges refer to the original, decompressed input bytes.
#[derive(Debug)]
pub struct ShaderArchive<'a> {
    pub name: &'a str,
    pub binaries: Vec<ShaderBinary<'a>>,
    pub programs: Vec<ShaderProgram<'a>>,
}

#[derive(Debug)]
pub struct ShaderBinary<'a> {
    pub record_range: Range<usize>,
    /// 0: vertex, 1: pixel, 2: geometry; other stages remain opaque.
    pub stage: u32,
    /// v9 packed field at +8. It is NOT the v8 data offset.
    pub unknown_word: u32,
    pub data: &'a [u8],
    pub gx2: Option<Gx2Shader<'a>>,
}

#[derive(Debug)]
pub struct Gx2Shader<'a> {
    pub code_range: Range<usize>,
    pub code: &'a [u8],
    pub uniform_blocks: Vec<ShaderBinding<'a>>,
    pub uniforms: Vec<ShaderBinding<'a>>,
    pub samplers: Vec<ShaderBinding<'a>>,
    pub attributes: Vec<ShaderBinding<'a>>,
}

/// Raw reflected values preserve native types/locations without inventing units.
#[derive(Debug)]
pub struct ShaderBinding<'a> {
    pub name: &'a str,
    /// Remaining GX2 record words, after the name offset.
    pub words: Vec<u32>,
}

#[derive(Debug)]
pub struct ShaderProgram<'a> {
    pub record_range: Range<usize>,
    pub name: &'a str,
    pub kind: u32,
    pub base_index: usize,
    pub variations: Vec<ShaderVariation<'a>>,
    pub variation_count: usize,
    /// The two additional v9 arrays are validated structurally but not decoded.
    pub additional_arrays: [Range<usize>; 2],
}

#[derive(Debug)]
pub struct ShaderVariation<'a> {
    pub name: &'a str,
    pub values: Vec<&'a str>,
    pub symbol: &'a str,
}

/// Validates a size-prefixed array and returns its size-prefixed records.
fn records(bytes: &[u8]) -> Result<Vec<(usize, &[u8])>> {
    if word(bytes, 0)? as usize != bytes.len() {
        return Err(invalid("SHARCB array size mismatch"));
    }
    let count = word(bytes, 4)? as usize;
    if count > bytes.len().saturating_sub(8) / 4 {
        return Err(invalid("SHARCB array count exceeds size"));
    }
    let mut result = Vec::new();
    let mut pos = 8;
    for _ in 0..count {
        let size = word(bytes, pos)? as usize;
        if size < 4 {
            return Err(invalid("SHARCB empty record"));
        }
        result.push((pos, slice(bytes, pos, size)?));
        pos += size;
    }
    if pos != bytes.len() {
        return Err(invalid("SHARCB array has unclaimed bytes"));
    }
    Ok(result)
}
fn array_at(bytes: &[u8], pos: usize) -> Result<&[u8]> {
    slice(bytes, pos, word(bytes, pos)? as usize)
}

impl<'a> ShaderArchive<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.get(..4) != Some(b"BAHS") {
            return Err(invalid(
                "SHARCB: only little-endian Wii U BAHS is supported",
            ));
        }
        if word(bytes, 4)? != 9 {
            return Err(invalid("SHARCB: unsupported version (expected 9)"));
        }
        if word(bytes, 8)? as usize != bytes.len() {
            return Err(invalid("SHARCB file size mismatch"));
        }
        if word(bytes, 12)? != 5 || word(bytes, 16)? != 0 {
            return Err(invalid("SHARCB: unsupported v9 flags or resolved pointers"));
        }
        let names_size = word(bytes, 20)? as usize;
        let names = slice(bytes, 24, names_size)?;
        let name = string(names)?;
        let binary_start = 24 + names_size;
        let array = array_at(bytes, binary_start)?;
        let mut binaries = Vec::new();
        for (offset, record) in records(array)? {
            let stage = word(record, 4)?;
            let unknown_word = word(record, 8)?;
            let data = slice(record, 16, word(record, 12)? as usize)?;
            if data.len() + 16 != record.len() {
                return Err(invalid("SHARCB binary payload size mismatch"));
            }
            let start = binary_start + offset;
            let gx2 = match stage {
                0 | 1 => Some(parse_gx2(
                    data,
                    names,
                    stage,
                    start + 16,
                    bytes,
                    binary_start + 8..binary_start + array.len(),
                )?),
                _ => None,
            };
            binaries.push(ShaderBinary {
                record_range: start..start + record.len(),
                stage,
                unknown_word,
                data,
                gx2,
            });
        }
        // A deduplicated program must still fit one supported GX2 payload,
        // beyond that payload's native header (never archive/record headers).
        for binary in &binaries {
            if let Some(gx2) = &binary.gx2 {
                let fits_payload = binaries.iter().any(|owner| {
                    let header = match owner.stage {
                        0 => 0x134,
                        1 => 0xe8,
                        _ => return false,
                    };
                    gx2.code_range.start >= owner.record_range.start + 16 + header
                        && gx2.code_range.end <= owner.record_range.end
                });
                if !fits_payload {
                    return Err(invalid("GX2 code is not contained in a supported payload"));
                }
            }
        }
        let program_start = binary_start + array.len();
        let array = array_at(bytes, program_start)?;
        if program_start + array.len() != bytes.len() {
            return Err(invalid("SHARCB unexpected trailing data"));
        }
        let mut programs = Vec::new();
        for (offset, record) in records(array)? {
            let name_size = word(record, 4)? as usize;
            let name = string(slice(record, 16, name_size)?)?;
            let kind = word(record, 8)?;
            let base_index = word(record, 12)? as usize;
            let mut pos = 16 + name_size;
            let array = array_at(record, pos)?;
            let mut variations = Vec::new();
            let mut variation_count = 1usize;
            for (_, variation) in records(array)? {
                let name_size = word(variation, 4)? as usize;
                let count = word(variation, 8)? as usize;
                let symbol_size = word(variation, 12)? as usize;
                let name = string(slice(variation, 16, name_size)?)?;
                let mut vpos = 16 + name_size;
                if count == 0 || count > variation.len().saturating_sub(vpos) {
                    return Err(invalid("SHARCB invalid variation value count"));
                }
                let mut values = Vec::new();
                for _ in 0..count {
                    let value = string(&variation[vpos..])?;
                    vpos += value.len() + 1;
                    values.push(value);
                }
                let symbol = if symbol_size == 0 {
                    ""
                } else {
                    string(slice(variation, vpos, symbol_size)?)?
                };
                if vpos + symbol_size != variation.len() {
                    return Err(invalid("SHARCB variation size mismatch"));
                }
                variation_count = variation_count
                    .checked_mul(count)
                    .ok_or_else(|| invalid("SHARCB variation count overflow"))?;
                variations.push(ShaderVariation {
                    name,
                    values,
                    symbol,
                });
            }
            pos += array.len();
            let mut additional_arrays = [0..0, 0..0];
            let start = program_start + offset;
            for range in &mut additional_arrays {
                let array = array_at(record, pos)?;
                records(array)?;
                *range = start + pos..start + pos + array.len();
                pos += array.len();
            }
            if pos != record.len() {
                return Err(invalid("SHARCB unexpected program fields"));
            }
            // v9 kind 8 records are stage 3; their semantics are not decoded.
            let stages: &[u32] = match kind {
                3 => &[0, 1],
                7 => &[0, 1, 2],
                8 => &[3],
                _ => return Err(invalid("SHARCB unsupported program kind")),
            };
            let count = variation_count
                .checked_mul(stages.len())
                .ok_or_else(|| invalid("SHARCB binary count overflow"))?;
            let end = base_index
                .checked_add(count)
                .ok_or_else(|| invalid("SHARCB binary index overflow"))?;
            let selected = binaries
                .get(base_index..end)
                .ok_or_else(|| invalid("SHARCB program references missing binaries"))?;
            if selected
                .iter()
                .enumerate()
                .any(|(i, binary)| binary.stage != stages[i % stages.len()])
            {
                return Err(invalid("SHARCB program stage order mismatch"));
            }
            programs.push(ShaderProgram {
                record_range: start..start + record.len(),
                name,
                kind,
                base_index,
                variations,
                variation_count,
                additional_arrays,
            });
        }
        Ok(Self {
            name,
            binaries,
            programs,
        })
    }
}

fn bindings<'a>(
    data: &[u8],
    names: &'a [u8],
    field: usize,
    stride: usize,
) -> Result<Vec<ShaderBinding<'a>>> {
    let count = word(data, field)? as usize;
    let start = word(data, field + 4)? as usize;
    let size = count
        .checked_mul(stride)
        .ok_or_else(|| invalid("GX2 reflection count overflow"))?;
    let table = slice(data, start, size)?;
    table
        .chunks_exact(stride)
        .map(|record| {
            let name_offset = word(record, 0)? as usize;
            let name = string(
                names
                    .get(name_offset..)
                    .ok_or_else(|| invalid("GX2 name outside archive string table"))?,
            )?;
            let words = (4..stride)
                .step_by(4)
                .map(|pos| word(record, pos))
                .collect::<Result<_>>()?;
            Ok(ShaderBinding { name, words })
        })
        .collect()
}

fn parse_gx2<'a>(
    data: &'a [u8],
    names: &'a [u8],
    stage: u32,
    start: usize,
    archive: &'a [u8],
    binary_range: Range<usize>,
) -> Result<Gx2Shader<'a>> {
    // Native structure offsets from devkitPro/wut include/gx2/shaders.h.
    let (header_size, size_field, reflection) = if stage == 0 {
        (0x134, 0xd0, 0xdc)
    } else {
        (0xe8, 0xa4, 0xb0)
    };
    slice(data, 0, header_size)?;
    let code_size = word(data, size_field)? as usize;
    // v9 deduplicates code: signed offsets may point into an earlier binary.
    let relative = word(data, size_field + 4)? as i32;
    let code_start = start
        .checked_add_signed(relative as isize)
        .ok_or_else(|| invalid("GX2 program offset overflow"))?;
    let code_end = code_start
        .checked_add(code_size)
        .ok_or_else(|| invalid("GX2 program size overflow"))?;
    if code_start < binary_range.start
        || code_end > binary_range.end
        || code_size == 0
        || code_size % 8 != 0
        || (code_start >= start && code_start < start + header_size)
    {
        return Err(invalid("GX2 invalid program range/alignment"));
    }
    let code = slice(archive, code_start, code_size)?;
    Ok(Gx2Shader {
        code_range: code_start..code_end,
        code,
        uniform_blocks: bindings(data, names, reflection, 12)?,
        uniforms: bindings(data, names, reflection + 8, 20)?,
        samplers: bindings(data, names, reflection + 32, 12)?,
        attributes: if stage == 0 {
            bindings(data, names, reflection + 40, 16)?
        } else {
            Vec::new()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn set(bytes: &mut [u8], offset: usize, value: usize) {
        bytes[offset..offset + 4].copy_from_slice(&(value as u32).to_le_bytes());
    }
    fn fixture() -> Vec<u8> {
        // One vertex/pixel pair, no macros, all reflection tables empty.
        let mut b = vec![0; 32];
        b[..4].copy_from_slice(b"BAHS");
        set(&mut b, 4, 9);
        set(&mut b, 12, 5);
        set(&mut b, 20, 8);
        b[24..28].copy_from_slice(b"own\0");
        let begin = b.len();
        b.extend([0; 8]);
        set(&mut b, begin + 4, 2);
        for (stage, header, field) in [(0, 0x134, 0xd0), (1, 0xe8, 0xa4)] {
            let pos = b.len();
            b.resize(pos + 16 + header + 8, 0);
            set(&mut b, pos, 16 + header + 8);
            set(&mut b, pos + 4, stage);
            set(&mut b, pos + 12, header + 8);
            set(&mut b, pos + 16 + field, 8);
            set(&mut b, pos + 20 + field, header);
        }
        let size = b.len() - begin;
        set(&mut b, begin, size);
        let p = b.len();
        b.extend([0; 8]);
        set(&mut b, p + 4, 1);
        let q = b.len();
        b.extend([0; 20]);
        set(&mut b, q + 4, 4);
        set(&mut b, q + 8, 3);
        b[q + 16..q + 20].copy_from_slice(b"run\0");
        for _ in 0..3 {
            b.extend(8u32.to_le_bytes());
            b.extend([0; 4]);
        }
        let size = b.len() - q;
        set(&mut b, q, size);
        let size = b.len() - p;
        set(&mut b, p, size);
        let size = b.len();
        set(&mut b, 8, size);
        b
    }
    #[test]
    fn extracts_native_program_ranges() {
        let b = fixture();
        let a = ShaderArchive::parse(&b).unwrap();
        assert_eq!(a.name, "own");
        assert_eq!(a.programs[0].name, "run");
        assert_eq!(a.programs[0].variation_count, 1);
        for binary in a.binaries {
            let gx2 = binary.gx2.unwrap();
            assert_eq!(&b[gx2.code_range], gx2.code);
            assert_eq!(gx2.code.len(), 8);
        }
    }
    #[test]
    fn reads_deduplicated_code_from_previous_payload() {
        let mut b = fixture();
        let a = ShaderArchive::parse(&b).unwrap();
        let code_start = a.binaries[0].gx2.as_ref().unwrap().code_range.start;
        let pixel_start = a.binaries[1].record_range.start + 16;
        set(
            &mut b,
            pixel_start + 0xa8,
            (code_start as i32 - pixel_start as i32) as u32 as usize,
        );
        let a = ShaderArchive::parse(&b).unwrap();
        assert_eq!(
            a.binaries[0].gx2.as_ref().unwrap().code,
            a.binaries[1].gx2.as_ref().unwrap().code
        );
        assert_eq!(
            a.binaries[1].gx2.as_ref().unwrap().code_range.start,
            code_start
        );
        // A pointer into the previous GX2 header is not deduplicated code.
        set(
            &mut b,
            pixel_start + 0xa8,
            (56i32 - pixel_start as i32) as u32 as usize,
        );
        assert!(ShaderArchive::parse(&b).is_err());
    }

    #[test]
    fn reads_reflection_name_from_archive_table() {
        let mut data = vec![0; 20];
        set(&mut data, 0, 1);
        set(&mut data, 4, 8);
        set(&mut data, 8, 0);
        set(&mut data, 12, 7);
        set(&mut data, 16, 64);
        let names = b"own\0";
        let table = bindings(&data, names, 0, 12).unwrap();
        assert_eq!(table[0].name, "own");
        assert_eq!(table[0].words, [7, 64]);
        set(&mut data, 8, names.len());
        assert!(bindings(&data, names, 0, 12).is_err());
    }

    #[test]
    fn reads_macro_values_and_checks_binary_product() {
        let mut b = fixture();
        let p = ShaderArchive::parse(&b).unwrap().programs[0]
            .record_range
            .start;
        let v = p + 20;
        // One macro, one allowed value, so the existing VS/PS pair suffices.
        let mut record = vec![0; 22];
        set(&mut record, 0, 22);
        set(&mut record, 4, 2);
        set(&mut record, 8, 1);
        set(&mut record, 12, 2);
        record[16..].copy_from_slice(b"M\0X\0S\0");
        b.splice(v + 8..v + 8, record);
        set(&mut b, v, 30);
        set(&mut b, v + 4, 1);
        let size = b.len();
        set(&mut b, 8, size);
        set(&mut b, p - 8, size - p + 8);
        set(&mut b, p, size - p);
        let a = ShaderArchive::parse(&b).unwrap();
        let var = &a.programs[0].variations[0];
        assert_eq!((var.name, var.symbol), ("M", "S"));
        assert_eq!(var.values, ["X"]);
        set(&mut b, v + 8 + 8, 0);
        assert!(ShaderArchive::parse(&b).is_err());
    }

    #[test]
    fn rejects_truncation_and_unsupported_versions() {
        let b = fixture();
        for n in 0..b.len() {
            assert!(ShaderArchive::parse(&b[..n]).is_err());
        }
        for (offset, value) in [
            (4, 8),
            (12, 1),
            (16, 1),
            (20, usize::MAX),
            (36, usize::MAX),
            (40, 0),
        ] {
            let mut b = b.clone();
            set(&mut b, offset, value);
            assert!(ShaderArchive::parse(&b).is_err());
        }
    }
    #[test]
    fn rejects_ranges_outside_binary_and_missing_program_indices() {
        let b = fixture();
        let program = ShaderArchive::parse(&b).unwrap().programs[0]
            .record_range
            .start;
        for (offset, value) in [
            (40 + 16 + 0xd4, 1),
            (40 + 16 + 0xd4, i32::MAX as usize),
            (40 + 16 + 0xdc, usize::MAX),
            (program + 12, 2),
            (program + 8, 8),
        ] {
            let mut b = b.clone();
            set(&mut b, offset, value);
            assert!(ShaderArchive::parse(&b).is_err());
        }
    }
}
