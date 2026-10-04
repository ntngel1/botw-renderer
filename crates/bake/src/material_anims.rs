//! Material animations read as values at one frame: what the game sets a
//! UMii villager's colours with (it plays a shader-parameter animation at
//! the frame of the parameter, e.g. `UMii_Hylia_Hair_Color` at
//! `hair.color`; see docs/research/umii.md).
//!
//! Wii U BFRES 3.4+ `FSHU` (`ShaderParamAnim`, groups 0x2C shader
//! parameters, 0x30 colours, 0x34 texture SRTs), `ShaderParamMatAnim`,
//! `ParamAnimInfo`, `AnimCurve`, as laid out in the Wii U BFRES format
//! (the same reader as `xlu::srt_animations`, over every parameter).

/// One parameter of one material at one frame: `(material, parameter,
/// [(component, value)])`; components are 32-bit words of the parameter.
pub type ParamValues = Vec<(String, String, Vec<(usize, f32)>)>;

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn bytes<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        self.0.get(at..at + N)?.try_into().ok()
    }
    fn u8(&self, at: usize) -> Option<u8> {
        self.0.get(at).copied()
    }
    fn u16(&self, at: usize) -> Option<u16> {
        self.bytes(at).map(u16::from_be_bytes)
    }
    fn u32(&self, at: usize) -> Option<u32> {
        self.bytes(at).map(u32::from_be_bytes)
    }
    fn f32(&self, at: usize) -> Option<f32> {
        self.bytes(at).map(f32::from_be_bytes)
    }
    fn pointer(&self, at: usize) -> Option<usize> {
        let offset = self.u32(at)? as i32;
        (offset != 0).then(|| (at as i64 + i64::from(offset)) as usize)
    }
    fn string(&self, at: usize) -> Option<String> {
        let start = self.pointer(at)?;
        let tail = self.0.get(start..)?;
        let end = tail.iter().position(|&b| b == 0)?;
        Some(String::from_utf8_lossy(&tail[..end]).into_owned())
    }
    /// Index group entries `(name, data offset)`.
    fn group(&self, at: usize) -> Vec<(String, usize)> {
        let Some(group) = self.pointer(at) else {
            return Vec::new();
        };
        let count = self.u32(group + 4).unwrap_or(0) as usize;
        (1..=count)
            .filter_map(|i| {
                let entry = group + 8 + i * 16;
                Some((self.string(entry + 8)?, self.pointer(entry + 12)?))
            })
            .collect()
    }
}

/// The names of a file's shader-parameter animations (all three groups).
pub fn shader_param_animation_names(bytes: &[u8]) -> Vec<String> {
    let r = Reader(bytes);
    if bytes.get(..4) != Some(b"FRES") {
        return Vec::new();
    }
    [0x2C, 0x30, 0x34]
        .into_iter()
        .flat_map(|g| r.group(g))
        .map(|(name, _)| name)
        .collect()
}

