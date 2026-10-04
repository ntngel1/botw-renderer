//! UMii: the villagers the game assembles from `UMii_*` models by the
//! parameters in their actor's `.bumii` (see docs/research/umii.md).
//!
//! [`Umii`] holds the parameters (an AAMP document, `ksys::mii::UMii`);
//! [`Umii::parts`] names the models the game builds a Hylian from
//! (`U-King.rpx` v208 `FUN_03444294`), [`Umii::colour_frames`] the frames of
//! the colour animations it plays on them (`FUN_03433360`, `FUN_03433a94`).

use std::collections::BTreeMap;

use botw_formats::params::GeneralParams;
use roead::byml::Byml;

/// `body.race`.
pub const HYLIAN: i32 = 0;

/// `hair.type` of a bald head (the game draws no hair model).
const BALD: i32 = 30;

/// A villager's parameters (`.bumii`), with the game's defaults
/// (`UMii::UMii`) for what the file leaves out.
#[derive(Clone, Debug, PartialEq)]
pub struct Umii {
    /// `ffsd.type`: 1 builds the villager from another actor's UMii (a Mii
    /// made by the player), not supported.
    pub ffsd_type: i32,
    pub race: i32,
    /// `body.type`: C, N, T, S, SK.
    pub body_type: i32,
    pub number: i32,
    pub weight: i32,
    pub height: i32,
    /// Boy, Man, OldMan, Girl, Woman, OldWoman.
    pub sex_age: i32,
    pub fav_color: i32,
    pub sub_color_1: i32,
    pub sub_color_2: i32,
    pub head_fav_color: i32,
    pub shoulder_fav_color: i32,
    pub shoulder_sub_color_1: i32,
    pub personality: String,
    pub backpack: i32,
    pub hat: i32,
    pub jaw: i32,
    pub wrinkle: i32,
    pub make: i32,
    pub skin_color: i32,
    pub hair_type: i32,
    pub hair_color: i32,
    pub hair_flip: bool,
    pub eye: Feature,
    pub eyebrow: Feature,
    pub nose: Feature,
    pub mouth: Feature,
    pub mustache: i32,
    pub mustache_scale: f32,
    pub beard_type: i32,
    pub beard_color: i32,
    pub glass_type: i32,
    pub glass_color: i32,
}

/// A face feature's parameters (`eye`, `eyebrow`, `nose`, `mouth`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feature {
    pub kind: i32,
    pub color: i32,
    pub trans_u: f32,
    pub trans_v: f32,
    pub rotate: f32,
    pub scale: f32,
    pub aspect: f32,
}

/// The models of a villager, in the order the game makes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    Body,
    Face,
    Nose,
    Hair,
    Glass,
    Mustache,
    Beard,
    BackPack,
    Hat,
}

/// `Mii/UMiiConstructionInfo.byml` (`Pack/Bootup.pack`): models some
/// parameter values use instead of their own (`Hair`: hair type → hair
/// model number; `Mouth` likewise for faces, absent in v208).
#[derive(Clone, Debug, Default)]
pub struct ConstructionInfo {
    pub hair: BTreeMap<String, String>,
    pub mouth: BTreeMap<String, String>,
    pub body_angles: BodyAngles,
}

impl ConstructionInfo {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let doc = Byml::from_binary(bytes).map_err(|e| format!("UMiiConstructionInfo: {e}"))?;
        let map = |key: &str| -> BTreeMap<String, String> {
            doc.as_map()
                .ok()
                .and_then(|m| m.get(key))
                .and_then(|v| v.as_map().ok())
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, v)| Some((k.to_string(), v.as_string().ok()?.to_string())))
                        .collect()
                })
                .unwrap_or_default()
        };
        Ok(Self {
            hair: map("Hair"),
            mouth: map("Mouth"),
            body_angles: BodyAngles::default(),
        })
    }
}

/// Hylian angular proportion tables from Bootup.pack/Mii/umii.bnetfp.
/// Native 0x03441d14 selects six height/weight frames; each entry contains
/// clavicle, arm, leg and crotch angles in degrees, before the runtime factor.
#[derive(Clone, Debug, Default)]
pub struct BodyAngles(pub [[[f32; 4]; 6]; 4]);

