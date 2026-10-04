//! Actor parameter documents (AAMP) that tune gameplay: the general
//! parameter list (`Actor/GeneralParamList/*.bgparamlist`, objects of named
//! values such as `Player`), the AI program (`Actor/AIProgram/*.baiprog`,
//! AIs, actions, behaviours and queries, each with its class name and the
//! static parameters `SInst` its class reads) and the physics setup
//! (`Actor/Physics/*.bphysics`: the character controller's forms and the
//! shapes of the rigid bodies) and the damage parameters
//! (`Actor/DamageParam/*.bdmgparam`).
//!
//! Only values are read here; what they mean for movement is decided by the
//! code using them. Names are looked up by their CRC32, so any name the game
//! uses can be asked for without a name table.

use roead::aamp::{Parameter, ParameterIO, ParameterList, ParameterObject};

use crate::{FormatError, Result};

/// A parsed `.bgparamlist`.
pub struct GeneralParams(ParameterIO);

impl GeneralParams {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ParameterIO::from_binary(bytes)
            .map(Self)
            .map_err(|_| FormatError::Invalid("bgparamlist: not AAMP"))
    }

    /// A number in object `object` (e.g. `Player`, `ClimbEnableAngle`).
    pub fn number(&self, object: &str, name: &str) -> Option<f32> {
        number(self.0.param_root.objects.get(object)?.get(name)?)
    }

    /// A text value (e.g. `SeriesArmor`, `SeriesType`).
    pub fn text(&self, object: &str, name: &str) -> Option<&str> {
        self.0
            .param_root
            .objects
            .get(object)?
            .get(name)?
            .as_str()
            .ok()
    }

    /// A yes/no value.
    pub fn flag(&self, object: &str, name: &str) -> Option<bool> {
        self.0
            .param_root
            .objects
            .get(object)?
            .get(name)?
            .as_bool()
            .ok()
    }

    /// A three-component vector.
    pub fn vec3(&self, object: &str, name: &str) -> Option<[f32; 3]> {
        match self.0.param_root.objects.get(object)?.get(name)? {
            Parameter::Vec3(v) => Some([v.x, v.y, v.z]),
            _ => None,
        }
    }
}

/// A parsed `.bdmgparam`: how an actor takes damage.
#[derive(Clone, Debug)]
pub struct DamageParams(ParameterIO);

impl DamageParams {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        ParameterIO::from_binary(bytes)
            .map(Self)
            .map_err(|_| FormatError::Invalid("bdmgparam: not AAMP"))
    }

    /// A number of the `Parameters` object in list `damage_param` (e.g.
    /// `FallDamageStartHeight`).
    pub fn number(&self, name: &str) -> Option<f32> {
        let list = self.0.param_root.lists.get("damage_param")?;
        number(list.objects.get("Parameters")?.get(name)?)
    }
}

/// A float or integer parameter as `f32`.
fn number(parameter: &Parameter) -> Option<f32> {
    match parameter {
        Parameter::F32(v) => Some(*v),
        Parameter::I32(v) => Some(*v as f32),
        Parameter::U32(v) => Some(*v as f32),
        _ => None,
    }
}

/// What an AI program entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiKind {
    Ai,
    Action,
    Behavior,
    Query,
}

/// One AI, action, behaviour or query of an AI program.
#[derive(Clone, Debug)]
pub struct AiEntry {
    pub kind: AiKind,
    /// The designers' name, often Japanese (`移動`, `壁登り`).
    pub name: String,
    /// The game class that runs it (`PlayerMove`, `PlayerActionClimb`).
    pub class: String,
    static_params: ParameterObject,
    /// `ChildIdx`: child entries by the parent's name for them.
    children: ParameterObject,
}

impl AiEntry {
    /// A static parameter (`SInst`) as a number.
    pub fn number(&self, name: &str) -> Option<f32> {
        number(self.static_params.get(name)?)
    }

