//! Model water and glass (`uking_material_behave` 103 / 102): the
//! materials the game draws in its translucent G-buffer pass, and the
//! model files' texture-SRT animations that scroll them
//! (`asset_format::xlu`, `docs/research/model-water-glass.md`).

use asset_format::xlu::{CurveKind, SrtAnimation, SrtCurve, Wrap, XluKind, XluLook, XluTexCoord};
use botw_formats::bfres::TextureImage;
use botw_formats::bfres::gx2;
use botw_formats::bfres::model::Material;

use crate::models::{Textures, game_texture, normal_map_texture};

/// The program family of a translucent G-buffer material, from its
/// options. The census of all 204 behave 102/103 materials (Wii U v208)
/// falls into 23 option signatures, each matching a sampled family once
/// `uking_enable_output_object_attribute` (near / far models) is left
/// aside; these rules pick the families whose G-buffer programs are read:
/// - behave 103 with `uking_enable_scene_color0_depthdiff1_effect` is the
///   basic model water (7920/7932/7944/7956, PS 7965), with
///   `uking_enable_gbuffer_xlu_blend` the blended one (7968/7980, PS 7977);
/// - behave 103 otherwise, blended into the G-buffer, are waterfalls:
///   `uking_color3_calc_type` 21 is family 11412/11424 (PS 11433), 8 is
///   11400 (PS 11409), `uking_normalmap_blend_ratio` 200 is
///   11256/11268/11280/11292 (PS 11265/11301); no other signature has
///   these values;
/// - behave 102 without combiner colours past 1 is glass (7824–7872:
///   PS 7845, refracting with `uking_enable_indirect1`: PS 7869/7881); the
///   one with more (7908, Vah Medoh) has no program in the cache.
// SI-MAT-02: families 11376, 11388, 11436, 11616 and 7908 are ordinary objects.
pub fn kind(material: &Material) -> XluKind {
    let option = |key: &str| material.shader_option(key).unwrap_or("");
    if option("gsys_gbuffer_xlu") != "1" || option("gsys_deferred_shading_material") != "1" {
        return XluKind::None;
    }
    match option("uking_material_behave") {
        // SI-MWT-05: the far water families take PS 7965 (theirs is not in the cache).
        "103" if option("uking_enable_scene_color0_depthdiff1_effect") == "1" => {
            if option("uking_enable_gbuffer_xlu_blend") == "1" {
                XluKind::WaterBlend
            } else {
                XluKind::Water
            }
        }
        "103" if option("uking_enable_gbuffer_xlu_blend") == "1" => {
            match (
                option("uking_color3_calc_type"),
                option("uking_normalmap_blend_ratio"),
            ) {
                ("21", _) => XluKind::Waterfall,
                ("8", _) => XluKind::WaterfallSquaredAlpha,
                (_, "200") => XluKind::WaterfallMixed,
                _ => XluKind::None,
            }
        }
        "102" if option("uking_enable_calc_color2") == "0" => {
            if option("uking_enable_indirect1") == "1" {
                XluKind::RefractingGlass
            } else {
                XluKind::Glass
            }
        }
        _ => XluKind::None,
    }
}

/// The shader samplers each family reads (from its G-buffer program).
fn samplers(kind: XluKind) -> &'static [&'static str] {
    match kind {
        XluKind::None => &[],
        XluKind::Water | XluKind::WaterBlend => &["_s0", "_n0", "_e0", "_t0"],
        XluKind::Glass | XluKind::RefractingGlass => &["_a0", "_s0"],
        // `_v0`: the vertex shader's height texture.
        XluKind::Waterfall | XluKind::WaterfallSquaredAlpha => &["_a0", "_s0", "_n0", "_e0", "_v0"],
        XluKind::WaterfallMixed => &["_a0", "_s0", "_n0", "_e0"],
    }
}

