//! `.sblwp` instance lists (magic `PrOD`): many copies of a few models, each
//! a position, rotation and uniform scale. MainField has one per map cell
//! for the trees the terrain system draws (`<cell>_TeraTree.sblwp`) and
//! several for clustered small objects (`_Clustering.sblwp`). Big-endian on
//! Wii U. See archived FORMATS.md. notes

use crate::{FormatError, Result};

/// One instance of a model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instance {
    pub translate: [f32; 3],
    /// Rotation in degrees about X, Y, Z (applied in that order).
    pub rotate: [f32; 3],
    pub scale: f32,
}

/// The instances of one model (by actor name).
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceGroup {
    pub name: String,
    pub instances: Vec<Instance>,
}

const INSTANCE_SIZE: usize = 0x20;

/// Parses a (possibly Yaz0-compressed) `PrOD` file.
pub fn parse(bytes: &[u8]) -> Result<Vec<InstanceGroup>> {
    let data = crate::yaz0::decompress_if(bytes)?;
    let data = &data[..];
    if data.get(..4) != Some(b"PrOD") {
        return Err(FormatError::Invalid("PrOD: bad magic"));
    }
    let u32_at = |offset: usize| -> Result<u32> {
        let bytes = data.get(offset..offset + 4).ok_or(FormatError::Invalid("PrOD: truncated"))?;
        Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
    };
    let f32_at = |offset: usize| u32_at(offset).map(f32::from_bits);
    let groups = u32_at(0x14)? as usize;
    let strings = u32_at(0x18)? as usize;
    // The string table: a count, a size, then 4-aligned names.
    let name_at = |offset: usize| -> Result<String> {
        let start = strings + offset;
        let rest = data.get(start..).ok_or(FormatError::Invalid("PrOD: name out of range"))?;
        let end = rest.iter().position(|&b| b == 0).ok_or(FormatError::Invalid("PrOD: unterminated name"))?;
        Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
    };

    let mut out = Vec::with_capacity(groups);
    let mut offset = 0x20;
    for _ in 0..groups {
        let size = u32_at(offset)? as usize;
        let count = u32_at(offset + 4)? as usize;
        let name = name_at(u32_at(offset + 8)? as usize)?;
        if size != count * INSTANCE_SIZE {
            return Err(FormatError::Invalid("PrOD: instance size mismatch"));
        }
        let mut instances = Vec::with_capacity(count);
        for i in 0..count {
            let at = offset + 0x10 + i * INSTANCE_SIZE;
            let v = |k: usize| f32_at(at + 4 * k);
            instances.push(Instance { translate: [v(0)?, v(1)?, v(2)?], rotate: [v(3)?, v(4)?, v(5)?], scale: v(6)? });
        }
        out.push(InstanceGroup { name, instances });
        offset += 0x10 + size;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file laid out like the game's `A-3_TeraTree.sblwp`.
    fn sample() -> Vec<u8> {
        let instance = |pos: [f32; 3], rot: [f32; 3], scale: f32| {
            let mut b = Vec::new();
            for v in pos.iter().chain(&rot).chain(&[scale]) {
                b.extend(v.to_be_bytes());
            }
            b.extend([0; 4]);
            b
        };
        let group = |name_offset: u32, instances: Vec<Vec<u8>>| {
            let mut b = Vec::new();
            b.extend(((instances.len() * 0x20) as u32).to_be_bytes());
            b.extend((instances.len() as u32).to_be_bytes());
            b.extend(name_offset.to_be_bytes());
            b.extend([0; 4]);
            instances.into_iter().for_each(|i| b.extend(i));
            b
        };
        let mut groups = group(8, vec![instance([1.0, 2.0, 3.0], [0.0, -0.0, 0.0], 1.0), instance([4.0, 5.0, 6.0], [0.0; 3], 2.0)]);
        groups.extend(group(12, vec![instance([7.0, 8.0, 9.0], [180.0, -38.8, 180.0], 1.0)]));
        let strings_at = 0x20 + groups.len() as u32;
        let mut strings = Vec::new();
        strings.extend(2u32.to_be_bytes());
        strings.extend(8u32.to_be_bytes());
        strings.extend(b"Ab\0\0Cd\0\0");
        let mut file = b"PrOD".to_vec();
        for v in [0x0100_0000, 1, strings_at - 8, strings_at + strings.len() as u32, 2, strings_at, 0] {
            file.extend(u32::to_be_bytes(v));
        }
        file.extend(groups);
        file.extend(strings);
        file
    }

    #[test]
    fn reads_groups_and_instances() {
        let groups = parse(&sample()).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "Ab");
        assert_eq!(groups[0].instances[1], Instance { translate: [4.0, 5.0, 6.0], rotate: [0.0; 3], scale: 2.0 });
        assert_eq!(groups[1].name, "Cd");
        assert_eq!(groups[1].instances[0].rotate, [180.0, -38.8, 180.0]);
    }

    #[test]
    fn rejects_other_files() {
        assert!(parse(b"BY\0\x02").is_err());
    }
}