    /// A static parameter as text.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.static_params.get(name)?.as_str().ok()
    }

    /// The child the parent calls `key` (`剣装備`, `戦闘`), as an index
    /// into [`AiProgram::entries`].
    pub fn child(&self, key: &str) -> Option<usize> {
        child_index(self.children.get(key)?)
    }

    /// Every child, as indices into [`AiProgram::entries`].
    pub fn children(&self) -> impl Iterator<Item = usize> + '_ {
        self.children.0.values().filter_map(child_index)
    }

    /// `ChildIdx` as the game keeps it: every value in file order, an
    /// `Int` cut to u16 (`parseAIActionIdx`, `0x03772b74`). A value of
    /// another type is `None` (the dump has none).
    pub fn child_idx(&self) -> Vec<Option<u16>> {
        self.children.0.values().map(idx_value).collect()
    }

    /// `SInst` in file order: each parameter's name hash (crc32) and value.
    pub fn static_values(&self) -> impl Iterator<Item = (u32, AiValue)> + '_ {
        self.static_params
            .0
            .iter()
            .map(|(name, value)| (name.hash(), AiValue::from(value)))
    }
}

/// A parameter value of an AI program, as the file types it.
#[derive(Clone, Debug, PartialEq)]
pub enum AiValue {
    Bool(bool),
    F32(f32),
    Int(i32),
    U32(u32),
    Vec3([f32; 3]),
    /// Any string type.
    Str(String),
    /// A type AI programs do not use.
    Other,
}

impl From<&Parameter> for AiValue {
    fn from(parameter: &Parameter) -> Self {
        match parameter {
            Parameter::Bool(v) => Self::Bool(*v),
            Parameter::F32(v) => Self::F32(*v),
            Parameter::I32(v) => Self::Int(*v),
            Parameter::U32(v) => Self::U32(*v),
            Parameter::Vec3(v) => Self::Vec3([v.x, v.y, v.z]),
            other => other
                .as_str()
                .map_or(Self::Other, |s| Self::Str(s.to_owned())),
        }
    }
}

/// An index list value (`ChildIdx`, `DemoAIActionIdx`): the game reads it
/// as s32 and keeps u16.
fn idx_value(parameter: &Parameter) -> Option<u16> {
    match parameter {
        Parameter::I32(i) => Some(*i as u16),
        _ => None,
    }
}

/// A `ChildIdx` value: an index into the AIs followed by the actions,
/// which is how [`AiProgram::entries`] starts.
fn child_index(parameter: &Parameter) -> Option<usize> {
    match parameter {
        Parameter::I32(i) => usize::try_from(*i).ok(),
        _ => None,
    }
}

/// A parsed `.baiprog`.
#[derive(Clone, Debug, Default)]
pub struct AiProgram {
    pub entries: Vec<AiEntry>,
    /// `DemoAIActionIdx` of the root, in file order.
    pub demo_ai_action_idx: Vec<Option<u16>>,
}

impl AiProgram {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let io = ParameterIO::from_binary(bytes)
            .map_err(|_| FormatError::Invalid("baiprog: not AAMP"))?;
        let mut entries = Vec::new();
        for (kind, list, prefix) in [
            (AiKind::Ai, "AI", "AI_"),
            (AiKind::Action, "Action", "Action_"),
            (AiKind::Behavior, "Behavior", "Behavior_"),
            (AiKind::Query, "Query", "Query_"),
        ] {
            let Some(list) = io.param_root.lists.get(list) else {
                continue;
            };
            for entry in (0..).map_while(|i| list.lists.get(format!("{prefix}{i}").as_str())) {
                entries.push(ai_entry(kind, entry));
            }
        }
        let demo_ai_action_idx = io
            .param_root
            .objects
            .get("DemoAIActionIdx")
            .map(|o| o.0.values().map(idx_value).collect())
            .unwrap_or_default();
        Ok(Self {
            entries,
            demo_ai_action_idx,
        })
    }

    /// The entries of one kind, in file order.
    pub fn of_kind(&self, kind: AiKind) -> impl Iterator<Item = &AiEntry> {
        self.entries.iter().filter(move |e| e.kind == kind)
    }

    /// The entries run by class `class`.
    pub fn of_class<'a>(&'a self, class: &'a str) -> impl Iterator<Item = &'a AiEntry> {
        self.entries.iter().filter(move |e| e.class == class)
    }

    /// Static parameter `name` of the first entry of class `class` that has it.
    pub fn number(&self, class: &str, name: &str) -> Option<f32> {
        self.of_class(class).find_map(|e| e.number(name))
    }
}

