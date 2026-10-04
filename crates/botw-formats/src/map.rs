//! Map units: where actors are placed. MainField is split into 1000 m cells
//! `A-1` … `J-8` (columns A–J west to east from x = −5000, rows 1–8 north to
//! south from z = −4000). Each cell has a `_Static` unit (in
//! `Pack/TitleBG.pack`) and a `_Dynamic` unit (loose in `Map/MainField/<cell>/`,
//! newest in the update). Both are Yaz0-compressed BYML with an `Objs` array.

use std::collections::BTreeMap;

use roead::byml::Byml;

use crate::content::ContentRoots;
use crate::{FormatError, Result};

/// A placed actor.
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedActor {
    /// Actor name, e.g. `Enemy_Bokoblin_Junior`.
    pub name: String,
    pub hash_id: u32,
    /// World position.
    pub translate: [f32; 3],
    /// Rotation in radians: a single Y angle, or X, Y, Z (applied in that
    /// order, like sead's `makeR`; see `ksys::map::MubinIter::getRotate`).
    pub rotate: [f32; 3],
    /// Scale (uniform scales are repeated).
    pub scale: [f32; 3],
    /// Scalar actor parameters (`!Parameters`), stringified.
    pub params: BTreeMap<String, String>,
}

/// Parses a (possibly Yaz0-compressed) map unit.
pub fn parse_unit(bytes: &[u8]) -> Result<Vec<PlacedActor>> {
    let data = crate::yaz0::decompress_if(bytes)?;
    let doc = Byml::from_binary(&data[..]).map_err(|_| FormatError::Invalid("map unit: not BYML"))?;
    let Ok(root) = doc.as_map() else { return Err(FormatError::Invalid("map unit: root is not a map")) };
    let Some(Byml::Array(objs)) = root.get("Objs") else { return Ok(Vec::new()) };
    Ok(objs.iter().filter_map(|obj| actor(obj.as_map().ok()?)).collect())
}

fn actor(obj: &roead::byml::Map) -> Option<PlacedActor> {
    let name = obj.get("UnitConfigName")?.as_string().ok()?.to_string();
    let hash_id = obj.get("HashId").and_then(|v| v.as_u32().ok()).unwrap_or(0);
    let translate = vec3(obj.get("Translate")?)?;
    let rotate = match obj.get("Rotate") {
        Some(Byml::Float(y)) => [0.0, *y, 0.0],
        Some(value) => vec3(value).unwrap_or_default(),
        None => [0.0; 3],
    };
    let scale = match obj.get("Scale") {
        Some(Byml::Float(s)) => [*s; 3],
        Some(value) => vec3(value).unwrap_or([1.0; 3]),
        None => [1.0; 3],
    };
    let params = match obj.get("!Parameters") {
        Some(Byml::Map(map)) => map
            .iter()
            .filter_map(|(key, value)| {
                let text = match value {
                    Byml::String(s) => s.to_string(),
                    Byml::Bool(b) => b.to_string(),
                    Byml::I32(i) => i.to_string(),
                    Byml::U32(u) => u.to_string(),
                    Byml::Float(f) => f.to_string(),
                    _ => return None,
                };
                Some((key.to_string(), text))
            })
            .collect(),
        _ => BTreeMap::new(),
    };
    Some(PlacedActor { name, hash_id, translate, rotate, scale, params })
}

fn vec3(value: &Byml) -> Option<[f32; 3]> {
    match value {
        Byml::Array(v) if v.len() == 3 => Some([v[0].as_float().ok()?, v[1].as_float().ok()?, v[2].as_float().ok()?]),
        Byml::Map(m) => Some([m.get("X")?.as_float().ok()?, m.get("Y")?.as_float().ok()?, m.get("Z")?.as_float().ok()?]),
        _ => None,
    }
}

/// Name of the MainField cell containing world `(x, z)`, e.g. `E-4`, or
/// `None` outside the playable map.
pub fn cell_name(x: f32, z: f32) -> Option<String> {
    let column = ((x + 5000.0) / 1000.0).floor();
    let row = ((z + 4000.0) / 1000.0).floor();
    ((0.0..10.0).contains(&column) && (0.0..8.0).contains(&row))
        .then(|| format!("{}-{}", (b'A' + column as u8) as char, row as u8 + 1))
}

/// Reads MainField map units from the content roots.
pub struct MainFieldUnits {
    /// `Pack/TitleBG.pack`, which holds the static units; read once.
    title_bg: Option<Vec<u8>>,
    roots: ContentRoots,
}

impl MainFieldUnits {
    pub fn new(roots: ContentRoots) -> Result<Self> {
        let title_bg = match roots.find("Pack/TitleBG.pack") {
            Some(path) => Some(std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?),
            None => None,
        };
        Ok(Self { title_bg, roots })
    }

    /// Every actor in a cell's static and dynamic units.
    pub fn cell(&self, cell: &str) -> Result<Vec<PlacedActor>> {
        let mut actors = Vec::new();
        if let Some(pack) = &self.title_bg {
            let sarc = roead::sarc::Sarc::new(&pack[..])?;
            if let Some(data) = sarc.get_data(&format!("Map/MainField/{cell}/{cell}_Static.smubin")) {
                actors.extend(parse_unit(data)?);
            }
        }
        if let Some(path) = self.roots.find(format!("Map/MainField/{cell}/{cell}_Dynamic.smubin")) {
            let bytes = std::fs::read(&path).map_err(|source| FormatError::Io { path, source })?;
            actors.extend(parse_unit(&bytes)?);
        }
        Ok(actors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roead::byml::Byml;

    #[test]
    fn names_cells_like_the_game() {
        assert_eq!(cell_name(-4999.0, -3999.0).as_deref(), Some("A-1"));
        assert_eq!(cell_name(-1100.0, 1900.0).as_deref(), Some("D-6"));
        assert_eq!(cell_name(4999.0, 3999.0).as_deref(), Some("J-8"));
        assert_eq!(cell_name(5001.0, 0.0), None);
    }

    #[test]
    fn parses_objs() {
        let obj = |name: &str, rotate: Byml| {
            let mut map = roead::byml::Map::default();
            map.insert("UnitConfigName".into(), Byml::String(name.into()));
            map.insert("HashId".into(), Byml::U32(7));
            map.insert("Translate".into(), Byml::Array(vec![Byml::Float(1.0), Byml::Float(2.0), Byml::Float(3.0)]));
            map.insert("Rotate".into(), rotate);
            let mut params = roead::byml::Map::default();
            params.insert("DropTable".into(), Byml::String("Normal".into()));
            map.insert("!Parameters".into(), Byml::Map(params));
            Byml::Map(map)
        };
        let mut root = roead::byml::Map::default();
        root.insert(
            "Objs".into(),
            Byml::Array(vec![
                obj("Enemy_Bokoblin_Junior", Byml::Float(90.0)),
                obj("Obj_Tree", Byml::Array(vec![Byml::Float(1.0), Byml::Float(2.0), Byml::Float(3.0)])),
            ]),
        );
        let bytes = Byml::Map(root).to_binary(roead::Endian::Big);
        let actors = parse_unit(&bytes).unwrap();
        assert_eq!(actors.len(), 2);
        assert_eq!(actors[0].name, "Enemy_Bokoblin_Junior");
        assert_eq!((actors[0].hash_id, actors[0].translate, actors[0].rotate), (7, [1.0, 2.0, 3.0], [0.0, 90.0, 0.0]));
        assert_eq!(actors[0].params["DropTable"], "Normal");
        assert_eq!(actors[1].rotate, [1.0, 2.0, 3.0]);
    }
}
