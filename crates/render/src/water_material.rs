//! Water shading (see `water_material.wgsl`): an extension of
//! `StandardMaterial` that follows the game's own water shader with its
//! maps (`WaterNrm`, `WaterEmm` in Terrain.Tex1, one layer per water kind),
//! table (`WaterAlb`) and `TeraWater` material as baked in `assets/water`
//! (see `asset-format`), or stand-ins and analytic ripples without them.
//! The water is opaque and drawn in the
//! transmissive phase, so its shader sees the scene behind it (the view's
//! transmission texture) and, with the camera's depth prepass, how deep the
//! water is.

use std::path::{Path, PathBuf};

use asset_format::paths;
use asset_format::terrain::water::kind;
use asset_format::texture::{Format, Texture};
use asset_format::water::{TeraWater, WATER_KINDS, WATER_TABLE_TEXELS, WaterTable};
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{
    ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline,
};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureViewDescriptor, TextureViewDimension,
};
use bevy::shader::{ShaderDefVal, ShaderRef};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use crate::climate::Weather;
use crate::clouds::{CloudParams, CloudShadows, CloudsNow};
use crate::look::LookTexture;
use crate::ready::SceneEpoch;

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExtension>;

const SHADER: Handle<Shader> = uuid_handle!("3e9a4c17-8b2d-4f05-a6c1-7d0e5b93f28a");
const FIELD_WATER: Handle<Shader> = uuid_handle!("c4d82e57-1a9b-4f3c-9e60-27b5d8a1f4e3");
const LAVA_SHADER: Handle<Shader> = uuid_handle!("8d41b6e2-5c07-4a93-b1f8-2e6a9c3d7f14");

pub struct WaterMaterialPlugin {
    /// The baked assets (`assets/water`).
    pub assets: PathBuf,
}

impl Plugin for WaterMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, FIELD_WATER, "field_water.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, SHADER, "water_material.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, LAVA_SHADER, "lava.wgsl", Shader::from_wgsl);
        let assets = self.assets.clone();
        let loading =
            Some(AsyncComputeTaskPool::get().spawn(async move { load_game_water(&assets) }));
        app.add_plugins((
            MaterialPlugin::<WaterMaterial>::default(),
            MaterialPlugin::<LavaMaterial>::default(),
        ))
        .insert_resource(WaterLook {
            material: Handle::default(),
            lava: Handle::default(),
            loading,
            clouds_behind: false,
        })
        .init_resource::<WaterFollow>()
        .add_systems(Startup, create_material)
        .add_systems(Update, (finish_loading, follow_weather))
        // In the frame the sky's clouds change, so that a frame drawn
        // right after the water takes them shows them.
        .add_systems(
            PostUpdate,
            follow_clouds.after(crate::clouds::update_clouds),
        );
        // The game's world has the sea around it (the baked world is always
        // the game's).
        app.add_systems(Startup, spawn_horizon_sea.after(create_material));
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct WaterExtension {
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub normals: Handle<Image>,
    #[uniform(102)]
    pub params: WaterParams,
    /// The shared look values (`look::LookTexture`, see `look.rs`).
    #[texture(103)]
    pub look: Handle<Image>,
    /// The sky's clouds, for their shadow on the water (see `clouds.rs`).
    #[texture(104)]
    #[sampler(105)]
    pub cloud_noise: Handle<Image>,
    #[uniform(106)]
    pub clouds: CloudParams,
    /// Foam per water kind (`WaterEmm`), sampled with `normals`' sampler.
    #[texture(107, dimension = "2d_array")]
    pub foam: Handle<Image>,
    /// The cloud shadow's texture (see `clouds.rs`).
    #[texture(108)]
    #[sampler(109)]
    pub cloud_shadow_map: Handle<Image>,
}

