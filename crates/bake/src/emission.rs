//! A `uking_mat` material's emission, followed through its colour
//! combiner (`uking_emission_color` and the `uking_colorN_*` it names) as
//! the game's G-buffer program computes it. `botw-formats`' own
//! `Material::emission` (the original format parser's) reads inputs 104–107 as
//! `const_color4–7` and stops at `uking_dynamic_exposure`; the programs
//! say otherwise:
//!
//! - inputs 104–111 are the scalars `const_value0–7`: program 4881 (Cemu
//!   `21d9f489394a0f84`, `uking_color0` = type 9 of 3, 100, 104, emission
//!   200) writes `const_value0 · const_color0 · _e0` to its emission
//!   output; programs 10329–10737 use `const_value0/2/4/6` exactly where
//!   their options name 104/106/108/110;
//! - type 2 is `A · B`: program 6081 (Cemu `53c73e610c4072f7`,
//!   `uking_color0` = type 2 of 104, 507/1) computes
//!   `const_value0 · (1 − uking_dynamic_exposure)`;
//! - input 507 is `uking_dynamic_exposure` (program 6069, channel 0: `e`;
//!   6081, channel 1: `1 − e`), which the game sets from the sky palette's
//!   `Exposure` (`FUN_034086d4` from `ENV_UpdateWeatherPalettes`).
//!
//! Inputs 112–115 are taken as `const_color4–7` (the options list them
//! between `const_color3` and `const_value0`; no program read for them).
//! Like `botw-formats`, only pass-through and products are followed, at
//! most two texture channels; anything else leaves the material dark.

use asset_format::model::DynamicExposure;
use botw_formats::bfres::maps::{Channel, Term};
use botw_formats::bfres::model::Material;

/// What a material emits: its term, and how it depends on
/// `uking_dynamic_exposure` (the term leaves that factor out).
pub struct Emission {
    pub term: Term,
    pub exposure: DynamicExposure,
}

/// The emission of the view's programs (`uking_enable_emission`,
/// `uking_emission_color`).
pub fn emission(material: &Material) -> Option<Emission> {
    of(material, "uking_enable_emission", "uking_emission_color")
}

/// The cube map's (`uking_enable_emission_cubemap`,
/// `uking_emission_color_cubemap`; see `models::cube_map_emission`).
pub fn cube_map_emission(material: &Material) -> Option<Emission> {
    of(
        material,
        "uking_enable_emission_cubemap",
        "uking_emission_color_cubemap",
    )
}

fn of(material: &Material, enable: &str, color: &str) -> Option<Emission> {
    if option(material, enable) != Some(1) {
        return None;
    }
    let mut exposure = DynamicExposure::None;
    let term = input(material, option(material, color)?, 0, 0, &mut exposure)?;
    let black = term.color.iter().all(|&c| c <= 0.0);
    (!black).then_some(Emission { term, exposure })
}

fn option(material: &Material, key: &str) -> Option<i32> {
    material.shader_option(key)?.parse().ok()
}

// SI-FMT-09: material channel meanings are our reading.
fn channel(value: i32) -> Option<Channel> {
    Some(match value / 10 {
        0 => Channel::Rgb,
        1 => Channel::R,
        2 => Channel::G,
        3 => Channel::B,
        4 => Channel::A,
        _ => return None,
    })
}

fn constant(color: [f32; 3]) -> Term {
    Term {
        texture: None,
        second: None,
        color,
    }
}

fn param(material: &Material, name: &str) -> Option<Vec<f32>> {
    material.shader_param(name).map(<[f32]>::to_vec)
}

fn input(
    material: &Material,
    value: i32,
    channel_option: i32,
    depth: u32,
    exposure: &mut DynamicExposure,
) -> Option<Term> {
    let picked = channel(channel_option);
    let color = |name: String| -> Option<Term> {
        let c = param(material, &name)?;
        let rgb = [*c.first()?, *c.get(1)?, *c.get(2)?];
        Some(constant(match picked?.index() {
            Some(i) => [*c.get(i)?; 3],
            None => rgb,
        }))
    };
    match value {
        0..=7 => {
            let (sampler, texture) = material.slot_texture(value as usize)?;
            Some(Term {
                texture: Some((sampler.to_owned(), texture.to_owned(), picked?)),
                second: None,
                color: [1.0; 3],
            })
        }
        100..=103 => color(format!("const_color{}", value - 100)),
        104..=111 => {
            let v = *param(material, &format!("const_value{}", value - 104))?.first()?;
            Some(constant([v; 3]))
        }
        // SI-EMI-01: inputs 112–115 read as const_color4–7.
        112..=115 => color(format!("const_color{}", value - 108)),
        200..=209 if depth < 4 => computed(material, value as u32 - 200, depth + 1, exposure),
        300 => Some(constant([1.0; 3])),
        507 if *exposure == DynamicExposure::None => {
            *exposure = match channel_option {
                0 => DynamicExposure::Exposure,
                1 => DynamicExposure::OneMinus,
                _ => return None,
            };
            Some(constant([1.0; 3]))
        }
        _ => None,
    }
}

/// The result of `uking_colorN_*`.
fn computed(
    material: &Material,
    n: u32,
    depth: u32,
    exposure: &mut DynamicExposure,
) -> Option<Term> {
    if option(material, &format!("uking_enable_calc_color{n}")) != Some(1) {
        return None;
    }
    let mut part = |p: &str| -> Option<Term> {
        let value = option(material, &format!("uking_color{n}_{p}"))?;
        let channel = option(material, &format!("uking_color{n}_{p}_channel")).unwrap_or(0);
        input(material, value, channel, depth, exposure)
    };
    let parts: &[&str] = match option(material, &format!("uking_color{n}_calc_type"))? {
        0 => &["A"],
        2 => &["A", "B"],
        9 => &["A", "B", "C"],
        11 => &["A", "B", "C", "D"],
        _ => return None,
    };
    let mut term = constant([1.0; 3]);
    for p in parts {
        term = times(term, part(p)?)?;
    }
    if option(material, &format!("uking_color{n}_clamp01")) == Some(1) {
        // Exact where the texture is white.
        term.color = term.color.map(|c| c.clamp(0.0, 1.0));
    }
    Some(term)
}

/// The product of two terms, if it takes at most two texture channels.
fn times(a: Term, b: Term) -> Option<Term> {
    let mut textures = [a.texture, a.second, b.texture, b.second]
        .into_iter()
        .flatten();
    let (texture, second) = (textures.next(), textures.next());
    if textures.next().is_some() {
        return None;
    }
    Some(Term {
        texture,
        second,
        color: std::array::from_fn(|i| a.color[i] * b.color[i]),
    })
}
