//! `botw::look`: the game's stylized light and air, shared by every
//! surface shader (terrain, objects, grass, water, characters, far trees).
//!
//! The values change every frame with the time of day, the climate and the
//! weather. Pushing them into every material asset would re-prepare
//! thousands of bind groups a frame (wave 2 lost ~100 ms a frame that way),
//! so they live in one tiny texture instead: [`LOOK_TEXELS`] RGBA texels in a
//! row, created once ([`LookTexture`]) and bound by every material that uses
//! the look. [`upload_look`] rewrites its texels from the [`Look`] resource
//! when that changes, and the noise, the sky table and the sky's cover
//! when they change: the render world writes just those rows into the same
//! GPU texture ([`write_look_rows`]), so the materials' bind groups stay
//! valid, no material asset changes, and the sky table's rows (most of the
//! texture) go to the GPU only when it is baked.
//!
//! # Filling the values (Rust)
//!
//! Systems of the light and air tracks write the [`Look`] resource (e.g. from
//! `daynight::Sky` and `daynight::Environment::renderer`) and run before
//! [`LookSystems::Upload`] in `PostUpdate`:
//!
//! ```ignore
//! app.add_systems(PostUpdate, fog_the_look.before(look::LookSystems::Upload));
//! fn fog_the_look(sky: Res<Sky>, mut look: ResMut<Look>) {
//!     look.haze = ...;
//! }
//! ```
//!
//! Each track owns one section of four texels (see [`Look`]); add named
//! fields inside your section, keep [`Look::texels`] and the `Look` struct
//! in `look.wgsl` in step, and leave the other sections alone. The defaults
//! are an exact identity: shaders that call the functions look the same.
//!
//! # Binding it in a material (WGSL + Rust)
//!
//! Non-bindless material (e.g. `TerrainExtension`, which shows the pattern):
//!
//! ```ignore
//! #[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
//! pub struct MyExtension {
//!     // ... your bindings ...
//!     /// The shared look values (`look::LookTexture`).
//!     #[texture(110)]
//!     pub look: Handle<Image>,
//! }
//! // when creating the material:  look: look_texture.0.clone()
//! ```
//! ```wgsl
//! #import botw::look::{read_look, diffuse_gain, apply_haze}
//! @group(#{MATERIAL_BIND_GROUP}) @binding(110) var look_texture: texture_2d<f32>;
//! let look = read_look(look_texture);
//! ```
//!
//! Bindless material (`ObjectShading`, an extension of the bindless
//! `StandardMaterial`): the texture joins the bindless index table
//! like any other texture. The table's own binding and the data array sit
//! right after the range, so they move up by one. For `ObjectShading`
//! (textures 101–103, table at 104, data at 105):
//!
//! ```ignore
//! #[data(100, ObjectParams, binding_array(106))]            // was 105
//! #[bindless(index_table(range(100..105), binding(105)))]   // was 100..104, 104
//! pub struct ObjectShading {
//!     // ... 100–103 as before ...
//!     #[texture(104)]
//!     pub look: Handle<Image>,
//! }
//! ```
//! ```wgsl
//! struct ObjectIndices { params: u32, shadow_map: u32, shadow_sampler: u32, mask: u32, look: u32 }
//! @group(#{MATERIAL_BIND_GROUP}) @binding(105) var<storage> object_indices: array<ObjectIndices>;
//! @group(#{MATERIAL_BIND_GROUP}) @binding(106) var<storage> object_params: array<ObjectParams>;
//! #else
//! @group(#{MATERIAL_BIND_GROUP}) @binding(104) var look_texture: texture_2d<f32>;
//! ...
//! #ifdef BINDLESS
//!     let look = read_look(bindless_textures_2d[indices.look]);
//! #else
//!     let look = read_look(look_texture);
//! #endif
//! ```
//!
//! (Checked: this renders on Metal, and a value changed every frame reaches
//! the trees without a frame-rate cost.)
//!
//! A material made without the texture (`Handle::default()`, Bevy's 1 × 1
//! default image) reads the identity (`read_look` checks the width).
//!
//! The texture is `Rgba16Float` (filterable everywhere, which bindless
//! texture arrays require): values up to 65 504 with ~3 significant digits.
//! Keep tiny coefficients scaled (e.g. fog density per km, not per metre).

use std::sync::Arc;

