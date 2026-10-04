//! The game's grass interaction maps (docs/research/wiiu-field-shading.md,
//! "Mowing and flattening"): around the camera the `InteractMap` manager
//! (`0x035a1530`) keeps `grass_mow` (256² R8G8 over 64 m), `grass_mow_wide`
//! (512² RGBA8, a ring over 512 m that remembers what was cut) and
//! `grass_lie` (256² R8G8 over 64 m: grass pressed flat, standing back up
//! by 0.95 a frame). The grass shaders read them through the transforms
//! `gsys_environment` 34–36 (`uv = xz·scale + offset`).
//!
//! The game draws its requests into the maps on the GPU (`tera_grass.sharcb`,
//! `interact_*`); the viewer draws the same shapes on the CPU and uploads
//! what changed. Mowing keeps the lowest value in the request type's
//! channel; the lie is a signed vector, seeds keeping the strongest push
//! each way. Where the world's statistics maps hide the grass (`hidden`),
//! the green of both mow maps goes to 0 as the 64 m map reaches it
//! (`interact_cut_array`). Approximation: the 64 m maps move with the
//! camera in whole texels (the game shifts the mow map by the exact
//! offset).

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use std::collections::HashSet;

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::hidden::HiddenGrass;

/// Texels along a side of the 64 m maps (`FUN_035a1c10`, `FUN_035a2048`).
pub const NEAR_TEXELS: i32 = 256;
/// Their span in metres (`2 × 32`, manager `+0x10`).
pub const NEAR_SPAN: f32 = 64.0;
/// Texels along a side of `grass_mow_wide` (`FUN_035a1e38`), one a metre.
pub const WIDE_TEXELS: i32 = 512;
/// The wide ring's centre moves in 8 m steps (`FUN_035a4500`).
pub const WIDE_SNAP: f32 = 8.0;
/// What is left of a lie each game frame (lie `+0x884`, `FUN_035a5780`).
pub const LIE_KEEP: f32 = 0.95;
/// A mow request's disc is half its `r` across: the shared quad runs ±0.5
/// (sead's unit quad; the lie seeds scale theirs by `2r` for a radius `r`,
/// the cut by `r`; docs/research/wiiu-field-shading.md, mowing).
pub const CUT_DISC: f32 = 0.5;
/// A body's footprint fades out over this margin (`DAT_104726b0`).
pub const FOOTPRINT_EDGE: f32 = 1.15;

/// A square window of texels following a point, `channels` floats a texel.
/// Texel (i, j) covers world `(origin + (i, j))·texel` to one texel on.
struct Window {
    texels: i32,
    texel: f32,
    channels: usize,
    origin: IVec2,
    data: Vec<f32>,
    clear: f32,
}

impl Window {
    fn new(texels: i32, span: f32, channels: usize, clear: f32) -> Self {
        Self {
            texels,
            texel: span / texels as f32,
            channels,
            origin: IVec2::splat(-texels / 2),
            data: vec![clear; (texels * texels) as usize * channels],
            clear,
        }
    }

    fn index(&self, i: i32, j: i32) -> usize {
        (j * self.texels + i) as usize * self.channels
    }

    /// Moves the window to have `center` in its middle texel; texels that
    /// come in are cleared. The shift in texels, if it moved.
    // SI-GRS-06: sword grass interaction windows and pressing are ours.
    fn follow(&mut self, center: Vec2) -> Option<IVec2> {
        let origin = (center / self.texel).floor().as_ivec2() - IVec2::splat(self.texels / 2);
        let shift = origin - self.origin;
        if shift == IVec2::ZERO {
            return None;
        }
        let mut data = Vec::with_capacity(self.data.len());
        for j in 0..self.texels {
            for i in 0..self.texels {
                let (si, sj) = (i + shift.x, j + shift.y);
                if (0..self.texels).contains(&si) && (0..self.texels).contains(&sj) {
                    let at = self.index(si, sj);
                    data.extend_from_slice(&self.data[at..at + self.channels]);
                } else {
                    data.extend(std::iter::repeat_n(self.clear, self.channels));
                }
            }
        }
        self.data = data;
        self.origin = origin;
        Some(shift)
    }

