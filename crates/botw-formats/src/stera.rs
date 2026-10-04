//! `.sstera` archives: a Yaz0-compressed SARC holding up to four terrain
//! tiles. The leading "s" in the extension marks Yaz0 compression, exactly
//! like `.sbfres` or `.smubin`.

pub use roead::Endian;

use crate::{FormatError, Result};

/// A decoded archive entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub data: Vec<u8>,
}

/// Decompresses (if needed) and unpacks a `.sstera` / `.stera` archive.
pub fn read(bytes: &[u8]) -> Result<Vec<Entry>> {
    let sarc_bytes = crate::yaz0::decompress_if(bytes)?;
    let sarc = roead::sarc::Sarc::new(sarc_bytes.as_ref())?;
    sarc.files()
        .map(|file| {
            let name = file
                .name()
                .ok_or(FormatError::UnnamedEntry(file.index()))?
                .to_owned();
            Ok(Entry {
                name,
                data: file.data().to_vec(),
            })
        })
        .collect()
}

/// Builds a `.sstera` archive (Yaz0 without actual compression). The game
/// never needs this; it is used by tests and generated data.
pub fn write(entries: &[Entry], endian: Endian) -> Vec<u8> {
    let mut writer = roead::sarc::SarcWriter::new(endian);
    for entry in entries {
        writer.add_file(entry.name.clone(), entry.data.clone());
    }
    crate::yaz0::compress_stored(&writer.to_binary())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_both_endians() {
        let entries = vec![
            Entry {
                name: "5100000000.hght".into(),
                data: vec![1, 2, 3, 4],
            },
            Entry {
                name: "5100000001.hght".into(),
                data: vec![5; 64],
            },
        ];
        for endian in [Endian::Big, Endian::Little] {
            let packed = write(&entries, endian);
            assert_eq!(&packed[..4], b"Yaz0");
            let mut unpacked = read(&packed).unwrap();
            unpacked.sort_by(|a, b| a.name.cmp(&b.name));
            assert_eq!(unpacked, entries);
        }
    }
}