fn ai_entry(kind: AiKind, list: &ParameterList) -> AiEntry {
    let def = list.objects.get("Def");
    let text = |name: &str| {
        def.and_then(|d| d.get(name))
            .and_then(|v| v.as_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    AiEntry {
        kind,
        name: text("Name"),
        class: text("ClassName"),
        static_params: list.objects.get("SInst").cloned().unwrap_or_default(),
        children: list.objects.get("ChildIdx").cloned().unwrap_or_default(),
    }
}

/// One shape the character controller can take (standing, crouching,
/// climbing), a vertical prism: `radius` wide, `height` tall from the feet.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterForm {
    pub kind: String,
    pub radius: f32,
    pub height: f32,
}

/// The character controller of a `.bphysics`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharacterController {
    pub forms: Vec<CharacterForm>,
    /// Water depth over the feet at which the controller floats.
    pub water_effective_height: Option<f32>,
    /// Scale of the water's lift on the controller.
    pub water_buoyancy_scale: Option<f32>,
    /// The controller body's mass (kg).
    pub mass: Option<f32>,
}

impl CharacterController {
    /// Reads the character controller from a `.bphysics`, if it has one.
    pub fn parse(bytes: &[u8]) -> Result<Option<Self>> {
        let io = ParameterIO::from_binary(bytes)
            .map_err(|_| FormatError::Invalid("bphysics: not AAMP"))?;
        let Some(list) = io
            .param_root
            .lists
            .get("ParamSet")
            .and_then(|set| set.lists.get("CharacterController"))
        else {
            return Ok(None);
        };
        // The header object's name is not in the game's string table; it is
        // the one with the controller's settings.
        let header = list
            .objects
            .iter()
            .map(|(_, o)| o)
            .find(|o| o.get("form_num").is_some());
        let header_number = |name: &str| header.and_then(|h| h.get(name)).and_then(number);
        let water_effective_height = header_number("water_effective_height");
        let water_buoyancy_scale = header_number("water_buoyancy_scale");
        let mass = header_number("mass");
        let forms = (0..)
            .map_while(|i| list.lists.get(format!("Form_{i}").as_str()))
            .filter_map(|form| {
                let kind = form
                    .objects
                    .get("FormHeader")?
                    .get("form_type")?
                    .as_str()
                    .ok()?
                    .to_owned();
                let shape = form.objects.get("ShapeParam_0")?;
                let radius = shape.get("radius").and_then(number)?;
                // A prism's `translate_0` holds its heights; the last is the top.
                let height = shape.get("translate_0")?.as_vec3().ok()?.z;
                Some(CharacterForm {
                    kind,
                    radius,
                    height,
                })
            })
            .collect();
        Ok(Some(Self {
            forms,
            water_effective_height,
            water_buoyancy_scale,
            mass,
        }))
    }

    pub fn form(&self, kind: &str) -> Option<&CharacterForm> {
        self.forms.iter().find(|f| f.kind == kind)
    }
}

/// One shape of a rigid body, in the body's own frame. A sphere has only
/// `from`; a capsule runs from `from` to `to`.
#[derive(Clone, Debug, PartialEq)]
pub struct BodyShape {
    /// `sphere`, `capsule`, `box`, ... as the file names it.
    pub kind: String,
    pub from: [f32; 3],
    pub to: [f32; 3],
    pub radius: f32,
}

/// A rigid body of a `.bphysics` rigid body set.
#[derive(Clone, Debug, PartialEq)]
pub struct RigidBody {
    /// The set's `set_name` (e.g. `Player`).
    pub set: String,
    /// `rigid_body_name` (e.g. `ClimbBody`).
    pub name: String,
    pub shapes: Vec<BodyShape>,
}

