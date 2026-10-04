//! Animation sequences (AS): how an actor picks, blends and times its
//! skeletal animations. An actor pack's `Actor/ASList/*.baslist` names the
//! animation files (`AddReses`), maps each AS name the game code plays to a
//! `Actor/AS/<file>.bas` (`ASDefines`), and sets cross-fade times between
//! particular pairs of AS (`CFDefines`). A `.bas` is a tree of elements:
//! selectors pick one child by a value (stick angle, speed, the previous
//! AS...), blenders mix children, containers play them in sequence, and
//! skeletal assets name an FSKA clip with its playback rate and the morph
//! (blend-in) time. All times are in frames of the game's 30 Hz.
//!
//! Element type names follow the zeldaret/botw decompilation
//! (`resResourceASResource.cpp`).

use roead::aamp::{ParameterIO, ParameterList, ParameterObject};

use crate::{FormatError, Result};

/// Element classes by type index, as the game registers them.
pub const ELEMENT_TYPES: [&str; 107] = [
    "AbsTemperatureBlender",
    "AbsTemperatureSelector",
    "ArmorSelector",
    "ArrowSelector",
    "AttentionSelector",
    "BoneBlender",
    "BoneVisibilityAsset",
    "BoolSelector",
    "ButtonSelector",
    "ChargeSelector",
    "ClearMatAnmAsset",
    "ComboSelector",
    "DiffAngleYBlender",
    "DiffAngleYSelector",
    "DirectionAngleBlender",
    "DirectionAngleSelector",
    "DistanceBlender",
    "DistanceSelector",
    "DungeonClearSelector",
    "DungeonNumberSelector",
    "EmotionSelector",
    "EventFlagSelector",
    "EyeSelector",
    "EyebrowSelector",
    "FaceEmotionSelector",
    "FootBLLifeSelector",
    "FootBRLifeSelector",
    "FootFLLifeSelector",
    "FootFRLifeSelector",
    "ForwardBentBlender",
    "ForwardBentSelector",
    "GearSelector",
    "GenerationSelector",
    "GrabTypeSelector",
    "GroundNormalBlender",
    "GroundNormalSelector",
    "GroundNormalSideBlender",
    "GroundNormalSideSelector",
    "MaskSelector",
    "MatVisibilityAsset",
    "MouthSelector",
    "NoAnmAsset",
    "NoLoopStickAngleBlender",
    "NoLoopStickAngleSelector",
    "NodePosSelector",
    "PersonalitySelector",
    "PostureSelector",
    "PreASSelector",
    "PreExclusionRandomSelector",
    "RandomSelector",
    "RideSelector",
    "RightStickAngleBlender",
    "RightStickAngleSelector",
    "RightStickValueBlender",
    "RightStickValueSelector",
    "RightStickXBlender",
    "RightStickXSelector",
    "RightStickYBlender",
    "RightStickYSelector",
    "SelfHeightSelector",
    "SelfWeightSelector",
    "SequencePlayContainer",
    "ShaderParamAsset",
    "ShaderParamColorAsset",
    "ShaderParamTexSRTAsset",
    "SizeBlender",
    "SizeSelector",
    "SkeltalAsset",
    "SpeedBlender",
    "SpeedSelector",
    "StickAngleBlender",
    "StickAngleSelector",
    "StickValueBlender",
    "StickValueSelector",
    "StickXBlender",
    "StickXSelector",
    "StickYBlender",
    "StickYSelector",
    "StressBlender",
    "StressSelector",
    "SyncPlayContainer",
    "TemperatureBlender",
    "TemperatureSelector",
    "TexturePatternAsset",
    "TimeSelector",
    "TiredBlender",
    "TiredSelector",
    "UseItemSelector",
    "UserAngle2Blender",
    "UserAngle2Selector",
    "UserAngleBlender",
    "UserAngleSelector",
    "UserSpeedBlender",
    "UserSpeedSelector",
    "VariationSelector",
    "WallAngleBlender",
    "WallAngleSelector",
    "WeaponDetailSelector",
    "WeaponSelector",
    "WeatherSelector",
    "WeightBlender",
    "WeightSelector",
    "WindVelocityBlender",
    "YSpeedBlender",
    "YSpeedSelector",
    "ZEx00ExposureBlender",
    "ZEx00ExposureSelector",
];

/// The game's default morph (blend-in) time of a skeletal asset, in frames.
pub const DEFAULT_MORPH: f32 = 5.0;

/// One node of an AS tree.
// SI-ANM-08: many AS element fields are not read.
#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub type_index: u16,
    /// Indices into [`AnimSeq::elements`].
    pub children: Vec<usize>,
    /// A skeletal asset's clip.
    pub file_name: Option<String>,
    /// Frames to blend in a skeletal asset (default [`DEFAULT_MORPH`]).
    pub morph: f32,
    /// Playback rate (`FrameCtrl`), 1 by default.
    pub rate: f32,
    /// First clip frame to play (`FrameCtrl` `StartFrame`), 0 by default.
    pub start_frame: f32,
    /// Last clip frame to play (`FrameCtrl` `EndFrame`); `None` plays to the
    /// clip's end. The game's default is −1 (`ASFrameCtrlParser`, range
    /// −1…100); 0 is a real frame — static poses hold frame 0 with it.
    pub end_frame: Option<f32>,
    /// Which foot must be ahead for a fresh start to begin halfway through
    /// the frames played (`FrameCtrl` `FootType`, 0 by default: never).
    pub foot_type: i32,
    /// Passes a loop plays before it counts as done (`FrameCtrl`
    /// `LoopStopCount`, −1 by default: never), plus a random whole number
    /// from 0 to `LoopStopCountRandom` (0 by default) when it is positive.
    pub loop_stop_count: f32,
    pub loop_stop_count_random: f32,
    /// Selector/blender ranges, one per child: `(start, end)`.
    pub ranges: Vec<(f32, f32)>,
    /// A selector's string keys, one per child (e.g. previous AS names).
    pub strings: Vec<String>,
    /// A sequence container's per-child values.
    pub floats: Vec<f32>,
    pub ints: Vec<i32>,
    /// A sequence container that loops over its children.
    pub sequence_loop: bool,
    /// A blender's input may change by at most this much per frame (its
    /// `InputLimit`; unlimited when absent).
    pub input_limit: Option<f32>,
    /// `JudgeOnce` as the file gives it; [`Element::judges_once`] adds the
    /// game's default.
    pub judge_once: Option<bool>,
    /// `NoSync` (the resource's +0x88, false by default): a blender or
    /// selector changing its choice on the way starts the new children
    /// afresh rather than in step with the phase of the one before
    /// (`0x039628f0`, `0x03965400`).
    pub no_sync: bool,
    /// The flag a `BoolSelector` tests (`Extend/BitIndex/BitIndex0`
    /// `TypeIndex`).
    pub bit_index: Option<i32>,
    /// A skeletal asset's hold events (`Extend/HoldEvents`): the frames
    /// over which each is on, under the number the game queries it by.
    pub hold_events: Vec<HoldEvent>,
    /// A skeletal asset's trigger events (`Extend/TriggerEvents`), each
    /// fired once at a frame.
    pub trigger_events: Vec<TriggerEvent>,
}

/// An event an AS clip holds over a span of its frames.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HoldEvent {
    /// The number the game's queries use: the file's `TypeIndex`, or 88
    /// past 53 (`ASHoldEventsParser::parse`, zeldaret/botw, matching; a
    /// trigger event's number is its `TypeIndex` + 54 instead, which the
    /// Wii U queries confirm, e.g. 0x54 for `ParaEquipOff`'s trigger 30).
    pub id: u16,
    pub start_frame: f32,
    pub end_frame: f32,
}