/// What the material's G-buffer program reads, or `None` when it is not a
/// translucent G-buffer material of a ported family. Its textures are
/// written to the folder (normal maps as `MaterialTextures::normal`).
pub fn look(
    material: &Material,
    find_texture: &dyn Fn(&str) -> Option<TextureImage>,
    textures: &mut Textures,
) -> Result<Option<XluLook>, String> {
    let kind = kind(material);
    if kind == XluKind::None {
        return Ok(None);
    }
    let option = |key: &str| {
        material
            .shader_option(key)
            .and_then(|v| v.parse::<i32>().ok())
    };
    let param = |name: &str| material.shader_param(name).unwrap_or(&[]);
    let mut look = XluLook {
        kind,
        ..Default::default()
    };
    for (i, texcoord) in look.texcoords.iter_mut().enumerate() {
        *texcoord = XluTexCoord {
            mapping: option(&format!("uking_texcoord{i}_mapping")).unwrap_or(0) as u32,
            srt: option(&format!("uking_texcoord{i}_srt")).unwrap_or(-1),
        };
    }
    for (i, color) in look.const_color.iter_mut().enumerate() {
        for (c, v) in color.iter_mut().zip(param(&format!("const_color{i}"))) {
            *c = *v;
        }
    }
    for (i, value) in look.const_value.iter_mut().enumerate() {
        *value = param(&format!("const_value{i}"))
            .first()
            .copied()
            .unwrap_or(0.0);
    }
    for (i, scale) in look.indirect_scale.iter_mut().enumerate() {
        for (c, v) in scale.iter_mut().zip(param(&format!("indirect_scale{i}"))) {
            *c = *v;
        }
    }
    for (i, srt) in look.tex_srt.iter_mut().enumerate() {
        *srt = match param(&format!("tex_srt{i}")) {
            &[mode, sx, sy, rotation, tx, ty] => [mode, sx, sy, rotation, tx, ty],
            _ => [0.0, 1.0, 1.0, 0.0, 0.0, 0.0],
        };
    }
    for &slot in samplers(kind) {
        // The shader's sampler reads the material's sampler it is assigned.
        let Some(source) = material
            .sampler_assign
            .iter()
            .find(|(s, _)| s == slot)
            .map(|(_, m)| m.as_str())
        else {
            continue;
        };
        let Some(name) = material.sampler_texture(source) else {
            continue;
        };
        let file = if source.starts_with("_n") {
            match find_texture(name) {
                Some(image) if matches!(image.format, gx2::Format::Bc5 { .. }) => {
                    textures.get(name, || game_texture(&image))?
                }
                Some(image) => {
                    textures.get(&format!("{name}.nrm"), || normal_map_texture(&image))?
                }
                None => None,
            }
        } else {
            textures.get(name, || game_texture(&find_texture(name)?))?
        };
        if let Some(file) = file {
            look.samplers.push((slot.to_owned(), file));
        }
    }
    Ok(Some(look))
}

// --- Texture-SRT animations (BFRES FSHU) ---

/// A model file's material animation: its name and, per material, what
/// it does to the `tex_srtN` parameters.
pub struct MaterialAnimation {
    pub name: String,
    pub materials: Vec<(String, SrtAnimation)>,
}

impl MaterialAnimation {
    /// The one a unit plays by itself: named after the unit with `_Auto`
    /// (any case).
    // SI-MWT-03: the self-playing animation is the unit's `_Auto` one.
    pub fn auto_for<'a>(animations: &'a [Self], unit: &str) -> Option<&'a Self> {
        let wanted = format!("{unit}_auto").to_ascii_lowercase();
        animations
            .iter()
            .find(|a| a.name.to_ascii_lowercase() == wanted)
    }

    pub fn material(&self, name: &str) -> Option<&SrtAnimation> {
        self.materials
            .iter()
            .find(|(m, _)| m == name)
            .map(|(_, a)| a)
    }
}

