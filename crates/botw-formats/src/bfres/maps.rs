//! What a `uking_mat` material draws from its textures besides albedo and
//! normals, read from its shader options (see archived FORMATS.md notes, "Material
//! maps"). The shader has eight texture slots, its samplers in this
//! order: `_a0 _s0 _n0 _e0 _t0 _a1 array0 array1`; the material's sampler
//! assignment binds each to one of its own textures (a wall's `_s0` slot
//! may hold its `_n0` normal map). Options name inputs by number: `0`–`7` a
//! slot, `100`–`107` `const_colorN`, `200`–`209` the result of the
//! `uking_colorN_*` calculation, `300` white; a companion `_channel` option
//! picks `10` red, `20` green, `30` blue, `40` alpha or `0` all of RGB.
//! Only the calculations whose meaning is clear from the data are followed:
//! pass-through and products of inputs; colours whose calculation is off
//! count as none.

use super::model::Material;

/// The shader's texture slots, in order.
pub const SLOTS: [&str; 8] = ["_a0", "_s0", "_n0", "_e0", "_t0", "_a1", "array0", "array1"];

/// A texture channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    Rgb,
    R,
    G,
    B,
    A,
}

impl Channel {
    fn from_option(value: i32) -> Option<Self> {
        // `x1` variants (11, 41) exist too; taken as the plain channel.
        // SI-FMT-09: material channel meanings are our reading.
        Some(match value / 10 {
            0 => Self::Rgb,
            1 => Self::R,
            2 => Self::G,
            3 => Self::B,
            4 => Self::A,
            _ => return None,
        })
    }

    /// Index into RGBA, or `None` for all of RGB.
    pub fn index(self) -> Option<usize> {
        match self {
            Self::Rgb => None,
            Self::R => Some(0),
            Self::G => Some(1),
            Self::B => Some(2),
            Self::A => Some(3),
        }
    }
}

/// `(material sampler, texture name, channel)`.
pub type TextureChannel = (String, String, Channel);

/// Up to two texture channels times a colour, or just a colour.
#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub texture: Option<TextureChannel>,
    /// A second channel the first is multiplied by (a glow mask times a
    /// pattern), only with `texture`.
    pub second: Option<TextureChannel>,
    /// Linear, may exceed 1 (the game's HDR units).
    pub color: [f32; 3],
}

impl Term {
    fn constant(color: [f32; 3]) -> Self {
        Self {
            texture: None,
            second: None,
            color,
        }
    }

    fn textured(texture: TextureChannel) -> Self {
        Self {
            texture: Some(texture),
            second: None,
            color: [1.0; 3],
        }
    }

    /// The product of two terms, if it takes at most two texture channels.
    fn times(self, other: Term) -> Option<Term> {
        let mut textures = [self.texture, self.second, other.texture, other.second]
            .into_iter()
            .flatten();
        let (texture, second) = (textures.next(), textures.next());
        if textures.next().is_some() {
            return None;
        }
        Some(Term {
            texture,
            second,
            color: std::array::from_fn(|i| self.color[i] * other.color[i]),
        })
    }

    /// All the texture channels, first first.
    pub fn textures(&self) -> impl Iterator<Item = &TextureChannel> {
        self.texture.iter().chain(&self.second)
    }

    /// Whether the term is black whatever the texture holds.
    pub fn is_black(&self) -> bool {
        self.color.iter().all(|&c| c <= 0.0)
    }
}

impl Material {
    fn option_i32(&self, key: &str) -> Option<i32> {
        self.shader_option(key)?.parse().ok()
    }

    /// The material sampler and texture bound to shader slot `slot`.
    pub fn slot_texture(&self, slot: usize) -> Option<(&str, &str)> {
        let shader = SLOTS.get(slot)?;
        let (_, sampler) = self.sampler_assign.iter().find(|(s, _)| s == shader)?;
        Some((sampler.as_str(), self.texture(sampler)?))
    }

