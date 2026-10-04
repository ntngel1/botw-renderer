//! `effects/tables.ron`: the constant tables the effect library's CPU code
//! reads, taken from the game's executable instead of being written into
//! the repository. The executable is an RPL (`code/U-King.rpx`): a
//! big-endian ELF whose sections carry the flag `0x08000000` when they are
//! zlib-compressed (`u32` inflated size, then a zlib stream).
//!
//! Addresses are those of `U-King.rpx` v208 (the update); the base game's
//! executable is laid out differently and fails the checks below.
//! docs/research/eft-runtime.md §3.1.

use std::path::{Path, PathBuf};

use asset_format::effects::{CURL_NOISE_SIZE, EffectTables, TABLES};
use botw_formats::content::ContentRoots;

/// sead's sine table: 256 entries `sin, Δsin, cos, Δcos`.
const SIN_TABLE: u32 = 0x103d_3794;
/// Pointers to the unit-vector tables of volume 5 and their sizes (the
/// counts are immediate values in `eft_Emit_SphereEquallyDivided`).
const SPHERE_TABLES: u32 = 0x1047_eea8;
const SPHERE_POINTS: [u32; 8] = [2, 3, 4, 6, 8, 12, 20, 32];
/// Pointers to the tables of volume 6, at `points − 4`, points 4..=64.
const SPHERE64_TABLES: u32 = 0x1047_eed0;
const SPHERE64_MIN: u32 = 4;
const SPHERE64_MAX: u32 = 64;
/// The shortest-arc rotation threshold (`0x03c0e8ec` copies it into the
/// global the volume functions read).
const ARC_EPSILON: u32 = 0x103d_3368;
/// The curl-noise texture's source table (32³ texels × 3 signed bytes)
/// and the generator's code that addresses it (`0x03b69644`).
const CURL_NOISE_TABLE: u32 = 0x103a_6920;
const CURL_NOISE_CODE: u32 = 0x03b6_9738;

/// Section flag: the data is zlib-compressed.
const SHF_RPL_ZLIB: u32 = 0x0800_0000;
/// `SHT_NOBITS`.
const SHT_NOBITS: u32 = 8;

pub fn bake(roots: &ContentRoots, out: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    for path in executables(roots) {
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| tables(&Rpl::parse(&bytes)?))
        {
            Ok(tables) => {
                crate::sky::write_ron::<_, EffectTables>(&out.join(TABLES), &tables)?;
                println!(
                    "effects: tables from {} ({} sphere tables, {} of up to 64 points)",
                    path.display(),
                    tables.sphere.len(),
                    tables.sphere64.len()
                );
                return Ok(());
            }
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }
    if errors.is_empty() {
        return Err("effect tables: no code/U-King.rpx next to the game folders".into());
    }
    Err(format!(
        "effect tables need U-King.rpx v208: {}",
        errors.join("; ")
    ))
}

/// `code/U-King.rpx` beside each content root, the update's first.
pub(crate) fn executables(roots: &ContentRoots) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in roots.roots().iter().rev() {
        for dir in root.ancestors().skip(1).take(3) {
            let path = dir.join("code/U-King.rpx");
            if path.is_file() && !found.contains(&path) {
                found.push(path);
                break;
            }
        }
    }
    found
}

