//! The game's `grass_color` map (docs/research/wiiu-field-shading.md,
//! "The `grass_color` map"): the mean colour of the terrain's two materials
//! under the grass, blended as the ground blends them, on a 3 m grid of
//! 47×47 cells around the camera. The blade shader mixes it into the dark
//! part of each blade, so the roots take the ground's colour.
//!
//! Game: the grid (3 m, 47 cells, wrapping), the texel (two palette colours
//! mixed by `(blend − 1)/254`, byte by byte), RGBA8 UNORM. Approximation: the
//! palette is each material's mean linear colour over level 0 (the game
//! draws the layer into a small target on the GPU, most likely its 1×1
//! mip), cells are world-aligned and sampled at their centres (the game's
//! grid origin is not recovered), and the texture is filtered linearly.
//! `BOTW_GRASS_COLOR=data` keeps the map's grass colour instead (the
//! earlier stand-in, for comparison: docs/CHOICES.md, GRASS-LIGHT-001).

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::heights::{HeightSampler, MaterialAt};
use crate::terrain_material::{TerrainAlbedo, TerrainLook};

/// Cell edge in metres (`0x102e9584`).
pub const CELL: f32 = 3.0;
/// Cells along each side (`0x035af354`).
pub const CELLS: i32 = 47;

/// The map and what it takes to fill it.
#[derive(Resource)]
pub struct GrassColorMap {
    pub image: Handle<Image>,
    pub use_baked_color: bool,
    palette: Palette,
    /// The cell the camera was in for the last fill.
    center: Option<IVec2>,
    fill: Option<Task<Vec<u8>>>,
    /// The map holds the game's colours; until then blades keep the map's
    /// grass colour.
    pub ready: bool,
}

enum Palette {
    /// Waiting for the terrain's textures.
    Waiting,
    Computing(Task<Vec<[u8; 4]>>),
    Ready(Arc<Vec<[u8; 4]>>),
    /// No terrain textures (no dump): the blades keep the map's colour.
    Unavailable,
}

impl FromWorld for GrassColorMap {
    fn from_world(world: &mut World) -> Self {
        let mut image = Image::new_fill(
            Extent3d {
                width: CELLS as u32,
                height: CELLS as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0, 0, 0, 255],
            TextureFormat::Rgba8Unorm,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        );
        // SI-GRS-08: grass palette from the level 0 mean.
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        });
        let image = world.resource_mut::<Assets<Image>>().add(image);
        Self {
            image,
            use_baked_color: std::env::var("BOTW_GRASS_COLOR").is_ok_and(|v| v == "data"),
            palette: Palette::Waiting,
            center: None,
            fill: None,
            ready: false,
        }
    }
}

impl GrassColorMap {
    pub fn set_baked_color(&mut self, enabled: bool) {
        if self.use_baked_color == enabled {
            return;
        }
        self.use_baked_color = enabled;
        self.palette = Palette::Waiting;
        self.fill = None;
        self.center = None;
        self.ready = false;
    }

    /// Filled for the camera's cell, or known to stay unused.
    pub fn is_settled(&self) -> bool {
        match self.palette {
            Palette::Unavailable => true,
            Palette::Ready(_) => self.ready && self.fill.is_none(),
            Palette::Waiting | Palette::Computing(_) => false,
        }
    }
}

/// One cell's colour: the two materials' palette colours mixed by the
/// blend, byte by byte (`0x030bfba0`: truncated), with `t = (blend − 1)/254`
/// (the sampled blend is one up on a sample, see `HeightSampler::material_at`).
pub fn cell_color(palette: &[[u8; 4]], at: MaterialAt) -> [u8; 4] {
    let last = palette.len().saturating_sub(1);
    let (Some(a), Some(b)) = (
        palette.get((at.material0 as usize).min(last)),
        palette.get((at.material1 as usize).min(last)),
    ) else {
        return [0, 0, 0, 255];
    };
    let t = ((at.blend as f32 - 1.0) / 254.0).clamp(0.0, 1.0);
    std::array::from_fn(|c| ((b[c] as f32 - a[c] as f32) * t + a[c] as f32) as u8)
}

