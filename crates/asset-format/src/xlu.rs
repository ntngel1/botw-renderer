//! Model materials the game draws in its translucent G-buffer pass
//! (`gsys_gbuffer_xlu`, `Model(GBuffer/Xlu)`): model water
//! (`uking_material_behave` 103) and glass / energy (102), lit like the
//! terrain's water by the deferred `field_water` pass (material id 22). See
//! `docs/research/model-water-glass.md`.
//!
//! A [`XluLook`] rides in a model material's [`MaterialInfo`] extras
//! (`MaterialInfo::xlu`); readers that do not know it skip it, and a
//! material without one is drawn as an ordinary object.
//!
//! [`MaterialInfo`]: crate::model::MaterialInfo

use serde::{Deserialize, Serialize};

/// Which G-buffer program family a material uses; each is a formula of its
/// own (`docs/research/model-water-glass.md`, "Programs"). The census of
/// the 204 behave 102/103 materials splits them into option signatures;
/// a material's options decide its family (`bake/src/xlu.rs`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum XluKind {
    /// Not one of the families ported: drawn as an ordinary object.
    #[default]
    None,
    /// Basic model water, PS 7965 (families 7920/7932/7944/7956): the
    /// TeraWater formula with the material's constants.
    Water,
    /// Water whose every output is blended into the G-buffer by the vertex
    /// alpha (`uking_enable_gbuffer_xlu_blend`), PS 7977 (7968/7980).
    WaterBlend,
    /// Glass: the fogged scene behind, tinted at grazing angles, PS 7845
    /// (7824/7836).
    Glass,
    /// Glass that bends the scene behind by its normal map
    /// (`uking_enable_indirect1`), PS 7869/7881 (7860/7872).
    RefractingGlass,
    /// Waterfalls of families 11412/11424, PS 11433: two colour and two
    /// normal layers over the depth behind, blended by the vertex alpha,
    /// pushed along the normal by a height texture.
    Waterfall,
    /// Waterfalls of family 11400, PS 11409: as [`Self::Waterfall`], but
    /// the opacity is not scaled by the vertex alpha, the blend is by its
    /// square, and back faces keep their normal.
    WaterfallSquaredAlpha,
    /// Waterfall foam of families 11256/11268/11280/11292, PS 11265/11301:
    /// the two layers mixed by the vertex red (`saturate(2r²)`), the second
    /// on the mesh's second UV set, opacity from the colour's alpha and
    /// the vertex red; no height push.
    WaterfallMixed,
}

/// How a texture coordinate set is made in the vertex shader
/// (`uking_texcoordN_mapping`, `uking_texcoordN_srt`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct XluTexCoord {
    /// 0: the mesh's first UV set; 1: its second; 11: world x/z (read from
    /// VS 7965 and VS 11301).
    pub mapping: u32,
    /// The `tex_srtN` applied after the mapping, or −1 for none.
    pub srt: i32,
}

/// What the G-buffer program of a translucent model material reads beyond
/// the mesh: the family, its texture coordinates, the material's constants
/// and textures, and the texture-SRT animation that scrolls them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct XluLook {
    pub kind: XluKind,
    /// Texture coordinate sets 0–3.
    pub texcoords: [XluTexCoord; 4],
    /// `const_color0–5`.
    pub const_color: [[f32; 4]; 6],
    /// `const_value0–7`.
    pub const_value: [f32; 8],
    /// `indirect_scale0–5`.
    pub indirect_scale: [[f32; 2]; 6],
    /// `tex_srt0–5`: mode, scale x/y, rotation, translation x/y.
    pub tex_srt: [[f32; 6]; 6],
    /// The shader's samplers (`_a0`, `_s0`, `_n0`, `_e0`, `_t0`, and the
    /// waterfalls' vertex texture `_v0`) and the
    /// KTX2 file (in the model's folder) each reads, after the material's
    /// sampler assignment. Normal maps are stored like
    /// `MaterialTextures::normal` (red and green kept).
    pub samplers: Vec<(String, String)>,
    /// The model's texture-SRT animation of this material, if it plays one.
    pub animation: Option<SrtAnimation>,
}

impl XluLook {
    /// The texture file the shader's sampler `slot` reads.
    pub fn sampler(&self, slot: &str) -> Option<&str> {
        self.samplers
            .iter()
            .find(|(s, _)| s == slot)
            .map(|(_, t)| t.as_str())
    }

    /// `tex_srt[i]` at animation frame `frame` (the static values without
    /// an animation).
    pub fn tex_srt_at(&self, i: usize, frame: f32) -> [f32; 6] {
        let mut srt = self.tex_srt[i];
        if let Some(animation) = &self.animation {
            animation.apply(i, frame, &mut srt);
        }
        srt
    }
}

