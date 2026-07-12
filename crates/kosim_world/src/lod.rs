//! Camera-driven level-of-detail chunk selection and isosurface meshing.
//!
//! The world is split into a distance-driven set of leaf *chunks*; [`desired_chunks`]
//! chooses which chunks (and at what LOD) the camera should see, enforcing a 2:1
//! balance so Transvoxel transition cells only ever bridge a single LOD jump.
//! [`mesh_one_chunk`] meshes one chunk with the `transvoxel` crate, fed a binary
//! `+1/-1` field so every edge crossing lands at the exact midpoint (`t = 0.5`) —
//! grid-aligned, no density interpolation. It runs off the main thread.
//!
//! Normals are recomputed from the finest-resolution occupancy gradient
//! ([`field_normal`]) so chunks of any LOD share identical normals where they meet
//! (no shading seam), and each vertex's material is the topsoil found by marching in
//! from the surface ([`surface_material`]), consistent across LODs.

use bevy::asset::RenderAssetUsages;
use bevy::platform::collections::HashSet;
use bevy::math::{IVec3, Vec3};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use transvoxel::generic_mesh::GenericMeshBuilder;
use transvoxel::prelude::{Block, extract_from_fn};
use transvoxel::transition_sides::{TransitionSide, TransitionSides, no_side};
use transvoxel::voxel_source::BlockDims;

use crate::VoxelWorld;
use crate::voxel::VoxelMaterial;

/// Marching cells per axis in a leaf chunk. A region is meshed with cell step
/// `region_voxels / CELLS_PER_CHUNK`, so a chunk keeps a bounded triangle budget
/// however large a volume it covers at a coarse LOD. A finest chunk (the ones that
/// get physics colliders) has `size == CELLS_PER_CHUNK` voxels (cell step 1).
pub const CELLS_PER_CHUNK: i64 = 16;

/// Identifies a leaf chunk to render: its minimum-corner voxel, its edge length in
/// voxels (which fixes the LOD), and a bitmask of the faces that need Transvoxel
/// transition cells because the neighbour there is one level finer. The mask is part
/// of the key so a chunk is re-meshed when its neighbours' LODs change.
///
/// Side bits follow [`transvoxel::transition_sides::TransitionSide`] order:
/// `1<<0`=LowX(-X), `1<<1`=HighX(+X), `1<<2`=LowY, `1<<3`=HighY, `1<<4`=LowZ, `1<<5`=HighZ.
pub type ChunkKey = (IVec3, i64, u8);

/// Geomorphing: a chunk's vertices slide between the parent-LOD surface and their
/// true position as the camera approaches (see `chunk_fade.wgsl`). The morph is
/// driven by camera distance normalised by the chunk's own LOD range: a chunk of
/// world size `S` is a leaf while `S/θ ≤ distance < 2S/θ` (θ = `lod_threshold`), so
/// its normalised distance ratio spans `[1, 2)`. Vertices are at full detail below
/// [`MORPH_START_RATIO`] and fully morphed to the parent surface above
/// [`MORPH_END_RATIO`]. The band ends well before 2 so that by the time a chunk is
/// actually swapped for its parent (or spawned in place of it) — including LOD
/// hysteresis and streaming latency — every vertex already sits on the parent
/// surface, making the swap geometrically invisible.
pub const MORPH_START_RATIO: f32 = 1.3;
pub const MORPH_END_RATIO: f32 = 1.6;

/// The camera-distance interval `(start, end)` over which a chunk of `size` voxels
/// morphs toward the parent-LOD surface.
pub fn morph_band(world: &VoxelWorld, size: i64) -> (f32, f32) {
    let world_size = size as f32 * world.config.min_voxel_size;
    let per = world_size / world.config.lod_threshold;
    (MORPH_START_RATIO * per, MORPH_END_RATIO * per)
}