/// See `WaterParams` in `water_material.wgsl`.
#[derive(ShaderType, Clone, Debug)]
pub struct WaterParams {
    // SI-WAT-03: aerial perspective over water is our own.
    /// 1 when the game's maps are bound, the sun glint's weather factor
    /// (see [`clear_sky_share`]), strength of the aerial perspective the
    /// water adds itself (the viewer's own, fitted to the game's
    /// screenshots), unused.
    pub surface: Vec4,
    /// The texture spaces, see [`TeraWater::texture_spaces`].
    pub srt: [Vec4; 12],
    /// `indirect_scale2.xy`, `indirect_scale4.xy`.
    pub indirect: Vec4,
    /// `const_color3.a`, `const_color5.a`, `const_value2`, `const_value3`.
    pub depth: Vec4,
    /// `const_value6`, `const_vector0.x`, `const_vector1.x`, unused.
    pub shape: Vec4,
    /// `const_color2`.
    pub normal_base: Vec4,
    /// Frames per second of the water's clock, phase per frame,
    /// environment[38].x and .w (see [`WaterClock`]).
    pub clock: Vec4,
    /// The water table (`WaterAlb`), kind-major.
    pub kinds: [Vec4; WATER_KINDS * WATER_TABLE_TEXELS],
}

impl WaterParams {
    fn new(material: &TeraWater, table: &[[[f32; 4]; WATER_TABLE_TEXELS]; WATER_KINDS]) -> Self {
        let m = material;
        Self {
            surface: Vec4::new(0.0, 1.0, 1.0, 0.0),
            srt: m.texture_spaces().map(Vec4::from_array),
            indirect: Vec4::new(
                m.indirect_scale2[0],
                m.indirect_scale2[1],
                m.indirect_scale4[0],
                m.indirect_scale4[1],
            ),
            depth: Vec4::new(
                m.const_color3[3],
                m.const_color5[3],
                m.const_value2,
                m.const_value3,
            ),
            shape: Vec4::new(m.const_value6, m.const_vector0[0], m.const_vector1[0], 0.0),
            normal_base: Vec4::from_array(m.const_color2),
            clock: WaterClock::GAME.uniform(),
            kinds: table_texels(table),
        }
    }
}

impl Default for WaterParams {
    fn default() -> Self {
        Self::new(&TeraWater::STAND_IN, &fallback_table())
    }
}

/// The water's clock and current scale. The game accumulates frames in
/// `ksys::tera::System` (a double, `+0x938`) and turns them into
/// `gsys_environment[37]` (phase) and `[38]` (scale of the flow) on the CPU;
/// see "Phase and scale" in docs/research/wiiu-water-variants.md.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterClock {
    /// Frames the game's accumulator gains per second.
    pub frames_per_second: f32,
    /// `S`: float at `0x1046f47c`.
    pub s: f32,
    /// `R`: float at System `+0x340`.
    pub r: f32,
}

impl WaterClock {
    /// Recovered statically: KSys sets the VFR base interval to 2 vsyncs
    /// (@`0x03413cf4`), so a delta frame is a 30 Hz frame and the accumulator
    /// gains 30 per second outside slow motion; nothing writes `S` after the
    /// image's 0.5, and only the System constructor writes `R` (20.0).
    pub const GAME: Self = Self {
        frames_per_second: 30.0,
        s: 0.5,
        r: 20.0,
    };

    /// The native literals: `c30` (0x3d088888, not 1/30 rounded) and `c02`.
    const C30: f32 = f32::from_bits(0x3d08_8888);
    const C02: f32 = f32::from_bits(0x3ca3_d70a);

    /// `(frames per second, phase per frame, environment[38].x, [38].w)` in
    /// the order the game computes them in f32.
    fn uniform(self) -> Vec4 {
        let a = 50.0 / self.s;
        let b = self.r / self.s;
        Vec4::new(
            self.frames_per_second,
            self.s * Self::C30,
            self.s * Self::C02,
            -(a * b),
        )
    }
}

// `TeraWater` (its parameters, `STAND_IN` and `texture_spaces`) lives in
// `asset_format::water`; `bake` resolves it from the game's material.

fn table_texels(
    table: &[[[f32; 4]; WATER_TABLE_TEXELS]; WATER_KINDS],
) -> [Vec4; WATER_KINDS * WATER_TABLE_TEXELS] {
    std::array::from_fn(|i| Vec4::from_array(table[i / WATER_TABLE_TEXELS][i % WATER_TABLE_TEXELS]))
}

