//! Where an actor's effects live: the actor pack's `ActorLink` names its
//! ELink user (`ElinkUser`); the ELink database (`ELink2/ELink2DB.sbelnk`
//! in `Pack/Bootup.pack`, see `crate::xlink`) says which emitter sets the
//! user plays, when, and with what scale, offset and colour; the sets are
//! in the effect file named after the user (`Effect/<user>.sesetlist`,
//! loose in the update). Textures marked resident are in
//! `Effect/GameResident.sesetlist`, also in `Pack/Bootup.pack`.

use roead::aamp::ParameterIO;

use crate::content::ContentRoots;
use crate::xlink::{AssetCall, Value, XLink};
use crate::{FormatError, Result};

/// The pack with the shared effect file and the ELink database.
pub const BOOTUP_PACK: &str = "Pack/Bootup.pack";
/// The shared effect file with the resident textures and meshes.
pub const RESIDENT_FILE: &str = "Effect/GameResident.sesetlist";
pub const ELINK_DB: &str = "ELink2/ELink2DB.sbelnk";

/// `Pack/Bootup.pack`, read once.
pub struct Bootup(Vec<u8>);

impl Bootup {
    pub fn read(roots: &ContentRoots) -> Result<Option<Self>> {
        let Some(path) = roots.find(BOOTUP_PACK) else {
            return Ok(None);
        };
        let pack = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
        let pack = crate::yaz0::decompress_if(&pack)?.into_owned();
        roead::sarc::Sarc::new(&pack[..])?;
        Ok(Some(Self(pack)))
    }

    /// An entry, decompressed.
    pub fn entry(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let sarc = roead::sarc::Sarc::new(&self.0[..])?;
        match sarc.get_data(name) {
            Some(data) => Ok(Some(crate::yaz0::decompress_if(data)?.into_owned())),
            None => Ok(None),
        }
    }
}

/// An emitter set an ELink user plays all the time, with the asset's
/// parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct AlwaysEffect {
    pub set: String,
    pub scale: f32,
    pub offset: [f32; 3],
    pub color: [f32; 4],
}

/// The emitter sets `user` plays unconditionally: the assets keyed
/// `Always` at the top of its call table. (Assets under containers play
/// only when a property, such as the weather, selects them.)
pub fn always_effects(db: &XLink, user: &str) -> Result<Vec<AlwaysEffect>> {
    let Some(calls) = db.user(user)? else {
        return Ok(Vec::new());
    };
    Ok(calls
        .iter()
        .filter(|c| !c.is_container && c.parent_index < 0 && c.key == "Always")
        .filter_map(always_effect)
        .collect())
}

fn always_effect(call: &AssetCall) -> Option<AlwaysEffect> {
    let Some(Value::Str(set)) = call.param("RuntimeAssetName") else {
        return None;
    };
    // SI-FMT-13: particle emitter field meanings from a third-party reader.
    // Random values count as the middle of their range; values following
    // a property (curves) as the default.
    let number =
        |name: &str, default: f32| call.param(name).and_then(Value::as_f32).unwrap_or(default);
    Some(AlwaysEffect {
        set: set.clone(),
        scale: number("Scale", 1.0),
        offset: [
            number("PositionX", 0.0),
            number("PositionY", 0.0),
            number("PositionZ", 0.0),
        ],
        color: [
            number("Red", 1.0),
            number("Green", 1.0),
            number("Blue", 1.0),
            number("Alpha", 1.0),
        ],
    })
}

/// The ELink user an actor's `ActorLink` (`Actor/ActorLink/*.bxml`, AAMP)
/// names, unless it is `Dummy`.
pub fn elink_user(actor_link: &[u8]) -> Result<Option<String>> {
    let io = ParameterIO::from_binary(actor_link)
        .map_err(|_| FormatError::Invalid("ActorLink: not AAMP"))?;
    let user = io
        .param_root
        .objects
        .get("LinkTarget")
        .and_then(|target| target.get("ElinkUser"))
        .and_then(|value| value.as_str().ok())
        .map(str::to_owned);
    Ok(user.filter(|u| !u.is_empty() && u != "Dummy"))
}

/// The decompressed effect file `Effect/<name>.sesetlist`, if the content
/// roots have one.
pub fn read_effect_file(roots: &ContentRoots, name: &str) -> Result<Option<Vec<u8>>> {
    let Some(path) = roots.find(format!("Effect/{name}.sesetlist")) else {
        return Ok(None);
    };
    let bytes = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
    Ok(Some(crate::yaz0::decompress_if(&bytes)?.into_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use roead::aamp::{Parameter, ParameterObject};

    fn actor_link(user: &str) -> Vec<u8> {
        let mut io = ParameterIO::new();
        let mut target = ParameterObject::new();
        target.insert("ElinkUser", Parameter::StringRef(user.into()));
        target.insert("ModelUser", Parameter::StringRef("None".into()));
        io.param_root.objects.insert("LinkTarget", target);
        io.to_binary()
    }

    #[test]
    fn finds_what_a_user_always_plays() {
        let bytes = crate::xlink::tests::sample();
        let db = XLink::parse(&bytes).unwrap();
        let played = always_effects(&db, "Test").unwrap();
        assert_eq!(
            played,
            [AlwaysEffect {
                set: "Test_Set".into(),
                scale: 2.5,
                offset: [0.0, 2.0, 0.0],
                color: [1.0; 4]
            }]
        );
        assert!(always_effects(&db, "Nobody").unwrap().is_empty());
    }

    #[test]
    fn reads_the_elink_user() {
        assert_eq!(
            elink_user(&actor_link("MountainCloud")).unwrap().as_deref(),
            Some("MountainCloud")
        );
        assert_eq!(elink_user(&actor_link("Dummy")).unwrap(), None);
        assert!(elink_user(b"not aamp").is_err());
    }
}
