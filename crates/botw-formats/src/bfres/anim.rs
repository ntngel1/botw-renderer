// SI-ANM-12: Sampled curves interpolated linearly; scale mode ignored.
//! Skeletal animations (FSKA) of Wii U BFRES files, e.g.
//! `Player_Animation.sbfres` (1167 animations for Link). Layout verified on
//! the dump (BFRES 4.5.0.3, big-endian, pointers relative to themselves):
//!
//! ```text
//! FSKA   0x00 magic, 0x04 name, 0x08 path, 0x0C flags (0x4 looping,
//!        0x300 scale mode, 0x1000 Euler XYZ rotations), 0x10 i32 frame
//!        count, 0x14 u16 bone animations, 0x16 u16 user data, 0x18 i32
//!        curve count, 0x1C baked size, 0x20 bone animations, 0x24 bind
//!        skeleton, 0x28 bind indices, 0x2C user data
//! bone   0x00 flags, 0x04 name, 0x08 u8 × 4 (curve and base offsets, curve
//!        count), 0x0C begin curve, 0x10 curves, 0x14 base values
//!        (24 bytes)
//! curve  0x00 u16 flags, 0x02 u16 key count, 0x04 target offset, 0x08 start
//!        frame, 0x0C end frame, 0x10 scale, 0x14 offset, 0x18 delta,
//!        0x1C frames, 0x20 keys (36 bytes)
//! ```
//!
//! Bone flags: bits 3–5 say which base values follow (scale ×3, rotation ×4,
//! translation ×3, in that order), bits 6–15 which components have curves.
//! Curve targets are offsets into a pose record: scale 0x04–0x0C,
//! translation 0x10–0x18, rotation 0x20–0x2C. Curve flags: bits 0–1 frame
//! type (f32, i16 / 32, u8), bits 2–3 key type (f32, i16, i8), bits 4–6
//! curve type (cubic, linear, baked float, …). Keys are `raw × scale`, plus
//! `offset` for the constant term. Animations bind to a skeleton by bone name.

use super::Reader;
use crate::{FormatError, Result};

#[derive(Clone, Debug)]
pub struct SkeletalAnim {
    pub name: String,
    /// Length in frames (30 per second).
    pub frame_count: f32,
    pub looping: bool,
    /// Rotations are Euler angles (X, Y, Z in radians) rather than quaternions.
    pub euler: bool,
    pub bones: Vec<BoneAnim>,
}

#[derive(Clone, Debug)]
pub struct BoneAnim {
    pub name: String,
    pub flags: u32,
    pub base_scale: Option<[f32; 3]>,
    pub base_rotation: Option<[f32; 4]>,
    pub base_translation: Option<[f32; 3]>,
    pub curves: Vec<Curve>,
}

/// A bone's pose at one frame; `None` where the animation leaves the bind pose.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BonePose {
    pub scale: Option<[f32; 3]>,
    /// Euler X, Y, Z (and unused W) or a quaternion, see [`SkeletalAnim::euler`].
    pub rotation: Option<[f32; 4]>,
    pub translation: Option<[f32; 3]>,
}

/// Which pose component a curve drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Scale(usize),
    Translation(usize),
    Rotation(usize),
}