/// A hand-made stand-in for the game's water table when there is no dump:
/// the same layout (see `WaterTable` and `water_material.wgsl`), values
/// chosen by eye. Per kind: the opacity's rate per metre of water (red;
/// green and blue for the shore foam), the same offsets in metres (red 0:
/// clear at the very edge) and how much a current foams.
fn fallback_table() -> [[[f32; 4]; WATER_TABLE_TEXELS]; WATER_KINDS] {
    let water = |rate: [f32; 3], offset: [f32; 3], churn: f32| {
        [
            [1.0, 1.0, 1.0, 1.0],
            [0.002, 0.01, 0.012, 1.0],
            [rate[0], rate[1], rate[2], 1.0],
            [0.0, offset[1], offset[2], 1.0],
            [churn, 0.2, 1.0, 1.0],
            [0.3, 0.95, 0.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    };
    let fresh = water([0.1, 0.6, 0.6], [0.05, 1.05, 1.1], 0.03);
    let mut lava = water([0.1, 0.6, 0.8], [0.3, 1.0, 0.9], 0.02);
    lava[0] = [1.0, 0.7, 0.05, 1.0];
    lava[1] = [0.6, 0.12, 0.01, 1.0];
    [
        fresh,
        water([0.05, 0.1, 0.1], [0.3, 0.3, 0.15], 0.05),
        water([0.2, 0.05, 0.7], [0.3, 0.2, 0.5], 0.02),
        lava,
        water([0.2, 0.3, 0.5], [0.05, 0.5, 0.5], 0.05),
        water([0.02, 0.01, 0.003], [0.3, 1.3, 0.5], 0.1),
        water([0.02, 0.6, 0.8], [0.05, 1.4, 1.0], 0.01),
        water([0.08, 0.3, 0.5], [0.05, 1.05, 1.1], 0.03),
    ]
}

impl MaterialExtension for WaterExtension {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    // Water neither casts shadows nor writes the depth prepass (it reads it).
    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    /// `specular_transmission > 0` only puts the water into the
    /// transmissive phase; the shader refracts on its own, so Bevy's
    /// refraction (a blurred background fetch in `apply_pbr_lighting`) is
    /// left out.
    fn specialize(
        _pipeline: &MaterialExtensionPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialExtensionKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader_defs.retain(|def| {
                !matches!(def, ShaderDefVal::Bool(name, _)
                    if name == "STANDARD_MATERIAL_SPECULAR_TRANSMISSION" || name == "STANDARD_MATERIAL_DIFFUSE_OR_SPECULAR_TRANSMISSION")
            });
        }
        Ok(())
    }
}

/// The game's water textures and table, as far as the dump has them.
struct GameWater {
    normals: Option<Image>,
    foam: Option<Image>,
    table: Option<WaterTable>,
    material: Option<TeraWater>,
}

pub type LavaMaterial = ExtendedMaterial<StandardMaterial, LavaExtension>;

/// Lava shading (see `lava.wgsl`): opaque, from the lava layers of the
/// water textures.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct LavaExtension {
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub normals: Handle<Image>,
    /// `WaterEmm`: where the lava glows.
    #[texture(102, dimension = "2d_array")]
    pub glow: Handle<Image>,
    #[uniform(103)]
    pub params: LavaParams,
    /// The shared look values (`look::LookTexture`, see `look.rs`).
    #[texture(104)]
    pub look: Handle<Image>,
}

/// See `LavaParams` in `lava.wgsl`.
#[derive(ShaderType, Clone, Debug)]
pub struct LavaParams {
    /// The hottest veins' colour and the dimmer glow's (table texels 0
    /// and 1 of the lava kind).
    pub hot: Vec4,
    pub glow: Vec4,
    /// Emitted radiance of the hottest veins (nits), pattern repeats per
    /// metre, creep speed, normal strength. Fitted to the game's
    /// screenshots of Death Mountain.
    pub settings: Vec4,
    /// Crust albedo, how bright the glow pattern must be to break the crust.
    pub crust: Vec4,
}

impl LavaParams {
    fn from_table(table: &[[[f32; 4]; WATER_TABLE_TEXELS]; WATER_KINDS]) -> Self {
        let lava = &table[usize::from(kind::LAVA)];
        Self {
            hot: Vec4::from_array(lava[0]),
            glow: Vec4::from_array(lava[1]),
            // SI-LGT-25: lava shading on Bevy PBR with fitted numbers.
            settings: Vec4::new(40_000.0, 0.04, 1.0, 0.8),
            crust: Vec4::new(0.05, 0.035, 0.03, 0.2),
        }
    }
}

