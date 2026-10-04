//! Yaz0, Nintendo's LZ77 variant used for every `s`-prefixed file
//! (`.sstera`, `.sbfres`, `.smubin`, …).
//!
//! Pure Rust so the workspace builds without CMake/C++ (roead's Yaz0 needs
//! both). Only decompression matters for reading game data; [`compress_stored`]
//! produces valid but uncompressed Yaz0 for tests and generated data.

use crate::{FormatError, Result};

const MAGIC: &[u8; 4] = b"Yaz0";
const HEADER_SIZE: usize = 16;

pub fn is_yaz0(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Decompresses `bytes` if they carry a Yaz0 header; otherwise borrows them.
pub fn decompress_if(bytes: &[u8]) -> Result<std::borrow::Cow<'_, [u8]>> {
    if is_yaz0(bytes) {
        decompress(bytes).map(std::borrow::Cow::Owned)
    } else {
        Ok(std::borrow::Cow::Borrowed(bytes))
    }
}

pub fn decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() < HEADER_SIZE || !is_yaz0(bytes) {
        return Err(FormatError::Yaz0("missing Yaz0 header"));
    }
    let size = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let mut out = Vec::with_capacity(size);
    let mut input = bytes[HEADER_SIZE..].iter().copied();
    let mut next = || input.next().ok_or(FormatError::Yaz0("truncated stream"));

    while out.len() < size {
        let group = next()?;
        for bit in (0..8).rev() {
            if out.len() >= size {
                break;
            }
            if group & (1 << bit) != 0 {
                out.push(next()?);
                continue;
            }
            // Back-reference: 4 bits length, 12 bits distance, optional length byte.
            let (b1, b2) = (next()?, next()?);
            let distance = ((usize::from(b1 & 0x0F) << 8) | usize::from(b2)) + 1;
            let length = match b1 >> 4 {
                0 => usize::from(next()?) + 0x12,
                n => usize::from(n) + 2,
            };
            if distance > out.len() {
                return Err(FormatError::Yaz0("back-reference before start of output"));
            }
            let start = out.len() - distance;
            // Byte by byte: the source may overlap the bytes being written.
            for i in 0..length {
                let byte = out[start + i];
                out.push(byte);
            }
        }
    }
    out.truncate(size);
    Ok(out)
}

/// Encodes `data` as Yaz0 using only literal bytes (no compression).
pub fn compress_stored(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_SIZE + data.len() + data.len() / 8 + 1);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(&[0; 8]);
    for chunk in data.chunks(8) {
        out.push(0xFF);
        out.extend_from_slice(chunk);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_header(size: u32, body: &[u8]) -> Vec<u8> {
        let mut out = MAGIC.to_vec();
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn decodes_short_and_long_back_references() {
        // "ab" as literals, then an overlapping copy of 4 bytes at distance 2,
        // then a long copy (length byte form: 0x12 + 2 = 20) at distance 1.
        let body = [
            0b1100_0000, // literal, literal, ref, ref
            b'a',
            b'b',
            0x20, 0x01, // length 2 + 2 = 4, distance 2
            0x00, 0x00, 0x02, // length 0x12 + 2 = 20, distance 1
        ];
        let out = decompress(&with_header(26, &body)).unwrap();
        let mut expected = b"ababab".to_vec();
        expected.extend(std::iter::repeat_n(b'b', 20));
        assert_eq!(out, expected);
    }

    #[test]
    fn stored_round_trip() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 251) as u8).collect();
        let packed = compress_stored(&data);
        assert!(is_yaz0(&packed));
        assert_eq!(decompress(&packed).unwrap(), data);
        assert_eq!(decompress(&compress_stored(&[])).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn rejects_bad_input() {
        assert!(decompress(b"nope").is_err());
        assert!(decompress(&with_header(10, &[0xFF, 1, 2])).is_err(), "truncated");
        assert!(decompress(&with_header(4, &[0x00, 0x10, 0x05])).is_err(), "reference before start");
        assert!(matches!(decompress_if(b"SARC").unwrap(), std::borrow::Cow::Borrowed(_)));
    }
}