// SI-WAT-04: texture SRT as Maya rotation without pivot.
/// A `tex_srt` value as the 2×3 matrix its vertex shader multiplies by
/// (two vectors, column-major: `(m00, m10, m01, m11)`, `(m02, m12, –, –)`):
/// `u' = sx (cos u + sin v) + tx`, `v' = sy (−sin u + cos v) + ty`, the
/// usual Maya-mode rotation without the pivot (as `TeraWater`'s
/// `texture_spaces`).
pub fn srt_matrix(srt: [f32; 6]) -> [[f32; 4]; 2] {
    let [_mode, sx, sy, rotation, tx, ty] = srt;
    let (sin, cos) = rotation.sin_cos();
    [
        [sx * cos, -sy * sin, sx * sin, sy * cos],
        [tx, ty, 0.0, 0.0],
    ]
}

/// A model file's animation of a material's `tex_srtN` parameters (BFRES
/// `FSHU`, the texture-SRT and shader-parameter animation groups): curves
/// over the parameters' components, played on a loop.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SrtAnimation {
    /// The animation's name in the model file.
    pub name: String,
    /// Its length in frames (the game's 30 Hz frames).
    pub frames: f32,
    /// It starts over at the end (flag 4).
    pub looping: bool,
    pub curves: Vec<SrtCurve>,
    /// Components set to a fixed value: `(tex_srt index, component, value)`.
    pub constants: Vec<(u8, u8, f32)>,
}

impl SrtAnimation {
    /// The frame of the animation `seconds` after it started.
    pub fn frame_at(&self, seconds: f32) -> f32 {
        let frame = seconds * 30.0;
        if self.frames <= 0.0 {
            0.0
        } else if self.looping {
            frame.rem_euclid(self.frames)
        } else {
            frame.min(self.frames)
        }
    }

    /// Writes its values for `tex_srt[srt]` at `frame` into `values`.
    pub fn apply(&self, srt: usize, frame: f32, values: &mut [f32; 6]) {
        for &(i, component, value) in &self.constants {
            if usize::from(i) == srt && usize::from(component) < 6 {
                values[usize::from(component)] = value;
            }
        }
        for curve in &self.curves {
            if usize::from(curve.srt) == srt && usize::from(curve.component) < 6 {
                values[usize::from(curve.component)] = curve.sample(frame);
            }
        }
    }

    /// Whether it changes `tex_srt[srt]` over time.
    pub fn moves(&self, srt: usize) -> bool {
        self.curves.iter().any(|c| usize::from(c.srt) == srt)
    }
}

/// How a BFRES animation curve is interpolated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurveKind {
    /// Per key `a + t(b + t(c + t·d))` over the segment.
    #[default]
    Cubic,
    /// Per key `a + b·t`.
    Linear,
    /// One value per key.
    Step,
}

/// What a curve does before its first or after its last frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wrap {
    #[default]
    Clamp,
    Repeat,
    Mirror,
    /// Repeats, adding the curve's `delta` each time round.
    Relative,
}

impl Wrap {
    /// From the curve flags' two-bit field.
    pub fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            1 => Self::Repeat,
            2 => Self::Mirror,
            3 => Self::Relative,
            _ => Self::Clamp,
        }
    }
}

/// One component of a `tex_srtN` over time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SrtCurve {
    /// The `tex_srt` index.
    pub srt: u8,
    /// 0 mode, 1–2 scale, 3 rotation, 4–5 translation (the curve's byte
    /// offset into the parameter over 4).
    pub component: u8,
    pub kind: CurveKind,
    pub start: f32,
    pub end: f32,
    pub pre_wrap: Wrap,
    pub post_wrap: Wrap,
    /// The value's change over the curve (for [`Wrap::Relative`]).
    pub delta: f32,
    pub frames: Vec<f32>,
    /// Scaled coefficients per key, as [`CurveKind`] says.
    pub keys: Vec<[f32; 4]>,
}

impl SrtCurve {
    pub fn sample(&self, frame: f32) -> f32 {
        let Some(first) = self.keys.first() else {
            return 0.0;
        };
        let length = self.end - self.start;
        if length <= 0.0 {
            return first[0];
        }
        let wrap = if frame < self.start {
            self.pre_wrap
        } else if frame > self.end {
            self.post_wrap
        } else {
            Wrap::Clamp
        };
        let cycles = ((frame - self.start) / length).floor();
        let (local, shift) = match wrap {
            Wrap::Clamp => (frame.clamp(self.start, self.end), 0.0),
            Wrap::Repeat => (self.start + (frame - self.start).rem_euclid(length), 0.0),
            Wrap::Relative => (
                self.start + (frame - self.start).rem_euclid(length),
                cycles * self.delta,
            ),
            Wrap::Mirror => {
                let t = (frame - self.start).rem_euclid(length);
                let backwards = (cycles as i64).rem_euclid(2) == 1;
                (
                    if backwards {
                        self.end - t
                    } else {
                        self.start + t
                    },
                    0.0,
                )
            }
        };
        self.at(local) + shift
    }