    /// Follows input `value` (with its `channel` option value).
    fn input(&self, value: i32, channel: i32, depth: u32) -> Option<Term> {
        let channel = Channel::from_option(channel)?;
        match value {
            0..=7 => {
                let (sampler, texture) = self.slot_texture(value as usize)?;
                Some(Term::textured((
                    sampler.to_owned(),
                    texture.to_owned(),
                    channel,
                )))
            }
            100..=107 => {
                let c = self.shader_param(&format!("const_color{}", value - 100))?;
                let rgb = [*c.first()?, *c.get(1)?, *c.get(2)?];
                Some(Term::constant(match channel.index() {
                    Some(i) => [*c.get(i)?; 3],
                    None => rgb,
                }))
            }
            200..=209 if depth < 4 => self.computed(value as u32 - 200, depth + 1),
            300 => Some(Term::constant([1.0; 3])),
            _ => None,
        }
    }

    /// The result of `uking_colorN_*`.
    fn computed(&self, n: u32, depth: u32) -> Option<Term> {
        let part = |p: &str| -> Option<Term> {
            let value = self.option_i32(&format!("uking_color{n}_{p}"))?;
            let channel = self
                .option_i32(&format!("uking_color{n}_{p}_channel"))
                .unwrap_or(0);
            self.input(value, channel, depth)
        };
        let enabled = self.option_i32(&format!("uking_enable_calc_color{n}")) == Some(1);
        let calc = self
            .option_i32(&format!("uking_color{n}_calc_type"))
            .unwrap_or(0);
        match (enabled, calc) {
            // A colour left off is not computed (world materials point their
            // metal at such a colour).
            (false, _) => None,
            (true, 0) => part("A"),
            // Products of three and four inputs (mask × colour × intensity).
            (true, 9) => part("A")?.times(part("B")?)?.times(part("C")?),
            (true, 11) => part("A")?
                .times(part("B")?)?
                .times(part("C")?)?
                .times(part("D")?),
            _ => None,
        }
    }

    /// What the material emits, if it glows and the shader's recipe for it
    /// can be followed.
    pub fn emission(&self) -> Option<Term> {
        if self.option_i32("uking_enable_emission") != Some(1) {
            return None;
        }
        let value = self.option_i32("uking_emission_color")?;
        let term = self.input(value, 0, 0)?;
        (!term.is_black()).then_some(term)
    }

    /// Where the material's metal mask comes from. `0` is the albedo slot,
    /// which no material means as metal (most materials leave the option
    /// at 0), so it counts as none. Only a metal map (`_mt0`) or a `_Spm`
    /// map (`_s0`) counts: characters leave the option at 3 whatever their
    /// slot 3 holds (Link's skin binds a damage albedo there, Miis their AO),
    /// and some walls point it at their normal map, which would turn skin
    /// and walls to metal.
    pub fn metalness(&self) -> Option<Term> {
        let value = self.option_i32("uking_metal_color")?;
        if value == 0 {
            return None;
        }
        let channel = self.option_i32("uking_metal_channel").unwrap_or(10);
        let term = self.input(value, channel, 0)?;
        let metal_map = term
            .texture
            .as_ref()
            .is_none_or(|(sampler, _, _)| sampler.starts_with("_mt") || sampler == "_s0");
        (metal_map && term.second.is_none() && !term.is_black()).then_some(term)
    }

    /// The colour of a material without an albedo texture, where its
    /// recipe (`uking_albedo_color`) is a plain colour: a `const_colorN`
    /// (a horse's mane, Ganon's blades) or a product of them. The water of
    /// dungeons, Zora's Domain and the castle (75 materials) replaces its
    /// albedo (`uking_enable_replace_albedo`) with `const_color3`.
    pub fn albedo_constant(&self) -> Option<[f32; 3]> {
        if self.option_i32("uking_enable_replace_albedo") == Some(1)
            && let Some(term) = self.input(self.option_i32("uking_replace_albedo_color")?, 0, 0)
            && term.texture.is_none()
        {
            return Some(term.color);
        }
        let value = self.option_i32("uking_albedo_color")?;
        let channel = self.option_i32("uking_albedo_channel").unwrap_or(0);
        let term = self.input(value, channel, 0)?;
        term.texture.is_none().then_some(term.color)
    }

    /// Whether the material is malice: all 111 materials that take their
    /// albedo from input 113 are `Grudge` ones (the game supplies that
    /// colour itself; it is in no parameter).
    pub fn is_malice(&self) -> bool {
        self.option_i32("uking_albedo_color") == Some(113)
    }

