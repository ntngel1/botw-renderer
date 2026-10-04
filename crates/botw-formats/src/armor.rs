//! Link's armour: each piece is an actor (`Armor_001_Head`, `Armor_001_Upper`,
//! `Armor_001_Lower`...; `Armor_Default_*` is what he wears with nothing on:
//! his hair, a belt, shorts). Its pack names the model (`bmodellist`) and its
//! general parameters (`resGParamListObjectArmor*.h` in the decompilation)
//! say what it does: `ArmorEffect` (`EffectType`, e.g. `ClimbSpeed`,
//! `SwimSpeed`, `ResistCold`, and its `EffectLevel`), `SeriesArmor` (the
//! set it belongs to and whether a full set gives a bonus), `ArmorHead`
//! (how the ears bend under it, `EarRotate` in degrees) and `ArmorUpper`
//! (`IsDispOffPorch`: the Sheikah Slate pouch is not shown with it).

use crate::Result;
use crate::actor::{ActorPacks, ModelRef};
use crate::params::GeneralParams;

/// Where a piece is worn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Head,
    Upper,
    Lower,
}

impl Slot {
    /// The slot an armour actor's name ends in.
    // SI-CMB-04: armour slot by name suffix; set bonus ignored.
    pub fn of(actor: &str) -> Option<Self> {
        let body = actor.trim_end_matches("_B");
        if body.ends_with("_Head") {
            Some(Self::Head)
        } else if body.ends_with("_Upper") {
            Some(Self::Upper)
        } else if body.ends_with("_Lower") {
            Some(Self::Lower)
        } else {
            None
        }
    }
}

/// One piece of armour.
#[derive(Clone, Debug, PartialEq)]
pub struct Armor {
    pub actor: String,
    pub slot: Slot,
    pub models: Vec<ModelRef>,
    /// The set it belongs to (`Hylia`, `Climb`, `Zora`...).
    pub series: String,
    /// What it does (`None`, `ClimbSpeed`, `SwimSpeed`...) and how strongly.
    pub effect: String,
    pub effect_level: i32,
    /// Whether a full set of such pieces gives the set bonus.
    pub set_bonus: bool,
    /// How the ears turn under a head piece, degrees about X, Y and Z.
    pub ear_rotate: [f32; 3],
    /// Whether the Sheikah Slate pouch is hidden under an upper piece.
    pub hides_pouch: bool,
}

impl Armor {
    /// Reads armour actor `actor` (`Ok(None)` if the dump has no such actor).
    pub fn read(packs: &ActorPacks, actor: &str) -> Result<Option<Self>> {
        let Some(slot) = Slot::of(actor) else {
            return Ok(None);
        };
        let Some(pack) = packs.open(actor)? else {
            return Ok(None);
        };
        let params = match pack.find("Actor/GeneralParamList/") {
            Some(bytes) => Some(GeneralParams::parse(&bytes)?),
            None => None,
        };
        Ok(Some(Self::from_params(
            actor,
            slot,
            packs.models(actor)?,
            params.as_ref(),
        )))
    }

    fn from_params(
        actor: &str,
        slot: Slot,
        models: Vec<ModelRef>,
        params: Option<&GeneralParams>,
    ) -> Self {
        let text = |object: &str, name: &str| {
            params
                .and_then(|p| p.text(object, name))
                .unwrap_or_default()
                .to_owned()
        };
        Self {
            actor: actor.to_owned(),
            slot,
            models,
            series: text("SeriesArmor", "SeriesType"),
            effect: text("ArmorEffect", "EffectType"),
            effect_level: params
                .and_then(|p| p.number("ArmorEffect", "EffectLevel"))
                .unwrap_or(0.0) as i32,
            set_bonus: params
                .and_then(|p| p.flag("SeriesArmor", "EnableCompBonus"))
                .unwrap_or(false),
            ear_rotate: params
                .and_then(|p| p.vec3("ArmorHead", "EarRotate"))
                .unwrap_or_default(),
            hides_pouch: params
                .and_then(|p| p.flag("ArmorUpper", "IsDispOffPorch"))
                .unwrap_or(false),
        }
    }

    /// Whether this is the nothing-worn piece of its slot.
    pub fn is_default(&self) -> bool {
        self.actor.starts_with("Armor_Default")
    }
}

// SI-CMB-04: armour slot by name suffix; set bonus ignored.
/// The level of effect `effect` all of `pieces` add up to.
pub fn effect_level<'a>(pieces: impl IntoIterator<Item = &'a Armor>, effect: &str) -> i32 {
    pieces
        .into_iter()
        .filter(|a| a.effect == effect)
        .map(|a| a.effect_level)
        .sum()
}

#[cfg(test)]
mod tests {
    use roead::aamp::{Parameter, ParameterIO, ParameterList, ParameterObject};

    use super::*;

    #[test]
    fn slots_follow_the_actor_name() {
        assert_eq!(Slot::of("Armor_001_Head"), Some(Slot::Head));
        assert_eq!(Slot::of("Armor_001_Head_B"), Some(Slot::Head));
        assert_eq!(Slot::of("Armor_Default_Upper"), Some(Slot::Upper));
        assert_eq!(Slot::of("Armor_014_Lower"), Some(Slot::Lower));
        assert_eq!(Slot::of("Weapon_Sword_001"), None);
    }

    #[test]
    fn reads_what_a_piece_does() {
        let mut effect = ParameterObject::new();
        effect.insert("EffectType", Parameter::StringRef("ClimbSpeed".into()));
        effect.insert("EffectLevel", Parameter::I32(1));
        let mut series = ParameterObject::new();
        series.insert("SeriesType", Parameter::StringRef("Climb".into()));
        series.insert("EnableCompBonus", Parameter::Bool(false));
        let mut io = ParameterIO::new();
        io.param_root = ParameterList::new()
            .with_object("ArmorEffect", effect)
            .with_object("SeriesArmor", series);
        let params = GeneralParams::parse(&io.to_binary()).unwrap();
        let armor = Armor::from_params("Armor_014_Upper", Slot::Upper, Vec::new(), Some(&params));
        assert_eq!(
            (
                armor.series.as_str(),
                armor.effect.as_str(),
                armor.effect_level
            ),
            ("Climb", "ClimbSpeed", 1)
        );
        assert!(!armor.set_bonus && !armor.hides_pouch && !armor.is_default());
        let lower = Armor {
            actor: "Armor_014_Lower".into(),
            slot: Slot::Lower,
            ..armor.clone()
        };
        let hood = Armor {
            effect: "None".into(),
            ..armor.clone()
        };
        assert_eq!(effect_level([&armor, &lower, &hood], "ClimbSpeed"), 2);
    }
}