use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    TextureDimension, TextureFormat, TextureUsages,
};
use bevy::render::renderer::RenderQueue;
use bevy::render::texture::GpuImage;
use bevy::render::{Extract, ExtractSchedule, Render, RenderApp, RenderSystems};

const SHADER: Handle<Shader> = uuid_handle!("3e7b1c94-6a2d-4f58-b0e3-9c41d5a7f862");

/// Texels in the look texture (`LOOK_TEXELS` in `look.wgsl`).
pub const LOOK_TEXELS: usize = 28;

/// Side of the fog's height noise (the game's `cloud_noise`), kept in rows
/// 1–64 of the look texture under the look's texels (`LOOK_NOISE` in
/// `look.wgsl`).
pub const LOOK_NOISE: usize = 64;

/// The fog's height noise, one byte per texel, row by row
/// ([`LOOK_NOISE`]²); without the game's texture a flat middle grey (no
/// shift). `fog.rs` loads it.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct LookNoise(pub Vec<u8>);

/// Side of the game's sky table (`sky_lut.rs`), kept in rows
/// [`LOOK_SKY_ROW`]… of the look texture (`LOOK_SKY` in `look.wgsl`).
pub const LOOK_SKY: usize = 256;
/// First row of the sky table (`LOOK_SKY_ROW` in `look.wgsl`).
pub const LOOK_SKY_ROW: usize = 1 + LOOK_NOISE;
/// Side of the field's map of the sky's cover (`sky_occlusion.rs`), kept in
/// rows [`LOOK_COVER_ROW`]… under the sky table (`LOOK_COVER` in
/// `look.wgsl`).
pub const LOOK_COVER: usize = 96;
/// First row of the map of the sky's cover (`LOOK_COVER_ROW` in
/// `look.wgsl`).
pub const LOOK_COVER_ROW: usize = LOOK_SKY_ROW + LOOK_SKY;
/// First row of the volume mask (`volume_mask.rs`), under the map of the
/// sky's cover (`LOOK_MASK_ROW` in `look.wgsl`). The render world draws it
/// there every frame; the CPU never writes these rows.
pub const LOOK_MASK_ROW: usize = LOOK_COVER_ROW + LOOK_COVER;
/// Room for the volume mask: its texels across and down (a view of up to
/// 4096 × 2304 pixels at 1/8 of its size).
pub const LOOK_MASK_SIZE: [usize; 2] = [512, 288];
/// Texels across the look texture: the widest of its parts (the mask).
pub const LOOK_WIDTH: usize = LOOK_MASK_SIZE[0];
/// Rows of the look texture.
const LOOK_ROWS: usize = LOOK_MASK_ROW + LOOK_MASK_SIZE[1];

/// The sky table baked from the game's (`sky_lut.rs`), [`LOOK_SKY`]² RGBA
/// texels row by row; `None` without the game's data (the texture's rows
/// stay black and the haze keeps its fitted colour).
#[derive(Resource, Clone, Debug, Default)]
pub struct LookSkyLut(pub Option<std::sync::Arc<Vec<[f32; 4]>>>);

/// The height (m) of what covers the sky above the field around the
/// camera, [`LOOK_COVER`]² texels row by row, west to east and north to
/// south (`sky_occlusion.rs`, which also fills [`Look::sky_occlusion`]);
/// `None` without one (the rows stay 0: open).
#[derive(Resource, Clone, Debug, Default)]
pub struct LookCover(pub Option<std::sync::Arc<Vec<f32>>>);

impl Default for LookNoise {
    fn default() -> Self {
        Self(vec![128; LOOK_NOISE * LOOK_NOISE])
    }
}

pub struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "look.wgsl", Shader::from_wgsl);
        let look = Look::default();
        let noise = LookNoise::default();
        let texture = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(look_image(&look.texels(), &noise));
        app.insert_resource(LookTexture(texture))
            .insert_resource(look)
            .insert_resource(noise)
            .init_resource::<LookSkyLut>()
            .init_resource::<LookCover>()
            .init_resource::<LookRows>()
            .add_plugins(ExtractResourcePlugin::<LookTexture>::default())
            .add_systems(PostUpdate, upload_look.in_set(LookSystems::Upload));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<PendingLookRows>()
            .add_systems(ExtractSchedule, extract_look_rows)
            .add_systems(
                Render,
                write_look_rows.in_set(RenderSystems::PrepareResources),
            );
    }
}