impl MaterialExtension for LavaExtension {
    fn fragment_shader() -> ShaderRef {
        LAVA_SHADER.into()
    }

    // Flat and glowing: nothing to shade below it.
    fn enable_shadows() -> bool {
        false
    }
}

/// Largest terrain tile whose lava gets a light: only the detailed tiles
/// near the camera (lights on the coarse ones would sit far from the lava).
const LAVA_LIGHT_TILE: f32 = 130.0;
/// Light per square metre of lava (lumens), and how far it reaches.
const LAVA_LUMENS_PER_M2: f32 = 300_000.0;
const LAVA_LIGHT_RANGE: f32 = 80.0;
const LAVA_LIGHT_MAX_LUMENS: f32 = 1.0e9;

/// A warm light a few metres above a patch's visible lava, without shadows
/// (cheap: there may be dozens), or `None` on coarse tiles.
pub fn lava_light(glow: &crate::mesh::LavaGlow, tile_size: f32) -> Option<(PointLight, Transform)> {
    if tile_size > LAVA_LIGHT_TILE || glow.area < 20.0 {
        return None;
    }
    let light = PointLight {
        color: Color::srgb(1.0, 0.55, 0.2),
        intensity: (glow.area * LAVA_LUMENS_PER_M2).min(LAVA_LIGHT_MAX_LUMENS),
        range: LAVA_LIGHT_RANGE,
        radius: 2.0,
        shadow_maps_enabled: false,
        ..default()
    };
    Some((
        light,
        Transform::from_translation(glow.center + Vec3::Y * 4.0),
    ))
}

/// The materials every water and lava surface shares.
#[derive(Resource)]
pub struct WaterLook {
    pub material: Handle<WaterMaterial>,
    pub lava: Handle<LavaMaterial>,
    loading: Option<Task<GameWater>>,
    /// The clouds shading the water lag behind the sky's.
    clouds_behind: bool,
}

impl WaterLook {
    /// The game's water maps are in, and the water has the current clouds.
    pub fn is_settled(&self) -> bool {
        self.loading.is_none() && !self.clouds_behind
    }
}

fn create_material(
    mut look: ResMut<WaterLook>,
    look_texture: Res<LookTexture>,
    clouds: Option<Res<CloudShadows>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut lava: ResMut<Assets<LavaMaterial>>,
) {
    // A flat 1×1 normal per layer until (or unless) the game's maps arrive,
    // and even, faint foam (half-hot lava).
    let kinds = WATER_KINDS as u32;
    let flat = images.add(texture_array(
        1,
        1,
        kinds,
        1,
        2,
        vec![128; 2 * WATER_KINDS],
        "water normals",
    ));
    let mut foam_values = vec![60; WATER_KINDS];
    foam_values[usize::from(kind::LAVA)] = 150;
    let even_foam = images.add(texture_array(1, 1, kinds, 1, 1, foam_values, "water foam"));
    look.lava = lava.add(LavaMaterial {
        base: StandardMaterial::default(),
        extension: LavaExtension {
            normals: flat.clone(),
            glow: even_foam.clone(),
            params: LavaParams::from_table(&fallback_table()),
            look: look_texture.0.clone(),
        },
    });
    look.material = materials.add(WaterMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            alpha_mode: AlphaMode::Opaque,
            // Only to be drawn in the transmissive phase (see `specialize`).
            specular_transmission: 1.0,
            perceptual_roughness: 0.04,
            reflectance: 0.5,
            ..default()
        },
        extension: WaterExtension {
            normals: flat,
            foam: even_foam,
            params: tuning(WaterParams::default()),
            look: look_texture.0.clone(),
            cloud_noise: clouds.as_ref().map(|c| c.noise.clone()).unwrap_or_default(),
            cloud_shadow_map: clouds
                .as_ref()
                .map(|c| c.shadow_map.clone())
                .unwrap_or_default(),
            clouds: CloudParams::without_shadows(),
        },
    });
}

/// The sea's surface height (`.water.extm` kind 7 everywhere, and the
/// game's `Horizon` model).
pub const SEA_LEVEL: f32 = 105.9;

