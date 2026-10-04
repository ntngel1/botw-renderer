//! The AI class definitions (`Actor/AIDef/AIDef_Game.product.sbyml` in
//! `Pack/Bootup.pack`): for every AI, Action, Behavior and Query class its
//! children's names (`childs`), its parameters and flags
//! (docs/research/wiiu-ai-framework.md, «Tree construction»).
//!
//! Only what the file holds is read here; how the game turns it into
//! `AIDefSet`/`AIDef` (`AIClassDef::getDef` `0x037bb544`, `doGetDef`
//! `0x037ba63c`) is ported in the game layer.

use std::collections::HashMap;

use roead::byml::Byml;

use crate::content::ContentRoots;
use crate::{FormatError, Result};

/// The AIDef file inside `Pack/Bootup.pack`.
pub const AIDEF_FILE: &str = "Actor/AIDef/AIDef_Game.product.sbyml";

/// One parameter of a class: the `Name` and `Type` of an entry of
/// `StaticInstParams` (or the other parameter lists); `None` where the
/// entry lacks the key or it is not a string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiDefParam {
    pub name: Option<String>,
    pub ty: Option<String>,
}

/// One class of the AIDef.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiDefClass {
    /// `childs`, in file order; `None` for an entry that is not a string.
    /// A `childs` that is not an array has no children: the game reads it
    /// as a node and counts only arrays and hashes (`0x0396ce3c`); the
    /// dump's are all `""`.
    pub children: Vec<Option<String>>,
    /// `StaticInstParams`, in file order.
    pub static_params: Vec<AiDefParam>,
    /// `TriggerAction`; false if absent.
    pub trigger_action: bool,
    /// `DynamicParamChild`; false if absent.
    pub dynamic_param_child: bool,
}

/// The four class lists of the AIDef, by class name. A class whose entry
/// is not a hash (`""` in the dump) is present with nothing.
#[derive(Clone, Debug, Default)]
pub struct AiDef {
    pub ais: HashMap<String, AiDefClass>,
    pub actions: HashMap<String, AiDefClass>,
    pub behaviors: HashMap<String, AiDefClass>,
    pub queries: HashMap<String, AiDefClass>,
}

impl AiDef {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let doc = Byml::from_binary(bytes).map_err(|_| FormatError::Invalid("AIDef: not BYML"))?;
        let root = doc
            .as_map()
            .map_err(|_| FormatError::Invalid("AIDef: not a hash"))?;
        let list = |key: &str| -> HashMap<String, AiDefClass> {
            let Some(Byml::Map(classes)) = root.get(key) else {
                return HashMap::new();
            };
            classes
                .iter()
                .map(|(name, entry)| (name.to_string(), class(entry)))
                .collect()
        };
        Ok(Self {
            ais: list("AIs"),
            actions: list("Actions"),
            behaviors: list("Behaviors"),
            queries: list("Querys"),
        })
    }

    /// Reads the AIDef from `Pack/Bootup.pack`; `None` if the dump has no
    /// such pack.
    pub fn load(roots: &ContentRoots) -> Result<Option<Self>> {
        let Some(path) = roots.find("Pack/Bootup.pack") else {
            return Ok(None);
        };
        let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
        let pack = crate::yaz0::decompress_if(&pack)?;
        let sarc = roead::sarc::Sarc::new(&pack[..])?;
        let data = sarc
            .get_data(AIDEF_FILE)
            .ok_or(FormatError::Invalid("Bootup.pack lacks the AIDef"))?;
        Self::parse(&crate::yaz0::decompress_if(data)?).map(Some)
    }
}

fn text(value: Option<&Byml>) -> Option<String> {
    match value {
        Some(Byml::String(s)) => Some(s.to_string()),
        _ => None,
    }
}

fn flag(entry: &roead::byml::Map, key: &str) -> bool {
    matches!(entry.get(key), Some(Byml::Bool(true)))
}

fn class(entry: &Byml) -> AiDefClass {
    let Byml::Map(entry) = entry else {
        return AiDefClass::default();
    };
    let children = match entry.get("childs") {
        Some(Byml::Array(names)) => names.iter().map(|n| text(Some(n))).collect(),
        _ => Vec::new(),
    };
    let static_params = match entry.get("StaticInstParams") {
        // An entry that is not a hash is left out, as the game skips it
        // (`doGetDef` `0x037ba63c`).
        Some(Byml::Array(params)) => params
            .iter()
            .filter_map(|p| match p {
                Byml::Map(p) => Some(AiDefParam {
                    name: text(p.get("Name")),
                    ty: text(p.get("Type")),
                }),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    AiDefClass {
        children,
        static_params,
        trigger_action: flag(entry, "TriggerAction"),
        dynamic_param_child: flag(entry, "DynamicParamChild"),
    }
}

#[cfg(test)]
mod tests {
    use roead::byml::{Byml, Map};

    use super::*;

    fn map(pairs: Vec<(&str, Byml)>) -> Byml {
        let mut m = Map::default();
        for (k, v) in pairs {
            m.insert(k.into(), v);
        }
        Byml::Map(m)
    }

    fn param(name: &str, ty: &str) -> Byml {
        map(vec![
            ("Name", Byml::String(name.into())),
            ("Type", Byml::String(ty.into())),
        ])
    }

    #[test]
    fn reads_children_parameters_and_flags() {
        let ais = map(vec![
            (
                "PlayerNormal",
                map(vec![
                    (
                        "childs",
                        Byml::Array(vec![
                            Byml::String("着地".into()),
                            Byml::String("落下".into()),
                        ]),
                    ),
                    (
                        "StaticInstParams",
                        Byml::Array(vec![param("ToFallHeightForJustRush", "Float")]),
                    ),
                    ("DynamicParamChild", Byml::Bool(true)),
                ]),
            ),
            ("DemoRootAI", map(vec![("childs", Byml::String("".into()))])),
        ]);
        let actions = map(vec![
            ("PlayerLand", Byml::String("".into())),
            (
                "DummyTriggerAction",
                map(vec![("TriggerAction", Byml::Bool(true))]),
            ),
        ]);
        let doc = map(vec![("AIs", ais), ("Actions", actions)]);
        let def = AiDef::parse(&doc.to_binary(roead::Endian::Big)).unwrap();

        let normal = &def.ais["PlayerNormal"];
        assert_eq!(
            normal.children,
            vec![Some("着地".to_owned()), Some("落下".to_owned())]
        );
        assert_eq!(normal.static_params.len(), 1);
        assert_eq!(
            normal.static_params[0].name.as_deref(),
            Some("ToFallHeightForJustRush")
        );
        assert_eq!(normal.static_params[0].ty.as_deref(), Some("Float"));
        assert!(normal.dynamic_param_child && !normal.trigger_action);
        assert!(def.ais["DemoRootAI"].children.is_empty());
        assert_eq!(def.actions["PlayerLand"], AiDefClass::default());
        assert!(def.actions["DummyTriggerAction"].trigger_action);
        assert!(def.behaviors.is_empty() && def.queries.is_empty());
        assert!(AiDef::parse(b"not byml").is_err());
    }
}