/// Rows of the look texture rewritten this frame: the first row of each
/// run and the run's bytes (whole rows).
#[derive(Resource, Clone, Default)]
pub struct LookRows(Vec<(u32, Arc<[u8]>)>);

/// Rows extracted and not yet written (the texture may not be on the GPU
/// yet), the latest bytes of each run.
#[derive(Resource, Default)]
struct PendingLookRows(Vec<(u32, Arc<[u8]>)>);

fn extract_look_rows(rows: Extract<Res<LookRows>>, mut pending: ResMut<PendingLookRows>) {
    if !rows.is_changed() {
        return;
    }
    for (first, bytes) in &rows.0 {
        pending.0.retain(|(f, _)| f != first);
        pending.0.push((*first, bytes.clone()));
    }
}

/// Writes the pending rows into the look texture, once it is on the GPU
/// (after `RenderSystems::PrepareAssets`, before the frame is drawn).
fn write_look_rows(
    texture: Option<Res<LookTexture>>,
    images: Res<RenderAssets<GpuImage>>,
    queue: Res<RenderQueue>,
    mut pending: ResMut<PendingLookRows>,
) {
    let Some(image) = texture.and_then(|t| images.get(&t.0)) else {
        return;
    };
    let row_bytes = LOOK_WIDTH * TEXEL_BYTES;
    for (first, bytes) in pending.0.drain(..) {
        let rows = (bytes.len() / row_bytes) as u32;
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &image.texture,
                mip_level: 0,
                origin: Origin3d {
                    x: 0,
                    y: first,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            &bytes,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_bytes as u32),
                rows_per_image: Some(rows),
            },
            Extent3d {
                width: LOOK_WIDTH as u32,
                height: rows,
                depth_or_array_layers: 1,
            },
        );
    }
}

/// Where the look texture is written; systems filling [`Look`] run before.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LookSystems {
    Upload,
}

/// The look texture every material that uses `botw::look` binds.
#[derive(Resource, Clone, ExtractResource)]
pub struct LookTexture(pub Handle<Image>);

/// The shared look values, one section of four texels per wave-3 track
/// (`docs/STYLE.md`). The default changes nothing.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Look {
    // Texels 0–3: light (track 1).
    /// Gain on lit surfaces after `apply_pbr_lighting` (texel 0.x; 1 = none).
    pub diffuse_gain: f32,
    /// Texels 4–7: air and haze (track 2).
    pub haze: [Vec4; 4],
    /// Texels 8–11: where the map of the sky's cover lies (`sky_occlusion.rs`,
    /// see [`LookCover`]): 8 its north-west corner (x in xy, z in zw, each as
    /// a whole part and the rest, for precision), 9.x its side (m), 9.w 1
    /// when there is a map.
    pub sky_occlusion: [Vec4; 4],
    /// Texels 12–15: the height fog (`fog.rs`).
    pub spare: [Vec4; 4],
    /// Texels 16–19: the ad hoc fog (`fog.rs`, `AdhocFog::texels`).
    pub adhoc: [Vec4; 4],
    /// Texels 20–23: the volume mask (`volume_mask.rs`): 20.xy its size
    /// (texels), 20.z the main view's `clip_from_view[1][1]`, 20.w 1 when
    /// it is drawn into rows [`LOOK_MASK_ROW`]….
    pub volume_mask: [Vec4; 4],
    /// Texels 24–27: the wet ground (`precipitation.rs`): 20.x
    /// `uking_dynamic_rain_ratio`, 20.y `uking_dynamic_rainfall`, 20.z the
    /// rain pulse's clock (`gsys_context[20].y`, frames wrapped at 120)
    /// as `time·0.125` wrapped to [0, 1).
    pub weather: [Vec4; 4],
}

impl Default for Look {
    fn default() -> Self {
        Self {
            diffuse_gain: 1.0,
            haze: [Vec4::ZERO; 4],
            sky_occlusion: [Vec4::ZERO; 4],
            spare: [Vec4::ZERO; 4],
            adhoc: [Vec4::ZERO; 4],
            volume_mask: [Vec4::ZERO; 4],
            weather: [Vec4::ZERO; 4],
        }
    }
}