impl RigidBody {
    /// Reads every rigid body of every rigid body set in a `.bphysics`.
    pub fn parse_all(bytes: &[u8]) -> Result<Vec<Self>> {
        let io = ParameterIO::from_binary(bytes)
            .map_err(|_| FormatError::Invalid("bphysics: not AAMP"))?;
        let Some(sets) = io
            .param_root
            .lists
            .get("ParamSet")
            .and_then(|set| set.lists.get("RigidBodySet"))
        else {
            return Ok(Vec::new());
        };
        let text = |object: &ParameterObject, name: &str| {
            object
                .get(name)
                .and_then(|v| v.as_str().ok())
                .map(str::to_owned)
        };
        let vec3 = |object: &ParameterObject, name: &str| match object.get(name) {
            Some(Parameter::Vec3(v)) => Some([v.x, v.y, v.z]),
            _ => None,
        };
        let mut bodies = Vec::new();
        // Headers and bodies are keyed by names missing from the game's
        // string table; they are found by the fields they hold.
        for (_, set) in sets.lists.iter() {
            let Some(set_name) = set.objects.iter().find_map(|(_, o)| text(o, "set_name")) else {
                continue;
            };
            for (_, body) in set.lists.iter() {
                let Some(name) = body
                    .objects
                    .iter()
                    .find_map(|(_, o)| text(o, "rigid_body_name"))
                else {
                    continue;
                };
                let shapes = (0..)
                    .map_while(|i| body.objects.get(format!("ShapeParam_{i}").as_str()))
                    .filter_map(|shape| {
                        let from = vec3(shape, "translate_0")?;
                        Some(BodyShape {
                            kind: text(shape, "shape_type")?,
                            from,
                            to: vec3(shape, "translate_1").unwrap_or(from),
                            radius: shape.get("radius").and_then(number)?,
                        })
                    })
                    .collect();
                bodies.push(Self {
                    set: set_name.clone(),
                    name,
                    shapes,
                });
            }
        }
        Ok(bodies)
    }
}

/// A shape of a `.bchemical`: which rigid body stands for the actor in
/// the chemical (wind, fire, water) system and how much of the elements
/// it blocks.
#[derive(Clone, Debug, PartialEq)]
pub struct ChemicalShape {
    /// `rigid_set_name` and `rigid_name` of the `.bphysics` body.
    pub rigid_set: String,
    pub rigid_name: String,
    pub element_occlusion: f32,
}

impl ChemicalShape {
    /// Reads the shapes of a `.bchemical`, pairing each `rigid_c_NN` with
    /// its `shape_NN`.
    pub fn parse_all(bytes: &[u8]) -> Result<Vec<Self>> {
        let io = ParameterIO::from_binary(bytes)
            .map_err(|_| FormatError::Invalid("bchemical: not AAMP"))?;
        let Some(body) = io
            .param_root
            .lists
            .get("chemical_root")
            .and_then(|root| root.lists.get("chemical_body"))
        else {
            return Ok(Vec::new());
        };
        let text = |object: &ParameterObject, name: &str| {
            object
                .get(name)
                .and_then(|v| v.as_str().ok())
                .map(str::to_owned)
        };
        let shapes = (0..)
            .map_while(|i| {
                let rigid = body.objects.get(format!("rigid_c_{i:02}").as_str())?;
                let shape = body.objects.get(format!("shape_{i:02}").as_str())?;
                Some((rigid, shape))
            })
            .filter_map(|(rigid, shape)| {
                Some(Self {
                    rigid_set: text(rigid, "rigid_set_name")?,
                    rigid_name: text(rigid, "rigid_name")?,
                    element_occlusion: shape.get("element_occlusion").and_then(number)?,
                })
            })
            .collect();
        Ok(shapes)
    }
}

#[cfg(test)]
mod tests {
    use roead::aamp::{Parameter, ParameterIO, ParameterList, ParameterObject};
    use roead::types::Vector3f;

    use super::*;

    fn obj(pairs: &[(&str, Parameter)]) -> ParameterObject {
        let mut object = ParameterObject::new();
        for (name, value) in pairs {
            object.insert(*name, value.clone());
        }
        object
    }

    fn document(root: ParameterList) -> Vec<u8> {
        let mut io = ParameterIO::new();
        io.param_root = root;
        io.to_binary()
    }

    fn string(text: &str) -> Parameter {
        Parameter::String32(text.into())
    }