/// The values animation `name` gives its materials' parameters at
/// `frame` (`None`: the file has no such animation).
pub fn shader_params_at(bytes: &[u8], name: &str, frame: f32) -> Option<ParamValues> {
    let r = Reader(bytes);
    if bytes.get(..4) != Some(b"FRES") {
        return None;
    }
    let at = [0x2C, 0x30, 0x34]
        .into_iter()
        .flat_map(|g| r.group(g))
        .find(|(n, _)| n == name)?
        .1;
    if r.0.get(at..at + 4)? != b"FSHU" {
        return None;
    }
    let count = r.u16(at + 0x14)? as usize;
    let list = r.pointer(at + 0x2C)?;
    let mut values = Vec::new();
    for m in 0..count {
        let at = list + m * 0x20;
        let params = r.u16(at)? as usize;
        let curve_count = r.u16(at + 2)? as usize;
        let constant_count = r.u16(at + 4)? as usize;
        let material = r.string(at + 0x10)?;
        let infos = r.pointer(at + 0x14);
        let curves_at = r.pointer(at + 0x18);
        let constants_at = r.pointer(at + 0x1C);
        for p in 0..params {
            let info = infos? + p * 0x10;
            let param = r.string(info + 0x0C)?;
            let first_curve = r.u16(info)? as usize;
            let float_curves = r.u16(info + 2)? as usize;
            let int_curves = r.u16(info + 4)? as usize;
            let first_constant = r.u16(info + 6)? as usize;
            let constants = r.u16(info + 8)? as usize;
            let mut components = Vec::new();
            for c in first_constant..(first_constant + constants).min(constant_count) {
                let at = constants_at? + c * 8;
                components.push(((r.u32(at)? / 4) as usize, r.f32(at + 4)?));
            }
            let last = (first_curve + float_curves + int_curves).min(curve_count);
            for c in first_curve..last {
                if let Some((component, value)) = curve_at(&r, curves_at? + c * 0x24, frame) {
                    components.retain(|(k, _)| *k != component);
                    components.push((component, value));
                }
            }
            components.sort_by_key(|(k, _)| *k);
            values.push((material.clone(), param, components));
        }
    }
    Some(values)
}

