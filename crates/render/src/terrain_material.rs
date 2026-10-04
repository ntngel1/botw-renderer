//! Terrain shading with the game's own material textures: an extension of
//! `StandardMaterial` (see `terrain_material.wgsl`), a shared albedo array
//! and a per-tile `.mate` texture.

use std::path::PathBuf;

use asset_format::paths;
use asset_format::terrain::{MaterialTable as BakedMaterials, MaterialTile};
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::shader::ShaderRef;

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

const SHADER: Handle<Shader> = uuid_handle!("6c3f1a52-9d0e-4b8e-a2b7-5f4d3c2e1b0a");

/// Number of terrain materials (`.mate` indices).
pub const MATERIALS: usize = 88;

pub struct TerrainMaterialPlugin {
    /// The `assets/` folder.
    pub assets: PathBuf,
}

impl Plugin for TerrainMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "terrain_material.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<TerrainMaterial>::default())
            .add_systems(Update, finish_loading);
        let look = match load(&self.assets, app.world().resource::<AssetServer>()) {
            Ok((loading, albedo)) => {
                app.insert_resource(albedo);
                TerrainLook::Loading(loading)
            }
            Err(error) => {
                warn!("terrain materials unavailable ({error}); using plain colours");
                TerrainLook::Plain
            }
        };
        app.insert_resource(look);
    }
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct TerrainExtension {
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub albedo: Handle<Image>,
    #[texture(102, sample_type = "u_int")]
    pub mate: Handle<Image>,
    /// Tile minimum corner (x, z) and edge length, in world units; `w` is
    /// [`PLAIN`] for tiles coloured by their vertices instead of textures.
    #[uniform(103)]
    pub tile: Vec4,
    #[uniform(104)]
    pub table: MaterialTable,
    /// Normals per material layer (`MaterialCmb`), a flat 1×1 array if absent.
    #[texture(105, dimension = "2d_array")]
    #[sampler(106)]
    pub normals: Handle<Image>,
    /// The cloud shadow's texture (see `clouds.rs`).
    #[texture(107)]
    #[sampler(108)]
    pub cloud_shadow_map: Handle<Image>,
    #[uniform(109)]
    pub clouds: crate::clouds::CloudParams,
    /// The shared look values (`look::LookTexture`, see `look.rs`).
    #[texture(110)]
    pub look: Handle<Image>,
    /// The ambient occlusion's rotations (`deferred_light::SsaoNoise`).
    #[texture(111)]
    pub ssao_noise: Handle<Image>,
    /// The environment's mean brightness (`deferred_light::CubeMean`).
    #[texture(112, sample_type = "float", filterable = false)]
    pub cube_mean: Handle<Image>,
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// `TerrainExtension::tile.w` of tiles without the game's textures: their
/// vertex colours are the albedo.
pub const PLAIN: f32 = 1.0;

/// White albedo, flat normals and a table for tiles without the game's
/// textures (see [`PLAIN`]).
pub fn plain_textures(images: &mut Assets<Image>) -> (Handle<Image>, Handle<Image>, MaterialTable) {
    let mut white = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![255, 255, 255, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    white.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    let table = MaterialTable {
        entries: [Vec4::new(1.0, 1.0, 0.0, 0.0); MATERIALS],
    };
    (images.add(white), images.add(flat_normals()), table)
}

/// Per material index: u scale, v scale, texture layer, unused.
#[derive(ShaderType, Clone, Debug)]
pub struct MaterialTable {
    pub entries: [Vec4; MATERIALS],
}

/// How terrain tiles are shaded.
#[derive(Resource)]
pub enum TerrainLook {
    /// Vertex colours from height and slope (no baked materials).
    Plain,
    /// The game's textures are loading; terrain waits for them.
    Loading(Loading),
    Textured {
        albedo: Handle<Image>,
        normals: Handle<Image>,
        table: Box<MaterialTable>,
    },
}

pub struct Loading {
    albedo: Handle<Image>,
    normals: Option<Handle<Image>>,
    table: Box<MaterialTable>,
}

/// the original renderer's terrain albedo array kept on the CPU, as far as the
/// grass reads it: each layer's mean linear colour (baked with the
/// materials).
#[derive(Resource, Clone)]
pub struct TerrainAlbedo(pub std::sync::Arc<Vec<[f32; 3]>>);

impl TerrainAlbedo {
    /// Mean linear RGB per albedo layer.
    pub fn layer_means(&self) -> &[[f32; 3]] {
        &self.0
    }
}

impl TerrainLook {
    pub fn is_loading(&self) -> bool {
        matches!(self, TerrainLook::Loading(_))
    }
}

/// The baked materials (`terrain/materials.ron`) and their texture arrays.
fn load(
    assets: &std::path::Path,
    server: &AssetServer,
) -> Result<(Loading, TerrainAlbedo), String> {
    let baked: BakedMaterials = asset_format::read_ron(&assets.join(paths::TERRAIN_MATERIALS))
        .map_err(|e| e.to_string())?;
    let layers = baked.materials.iter().map(|m| m.layer as usize + 1).max();
    let mut means = vec![[0.0; 3]; layers.unwrap_or(0)];
    for material in &baked.materials {
        means[material.layer as usize] = material.mean_albedo;
    }
    let entries = std::array::from_fn(|m| match baked.materials.get(m) {
        Some(material) => Vec4::new(
            material.uv_scale[0],
            material.uv_scale[1],
            material.layer as f32,
            0.0,
        ),
        // The TSCB's default UV scale is 0.1 (10 m).
        None => Vec4::new(0.1, 0.1, 0.0, 0.0),
    });
    // SI-FMT-10: texture mips and BC conversions are ours (baked).
    let repeat = |settings: &mut ImageLoaderSettings| {
        settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 16,
            ..default()
        });
    };
    let loading = Loading {
        albedo: server
            .load_builder()
            .with_settings(repeat)
            .load(paths::TERRAIN_ALBEDO),
        normals: (baked.normals_size > 0).then(|| {
            server
                .load_builder()
                .with_settings(repeat)
                .load(paths::TERRAIN_NORMALS)
        }),
        table: Box::new(MaterialTable { entries }),
    };
    Ok((loading, TerrainAlbedo(std::sync::Arc::new(means))))
}

