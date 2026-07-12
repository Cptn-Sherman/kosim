//! Isosurface meshing of the binary voxel world.
//!
//! Each *marching cell* spans eight neighbouring voxel *centres*; its 8-bit corner
//! occupancy mask indexes [`crate::tables`] to pick the triangulation. Every output
//! vertex sits at the exact midpoint of the cell edge it lies on -- the `t = 0.5`
//! case, never interpolated by a density value -- so the surface is grid-aligned to
//! the half-voxel lattice and passes cleanly through the faces between solid and
//! empty voxels. Because the placement is exact, a vertex is identified by an
//! integer lattice key, so shared vertices weld with no floating-point tolerance.
//!
//! Two entry points share the same cell emitter:
//! * [`build_terrain_mesh`] meshes the whole world at full resolution into a single
//!   welded mesh -- used once for the static physics collider, which stays
//!   LOD-independent.
//! * [`desired_chunks`] + [`mesh_one_chunk`] drive the visual mesh: the world is
//!   split into a camera-distance-driven set of leaf *chunks*, each meshed
//!   independently (and off the main thread) so only the chunks whose LOD actually
//!   changed are ever rebuilt. Each chunk is meshed with a one-cell *apron* -- an
//!   extra ring of neighbour cells that contribute to boundary-vertex normals but
//!   whose own triangles are dropped -- so per-chunk meshes share identical normals
//!   at their seams and light without creases.
//!
//! Phase 2b will add Transvoxel transition cells to stitch the geometric cracks
//! that still appear where two chunks of *different* LOD meet, and close the world's
//! outer boundary fringe (per-chunk meshing tiles `[0, dim)` and so omits the `-1`
//! shell face that [`build_terrain_mesh`] still meshes for the collider).

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::math::{IVec3, Vec3};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use transvoxel::generic_mesh::GenericMeshBuilder;
use transvoxel::prelude::{Block, extract_from_fn};
use transvoxel::transition_sides::{TransitionSide, TransitionSides, no_side};
use transvoxel::voxel_source::BlockDims;

use crate::VoxelWorld;
use crate::tables::{CORNER_OFFSETS, EDGE_CORNERS, EDGE_TABLE, TRI_TABLE};
use crate::voxel::VoxelMaterial;

/// Marching cells per axis in a leaf chunk. A region is meshed with cell step
/// `region_voxels / CELLS_PER_CHUNK`, so a chunk keeps a bounded triangle budget
/// however large a volume it covers at a coarse LOD.
const CELLS_PER_CHUNK: i64 = 16;

/// Identifies a leaf chunk to render: its minimum-corner voxel, its edge length in
/// voxels (which fixes the LOD), and a bitmask of the faces that need Transvoxel
/// transition cells because the neighbour there is one level finer. The mask is part
/// of the key so a chunk is re-meshed when its neighbours' LODs change.
///
/// Side bits follow [`transvoxel::transition_sides::TransitionSide`] order:
/// `1<<0`=LowX(-X), `1<<1`=HighX(+X), `1<<2`=LowY, `1<<3`=HighY, `1<<4`=LowZ, `1<<5`=HighZ.
pub type ChunkKey = (IVec3, i64, u8);

/// A welded vertex is keyed by its exact half-voxel lattice position *and* the LOD
/// step it was built at. Within a chunk the step is constant; the tag also keeps the
/// full-resolution collider mesh from welding across LODs.
type VKey = (IVec3, i32);
/// `(weld key, world position, linear RGBA)` for one triangle corner.
type Vert = (VKey, Vec3, [f32; 4]);

/// A finished terrain mesh plus the buffers needed to build a matching physics
/// collider. The collider shares the mesh's welded vertices and triangle winding,
/// so physics lines up exactly with what is drawn.
pub struct TerrainMesh {
    pub mesh: Mesh,
    pub collider_vertices: Vec<Vec3>,
    pub collider_indices: Vec<[u32; 3]>,
}

/// Accumulates welded isosurface geometry.
#[derive(Default)]
struct MeshBuilder {
    positions: Vec<Vec3>,
    /// Area-weighted face-normal sums; normalised on output.
    normals: Vec<Vec3>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    vertex_map: HashMap<VKey, u32>,
}

impl MeshBuilder {
    /// Fetch or create the welded vertex at `key`, positioned at `pos` with surface
    /// `color`. The colour is taken from the first cell to emit the vertex; that is
    /// good enough while terrain is vertex-coloured (texturing is Phase 5).
    fn vertex(&mut self, key: VKey, pos: Vec3, color: [f32; 4]) -> u32 {
        if let Some(&idx) = self.vertex_map.get(&key) {
            return idx;
        }
        let idx = self.positions.len() as u32;
        self.positions.push(pos);
        self.normals.push(Vec3::ZERO);
        self.colors.push(color);
        self.vertex_map.insert(key, idx);
        idx
    }

