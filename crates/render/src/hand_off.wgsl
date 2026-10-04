// Dissolving a tree's model into its billboard. Both use the same levels as
// Bevy's visibility-range crossfade (-16 to 16) and the same mask of ranks
// (0-1, evenly spread) tiled over the screen: at level n the model hides the
// texels ranked below n/16 and the billboard shows only those.

#define_import_path botw::hand_off

const MASK_SIZE: u32 = 32u;

fn mask_rank(mask: texture_2d<f32>, frag_xy: vec2<f32>) -> f32 {
    return textureLoad(mask, vec2<u32>(frag_xy) % vec2<u32>(MASK_SIZE), 0).r;
}

// Whether a model's pixel at `frag_xy` is dissolved at `level`: positive
// levels fade the model out with distance, negative ones fade it in.
// SI-LGT-23: dissolve levels and linear hand-off level are ours.
fn model_hidden(mask: texture_2d<f32>, frag_xy: vec2<f32>, level: i32) -> bool {
    if level == 0 {
        return false;
    }
    if level <= -16 || level >= 16 {
        return true;
    }
    let rank = mask_rank(mask, frag_xy);
    if level > 0 {
        return rank < f32(level) / 16.0;
    }
    return rank >= f32(16 + level) / 16.0;
}

// Whether a billboard's pixel is still left to its model at `level` (0: all
// of it, 16: none), the complement of `model_hidden`.
fn billboard_hidden(mask: texture_2d<f32>, frag_xy: vec2<f32>, level: i32) -> bool {
    if level >= 16 {
        return false;
    }
    if level <= 0 {
        return true;
    }
    return mask_rank(mask, frag_xy) >= f32(level) / 16.0;
}

// The dissolve level at `distance` for a hand-off between `start` and `end`,
// rounded like Bevy's.
fn hand_off_level(distance: f32, start: f32, end: f32) -> i32 {
    return clamp(i32(round((distance - start) / max(end - start, 1e-3) * 16.0)), 0, 16);
}