/// Reads the tables and checks they are what the code expects.
pub fn tables(rpl: &Rpl) -> Result<EffectTables, String> {
    let sin_cos: Vec<[f32; 4]> = (0..256)
        .map(|i| {
            let at = SIN_TABLE + 16 * i;
            Ok([
                rpl.f32(at)?,
                rpl.f32(at + 4)?,
                rpl.f32(at + 8)?,
                rpl.f32(at + 12)?,
            ])
        })
        .collect::<Result<_, String>>()?;
    // Entry 0 is sin 0 = 0, cos 0 = 1; entry 64 a quarter turn.
    let check = |ok: bool, what: &str| ok.then_some(()).ok_or(format!("{what} not found"));
    check(
        sin_cos[0][0] == 0.0 && sin_cos[0][2] == 1.0 && (sin_cos[64][0] - 1.0).abs() < 1e-6,
        "sine table",
    )?;
    let unit_vectors = |table: u32, points: u32| -> Result<Vec<[f32; 3]>, String> {
        let base = rpl.u32(table)?;
        let vectors: Vec<[f32; 3]> = (0..points)
            .map(|k| {
                let at = base + 12 * k;
                Ok([rpl.f32(at)?, rpl.f32(at + 4)?, rpl.f32(at + 8)?])
            })
            .collect::<Result<_, String>>()?;
        let unit = vectors
            .iter()
            .all(|v| ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - 1.0).abs() < 1e-3);
        check(unit, &format!("unit vectors at {table:08x}"))?;
        Ok(vectors)
    };
    let sphere = SPHERE_POINTS
        .iter()
        .enumerate()
        .map(|(i, &n)| unit_vectors(SPHERE_TABLES + 4 * i as u32, n))
        .collect::<Result<_, _>>()?;
    let sphere64 = (SPHERE64_MIN..=SPHERE64_MAX)
        .map(|n| unit_vectors(SPHERE64_TABLES + 4 * (n - SPHERE64_MIN), n))
        .collect::<Result<_, _>>()?;
    let arc_epsilon = rpl.f32(ARC_EPSILON)?;
    check(arc_epsilon > 0.0 && arc_epsilon < 1e-3, "arc threshold")?;
    // The curl-noise generator addresses its table as `0x103a5d20 + 0xc00`
    // (`lis r6, 0x103a; addi r6, r6, 0x5d20` … `addi r6, r6, 0xc00`).
    check(
        rpl.u32(CURL_NOISE_CODE)? == 0x3cc0_103a && rpl.u32(CURL_NOISE_CODE + 0xc)? == 0x38c6_5d20,
        "curl-noise generator",
    )?;
    let side = CURL_NOISE_SIZE as u32;
    let curl_noise = rpl
        .bytes(CURL_NOISE_TABLE, side * side * side * 3)?
        .to_vec();
    Ok(EffectTables {
        sin_cos,
        sphere,
        sphere64,
        arc_epsilon,
        curl_noise,
    })
}

/// An RPL's loaded sections: virtual address and (inflated) bytes.
pub struct Rpl {
    sections: Vec<(u32, Vec<u8>)>,
}

impl Rpl {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let be16 = |at: usize| -> Result<u16, String> {
            bytes
                .get(at..at + 2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .ok_or_else(|| "truncated ELF".to_string())
        };
        let be32 = |at: usize| -> Result<u32, String> {
            bytes
                .get(at..at + 4)
                .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
                .ok_or_else(|| "truncated ELF".to_string())
        };
        if bytes.get(..4) != Some(b"\x7fELF") || bytes.get(4) != Some(&1) {
            return Err("not a 32-bit ELF".into());
        }
        let shoff = be32(0x20)? as usize;
        let shentsize = usize::from(be16(0x2e)?);
        let shnum = usize::from(be16(0x30)?);
        let mut sections = Vec::new();
        for i in 0..shnum {
            let h = shoff + i * shentsize;
            let (kind, flags, addr) = (be32(h + 4)?, be32(h + 8)?, be32(h + 12)?);
            let (offset, size) = (be32(h + 16)? as usize, be32(h + 20)? as usize);
            if addr == 0 || kind == SHT_NOBITS || size == 0 {
                continue;
            }
            let raw = bytes
                .get(offset..offset + size)
                .ok_or_else(|| format!("section {i} outside the file"))?;
            let data = if flags & SHF_RPL_ZLIB != 0 {
                let inflated = u32::from_be_bytes(raw[..4].try_into().unwrap()) as usize;
                let data = zlib_inflate(&raw[4..])?;
                if data.len() != inflated {
                    return Err(format!("section {i}: inflated size mismatch"));
                }
                data
            } else {
                raw.to_vec()
            };
            sections.push((addr, data));
        }
        Ok(Self { sections })
    }

    pub fn bytes(&self, address: u32, len: u32) -> Result<&[u8], String> {
        self.sections
            .iter()
            .find_map(|(base, data)| {
                let start = address.checked_sub(*base)? as usize;
                data.get(start..start + len as usize)
            })
            .ok_or_else(|| format!("address {address:08x} not in a section"))
    }

    pub fn u32(&self, address: u32) -> Result<u32, String> {
        Ok(u32::from_be_bytes(
            self.bytes(address, 4)?.try_into().unwrap(),
        ))
    }

    pub fn f32(&self, address: u32) -> Result<f32, String> {
        self.u32(address).map(f32::from_bits)
    }
}