/// Bounds-checked big-endian reads of a BFRES (Wii U).
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
        self.u32(at).map(f32::from_bits)
    }
    /// The target of the self-relative pointer at `at`.
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
    /// The entries of the index group the pointer at `at` points to.
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

/// The `tex_srtN` animations of a (decompressed) BFRES: the FRES groups of
/// shader-parameter (3) and texture-SRT (5) animations. Layout of FRES
/// version 3.4+ (BotW: 4.5.0.3), as `FSHU` / `ShaderParamMatAnim` /
/// `ParamAnimInfo` / `AnimCurve` are laid out in the Wii U BFRES format;
/// other parameters than `tex_srtN` are left out.
pub fn srt_animations(bytes: &[u8]) -> Vec<MaterialAnimation> {
    let r = Reader(bytes);
    if bytes.get(..4) != Some(b"FRES") {
        return Vec::new();
    }
    [0x2C, 0x34]
        .into_iter()
        .flat_map(|group| r.group(group))
        .filter_map(|(name, at)| read_animation(&r, name, at))
        .collect()
}

fn read_animation(r: &Reader, name: String, at: usize) -> Option<MaterialAnimation> {
    if r.0.get(at..at + 4)? != b"FSHU" {
        return None;
    }
    let flags = r.u32(at + 0x0C)?;
    let frames = r.u32(at + 0x10)? as i32 as f32;
    let count = r.u16(at + 0x14)? as usize;
    let list = r.pointer(at + 0x2C)?;
    let mut materials = Vec::new();
    for m in 0..count {
        let at = list + m * 0x20;
        let params = r.u16(at)? as usize;
        let curve_count = r.u16(at + 2)? as usize;
        let constant_count = r.u16(at + 4)? as usize;
        let material = r.string(at + 0x10)?;
        let infos = r.pointer(at + 0x14);
        let curves_at = r.pointer(at + 0x18);
        let constants_at = r.pointer(at + 0x1C);
        let mut animation = SrtAnimation {
            name: name.clone(),
            frames,
            looping: flags & 4 != 0,
            ..Default::default()
        };
        for p in 0..params {
            let info = infos? + p * 0x10;
            let Some(srt) = r
                .string(info + 0x0C)?
                .strip_prefix("tex_srt")
                .and_then(|n| n.parse::<u8>().ok())
            else {
                continue;
            };
            let first_curve = r.u16(info)? as usize;
            let float_curves = r.u16(info + 2)? as usize;
            let first_constant = r.u16(info + 6)? as usize;
            let constants = r.u16(info + 8)? as usize;
            for c in first_curve..(first_curve + float_curves).min(curve_count) {
                if let Some(curve) = read_curve(r, curves_at? + c * 0x24, srt) {
                    animation.curves.push(curve);
                }
            }
            for c in first_constant..(first_constant + constants).min(constant_count) {
                let at = constants_at? + c * 8;
                let component = (r.u32(at)? / 4) as u8;
                animation.constants.push((srt, component, r.f32(at + 4)?));
            }
        }
        if !animation.curves.is_empty() || !animation.constants.is_empty() {
            materials.push((material, animation));
        }
    }
    Some(MaterialAnimation { name, materials })
}