    #[test]
    fn reads_damage_parameters() {
        let root = ParameterList::new().with_list(
            "damage_param",
            ParameterList::new().with_object(
                "Parameters",
                obj(&[
                    ("FallDamageStartHeight", Parameter::F32(20.0)),
                    ("FallDamageMin", Parameter::I32(4)),
                ]),
            ),
        );
        let params = DamageParams::parse(&document(root)).unwrap();
        assert_eq!(params.number("FallDamageStartHeight"), Some(20.0));
        assert_eq!(params.number("FallDamageMin"), Some(4.0));
        assert_eq!(params.number("FallDamagePerMeter"), None);
        // Parameters outside the list are not the damage parameters.
        let flat = ParameterList::new()
            .with_object("Parameters", obj(&[("FallDamageMin", Parameter::I32(4))]));
        let flat = DamageParams::parse(&document(flat)).unwrap();
        assert_eq!(flat.number("FallDamageMin"), None);
        assert!(DamageParams::parse(b"not a parameter document").is_err());
    }

    #[test]
    fn reads_general_parameters() {
        let root = ParameterList::new().with_object(
            "Player",
            obj(&[
                ("ClimbEnableAngle", Parameter::F32(48.0)),
                ("ShortDashImpulse", Parameter::I32(900)),
            ]),
        );
        let params = GeneralParams::parse(&document(root)).unwrap();
        assert_eq!(params.number("Player", "ClimbEnableAngle"), Some(48.0));
        assert_eq!(params.number("Player", "ShortDashImpulse"), Some(900.0));
        assert_eq!(params.number("Player", "Missing"), None);
        assert_eq!(params.number("Enemy", "ClimbEnableAngle"), None);
    }

    #[test]
    fn reads_text_flags_and_vectors() {
        let root = ParameterList::new().with_object(
            "ArmorHead",
            obj(&[
                ("MaskType", string("Bokoblin")),
                ("IsDispOffPorch", Parameter::Bool(true)),
                (
                    "EarRotate",
                    Parameter::Vec3(roead::types::Vector3f {
                        x: 0.0,
                        y: 50.0,
                        z: 0.0,
                    }),
                ),
            ]),
        );
        let params = GeneralParams::parse(&document(root)).unwrap();
        assert_eq!(params.text("ArmorHead", "MaskType"), Some("Bokoblin"));
        assert_eq!(params.flag("ArmorHead", "IsDispOffPorch"), Some(true));
        assert_eq!(
            params.vec3("ArmorHead", "EarRotate"),
            Some([0.0, 50.0, 0.0])
        );
    }

    #[test]
    fn links_ai_entries_to_their_children() {
        let entry = |class: &str, children: &[(&str, i32)]| {
            let links: Vec<_> = children
                .iter()
                .map(|(key, i)| (*key, Parameter::I32(*i)))
                .collect();
            ParameterList::new()
                .with_object("Def", obj(&[("ClassName", string(class))]))
                .with_object("ChildIdx", obj(&links))
        };
        let ais = ParameterList::new()
            .with_list(
                "AI_0",
                entry("WeaponSelector", &[("剣装備", 1), ("槍装備", 2)]),
            )
            .with_list("AI_1", entry("EnemyBattle", &[("戦闘攻撃", 3)]))
            .with_list("AI_2", entry("EnemyBattle", &[]));
        let actions = ParameterList::new().with_list("Action_0", entry("Attack", &[]));
        let program = AiProgram::parse(&document(
            ParameterList::new()
                .with_list("AI", ais)
                .with_list("Action", actions),
        ))
        .unwrap();

        let selector = &program.entries[0];
        assert_eq!(selector.child("剣装備"), Some(1));
        assert_eq!(selector.child("槍装備"), Some(2));
        assert_eq!(selector.child("大剣装備"), None);
        assert_eq!(selector.children().count(), 2);
        // Indices go on from the AIs into the actions.
        let attack = program.entries[1].child("戦闘攻撃").unwrap();
        assert_eq!(program.entries[attack].class, "Attack");
    }