    /// `(scale, offset)` of the shaders' `uv = xz·scale + offset`.
    fn transform(&self) -> Vec4 {
        let scale = 1.0 / (self.texels as f32 * self.texel);
        let offset = -self.origin.as_vec2() / self.texels as f32;
        Vec4::new(scale, scale, offset.x, offset.y)
    }

    /// The world point at a texel's centre.
    fn point(&self, i: i32, j: i32) -> Vec2 {
        (self.origin + IVec2::new(i, j)).as_vec2() * self.texel + self.texel * 0.5
    }

    /// Calls `texel(values, world point)` for every texel whose centre is
    /// within the square `center ± half`. Whether any.
    fn stamp(&mut self, center: Vec2, half: f32, mut texel: impl FnMut(&mut [f32], Vec2)) -> bool {
        let lo = ((center - half) / self.texel).floor().as_ivec2() - self.origin;
        let hi = ((center + half) / self.texel).ceil().as_ivec2() - self.origin;
        let mut any = false;
        for j in lo.y.max(0)..hi.y.min(self.texels) {
            for i in lo.x.max(0)..hi.x.min(self.texels) {
                let point = self.point(i, j);
                let at = self.index(i, j);
                texel(&mut self.data[at..at + self.channels], point);
                any = true;
            }
        }
        any
    }

    fn bytes(&self) -> Vec<u8> {
        self.data.iter().map(|v| unorm8(*v)).collect()
    }
}

fn unorm8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The wide map: a ring of 512 × 512 one-metre texels, indexed by world
/// texel modulo 512 (the shaders sample it with repeat), valid within 256 m
/// of a centre that moves in 8 m steps; texels the square takes over are
/// reset to 1 (`FUN_035a658c`).
struct Ring {
    center: Option<IVec2>,
    data: Vec<f32>,
}

impl Ring {
    fn new() -> Self {
        Self {
            center: None,
            data: vec![1.0; (WIDE_TEXELS * WIDE_TEXELS * 4) as usize],
        }
    }

    fn index(x: i32, z: i32) -> usize {
        (z.rem_euclid(WIDE_TEXELS) * WIDE_TEXELS + x.rem_euclid(WIDE_TEXELS)) as usize * 4
    }

    /// Follows `point`; clears the texels that the new square takes over.
    fn follow(&mut self, point: Vec2) -> bool {
        let center = (point / WIDE_SNAP).round().as_ivec2() * WIDE_SNAP as i32;
        let Some(old) = self.center.replace(center) else {
            return false;
        };
        if old == center {
            return false;
        }
        let half = WIDE_TEXELS / 2;
        let inside = |c: IVec2, x: i32, z: i32| {
            (c.x - half..c.x + half).contains(&x) && (c.y - half..c.y + half).contains(&z)
        };
        for z in center.y - half..center.y + half {
            for x in center.x - half..center.x + half {
                if !inside(old, x, z) {
                    let at = Self::index(x, z);
                    self.data[at..at + 4].fill(1.0);
                }
            }
        }
        true
    }

    fn stamp(&mut self, center: Vec2, half: f32, mut texel: impl FnMut(&mut [f32], Vec2)) -> bool {
        let (lo, hi) = (
            (center - half).floor().as_ivec2(),
            (center + half).ceil().as_ivec2(),
        );
        let mut any = false;
        for z in lo.y..hi.y {
            for x in lo.x..hi.x {
                let at = Self::index(x, z);
                texel(&mut self.data[at..at + 4], IVec2::new(x, z).as_vec2() + 0.5);
                any = true;
            }
        }
        any
    }

    /// The texel's values at a world point.
    fn at(&self, point: Vec2) -> [f32; 4] {
        let p = point.floor().as_ivec2();
        let at = Self::index(p.x, p.y);
        self.data[at..at + 4].try_into().unwrap()
    }

