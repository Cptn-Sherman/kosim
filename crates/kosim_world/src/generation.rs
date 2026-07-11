//! Procedural generation of a sample voxel scene using fractal noise.
//!
//! Terrain is heightmap based: a fractal Brownian-motion (fBm) Perlin field is
//! sampled per column to produce a surface height, then the octree is built
//! top-down so that homogeneous regions (deep stone, open sky) never recurse to
//! individual voxels. This keeps generation fast and produces an already-
//! compressed tree.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use crate::octree::OctNode;
use crate::voxel::{Voxel, VoxelMaterial};

/// Number of surface voxels coloured as grass/snow (the very top layer).
const TOPSOIL_GRASS: i32 = 1;
/// Additional voxels below the grass coloured as dirt before stone begins.
const TOPSOIL_DIRT: i32 = 3;
/// Total depth of the non-stone surface band.
const SURFACE_BAND: i32 = TOPSOIL_GRASS + TOPSOIL_DIRT;

/// Builds a procedural heightmap terrain and returns the octree root.
///
/// * `dim` — world edge length in voxels (a power of two, `2^max_depth`).
/// * `seed` — noise seed for reproducibility.
pub struct TerrainGenerator {
    dim: i64,
    /// Per-column surface height in voxels, indexed `x * dim + z`.
    heights: Vec<i32>,
    /// Height above which surfaces are snow-capped.
    snow_line: i32,
    /// Height below which flat surfaces are sandy beaches.
    sand_line: i32,
}

impl TerrainGenerator {
    pub fn new(dim: i64, seed: u32) -> Self {
        // A handful of octaves gives rolling hills with some fine detail.
        let fbm = Fbm::<Perlin>::new(seed)
            .set_octaves(5)
            .set_persistence(0.5)
            .set_frequency(1.0);

        // The terrain occupies most of the vertical range: a wide amplitude band
        // gives dramatic relief (deep valleys to tall, near-top peaks) while
        // still leaving solid ground below and a sliver of open air above for the
        // LOD to work with.
        let min_h = (dim as f64 * 0.15) as i32;
        let max_h = (dim as f64 * 0.92) as i32;
        // Spatial scale: a few hills across the whole world.
        let scale = 3.0_f64;

        let mut heights = vec![0i32; (dim * dim) as usize];
        for x in 0..dim {
            for z in 0..dim {
                let nx = x as f64 / dim as f64 * scale;
                let nz = z as f64 / dim as f64 * scale;
                // fBm returns roughly [-1, 1]; remap to [0, 1].
                let n = (fbm.get([nx, nz]) * 0.5 + 0.5).clamp(0.0, 1.0);
                // Bias with a mild curve so peaks are less common than plains.
                let n = n * n * (3.0 - 2.0 * n); // smoothstep
                let h = min_h + (n * (max_h - min_h) as f64) as i32;
                heights[(x * dim + z) as usize] = h.clamp(1, dim as i32);
            }
        }

        let snow_line = (max_h as f64 * 0.82) as i32;
        let sand_line = min_h + 2;

        Self {
            dim,
            heights,
            snow_line,
            sand_line,
        }
    }

    #[inline]
    fn height(&self, x: i64, z: i64) -> i32 {
        self.heights[(x * self.dim + z) as usize]
    }

    /// Minimum and maximum surface height over the square column footprint
    /// `[x0, x0+size) x [z0, z0+size)`.
    fn column_min_max(&self, x0: i64, z0: i64, size: i64) -> (i32, i32) {
        let mut lo = i32::MAX;
        let mut hi = i32::MIN;
        for x in x0..x0 + size {
            for z in z0..z0 + size {
                let h = self.height(x, z);
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        (lo, hi)
    }

    /// Material for the solid voxel at `(x, y, z)` given its column surface `h`.
    fn material_at(&self, y: i64, h: i32) -> VoxelMaterial {
        let depth_from_top = h - 1 - y as i32;
        if depth_from_top < TOPSOIL_GRASS {
            if h >= self.snow_line {
                VoxelMaterial::Snow
            } else if h <= self.sand_line {
                VoxelMaterial::Sand
            } else {
                VoxelMaterial::Grass
            }
        } else if depth_from_top < SURFACE_BAND {
            VoxelMaterial::Dirt
        } else {
            VoxelMaterial::Stone
        }
    }

    /// Recursively build the octree for the cube whose minimum corner is
    /// `(x0, y0, z0)` with edge length `size` voxels.
    pub fn build(&self, x0: i64, y0: i64, z0: i64, size: i64) -> OctNode {
        let (hmin, hmax) = self.column_min_max(x0, z0, size);

        // Entire region sits above every column's surface: pure air.
        if y0 as i32 >= hmax {
            return OctNode::Empty;
        }

        // A single voxel: resolve directly.
        if size == 1 {
            let h = self.height(x0, z0);
            return if (y0 as i32) < h {
                OctNode::Leaf(Voxel::new(self.material_at(y0, h)))
            } else {
                OctNode::Empty
            };
        }

        // Entire region is deep below the surface band: uniform stone. This is
        // the key pruning step that keeps deep ground from recursing to voxels.
        if (y0 + size) as i32 <= hmin - SURFACE_BAND {
            return OctNode::Leaf(Voxel::new(VoxelMaterial::Stone));
        }

        // Otherwise the region straddles the surface: subdivide.
        let half = size / 2;
        let children = std::array::from_fn(|i| {
            let ox = (i & 1) as i64 * half;
            let oy = ((i >> 1) & 1) as i64 * half;
            let oz = ((i >> 2) & 1) as i64 * half;
            self.build(x0 + ox, y0 + oy, z0 + oz, half)
        });
        OctNode::from_children(children)
    }
}

/// Generate the sample terrain octree for a world of `dim` voxels per axis.
pub fn generate_terrain(dim: i64, seed: u32) -> OctNode {
    let generator = TerrainGenerator::new(dim, seed);
    generator.build(0, 0, 0, dim)
}
