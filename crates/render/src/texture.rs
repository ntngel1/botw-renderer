//! Baked textures (`asset_format::texture::Texture`, KTX2) as Bevy images,
//! in their own (usually compressed) formats. The helpers of
//! the original renderer's `models.rs` (`texture_format`, `sampler`, `gpu_image`,
//! `texture_layer_image`), working on the baked texture instead of the
//! dump's `TextureImage`.

use asset_format::texture::{Format, Texture};
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};

fn texture_format(format: Format) -> Option<TextureFormat> {
    Some(match format {
        Format::Rgba8 { srgb: true } => TextureFormat::Rgba8UnormSrgb,
        Format::Rgba8 { srgb: false } => TextureFormat::Rgba8Unorm,
        Format::Bc1 { srgb: true } => TextureFormat::Bc1RgbaUnormSrgb,
        Format::Bc1 { srgb: false } => TextureFormat::Bc1RgbaUnorm,
        Format::Bc2 { srgb: true } => TextureFormat::Bc2RgbaUnormSrgb,
        Format::Bc2 { srgb: false } => TextureFormat::Bc2RgbaUnorm,
        Format::Bc3 { srgb: true } => TextureFormat::Bc3RgbaUnormSrgb,
        Format::Bc3 { srgb: false } => TextureFormat::Bc3RgbaUnorm,
        Format::Bc4 { signed: false } => TextureFormat::Bc4RUnorm,
        Format::Bc4 { signed: true } => TextureFormat::Bc4RSnorm,
        Format::Bc5 { signed: false } => TextureFormat::Bc5RgUnorm,
        Format::Bc5 { signed: true } => TextureFormat::Bc5RgSnorm,
        // Effect textures (the original renderer's models never use them).
        Format::R8 => TextureFormat::R8Unorm,
        Format::Rg8 => TextureFormat::Rg8Unorm,
        // the original renderer's `Format::Other`: formats its textures never use.
        Format::Rgba16Float => return None,
    })
}

// SI-MAT-01: alpha test and samplers are not the FMAT ones.
pub(crate) fn sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    })
}

/// A GPU image in the texture's own (usually compressed) format.
pub fn gpu_image(texture: &Texture, _color: bool) -> Option<Image> {
    let format = texture_format(texture.format)?;
    // wgpu needs whole blocks at level 0: round odd sizes up (the stored
    // blocks already cover them; the texture stretches by under 4 pixels).
    let (width, height) = if texture.format.is_block_compressed() {
        (
            texture.width.next_multiple_of(4),
            texture.height.next_multiple_of(4),
        )
    } else {
        (texture.width, texture.height)
    };
    let mip_levels = if (width, height) == (texture.width, texture.height) {
        texture.mip_levels
    } else {
        1
    };
    let data = if mip_levels == texture.mip_levels {
        texture.data.clone()
    } else {
        texture.data[..texture.level_bytes(0) * texture.layers as usize].to_vec()
    };
    Some(Image {
        data: Some(data),
        data_order: TextureDataOrder::MipMajor,
        texture_descriptor: TextureDescriptor {
            label: None,
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: texture.layers,
            },
            mip_level_count: mip_levels,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        sampler: sampler(),
        texture_view_descriptor: None,
        asset_usage: RenderAssetUsages::RENDER_WORLD,
        copy_on_resize: false,
    })
}

/// One array layer of a texture, with its mips, as a 2D GPU image.
pub fn texture_layer_image(texture: &Texture, layer: u32) -> Option<Image> {
    let mut image = gpu_image(texture, true)?;
    let levels = image.texture_descriptor.mip_level_count;
    let data: Option<Vec<Vec<u8>>> = (0..levels)
        .map(|level| texture.layer_data(level, layer).map(<[u8]>::to_vec))
        .collect();
    image.data = Some(data?.concat());
    image.texture_descriptor.size.depth_or_array_layers = 1;
    Some(image)
}

/// A baked texture and the name of the file it was read from: what the
/// dump's `TextureImage` gave the original renderer (its name and texels).
pub struct NamedTexture {
    pub name: String,
    pub texture: Texture,
}

impl std::ops::Deref for NamedTexture {
    type Target = Texture;

    fn deref(&self) -> &Texture {
        &self.texture
    }
}

/// Half the size of an RGBA8 image (2×2 box filter, edges clamped): the
/// next mip level. the original format parser's `bfres::bc::downsample`.
pub fn downsample(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let (nw, nh) = ((width / 2).max(1), (height / 2).max(1));
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; nw as usize * nh as usize * 4];
    for y in 0..nh as usize {
        for x in 0..nw as usize {
            for c in 0..4 {
                let at = |xx: usize, yy: usize| {
                    u32::from(rgba[(yy.min(h - 1) * w + xx.min(w - 1)) * 4 + c])
                };
                let sum = at(2 * x, 2 * y)
                    + at(2 * x + 1, 2 * y)
                    + at(2 * x, 2 * y + 1)
                    + at(2 * x + 1, 2 * y + 1);
                out[(y * nw as usize + x) * 4 + c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    (out, nw, nh)
}
