//! Procedural generation of a spherical planet using fractal noise.
//!
//! The world is a cube of voxels; the planet is the ball of solid matter centred in
//! it. A voxel is solid when its distance from the planet centre is less than the
//! local surface radius — a base radius plus a 3-D fractal Brownian-motion field
//! sampled over the surface direction, giving continents and mountains. The octree
//! is built recursively: whole regions that are entirely inside the planet collapse
//! to a stone leaf and regions entirely outside collapse to empty, so only the thin
//! surface shell recurses to individual voxels.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use crate::octree::OctNode;
use crate::voxel::{Voxel, VoxelMaterial};

/// Voxels of grass/snow/sand at the very surface.
const TOPSOIL: f64 = 1.0;
/// Total depth of the non-stone surface band (grass/sand/snow over dirt).
const SURFACE_BAND: f64 = 4.0;

/// Generates a planet octree centred in a `dim`-voxel cube.
pub struct PlanetGenerator {
    /// Planet centre in voxel coordinates (the cube centre).
    center: f64,
    /// Mean surface radius in voxels.
    base_radius: f64,
    /// Peak-to-mean relief of the surface noise, in voxels.
    amplitude: f64,
    fbm: Fbm<Perlin>,
}

impl PlanetGenerator {
    pub fn new(dim: i64, seed: u32) -> Self {
        let fbm = Fbm::<Perlin>::new(seed)
            .set_octaves(4)
            .set_persistence(0.5)
            .set_frequency(1.0);
        let base_radius = dim as f64 * 0.42;
        Self {
            center: dim as f64 / 2.0,
            base_radius,
            amplitude: base_radius * 0.10,
            fbm,
        }
    }

    /// Surface radius (voxels) in the direction of the unit vector `dir`.
    fn surface_radius(&self, dir: [f64; 3]) -> f64 {
        // A few large features across the sphere.
        const FREQ: f64 = 2.5;
        let n = self
            .fbm
            .get([dir[0] * FREQ, dir[1] * FREQ, dir[2] * FREQ]);
        self.base_radius + n * self.amplitude
    }

    /// Minimum and maximum distance from the planet centre to any point of the cubic
    /// region with minimum corner `(x0, y0, z0)` and edge length `size` (voxels).
    fn region_distance_range(&self, x0: i64, y0: i64, z0: i64, size: i64) -> (f64, f64) {
        let mut min_sq = 0.0;
        let mut max_sq = 0.0;
        for (lo, hi) in [(x0, x0 + size), (y0, y0 + size), (z0, z0 + size)] {
            let lo = lo as f64;
            let hi = hi as f64;
            // Nearest point on this axis' span to the centre (0 if the centre is
            // inside the span), and the farther of the two ends.
            let near = if self.center < lo {
                lo - self.center
            } else if self.center > hi {
                self.center - hi
            } else {
                0.0
            };
            let far = (self.center - lo).abs().max((self.center - hi).abs());
            min_sq += near * near;
            max_sq += far * far;
        }
        (min_sq.sqrt(), max_sq.sqrt())
    }

    /// Material for a solid voxel at distance `d` from the centre whose column
    /// surface radius is `sr`, in the direction `dir`.
    fn material_at(&self, d: f64, sr: f64, dir: [f64; 3]) -> VoxelMaterial {
        let depth = sr - d;
        if depth < TOPSOIL {
            // Snowy near the poles, sandy in the low equatorial basins, else grass.
            let latitude = dir[1].abs();
            if latitude > 0.8 {
                VoxelMaterial::Snow
            } else if sr < self.base_radius - self.amplitude * 0.4 {
                VoxelMaterial::Sand
            } else {
                VoxelMaterial::Grass
            }
        } else if depth < SURFACE_BAND {
            VoxelMaterial::Dirt
        } else {
            VoxelMaterial::Stone
        }
    }

    /// Recursively build the octree for the cube with minimum corner `(x0, y0, z0)`
    /// and edge length `size` voxels.
    pub fn build(&self, x0: i64, y0: i64, z0: i64, size: i64) -> OctNode {
        let (min_d, max_d) = self.region_distance_range(x0, y0, z0, size);

        // Entirely outside the planet: empty.
        if min_d >= self.base_radius + self.amplitude {
            return OctNode::Empty;
        }
        // Entirely inside, below the surface band: solid stone. The key pruning step.
        if max_d <= self.base_radius - self.amplitude - SURFACE_BAND {
            return OctNode::Leaf(Voxel::new(VoxelMaterial::Stone));
        }

        // A single voxel straddling the shell: resolve it at its centre.
        if size == 1 {
            let px = x0 as f64 + 0.5 - self.center;
            let py = y0 as f64 + 0.5 - self.center;
            let pz = z0 as f64 + 0.5 - self.center;
            let d = (px * px + py * py + pz * pz).sqrt();
            if d < 1.0e-6 {
                return OctNode::Leaf(Voxel::new(VoxelMaterial::Stone));
            }
            let dir = [px / d, py / d, pz / d];
            let sr = self.surface_radius(dir);
            return if d < sr {
                OctNode::Leaf(Voxel::new(self.material_at(d, sr, dir)))
            } else {
                OctNode::Empty
            };
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

/// Generate the planet octree for a world of `dim` voxels per axis.
pub fn generate_terrain(dim: i64, seed: u32) -> OctNode {
    let generator = PlanetGenerator::new(dim, seed);
    generator.build(0, 0, 0, dim)
}