impl Look {
    /// The texels as the shader reads them (`Look` in `look.wgsl`).
    pub fn texels(&self) -> [Vec4; LOOK_TEXELS] {
        let mut texels = [Vec4::ZERO; LOOK_TEXELS];
        texels[0] = Vec4::new(self.diffuse_gain, 0.0, 0.0, 0.0);
        texels[4..8].copy_from_slice(&self.haze);
        texels[8..12].copy_from_slice(&self.sky_occlusion);
        texels[12..16].copy_from_slice(&self.spare);
        texels[16..20].copy_from_slice(&self.adhoc);
        texels[20..24].copy_from_slice(&self.volume_mask);
        texels[24..28].copy_from_slice(&self.weather);
        texels
    }
}

/// The look texture for `texels` and the fog's noise, without a sky table.
fn look_image(texels: &[Vec4; LOOK_TEXELS], noise: &LookNoise) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: LOOK_WIDTH as u32,
            height: LOOK_ROWS as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        texel_bytes(texels, noise),
        TextureFormat::Rgba16Float,
        // Uploaded once; later changes go to the GPU row by row.
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    // The volume mask is drawn into its rows (`volume_mask.rs`).
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    image
}

/// Bytes of one texel.
const TEXEL_BYTES: usize = 4 * 2;

/// Row 0: the look's texels; rows 1–64: the noise in red, in the first 64
/// columns; rows [`LOOK_SKY_ROW`]…: the sky table (black until baked);
/// rows [`LOOK_COVER_ROW`]…: the sky's cover (0 until there is a map);
/// rows [`LOOK_MASK_ROW`]…: the volume mask (0 until drawn).
fn texel_bytes(texels: &[Vec4; LOOK_TEXELS], noise: &LookNoise) -> Vec<u8> {
    let mut bytes = vec![0; LOOK_WIDTH * LOOK_ROWS * TEXEL_BYTES];
    write_texels(&mut bytes, texels);
    write_noise(&mut bytes, noise);
    bytes
}

fn put(bytes: &mut [u8], at: usize, texel: [f32; 4]) {
    for (c, v) in texel.into_iter().enumerate() {
        let i = at * TEXEL_BYTES + 2 * c;
        bytes[i..i + 2].copy_from_slice(&f16_bits(v).to_le_bytes());
    }
}

fn write_texels(bytes: &mut [u8], texels: &[Vec4; LOOK_TEXELS]) {
    for (x, t) in texels.iter().enumerate() {
        put(bytes, x, t.to_array());
    }
}

fn write_noise(bytes: &mut [u8], noise: &LookNoise) {
    for (i, &v) in noise.0.iter().enumerate() {
        let (x, y) = (i % LOOK_NOISE, i / LOOK_NOISE);
        put(
            bytes,
            (1 + y) * LOOK_WIDTH + x,
            [f32::from(v) / 255.0, 0.0, 0.0, 0.0],
        );
    }
}

fn write_sky(bytes: &mut [u8], sky: &[[f32; 4]]) {
    for (i, &t) in sky.iter().take(LOOK_SKY * LOOK_SKY).enumerate() {
        let (x, y) = (i % LOOK_SKY, i / LOOK_SKY);
        put(bytes, (LOOK_SKY_ROW + y) * LOOK_WIDTH + x, t);
    }
}

/// The cover's heights, each as its whole metres in red and the rest in
/// green (half floats hold whole numbers exactly up to 2048).
fn write_cover(bytes: &mut [u8], cover: Option<&[f32]>) {
    for i in 0..LOOK_COVER * LOOK_COVER {
        let (x, y) = (i % LOOK_COVER, i / LOOK_COVER);
        let h = cover.and_then(|c| c.get(i)).copied().unwrap_or(0.0);
        put(
            bytes,
            (LOOK_COVER_ROW + y) * LOOK_WIDTH + x,
            [h.floor(), h - h.floor(), 0.0, 0.0],
        );
    }
}