    fn bytes(&self) -> Vec<u8> {
        self.data.iter().map(|v| unorm8(*v)).collect()
    }
}

/// What a mow request is (`FUN_035ace58`'s type 1, 2, 4, 8): the channel
/// of the maps it writes (the 64 m map has only the first two).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MowKind {
    /// A blade's cut (type 1): blades there go, tufts drop to 35 %.
    Cut,
    /// Type 2: tufts go too (callers not identified).
    #[allow(dead_code)]
    Clear,
    /// Fire (type 4): the wide map's blue, the grass's burnt colour.
    Burn,
    /// Type 8: the wide map's alpha, half height and stubble colour.
    #[allow(dead_code)]
    Flatten,
}

impl MowKind {
    fn channel(self) -> usize {
        match self {
            MowKind::Cut => 0,
            MowKind::Clear => 1,
            MowKind::Burn => 2,
            MowKind::Flatten => 3,
        }
    }
}

/// A request to mow the grass in a disc (`FUN_035ace58`: position, `r`,
/// intensity, type; the disc is `CUT_DISC·r` across the centre).
#[derive(Message, Clone, Debug, PartialEq)]
pub struct MowGrass {
    pub center: Vec2,
    pub r: f32,
    pub intensity: f32,
    pub kind: MowKind,
}

/// A request to press the grass flat (`FUN_035a5780`'s seeds).
#[derive(Message, Clone, Debug, PartialEq)]
pub enum PressGrass {
    /// Outwards from `center`, fully at the centre, none at `radius`
    /// (`FUN_036ff498` with no direction and no inner radius).
    Radial { center: Vec2, radius: f32 },
    /// A body standing on the grass: outwards from a box of `size` (x, z)
    /// at `center`, fully up to a metre inside the box's margin of 1.15 m
    /// (`FUN_035a393c` → `FUN_035a134c`, `interact_seed` square).
    Footprint { center: Vec2, size: Vec2 },
}

/// The maps, their textures and whether each changed since the upload.
#[derive(Resource)]
pub struct InteractMaps {
    mow: Window,
    /// The lie as a vector (x, z) a texel, −1 to 1, as the 8-bit map holds
    /// it (`0.5 + v/2`).
    lie: Window,
    wide: Ring,
    pub mow_image: Handle<Image>,
    pub wide_image: Handle<Image>,
    pub lie_image: Handle<Image>,
    changed: [bool; 3],
    /// Some grass is pressed (the lie decays each frame until none is).
    lying: bool,
    /// Game frames not yet decayed.
    pending: f32,
    /// The maps have not followed the camera yet.
    fresh: bool,
}

