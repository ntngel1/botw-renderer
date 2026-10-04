//! Shared sampling of game skeletal clips for full and incremental baking.
use asset_format::anim::{Clip, Track};
use botw_formats::bfres::anim::SkeletalAnim;
use glam::{EulerRot, Quat};

/// An animation sampled at every frame for every bone it animates (the
/// viewer's `character::bake` before it maps bones to joints): the
/// components it has, Euler rotations as quaternions.
pub(crate) fn sample_clip(anim: &SkeletalAnim) -> Clip {
    let frame_count = anim.frame_count.max(0.0) as u32;
    let samples = frame_count as usize + 1;
    let tracks = anim
        .bones
        .iter()
        .map(|bone| {
            let mut track = Track {
                bone: bone.name.clone(),
                ..Default::default()
            };
            for frame in 0..samples {
                let pose = bone.sample(frame as f32);
                if let Some(t) = pose.translation {
                    track.translation.push(t);
                }
                if let Some([x, y, z, w]) = pose.rotation {
                    let rotation = if anim.euler {
                        Quat::from_euler(EulerRot::ZYX, z, y, x)
                    } else {
                        Quat::from_xyzw(x, y, z, w).normalize()
                    };
                    track.rotation.push(rotation.to_array());
                }
                if let Some(s) = pose.scale {
                    track.scale.push(s);
                }
            }
            // A component is animated at every frame or not at all.
            for (len, clear) in [
                (track.translation.len(), 0),
                (track.rotation.len(), 1),
                (track.scale.len(), 2),
            ] {
                if len != samples {
                    match clear {
                        0 => track.translation.clear(),
                        1 => track.rotation.clear(),
                        _ => track.scale.clear(),
                    }
                }
            }
            track
        })
        .collect();
    Clip {
        name: anim.name.clone(),
        frame_count,
        looping: anim.looping,
        tracks,
    }
}