/// How far the horizon sea reaches from the map's centre: past the
/// camera's far plane from anywhere on the terrain grid.
// SI-WAT-06: our own sea ring to 48 km, not the Horizon model.
const HORIZON_REACH: f32 = 48_000.0;

/// Depth of the horizon sea for its waves: deep everywhere.
const HORIZON_DEPTH: f32 = 1000.0;

/// The sea beyond the terrain grid, like the game's `Horizon` model: a flat
/// square ring at sea level around the grid (the tiles' own water covers
/// the grid), so the sea runs on to the horizon.
fn spawn_horizon_sea(
    mut commands: Commands,
    look: Res<WaterLook>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    commands.spawn((
        Name::new("horizon sea"),
        Mesh3d(meshes.add(horizon_mesh(
            asset_format::terrain::WORLD_SIZE / 2.0,
            HORIZON_REACH,
        ))),
        MeshMaterial3d(look.material.clone()),
        Transform::default(),
        NotShadowCaster,
        NoFrustumCulling,
    ));
}

/// A square ring at sea level between half-widths `inner` and `outer`,
/// facing up, carrying the sea's kind like the tiles' water.
fn horizon_mesh(inner: f32, outer: f32) -> Mesh {
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let mut positions = Vec::new();
    for (x, z) in corners {
        positions.push([x * inner, SEA_LEVEL, z * inner]);
        positions.push([x * outer, SEA_LEVEL, z * outer]);
    }
    let count = positions.len();
    // Side k joins corners k and k + 1 (inner 2k, outer 2k + 1); the
    // corners run clockwise seen from above, so this winds counter-clockwise.
    let mut indices = Vec::new();
    for k in 0..4u32 {
        let (i0, o0, i1, o1) = (2 * k, 2 * k + 1, (2 * k + 2) % 8, (2 * k + 3) % 8);
        indices.extend_from_slice(&[i0, o1, o0, i0, i1, o1]);
    }
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; count])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![[f32::from(kind::SEA), HORIZON_DEPTH]; count],
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_1,
        vec![[crate::mesh::game_flow(0.0); 2]; count],
    )
    .with_inserted_indices(Indices::U32(indices))
}

/// Least time between updates of the clouds shading the water, in real
/// seconds. Changing the one material re-extracts and re-specializes every
/// water mesh that uses it, and the sky's values (its colours follow the
/// exposure) change every frame; the clouds' motion comes from the time in
/// the shader, so steps are not visible.
// SI-WAT-07: water follows the sky once a second (pipeline rebuild limit).
const FOLLOW_INTERVAL: f32 = 1.0;

/// Least real seconds between the water's updates of the clouds and the
/// weather ([`FOLLOW_INTERVAL`]). Screenshots temporarily disable the
/// interval while waiting for water reflections to catch up.
#[derive(Resource)]
pub struct WaterFollow {
    pub interval: f32,
}

impl Default for WaterFollow {
    fn default() -> Self {
        Self {
            interval: FOLLOW_INTERVAL,
        }
    }
}

/// The game's `uking_dynamic_cloud_ratio`, which scales the sun's glint in
/// its deferred shading: `1 −` the weight of its overcast sky state
/// ([`Weather::overcast_sky`]).
fn clear_sky_share(weather: &Weather) -> f32 {
    1.0 - weather.overcast_sky()
}

/// The glint's weather factor follows the weather, in steps like the
/// clouds (each change re-specializes every water mesh).
fn follow_weather(
    look: Res<WaterLook>,
    weather: Option<Res<Weather>>,
    real: Res<Time<Real>>,
    follow: Res<WaterFollow>,
    mut last: Local<f32>,
    mut materials: ResMut<Assets<WaterMaterial>>,
) {
    let Some(weather) = weather else { return };
    let share = clear_sky_share(&weather);
    let elapsed = real.elapsed_secs();
    let differs = materials
        .get(&look.material)
        .is_some_and(|m| (m.extension.params.surface.y - share).abs() > 0.01);
    if differs
        && elapsed - *last >= follow.interval
        && let Some(mut material) = materials.get_mut(&look.material)
    {
        material.extension.params.surface.y = share;
        *last = elapsed;
    }
}