impl BodyAngles {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let params = GeneralParams::parse(bytes).map_err(|e| e.to_string())?;
        let mut table = Self::default();
        for (class, name) in ["child", "man", "woman", "old"].iter().enumerate() {
            for (frame, suffix) in [
                "slim",
                "normal",
                "fat",
                "tall_slim",
                "tall_normal",
                "tall_fat",
            ]
            .iter()
            .enumerate()
            {
                for (bone, part) in ["clavicle", "arm", "leg", "crotch"].iter().enumerate() {
                    // The native constructor initializes absent values to zero.
                    let value = params
                        .number("Hylia", &format!("{name}_{part}_{suffix}"))
                        .unwrap_or(0.0);
                    if !value.is_finite() {
                        return Err(format!(
                            "non-finite Hylian angular parameter {name}/{part}/{suffix}"
                        ));
                    }
                    table.0[class][frame][bone] = value;
                }
            }
        }
        Ok(table)
    }

    /// Normal Hylian class selector 0x0342fc14 and table selector 0x03441d14.
    pub fn at(&self, umii: &Umii) -> [f32; 4] {
        let class = match umii.age_class() {
            0 => 0,
            2 => 3,
            _ => 1 + usize::from(umii.is_female()),
        };
        usize::try_from(umii.height_weight())
            .ok()
            .and_then(|frame| self.0[class].get(frame))
            .copied()
            .unwrap_or([0.0; 4])
    }
}

/// Body type letters (`0x10554df8`).
const BODY_TYPES: [&str; 5] = ["C", "N", "T", "S", "SK"];
/// Sex and age letters (`0x10554c20`): boy, man, old man, girl, woman, old
/// woman.
const SEX_AGE: [&str; 6] = ["B", "M", "X", "G", "W", "Y"];
/// Face letters by age class (`0x10554e78`).
const FACES: [&str; 3] = ["B", "M", "W"];