/// An `AnimCurve` over a float component of `tex_srt[srt]`: flags
/// (frame type bits 0–1, key type 2–3, curve type 4–6, pre-wrap 8–9,
/// post-wrap 12–13), key count, target byte offset, start and end frame,
/// key scale and offset, the value's change (`delta`), frames, keys.
fn read_curve(r: &Reader, at: usize, srt: u8) -> Option<SrtCurve> {
    let flags = r.u16(at)?;
    let count = r.u16(at + 2)? as usize;
    let target = r.u32(at + 4)?;
    let start = r.f32(at + 0x08)?;
    let end = r.f32(at + 0x0C)?;
    let scale = r.f32(at + 0x10)?;
    let offset = r.f32(at + 0x14)?;
    let delta = r.f32(at + 0x18)?;
    let (kind, per_key) = match (flags >> 4) & 7 {
        0 => (CurveKind::Cubic, 4),
        1 => (CurveKind::Linear, 2),
        2 => (CurveKind::Step, 1),
        // Integer and boolean curves do not drive an SRT.
        _ => return None,
    };
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
            0 => r.f32(keys_at + i * 4),
            1 => r.u16(keys_at + i * 2).map(|v| f32::from(v as i16)),
            _ => r.u8(keys_at + i).map(|v| f32::from(v as i8)),
        }
    };
    let keys = (0..count)
        .map(|k| {
            let mut key = [0.0; 4];
            for (c, value) in key.iter_mut().enumerate().take(per_key) {
                *value = raw(k * per_key + c)? * scale + if c == 0 { offset } else { 0.0 };
            }
            Some(key)
        })
        .collect::<Option<Vec<_>>>()?;
    Some(SrtCurve {
        srt,
        component: (target / 4) as u8,
        kind,
        start,
        end,
        pre_wrap: Wrap::from_bits(flags >> 8),
        post_wrap: Wrap::from_bits(flags >> 12),
        delta,
        frames,
        keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn material(options: &[(&str, &str)]) -> Material {
        Material {
            name: "Mt_Water".into(),
            textures: Vec::new(),
            render_info: Vec::new(),
            shader_archive: "uking_mat".into(),
            shading_model: "uking_mat".into(),
            shader_options: options
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            sampler_assign: Vec::new(),
            samplers: Vec::new(),
            render_state: Default::default(),
            shader_params: Vec::new(),
        }
    }

    #[test]
    fn options_pick_the_family() {
        let base = [
            ("gsys_gbuffer_xlu", "1"),
            ("gsys_deferred_shading_material", "1"),
        ];
        let with = |more: &[(&str, &str)]| {
            let mut options = base.to_vec();
            options.extend_from_slice(more);
            kind(&material(&options))
        };
        let water = [
            ("uking_material_behave", "103"),
            ("uking_enable_scene_color0_depthdiff1_effect", "1"),
        ];
        assert_eq!(with(&water), XluKind::Water);
        let mut blend = water.to_vec();
        blend.push(("uking_enable_gbuffer_xlu_blend", "1"));
        assert_eq!(with(&blend), XluKind::WaterBlend);
        assert_eq!(
            with(&[
                ("uking_material_behave", "103"),
                ("uking_color3_calc_type", "21"),
                ("uking_enable_gbuffer_xlu_blend", "1"),
            ]),
            XluKind::Waterfall
        );
        assert_eq!(
            with(&[
                ("uking_material_behave", "103"),
                ("uking_color3_calc_type", "8"),
                ("uking_enable_gbuffer_xlu_blend", "1"),
            ]),
            XluKind::WaterfallSquaredAlpha
        );
        assert_eq!(
            with(&[
                ("uking_material_behave", "103"),
                ("uking_normalmap_blend_ratio", "200"),
                ("uking_enable_gbuffer_xlu_blend", "1"),
            ]),
            XluKind::WaterfallMixed
        );
        assert_eq!(
            with(&[
                ("uking_material_behave", "103"),
                ("uking_color3_calc_type", "8")
            ]),
            XluKind::None
        );
        let glass = [
            ("uking_material_behave", "102"),
            ("uking_enable_calc_color2", "0"),
        ];
        assert_eq!(with(&glass), XluKind::Glass);
        let mut bent = glass.to_vec();
        bent.push(("uking_enable_indirect1", "1"));
        assert_eq!(with(&bent), XluKind::RefractingGlass);
        assert_eq!(
            with(&[
                ("uking_material_behave", "102"),
                ("uking_enable_calc_color2", "1"),
            ]),
            XluKind::None
        );
        assert_eq!(kind(&material(&water)), XluKind::None);
    }
}