    /// Emit one triangle. Vertices are passed in the table's winding order, which
    /// yields an outward (CCW-from-air) face. The unnormalised cross product is
    /// added to each corner so vertex normals come out area-weighted and smooth.
    ///
    /// `owned` triangles are drawn; unowned (apron) triangles still contribute their
    /// normal to shared boundary vertices but are not indexed, so a chunk's seam
    /// normals match its neighbour's exactly.
    fn triangle(&mut self, owned: bool, a: Vert, b: Vert, c: Vert) {
        let ia = self.vertex(a.0, a.1, a.2);
        let ib = self.vertex(b.0, b.1, b.2);
        let ic = self.vertex(c.0, c.1, c.2);
        // Degenerate triangles never occur: the three edges of a marching-cubes
        // case are always distinct, and distinct edges have distinct midpoints.
        let face_normal = (b.1 - a.1).cross(c.1 - a.1);
        self.normals[ia as usize] += face_normal;
        self.normals[ib as usize] += face_normal;
        self.normals[ic as usize] += face_normal;
        if owned {
            self.indices.extend_from_slice(&[ia, ib, ic]);
        }
    }

    /// Finish into a render mesh plus collider buffers (used for the full-resolution
    /// collider, where every triangle is owned).
    fn finish(self) -> TerrainMesh {
        let MeshBuilder {
            positions,
            normals,
            colors,
            indices,
            ..
        } = self;

        let out_normals: Vec<[f32; 3]> = normals
            .iter()
            .map(|n| n.normalize_or_zero().to_array())
            .collect();
        let collider_vertices = positions.clone();
        let collider_indices: Vec<[u32; 3]> =
            indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            positions.iter().map(|p| p.to_array()).collect::<Vec<_>>(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, out_normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32(indices));

        TerrainMesh {
            mesh,
            collider_vertices,
            collider_indices,
        }
    }

}

/// The finest voxel that represents a `step`-sized corner block: the voxel at the
/// block's centre. At `step == 1` this is the block itself, so full-resolution
/// meshing samples voxels directly.
#[inline]
fn sample_voxel(base: IVec3, step: i64) -> IVec3 {
    base + IVec3::splat((step / 2) as i32)
}

/// Emit the isosurface triangles for one marching cell. `corner_base[i]` is the
/// minimum-corner global voxel coordinate of corner `i`'s `step`-sized block.
/// `owned` cells are drawn; unowned apron cells only feed boundary normals.
fn emit_cell(
    world: &VoxelWorld,
    corner_base: [IVec3; 8],
    step: i64,
    owned: bool,
    builder: &mut MeshBuilder,
) {
    // Corner occupancy -> case mask. Bit `i` is set when corner `i` is solid.
    let mut mask = 0u8;
    for i in 0..8 {
        let s = sample_voxel(corner_base[i], step);
        if world.is_solid_voxel(s.x as i64, s.y as i64, s.z as i64) {
            mask |= 1 << i;
        }
    }

    if EDGE_TABLE[mask as usize] == 0 {
        return;
    }

    let mvs = world.config.min_voxel_size;
    let origin = world.config.origin;
    // Build the (key, position, colour) for the vertex on cube edge `e`.
    let edge_vertex = |e: usize| -> Vert {
        let [ca, cb] = EDGE_CORNERS[e];
        let ba = corner_base[ca];
        let bb = corner_base[cb];
        // Exact integer key: the sum of the two bridged corner coordinates, tagged
        // with the LOD step so different resolutions never share a vertex.
        let key_pos = ba + bb;
        let key = (key_pos, step as i32);
        // World midpoint of the two corner block centres
        // (`centre = origin + (base + step/2) * mvs`).
        let pos = origin + (key_pos.as_vec3() * 0.5 + Vec3::splat(step as f32 * 0.5)) * mvs;
        // Colour from whichever corner is solid (a crossed edge has exactly one).
        let solid_base = if (mask & (1 << ca)) != 0 { ba } else { bb };
        let s = sample_voxel(solid_base, step);
        let color = world
            .voxel_material(s.x as i64, s.y as i64, s.z as i64)
            .map(|m| m.linear_rgba())
            .unwrap_or([1.0, 0.0, 1.0, 1.0]);
        (key, pos, color)
    };

    let tri = &TRI_TABLE[mask as usize];
    let mut i = 0;
    while i < tri.len() && tri[i] != -1 {
        let a = edge_vertex(tri[i] as usize);
        let b = edge_vertex(tri[i + 1] as usize);
        let c = edge_vertex(tri[i + 2] as usize);
        builder.triangle(owned, a, b, c);
        i += 3;
    }
}