impl Umii {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let doc = GeneralParams::parse(bytes).map_err(|e| format!("bumii: {e}"))?;
        let int = |o: &str, n: &str, default: i32| doc.number(o, n).map_or(default, |v| v as i32);
        let float = |o: &str, n: &str, default: f32| doc.number(o, n).unwrap_or(default);
        let feature = |o: &str, defaults: [f32; 7]| Feature {
            kind: int(o, "type", defaults[0] as i32),
            color: int(o, "color", defaults[1] as i32),
            trans_u: float(o, "trans_u", defaults[2]),
            trans_v: float(o, "trans_v", defaults[3]),
            rotate: float(o, "rotate", defaults[4]),
            scale: float(o, "scale", defaults[5]),
            aspect: float(o, "aspect", defaults[6]),
        };
        Ok(Self {
            ffsd_type: int("ffsd", "type", 0),
            race: int("body", "race", 0),
            body_type: int("body", "type", 0),
            number: int("body", "number", 0),
            weight: int("body", "weight", 1),
            height: int("body", "height", 0),
            sex_age: int("personal", "sex_age", 1),
            fav_color: int("personal", "fav_color", 0),
            sub_color_1: int("personal", "sub_color_1", -1),
            sub_color_2: int("personal", "sub_color_2", -1),
            head_fav_color: int("personal", "head_fav_color", -1),
            shoulder_fav_color: int("personal", "shoulder_fav_color", -1),
            shoulder_sub_color_1: int("personal", "shoulder_sub_color_1", -1),
            personality: doc
                .text("personal", "personality")
                .unwrap_or_default()
                .to_owned(),
            backpack: int("common", "backpack", -1),
            hat: int("common", "hat", -1),
            jaw: int("shape", "jaw", 0),
            wrinkle: int("shape", "wrinkle", 0),
            make: int("shape", "make", 0),
            skin_color: int("shape", "skin_color", 0),
            hair_type: int("hair", "type", 0),
            hair_color: int("hair", "color", 0),
            hair_flip: doc.flag("hair", "flip").unwrap_or(false),
            eye: feature("eye", [0.0, 0.0, 2.0, 12.0, 4.0, 4.0, 3.0]),
            eyebrow: feature("eyebrow", [0.0, 1.0, 2.0, 7.0, 0.0, 4.0, 3.0]),
            nose: feature("nose", [1.0, 0.0, 0.0, 9.0, 0.0, 4.0, 0.0]),
            mouth: feature("mouth", [3.0, 0.0, 0.0, 13.0, 0.0, 4.0, 3.0]),
            mustache: int("beard", "mustache", 0),
            mustache_scale: float("beard", "scale", 0.0),
            beard_type: int("beard", "type", 0),
            beard_color: int("beard", "color", 0),
            glass_type: int("glass", "type", 0),
            glass_color: int("glass", "color", 0),
        })
    }

    /// Female (`UMii::isFemale`).
    pub fn is_female(&self) -> bool {
        self.sex_age > 2
    }

    /// 0 child, 1 adult, 2 old (`FUN_0342fbb4`).
    pub fn age_class(&self) -> usize {
        match self.sex_age {
            0 | 3 => 0,
            1 | 4 => 1,
            _ => 2,
        }
    }

    /// Index into the face letters (`FUN_0342fcc4`): child B, man M,
    /// woman W, and the old W too.
    pub fn face_class(&self) -> usize {
        match self.age_class() {
            0 => 0,
            2 => 2,
            _ => 1 + usize::from(self.is_female()),
        }
    }

    /// The body animation file: the men's or the women's.
    pub fn body_animations(&self) -> &'static str {
        if self.is_female() {
            "UMii_Common_Body_W_Animation"
        } else {
            "UMii_Common_Body_M_Animation"
        }
    }

    /// `3 * height + weight`, the frame of the `*_HeightWeight` animations
    /// (`UMii::getHeightWeightIndex`).
    pub fn height_weight(&self) -> i32 {
        3 * self.height + self.weight
    }

    fn sex_letter(&self) -> &'static str {
        SEX_AGE.get(self.sex_age as usize).copied().unwrap_or("M")
    }

    fn body_letter(&self) -> &'static str {
        BODY_TYPES
            .get(self.body_type as usize)
            .copied()
            .unwrap_or("C")
    }

    /// The models of a Hylian villager (`FUN_03444294` over parts 0–8),
    /// `None` for the other races.
    pub fn parts(&self, info: &ConstructionInfo) -> Option<Vec<(Part, String)>> {
        if self.race != HYLIAN {
            return None;
        }
        let mut parts = Vec::new();
        // Body: type N takes its letter from the age class.
        let body = if self.body_type == 1 {
            let letter = match self.age_class() {
                0 => "B",
                1 => self.sex_letter(),
                _ => "X",
            };
            format!("UMii_Hylia_BodyN_{letter}_{:03}", self.number)
        } else {
            format!(
                "UMii_Hylia_Body{}_{}_{:03}",
                self.body_letter(),
                self.sex_letter(),
                self.number
            )
        };
        parts.push((Part::Body, body));
        let face_letter = FACES[self.face_class()];
        let mouth = format!("{:03}", self.mouth.kind);
        let face = match info.mouth.get(&mouth) {
            Some(other) => format!("UMii_Hylia_Face_{face_letter}_{other}"),
            None => format!("UMii_Hylia_Face_{face_letter}_{mouth}"),
        };
        parts.push((Part::Face, face));
        parts.push((
            Part::Nose,
            format!("UMii_Hylia_Nose_C_{:03}", self.nose.kind),
        ));
        // Hair: under no hat (or the Hylian everyday hat 0 of bodies C, T
        // and S), unless bald.
        let hatless = self.hat < 0
            || (self.race == HYLIAN && self.body_type != 1 && self.body_type != 4 && self.hat == 0);
        if hatless && self.hair_type != BALD {
            let hair = if self.age_class() == 0 {
                let kind = if self.hair_type > 6 {
                    0
                } else {
                    self.hair_type
                };
                format!("UMii_Hylia_Hair_B_{kind:03}")
            } else {
                let kind = format!("{:03}", self.hair_type);
                match info.hair.get(&kind) {
                    Some(other) => format!("UMii_Hylia_Hair_C_{other}"),
                    None => format!("UMii_Hylia_Hair_C_{kind}"),
                }
            };
            parts.push((Part::Hair, hair));
        }
        if self.glass_type != 0 {
            parts.push((
                Part::Glass,
                format!("UMii_Hylia_Glass_C_{:03}", self.glass_type),
            ));
        }
        let grown_man = !self.is_female() && self.age_class() != 0;
        if self.mustache != 0 && grown_man {
            parts.push((
                Part::Mustache,
                format!("UMii_Hylia_Mustache_C_{:03}", self.mustache),
            ));
        }
        if (1..=3).contains(&self.beard_type) && grown_man {
            parts.push((
                Part::Beard,
                format!("UMii_Hylia_Beard_C_{:03}", self.beard_type),
            ));
        }
        if self.backpack == 0 {
            parts.push((Part::BackPack, "UMii_Hylia_BackPack_C_000".into()));
        }
        if self.hat >= 0 && self.hat != 4 {
            let hat = match self.body_type {
                1 | 4 => format!(
                    "UMii_Hylia_Hat{}_{}_{:03}",
                    self.body_letter(),
                    self.sex_letter(),
                    self.hat
                ),
                _ => format!("UMii_Hylia_HatC_C_{:03}", self.hat),
            };
            parts.push((Part::Hat, hat));
        }
        Some(parts)
    }

    /// The frame of each colour animation the game plays on a Hylian
    /// (`FUN_03433360`, `FUN_03433764` for the body and the backpack,
    /// `FUN_03433a94`): `(animation name, frame)`. `body` and `backpack`
    /// are the models' names (their own `<model>_Favorite_Color` etc.).
    pub fn colour_frames(&self, body: &str, backpack: Option<&str>) -> Vec<(String, i32)> {
        let or = |value: i32, fallback: i32| if value == -1 { fallback } else { value };
        let mut frames = vec![
            (
                "UMii_Hylia_Body_Face_Nose_Skin_Color".to_owned(),
                self.skin_color,
            ),
            ("Lip_Color".to_owned(), self.mouth.color),
            ("Eyeball_Color".to_owned(), self.eye.color),
            ("Eyeball_Size".to_owned(), self.eye.kind),
            ("Brow_Color".to_owned(), self.eyebrow.color),
        ];
        let hair_or_hat = self.hair_type != BALD || self.hat >= 0;
        if hair_or_hat {
            frames.push(("UMii_Hylia_Hair_Color".into(), self.hair_color));
        }
        if self.glass_type != 0 {
            frames.push(("UMii_Hylia_Glass_Color".into(), self.glass_color));
        }
        if self.mustache != 0 || self.beard_type != 0 {
            frames.push(("UMii_Hylia_Mustache_Beard_Color".into(), self.beard_color));
            if self.beard_type > 3 {
                frames.push(("MakeUpBeard_Color".into(), self.beard_color));
            }
        }
        let fav = self.fav_color;
        frames.push((format!("{body}_Favorite_Color"), fav));
        frames.push((format!("{body}_Sub_Color_1"), or(self.sub_color_1, fav)));
        frames.push((format!("{body}_Sub_Color_2"), or(self.sub_color_2, fav)));
        if let Some(backpack) = backpack {
            let shoulder = or(self.shoulder_fav_color, fav);
            frames.push((format!("{backpack}_Favorite_Color"), shoulder));
            frames.push((
                format!("{backpack}_Sub_Color_1"),
                or(self.shoulder_sub_color_1, shoulder),
            ));
        }
        if hair_or_hat {
            frames.push((
                "UMii_Hylia_Hair_Favorite_Color".into(),
                or(self.head_fav_color, fav),
            ));
        }
        frames
    }

    /// The idle the villager's wait sequence (`UH_M_Wait`, `UH_W_Wait`)
    /// loops when nothing else happens: by personality, `*_Active` →
    /// `Positive_Wait`, `*_Deflated` → `Negative_Wait`, else
    /// `Default_Wait`.
    pub fn wait_clip(&self) -> &'static str {
        if self.personality.ends_with("_Active") {
            "Positive_Wait"
        } else if self.personality.ends_with("_Deflated") {
            "Negative_Wait"
        } else {
            "Default_Wait"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_body_angle_selector_separates_age_sex_and_height_weight() {
        let mut table = BodyAngles::default();
        for class in 0..4 {
            for frame in 0..6 {
                table.0[class][frame] = [10.0 * class as f32 + frame as f32; 4];
            }
        }
        let mut umii = villager();
        umii.height = 1;
        umii.weight = 2;
        for (sex_age, class) in [(0, 0), (3, 0), (1, 1), (4, 2), (2, 3), (5, 3)] {
            umii.sex_age = sex_age;
            assert_eq!(table.at(&umii), [10.0 * class as f32 + 5.0; 4]);
        }
        // Native getter returns zero outside its six table branches.
        umii.height = 2;
        assert_eq!(table.at(&umii), [0.0; 4]);
        umii.height = -1;
        assert_eq!(table.at(&umii), [0.0; 4]);
    }

    fn villager() -> Umii {
        Umii {
            ffsd_type: 0,
            race: 0,
            body_type: 0,
            number: 0,
            weight: 1,
            height: 0,
            sex_age: 1,
            fav_color: 0,
            sub_color_1: -1,
            sub_color_2: -1,
            head_fav_color: -1,
            shoulder_fav_color: -1,
            shoulder_sub_color_1: -1,
            personality: "Man_Normal".into(),
            backpack: -1,
            hat: -1,
            jaw: 6,
            wrinkle: 2,
            make: 0,
            skin_color: 0,
            hair_type: 88,
            hair_color: 1,
            hair_flip: false,
            eye: Feature {
                kind: 2,
                color: 0,
                trans_u: 2.0,
                trans_v: 12.0,
                rotate: 4.0,
                scale: 4.0,
                aspect: 3.0,
            },
            eyebrow: Feature {
                kind: 6,
                color: 0,
                trans_u: 2.0,
                trans_v: 7.0,
                rotate: 0.0,
                scale: 4.0,
                aspect: 3.0,
            },
            nose: Feature {
                kind: 1,
                color: 0,
                trans_u: 0.0,
                trans_v: 9.0,
                rotate: 0.0,
                scale: 4.0,
                aspect: 0.0,
            },
            mouth: Feature {
                kind: 6,
                color: 0,
                trans_u: 0.0,
                trans_v: 13.0,
                rotate: 0.0,
                scale: 4.0,
                aspect: 3.0,
            },
            mustache: 2,
            mustache_scale: 3.264,
            beard_type: 0,
            beard_color: 1,
            glass_type: 0,
            glass_color: 0,
        }
    }

    #[test]
    fn names_a_hylian_mans_models_like_the_game() {
        let mut info = ConstructionInfo::default();
        info.hair.insert("001".into(), "029".into());
        let parts = villager().parts(&info).unwrap();
        let names: Vec<&str> = parts.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "UMii_Hylia_BodyC_M_000",
                "UMii_Hylia_Face_M_006",
                "UMii_Hylia_Nose_C_001",
                "UMii_Hylia_Hair_C_088",
                "UMii_Hylia_Mustache_C_002",
            ]
        );
        let mut remapped = villager();
        remapped.hair_type = 1;
        assert!(
            remapped
                .parts(&info)
                .unwrap()
                .contains(&(Part::Hair, "UMii_Hylia_Hair_C_029".into()))
        );
    }

    #[test]
    fn children_and_the_old_take_their_own_models() {
        let info = ConstructionInfo::default();
        let mut girl = villager();
        girl.sex_age = 3;
        girl.hair_type = 9;
        let parts = girl.parts(&info).unwrap();
        assert_eq!(parts[0].1, "UMii_Hylia_BodyC_G_000");
        assert_eq!(parts[1].1, "UMii_Hylia_Face_B_006");
        assert!(parts.contains(&(Part::Hair, "UMii_Hylia_Hair_B_000".into())));
        // No moustache on a child.
        assert!(!parts.iter().any(|(p, _)| *p == Part::Mustache));
        let mut old = villager();
        old.sex_age = 2;
        old.body_type = 1;
        let parts = old.parts(&info).unwrap();
        assert_eq!(parts[0].1, "UMii_Hylia_BodyN_X_000");
        assert_eq!(parts[1].1, "UMii_Hylia_Face_W_006");
    }

    #[test]
    fn a_hat_hides_the_hair_but_the_everyday_one() {
        let info = ConstructionInfo::default();
        let mut hatted = villager();
        hatted.hat = 2;
        let parts = hatted.parts(&info).unwrap();
        assert!(!parts.iter().any(|(p, _)| *p == Part::Hair));
        assert!(parts.contains(&(Part::Hat, "UMii_Hylia_HatC_C_002".into())));
        hatted.hat = 0;
        assert!(
            hatted
                .parts(&info)
                .unwrap()
                .iter()
                .any(|(p, _)| *p == Part::Hair)
        );
    }

    #[test]
    fn unset_sub_colours_follow_the_favourite() {
        let mut v = villager();
        v.fav_color = 7;
        v.sub_color_2 = 3;
        let frames = v.colour_frames("UMii_Hylia_BodyC_M_000", None);
        let frame = |name: &str| frames.iter().find(|(n, _)| n == name).map(|(_, f)| *f);
        assert_eq!(frame("UMii_Hylia_BodyC_M_000_Sub_Color_1"), Some(7));
        assert_eq!(frame("UMii_Hylia_BodyC_M_000_Sub_Color_2"), Some(3));
        assert_eq!(frame("UMii_Hylia_Hair_Color"), Some(1));
        assert_eq!(frame("UMii_Hylia_Mustache_Beard_Color"), Some(1));
    }
}