/// The geomorph factor (0 = full detail, 1 = parent surface) for a chunk whose
/// centre the camera sees from `camera_pos`. Computed on the CPU and fed to the
/// shader as a per-chunk uniform — rather than per-vertex from the view — because
/// the shadow pass's `view` is the *light*, not the camera; sharing one value keeps
/// rendered and shadow-map geometry identical. Quantised so the material uniform
/// only changes (and re-uploads) at coarse steps.
pub fn morph_factor(world: &VoxelWorld, key: ChunkKey, camera_pos: Vec3) -> f32 {
    let (region_min, size, _) = key;
    let mvs = world.config.min_voxel_size;
    let center = world.config.origin + (region_min.as_vec3() + Vec3::splat(size as f32 * 0.5)) * mvs;
    let (d0, d1) = morph_band(world, size);
    let t = ((center.distance(camera_pos) - d0) / (d1 - d0)).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t); // smoothstep
    (s * 64.0).round() / 64.0
}

/// The set of leaf chunks the camera should currently see. Walks the chunk octree
/// choosing an LOD per region by distance, enforces a 2:1 balance (face-adjacent
/// chunks differ by at most one level, which Transvoxel transition cells require),
/// then tags each chunk with the faces where its neighbour is one level finer. Cheap
/// — no meshing.
pub fn desired_chunks(world: &VoxelWorld, camera_pos: Vec3) -> Vec<ChunkKey> {
    let mut leaves = HashSet::new();
    collect_leaves(world, IVec3::ZERO, world.dim, camera_pos, &mut leaves);
    balance_leaves(&mut leaves, world.dim);

    leaves
        .iter()
        .map(|&(region_min, size)| {
            let sides = transition_sides(&leaves, region_min, size, world.dim);
            (region_min, size, sides)
        })
        .collect()
}

/// Is a region of edge length `world_size` centred at `center` close enough to the
/// camera to warrant subdividing it for more detail?
fn should_subdivide(world: &VoxelWorld, center: Vec3, world_size: f32, camera_pos: Vec3) -> bool {
    let dist = center.distance(camera_pos).max(1.0e-3);
    world_size / dist > world.config.lod_threshold
}

/// Walk the virtual chunk octree, subdividing a region while it is large relative to
/// its camera distance (down to `CELLS_PER_CHUNK` voxels), recording a leaf region
/// `(region_min, size)` wherever subdivision stops.
fn collect_leaves(
    world: &VoxelWorld,
    region_min: IVec3,
    size: i64,
    camera_pos: Vec3,
    out: &mut HashSet<(IVec3, i64)>,
) {
    // Skip regions (and their whole subtree) that contain no surface — most of a
    // planet's volume is empty air or solid interior, and processing those was the
    // bulk of the per-move cost.
    if !world.region_has_surface(region_min, size) {
        return;
    }

    let mvs = world.config.min_voxel_size;
    let world_size = size as f32 * mvs;
    let center =
        world.config.origin + (region_min.as_vec3() + Vec3::splat(size as f32 * 0.5)) * mvs;

    if size > CELLS_PER_CHUNK && should_subdivide(world, center, world_size, camera_pos) {
        let half = (size / 2) as i32;
        for i in 0..8 {
            let offset = IVec3::new(
                (i & 1) as i32 * half,
                ((i >> 1) & 1) as i32 * half,
                ((i >> 2) & 1) as i32 * half,
            );
            collect_leaves(world, region_min + offset, size / 2, camera_pos, out);
        }
    } else {
        out.insert((region_min, size));
    }
}

/// The eight children of a leaf region, each half its size.
fn children(region_min: IVec3, size: i64) -> [(IVec3, i64); 8] {
    let half = (size / 2) as i32;
    std::array::from_fn(|i| {
        let offset = IVec3::new(
            (i & 1) as i32 * half,
            ((i >> 1) & 1) as i32 * half,
            ((i >> 2) & 1) as i32 * half,
        );
        (region_min + offset, size / 2)
    })
}

/// The leaf region that contains voxel point `p`, if any (the partition tiles
/// `[0, dim)` exactly, so at most one size matches). `None` outside the world.
fn leaf_containing(leaves: &HashSet<(IVec3, i64)>, p: IVec3, dim: i64) -> Option<(IVec3, i64)> {
    let mut size = CELLS_PER_CHUNK;
    while size <= dim {
        let rm = IVec3::new(
            (p.x as i64).div_euclid(size) as i32 * size as i32,
            (p.y as i64).div_euclid(size) as i32 * size as i32,
            (p.z as i64).div_euclid(size) as i32 * size as i32,
        );
        if leaves.contains(&(rm, size)) {
            return Some((rm, size));
        }
        size *= 2;
    }
    None
}