/// Writes [`Look`], the noise, the sky table and the sky's cover into the
/// look texture's bytes, each when it changed, and hands the rows that
/// changed to the render world ([`LookRows`]).
pub fn upload_look(
    look: Res<Look>,
    noise: Res<LookNoise>,
    sky: Res<LookSkyLut>,
    cover: Res<LookCover>,
    mut rows: ResMut<LookRows>,
    mut bytes: Local<Vec<u8>>,
    mut written: Local<Option<[Vec4; LOOK_TEXELS]>>,
) {
    let texels = look.texels();
    let texels_changed = look.is_changed() && *written != Some(texels);
    if !texels_changed && !noise.is_changed() && !sky.is_changed() && !cover.is_changed() {
        return;
    }
    let size = LOOK_WIDTH * LOOK_ROWS * TEXEL_BYTES;
    if bytes.len() != size {
        *bytes = vec![0; size];
    }
    let row_bytes = LOOK_WIDTH * TEXEL_BYTES;
    let mut changed = Vec::new();
    let mut take = |bytes: &[u8], first: usize, count: usize| {
        let run = &bytes[first * row_bytes..(first + count) * row_bytes];
        changed.push((first as u32, Arc::from(run)));
    };
    write_texels(&mut bytes, &texels);
    take(&bytes, 0, 1);
    if noise.is_changed() {
        write_noise(&mut bytes, &noise);
        take(&bytes, 1, LOOK_NOISE);
    }
    if sky.is_changed() {
        match &sky.0 {
            Some(table) => write_sky(&mut bytes, table),
            None => bytes[LOOK_SKY_ROW * row_bytes..LOOK_COVER_ROW * row_bytes].fill(0),
        }
        take(&bytes, LOOK_SKY_ROW, LOOK_SKY);
    }
    if cover.is_changed() {
        write_cover(&mut bytes, cover.0.as_deref().map(Vec::as_slice));
        take(&bytes, LOOK_COVER_ROW, LOOK_COVER);
    }
    rows.0 = changed;
    *written = Some(texels);
}

