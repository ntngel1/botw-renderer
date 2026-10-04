//! The game's effect links (ELink, xlink2: `ELink2/ELink2DB.sbelnk`) for
//! the users the renderer plays: which emitter sets a user plays, when
//! (its property triggers, always triggers and keys) and how (the asset's
//! placement and live parameters). Written by `bake` (step `elink`), read
//! by the renderer's ELink evaluator (docs/research/elink.md).
//!
//! ```text
//! effects/elink.ron   ElinkDb
//! ```
//!
//! A user's call table is kept as the game has it: entry `i` is an asset
//! (an emitter set with parameters) or a container (switch, blend,
//! random, sequence) whose children are entries `children`; a child
//! carries the condition its parent selects it by. Triggers name the
//! entry they start.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// `effects/elink.ron`.
pub const ELINK: &str = "effects/elink.ron";

/// The ELink subset the renderer plays.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ElinkDb {
    /// Users by name (`Camera`, `Chemical`, the weather locators …).
    pub users: BTreeMap<String, User>,
    /// Effect-only actors on the map whose user plays a container by its
    /// key `Always` (the distant weather: `Rain_Distance` …), placed.
    pub placed: Vec<PlacedUser>,
    /// Actors with models in the baked cells whose user plays a container
    /// by `Always`: actor name → user.
    pub actor_users: BTreeMap<String, String>,
}

/// A placed actor and the user it plays.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlacedUser {
    pub actor: String,
    pub user: String,
    pub hash_id: u32,
    pub translate: [f32; 3],
    /// Radians, applied X, Y, Z (as `objects::PlacedActor`).
    pub rotate: [f32; 3],
    pub scale: [f32; 3],
}

/// An ELink user (`ResUserHeader` and what hangs off it).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct User {
    /// Local properties by name (set by the user's owner).
    pub local_properties: Vec<String>,
    /// The asset call table.
    pub calls: Vec<Call>,
    /// Property triggers (`ResPropertyTrigger`), in table order.
    pub property_triggers: Vec<PropertyTrigger>,
    /// Always triggers (`ResAlwaysTrigger`): entries played all the time.
    pub always_triggers: Vec<usize>,
}

impl User {
    /// The top-level entry with key `key`.
    pub fn key(&self, key: &str) -> Option<usize> {
        self.calls
            .iter()
            .position(|c| c.parent.is_none() && c.key == key)
    }
}

/// An entry of the asset call table (`ResAssetCallTable`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub key: String,
    /// The containing entry.
    pub parent: Option<usize>,
    /// How the parent selects this entry (switch and random containers).
    pub condition: Option<Condition>,
    /// The raw `duration` field (`+0x08`).
    pub duration: i32,
    pub kind: CallKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CallKind {
    Asset(Box<Asset>),
    Container(Container),
    /// A container without parameters (`paramStartPos` −1): plays nothing.
    Empty,
}

/// `ResContainerParam` (+ `ResSwitchContainerParam`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Container {
    pub kind: ContainerKind,
    /// Child entries, `childrenStartIndex..=childrenEndIndex`.
    pub children: Vec<usize>,
    /// The property a switch watches.
    pub watch: Option<PropertyRef>,
}

/// `xlink2::ContainerType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerKind {
    /// The first child whose condition holds (no condition: always).
    Switch,
    /// One child by weight, drawn when it starts.
    Random,
    Random2,
    /// Every child.
    Blend,
    /// The children one after the other.
    Sequence,
}

/// A property read by a switch, a trigger or a curve.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropertyRef {
    pub name: String,
    /// A global property (the system's) or one of the user's.
    pub global: bool,
}

/// `ResSwitchCondition` / `ResRandomCondition`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    /// `property <compare> value`.
    Switch {
        compare: Compare,
        value: Value,
    },
    Random {
        weight: f32,
    },
}

/// A condition's value, by the property's type in the resource.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    /// An enum property's entry, by name (resolved to its index at run
    /// time; a name the property lacks is −1).
    Enum(String),
    Int(i32),
    Float(f32),
}

/// The compare types as the resource numbers them (0…5); the decomp's
/// `xlink2Types.h` lists them reversed (Wii U `0x03b9a954`, `0x03b9a8b8`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Compare {
    Equal,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
    NotEqual,
}

impl Compare {
    pub fn from_raw(raw: u32) -> Option<Self> {
        Some(match raw {
            0 => Self::Equal,
            1 => Self::Greater,
            2 => Self::GreaterOrEqual,
            3 => Self::Less,
            4 => Self::LessOrEqual,
            5 => Self::NotEqual,
            _ => return None,
        })
    }

    /// `property <self> value`.
    pub fn holds<T: PartialOrd>(self, property: T, value: T) -> bool {
        match self {
            Self::Equal => property == value,
            Self::Greater => property > value,
            Self::GreaterOrEqual => property >= value,
            Self::Less => property < value,
            Self::LessOrEqual => property <= value,
            Self::NotEqual => property != value,
        }
    }
}

/// `ResPropertyTrigger` with its `ResProperty`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertyTrigger {
    pub property: PropertyRef,
    /// The entry it plays.
    pub call: usize,
    /// When it plays (no condition: never, as the game's controller).
    pub condition: Option<Condition>,
}