    #[test]
    fn reads_static_parameters_of_ai_classes() {
        let entry = |name: &str, class: &str, params: ParameterObject| {
            ParameterList::new()
                .with_object(
                    "Def",
                    obj(&[("Name", string(name)), ("ClassName", string(class))]),
                )
                .with_object("SInst", params)
        };
        let actions = ParameterList::new()
            .with_list(
                "Action_0",
                entry(
                    "Move",
                    "PlayerMove",
                    obj(&[("EnergyDash", Parameter::F32(250.0))]),
                ),
            )
            .with_list(
                "Action_1",
                entry(
                    "Ladder",
                    "PlayerLadderJump",
                    obj(&[("EnergyJump", Parameter::F32(1.0))]),
                ),
            )
            .with_list(
                "Action_2",
                entry(
                    "Jump",
                    "PlayerJump",
                    obj(&[("JumpHeight", Parameter::F32(0.5))]),
                ),
            );
        let ais = ParameterList::new().with_list(
            "AI_0",
            entry(
                "Swim",
                "PlayerSwim",
                obj(&[("EnableHeight", Parameter::F32(-1.0))]),
            ),
        );
        let program = AiProgram::parse(&document(
            ParameterList::new()
                .with_list("AI", ais)
                .with_list("Action", actions),
        ))
        .unwrap();

        assert_eq!(program.entries.len(), 4);
        assert_eq!(program.entries[0].kind, AiKind::Ai);
        assert_eq!(program.number("PlayerMove", "EnergyDash"), Some(250.0));
        assert_eq!(program.number("PlayerJump", "JumpHeight"), Some(0.5));
        assert_eq!(program.number("PlayerSwim", "EnableHeight"), Some(-1.0));
        assert_eq!(program.number("PlayerJump", "EnergyDash"), None);
        assert_eq!(
            program.of_class("PlayerLadderJump").next().unwrap().name,
            "Ladder"
        );
    }

    #[test]
    fn keeps_child_indices_static_values_and_demo_indices_in_file_order() {
        let normal = ParameterList::new()
            .with_object("Def", obj(&[("ClassName", string("PlayerNormal"))]))
            .with_object(
                "ChildIdx",
                obj(&[
                    ("落下", Parameter::I32(3)),
                    ("着地", Parameter::I32(2)),
                    ("騎乗", Parameter::I32(0x1_0001)),
                    ("壊れ", Parameter::F32(1.0)),
                ]),
            )
            .with_object(
                "SInst",
                obj(&[
                    ("ToFallHeightForJustRush", Parameter::F32(2.0)),
                    ("Count", Parameter::I32(-1)),
                    ("Tree", Parameter::StringRef("x".into())),
                ]),
            );
        let program = AiProgram::parse(&document(
            ParameterList::new()
                .with_list("AI", ParameterList::new().with_list("AI_0", normal))
                .with_object(
                    "DemoAIActionIdx",
                    obj(&[("a", Parameter::I32(5)), ("b", Parameter::I32(4))]),
                ),
        ))
        .unwrap();

        let normal = &program.entries[0];
        assert_eq!(normal.child_idx(), vec![Some(3), Some(2), Some(1), None]);
        let values: Vec<_> = normal.static_values().collect();
        assert_eq!(
            values,
            vec![
                (
                    roead::aamp::Name::from("ToFallHeightForJustRush").hash(),
                    AiValue::F32(2.0)
                ),
                (roead::aamp::Name::from("Count").hash(), AiValue::Int(-1)),
                (
                    roead::aamp::Name::from("Tree").hash(),
                    AiValue::Str("x".into())
                ),
            ]
        );
        assert_eq!(program.demo_ai_action_idx, vec![Some(5), Some(4)]);
        assert_eq!(program.of_kind(AiKind::Ai).count(), 1);
        assert_eq!(program.of_kind(AiKind::Action).count(), 0);
    }