/// The six face-neighbour probe points of a leaf, as `(face_bit, point)` just
/// outside each face centre (in voxel coords). Bits follow [`ChunkKey`] order.
fn face_probes(region_min: IVec3, size: i64) -> [(u8, IVec3); 6] {
    let s = size as i32;
    let h = (size / 2) as i32;
    let (x, y, z) = (region_min.x, region_min.y, region_min.z);
    [
        (1 << 0, IVec3::new(x - 1, y + h, z + h)), // LowX
        (1 << 1, IVec3::new(x + s, y + h, z + h)), // HighX
        (1 << 2, IVec3::new(x + h, y - 1, z + h)), // LowY
        (1 << 3, IVec3::new(x + h, y + s, z + h)), // HighY
        (1 << 4, IVec3::new(x + h, y + h, z - 1)), // LowZ
        (1 << 5, IVec3::new(x + h, y + h, z + s)), // HighZ
    ]
}

/// Split any leaf whose face-neighbour is more than one level coarser, until the set
/// is 2:1 balanced. Transvoxel transition cells only bridge a single 2× jump, so an
/// unbalanced 4× jump would still crack.
fn balance_leaves(leaves: &mut HashSet<(IVec3, i64)>, dim: i64) {
    let mut queue: Vec<(IVec3, i64)> = leaves.iter().copied().collect();
    while let Some((region_min, size)) = queue.pop() {
        if !leaves.contains(&(region_min, size)) {
            continue; // already split
        }
        for (_, probe) in face_probes(region_min, size) {
            if let Some((n_min, n_size)) = leaf_containing(leaves, probe, dim) {
                if n_size > 2 * size {
                    leaves.remove(&(n_min, n_size));
                    for child in children(n_min, n_size) {
                        leaves.insert(child);
                        queue.push(child);
                    }
                    // The coarse neighbour changed; re-examine this leaf later too.
                    queue.push((region_min, size));
                }
            }
        }
    }
}

/// The transition-side bitmask for a (balanced) leaf: a bit is set on each face
/// whose neighbour is exactly one level finer (half the size = double resolution).
fn transition_sides(leaves: &HashSet<(IVec3, i64)>, region_min: IVec3, size: i64, dim: i64) -> u8 {
    let mut sides = 0u8;
    for (bit, probe) in face_probes(region_min, size) {
        if let Some((_, n_size)) = leaf_containing(leaves, probe, dim)
            && n_size < size
        {
            sides |= bit;
        }
    }
    sides
}

/// Convert a [`ChunkKey`] side bitmask into the crate's [`TransitionSides`] set.
fn side_flags(sides: u8) -> TransitionSides {
    const ORDER: [TransitionSide; 6] = [
        TransitionSide::LowX,
        TransitionSide::HighX,
        TransitionSide::LowY,
        TransitionSide::HighY,
        TransitionSide::LowZ,
        TransitionSide::HighZ,
    ];
    let mut flags = no_side();
    for (i, side) in ORDER.into_iter().enumerate() {
        if sides & (1 << i) != 0 {
            flags |= side;
        }
    }
    flags
}

/// Mesh one leaf chunk into its own render mesh using Transvoxel, with transition
/// cells on the faces given by `sides`. The field is binary (+1 solid / -1 air) so
/// every edge crossing lands at the exact midpoint (`t = 0.5`) — grid-aligned, no
/// density interpolation. Pure over `world`, so it runs off the main thread.
pub fn mesh_one_chunk(world: &VoxelWorld, region_min: IVec3, size: i64, sides: u8) -> Mesh {
    let mvs = world.config.min_voxel_size;
    let origin = world.config.origin;
    let base = origin + region_min.as_vec3() * mvs;
    let block = Block {
        dims: BlockDims {
            base: [base.x, base.y, base.z],
            size: size as f32 * mvs,
        },
        subdivisions: CELLS_PER_CHUNK as usize,
    };

    // Binary density sampled at grid points. On transition faces the crate samples at
    // half-cell spacing; a finest chunk (step 1) never has a finer neighbour, so those
    // half points always fall on integer voxel coordinates.
    let field = |x: f32, y: f32, z: f32| -> f32 {
        let vx = ((x - origin.x) / mvs).round() as i64;
        let vy = ((y - origin.y) / mvs).round() as i64;
        let vz = ((z - origin.z) / mvs).round() as i64;
        if world.is_solid_voxel(vx, vy, vz) { 1.0 } else { -1.0 }
    };

    let mesh = extract_from_fn(
        field,
        &block,
        0.0_f32,
        side_flags(sides),
        GenericMeshBuilder::new(),
    )
    .build();

    let step = (size / CELLS_PER_CHUNK).max(1);
    to_bevy_mesh(world, mesh, region_min, step)
}