/// An `AnimCurve`'s target component and value at `frame`: flags (frame
/// type bits 0–1, key type 2–3, curve type 4–6), key count, target byte
/// offset, start and end frame, key scale and offset, delta, frames, keys.
/// Frames outside the keys clamp to the first or last key.
fn curve_at(r: &Reader, at: usize, frame: f32) -> Option<(usize, f32)> {
    let flags = r.u16(at)?;
    let count = r.u16(at + 2)? as usize;
    let target = r.u32(at + 4)? as usize / 4;
    let scale = r.f32(at + 0x10)?;
    let offset = r.f32(at + 0x14)?;
    // Cubic 4 coefficients a key, linear 2, the rest (baked and stepped
    // floats and integers) one.
    let per_key = match (flags >> 4) & 7 {
        0 => 4,
        1 => 2,
        _ => 1,
    };
    let integer = (flags >> 4) & 7 >= 4;
    let frames_at = r.pointer(at + 0x1C)?;
    let keys_at = r.pointer(at + 0x20)?;
    let frames = (0..count)
        .map(|i| match flags & 3 {
            0 => r.f32(frames_at + i * 4),
            1 => r.u16(frames_at + i * 2).map(|v| v as i16 as f32 / 32.0),
            _ => r.u8(frames_at + i).map(f32::from),
        })
        .collect::<Option<Vec<_>>>()?;
    let raw = |i: usize| -> Option<f32> {
        match (flags >> 2) & 3 {
            0 if integer => r.u32(keys_at + i * 4).map(|v| v as i32 as f32),
            0 => r.f32(keys_at + i * 4),
            1 => r.u16(keys_at + i * 2).map(|v| f32::from(v as i16)),
            _ => r.u8(keys_at + i).map(|v| f32::from(v as i8)),
        }
    };
    let key = |k: usize| -> Option<[f32; 4]> {
        let mut key = [0.0; 4];
        for (c, value) in key.iter_mut().enumerate().take(per_key) {
            *value = raw(k * per_key + c)? * scale + if c == 0 { offset } else { 0.0 };
        }
        Some(key)
    };
    let first = *frames.first()?;
    if frame <= first {
        return Some((target, key(0)?[0]));
    }
    let i = frames.partition_point(|&f| f <= frame).saturating_sub(1);
    let k = key(i)?;
    let Some(&next) = frames.get(i + 1) else {
        return Some((target, k[0]));
    };
    let t = (frame - frames[i]) / (next - frames[i]).max(1e-6);
    let value = match per_key {
        4 => k[0] + t * (k[1] + t * (k[2] + t * k[3])),
        2 => k[0] + k[1] * t,
        _ if integer => k[0],
        _ => k[0] + (key(i + 1)?[0] - k[0]) * t,
    };
    Some((target, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file with one `FSHU` in group 0x2C animating `const_color1` of
    /// `Mt_Hair`: a constant red and a stepped-key green.
    fn fixture() -> Vec<u8> {
        let mut b = vec![0u8; 0x400];
        let put32 =
            |b: &mut Vec<u8>, at: usize, v: u32| b[at..at + 4].copy_from_slice(&v.to_be_bytes());
        let put16 =
            |b: &mut Vec<u8>, at: usize, v: u16| b[at..at + 2].copy_from_slice(&v.to_be_bytes());
        let ptr = |b: &mut Vec<u8>, at: usize, to: usize| {
            b[at..at + 4].copy_from_slice(&((to as i64 - at as i64) as i32).to_be_bytes())
        };
        let text = |b: &mut Vec<u8>, at: usize, s: &str| {
            b[at..at + s.len()].copy_from_slice(s.as_bytes());
        };
        b[..4].copy_from_slice(b"FRES");
        // Group with one entry.
        ptr(&mut b, 0x2C, 0x40);
        put32(&mut b, 0x44, 1);
        text(&mut b, 0x300, "Hair_Color");
        ptr(&mut b, 0x40 + 8 + 16 + 8, 0x300);
        ptr(&mut b, 0x40 + 8 + 16 + 12, 0x80);
        // FSHU.
        b[0x80..0x84].copy_from_slice(b"FSHU");
        put16(&mut b, 0x80 + 0x14, 1);
        ptr(&mut b, 0x80 + 0x2C, 0xC0);
        // Material anim: 1 param, 1 curve, 1 constant.
        put16(&mut b, 0xC0, 1);
        put16(&mut b, 0xC2, 1);
        put16(&mut b, 0xC4, 1);
        text(&mut b, 0x320, "Mt_Hair");
        ptr(&mut b, 0xC0 + 0x10, 0x320);
        ptr(&mut b, 0xC0 + 0x14, 0x100);
        ptr(&mut b, 0xC0 + 0x18, 0x140);
        ptr(&mut b, 0xC0 + 0x1C, 0x1A0);
        // Param info: curve 0 (float), constant 0.
        put16(&mut b, 0x102, 1);
        put16(&mut b, 0x108, 1);
        text(&mut b, 0x340, "const_color1");
        ptr(&mut b, 0x100 + 0x0C, 0x340);
        // Curve: stepped float (type 2), f32 frames and keys, target 4.
        put16(&mut b, 0x140, 2 << 4);
        put16(&mut b, 0x142, 2);
        put32(&mut b, 0x144, 4);
        put32(&mut b, 0x150, 1.0f32.to_bits());
        ptr(&mut b, 0x140 + 0x1C, 0x180);
        ptr(&mut b, 0x140 + 0x20, 0x190);
        put32(&mut b, 0x180, 0.0f32.to_bits());
        put32(&mut b, 0x184, 1.0f32.to_bits());
        put32(&mut b, 0x190, 0.25f32.to_bits());
        put32(&mut b, 0x194, 0.75f32.to_bits());
        // Constant: component 0 = 0.5.
        put32(&mut b, 0x1A0, 0);
        put32(&mut b, 0x1A4, 0.5f32.to_bits());
        b
    }

    #[test]
    fn reads_a_parameter_at_a_frame() {
        let file = fixture();
        assert_eq!(shader_param_animation_names(&file), ["Hair_Color"]);
        let at = |frame| shader_params_at(&file, "Hair_Color", frame).unwrap();
        assert_eq!(
            at(0.0),
            [(
                "Mt_Hair".to_owned(),
                "const_color1".to_owned(),
                vec![(0, 0.5), (1, 0.25)]
            )]
        );
        assert_eq!(at(1.0)[0].2, [(0, 0.5), (1, 0.75)]);
        assert!(shader_params_at(&file, "Other", 0.0).is_none());
    }
}