/// Build the full-resolution isosurface mesh (and matching collider buffers) for
/// `world`. Cells range `-1..dim` so the outermost cells reach the shell voxels and
/// the surface closes at the world boundary.
pub fn build_terrain_mesh(world: &VoxelWorld) -> TerrainMesh {
    let mut builder = MeshBuilder::default();
    let dim = world.dim;
    for cx in -1..dim {
        for cy in -1..dim {
            for cz in -1..dim {
                let cell = IVec3::new(cx as i32, cy as i32, cz as i32);
                let corner_base = std::array::from_fn(|i| cell + CORNER_OFFSETS[i]);
                emit_cell(world, corner_base, 1, true, &mut builder);
            }
        }
    }
    builder.finish()
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
        (1 << 0, IVec3::new(x - 1, y + h, z + h)),     // LowX
        (1 << 1, IVec3::new(x + s, y + h, z + h)),     // HighX
        (1 << 2, IVec3::new(x + h, y - 1, z + h)),     // LowY
        (1 << 3, IVec3::new(x + h, y + s, z + h)),     // HighY
        (1 << 4, IVec3::new(x + h, y + h, z - 1)),     // LowZ
        (1 << 5, IVec3::new(x + h, y + h, z + s)),     // HighZ
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
    to_bevy_mesh(world, mesh, step)
}

/// The surface (topsoil) material at a vertex: march inward from just outside the
/// vertex along `-normal` and take the first solid voxel. Starting outside (by a
/// whole coarse cell) makes this find the same fine-resolution surface voxel at every
/// LOD, so a vertex's material no longer changes when its chunk switches LOD.
fn surface_material(world: &VoxelWorld, p: Vec3, n: Vec3, step: i64) -> VoxelMaterial {
    let mvs = world.config.min_voxel_size;
    let origin = world.config.origin;
    let march = step as f32 * mvs; // one coarse cell
    let sample_step = mvs * 0.5;
    let start = p + n * march; // safely outside the fine surface
    let count = ((2.0 * march) / sample_step).ceil() as i32 + 1;
    for i in 0..count {
        let sp = start - n * (i as f32 * sample_step);
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
        [0.0, 1.0, 0.0]
    }
}

/// Convert a Transvoxel [`transvoxel::generic_mesh::Mesh`] to a Bevy mesh. Normals
/// are recomputed seam-consistently (see [`field_normal`]); each vertex's colour is
/// the material of the voxel just inside the surface.
fn to_bevy_mesh(world: &VoxelWorld, mesh: transvoxel::generic_mesh::Mesh<f32>, step: i64) -> Mesh {
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

    // The vertex colour carries the material's texture-array layer index in its red
    // channel (unclamped float); the chunk shader reads it back to pick a texture.
    let colors: Vec<[f32; 4]> = positions
        .iter()
        .zip(normals.iter())
        .map(|(p, n)| {
            let layer =
                surface_material(world, Vec3::from_array(*p), Vec3::from_array(*n), step).layer();
            [layer as f32, 0.0, 0.0, 1.0]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WorldConfig;
    use crate::octree::OctNode;
    use crate::voxel::{Voxel, VoxelMaterial};
    use std::collections::HashMap;

    /// A single solid voxel surrounded by air must mesh to a closed octahedron:
    /// six face-midpoint vertices, eight triangles, every edge shared by exactly
    /// two triangles, and every normal pointing away from the voxel centre. This
    /// pins the corner offsets, both lookup tables, vertex welding, and winding.
    #[test]
    fn single_voxel_is_a_closed_outward_octahedron() {
        let mut children: [OctNode; 8] = std::array::from_fn(|_| OctNode::Empty);
        children[0] = OctNode::Leaf(Voxel::new(VoxelMaterial::Stone));
        let world = VoxelWorld {
            root: OctNode::Branch(Box::new(children)),
            dim: 2,
            config: WorldConfig {
                min_voxel_size: 1.0,
                max_depth: 1,
                origin: Vec3::ZERO,
                ..WorldConfig::default()
            },
        };

        let terrain = build_terrain_mesh(&world);

        assert_eq!(terrain.collider_vertices.len(), 6, "octahedron has 6 vertices");
        assert_eq!(terrain.collider_indices.len(), 8, "octahedron has 8 faces");

        // Closed manifold: each undirected edge appears in exactly two triangles.
        let mut edge_uses: HashMap<(u32, u32), u32> = HashMap::new();
        for tri in &terrain.collider_indices {
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                *edge_uses.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        assert!(
            edge_uses.values().all(|&n| n == 2),
            "every edge should be shared by two triangles: {edge_uses:?}"
        );

        // Every face winds outward: its geometric normal points away from the
        // voxel centre at (0.5, 0.5, 0.5).
        let center = Vec3::splat(0.5);
        for tri in &terrain.collider_indices {
            let p0 = terrain.collider_vertices[tri[0] as usize];
            let p1 = terrain.collider_vertices[tri[1] as usize];
            let p2 = terrain.collider_vertices[tri[2] as usize];
            let normal = (p1 - p0).cross(p2 - p0);
            let outward = (p0 + p1 + p2) / 3.0 - center;
            assert!(
                normal.dot(outward) > 0.0,
                "face {tri:?} winds inward (normal {normal:?}, outward {outward:?})"
            );
        }
    }
}