/// World-space centre of the planet (the cube's centre).
fn planet_center(world: &VoxelWorld) -> Vec3 {
    world.config.origin + Vec3::splat(world.dim as f32 * world.config.min_voxel_size * 0.5)
}

/// The surface (topsoil) material at a vertex: march inward from just outside the
/// vertex, **radially** (toward the planet centre), and take the first solid voxel.
/// Marching radially — rather than along the mesh normal, which can be unreliable on
/// coarse chunks — finds the same fine-resolution topsoil voxel at every LOD and on
/// every side of the planet, so a vertex's material never changes with LOD.
fn surface_material(world: &VoxelWorld, p: Vec3, step: i64) -> VoxelMaterial {
    let mvs = world.config.min_voxel_size;
    let origin = world.config.origin;
    let up = (p - planet_center(world)).normalize_or(Vec3::Y); // radial outward
    let march = step as f32 * mvs; // one coarse cell
    let sample_step = mvs * 0.5;
    let start = p + up * march; // safely outside the fine surface
    let count = ((2.0 * march) / sample_step).ceil() as i32 + 1;
    for i in 0..count {
        let sp = start - up * (i as f32 * sample_step);
        let v = (sp - origin) / mvs;
        if let Some(material) =
            world.voxel_material(v.x.floor() as i64, v.y.floor() as i64, v.z.floor() as i64)
        {
            return material;
        }
    }
    VoxelMaterial::Stone
}

/// Outward surface normal at world position `p`, from the trilinear gradient of the
/// binary occupancy field at *finest* resolution. It depends only on the position
/// and the world, never on which chunk (or LOD) is meshing — so two chunks meeting
/// at a vertex compute the identical normal and light without a seam. Trilinear
/// sampling (rather than a raw central difference) keeps the normals smooth.
fn field_normal(world: &VoxelWorld, p: Vec3) -> [f32; 3] {
    let mvs = world.config.min_voxel_size;
    // Position in voxel-*centre* space: integer c samples the centre of voxel c.
    let c = (p - world.config.origin) / mvs - Vec3::splat(0.5);
    let base = c.floor();
    let f = c - base;
    let sample = |ix: f32, iy: f32, iz: f32| -> f32 {
        let solid = world.is_solid_voxel(
            (base.x + ix) as i64,
            (base.y + iy) as i64,
            (base.z + iz) as i64,
        );
        if solid { 1.0 } else { 0.0 }
    };
    let c000 = sample(0.0, 0.0, 0.0);
    let c100 = sample(1.0, 0.0, 0.0);
    let c010 = sample(0.0, 1.0, 0.0);
    let c110 = sample(1.0, 1.0, 0.0);
    let c001 = sample(0.0, 0.0, 1.0);
    let c101 = sample(1.0, 0.0, 1.0);
    let c011 = sample(0.0, 1.0, 1.0);
    let c111 = sample(1.0, 1.0, 1.0);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    // Analytic gradient of the trilinear interpolant.
    let gx = lerp(
        lerp(c100 - c000, c110 - c010, f.y),
        lerp(c101 - c001, c111 - c011, f.y),
        f.z,
    );
    let gy = lerp(
        lerp(c010 - c000, c110 - c100, f.x),
        lerp(c011 - c001, c111 - c101, f.x),
        f.z,
    );
    let gz = lerp(
        lerp(c001 - c000, c101 - c100, f.x),
        lerp(c011 - c010, c111 - c110, f.x),
        f.y,
    );
    // Occupancy increases *into* the solid, so the outward normal is the negated
    // gradient.
    let grad = Vec3::new(gx, gy, gz);
    if grad.length_squared() > 1.0e-8 {
        (-grad.normalize()).to_array()
    } else {
        // Uniform field here (can happen on coarse chunks): fall back to the radial
        // (outward) direction rather than a fixed +Y, which is wrong under the planet.
        (p - planet_center(world)).normalize_or(Vec3::Y).to_array()
    }
}