/// An event an AS clip fires at one of its frames.
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerEvent {
    /// The file's `TypeIndex` (the game queries it as `TypeIndex` + 54;
    /// type 7 starts the face AS named by `value`).
    pub type_index: i32,
    pub frame: f32,
    pub value: String,
}

/// The number the game gives a hold event of file type `type_index`.
pub fn hold_event_id(type_index: i32) -> u16 {
    if (0..=53).contains(&type_index) {
        type_index as u16
    } else {
        88
    }
}

/// What a selector or blender of an AS tree goes by: a value (speed,
/// stick, angle, a flag as 0 or 1) or, for selectors keyed by strings
/// (weapon, previous AS, the foot ahead), a key.
#[derive(Clone, Debug, PartialEq)]
pub enum AsInput {
    Value(f32),
    Key(String),
}

impl Element {
    pub fn type_name(&self) -> &'static str {
        ELEMENT_TYPES
            .get(self.type_index as usize)
            .copied()
            .unwrap_or("?")
    }

    /// Blenders mix their children by an input value.
    pub fn is_blender(&self) -> bool {
        self.type_name().ends_with("Blender")
    }

    /// Selectors play one child, chosen by an input value.
    pub fn is_selector(&self) -> bool {
        self.type_name().ends_with("Selector")
    }

    /// Whether the node chooses only when it (re)starts, keeping that
    /// choice every frame after (`JudgeOnce`, the resource's +0x98): by
    /// default selectors do (`0x0392ca9c`) and blenders do not
    /// (`0x0392cc8c`), as the Wii U game (v208) registers them.
    pub fn judges_once(&self) -> bool {
        self.judge_once.unwrap_or(self.is_selector())
    }

    /// The child position a selector picks at input `value`, as the Wii U
    /// game (v208) does. A selector with ranges (`0x03969808`, the choice
    /// slot +0x13c of `YSpeedSelector` and other range selectors) takes the
    /// first child whose range holds the value, `start <= value < end`, or
    /// a zero-width range at exactly that value; failing that, the last
    /// child if the value is exactly its range's end; else none (−1 in the
    /// game). A `BoolSelector` (`0x03965ce8`) takes child 1 when its flag
    /// is set (`value` ≠ 0), else child 0. Angle selectors
    /// ([`Element::select_angle`]) wrap round. Selectors keyed by strings
    /// are not chosen here.
    pub fn select(&self, value: f32) -> Option<usize> {
        match self.type_name() {
            "BoolSelector" => return Some(usize::from(value != 0.0)),
            "StickAngleSelector"
            | "RightStickAngleSelector"
            | "DirectionAngleSelector"
            | "UserAngleSelector"
            | "UserAngle2Selector" => return self.select_angle(value),
            _ => {}
        }
        let ranges = &self.ranges;
        let held = ranges.iter().position(|&(start, end)| {
            (start <= value && value < end) || (start == value && value == end)
        });
        held.or_else(|| {
            ranges
                .last()
                .filter(|&&(_, end)| end == value)
                .map(|_| ranges.len() - 1)
        })
    }

    /// The child position an angle selector picks at `value` degrees, as
    /// the Wii U game (v208) does (`0x0396788c`, the choice slot +0x13c of
    /// the node vtable `0x10345074` shared by `StickAngleSelector` and the
    /// other angle selectors): the value is brought into [−180, 180] by one
    /// turn; ranges at the end of the list reaching past 180 are first
    /// tried a turn lower, then every range as it is, each holding
    /// `start <= value < end` or a zero-width range at exactly that value;
    /// failing that, the last child if the value is exactly its range's
    /// end; else none. So `[90, 270]` takes the back half either side and
    /// `[1, 359]` everything but `[-1, 1)`.
    pub fn select_angle(&self, value: f32) -> Option<usize> {
        const MIN: f32 = -180.0;
        const MAX: f32 = 180.0;
        const TURN: f32 = MAX - MIN;
        let value = if value > MAX {
            value - TURN
        } else if value < MIN {
            value + TURN
        } else {
            value
        };
        let ranges = &self.ranges;
        let n = ranges.len();
        let past = ranges
            .iter()
            .rev()
            .take_while(|&&(_, end)| end > MAX)
            .count();
        let holds = |start: f32, end: f32| {
            (start <= value && value < end) || (start == value && value == end)
        };
        let lowered = (n - past..n).find(|&i| holds(ranges[i].0 - TURN, ranges[i].1 - TURN));
        lowered
            .or_else(|| (0..n).find(|&i| holds(ranges[i].0, ranges[i].1)))
            .or_else(|| {
                ranges
                    .last()
                    .filter(|&&(_, end)| end == value)
                    .map(|_| n - 1)
            })
    }

    /// The child position a `NodePosSelector` picks, as the Wii U game
    /// (v208) does (`0x03968744`, the choice slot +0x13c of vtable
    /// `0x10345294`): each child's key names a bone and an axis of the
    /// model (`"<bone>,<axis>"`, axis `X`, `Y` or `Z`, parsed by
    /// `0x03967f70`), and the child whose bone is furthest along its axis
    /// plays — strictly further, so the first on a tie. `position` gives a
    /// bone's position in the model's axes (from the model's origin); an
    /// unknown bone or axis leaves its child out. None without any.
    pub fn select_node_pos(
        &self,
        position: &mut dyn FnMut(&str) -> Option<[f32; 3]>,
    ) -> Option<usize> {
        let mut best: Option<(usize, f32)> = None;
        for (i, key) in self.strings.iter().enumerate().take(self.children.len()) {
            let Some((bone, axis)) = key.split_once(',') else {
                continue;
            };
            let axis = match axis {
                "X" => 0,
                "Y" => 1,
                "Z" => 2,
                _ => continue,
            };
            let Some(at) = position(bone) else {
                continue;
            };
            if best.is_none_or(|(_, far)| at[axis] > far) {
                best = Some((i, at[axis]));
            }
        }
        best.map(|(i, _)| i)
    }

    /// The child position a selector keyed by strings picks for `key`: the
    /// child with that key, else the one keyed `default`.
    pub fn select_key(&self, key: &str) -> Option<usize> {
        let position = |k: &str| self.strings.iter().position(|s| s == k);
        position(key).or_else(|| position("default"))
    }

    /// The AS input a selector or blender of this class reads, as the Wii
    /// U game (v208) registers it (the class table `0x1047d0f0`, its fourth
    /// column; a selector and a blender of one name share it). `None` for
    /// assets, containers and classes outside the table (`BoneBlender`).
    pub fn input(&self) -> Option<u8> {
        let name = self.type_name();
        let class = name
            .strip_suffix("Blender")
            .or_else(|| name.strip_suffix("Selector"))?;
        INPUTS
            .iter()
            .find(|&&(c, _)| c == class)
            .map(|&(_, input)| input)
    }

    /// Whether this is a blender of a wrap-around angle (`StickAngle`,
    /// `RightStickAngle`, `DirectionAngle`, `UserAngle`, `UserAngle2`: the
    /// Wii U game's factory `0x0392e668`, node vtable `0x10328b20`), which
    /// chooses its pair round the circle ([`Element::pair`]) and eases its
    /// input the short way round (`0x0385e908`).
    pub fn is_angle_blender(&self) -> bool {
        matches!(
            self.type_name(),
            "StickAngleBlender"
                | "RightStickAngleBlender"
                | "DirectionAngleBlender"
                | "UserAngleBlender"
                | "UserAngle2Blender"
        )
    }

    /// The children a blender plays at input `value` and the second's
    /// share, as the Wii U game (v208) chooses them (node slot +0x13c).
    ///
    /// Most blenders (`0x03963fe8`): the first child whose range holds the
    /// value (`start <= v < end`); if the next one's does too, both, the
    /// second's share `t` across where the two ranges overlap (under 0.01
    /// none, over 0.99 all, the second kept either way), or the next alone
    /// when the overlap is empty. Held by none: the last child from the
    /// first range's start on, else the first.
    ///
    /// Angle blenders ([`Element::is_angle_blender`], `0x038662a0`): the
    /// value is first brought into [−180, 180] by one turn, and ranges at
    /// the end of the list reaching past 180 are tried first, a turn lower,
    /// so the last child pairs with the first; otherwise the same, except
    /// that an empty overlap keeps whichever of the two reaches further.
    pub fn pair(&self, value: f32) -> Pair {
        let ranges = &self.ranges;
        let last = self.children.len().max(1) - 1;
        let alone = |a| Pair { a, b: None, t: 0.0 };
        let share = |v: f32, low: f32, high: f32| {
            let t = (v - low) / (high - low);
            if t < SHARE_EPSILON {
                0.0
            } else if t > 1.0 - SHARE_EPSILON {
                1.0
            } else {
                t
            }
        };
        let n = ranges.len() as isize;
        let (value, first, turn) = if self.is_angle_blender() {
            const MIN: f32 = -180.0;
            const MAX: f32 = 180.0;
            let value = if value > MAX {
                value - (MAX - MIN)
            } else if value < MIN {
                value + (MAX - MIN)
            } else {
                value
            };
            let past = ranges
                .iter()
                .rev()
                .take_while(|&&(_, end)| end > MAX)
                .count();
            (value, -(past as isize), MAX - MIN)
        } else {
            (value, 0, 0.0)
        };
        let position = |i: isize| if i < 0 { i + n } else { i } as usize;
        let range = |i: isize| {
            let (start, end) = ranges[position(i)];
            if i < 0 {
                (start - turn, end - turn)
            } else {
                (start, end)
            }
        };
        let holds = |(start, end): (f32, f32)| start <= value && value < end;
        for i in first..n {
            let here = range(i);
            if !holds(here) {
                continue;
            }
            let next = i + 1;
            if next >= n || !holds(range(next)) {
                return alone(position(i));
            }
            let there = range(next);
            let low = here.0.max(there.0);
            let high = here.1.min(there.1);
            // Both holding the value, the overlap is never empty; the
            // game still guards it: the linear choice takes the next, the
            // angle one whichever reaches further.
            if high - low <= 0.0 {
                return if turn > 0.0 && there.1 <= here.1 {
                    alone(position(i))
                } else {
                    alone(position(next))
                };
            }
            return Pair {
                a: position(i),
                b: Some(position(next)),
                t: share(value, low, high),
            };
        }
        match ranges.first() {
            Some(&(start, _)) if value >= start => alone(last),
            _ => alone(0),
        }
    }

    /// How a blender mixes its children at input `value`: `(child
    /// position, weight)` for its [`Element::pair`], the first by 1 − `t`,
    /// the second by `t`.
    pub fn blend_weights(&self, value: f32) -> Vec<(usize, f32)> {
        self.pair(value).weights()
    }
}