/// An asset: an emitter set and its parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    /// `RuntimeAssetName`.
    pub set: String,
    /// The effect file holding the set (`EffectIndex::files`): the user's
    /// own, else the resident one; `None` when no baked file has it (a
    /// blank such as `@Blank`).
    pub file: Option<String>,
    /// `Matrix` (0…6, `XLINK_AssetExecutorELinkCalcMtx` `0x03b824dc`).
    pub matrix: i32,
    /// `RotateSource`.
    pub rotate_source: i32,
    pub params: AssetParams,
}

/// The asset's numeric parameters; defaults are the parameter table's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AssetParams {
    pub scale: Param,
    pub position: [Param; 3],
    /// Radians, applied X, Y, Z.
    pub rotation: [Param; 3],
    /// Red, green, blue, alpha.
    pub color: [Param; 4],
    pub emission_rate: Param,
    pub emission_scale: Param,
    pub emission_interval: Param,
    pub life_scale: Param,
    pub directional_velocity: Param,
    /// Frames the asset plays for (`Duration`), if set.
    pub duration: Option<Param>,
    /// Frames before it starts (`Delay`), if set.
    pub delay: Option<Param>,
}

/// A parameter value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Param {
    Const(f32),
    /// Drawn once when the asset starts.
    Random {
        min: f32,
        max: f32,
        spread: Spread,
    },
    /// Follows a property, every frame.
    Curve(Curve),
}

impl Param {
    pub fn constant(&self) -> Option<f32> {
        match self {
            Param::Const(v) => Some(*v),
            _ => None,
        }
    }
}

/// `ResCurveCallTable` and its points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub property: PropertyRef,
    /// `curveType` (0 linear, clamped at the ends).
    pub kind: u16,
    /// `(property, value)` points, as stored.
    pub points: Vec<[f32; 2]>,
}

impl Curve {
    /// The value at `x`: linear between the points, clamped to the end
    /// points (curve type 0; docs/research/weather.md §2.3).
    pub fn eval(&self, x: f32) -> f32 {
        let points = &self.points;
        let (Some(first), Some(last)) = (points.first(), points.last()) else {
            return 0.0;
        };
        if x <= first[0] {
            return first[1];
        }
        if x >= last[0] {
            return last[1];
        }
        for pair in points.windows(2) {
            let ([x0, y0], [x1, y1]) = (pair[0], pair[1]);
            if x <= x1 {
                if x1 <= x0 {
                    return y1;
                }
                return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
            }
        }
        last[1]
    }
}

/// How a random parameter draws its value (`ValueReferenceType` 3, 6…17).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Spread {
    Uniform,
    /// `Random{2,3,4,1Point5}Pow`.
    Centered(f32),
    /// `Random*PowWeightMin`.
    WeightMin(f32),
    /// `Random*PowWeightMax`.
    WeightMax(f32),
}

impl Spread {
    /// The value for a uniform `r` in `[0, 1)` (Wii U `0x03b986c4`,
    /// `0x03b98744`, `0x03b98838`, `0x03b988d8`).
    pub fn sample(self, min: f32, max: f32, r: f32) -> f32 {
        match self {
            Spread::Uniform => min + (max - min) * r,
            Spread::Centered(power) => {
                let half = (max - min).abs() / 2.0;
                let signed = 2.0 * r - 1.0;
                let offset = signed.abs().powf(power) * half;
                if signed < 0.0 {
                    min + half - offset
                } else {
                    min + half + offset
                }
            }
            Spread::WeightMin(power) => min + r.powf(power) * (max - min).abs(),
            Spread::WeightMax(power) => min + (1.0 - r.powf(power)) * (max - min).abs(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_types_follow_the_resource_order() {
        let order = [
            (0, Compare::Equal),
            (1, Compare::Greater),
            (2, Compare::GreaterOrEqual),
            (3, Compare::Less),
            (4, Compare::LessOrEqual),
            (5, Compare::NotEqual),
        ];
        for (raw, compare) in order {
            assert_eq!(Compare::from_raw(raw), Some(compare));
        }
        assert_eq!(Compare::from_raw(6), None);
        // `気温 ≤ −2` picks snow, `濃度:雨 > 0` rain.
        assert!(Compare::LessOrEqual.holds(-2.0, -2.0));
        assert!(!Compare::Greater.holds(0.0, 0.0));
        assert!(Compare::Greater.holds(0.1, 0.0));
        assert!(Compare::GreaterOrEqual.holds(2.0, 2.0));
        assert!(Compare::Less.holds(0.4, 0.5));
        assert!(Compare::NotEqual.holds(1, 2));
    }

    #[test]
    fn curves_are_linear_and_clamped() {
        let curve = Curve {
            property: PropertyRef {
                name: "濃度:雨".into(),
                global: true,
            },
            kind: 0,
            points: vec![[0.0, 0.0], [1.0, 0.7]],
        };
        assert_eq!(curve.eval(-1.0), 0.0);
        assert!((curve.eval(0.5) - 0.35).abs() < 1e-6);
        // Heavy rain (2) reads the value at 1.
        assert_eq!(curve.eval(2.0), 0.7);
        let steps = Curve {
            points: vec![[4.999, 0.005], [5.0, 1.0], [5.0, 2.0]],
            ..curve
        };
        assert_eq!(steps.eval(4.0), 0.005);
        assert_eq!(steps.eval(6.0), 2.0);
        assert!((steps.eval(4.9995) - 0.5025).abs() < 1e-3);
    }
}