/// The clouds whose shadow falls on the water follow the sky's, in steps.
pub(crate) fn follow_clouds(
    mut look: ResMut<WaterLook>,
    now: Option<Res<CloudsNow>>,
    real: Res<Time<Real>>,
    follow: Res<WaterFollow>,
    epoch: Option<Res<SceneEpoch>>,
    mut last: Local<Option<f32>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
) {
    let Some(now) = now else { return };
    // A new scene takes its clouds right away.
    if epoch.is_some_and(|epoch| epoch.is_changed()) {
        *last = None;
    }
    let differs = materials
        .get(&look.material)
        .is_some_and(|m| m.extension.clouds != now.0);
    look.bypass_change_detection().clouds_behind = differs;
    let elapsed = real.elapsed_secs();
    if last.is_some_and(|last| elapsed - last < follow.interval) {
        return;
    }
    if differs && let Some(mut material) = materials.get_mut(&look.material) {
        material.extension.clouds = now.0.clone();
        *last = Some(elapsed);
        look.bypass_change_detection().clouds_behind = false;
    }
}

/// For fitting the viewer's own part of the water to the game's
/// screenshots without rebuilding: `BOTW_WATER="air"` overrides
/// `WaterParams::surface.z`, the aerial perspective's strength.
// SI-WAT-03: aerial perspective over water is our own.
fn tuning(mut params: WaterParams) -> WaterParams {
    if let Some(air) = std::env::var("BOTW_WATER")
        .ok()
        .and_then(|text| text.trim().parse().ok())
    {
        params.surface.z = air;
    }
    params
}

fn finish_loading(
    mut look: ResMut<WaterLook>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
    mut lava: ResMut<Assets<LavaMaterial>>,
) {
    let Some(task) = &mut look.loading else {
        return;
    };
    let Some(game) = block_on(poll_once(task)) else {
        return;
    };
    look.loading = None;
    let normals = game.normals.map(|image| images.add(image));
    let foam = game.foam.map(|image| images.add(image));
    if let Some(mut material) = materials.get_mut(&look.material) {
        if let Some(normals) = &normals {
            material.extension.normals = normals.clone();
            material.extension.params.surface.x = 1.0;
        }
        if let Some(foam) = &foam {
            material.extension.foam = foam.clone();
        }
        if let Some(table) = &game.table {
            material.extension.params.kinds = table_texels(&table.kinds);
        }
        if let Some(tera_water) = &game.material {
            let kinds = material.extension.params.kinds;
            material.extension.params = WaterParams {
                surface: material.extension.params.surface,
                kinds,
                ..WaterParams::new(tera_water, &fallback_table())
            };
        }
    }
    if let Some(mut material) = lava.get_mut(&look.lava) {
        if let Some(normals) = normals {
            material.extension.normals = normals;
        }
        if let Some(foam) = foam {
            material.extension.glow = foam;
        }
        if let Some(table) = &game.table {
            material.extension.params = LavaParams {
                settings: material.extension.params.settings,
                ..LavaParams::from_table(&table.kinds)
            };
        }
    }
}

fn load_game_water(assets: &Path) -> GameWater {
    let table = asset_format::read_ron::<WaterTable>(&assets.join(paths::WATER_TABLE))
        .inspect_err(|error| warn!("water table unavailable: {error}"))
        .ok();
    let material = asset_format::read_ron::<TeraWater>(&assets.join(paths::WATER_MATERIAL))
        .inspect_err(|error| {
            warn!("water material unavailable, using the Wii U v208 values: {error}")
        })
        .ok();
    let [normals, foam] = load_textures(assets);
    GameWater {
        normals,
        foam,
        table,
        material,
    }
}

/// `WaterNrm` (normals) and `WaterEmm` (foam) as RG8 and R8 arrays with
/// full mip chains, as `bake` converts them (the dump has only their first
/// levels).
fn load_textures(assets: &Path) -> [Option<Image>; 2] {
    [
        (paths::WATER_NORMALS, 2, "water normals"),
        (paths::WATER_FOAM, 1, "water foam"),
    ]
    .map(|(path, channels, label)| {
        Texture::read(&assets.join(path))
            .map_err(|error| error.to_string())
            .and_then(|texture| baked_array(&texture, channels, label))
            .inspect_err(|error| warn!("water textures unavailable: {error}"))
            .ok()
    })
}