    /// The specular mask: a channel of slot 1 (`_s0`: a `_Spm` map, or the
    /// blue channel of a normal map bound there). Red and green of a normal
    /// map are its normal, never a mask.
    pub fn specular_mask(&self) -> Option<Term> {
        let channel = self.option_i32("uking_specular_channel")?;
        let term = self.input(1, channel, 0)?;
        let (sampler, _, channel) = term.texture.as_ref()?;
        let normal = sampler.starts_with("_n") || sampler == "tmc";
        (!(normal && matches!(channel, Channel::R | Channel::G | Channel::Rgb))).then_some(term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bfres::model::ShaderParam;

    fn material(
        textures: &[(&str, &str)],
        assign: &[(&str, &str)],
        options: &[(&str, &str)],
    ) -> Material {
        let pairs = |list: &[(&str, &str)]| {
            list.iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect()
        };
        Material {
            name: "m".into(),
            textures: pairs(textures),
            render_info: Vec::new(),
            shader_archive: "uking_mat".into(),
            shading_model: "uking_mat".into(),
            shader_options: pairs(options),
            sampler_assign: pairs(assign),
            samplers: Vec::new(),
            render_state: Default::default(),
            shader_params: vec![
                ShaderParam {
                    name: "const_color0".into(),
                    kind: 15,
                    values: vec![20.0, 5.0, 0.3, 1.0],
                },
                ShaderParam {
                    name: "const_color4".into(),
                    kind: 15,
                    values: vec![1.0, 1.0, 1.0, 1.0],
                },
            ],
        }
    }

    /// A lava crater's material, as in `FldObj_CliffDeathMt_B-00`.
    fn crater() -> Material {
        material(
            &[
                ("_a0", "Lava_Alb"),
                ("_e0", "Lava_Emm"),
                ("_n0", "Lava_Nrm"),
            ],
            &[
                ("_a0", "_a0"),
                ("_s0", "_n0"),
                ("_n0", "_e0"),
                ("_e0", "_e0"),
            ],
            &[
                ("uking_enable_emission", "1"),
                ("uking_emission_color", "202"),
                ("uking_enable_calc_color2", "1"),
                ("uking_color2_calc_type", "9"),
                ("uking_color2_A", "2"),
                ("uking_color2_A_channel", "10"),
                ("uking_color2_B", "100"),
                ("uking_color2_C", "104"),
                ("uking_metal_color", "201"),
                ("uking_metal_channel", "10"),
                ("uking_color1_A", "1"),
                ("uking_color1_A_channel", "30"),
                ("uking_specular_channel", "30"),
            ],
        )
    }

    #[test]
    fn emission_is_the_mask_times_the_colours() {
        let emission = crater().emission().unwrap();
        assert_eq!(
            emission.texture,
            Some(("_e0".into(), "Lava_Emm".into(), Channel::R))
        );
        assert_eq!(emission.color, [20.0, 5.0, 0.3]);
    }

    #[test]
    fn emission_may_take_two_masks() {
        // Slot 2's red times slot 3's alpha times two colours.
        let mut m = crater();
        m.shader_options
            .retain(|(k, _)| !k.starts_with("uking_color2"));
        for (k, v) in [
            ("calc_type", "11"),
            ("A", "2"),
            ("A_channel", "10"),
            ("B", "100"),
            ("C", "3"),
            ("C_channel", "40"),
            ("D", "104"),
        ] {
            m.shader_options
                .push((format!("uking_color2_{k}"), v.into()));
        }
        let emission = m.emission().unwrap();
        assert_eq!(
            emission.texture,
            Some(("_e0".into(), "Lava_Emm".into(), Channel::R))
        );
        assert_eq!(
            emission.second,
            Some(("_e0".into(), "Lava_Emm".into(), Channel::A))
        );
        assert_eq!(emission.color, [20.0, 5.0, 0.3]);
        assert_eq!(emission.textures().count(), 2);
    }

    #[test]
    fn textureless_albedo_comes_from_a_constant() {
        let mut m = crater();
        m.shader_options
            .push(("uking_albedo_color".into(), "100".into()));
        assert_eq!(m.albedo_constant(), Some([20.0, 5.0, 0.3]));
        m.shader_options.retain(|(k, _)| k != "uking_albedo_color");
        m.shader_options
            .push(("uking_albedo_color".into(), "0".into()));
        assert_eq!(m.albedo_constant(), None, "slot 0 is a texture");
        assert!(!m.is_malice());
        m.shader_options.retain(|(k, _)| k != "uking_albedo_color");
        m.shader_options
            .push(("uking_albedo_color".into(), "113".into()));
        assert!(m.is_malice());
        // Water replaces its albedo with a colour.
        m.shader_options
            .push(("uking_enable_replace_albedo".into(), "1".into()));
        m.shader_options
            .push(("uking_replace_albedo_color".into(), "104".into()));
        assert_eq!(m.albedo_constant(), Some([1.0, 1.0, 1.0]));
    }

    #[test]
    fn slot_one_holds_the_specular_mask() {
        let m = crater();
        // The wall's normal map sits in slot 1: its blue channel is the mask.
        assert_eq!(
            m.specular_mask().unwrap().texture,
            Some(("_n0".into(), "Lava_Nrm".into(), Channel::B))
        );
        // Metal points at colour 1, which is off: no metal.
        assert_eq!(m.metalness(), None);
    }

    #[test]
    fn spm_maps_hold_specular_and_metal() {
        // `Weapon_Sword_001`: metal is colour 0, the Spm map's green.
        let m = material(
            &[
                ("_a0", "Sword_Alb"),
                ("_n0", "Sword_Nrm"),
                ("_s0", "Sword_Spm"),
            ],
            &[("_a0", "_a0"), ("_s0", "_s0"), ("_n0", "_n0")],
            &[
                ("uking_enable_calc_color0", "1"),
                ("uking_color0_A", "1"),
                ("uking_color0_A_channel", "20"),
                ("uking_metal_color", "200"),
                ("uking_metal_channel", "10"),
                ("uking_specular_channel", "10"),
            ],
        );
        assert_eq!(
            m.metalness().unwrap().texture,
            Some(("_s0".into(), "Sword_Spm".into(), Channel::G))
        );
        assert_eq!(
            m.specular_mask().unwrap().texture,
            Some(("_s0".into(), "Sword_Spm".into(), Channel::R))
        );
        assert_eq!(m.emission(), None);
    }

    #[test]
    fn unclear_recipes_are_left_alone() {
        let mut m = crater();
        // Unbound slot, unknown calculation, the albedo slot as metal, a
        // normal map's red as specular.
        m.sampler_assign.retain(|(s, _)| s != "_n0");
        assert_eq!(m.emission(), None);
        let mut m = crater();
        m.shader_options
            .iter_mut()
            .find(|(k, _)| k == "uking_color2_calc_type")
            .unwrap()
            .1 = "5".into();
        assert_eq!(m.emission(), None);
        m.shader_options
            .iter_mut()
            .find(|(k, _)| k == "uking_metal_color")
            .unwrap()
            .1 = "0".into();
        assert_eq!(m.metalness(), None);
        m.shader_options
            .iter_mut()
            .find(|(k, _)| k == "uking_metal_color")
            .unwrap()
            .1 = "2".into();
        assert_eq!(
            m.metalness(),
            None,
            "slot 2 holds the emission mask, not metal"
        );
        m.textures.push(("_mt0".into(), "Lava_Mtl".into()));
        m.sampler_assign
            .iter_mut()
            .find(|(s, _)| s == "_n0")
            .unwrap()
            .1 = "_mt0".into();
        assert!(
            m.metalness().is_some(),
            "a metal map in a slot of its own is fine"
        );
        m.shader_options
            .iter_mut()
            .find(|(k, _)| k == "uking_specular_channel")
            .unwrap()
            .1 = "10".into();
        assert_eq!(m.specular_mask(), None);
    }

    #[test]
    fn samplers_are_found_without_their_underscore() {
        let m = material(&[("a0", "Rock_Alb"), ("_n0", "Rock_Nrm")], &[], &[]);
        assert_eq!(m.sampler_texture("_a0"), Some("Rock_Alb"));
        assert_eq!(m.sampler_texture("_n0"), Some("Rock_Nrm"));
        assert_eq!(m.sampler_texture("_ms0"), None);
    }
}