/// A zlib stream (RFC 1950) inflated (RFC 1951); the Adler-32 is checked.
pub fn zlib_inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 6 || (u16::from(data[0]) << 8 | u16::from(data[1])) % 31 != 0 {
        return Err("bad zlib header".into());
    }
    if data[0] & 0x0f != 8 || data[1] & 0x20 != 0 {
        return Err("unsupported zlib stream".into());
    }
    let (out, used) = inflate(&data[2..])?;
    let end = 2 + used;
    let adler = data
        .get(end..end + 4)
        .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
        .ok_or("missing Adler-32")?;
    if adler != adler32(&out) {
        return Err("Adler-32 mismatch".into());
    }
    Ok(out)
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in data.chunks(5552) {
        for &byte in chunk {
            a += u32::from(byte);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    b << 16 | a
}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    bits: u32,
}

impl Bits<'_> {
    fn need(&mut self, n: u32) -> Result<(), String> {
        while self.bits < n {
            let byte = *self.data.get(self.pos).ok_or("truncated deflate stream")?;
            self.pos += 1;
            self.bit |= u32::from(byte) << self.bits;
            self.bits += 8;
        }
        Ok(())
    }

    fn take(&mut self, n: u32) -> Result<u32, String> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let value = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.bits -= n;
        Ok(value)
    }

    fn align(&mut self) {
        let drop = self.bits % 8;
        self.bit >>= drop;
        self.bits -= drop;
    }

    /// Bytes consumed (whole bytes still buffered are given back).
    fn consumed(&self) -> usize {
        self.pos - (self.bits / 8) as usize
    }
}