/// Dense occupancy of the **parent-LOD** sample grid (double-`step`) over one
/// chunk plus a margin, with trilinear evaluation. The parent chunk's mesh puts
/// vertices at midpoints of parent-grid edges — which is exactly the piecewise
/// linear approximation of this field's 0.5 isosurface — so the trilinear crossing
/// is the correct, *continuous* geomorph target. (An earlier version sampled the
/// binary field with nearest-grid rounding; the staircase aliasing made adjacent
/// vertices latch onto different steps and produced spike artifacts whose shadows
/// floated free of the visible terrain.)
struct ParentField {
    /// Minimum corner, in parent-grid units (voxel coords / `parent_step`).
    min: IVec3,
    /// Corners per axis.
    dim: i32,
    parent_step: i64,
    data: Vec<bool>,
}

impl ParentField {
    /// Extra parent cells beyond the chunk so the offset march stays in cache.
    const MARGIN: i32 = 3;

    fn build(world: &VoxelWorld, region_min: IVec3, size: i64) -> Self {
        let parent_step = (size / CELLS_PER_CHUNK).max(1) * 2;
        // The chunk spans `size / parent_step` parent cells; +1 for corners.
        let cells = (size / parent_step) as i32;
        let min = region_min / parent_step as i32 - IVec3::splat(Self::MARGIN);
        let dim = cells + 1 + 2 * Self::MARGIN;
        let mut data = Vec::with_capacity((dim as usize).pow(3));
        for z in 0..dim {
            for y in 0..dim {
                for x in 0..dim {
                    data.push(world.is_solid_voxel(
                        (min.x + x) as i64 * parent_step,
                        (min.y + y) as i64 * parent_step,
                        (min.z + z) as i64 * parent_step,
                    ));
                }
            }
        }
        Self {
            min,
            dim,
            parent_step,
            data,
        }
    }

    #[inline]
    fn corner(&self, world: &VoxelWorld, x: i32, y: i32, z: i32) -> f32 {
        let solid = if (0..self.dim).contains(&x)
            && (0..self.dim).contains(&y)
            && (0..self.dim).contains(&z)
        {
            self.data[((z * self.dim + y) * self.dim + x) as usize]
        } else {
            // Outside the cached window (rare): evaluate procedurally.
            world.is_solid_voxel(
                (self.min.x + x) as i64 * self.parent_step,
                (self.min.y + y) as i64 * self.parent_step,
                (self.min.z + z) as i64 * self.parent_step,
            )
        };
        if solid { 1.0 } else { 0.0 }
    }

    /// Trilinear occupancy of the parent grid at world position `p` (0 air … 1
    /// solid). Continuous in `p`, so neighbouring vertices get consistent values.
    fn occupancy(&self, world: &VoxelWorld, p: Vec3) -> f32 {
        let mvs = world.config.min_voxel_size;
        let g = (p - world.config.origin) / (mvs * self.parent_step as f32)
            - self.min.as_vec3();
        let base = g.floor();
        let f = g - base;
        let (bx, by, bz) = (base.x as i32, base.y as i32, base.z as i32);
        let c = |dx: i32, dy: i32, dz: i32| self.corner(world, bx + dx, by + dy, bz + dz);
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let x00 = lerp(c(0, 0, 0), c(1, 0, 0), f.x);
        let x10 = lerp(c(0, 1, 0), c(1, 1, 0), f.x);
        let x01 = lerp(c(0, 0, 1), c(1, 0, 1), f.x);
        let x11 = lerp(c(0, 1, 1), c(1, 1, 1), f.x);
        let y0 = lerp(x00, x10, f.y);
        let y1 = lerp(x01, x11, f.y);
        lerp(y0, y1, f.z)
    }
}