/// A baked RG8 (`channels` 2) or R8 array (mip-major, as KTX2 stores it).
fn baked_array(texture: &Texture, channels: usize, label: &'static str) -> Result<Image, String> {
    let format = if channels == 2 {
        Format::Rg8
    } else {
        Format::R8
    };
    if texture.format != format {
        return Err(format!(
            "{label}: expected {format:?}, got {:?}",
            texture.format
        ));
    }
    let mut image = texture_array(
        texture.width,
        texture.height,
        texture.layers,
        texture.mip_levels,
        channels,
        texture.data.clone(),
        label,
    );
    image.data_order = TextureDataOrder::MipMajor;
    Ok(image)
}

/// An RG8 (`channels` 2) or R8 texture array (layer-major data) with a
/// repeating, filtered sampler.
fn texture_array(
    width: u32,
    height: u32,
    layers: u32,
    levels: u32,
    channels: usize,
    data: Vec<u8>,
    label: &'static str,
) -> Image {
    Image {
        data: Some(data),
        data_order: TextureDataOrder::LayerMajor,
        texture_descriptor: TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: layers,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: if channels == 2 {
                TextureFormat::Rg8Unorm
            } else {
                TextureFormat::R8Unorm
            },
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        sampler: ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 8,
            ..default()
        }),
        texture_view_descriptor: Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        }),
        asset_usage: RenderAssetUsages::RENDER_WORLD,
        copy_on_resize: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn texture_spaces_chain_from_world_metres() {
        let spaces = TeraWater::STAND_IN.texture_spaces().map(Vec4::from_array);
        let apply = |i: usize, p: Vec2| {
            spaces[2 * i].xy() * p.x + spaces[2 * i].zw() * p.y + spaces[2 * i + 1].xy()
        };
        // The base space repeats every 80 m (tex_srt3 0.0125) along x and z.
        assert!((apply(0, Vec2::new(80.0, 0.0)) - Vec2::X).length() < 1e-5);
        assert!((apply(0, Vec2::new(0.0, 80.0)) - Vec2::Y).length() < 1e-5);
        // A rotated space keeps lengths times its scale (tex_srt5: 0.4, 30°).
        let turned = apply(5, Vec2::X);
        assert!((turned.length() - 0.4).abs() < 1e-5);
        assert!((turned.y.atan2(turned.x) + 30f32.to_radians()).abs() < 1e-5);
    }

    #[test]
    fn glint_fades_with_the_overcast_weathers() {
        assert_eq!(clear_sky_share(&Weather::default()), 1.0);
        assert_eq!(clear_sky_share(&Weather::settled(1)), 0.0); // Cloudy
        // BlueskyRain keeps the sun.
        assert_eq!(clear_sky_share(&Weather::settled(8)), 1.0);
        // Halfway from the clear sky state to the overcast one.
        let mut weather = Weather::settled(1);
        weather.sky.from = asset_format::env::SKY_CLEAR;
        weather.sky.transition = 0.25;
        assert!((clear_sky_share(&weather) - 0.75).abs() < 1e-6);
    }

    #[test]
    fn clock_follows_the_game_formulas() {
        let clock = WaterClock::GAME.uniform();
        // 30 frames a second at 0.5 × c30 per frame: about half a cycle.
        assert!((clock.x * clock.y - 0.5).abs() < 1e-6);
        // The flow value scales by environment[38].x × .w = −R/S.
        assert!((clock.z * clock.w + 40.0).abs() < 1e-3);
    }

    #[test]
    fn horizon_ring_faces_up_around_a_hole() {
        let mesh = horizon_mesh(8000.0, 48_000.0);
        let Some(VertexAttributeValues::Float32x3(positions)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("indices")
        };
        assert_eq!(indices.len(), 8 * 3);
        let mut area = 0.0;
        for triangle in indices.chunks(3) {
            let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(positions[triangle[i] as usize]));
            let normal = (b - a).cross(c - a);
            assert!(normal.y > 0.0, "triangle {triangle:?} faces down");
            area += normal.y / 2.0;
        }
        // The ring's area: the outer square minus the hole.
        let expected = 96_000.0f32.powi(2) - 16_000.0f32.powi(2);
        assert!((area - expected).abs() / expected < 1e-4);
        assert!(positions.iter().all(|p| p[1] == SEA_LEVEL));
    }
}
