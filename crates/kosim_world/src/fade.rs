//! Terrain chunk material: triplanar texture-array shading plus a dithered LOD
//! crossfade, and procedural generation of the texture array.
//!
//! Chunks are drawn with [`ChunkMaterial`] — a [`StandardMaterial`] extended with a
//! terrain texture array + a screen-door dither (see `assets/shaders/chunk_fade.wgsl`).
//! Each vertex carries its material's texture-array layer in the red vertex-colour
//! channel; the fragment shader triplanar-samples that layer (the isosurface has no
//! consistent UVs, so texturing is projected from world XYZ).
//!
//! The dither `fade` value crossfades LODs: a fresh chunk dissolves in (`fade`
//! 0 → 1) while the chunk it replaces stays fully opaque as a backing until it
//! despawns, so the incoming chunk's dither holes reveal the old terrain, never the
//! background.

use bevy::asset::{Asset, Handle, RenderAssetUsages};
use bevy::color::ColorToComponents;
use bevy::ecs::component::Component;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, StandardMaterial};
use bevy::reflect::Reflect;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

use crate::voxel::{MATERIAL_COUNT, VoxelMaterial};

/// The terrain chunk material: `StandardMaterial` plus the texture-array + dither
/// extension.
pub type ChunkMaterial = ExtendedMaterial<StandardMaterial, ChunkFade>;

/// Edge length in texels of each per-material terrain texture.
pub const TEXTURE_SIZE: u32 = 32;

/// Time in seconds for a new chunk to fully dither in.
pub const FADE_SECONDS: f32 = 0.4;
/// How long a replaced chunk stays (opaque, as a backing) before despawning. Kept
/// just above [`FADE_SECONDS`] so replacements are solid before it is removed, while
/// not piling up too many backing chunks during fast camera movement.
pub const RETIRE_SECONDS: f32 = 0.5;

/// StandardMaterial extension: the terrain texture array plus the per-chunk fade.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct ChunkFade {
    #[uniform(100)]
    pub fade: f32,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub array: Option<Handle<Image>>,
}

impl MaterialExtension for ChunkFade {
    fn fragment_shader() -> ShaderRef {
        "shaders/chunk_fade.wgsl".into()
    }
}

/// Per-chunk fade state. A fresh chunk dithers in (`value` 0 → 1). When replaced it
/// becomes `retiring`: it snaps to fully opaque and stays as a solid backing (so the
/// incoming chunk's dither holes reveal it, not the background) until `timer` reaches
/// [`RETIRE_SECONDS`], then it despawns.
#[derive(Component)]
pub struct Fade {
    pub value: f32,
    pub retiring: bool,
    pub timer: f32,
}

/// Tileable 2-octave value noise in `[0, 1]` used to give each texture some grain.
fn value_noise(x: f32, y: f32, layer: u32, period: i32) -> f32 {
    fn corner(gx: i32, gy: i32, layer: u32, period: i32) -> f32 {
        let ix = gx.rem_euclid(period) as u32;
        let iy = gy.rem_euclid(period) as u32;
        let mut h = ix
            .wrapping_mul(374761393)
            ^ iy.wrapping_mul(668265263)
            ^ layer.wrapping_mul(2246822519);
        h = (h ^ (h >> 13)).wrapping_mul(1274126177);
        (h & 0xffff) as f32 / 65535.0
    }
    let gx = x.floor() as i32;
    let gy = y.floor() as i32;
    let fx = x - gx as f32;
    let fy = y - gy as f32;
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);
    let a = corner(gx, gy, layer, period);
    let b = corner(gx + 1, gy, layer, period);
    let c = corner(gx, gy + 1, layer, period);
    let d = corner(gx + 1, gy + 1, layer, period);
    let ab = a + (b - a) * ux;
    let cd = c + (d - c) * ux;
    ab + (cd - ab) * uy
}

/// Build the terrain texture array: one procedural [`TEXTURE_SIZE`]² sRGB texture per
/// [`VoxelMaterial`], stacked as array layers in [`VoxelMaterial::layer`] order. Each
/// texture is its base colour modulated by tileable value noise for surface grain.
pub fn build_terrain_texture_array() -> Image {
    let size = TEXTURE_SIZE;
    let mut data: Vec<u8> = Vec::with_capacity((MATERIAL_COUNT * size * size * 4) as usize);

    for material in VoxelMaterial::all() {
        let layer = material.layer();
        let base = material.srgb().to_srgba().to_f32_array();
        for py in 0..size {
            for px in 0..size {
                let fx = px as f32;
                let fy = py as f32;
                // Two tiling octaves (periods divide TEXTURE_SIZE so the tile wraps).
                let n1 = value_noise(fx * 8.0 / size as f32, fy * 8.0 / size as f32, layer, 8);
                let n2 = value_noise(
                    fx * 16.0 / size as f32,
                    fy * 16.0 / size as f32,
                    layer * 7 + 1,
                    16,
                );
                let brightness = 0.72 + 0.56 * (0.65 * n1 + 0.35 * n2);
                for c in 0..3 {
                    data.push(((base[c] * brightness).clamp(0.0, 1.0) * 255.0) as u8);
                }
                data.push(255);
            }
        }
    }

    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: MATERIAL_COUNT,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Repeat so triplanar UVs tile, and nearest filtering for a crisp pixelated look.
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Nearest,
        mipmap_filter: ImageFilterMode::Nearest,
        ..Default::default()
    });
    image
}