/// Signed distance along the vertex normal `n` from `p` to the parent-LOD surface
/// (the 0.5 crossing of [`ParentField::occupancy`]) — the geomorph target.
/// Displacing the vertex by this offset puts it on the surface the parent chunk
/// renders. The crossing nearest the vertex wins so thin features don't snap the
/// vertex to a different surface sheet; none within ±2 parent cells → 0 (don't
/// morph).
///
/// Deterministic in `(p, n, step)` alone — never in chunk identity — so two
/// same-LOD chunks sharing a vertex compute the identical target (no morph cracks).
fn parent_surface_offset(world: &VoxelWorld, field: &ParentField, p: Vec3, n: Vec3, step: i64) -> f32 {
    // Quarter of a parent cell per sample, spanning ±2 parent cells.
    let h = step as f32 * world.config.min_voxel_size * 0.5;
    const STEPS: i32 = 8;
    let mut best: Option<f32> = None;
    let mut prev = field.occupancy(world, p + n * (-(STEPS as f32) * h)) - 0.5;
    for i in (1 - STEPS)..=STEPS {
        let x = i as f32 * h;
        let cur = field.occupancy(world, p + n * x) - 0.5;
        if prev * cur < 0.0 {
            // Bracketed the 0.5 level: linear refine inside the bracket.
            let t = x - h + h * (-prev) / (cur - prev);
            if best.is_none_or(|b: f32| t.abs() < b.abs()) {
                best = Some(t);
            }
        }
        prev = cur;
    }
    best.unwrap_or(0.0)
}

/// Convert a Transvoxel [`transvoxel::generic_mesh::Mesh`] to a Bevy mesh. Normals
/// are recomputed seam-consistently (see [`field_normal`]); each vertex's colour
/// carries per-vertex terrain data (texturing has no UVs to spare):
/// - `r`: the material's texture-array layer,
/// - `gba`: the geomorph displacement vector to the parent-LOD surface (world
///   units) — the parent-offset march along the normal, premultiplied by an
///   edge-pin weight that is 0 at chunk faces (keeping Transvoxel transition
///   stitching and same-LOD borders watertight while interiors morph) and ramps to
///   1 a few cells inward. A full vector rather than a scalar because the shadow
///   prepass has no normal attribute to displace along.
fn to_bevy_mesh(
    world: &VoxelWorld,
    mesh: transvoxel::generic_mesh::Mesh<f32>,
    region_min: IVec3,
    step: i64,
) -> Mesh {
    let positions: Vec<[f32; 3]> = mesh
        .positions
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    let indices: Vec<u32> = mesh.triangle_indices.iter().map(|&i| i as u32).collect();

    let normals: Vec<[f32; 3]> = positions
        .iter()
        .map(|p| field_normal(world, Vec3::from_array(*p)))
        .collect();

    let mvs = world.config.min_voxel_size;
    let base = world.config.origin + region_min.as_vec3() * mvs;
    let cell = step as f32 * mvs;
    // Cells over which the geomorph weight ramps from 0 (chunk face) to 1.
    const PIN_RAMP_CELLS: f32 = 3.0;
    let parent_field = ParentField::build(world, region_min, step * CELLS_PER_CHUNK);

    let colors: Vec<[f32; 4]> = positions
        .iter()
        .zip(&normals)
        .map(|(p, n)| {
            let p = Vec3::from_array(*p);
            let n = Vec3::from_array(*n);
            let layer = surface_material(world, p, step).layer();
            let offset = parent_surface_offset(world, &parent_field, p, n, step);
            // Distance (in cells) to the nearest chunk face → edge-pin weight.
            let c = (p - base) / cell;
            let to_face = c.min(Vec3::splat(CELLS_PER_CHUNK as f32) - c);
            let weight = (to_face.min_element() / PIN_RAMP_CELLS).clamp(0.0, 1.0);
            let d = n * (offset * weight);
            [layer as f32, d.x, d.y, d.z]
        })
        .collect();

    let mut out = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    out.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    out.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    out.insert_indices(Indices::U32(indices));
    out
}
