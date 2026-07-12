//! Voxel value types stored in the octree leaves.
//!
//! A voxel is the smallest unit of world matter. The physical edge length of a
//! leaf voxel is [`crate::WorldConfig::min_voxel_size`] (0.25 units by default);
//! everything larger is a merged region in the octree.

use bevy::color::{Color, ColorToComponents};

/// The kind of matter occupying a voxel. Determines the vertex colour used when
/// meshing. `Empty` is never stored as a value; absence of matter is represented
/// by [`crate::octree::OctNode::Empty`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum VoxelMaterial {
    Stone,
    Dirt,
    Grass,
    Sand,
    Snow,
}

/// Number of distinct voxel materials — the layer count of the terrain texture array.
pub const MATERIAL_COUNT: u32 = 5;

impl VoxelMaterial {
    /// Layer index of this material in the terrain texture array (`0..MATERIAL_COUNT`).
    /// Must match the order textures are packed in `fade::build_terrain_texture_array`.
    pub fn layer(self) -> u32 {
        self as u32
    }

    /// Every material in layer order.
    pub fn all() -> [VoxelMaterial; MATERIAL_COUNT as usize] {
        [
            VoxelMaterial::Stone,
            VoxelMaterial::Dirt,
            VoxelMaterial::Grass,
            VoxelMaterial::Sand,
            VoxelMaterial::Snow,
        ]
    }

    /// The base sRGB colour for this material.
    pub fn srgb(self) -> Color {
        match self {
            VoxelMaterial::Stone => Color::srgb(0.42, 0.42, 0.45),
            VoxelMaterial::Dirt => Color::srgb(0.35, 0.24, 0.15),
            VoxelMaterial::Grass => Color::srgb(0.28, 0.52, 0.20),
            VoxelMaterial::Sand => Color::srgb(0.76, 0.70, 0.50),
            VoxelMaterial::Snow => Color::srgb(0.92, 0.94, 0.98),
        }
    }

    /// Linear RGBA suitable for a mesh `ATTRIBUTE_COLOR` vertex attribute.
    ///
    /// Bevy interprets vertex colours as linear values, so we convert from the
    /// authored sRGB colour once here.
    pub fn linear_rgba(self) -> [f32; 4] {
        self.srgb().to_linear().to_f32_array()
    }
}

/// A single unit of solid world matter.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Voxel {
    pub material: VoxelMaterial,
}

impl Voxel {
    pub const fn new(material: VoxelMaterial) -> Self {
        Self { material }
    }
}