/// A canonical Huffman code: counts per length, symbols by code.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Self, String> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[usize::from(l)] += 1;
        }
        counts[0] = 0;
        let mut offsets = [0u16; 16];
        for l in 1..16 {
            offsets[l] = offsets[l - 1] + counts[l - 1];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[usize::from(offsets[usize::from(l)])] = symbol as u16;
                offsets[usize::from(l)] += 1;
            }
        }
        Ok(Self { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, String> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.take(1)? as i32;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err("bad Huffman code".into())
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// Raw deflate; returns the data and the bytes consumed.
fn inflate(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let mut bits = Bits {
        data,
        pos: 0,
        bit: 0,
        bits: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = bits.take(1)? == 1;
        match bits.take(2)? {
            0 => {
                bits.align();
                let len = bits.take(16)?;
                let nlen = bits.take(16)?;
                if len != !nlen & 0xffff {
                    return Err("bad stored block".into());
                }
                for _ in 0..len {
                    out.push(bits.take(8)? as u8);
                }
            }
            1 => {
                let mut lengths = [0u8; 288];
                lengths[..144].fill(8);
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                lengths[280..].fill(8);
                let lit = Huffman::new(&lengths)?;
                let dist = Huffman::new(&[5u8; 30])?;
                block(&mut bits, &mut out, &lit, &dist)?;
            }
            2 => {
                let nlit = bits.take(5)? as usize + 257;
                let ndist = bits.take(5)? as usize + 1;
                let nclen = bits.take(4)? as usize + 4;
                const ORDER: [usize; 19] = [
                    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
                ];
                let mut clen = [0u8; 19];
                for &i in &ORDER[..nclen] {
                    clen[i] = bits.take(3)? as u8;
                }
                let clen = Huffman::new(&clen)?;
                let mut lengths = vec![0u8; nlit + ndist];
                let mut i = 0;
                while i < lengths.len() {
                    let symbol = clen.decode(&mut bits)?;
                    let (value, repeat) = match symbol {
                        0..=15 => (symbol as u8, 1),
                        16 => {
                            let previous = *lengths[..i].last().ok_or("repeat with nothing")?;
                            (previous, 3 + bits.take(2)?)
                        }
                        17 => (0, 3 + bits.take(3)?),
                        _ => (0, 11 + bits.take(7)?),
                    };
                    for _ in 0..repeat {
                        *lengths.get_mut(i).ok_or("code lengths overflow")? = value;
                        i += 1;
                    }
                }
                let lit = Huffman::new(&lengths[..nlit])?;
                let dist = Huffman::new(&lengths[nlit..])?;
                block(&mut bits, &mut out, &lit, &dist)?;
            }
            _ => return Err("bad block type".into()),
        }
        if last {
            break;
        }
    }
    bits.align();
    Ok((out, bits.consumed()))
}

fn block(bits: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman) -> Result<(), String> {
    loop {
        let symbol = lit.decode(bits)?;
        match symbol {
            0..=255 => out.push(symbol as u8),
            256 => return Ok(()),
            _ => {
                let i = usize::from(symbol - 257);
                let len = usize::from(*LENGTH_BASE.get(i).ok_or("bad length")?)
                    + bits.take(u32::from(LENGTH_EXTRA[i]))? as usize;
                let d = usize::from(dist.decode(bits)?);
                let distance = usize::from(*DIST_BASE.get(d).ok_or("bad distance")?)
                    + bits.take(u32::from(DIST_EXTRA[d]))? as usize;
                let start = out
                    .len()
                    .checked_sub(distance)
                    .ok_or("distance too far back")?;
                for k in 0..len {
                    out.push(out[start + k]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inflates_fixed_dynamic_and_stored_blocks() {
        // `zlib.compress(b"hello hello hello hello")` (fixed Huffman).
        let fixed = [
            0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x57, 0xc8, 0x40, 0x27, 0x01, 0x68, 0x03,
            0x08, 0xb1,
        ];
        assert_eq!(zlib_inflate(&fixed).unwrap(), b"hello hello hello hello");
        // `zlib.compress(bytes((i*i*7+i)%36+97 for i in range(400)), 9)`
        // (dynamic Huffman).
        let dynamic = [
            0x78, 0xda, 0xed, 0xca, 0xa1, 0x0d, 0x00, 0x41, 0x10, 0x02, 0xc0, 0x5a, 0x51, 0x04,
            0x81, 0x42, 0x6d, 0x10, 0xb4, 0x7e, 0x5d, 0xbc, 0xfa, 0xd1, 0x03, 0x6d, 0x02, 0x7b,
            0xe8, 0xac, 0x50, 0x97, 0xda, 0xcd, 0x89, 0x91, 0x57, 0x5c, 0x89, 0xff, 0x7c, 0x76,
            0x1e, 0xdb, 0xdb, 0xb0, 0x11,
        ];
        let expected: Vec<u8> = (0..400u32)
            .map(|i| ((i * i * 7 + i) % 36 + 97) as u8)
            .collect();
        assert_eq!(zlib_inflate(&dynamic).unwrap(), expected);
        // `zlib.compress(b"abc", 0)` (stored).
        let stored = [
            0x78, 0x01, 0x01, 0x03, 0x00, 0xfc, 0xff, 0x61, 0x62, 0x63, 0x02, 0x4d, 0x01, 0x27,
        ];
        assert_eq!(zlib_inflate(&stored).unwrap(), b"abc");
        assert!(zlib_inflate(&stored[..stored.len() - 1]).is_err());
    }

    /// `BOTW_RPX=…/update/code/U-King.rpx cargo test -p bake -- --ignored`
    #[test]
    #[ignore]
    fn extracts_from_the_game() {
        let path = std::env::var("BOTW_RPX").expect("BOTW_RPX");
        let rpl = Rpl::parse(&std::fs::read(path).unwrap()).unwrap();
        let t = tables(&rpl).unwrap();
        assert_eq!(t.sin_cos.len(), 256);
        assert_eq!(t.sphere[0], vec![[0.0, 1.0, 0.0], [0.0, -1.0, 0.0]]);
        assert_eq!(t.sphere64.len(), 61);
        assert_eq!(t.sphere64[60].len(), 64);
        assert_eq!(t.arc_epsilon, f32::from_bits(0x3480_0000));
    }

    #[test]
    fn reads_sections_by_address() {
        // A minimal ELF: header, one uncompressed section at 0x1000.
        let mut elf = vec![0u8; 0x34];
        elf[..5].copy_from_slice(b"\x7fELF\x01");
        let data_at = elf.len();
        elf.extend_from_slice(&[0, 0, 0, 42, 0x3f, 0x80, 0, 0]);
        let shoff = elf.len();
        elf[0x20..0x24].copy_from_slice(&(shoff as u32).to_be_bytes());
        elf[0x2e..0x30].copy_from_slice(&40u16.to_be_bytes());
        elf[0x30..0x32].copy_from_slice(&2u16.to_be_bytes());
        elf.extend_from_slice(&[0u8; 40]);
        let mut h = [0u8; 40];
        h[4..8].copy_from_slice(&1u32.to_be_bytes());
        h[12..16].copy_from_slice(&0x1000u32.to_be_bytes());
        h[16..20].copy_from_slice(&(data_at as u32).to_be_bytes());
        h[20..24].copy_from_slice(&8u32.to_be_bytes());
        elf.extend_from_slice(&h);
        let rpl = Rpl::parse(&elf).unwrap();
        assert_eq!(rpl.u32(0x1000).unwrap(), 42);
        assert_eq!(rpl.f32(0x1004).unwrap(), 1.0);
        assert!(rpl.u32(0x1006).is_err());
    }
}
