//! Procedural generation of a spherical planet using fractal noise.
//!
//! The world is a cube of voxels; the planet is the ball of solid matter centred in
//! it. A voxel is solid when its distance from the planet centre is less than the
//! local surface radius — a base radius plus a 3-D fractal Brownian-motion field
//! sampled over the surface direction, giving continents and mountains.
//!
//! Solidity and material are evaluated **procedurally per voxel** (no pre-built
//! octree), so the whole planet is never materialised at once — meshing only ever
//! samples the voxels near the camera. This keeps generation cost independent of the
//! planet's size.

use noise::{Fbm, MultiFractal, NoiseFn, Perlin};

use crate::voxel::VoxelMaterial;

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

    /// Distance from the planet centre to voxel `(x, y, z)`'s centre, and the unit
    /// direction to it (a fallback direction exactly at the centre).
    fn voxel_distance_dir(&self, x: i64, y: i64, z: i64) -> (f64, [f64; 3]) {
        let px = x as f64 + 0.5 - self.center;
        let py = y as f64 + 0.5 - self.center;
        let pz = z as f64 + 0.5 - self.center;
        let d = (px * px + py * py + pz * pz).sqrt();
        if d < 1.0e-6 {
            (0.0, [0.0, 1.0, 0.0])
        } else {
            (d, [px / d, py / d, pz / d])
        }
    }

    /// Is the voxel at `(x, y, z)` inside the planet?
    pub fn is_solid(&self, x: i64, y: i64, z: i64) -> bool {
        let (d, dir) = self.voxel_distance_dir(x, y, z);
        d < self.surface_radius(dir)
    }

    /// The material of the voxel at `(x, y, z)`, or `None` if it is outside the planet.
    pub fn material_at_voxel(&self, x: i64, y: i64, z: i64) -> Option<VoxelMaterial> {
        let (d, dir) = self.voxel_distance_dir(x, y, z);
        let sr = self.surface_radius(dir);
        (d < sr).then(|| self.material_at(d, sr, dir))
    }

    /// Minimum and maximum distance from the planet centre to any point of the cubic
    /// region with minimum corner `(x0, y0, z0)` and edge length `size` (voxels).
    fn region_distance_range(&self, x0: i64, y0: i64, z0: i64, size: i64) -> (f64, f64) {
        let mut min_sq = 0.0;
        let mut max_sq = 0.0;
        for (lo, hi) in [(x0, x0 + size), (y0, y0 + size), (z0, z0 + size)] {
            let (lo, hi) = (lo as f64, hi as f64);
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

    /// Might the cubic region straddle the planet surface (i.e. produce any mesh)?
    /// Conservative: regions entirely outside or entirely inside the noise shell are
    /// rejected, so the LOD walk can skip them (and their whole subtree) without
    /// descending. This is what keeps the chunk walk proportional to surface area
    /// rather than world volume.
    pub fn region_has_surface(&self, x0: i64, y0: i64, z0: i64, size: i64) -> bool {
        let (min_d, max_d) = self.region_distance_range(x0, y0, z0, size);
        min_d < self.base_radius + self.amplitude && max_d > self.base_radius - self.amplitude
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
}