/// Blend shares under this count as none and over 1 less this as all
/// (`0x03963fe8`, `0x038662a0` via `0x0396446c`).
const SHARE_EPSILON: f32 = 0.01;

/// The input each selector and blender class reads, by the class's name
/// less `Selector`/`Blender` (the Wii U game's class table `0x1047d0f0`,
/// v208; `BoolSelector` reads its bits from input 66).
const INPUTS: &[(&str, u8)] = &[
    ("Bool", 66),
    ("Button", 46),
    ("Charge", 42),
    ("Combo", 43),
    ("DiffAngleY", 26),
    ("DirectionAngle", 9),
    ("Distance", 16),
    ("DungeonClear", 64),
    ("DungeonNumber", 41),
    ("Emotion", 55),
    ("EventFlag", 65),
    ("Eye", 37),
    ("Eyebrow", 38),
    ("FaceEmotion", 56),
    ("FootBLLife", 34),
    ("FootBRLife", 33),
    ("FootFLLife", 32),
    ("FootFRLife", 31),
    ("ForwardBent", 18),
    ("Gear", 54),
    ("Generation", 35),
    ("GrabType", 49),
    ("GroundNormal", 21),
    ("GroundNormalSide", 22),
    ("Mask", 58),
    ("Mouth", 36),
    ("NoLoopStickAngle", 7),
    ("NodePos", 63),
    ("Personality", 50),
    ("Posture", 59),
    ("PreAS", 51),
    ("PreExclusionRandom", 30),
    ("Random", 30),
    ("Ride", 61),
    ("RightStickAngle", 8),
    ("RightStickValue", 3),
    ("RightStickX", 4),
    ("RightStickY", 5),
    ("SelfHeight", 39),
    ("SelfWeight", 40),
    ("Size", 17),
    ("Speed", 19),
    ("StickAngle", 6),
    ("StickValue", 0),
    ("StickX", 1),
    ("StickY", 2),
    ("Stress", 14),
    ("Temperature", 23),
    ("Time", 52),
    ("Tired", 13),
    ("UseItem", 62),
    ("UserAngle", 11),
    ("UserAngle2", 12),
    ("UserSpeed", 10),
    ("Variation", 47),
    ("WallAngle", 15),
    ("WeaponDetail", 45),
    ("Weapon", 44),
    ("Weather", 53),
    ("Weight", 25),
    ("WindVelocity", 27),
    ("YSpeed", 20),
    ("ZEx00Exposure", 29),
];

/// The children a blender plays (positions among its children) and the
/// second's share of the weight, `a` having the rest (the node's state:
/// bytes 0 and 1, −1 for no second, and the float at +4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pair {
    pub a: usize,
    pub b: Option<usize>,
    pub t: f32,
}

impl Pair {
    /// `(child position, weight)`: `a` by 1 − `t`, `b` by `t`.
    pub fn weights(self) -> Vec<(usize, f32)> {
        std::iter::once((self.a, 1.0 - self.t))
            .chain(self.b.map(|b| (b, self.t)))
            .collect()
    }
}

/// A parsed `.bas`: element 0 is the root.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimSeq {
    pub elements: Vec<Element>,
}

