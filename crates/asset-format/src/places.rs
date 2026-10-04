//! Named places (`places.ron`): the game's location markers, and the
//! regions baked around them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Places {
    pub places: Vec<Place>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Place {
    /// Our name, e.g. `hateno`.
    pub name: String,
    /// The marker's `MessageID` in the game's map (e.g. `Hateno`).
    pub marker: String,
    /// The marker's position, world units (+X east, +Y up, +Z south).
    pub position: [f32; 3],
}

impl Places {
    pub fn find(&self, name: &str) -> Option<&Place> {
        self.places
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(name) || p.marker.eq_ignore_ascii_case(name))
    }
}
