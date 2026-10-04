//! The characters' material: the game's deferred shading of characters
//! (`uking_sys_shading` programs 0, 4, 12, 16, 20; see
//! docs/research/wiiu-character-shading.md) instead of physically based
//! light. Link, his outfits, the Bokoblins and what they hold (weapons, the
//! paraglider) get a hard step where the main light turns away, an ambient
//! from the sky above and below the view that does not follow the normal,
//! rim lights where the depth breaks off behind them, cloud shadows and the
//! shared haze (`botw::look`).
//!
//! `character_material.wgsl` does the lighting itself (no
//! `apply_pbr_lighting`).
//!
//! Ported from the original renderer as is; the environment's mean brightness
//! (`deferred_light::CubeMean`) is bound as on the objects (texture 106),
//! and the specular masks' gloss table comes with the call
//! ([`CharacterShading::for_material`]) since the models' module has none
//! for characters.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::ecs::system::SystemParam;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;

use crate::clouds::{CloudParams, CloudShadows, update_clouds};
use crate::deferred_light::CubeMean;
use crate::look::LookTexture;
use crate::models::{Gloss, MaterialLook};

const SHADER: Handle<Shader> = uuid_handle!("c5a8e2f1-7d34-4b96-8e0c-2f9a61d4b387");

pub type CharacterMaterial = ExtendedMaterial<StandardMaterial, CharacterShading>;

pub struct CharacterMaterialPlugin;

impl Plugin for CharacterMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "character_material.wgsl", Shader::from_wgsl);
        app.add_plugins((
            MaterialPlugin::<CharacterMaterial>::default(),
            crate::face_material::FaceMaterialPlugin,
        ))
        .add_systems(
            PostUpdate,
            share_cloud_layers
                .after(update_clouds)
                .before(TransformSystems::Propagate),
        );
    }
}

/// Bindless like `StandardMaterial`, so characters and held things batch.
#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
#[data(100, CharacterParams, binding_array(108))]
#[bindless(index_table(range(100..107), binding(107)))]
pub struct CharacterShading {
    pub params: CharacterParams,
    /// The cloud shadow's texture (see `clouds.rs`).
    #[texture(101)]
    #[sampler(102)]
    pub shadow_map: Handle<Image>,
    /// The shared look values (`look::LookTexture`).
    #[texture(103)]
    pub look: Handle<Image>,
    /// Extra maps of the material (red: ambient occlusion, green: the eyes'
    /// shadow, blue: the skin's transmission), white without.
    #[texture(104)]
    #[sampler(105)]
    pub maps: Option<Handle<Image>>,
    /// The environment's mean brightness (`deferred_light::CubeMean`), as
    /// `ObjectShading::cube_mean`.
    #[texture(106, sample_type = "float", filterable = false)]
    pub cube_mean: Handle<Image>,
}

/// See `CharacterParams` in `character_material.wgsl`.
#[derive(ShaderType, Clone, Debug, PartialEq)]
pub struct CharacterParams {
    pub clouds: CloudParams,
    /// x: `uking_chara_size`, y: `uking_material_behave`, z: 1 when the
    /// extra maps carry the skin's transmission, w: the maps' UV set.
    pub kind: Vec4,
    /// The behaviour's colour (`const_colorN` of behave 100 + N).
    pub behave: Vec4,
    /// How the specular mask was turned into roughness (roughness without
    /// and with a full mask, xy; the metal at a full metal mask, z), so the
    /// shader can read the mask back; the highlights' strength (w,
    /// `uking_grossy_intensity`).
    pub gloss: Vec4,
}

impl From<&CharacterShading> for CharacterParams {
    fn from(shading: &CharacterShading) -> Self {
        shading.params.clone()
    }
}

impl MaterialExtension for CharacterShading {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// What every character material shares: the clouds (their shadows) and the
/// look texture.
#[derive(SystemParam)]
pub struct CharacterShared<'w> {
    clouds: Option<Res<'w, CloudShadows>>,
    look: Option<Res<'w, LookTexture>>,
    cube_mean: Option<Res<'w, CubeMean>>,
}

impl CharacterShared<'_> {
    /// The clouds' shadows, if there are clouds.
    pub fn clouds(&self) -> Option<&CloudShadows> {
        self.clouds.as_deref()
    }

    /// The shared look texture, if the look is running.
    pub fn look(&self) -> Option<&LookTexture> {
        self.look.as_deref()
    }

    /// The shading every character material starts from.
    pub fn template(&self) -> CharacterShading {
        // The same cloud shadows as the other models' (none without clouds).
        let clouds = self.clouds.as_deref();
        CharacterShading {
            params: CharacterParams {
                clouds: clouds.map_or_else(CloudParams::without_shadows, |c| c.params.clone()),
                kind: Vec4::ZERO,
                behave: Vec4::ZERO,
                gloss: Vec4::ZERO,
            },
            shadow_map: clouds.map(|c| c.shadow_map.clone()).unwrap_or_default(),
            look: self.look.as_ref().map(|l| l.0.clone()).unwrap_or_default(),
            maps: None,
            cube_mean: self
                .cube_mean
                .as_ref()
                .map(|m| m.0.clone())
                .unwrap_or_default(),
        }
    }
}

impl CharacterShading {
    /// The template for a material that says `look` about its shading, whose
    /// masks became roughness and metal by `gloss`, with its extra `maps`
    /// (read with the second UV set if `maps_uv1`).
    pub fn for_material(
        &self,
        look: &MaterialLook,
        gloss: Gloss,
        maps: Option<Handle<Image>>,
        maps_uv1: bool,
    ) -> Self {
        let behave = look.behave.color.map_or(Vec4::ZERO, Vec4::from);
        let mut shading = self.clone();
        let transmission = if look.transmission { 1.0 } else { 0.0 };
        shading.params.kind = Vec4::new(
            look.chara_size,
            look.behave.code as f32,
            transmission,
            if maps_uv1 { 1.0 } else { 0.0 },
        );
        shading.maps = maps;
        shading.params.behave = behave;
        shading.params.gloss = Vec4::new(
            gloss.roughness.0,
            gloss.roughness.1,
            gloss.metal,
            // SI-LGT-17: character saturation, gloss and AO are our fits.
            look.gloss_intensity,
        );
        shading
    }
}

/// Gives the character materials the clouds' current layers and shadow
/// when they change (like
/// `clouds::share_layers` does for the other materials).
fn share_cloud_layers(
    shadows: Option<Res<CloudShadows>>,
    mut shared: Local<u32>,
    mut materials: ResMut<Assets<CharacterMaterial>>,
) {
    let Some(shadows) = shadows else { return };
    if *shared == shadows.revision {
        return;
    }
    *shared = shadows.revision;
    for (_, material) in materials.iter_mut() {
        let params = &mut material.extension.params.clouds;
        // Materials made without clouds keep taking none.
        if params.shadow.y > 0.0 {
            params.upper = shadows.params.upper.clone();
            params.lower = shadows.params.lower.clone();
            crate::clouds::share_shadow(params, &shadows.params);
        }
    }
}