impl AnimSeq {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let io =
            ParameterIO::from_binary(bytes).map_err(|_| FormatError::Invalid("bas: not AAMP"))?;
        let Some(list) = io.param_root.lists.get("Elements") else {
            return Ok(Self::default());
        };
        let elements = (0..)
            .map_while(|i| list.lists.get(format!("Element{i}").as_str()))
            .map(element)
            .collect();
        Ok(Self { elements })
    }

    /// The skeletal assets under element `index`, depth first: every clip
    /// this AS can play from there.
    pub fn clips(&self, index: usize) -> Vec<&Element> {
        let mut found = Vec::new();
        let mut stack = vec![index];
        let mut seen = vec![false; self.elements.len()];
        while let Some(i) = stack.pop() {
            let Some(element) = self.elements.get(i) else {
                continue;
            };
            if std::mem::replace(&mut seen[i], true) {
                continue;
            }
            if element.file_name.is_some() {
                found.push(element);
            }
            stack.extend(element.children.iter().rev());
        }
        found
    }

    /// The first skeletal asset playing clip `name`.
    pub fn clip(&self, name: &str) -> Option<&Element> {
        self.elements
            .iter()
            .find(|e| e.file_name.as_deref() == Some(name))
    }

    /// Index of the first skeletal asset playing clip `name`.
    pub fn clip_index(&self, name: &str) -> Option<usize> {
        self.elements
            .iter()
            .position(|e| e.file_name.as_deref() == Some(name))
    }

    /// The element the selectors from `index` down lead to: each selector
    /// picks a child by its input ([`Element::select`]) until an element
    /// that is no selector (a clip, blender or container). `input` gives a
    /// selector its value (by element index); `None` when a selector has no
    /// input or picks no child.
    pub fn select(
        &self,
        index: usize,
        input: &mut dyn FnMut(usize, &Element) -> Option<f32>,
    ) -> Option<usize> {
        let mut index = index;
        for _ in 0..32 {
            let element = self.elements.get(index)?;
            if !element.is_selector() {
                return Some(index);
            }
            let position = element.select(input(index, element)?)?;
            index = *element.children.get(position)?;
        }
        None
    }

    /// The blend around the clip at element `base`: the clips the blenders
    /// above it mix, with their weights (summing to 1). Evaluation starts at
    /// the topmost blender of the unbroken chain of blenders above `base`.
    /// `input` gives a blender its value (by element index); a blender
    /// without one — and any selector — keeps to the branch leading to
    /// `base`, or its first child off that branch.
    pub fn blend(
        &self,
        base: usize,
        input: &mut dyn FnMut(usize, &Element) -> Option<f32>,
    ) -> Vec<(usize, f32)> {
        let mut parent = vec![None; self.elements.len()];
        // Elements are shared between branches; the first parent found wins.
        for (index, element) in self.elements.iter().enumerate() {
            for &child in &element.children {
                if child < parent.len() && parent[child].is_none() && child != index {
                    parent[child] = Some(index);
                }
            }
        }
        let mut path = vec![base];
        let mut top = base;
        while let Some(up) = parent.get(top).copied().flatten() {
            if !self.elements[up].is_blender() || path.contains(&up) {
                break;
            }
            path.push(up);
            top = up;
        }
        let mut weights: Vec<(usize, f32)> = Vec::new();
        self.visit(top, 1.0, &path, input, &mut weights, 0);
        weights
    }

    /// Every clip the tree plays from element `index`, with its weight
    /// (summing to 1): selectors pick one child (by value, see
    /// [`Element::select`], or by key, [`Element::select_key`]), blenders
    /// mix theirs ([`Element::blend_weights`]) and containers play their
    /// first child. `input` gives each selector and blender its input (by
    /// element index); without one a selector or blender keeps to its first
    /// child. A selector that picks nothing drops its branch.
    pub fn mix(
        &self,
        index: usize,
        input: &mut dyn FnMut(usize, &Element) -> Option<AsInput>,
    ) -> Vec<(usize, f32)> {
        let mut out: Vec<(usize, f32)> = Vec::new();
        let mut stack = vec![(index, 1.0_f32, 0_usize)];
        while let Some((index, weight, depth)) = stack.pop() {
            let Some(element) = self.elements.get(index) else {
                continue;
            };
            if depth > 32 || weight <= 1e-4 {
                continue;
            }
            if element.file_name.is_some() || element.children.is_empty() {
                match out.iter_mut().find(|(i, _)| *i == index) {
                    Some((_, w)) => *w += weight,
                    None => out.push((index, weight)),
                }
                continue;
            }
            let children: Vec<(usize, f32)> = match input(index, element) {
                Some(AsInput::Value(value)) if element.is_blender() => element.blend_weights(value),
                Some(AsInput::Value(value)) if element.is_selector() => element
                    .select(value)
                    .map(|i| (i, 1.0))
                    .into_iter()
                    .collect(),
                Some(AsInput::Key(key)) if element.is_selector() => element
                    .select_key(&key)
                    .map(|i| (i, 1.0))
                    .into_iter()
                    .collect(),
                _ => vec![(0, 1.0)],
            };
            for (position, w) in children.into_iter().rev() {
                if let Some(&child) = element.children.get(position) {
                    stack.push((child, weight * w, depth + 1));
                }
            }
        }
        out
    }

    fn visit(
        &self,
        index: usize,
        weight: f32,
        path: &[usize],
        input: &mut dyn FnMut(usize, &Element) -> Option<f32>,
        out: &mut Vec<(usize, f32)>,
        depth: usize,
    ) {
        let Some(element) = self.elements.get(index) else {
            return;
        };
        if depth > 32 || weight <= 1e-4 {
            return;
        }
        if element.file_name.is_some() || element.children.is_empty() {
            match out.iter_mut().find(|(i, _)| *i == index) {
                Some((_, w)) => *w += weight,
                None => out.push((index, weight)),
            }
            return;
        }
        let on_path = element.children.iter().position(|c| path.contains(c));
        let children: Vec<(usize, f32)> = match (element.is_blender(), on_path) {
            (true, on_path) => match input(index, element) {
                Some(value) => element.blend_weights(value),
                None => vec![(on_path.unwrap_or(0), 1.0)],
            },
            (false, Some(i)) => vec![(i, 1.0)],
            (false, None) => vec![(0, 1.0)],
        };
        for (position, w) in children {
            if let Some(&child) = element.children.get(position) {
                self.visit(child, weight * w, path, input, out, depth + 1);
            }
        }
    }
}