    /// The value at `frame` within the curve's keys.
    fn at(&self, frame: f32) -> f32 {
        let i = self
            .frames
            .partition_point(|&f| f <= frame)
            .saturating_sub(1)
            .min(self.keys.len() - 1);
        let key = self.keys[i];
        let (Some(&from), Some(&to)) = (self.frames.get(i), self.frames.get(i + 1)) else {
            return key[0];
        };
        let t = ((frame - from) / (to - from).max(1e-6)).clamp(0.0, 1.0);
        match self.kind {
            CurveKind::Cubic => key[0] + t * (key[1] + t * (key[2] + t * key[3])),
            CurveKind::Linear => key[0] + key[1] * t,
            CurveKind::Step => key[0],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DgnObj_DungeonWater50x50_A_01_auto`'s first curve: tex_srt0's
    /// translation x, linear from −0.118 by 30 over 4000 frames.
    fn scroll() -> SrtCurve {
        SrtCurve {
            srt: 0,
            component: 4,
            kind: CurveKind::Linear,
            start: 0.0,
            end: 4000.0,
            delta: 30.0,
            frames: vec![0.0, 4000.0],
            keys: vec![[-0.118, 30.0, 0.0, 0.0], [29.882, 0.0, 0.0, 0.0]],
            ..Default::default()
        }
    }

    #[test]
    fn samples_linear_and_cubic_curves() {
        let curve = scroll();
        assert!((curve.sample(2000.0) - 14.882).abs() < 1e-4);
        assert!((curve.sample(-5.0) + 0.118).abs() < 1e-6);
        // A cubic wave over 0–800 that repeats (`_auto` tex_srt2).
        let wave = SrtCurve {
            kind: CurveKind::Cubic,
            start: 0.0,
            end: 800.0,
            pre_wrap: Wrap::Repeat,
            post_wrap: Wrap::Repeat,
            frames: vec![0.0, 400.0, 800.0],
            keys: vec![
                [-0.496, 0.0, 3.0, -2.008],
                [0.496, 0.0, -3.0, 2.008],
                [-0.496, 0.0, 0.0, 0.0],
            ],
            ..Default::default()
        };
        assert!((wave.sample(400.0) - 0.496).abs() < 1e-6);
        assert!((wave.sample(1200.0) - wave.sample(400.0)).abs() < 1e-5);
        assert!((wave.sample(-400.0) - wave.sample(400.0)).abs() < 1e-5);
    }

    #[test]
    fn relative_curves_keep_climbing() {
        let mut curve = scroll();
        curve.post_wrap = Wrap::Relative;
        assert!((curve.sample(6000.0) - (curve.sample(2000.0) + 30.0)).abs() < 1e-3);
    }

    #[test]
    fn animations_loop_and_override_their_components() {
        let animation = SrtAnimation {
            frames: 4000.0,
            looping: true,
            curves: vec![scroll()],
            constants: vec![(0, 0, 0.0), (0, 5, 0.25)],
            ..Default::default()
        };
        assert!((animation.frame_at(150.0) - 500.0).abs() < 1e-3);
        let mut srt = [1.0, 0.1, 0.1, 0.0, 0.0, 0.0];
        animation.apply(0, 2000.0, &mut srt);
        assert_eq!(srt[..4], [0.0, 0.1, 0.1, 0.0]);
        assert!((srt[4] - 14.882).abs() < 1e-4 && srt[5] == 0.25);
        assert!(animation.moves(0) && !animation.moves(1));
    }

    #[test]
    fn srt_matrices_scale_and_turn() {
        let [m, t] = srt_matrix([0.0, 2.0, 3.0, 0.0, 0.5, 0.25]);
        assert_eq!(m, [2.0, 0.0, 0.0, 3.0]);
        assert_eq!(t, [0.5, 0.25, 0.0, 0.0]);
    }

    #[test]
    fn old_extras_read_without_the_new_fields() {
        let look: XluLook = serde_json::from_str(r#"{"kind":"Glass"}"#).unwrap();
        assert_eq!(look.kind, XluKind::Glass);
        assert!(look.animation.is_none() && look.samplers.is_empty());
    }
}