fn map_image(texels: i32, format: TextureFormat, bytes: Vec<u8>, repeat: bool) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: texels as u32,
            height: texels as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        format,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    let address = if repeat {
        ImageAddressMode::Repeat
    } else {
        ImageAddressMode::ClampToEdge
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// The lie map's texel for a push `v` (x, z): the grass shaders read it
/// back as `(2·x − 1, 1 − 2·y)`.
fn lie_texel(v: [f32; 2]) -> [u8; 2] {
    [unorm8(0.5 + v[0] / 2.0), unorm8(0.5 - v[1] / 2.0)]
}

fn lie_value(texel: [u8; 2]) -> [f32; 2] {
    [
        texel[0] as f32 / 255.0 * 2.0 - 1.0,
        1.0 - texel[1] as f32 / 255.0 * 2.0,
    ]
}

impl FromWorld for InteractMaps {
    fn from_world(world: &mut World) -> Self {
        let mow = Window::new(NEAR_TEXELS, NEAR_SPAN, 2, 1.0);
        let lie = Window::new(NEAR_TEXELS, NEAR_SPAN, 2, 0.0);
        let wide = Ring::new();
        let mut images = world.resource_mut::<Assets<Image>>();
        let mow_image = images.add(map_image(
            NEAR_TEXELS,
            TextureFormat::Rg8Unorm,
            mow.bytes(),
            false,
        ));
        let lie_image = images.add(map_image(
            NEAR_TEXELS,
            TextureFormat::Rg8Unorm,
            lie_bytes(&lie),
            false,
        ));
        let wide_image = images.add(map_image(
            WIDE_TEXELS,
            TextureFormat::Rgba8Unorm,
            wide.bytes(),
            true,
        ));
        Self {
            mow,
            lie,
            wide,
            mow_image,
            wide_image,
            lie_image,
            changed: [false; 3],
            lying: false,
            pending: 0.0,
            fresh: true,
        }
    }
}

fn lie_bytes(lie: &Window) -> Vec<u8> {
    lie.data
        .chunks(2)
        .flat_map(|v| lie_texel([v[0], v[1]]))
        .collect()
}

impl InteractMaps {
    /// `gsys_environment` 34, 35, 36: the lie, mow and wide maps' transforms.
    pub fn transforms(&self) -> [Vec4; 3] {
        let wide = 1.0 / WIDE_TEXELS as f32;
        [
            self.lie.transform(),
            self.mow.transform(),
            Vec4::new(wide, wide, 0.0, 0.0),
        ]
    }

    /// Moves the maps with the camera; the mow map takes what comes in from
    /// the wide map (`interact_reprint` of its red and green), and the
    /// square metres that come wholly into it lose their grass where
    /// `hidden` says (`interact_cut_array`).
    fn follow(&mut self, eye: Vec2, hidden: &mut impl FnMut(IVec2) -> bool) {
        if self.wide.follow(eye) {
            self.changed[1] = true;
        }
        let shift = self.mow.follow(eye);
        if shift.is_some() || self.fresh {
            // Everything is new to a map that has not followed yet.
            let shift = shift
                .filter(|_| !self.fresh)
                .unwrap_or(IVec2::splat(NEAR_TEXELS));
            let mut metres = HashSet::new();
            for j in 0..NEAR_TEXELS {
                for i in 0..NEAR_TEXELS {
                    let (si, sj) = (i + shift.x, j + shift.y);
                    if !((0..NEAR_TEXELS).contains(&si) && (0..NEAR_TEXELS).contains(&sj)) {
                        let point = self.mow.point(i, j);
                        let wide = self.wide.at(point);
                        let at = self.mow.index(i, j);
                        self.mow.data[at..at + 2].copy_from_slice(&wide[..2]);
                        metres.insert(point.floor().as_ivec2());
                    }
                }
            }
            for metre in metres {
                if hidden(metre) {
                    self.cut_metre(metre);
                }
            }
            self.fresh = false;
            self.changed[0] = true;
        }
        if self.lie.follow(eye).is_some() {
            self.changed[2] = true;
        }
    }

    /// Zeroes the green of a square metre in both mow maps, if it lies
    /// wholly in the 64 m map (`interact_cut_array`: a point of the metre's
    /// size, blended to the lowest; `0x035ad30c` keeps a point back until it
    /// fits).
    fn cut_metre(&mut self, metre: IVec2) {
        let per_metre = (1.0 / self.mow.texel).round() as i32;
        let lo = metre * per_metre - self.mow.origin;
        let hi = lo + IVec2::splat(per_metre);
        if lo.min_element() < 0 || hi.max_element() > NEAR_TEXELS {
            return;
        }
        for j in lo.y..hi.y {
            for i in lo.x..hi.x {
                let at = self.mow.index(i, j);
                self.mow.data[at + 1] = 0.0;
            }
        }
        let at = Ring::index(metre.x, metre.y);
        self.wide.data[at + 1] = 0.0;
        self.changed[1] = true;
    }

    /// Mows a disc (`interact_cut`, blended to the lowest): the request's
    /// channel down to `1 − intensity·(1 − d)`, `d` the distance over the
    /// disc's radius; only cuts and clears reach the 64 m map.
    pub fn mow(&mut self, cut: &MowGrass) {
        let radius = CUT_DISC * cut.r;
        let channel = cut.kind.channel();
        let texel = |values: &mut [f32], point: Vec2| {
            let d = point.distance(cut.center) / radius.max(1e-4);
            if d <= 1.0 {
                values[channel] =
                    values[channel].min((1.0 - cut.intensity * (1.0 - d)).clamp(0.0, 1.0));
            }
        };
        if channel < 2 && self.mow.stamp(cut.center, radius, texel) {
            self.changed[0] = true;
        }
        if self.wide.stamp(cut.center, radius, texel) {
            self.changed[1] = true;
        }
    }

    /// Presses grass flat (`interact_seed`, blended to the strongest push
    /// each way along x and z).
    pub fn press(&mut self, press: &PressGrass) {
        let strongest = |values: &mut [f32], push: Vec2| {
            for (v, p) in values.iter_mut().zip([push.x, push.y]) {
                let (pos, neg) = (
                    v.max(0.0).max(p.max(0.0)),
                    (-*v).max(0.0).max((-p).max(0.0)),
                );
                *v = pos - neg;
            }
        };
        let pressed = match *press {
            PressGrass::Radial { center, radius } => {
                self.lie.stamp(center, radius, |values, point| {
                    let offset = point - center;
                    let d = offset.length() / radius.max(1e-4);
                    if d <= 1.0 && d > 0.0 {
                        strongest(values, offset.normalize() * (1.0 - d));
                    }
                })
            }
            PressGrass::Footprint { center, size } => {
                let outer = size * 0.5 + FOOTPRINT_EDGE;
                let inner = size / (size + 2.0 * FOOTPRINT_EDGE);
                self.lie
                    .stamp(center, outer.max_element(), |values, point| {
                        let q = (point - center) / outer;
                        if q.abs().max_element() > 1.0 {
                            return;
                        }
                        // Full a metre in from the square's edge.
                        let edge = ((Vec2::ONE - q.abs()) * outer)
                            .min_element()
                            .clamp(0.0, 1.0);
                        let direction = (q * inner).normalize_or_zero();
                        strongest(values, direction * edge);
                    })
            }
        };
        if pressed {
            self.lying = true;
            self.changed[2] = true;
        }
    }

    /// Game frames of lie standing back up: × 0.95 a frame, through the
    /// 8-bit map each frame (`interact_copy` 1, `interact_merge`).
    // SI-GRS-06: sword grass interaction windows and pressing are ours.
    fn decay(&mut self, seconds: f32) {
        self.pending =
            (self.pending + seconds * super::wind::FRAME_RATE).min(super::wind::FRAME_RATE);
        while self.pending >= 1.0 {
            self.pending -= 1.0;
            if !self.lying {
                continue;
            }
            // Until a frame changes no texel (a small bend stays: 5 % of
            // it rounds back in 8 bits).
            let mut lying = false;
            for v in self.lie.data.chunks_mut(2) {
                let before = lie_texel([v[0], v[1]]);
                let stored = lie_value(before);
                v[0] = stored[0] * LIE_KEEP;
                v[1] = stored[1] * LIE_KEEP;
                lying |= lie_texel([v[0], v[1]]) != before;
            }
            self.lying = lying;
            self.changed[2] = true;
        }
    }
}

/// Moves the maps with the camera, applies the frame's requests, and
/// uploads what changed.
pub fn update_maps(
    time: Res<Time>,
    cameras: Query<&GlobalTransform, crate::camera::MainCamera>,
    mut mows: MessageReader<MowGrass>,
    mut presses: MessageReader<PressGrass>,
    mut maps: ResMut<InteractMaps>,
    mut hidden: ResMut<HiddenGrass>,
    mut images: ResMut<Assets<Image>>,
) {
    if let Ok(camera) = cameras.single() {
        maps.follow(camera.translation().xz(), &mut |metre| hidden.hidden(metre));
    }
    maps.decay(time.delta_secs());
    for cut in mows.read() {
        maps.mow(cut);
    }
    for press in presses.read() {
        maps.press(press);
    }
    let maps = &mut *maps;
    if maps.changed[0]
        && let Some(mut image) = images.get_mut(&maps.mow_image)
    {
        image.data = Some(maps.mow.bytes());
    }
    if maps.changed[1]
        && let Some(mut image) = images.get_mut(&maps.wide_image)
    {
        image.data = Some(maps.wide.bytes());
    }
    if maps.changed[2]
        && let Some(mut image) = images.get_mut(&maps.lie_image)
    {
        image.data = Some(lie_bytes(&maps.lie));
    }
    maps.changed = [false; 3];
}

#[cfg(test)]
mod tests {
    use super::*;

    fn maps() -> InteractMaps {
        let mut maps = InteractMaps {
            mow: Window::new(NEAR_TEXELS, NEAR_SPAN, 2, 1.0),
            lie: Window::new(NEAR_TEXELS, NEAR_SPAN, 2, 0.0),
            wide: Ring::new(),
            mow_image: Handle::default(),
            wide_image: Handle::default(),
            lie_image: Handle::default(),
            changed: [false; 3],
            lying: false,
            pending: 0.0,
            fresh: true,
        };
        maps.follow(Vec2::ZERO, &mut |_| false);
        maps
    }

    fn near(window: &Window, point: Vec2) -> &[f32] {
        let t = window.transform();
        let texel = ((point * t.xy() + t.zw()) * NEAR_TEXELS as f32)
            .floor()
            .as_ivec2();
        let at = window.index(texel.x, texel.y);
        &window.data[at..at + window.channels]
    }

    #[test]
    fn a_cut_mows_its_channel_and_the_wide_map_remembers_it() {
        let mut maps = maps();
        maps.mow(&MowGrass {
            center: Vec2::new(3.0, -2.0),
            r: 4.0,
            intensity: 4.0,
            kind: MowKind::Cut,
        });
        // The disc is 2 m across the centre, the middle 3/4 of it fully cut.
        assert_eq!(near(&maps.mow, Vec2::new(3.1, -2.1)), [0.0, 1.0]);
        assert_eq!(near(&maps.mow, Vec2::new(5.5, -2.0)), [1.0, 1.0]);
        assert_eq!(maps.wide.at(Vec2::new(3.0, -2.0)), [0.0, 1.0, 1.0, 1.0]);
        // Fire only reaches the wide map.
        maps.mow(&MowGrass {
            center: Vec2::new(-10.0, 0.0),
            r: 4.0,
            intensity: 4.0,
            kind: MowKind::Burn,
        });
        assert_eq!(maps.wide.at(Vec2::new(-10.0, 0.0)), [1.0, 1.0, 0.0, 1.0]);
        assert_eq!(near(&maps.mow, Vec2::new(-10.0, 0.0)), [1.0, 1.0]);
    }

    #[test]
    fn the_mow_map_takes_the_wide_maps_cuts_as_it_moves() {
        let mut maps = maps();
        maps.mow(&MowGrass {
            center: Vec2::new(100.0, 0.0),
            r: 8.0,
            intensity: 4.0,
            kind: MowKind::Cut,
        });
        // Out of the 64 m map's reach, but the wide map has it.
        assert_eq!(maps.wide.at(Vec2::new(100.0, 0.0))[0], 0.0);
        maps.follow(Vec2::new(90.0, 0.0), &mut |_| false);
        assert_eq!(near(&maps.mow, Vec2::new(100.0, 0.0))[0], 0.0);
    }

    #[test]
    fn hidden_metres_lose_their_grass_as_the_mow_map_reaches_them() {
        let mut maps = maps();
        let mut hidden = |m: IVec2| m == IVec2::new(3, -2) || m == IVec2::new(40, 0);
        maps.follow(Vec2::new(0.1, 0.0), &mut hidden);
        // Only once a metre comes into the 64 m map (the start was mapped
        // with nothing hidden).
        assert_eq!(near(&maps.mow, Vec2::new(3.5, -1.5)), [1.0, 1.0]);
        maps.follow(Vec2::new(60.0, 0.0), &mut hidden);
        // The whole metre, green only, in both maps.
        for corner in [Vec2::new(40.05, 0.05), Vec2::new(40.95, 0.95)] {
            assert_eq!(near(&maps.mow, corner), [1.0, 0.0]);
        }
        assert_eq!(near(&maps.mow, Vec2::new(41.05, 0.5)), [1.0, 1.0]);
        assert_eq!(maps.wide.at(Vec2::new(40.5, 0.5)), [1.0, 0.0, 1.0, 1.0]);
        // The wide map remembers it after the 64 m map has moved on.
        maps.follow(Vec2::new(200.0, 0.0), &mut hidden);
        maps.follow(Vec2::new(60.0, 0.0), &mut |_| false);
        assert_eq!(near(&maps.mow, Vec2::new(40.5, 0.5)), [1.0, 0.0]);
    }

    #[test]
    fn a_fresh_map_hides_what_it_starts_on() {
        let mut maps = maps();
        maps.fresh = true;
        maps.follow(Vec2::ZERO, &mut |m| m == IVec2::new(3, -2));
        assert_eq!(near(&maps.mow, Vec2::new(3.5, -1.5)), [1.0, 0.0]);
    }

    #[test]
    fn a_push_presses_outwards_and_stands_back_up() {
        let mut maps = maps();
        maps.press(&PressGrass::Radial {
            center: Vec2::ZERO,
            radius: 2.0,
        });
        let east = near(&maps.lie, Vec2::new(0.5, 0.05))[0];
        let south = near(&maps.lie, Vec2::new(0.05, 0.5))[1];
        assert!(east > 0.6 && south > 0.6, "{east} {south}");
        // As the shader reads it back: pushed towards +x.
        let texel = lie_texel([east, 0.0]);
        assert!(texel[0] > 200);
        // A second, weaker push the same way keeps the stronger.
        maps.press(&PressGrass::Radial {
            center: Vec2::new(-1.0, 0.0),
            radius: 1.2,
        });
        assert_eq!(near(&maps.lie, Vec2::new(0.5, 0.05))[0], east);
        // 90 game frames: 0.95^90 of it would be 1 %, but through the
        // 8-bit map each frame a small bend stays (5 % of it rounds back).
        maps.decay(1.0);
        maps.decay(1.0);
        maps.decay(1.0);
        let left = near(&maps.lie, Vec2::new(0.5, 0.05))[0];
        assert!(left > 0.0 && left < 0.08, "{left}");
    }

    #[test]
    fn a_footprint_presses_around_the_body() {
        let mut maps = maps();
        maps.press(&PressGrass::Footprint {
            center: Vec2::ZERO,
            size: Vec2::splat(0.6),
        });
        // Pressed away from the body, fully just outside it, none 1.45 m out.
        let close = near(&maps.lie, Vec2::new(0.4, 0.02));
        let far = near(&maps.lie, Vec2::new(1.6, 0.02));
        assert!(close[0] > 0.9, "{close:?}");
        assert_eq!(far, [0.0, 0.0]);
    }

    #[test]
    fn the_wide_ring_forgets_what_it_leaves() {
        let mut wide = Ring::new();
        wide.follow(Vec2::ZERO);
        wide.stamp(Vec2::new(-200.0, 0.0), 2.0, |v, _| v.fill(0.0));
        wide.follow(Vec2::new(40.0, 0.0));
        // Still within 256 m of the new centre.
        assert_eq!(wide.at(Vec2::new(-200.0, 0.0)), [0.0; 4]);
        wide.follow(Vec2::new(300.0, 0.0));
        assert_eq!(wide.at(Vec2::new(-200.0, 0.0)), [1.0; 4]);
    }
}