/// `value` as IEEE half-precision bits (round to nearest even; out of range
/// becomes infinity, tiny values subnormal or zero).
fn f16_bits(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x007f_ffff;
    if exponent == 0xff {
        // Infinity or NaN (keep NaN a NaN).
        return sign | 0x7c00 | if mantissa != 0 { 0x200 } else { 0 };
    }
    let half_exponent = exponent - 127 + 15;
    if half_exponent >= 0x1f {
        return sign | 0x7c00;
    }
    if half_exponent <= 0 {
        if half_exponent < -10 {
            return sign;
        }
        // Subnormal: shift the mantissa with its implicit bit into place.
        let full = mantissa | 0x0080_0000;
        let shift = (14 - half_exponent) as u32;
        let half = full >> shift;
        let rest = full & ((1 << shift) - 1);
        let halfway = 1 << (shift - 1);
        let round = u32::from(rest > halfway || (rest == halfway && half & 1 == 1));
        return sign | (half + round) as u16;
    }
    let half = ((half_exponent as u32) << 10) | (mantissa >> 13);
    let rest = mantissa & 0x1fff;
    let round = u32::from(rest > 0x1000 || (rest == 0x1000 && half & 1 == 1));
    // A carry out of the mantissa correctly bumps the exponent (up to infinity).
    sign | (half + round) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_round_trip_common_values() {
        assert_eq!(f16_bits(0.0), 0x0000);
        assert_eq!(f16_bits(-0.0), 0x8000);
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(-2.0), 0xc000);
        assert_eq!(f16_bits(65504.0), 0x7bff);
        assert_eq!(f16_bits(1.0e6), 0x7c00);
        assert_eq!(f16_bits(f32::NAN) & 0x7c00, 0x7c00);
        // 1 + 2^-11 lies halfway between 1 and the next half; ties go to even.
        assert_eq!(f16_bits(1.0 + 1.0 / 2048.0), 0x3c00);
        assert_eq!(f16_bits(1.0 + 3.0 / 2048.0), 0x3c02);
        // The smallest subnormal and the largest one.
        assert_eq!(f16_bits(2.0f32.powi(-24)), 0x0001);
        assert_eq!(f16_bits(2.0f32.powi(-15)), 0x0200);
        assert_eq!(f16_bits(2.0f32.powi(-30)), 0x0000);
    }

    #[test]
    fn default_look_is_the_identity_layout() {
        let texels = Look::default().texels();
        assert_eq!(texels[0], Vec4::new(1.0, 0.0, 0.0, 0.0));
        assert!(texels[1..].iter().all(|t| *t == Vec4::ZERO));
        let bytes = texel_bytes(&texels, &LookNoise::default());
        assert_eq!(
            bytes.len(),
            LOOK_WIDTH * (1 + LOOK_NOISE + LOOK_SKY + LOOK_COVER + LOOK_MASK_SIZE[1]) * 4 * 2
        );
        // Texel 0 red = 1.0 in half precision, little-endian.
        assert_eq!(&bytes[..2], &[0x00, 0x3c]);
        // The noise starts on row 1, in red: without the game's, 128/255.
        let row = LOOK_WIDTH * 4 * 2;
        assert_eq!(&bytes[row..row + 2], &f16_bits(128.0 / 255.0).to_le_bytes());
        // Its 64 columns only; the sky table's rows stay black.
        assert!(bytes[row + LOOK_NOISE * 8..2 * row].iter().all(|&b| b == 0));
        assert!(bytes[LOOK_SKY_ROW * row..].iter().all(|&b| b == 0));
    }

    #[test]
    fn sky_table_lands_under_the_noise() {
        let mut bytes = texel_bytes(&Look::default().texels(), &LookNoise::default());
        let mut sky = vec![[0.0; 4]; LOOK_SKY * LOOK_SKY];
        sky[LOOK_SKY + 3] = [2.0, 0.0, 0.0, 1.0];
        write_sky(&mut bytes, &sky);
        let at = ((LOOK_SKY_ROW + 1) * LOOK_WIDTH + 3) * 8;
        assert_eq!(&bytes[at..at + 2], &f16_bits(2.0).to_le_bytes());
    }

    #[test]
    fn sections_land_in_their_texels() {
        let look = Look {
            diffuse_gain: 2.0,
            haze: [Vec4::splat(4.0), Vec4::ZERO, Vec4::ZERO, Vec4::splat(7.0)],
            sky_occlusion: [Vec4::splat(8.0); 4],
            spare: [Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, Vec4::splat(15.0)],
            adhoc: [Vec4::splat(16.0), Vec4::ZERO, Vec4::ZERO, Vec4::ZERO],
            volume_mask: [Vec4::splat(20.0), Vec4::ZERO, Vec4::ZERO, Vec4::ZERO],
            weather: [Vec4::splat(24.0), Vec4::ZERO, Vec4::ZERO, Vec4::ZERO],
        };
        let texels = look.texels();
        assert_eq!(texels[0].x, 2.0);
        assert_eq!(
            (texels[4], texels[7], texels[8], texels[15]),
            (
                Vec4::splat(4.0),
                Vec4::splat(7.0),
                Vec4::splat(8.0),
                Vec4::splat(15.0)
            )
        );
        assert_eq!(texels[16], Vec4::splat(16.0));
        assert_eq!(texels[20], Vec4::splat(20.0));
        assert_eq!(texels[24], Vec4::splat(24.0));
    }

    #[test]
    fn shader_rows_match() {
        let shader = include_str!("look.wgsl");
        for line in [
            format!("const LOOK_TEXELS: u32 = {LOOK_TEXELS}u;"),
            format!("const LOOK_MASK_ROW: i32 = LOOK_COVER_ROW + LOOK_COVER;"),
        ] {
            assert!(shader.contains(&line), "{line}");
        }
        assert_eq!(LOOK_MASK_ROW, LOOK_COVER_ROW + LOOK_COVER);
    }

    #[test]
    fn cover_lands_under_the_sky_table_in_two_parts() {
        let mut bytes = texel_bytes(&Look::default().texels(), &LookNoise::default());
        let mut cover = vec![0.0; LOOK_COVER * LOOK_COVER];
        cover[LOOK_COVER + 2] = 987.654;
        write_cover(&mut bytes, Some(&cover));
        let at = ((LOOK_COVER_ROW + 1) * LOOK_WIDTH + 2) * 8;
        assert_eq!(&bytes[at..at + 2], &f16_bits(987.0).to_le_bytes());
        let rest = f16_bits(0.654);
        assert_eq!(&bytes[at + 2..at + 4], &rest.to_le_bytes());
        // The sky table's rows are left alone.
        assert!(
            bytes[LOOK_SKY_ROW * LOOK_WIDTH * 8..LOOK_COVER_ROW * LOOK_WIDTH * 8]
                .iter()
                .all(|&b| b == 0)
        );
    }
}