/// The palette: each material's mean colour (linear, as RGBA8 bytes).
fn palette(albedo: &TerrainAlbedo, layers: Vec<u32>) -> Vec<[u8; 4]> {
    let means = albedo.layer_means();
    layers
        .iter()
        .map(|&layer| {
            let mean = means.get(layer as usize).copied().unwrap_or([0.0; 3]);
            let [r, g, b] = mean.map(|c| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            [r, g, b, 255]
        })
        .collect()
}

/// The map's texels for the cells around `center` (texel = cell index
/// modulo 47, so the map wraps as the camera moves).
fn fill(sampler: &HeightSampler, palette: &[[u8; 4]], center: IVec2) -> Vec<u8> {
    let mut data = vec![0u8; (CELLS * CELLS * 4) as usize];
    let half = CELLS / 2;
    for j in center.y - half..=center.y + half {
        for i in center.x - half..=center.x + half {
            let (x, z) = ((i as f32 + 0.5) * CELL, (j as f32 + 0.5) * CELL);
            let color = sampler
                .material_at(x, z)
                .map_or([0, 0, 0, 255], |at| cell_color(palette, at));
            let texel = (j.rem_euclid(CELLS) * CELLS + i.rem_euclid(CELLS)) as usize;
            data[texel * 4..][..4].copy_from_slice(&color);
        }
    }
    data
}

pub(super) fn update_color_map(
    mut map: ResMut<GrassColorMap>,
    grass: Res<super::Grass>,
    look: Option<Res<TerrainLook>>,
    albedo: Option<Res<TerrainAlbedo>>,
    cameras: Query<&GlobalTransform, crate::camera::MainCamera>,
    mut images: ResMut<Assets<Image>>,
) {
    let map = &mut *map;
    match &mut map.palette {
        Palette::Waiting if map.use_baked_color => {
            map.palette = Palette::Unavailable;
        }
        Palette::Waiting => match (look.as_deref(), albedo) {
            (Some(TerrainLook::Textured { table, .. }), Some(albedo)) => {
                let layers = table.entries.iter().map(|e| e.z as u32).collect();
                let albedo = (*albedo).clone();
                map.palette = Palette::Computing(
                    AsyncComputeTaskPool::get().spawn(async move { palette(&albedo, layers) }),
                );
            }
            (None | Some(TerrainLook::Plain), _) => map.palette = Palette::Unavailable,
            _ => {}
        },
        Palette::Computing(task) => {
            if let Some(palette) = block_on(poll_once(task)) {
                map.palette = Palette::Ready(Arc::new(palette));
            }
        }
        Palette::Ready(_) | Palette::Unavailable => {}
    }
    let Palette::Ready(palette) = &map.palette else {
        return;
    };

    if let Some(task) = &mut map.fill
        && let Some(data) = block_on(poll_once(task))
    {
        map.fill = None;
        if let Some(mut image) = images.get_mut(&map.image) {
            image.data = Some(data);
        }
        map.ready = true;
    }
    let Ok(camera) = cameras.single() else { return };
    let center = (camera.translation().xz() / CELL).floor().as_ivec2();
    if map.fill.is_none() && map.center != Some(center) {
        map.center = Some(center);
        let (sampler, palette) = (grass.sampler.clone(), palette.clone());
        map.fill = Some(
            AsyncComputeTaskPool::get().spawn(async move { fill(&sampler, &palette, center) }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_mix_the_two_materials_by_the_blend() {
        let palette = [[100, 200, 0, 255], [200, 0, 50, 255]];
        let at = |material0, material1, blend| MaterialAt {
            material0,
            material1,
            blend,
        };
        // On a sample the blend is one up: 1 is all the first, 255 the second.
        assert_eq!(cell_color(&palette, at(0, 1, 1)), [100, 200, 0, 255]);
        assert_eq!(cell_color(&palette, at(0, 1, 0)), [100, 200, 0, 255]);
        assert_eq!(cell_color(&palette, at(0, 1, 255)), [200, 0, 50, 255]);
        assert_eq!(cell_color(&palette, at(0, 1, 128)), [150, 100, 25, 255]);
        // Indices past the palette take its last colour, as the game clamps.
        assert_eq!(cell_color(&palette, at(9, 9, 1)), [200, 0, 50, 255]);
    }
}