fn finish_loading(
    mut look: ResMut<TerrainLook>,
    mut images: ResMut<Assets<Image>>,
    server: Res<AssetServer>,
) {
    let TerrainLook::Loading(loading) = &*look else {
        return;
    };
    let handles = std::iter::once(&loading.albedo).chain(&loading.normals);
    if let Some(failed) = handles
        .clone()
        .find(|h| server.load_state(h.id()).is_failed())
    {
        warn!(
            "terrain texture {:?} failed to load; using plain colours",
            server.get_path(failed.id())
        );
        *look = TerrainLook::Plain;
        return;
    }
    if !handles.into_iter().all(|h| server.is_loaded(h.id())) {
        return;
    }
    let normals = match &loading.normals {
        Some(normals) => normals.clone(),
        None => images.add(flat_normals()),
    };
    info!("terrain textures ready");
    *look = TerrainLook::Textured {
        albedo: loading.albedo.clone(),
        normals,
        table: loading.table.clone(),
    };
}

/// A one-texel array of the flat normal without gloss (blue), for when
/// `MaterialCmb` is missing.
fn flat_normals() -> Image {
    let mut image = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![128, 128, 0, 255],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image
}

/// A tile's `.mate` data as an integer texture (material 0, material 1,
/// blend, unknown per sample).
pub fn mate_image(material: &MaterialTile) -> Image {
    Image::new(
        Extent3d {
            width: 256,
            height: 256,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        material.to_bytes(),
        TextureFormat::Rgba8Uint,
        RenderAssetUsages::RENDER_WORLD,
    )
}

/// Stand-in `.mate` for tiles without one: material 0 everywhere.
pub fn blank_mate() -> Image {
    Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![0, 0, 0, 0],
        TextureFormat::Rgba8Uint,
        RenderAssetUsages::RENDER_WORLD,
    )
}