impl Target {
    fn from_offset(offset: u32) -> Option<Self> {
        Some(match offset {
            0x04 | 0x08 | 0x0C => Target::Scale((offset as usize - 0x04) / 4),
            0x10 | 0x14 | 0x18 => Target::Translation((offset as usize - 0x10) / 4),
            0x20 | 0x24 | 0x28 | 0x2C => Target::Rotation((offset as usize - 0x20) / 4),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveKind {
    /// Per key the coefficients of `c0 + c1 t + c2 t² + c3 t³`, `t` from 0
    /// to 1 across the segment to the next key.
    Cubic,
    /// Per key a value and its change over the segment.
    Linear,
    /// One value per key (baked or stepped).
    Sampled,
}

#[derive(Clone, Debug)]
pub struct Curve {
    pub target: Target,
    pub kind: CurveKind,
    pub frames: Vec<f32>,
    /// Scaled coefficients per key (unused ones zero).
    pub keys: Vec<[f32; 4]>,
}

impl Curve {
    pub fn sample(&self, frame: f32) -> f32 {
        let Some(&first) = self.frames.first() else {
            return 0.0;
        };
        if frame <= first {
            return self.keys[0][0];
        }
        // The last key at or before `frame`.
        let i = self
            .frames
            .partition_point(|&f| f <= frame)
            .saturating_sub(1);
        let key = self.keys[i];
        let Some(&next_frame) = self.frames.get(i + 1) else {
            return key[0];
        };
        let t = (frame - self.frames[i]) / (next_frame - self.frames[i]).max(1e-6);
        match self.kind {
            CurveKind::Cubic => key[0] + t * (key[1] + t * (key[2] + t * key[3])),
            CurveKind::Linear => key[0] + key[1] * t,
            // SI-ANM-12: Sampled curves interpolated linearly; scale mode ignored.
            CurveKind::Sampled => key[0] + (self.keys[i + 1][0] - key[0]) * t,
        }
    }
}

impl BoneAnim {
    /// The bone's pose at `frame`: curves override the base values.
    pub fn sample(&self, frame: f32) -> BonePose {
        self.sample_with_bind(frame, BonePose::default())
    }

    /// Sample over the bound FSKL pose, retaining components without curves.
    /// `bind.rotation` must use the animation's Euler/quaternion representation.
    /// Wii U v208: 0x03c0b48c initializes each channel from authored base values
    /// or FSKL (0x03c0b4f8 scale, 0x03c0b56c rotation, 0x03c0b5d0 translation);
    /// 0x03c0b608 then overwrites only the components targeted by curves.
    pub fn sample_with_bind(&self, frame: f32, bind: BonePose) -> BonePose {
        let mut pose = BonePose {
            scale: self.base_scale.or(bind.scale),
            rotation: self.base_rotation.or(bind.rotation),
            translation: self.base_translation.or(bind.translation),
        };
        for curve in &self.curves {
            let value = curve.sample(frame);
            match curve.target {
                Target::Scale(i) => pose.scale.get_or_insert([1.0; 3])[i] = value,
                Target::Translation(i) => pose.translation.get_or_insert([0.0; 3])[i] = value,
                Target::Rotation(i) => pose.rotation.get_or_insert([0.0, 0.0, 0.0, 1.0])[i] = value,
            }
        }
        pose
    }
}

const FLAG_LOOPING: u32 = 0x4;
const FLAG_EULER: u32 = 0x1000;
const BASE_SCALE: u32 = 1 << 3;
const BASE_ROTATION: u32 = 1 << 4;
const BASE_TRANSLATION: u32 = 1 << 5;

pub(crate) fn read_skeletal_anim(r: &Reader, at: usize) -> Result<SkeletalAnim> {
    if r.slice(at, 4)? != b"FSKA" {
        return Err(FormatError::Invalid("bfres: animation without FSKA magic"));
    }
    let name = r.string_at(at + 0x04)?.unwrap_or_default();
    let flags = r.u32(at + 0x0C)?;
    let frame_count = r.i32(at + 0x10)? as f32;
    let bone_count = r.u16(at + 0x14)? as usize;
    let bones = match r.pointer(at + 0x20)? {
        Some(list) => (0..bone_count)
            .map(|i| read_bone(r, list + i * 24))
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    // SI-ANM-12: Sampled curves interpolated linearly; scale mode ignored.
    Ok(SkeletalAnim {
        name,
        frame_count,
        looping: flags & FLAG_LOOPING != 0,
        euler: flags & FLAG_EULER != 0,
        bones,
    })
}

fn read_bone(r: &Reader, at: usize) -> Result<BoneAnim> {
    let flags = r.u32(at)?;
    let name = r.string_at(at + 0x04)?.unwrap_or_default();
    let curve_count = r.u8(at + 0x0A)? as usize;
    let curves = match r.pointer(at + 0x10)? {
        Some(list) => (0..curve_count)
            .filter_map(|i| read_curve(r, list + i * 0x24).transpose())
            .collect::<Result<_>>()?,
        None => Vec::new(),
    };
    let (mut base_scale, mut base_rotation, mut base_translation) = (None, None, None);
    if let Some(mut p) = r.pointer(at + 0x14)? {
        let mut floats = |n: usize| -> Result<Vec<f32>> {
            let values = (0..n).map(|i| r.f32(p + i * 4)).collect();
            p += n * 4;
            values
        };
        if flags & BASE_SCALE != 0 {
            base_scale = Some(floats(3)?.try_into().unwrap());
        }
        if flags & BASE_ROTATION != 0 {
            base_rotation = Some(floats(4)?.try_into().unwrap());
        }
        if flags & BASE_TRANSLATION != 0 {
            base_translation = Some(floats(3)?.try_into().unwrap());
        }
    }
    Ok(BoneAnim {
        name,
        flags,
        base_scale,
        base_rotation,
        base_translation,
        curves,
    })
}

/// A curve, or `None` for kinds a skeleton does not use (integer, boolean).
fn read_curve(r: &Reader, at: usize) -> Result<Option<Curve>> {
    let flags = r.u16(at)?;
    let count = r.u16(at + 0x02)? as usize;
    let Some(target) = Target::from_offset(r.u32(at + 0x04)?) else {
        return Ok(None);
    };
    let scale = r.f32(at + 0x10)?;
    let offset = r.f32(at + 0x14)?;
    let (kind, per_key) = match (flags >> 4) & 7 {
        0 => (CurveKind::Cubic, 4),
        1 => (CurveKind::Linear, 2),
        2 => (CurveKind::Sampled, 1),
        _ => return Ok(None),
    };
    let frames_at = r
        .pointer(at + 0x1C)?
        .ok_or(FormatError::Invalid("bfres: curve without frames"))?;
    let keys_at = r
        .pointer(at + 0x20)?
        .ok_or(FormatError::Invalid("bfres: curve without keys"))?;
    let frames = (0..count)
        .map(|i| match flags & 3 {
            0 => r.f32(frames_at + i * 4),
            1 => Ok(r.u16(frames_at + i * 2)? as i16 as f32 / 32.0),
            _ => Ok(r.u8(frames_at + i)? as f32),
        })
        .collect::<Result<Vec<_>>>()?;
    let raw = |i: usize| -> Result<f32> {
        match (flags >> 2) & 3 {
            0 => r.f32(keys_at + i * 4),
            1 => Ok(r.u16(keys_at + i * 2)? as i16 as f32),
            _ => Ok(r.u8(keys_at + i)? as i8 as f32),
        }
    };
    let keys = (0..count)
        .map(|k| {
            let mut key = [0.0; 4];
            for (c, value) in key.iter_mut().enumerate().take(per_key) {
                *value = raw(k * per_key + c)? * scale + if c == 0 { offset } else { 0.0 };
            }
            Ok(key)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(Curve {
        target,
        kind,
        frames,
        keys,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_cubic_linear_and_sampled_curves() {
        let cubic = Curve {
            target: Target::Rotation(0),
            kind: CurveKind::Cubic,
            frames: vec![0.0, 10.0, 20.0],
            keys: vec![
                [1.0, 2.0, 0.0, 0.0],
                [3.0, 0.0, 0.0, -1.0],
                [2.0, 0.0, 0.0, 0.0],
            ],
        };
        assert_eq!(cubic.sample(-5.0), 1.0);
        assert!((cubic.sample(5.0) - 2.0).abs() < 1e-6);
        assert!((cubic.sample(15.0) - 2.875).abs() < 1e-6);
        assert_eq!(cubic.sample(25.0), 2.0);

        let linear = Curve {
            kind: CurveKind::Linear,
            keys: vec![[0.0, 4.0, 0.0, 0.0], [4.0, -4.0, 0.0, 0.0], [0.0; 4]],
            ..cubic.clone()
        };
        assert!((linear.sample(12.5) - 3.0).abs() < 1e-6);

        let sampled = Curve {
            kind: CurveKind::Sampled,
            keys: vec![[0.0; 4], [10.0, 0.0, 0.0, 0.0], [20.0, 0.0, 0.0, 0.0]],
            ..cubic
        };
        assert!((sampled.sample(5.0) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn partial_curves_preserve_bound_components_and_authored_bases() {
        let bind = BonePose {
            scale: Some([0.8, 1.2, 0.9]),
            rotation: Some([0.2, 0.3, 0.4, 1.0]),
            translation: Some([2.0, 3.0, 4.0]),
        };
        let mut bone = BoneAnim {
            name: "Spine_1".into(),
            flags: BASE_ROTATION,
            base_scale: None,
            base_rotation: Some([0.5, 0.6, 0.7, 1.0]),
            base_translation: None,
            curves: vec![
                Curve {
                    target: Target::Scale(0),
                    kind: CurveKind::Linear,
                    frames: vec![0.0, 2.0],
                    keys: vec![[1.0, 0.5, 0.0, 0.0], [1.5, 0.0, 0.0, 0.0]],
                },
                Curve {
                    target: Target::Translation(1),
                    kind: CurveKind::Sampled,
                    frames: vec![0.0],
                    keys: vec![[7.0, 0.0, 0.0, 0.0]],
                },
            ],
        };
        let pose = bone.sample_with_bind(1.0, bind);
        assert_eq!(pose.scale, Some([1.25, 1.2, 0.9]));
        assert_eq!(pose.translation, Some([2.0, 7.0, 4.0]));
        assert_eq!(pose.rotation, bone.base_rotation);
        // A channel with no authored base or curve remains exactly the bind pose.
        bone.base_rotation = None;
        assert_eq!(bone.sample_with_bind(1.0, bind).rotation, bind.rotation);
        // Callers without a skeleton retain the existing sparse sampling contract.
        let sparse = bone.sample(1.0);
        assert_eq!(sparse.scale, Some([1.25, 1.0, 1.0]));
        assert_eq!(sparse.translation, Some([0.0, 7.0, 0.0]));
        assert_eq!(sparse.rotation, None);
    }

    #[test]
    fn curves_override_base_values() {
        let bone = BoneAnim {
            name: "Spine_1".into(),
            flags: BASE_SCALE | BASE_ROTATION,
            base_scale: Some([1.0; 3]),
            base_rotation: Some([0.5, 0.25, 0.0, 1.0]),
            base_translation: None,
            curves: vec![Curve {
                target: Target::Rotation(1),
                kind: CurveKind::Sampled,
                frames: vec![0.0],
                keys: vec![[0.75, 0.0, 0.0, 0.0]],
            }],
        };
        let pose = bone.sample(3.0);
        assert_eq!(pose.rotation, Some([0.5, 0.75, 0.0, 1.0]));
        assert_eq!(pose.scale, Some([1.0; 3]));
        assert_eq!(pose.translation, None);
        assert_eq!(Target::from_offset(0x28), Some(Target::Rotation(2)));
        assert_eq!(Target::from_offset(0x14), Some(Target::Translation(1)));
    }
}