    #[test]
    fn reads_the_character_controller_forms() {
        let form = |kind: &str, height: f32| {
            ParameterList::new()
                .with_object("FormHeader", obj(&[("form_type", string(kind))]))
                .with_object(
                    "ShapeParam_0",
                    obj(&[
                        ("shape_type", string("character_prism")),
                        (
                            "translate_0",
                            Parameter::Vec3(Vector3f {
                                x: 0.5,
                                y: height - 0.2,
                                z: height,
                            }),
                        ),
                        ("radius", Parameter::F32(0.4)),
                    ]),
                )
        };
        let controller = ParameterList::new()
            .with_object(
                "Header",
                obj(&[
                    ("form_num", Parameter::I32(2)),
                    ("water_effective_height", Parameter::F32(1.3)),
                    ("water_buoyancy_scale", Parameter::F32(0.5)),
                    ("mass", Parameter::F32(150.0)),
                ]),
            )
            .with_list("Form_0", form("Standing", 1.7))
            .with_list("Form_1", form("Climbing", 1.2));
        let root = ParameterList::new().with_list(
            "ParamSet",
            ParameterList::new().with_list("CharacterController", controller),
        );
        let parsed = CharacterController::parse(&document(root))
            .unwrap()
            .unwrap();
        assert_eq!(parsed.water_effective_height, Some(1.3));
        assert_eq!(parsed.water_buoyancy_scale, Some(0.5));
        assert_eq!(parsed.mass, Some(150.0));
        assert_eq!(
            parsed.form("Standing"),
            Some(&CharacterForm {
                kind: "Standing".into(),
                radius: 0.4,
                height: 1.7
            })
        );
        assert_eq!(parsed.form("Climbing").unwrap().height, 1.2);
        assert!(
            CharacterController::parse(&document(ParameterList::new()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn reads_chemical_shapes() {
        let body = ParameterList::new()
            .with_object(
                "rigid_c_00",
                obj(&[
                    ("rigid_set_name", string("Chemical")),
                    ("rigid_name", string("TgtChemical")),
                ]),
            )
            .with_object(
                "shape_00",
                obj(&[("element_occlusion", Parameter::F32(0.8))]),
            );
        let root = ParameterList::new().with_list(
            "chemical_root",
            ParameterList::new().with_list("chemical_body", body),
        );
        assert_eq!(
            ChemicalShape::parse_all(&document(root)).unwrap(),
            [ChemicalShape {
                rigid_set: "Chemical".into(),
                rigid_name: "TgtChemical".into(),
                element_occlusion: 0.8,
            }]
        );
        assert!(
            ChemicalShape::parse_all(&document(ParameterList::new()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn reads_rigid_body_shapes() {
        let v = |x: f32, y: f32, z: f32| Parameter::Vec3(Vector3f { x, y, z });
        let capsule = obj(&[
            ("shape_type", string("capsule")),
            ("translate_0", v(0.0, 0.5, -0.1)),
            ("translate_1", v(0.0, 1.2, -0.1)),
            ("radius", Parameter::F32(0.2)),
        ]);
        let sphere = obj(&[
            ("shape_type", string("sphere")),
            ("translate_0", v(0.0, 0.4, 0.0)),
            ("radius", Parameter::F32(0.4)),
        ]);
        let body = |name: &str, shapes: &[ParameterObject]| {
            let mut list = ParameterList::new()
                .with_object("Param", obj(&[("rigid_body_name", string(name))]));
            for (i, shape) in shapes.iter().enumerate() {
                list = list.with_object(format!("ShapeParam_{i}").as_str(), shape.clone());
            }
            list
        };
        let set = ParameterList::new()
            .with_object("Header", obj(&[("set_name", string("Player"))]))
            .with_list("A", body("ClimbMoveCheck", &[capsule]))
            .with_list("B", body("Tgt", &[sphere.clone(), sphere]));
        let root = ParameterList::new().with_list(
            "ParamSet",
            ParameterList::new()
                .with_list("RigidBodySet", ParameterList::new().with_list("Set", set)),
        );
        let bodies = RigidBody::parse_all(&document(root)).unwrap();

        assert_eq!(bodies.len(), 2);
        let check = bodies.iter().find(|b| b.name == "ClimbMoveCheck").unwrap();
        assert_eq!(check.set, "Player");
        assert_eq!(
            check.shapes,
            [BodyShape {
                kind: "capsule".into(),
                from: [0.0, 0.5, -0.1],
                to: [0.0, 1.2, -0.1],
                radius: 0.2
            }]
        );
        let target = bodies.iter().find(|b| b.name == "Tgt").unwrap();
        assert_eq!(target.shapes.len(), 2);
        assert_eq!(
            target.shapes[0].to, target.shapes[0].from,
            "a sphere has one centre"
        );
        assert!(
            RigidBody::parse_all(&document(ParameterList::new()))
                .unwrap()
                .is_empty()
        );
    }
}