fn element(list: &ParameterList) -> Element {
    let params = list.objects.get("Parameters");
    let get = |name: &str| params.and_then(|p| p.get(name));
    let children = list
        .objects
        .get("Children")
        .map(|c| {
            c.iter()
                .filter_map(|(_, v)| v.as_int::<i64>().ok())
                .map(|i| i as usize)
                .collect()
        })
        .unwrap_or_default();
    let extend = list.lists.get("Extend");
    let first = |name: &str| {
        extend
            .and_then(|e| e.lists.get(name))
            .and_then(|l| l.objects.get(format!("{name}0").as_str()))
    };
    let values = |name: &str| {
        first(name)
            .map(|o| o.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let ranges = extend
        .and_then(|e| e.lists.get("Ranges"))
        .map(|l| {
            (0..)
                .map_while(|i| l.objects.get(format!("Range{i}").as_str()))
                .map(|r| {
                    (
                        float(r, "Start").unwrap_or(0.0),
                        float(r, "End").unwrap_or(0.0),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    Element {
        type_index: get("TypeIndex")
            .and_then(|v| v.as_int::<i64>().ok())
            .unwrap_or(-1) as u16,
        children,
        file_name: get("FileName")
            .and_then(|v| v.as_str().ok())
            .map(str::to_owned),
        morph: get("Morph")
            .and_then(|v| v.as_f32().ok())
            .unwrap_or(DEFAULT_MORPH),
        rate: first("FrameCtrl")
            .and_then(|o| float(o, "Rate"))
            .unwrap_or(1.0),
        start_frame: first("FrameCtrl")
            .and_then(|o| float(o, "StartFrame"))
            .unwrap_or(0.0),
        end_frame: first("FrameCtrl")
            .and_then(|o| float(o, "EndFrame"))
            .filter(|&frame| frame >= 0.0),
        foot_type: first("FrameCtrl")
            .and_then(|o| o.get("FootType"))
            .and_then(|v| v.as_int::<i32>().ok())
            .unwrap_or(0),
        loop_stop_count: first("FrameCtrl")
            .and_then(|o| float(o, "LoopStopCount"))
            .unwrap_or(-1.0),
        loop_stop_count_random: first("FrameCtrl")
            .and_then(|o| float(o, "LoopStopCountRandom"))
            .unwrap_or(0.0),
        ranges,
        strings: values("StringArray")
            .iter()
            .filter_map(|v| v.as_str().ok())
            .map(str::to_owned)
            .collect(),
        floats: values("FloatArray")
            .iter()
            .filter_map(|v| v.as_f32().ok())
            .collect(),
        ints: values("IntArray")
            .iter()
            .filter_map(|v| v.as_int::<i32>().ok())
            .collect(),
        sequence_loop: get("SequenceLoop")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false),
        input_limit: get("InputLimit")
            .and_then(|v| v.as_f32().ok())
            .filter(|&limit| limit >= 0.0),
        judge_once: get("JudgeOnce").and_then(|v| v.as_bool().ok()),
        no_sync: get("NoSync")
            .and_then(|v| v.as_bool().ok())
            .unwrap_or(false),
        bit_index: extend
            .and_then(|e| e.lists.get("BitIndex"))
            .and_then(|l| l.objects.get("BitIndex0"))
            .and_then(|o| o.get("TypeIndex"))
            .and_then(|v| v.as_int::<i32>().ok()),
        hold_events: extend
            .and_then(|e| e.lists.get("HoldEvents"))
            .map(|l| {
                (0..)
                    .map_while(|i| l.objects.get(format!("Event{i}").as_str()))
                    .map(|e| HoldEvent {
                        id: hold_event_id(
                            e.get("TypeIndex")
                                .and_then(|v| v.as_int::<i32>().ok())
                                .unwrap_or(-1),
                        ),
                        start_frame: float(e, "StartFrame").unwrap_or(0.0),
                        end_frame: float(e, "EndFrame").unwrap_or(0.0),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        trigger_events: extend
            .and_then(|e| e.lists.get("TriggerEvents"))
            .map(|l| {
                (0..)
                    .map_while(|i| l.objects.get(format!("Event{i}").as_str()))
                    .map(|e| TriggerEvent {
                        type_index: e
                            .get("TypeIndex")
                            .and_then(|v| v.as_int::<i32>().ok())
                            .unwrap_or(-1),
                        frame: float(e, "Frame").unwrap_or(0.0),
                        value: text(e, "Value").unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn float(object: &ParameterObject, name: &str) -> Option<f32> {
    object.get(name).and_then(|v| v.as_f32().ok())
}

fn text(object: &ParameterObject, name: &str) -> Option<String> {
    object
        .get(name)
        .and_then(|v| v.as_str().ok())
        .map(str::to_owned)
}

/// A cross-fade rule: switching from AS `pre` to one of `posts` takes that
/// many frames (an empty post name means any AS not in `excepts`).
#[derive(Clone, Debug, PartialEq)]
pub struct CrossFade {
    pub pre: String,
    pub excepts: Vec<String>,
    /// `(post AS, frames)`.
    pub posts: Vec<(String, f32)>,
}

/// A parsed `.baslist`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AsList {
    /// Animation BFRES files (e.g. `Player_Animation`).
    pub anim_files: Vec<String>,
    /// `(AS name, .bas file stem)`.
    pub defines: Vec<(String, String)>,
    pub cross_fades: Vec<CrossFade>,
}

impl AsList {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let io = ParameterIO::from_binary(bytes)
            .map_err(|_| FormatError::Invalid("baslist: not AAMP"))?;
        let root = &io.param_root;
        let anim_files = numbered(root.lists.get("AddReses"), "AddRes_")
            .filter_map(|o| text(o, "Anim"))
            .collect();
        let defines = numbered(root.lists.get("ASDefines"), "ASDefine_")
            .filter_map(|o| Some((text(o, "Name")?, text(o, "Filename")?)))
            .collect();
        let cross_fades = root
            .lists
            .get("CFDefines")
            .map(|l| {
                (0..)
                    .map_while(|i| l.lists.get(format!("CFDefine_{i}").as_str()))
                    .map(cross_fade)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            anim_files,
            defines,
            cross_fades,
        })
    }

    /// The `.bas` file stem of AS `name`.
    pub fn file_of(&self, name: &str) -> Option<&str> {
        self.defines
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, f)| f.as_str())
    }

    /// Frames to cross-fade from AS `pre` to `post`, if a rule sets it.
    pub fn cross_fade(&self, pre: &str, post: &str) -> Option<f32> {
        let rule = self.cross_fades.iter().find(|c| c.pre == pre)?;
        if rule.excepts.iter().any(|e| e == post) {
            return None;
        }
        let exact = rule.posts.iter().find(|(name, _)| name == post);
        exact
            .or_else(|| rule.posts.iter().find(|(name, _)| name.is_empty()))
            .map(|(_, frames)| *frames)
    }
}

/// Objects `<prefix>0`, `<prefix>1`, ... of a list, until one is missing.
fn numbered<'a>(
    list: Option<&'a ParameterList>,
    prefix: &'a str,
) -> impl Iterator<Item = &'a ParameterObject> {
    (0..).map_while(move |i| list?.objects.get(format!("{prefix}{i}").as_str()))
}

fn cross_fade(define: &ParameterList) -> CrossFade {
    CrossFade {
        pre: define
            .objects
            .get("CFPre")
            .and_then(|o| text(o, "Name"))
            .unwrap_or_default(),
        excepts: define
            .objects
            .get("CFExcepts")
            .map(|o| {
                o.iter()
                    .filter_map(|(_, v)| v.as_str().ok().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        posts: numbered(define.lists.get("CFPosts"), "CFPost_")
            .map(|o| {
                // A float in the game's list (`ASList::CFPost`).
                let frames = o
                    .get("Frame")
                    .and_then(|v| {
                        v.as_f32()
                            .ok()
                            .or_else(|| v.as_int::<i64>().ok().map(|i| i as f32))
                    })
                    .unwrap_or(0.0);
                (text(o, "Name").unwrap_or_default(), frames)
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use roead::aamp::{Parameter, ParameterIO, ParameterList, ParameterObject};

    use super::*;

    fn obj(pairs: &[(&str, Parameter)]) -> ParameterObject {
        let mut object = ParameterObject::new();
        for (name, value) in pairs {
            object.insert(*name, value.clone());
        }
        object
    }

    fn string(text: &str) -> Parameter {
        Parameter::StringRef(text.into())
    }

    fn document(root: ParameterList) -> Vec<u8> {
        let mut io = ParameterIO::new();
        io.param_root = root;
        io.to_binary()
    }

    #[test]
    fn parses_an_as_tree() {
        // A speed selector choosing between a walk and a run clip.
        let ranges = ParameterList::new()
            .with_object(
                "Range0",
                obj(&[("Start", Parameter::F32(0.0)), ("End", Parameter::F32(0.1))]),
            )
            .with_object(
                "Range1",
                obj(&[
                    ("Start", Parameter::F32(0.1)),
                    ("End", Parameter::F32(99.0)),
                ]),
            );
        let selector = ParameterList::new()
            .with_object("Parameters", obj(&[("TypeIndex", Parameter::I32(69))]))
            .with_object(
                "Children",
                obj(&[("Child0", Parameter::I32(1)), ("Child1", Parameter::I32(2))]),
            )
            .with_list("Extend", ParameterList::new().with_list("Ranges", ranges));
        let rate =
            ParameterList::new().with_object("FrameCtrl0", obj(&[("Rate", Parameter::F32(0.9))]));
        let walk = ParameterList::new()
            .with_object(
                "Parameters",
                obj(&[
                    ("TypeIndex", Parameter::I32(67)),
                    ("FileName", string("Walk")),
                    ("Morph", Parameter::F32(8.0)),
                ]),
            )
            .with_list("Extend", ParameterList::new().with_list("FrameCtrl", rate));
        let run = ParameterList::new().with_object(
            "Parameters",
            obj(&[
                ("TypeIndex", Parameter::I32(67)),
                ("FileName", string("Run")),
            ]),
        );
        let elements = ParameterList::new()
            .with_list("Element0", selector)
            .with_list("Element1", walk)
            .with_list("Element2", run);
        let seq = AnimSeq::parse(&document(
            ParameterList::new().with_list("Elements", elements),
        ))
        .unwrap();

        assert_eq!(seq.elements.len(), 3);
        assert_eq!(seq.elements[0].type_name(), "SpeedSelector");
        assert_eq!(seq.elements[0].children, vec![1, 2]);
        assert_eq!(seq.elements[0].ranges, vec![(0.0, 0.1), (0.1, 99.0)]);
        let walk = seq.clip("Walk").unwrap();
        assert_eq!((walk.morph, walk.rate), (8.0, 0.9));
        let run = seq.clip("Run").unwrap();
        assert_eq!((run.morph, run.rate), (DEFAULT_MORPH, 1.0));
        let names: Vec<_> = seq
            .clips(0)
            .iter()
            .filter_map(|e| e.file_name.clone())
            .collect();
        assert_eq!(names, ["Walk", "Run"]);
    }

    #[test]
    fn reads_hold_events_under_the_game_numbers() {
        // Player_Land.bas's turn-to-run landings hold event 11 over frames
        // 0–3; a type past 53 becomes 88, as `ASHoldEventsParser` makes it.
        let event = |type_index: i32, start: f32, end: f32| {
            obj(&[
                ("TypeIndex", Parameter::I32(type_index)),
                ("StartFrame", Parameter::F32(start)),
                ("EndFrame", Parameter::F32(end)),
                ("Value", string("")),
            ])
        };
        let holds = ParameterList::new()
            .with_object("Event0", event(11, 0.0, 3.0))
            .with_object("Event1", event(60, 1.0, 2.0));
        let asset = ParameterList::new()
            .with_object(
                "Parameters",
                obj(&[
                    ("TypeIndex", Parameter::I32(67)),
                    ("FileName", string("Nml_Move_Run_Land_TurnToRun_R180")),
                ]),
            )
            .with_list(
                "Extend",
                ParameterList::new().with_list("HoldEvents", holds),
            );
        let seq = AnimSeq::parse(&document(ParameterList::new().with_list(
            "Elements",
            ParameterList::new().with_list("Element0", asset),
        )))
        .unwrap();
        let events = &seq.elements[0].hold_events;
        assert_eq!(
            events,
            &[
                HoldEvent {
                    id: 11,
                    start_frame: 0.0,
                    end_frame: 3.0
                },
                HoldEvent {
                    id: 88,
                    start_frame: 1.0,
                    end_frame: 2.0
                },
            ]
        );
        assert_eq!((hold_event_id(53), hold_event_id(-1)), (53, 88));
    }

    #[test]
    fn reads_trigger_events_with_their_frames_and_values() {
        // Player_Wait.bas's stretch: its face AS at frame 0, the resting
        // face back at its last frame.
        let event = |frame: f32, value: &str| {
            obj(&[
                ("TypeIndex", Parameter::I32(7)),
                ("Frame", Parameter::F32(frame)),
                ("Value", string(value)),
            ])
        };
        let triggers = ParameterList::new()
            .with_object("Event0", event(0.0, "FaceRandomRelaxUpper"))
            .with_object("Event1", event(222.0, "FaceDefault"));
        let asset = ParameterList::new()
            .with_object(
                "Parameters",
                obj(&[
                    ("TypeIndex", Parameter::I32(67)),
                    ("FileName", string("Random_Nml_Wait_RelaxUpper")),
                ]),
            )
            .with_list(
                "Extend",
                ParameterList::new().with_list("TriggerEvents", triggers),
            );
        let seq = AnimSeq::parse(&document(ParameterList::new().with_list(
            "Elements",
            ParameterList::new().with_list("Element0", asset),
        )))
        .unwrap();
        let events = &seq.elements[0].trigger_events;
        let read: Vec<_> = events
            .iter()
            .map(|e| (e.type_index, e.frame, e.value.as_str()))
            .collect();
        assert_eq!(
            read,
            [(7, 0.0, "FaceRandomRelaxUpper"), (7, 222.0, "FaceDefault")]
        );
        assert!(seq.elements[0].hold_events.is_empty());
    }

    #[test]
    fn mixes_the_whole_tree_by_values_and_keys() {
        // Player_Land.bas in short: the weapon (by key), the landing speed,
        // then the stick: the standing landing or the foot ahead (by key).
        let mut foot = node("NodePosSelector", vec![5, 6], Vec::new(), None);
        foot.strings = vec!["Toe_R,Z".into(), "Toe_L,Z".into()];
        let mut weapon = node("WeaponSelector", vec![1, 1], Vec::new(), None);
        weapon.strings = vec!["WeaponSpear".into(), "default".into()];
        let seq = AnimSeq {
            elements: vec![
                weapon,
                node(
                    "SpeedBlender",
                    vec![2, 3],
                    vec![(0.0, 0.17), (0.01, 9999.0)],
                    None,
                ),
                clip("Nml_Wait_Land"),
                node(
                    "StickValueBlender",
                    vec![4, 7],
                    vec![(0.0, 0.99), (0.5, 9999.0)],
                    None,
                ),
                foot,
                clip("Run_Land_FootR"),
                clip("Run_Land_FootL"),
                clip("Run_Land_ToRun"),
            ],
        };
        let mix = |speed: f32, stick: f32, toe: &str| {
            let toe = toe.to_owned();
            seq.mix(0, &mut |_, e| match e.type_name() {
                "WeaponSelector" => Some(AsInput::Key("WeaponSmallSword".into())),
                "SpeedBlender" => Some(AsInput::Value(speed)),
                "StickValueBlender" => Some(AsInput::Value(stick)),
                "NodePosSelector" => Some(AsInput::Key(toe.clone())),
                _ => None,
            })
        };
        assert!(close(&mix(0.0, 0.0, "Toe_L,Z"), &[(2, 1.0)]));
        assert!(close(&mix(0.2, 0.0, "Toe_L,Z"), &[(6, 1.0)]));
        assert!(close(&mix(0.2, 1.0, "Toe_R,Z"), &[(7, 1.0)]));
        // Half way through both overlaps: four clips.
        assert!(close(
            &mix(0.09, 0.745, "Toe_R,Z"),
            &[(2, 0.5), (5, 0.25), (7, 0.25)]
        ));
        // A key no child has falls back to `default`; none at all, the first.
        assert!(close(&seq.mix(0, &mut |_, _| None), &[(2, 1.0)]));
    }

    #[test]
    fn reads_the_played_frame_range() {
        // One clip split between two AS: a turn to frame 4, then the rest
        // from frame 5; a pose holding frame 0; the default −1 end.
        let asset = |file: &str, frames: &[(&str, f32)]| {
            let frame_ctrl: Vec<_> = frames
                .iter()
                .map(|&(name, v)| (name, Parameter::F32(v)))
                .collect();
            ParameterList::new()
                .with_object(
                    "Parameters",
                    obj(&[
                        ("TypeIndex", Parameter::I32(67)),
                        ("FileName", string(file)),
                    ]),
                )
                .with_list(
                    "Extend",
                    ParameterList::new().with_list(
                        "FrameCtrl",
                        ParameterList::new().with_object("FrameCtrl0", obj(&frame_ctrl)),
                    ),
                )
        };
        let parse = |element: ParameterList| {
            let elements = ParameterList::new().with_list("Element0", element);
            AnimSeq::parse(&document(
                ParameterList::new().with_list("Elements", elements),
            ))
            .unwrap()
            .elements
            .remove(0)
        };
        let range = |e: Element| (e.start_frame, e.end_frame);

        assert_eq!(
            range(parse(asset("Turn", &[("EndFrame", 4.0)]))),
            (0.0, Some(4.0))
        );
        assert_eq!(
            range(parse(asset("Turn", &[("StartFrame", 5.0)]))),
            (5.0, None)
        );
        assert_eq!(
            range(parse(asset("Pose", &[("EndFrame", 0.0)]))),
            (0.0, Some(0.0))
        );
        assert_eq!(
            range(parse(asset("Whole", &[("EndFrame", -1.0)]))),
            (0.0, None)
        );
        let bare = ParameterList::new().with_object(
            "Parameters",
            obj(&[
                ("TypeIndex", Parameter::I32(67)),
                ("FileName", string("Bare")),
            ]),
        );
        assert_eq!(range(parse(bare)), (0.0, None));
    }

    fn node(
        type_name: &str,
        children: Vec<usize>,
        ranges: Vec<(f32, f32)>,
        file: Option<&str>,
    ) -> Element {
        Element {
            type_index: ELEMENT_TYPES.iter().position(|t| *t == type_name).unwrap() as u16,
            children,
            file_name: file.map(str::to_owned),
            morph: DEFAULT_MORPH,
            rate: 1.0,
            start_frame: 0.0,
            end_frame: None,
            foot_type: 0,
            loop_stop_count: -1.0,
            loop_stop_count_random: 0.0,
            ranges,
            strings: Vec::new(),
            floats: Vec::new(),
            ints: Vec::new(),
            sequence_loop: false,
            input_limit: None,
            judge_once: None,
            no_sync: false,
            bit_index: None,
            hold_events: Vec::new(),
            trigger_events: Vec::new(),
        }
    }

    fn clip(name: &str) -> Element {
        node("SkeltalAsset", Vec::new(), Vec::new(), Some(name))
    }

    fn close(weights: &[(usize, f32)], expected: &[(usize, f32)]) -> bool {
        let weight = |list: &[(usize, f32)], i: usize| {
            list.iter()
                .filter(|(c, _)| *c == i)
                .map(|(_, w)| w)
                .sum::<f32>()
        };
        let all: Vec<usize> = weights.iter().chain(expected).map(|(i, _)| *i).collect();
        all.iter()
            .all(|&i| (weight(weights, i) - weight(expected, i)).abs() < 1e-4)
    }

    #[test]
    fn blenders_cross_fade_neighbours_where_ranges_overlap() {
        // Player_Move.bas's run: turning right, straight, turning left.
        let curve = node(
            "NoLoopStickAngleBlender",
            vec![1, 2, 3],
            vec![(-180.0, 0.0), (-90.0, 90.0), (0.0, 180.0)],
            None,
        );
        assert!(close(&curve.blend_weights(0.0), &[(1, 1.0)]));
        assert!(close(&curve.blend_weights(-45.0), &[(0, 0.5), (1, 0.5)]));
        assert!(close(&curve.blend_weights(-120.0), &[(0, 1.0)]));
        assert!(close(
            &curve.blend_weights(60.0),
            &[(1, 1.0 / 3.0), (2, 2.0 / 3.0)]
        ));
        assert!(
            close(&curve.blend_weights(300.0), &[(2, 1.0)]),
            "past every range: the last"
        );
        assert!(
            close(&curve.blend_weights(-300.0), &[(0, 1.0)]),
            "before every range: the first"
        );
        // Under 0.01 of the overlap: the first alone, the second kept.
        assert_eq!(
            curve.pair(-89.5),
            Pair {
                a: 0,
                b: Some(1),
                t: 0.0
            }
        );
        // The slope blender: full uphill clip at -45 degrees, flat from -15.
        let slope = node(
            "GroundNormalBlender",
            vec![1, 2, 3],
            vec![(-45.0, -15.0), (-45.0, 45.0), (15.0, 45.0)],
            None,
        );
        assert!(close(&slope.blend_weights(-45.0), &[(0, 1.0)]));
        assert!(close(&slope.blend_weights(-30.0), &[(0, 0.5), (1, 0.5)]));
        assert!(close(&slope.blend_weights(5.0), &[(1, 1.0)]));
        // The glider's stick angle wraps: backwards fades into turning right.
        let glide = node(
            "StickAngleBlender",
            vec![1, 2, 3, 4],
            vec![(-177.5, -2.5), (-87.5, 87.5), (2.5, 177.5), (92.5, 267.5)],
            None,
        );
        assert!(close(&glide.blend_weights(180.0), &[(3, 1.0)]));
        assert!(close(&glide.blend_weights(-135.0), &[(3, 0.5), (0, 0.5)]));
        assert!(close(&glide.blend_weights(-90.0), &[(0, 1.0)]));
        // Backwards to the right: the back clip, a turn lower, pairs with
        // turning right (`0x038662a0`); to the left, turning left with it.
        let back_right = glide.pair(-100.0);
        assert_eq!((back_right.a, back_right.b), (3, Some(0)));
        assert!((back_right.t - 77.5 / 85.0).abs() < 1e-5);
        let back_left = glide.pair(100.0);
        assert_eq!((back_left.a, back_left.b), (2, Some(3)));
        assert!((back_left.t - 7.5 / 85.0).abs() < 1e-5);
        assert_eq!(glide.pair(460.0), glide.pair(100.0), "one turn round");
    }

    #[test]
    fn nodes_read_the_input_of_their_class() {
        let input = |kind: &str| node(kind, Vec::new(), Vec::new(), None).input();
        assert_eq!(input("StickValueBlender"), Some(0));
        assert_eq!(input("StickValueSelector"), Some(0));
        assert_eq!(input("NoLoopStickAngleBlender"), Some(7));
        assert_eq!(input("StickAngleBlender"), Some(6));
        assert_eq!(input("SizeBlender"), Some(17));
        assert_eq!(input("WallAngleBlender"), Some(15));
        assert_eq!(input("UserAngle2Blender"), Some(12));
        assert_eq!(input("BoneBlender"), None);
        assert_eq!(input("SkeltalAsset"), None);
        let angle = |kind: &str| node(kind, Vec::new(), Vec::new(), None).is_angle_blender();
        assert!(angle("StickAngleBlender") && !angle("NoLoopStickAngleBlender"));
    }

    #[test]
    fn selectors_pick_one_child_by_their_input() {
        // Player_ParaEquipOn.bas: a flag, then the fall speed in m/frame.
        let speed = vec![(-9999.0, -0.6), (-0.6, 9999.0)];
        let seq = AnimSeq {
            elements: vec![
                node("BoolSelector", vec![1, 4], Vec::new(), None),
                node("YSpeedSelector", vec![2, 3], speed.clone(), None),
                clip("Equip_Float_On_Fall"),
                clip("Equip_Float_On"),
                node("YSpeedSelector", vec![5, 6], speed, None),
                clip("Shield_Board_Equip_Para_Fall"),
                clip("Shield_Board_Equip_Para"),
            ],
        };
        let pick = |flag: f32, y_speed: f32| {
            let index = seq.select(0, &mut |_, e| match e.type_name() {
                "BoolSelector" => Some(flag),
                "YSpeedSelector" => Some(y_speed),
                _ => None,
            });
            index.and_then(|i| seq.elements[i].file_name.as_deref())
        };
        assert_eq!(pick(0.0, -0.7), Some("Equip_Float_On_Fall"));
        // A range holds its start, not its end.
        assert_eq!(pick(0.0, -0.6), Some("Equip_Float_On"));
        assert_eq!(pick(1.0, -1.0), Some("Shield_Board_Equip_Para_Fall"));
        // The last range's end still counts; past it nothing is chosen.
        assert_eq!(pick(0.0, 9999.0), Some("Equip_Float_On"));
        assert_eq!(pick(0.0, 10000.0), None);
        assert_eq!(seq.select(0, &mut |_, _| None), None, "no input");
    }

    #[test]
    fn angle_selectors_wrap_round() {
        // Player_Move.bas after `Brake`: nothing ahead, a start behind.
        let back = node(
            "StickAngleSelector",
            vec![1, 2],
            vec![(-90.0, 90.0), (90.0, 270.0)],
            None,
        );
        assert_eq!(back.select(0.0), Some(0));
        assert_eq!(back.select(-89.0), Some(0));
        assert_eq!(back.select(90.0), Some(1));
        assert_eq!(back.select(-90.5), Some(1), "a turn lower first");
        assert_eq!(back.select(-90.0), Some(0), "the lowered range ends there");
        assert_eq!(back.select(-179.0), Some(1));
        assert_eq!(back.select(200.0), Some(1), "brought within ±180");
        // After `Land`: [-1, 1) straight on, anything else turning.
        let turned = node(
            "StickAngleSelector",
            vec![1, 2],
            vec![(-1.0, 1.0), (1.0, 359.0)],
            None,
        );
        assert_eq!(turned.select(0.5), Some(0));
        assert_eq!(turned.select(-1.0), Some(0));
        assert_eq!(turned.select(-1.5), Some(1));
        assert_eq!(turned.select(1.0), Some(1));
        // A plain range selector does not wrap.
        let plain = node(
            "SpeedSelector",
            vec![1, 2],
            vec![(-90.0, 90.0), (90.0, 270.0)],
            None,
        );
        assert_eq!(plain.select(-120.0), None);
    }

    #[test]
    fn node_pos_selector_takes_the_bone_furthest_along() {
        // `Player_Link_Brake.bas` out of a dash: the left toe against the
        // left ankle, not left against right.
        let mut dash = node("NodePosSelector", vec![1, 2], Vec::new(), None);
        dash.strings = vec!["Toe_L,Z".into(), "Ankle_L,Z".into()];
        let pose = |toe_z: f32| {
            move |bone: &str| match bone {
                "Toe_L" => Some([0.1, 0.05, toe_z]),
                "Ankle_L" => Some([0.1, 0.1, 0.0]),
                _ => None,
            }
        };
        assert_eq!(dash.select_node_pos(&mut pose(0.12)), Some(0));
        assert_eq!(dash.select_node_pos(&mut pose(-0.05)), Some(1));
        assert_eq!(
            dash.select_node_pos(&mut pose(0.0)),
            Some(0),
            "the first on a tie"
        );
        // An unknown bone or axis leaves its child out.
        let mut odd = node("NodePosSelector", vec![1, 2], Vec::new(), None);
        odd.strings = vec!["Toe_L,W".into(), "Hip,Z".into()];
        assert_eq!(odd.select_node_pos(&mut pose(1.0)), None);
    }

    #[test]
    fn blends_the_tree_around_a_clip() {
        // A stick-value blender (walk or run, no input given) over the run's
        // turn blender, over the slope blender holding the plain run.
        let seq = AnimSeq {
            elements: vec![
                node("SequencePlayContainer", vec![1], Vec::new(), None),
                node(
                    "StickValueBlender",
                    vec![2, 3],
                    vec![(0.0, 0.99), (0.5, 1.0)],
                    None,
                ),
                clip("Walk"),
                node(
                    "NoLoopStickAngleBlender",
                    vec![4, 5, 6],
                    vec![(-180.0, 0.0), (-90.0, 90.0), (0.0, 180.0)],
                    None,
                ),
                clip("Run_Curve_R"),
                node(
                    "GroundNormalBlender",
                    vec![7, 8, 9],
                    vec![(-45.0, -15.0), (-45.0, 45.0), (15.0, 45.0)],
                    None,
                ),
                clip("Run_Curve_L"),
                clip("Run_Up"),
                clip("Run"),
                clip("Run_Down"),
            ],
        };
        let base = seq.clip_index("Run").unwrap();
        let blend = seq.blend(base, &mut |_, e| match e.type_name() {
            "NoLoopStickAngleBlender" => Some(-45.0),
            "GroundNormalBlender" => Some(-30.0),
            _ => None,
        });
        assert!(
            close(&blend, &[(4, 0.5), (7, 0.25), (8, 0.25)]),
            "{blend:?}"
        );
        let alone = seq.blend(base, &mut |_, _| None);
        assert!(close(&alone, &[(8, 1.0)]), "no inputs: the clip itself");
    }

    #[test]
    fn parses_an_as_list_with_cross_fades() {
        let posts = ParameterList::new()
            .with_object(
                "CFPost_0",
                obj(&[("Name", string("Move")), ("Frame", Parameter::F32(3.0))]),
            )
            .with_object(
                "CFPost_1",
                obj(&[("Name", string("")), ("Frame", Parameter::I32(8))]),
            );
        let fade = ParameterList::new()
            .with_object("CFPre", obj(&[("Name", string("Land"))]))
            .with_object("CFExcepts", obj(&[("Name_0", string("Jump"))]))
            .with_list("CFPosts", posts);
        let root = ParameterList::new()
            .with_list(
                "AddReses",
                ParameterList::new()
                    .with_object("AddRes_0", obj(&[("Anim", string("Player_Animation"))])),
            )
            .with_list(
                "ASDefines",
                ParameterList::new().with_object(
                    "ASDefine_0",
                    obj(&[
                        ("Name", string("Move")),
                        ("Filename", string("Player_Move")),
                    ]),
                ),
            )
            .with_list(
                "CFDefines",
                ParameterList::new().with_list("CFDefine_0", fade),
            );
        let list = AsList::parse(&document(root)).unwrap();

        assert_eq!(list.anim_files, ["Player_Animation"]);
        assert_eq!(list.file_of("Move"), Some("Player_Move"));
        assert_eq!(list.cross_fade("Land", "Move"), Some(3.0));
        assert_eq!(list.cross_fade("Land", "Wait"), Some(8.0), "any other AS");
        assert_eq!(list.cross_fade("Land", "Jump"), None, "excepted");
        assert_eq!(list.cross_fade("Move", "Land"), None);
    }
}
