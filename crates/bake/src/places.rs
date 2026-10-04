//! Named places: the game's location markers in the field-wide map unit
//! (`Pack/Bootup.pack` → `Map/MainField/Static.smubin`).

use asset_format::{Place, Places};
use botw_formats::content::ContentRoots;
use roead::byml::Byml;

const BOOTUP_PACK: &str = "Pack/Bootup.pack";
const FIELD_STATIC: &str = "Map/MainField/Static.smubin";

/// Every `LocationMarker` with a `MessageID`, named by it in lower case.
pub fn read(roots: &ContentRoots) -> Result<Places, String> {
    let path = roots
        .find(BOOTUP_PACK)
        .ok_or("Pack/Bootup.pack not found")?;
    let pack = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let pack = roead::sarc::Sarc::new(&pack[..]).map_err(|e| format!("Bootup.pack: {e}"))?;
    let unit = pack
        .get_data(FIELD_STATIC)
        .ok_or("Bootup.pack has no Map/MainField/Static.smubin")?;
    let unit = botw_formats::yaz0::decompress_if(unit).map_err(|e| e.to_string())?;
    let doc = Byml::from_binary(&unit[..]).map_err(|e| format!("Static.smubin: {e}"))?;
    let map = doc.as_map().map_err(|_| "Static.smubin: not a map")?;
    let Some(Byml::Array(markers)) = map.get("LocationMarker") else {
        return Err("Static.smubin: no LocationMarker list".into());
    };
    let mut places: Vec<Place> = markers
        .iter()
        .filter_map(|marker| {
            let m = marker.as_map().ok()?;
            let id = m.get("MessageID")?.as_string().ok()?.clone();
            (!id.is_empty()).then(|| Place {
                name: id.to_lowercase(),
                position: translate(m.get("Translate")),
                marker: id.to_string(),
            })
        })
        .collect();
    places.sort_by(|a, b| a.name.cmp(&b.name));
    places.dedup_by(|a, b| a.name == b.name);
    Ok(Places { places })
}

fn translate(value: Option<&Byml>) -> [f32; 3] {
    match value {
        Some(Byml::Array(v)) if v.len() == 3 => {
            std::array::from_fn(|i| v[i].as_float().unwrap_or(0.0))
        }
        Some(Byml::Map(m)) => {
            ["X", "Y", "Z"].map(|k| m.get(k).and_then(|v| v.as_float().ok()).unwrap_or(0.0))
        }
        _ => [0.0; 3],
    }
}
