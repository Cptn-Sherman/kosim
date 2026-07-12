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

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::math::{IVec3, Vec3};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

use crate::VoxelWorld;
use crate::tables::{CORNER_OFFSETS, EDGE_CORNERS, EDGE_TABLE, TRI_TABLE};

/// Marching cells per axis in a leaf chunk. A region is meshed with cell step
/// `region_voxels / CELLS_PER_CHUNK`, so a chunk keeps a bounded triangle budget
/// however large a volume it covers at a coarse LOD.
const CELLS_PER_CHUNK: i64 = 16;

/// Identifies a leaf chunk: its minimum-corner voxel and its edge length in voxels.
/// The edge length fixes the LOD (cell step), so `(region_min, size)` is unique.
pub type ChunkKey = (IVec3, i64);

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

    /// Finish into a render mesh only, dropping vertices that no owned triangle
    /// references (the apron leaves some behind). Normals are accumulated across the
    /// apron first, so kept boundary vertices are already seam-consistent.
    fn finish_mesh(self) -> Mesh {
        let MeshBuilder {
            positions,
            normals,
            colors,
            indices,
            ..
        } = self;

        let mut remap = vec![u32::MAX; positions.len()];
        let mut out_pos: Vec<[f32; 3]> = Vec::new();
        let mut out_norm: Vec<[f32; 3]> = Vec::new();
        let mut out_col: Vec<[f32; 4]> = Vec::new();
        let mut out_idx: Vec<u32> = Vec::with_capacity(indices.len());
        for &old in &indices {
            let mut ni = remap[old as usize];
            if ni == u32::MAX {
                ni = out_pos.len() as u32;
                remap[old as usize] = ni;
                out_pos.push(positions[old as usize].to_array());
                out_norm.push(normals[old as usize].normalize_or_zero().to_array());
                out_col.push(colors[old as usize]);
            }
            out_idx.push(ni);
        }

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, out_pos);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, out_norm);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, out_col);
        mesh.insert_indices(Indices::U32(out_idx));
        mesh
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

/// The set of leaf chunks the camera should currently see, as `(region_min, size)`
/// keys. Cheap: it walks the chunk octree choosing an LOD per region but does no
/// meshing.
pub fn desired_chunks(world: &VoxelWorld, camera_pos: Vec3) -> Vec<ChunkKey> {
    let mut out = Vec::new();
    collect_chunk_keys(world, IVec3::ZERO, world.dim, camera_pos, &mut out);
    out
}

/// Is a region of edge length `world_size` centred at `center` close enough to the
/// camera to warrant subdividing it for more detail?
fn should_subdivide(world: &VoxelWorld, center: Vec3, world_size: f32, camera_pos: Vec3) -> bool {
    let dist = center.distance(camera_pos).max(1.0e-3);
    world_size / dist > world.config.lod_threshold
}

/// Walk the virtual chunk octree, subdividing a region while it is large relative to
/// its camera distance (down to `CELLS_PER_CHUNK` voxels), recording a leaf chunk
/// key wherever subdivision stops.
fn collect_chunk_keys(
    world: &VoxelWorld,
    region_min: IVec3,
    size: i64,
    camera_pos: Vec3,
    out: &mut Vec<ChunkKey>,
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
            collect_chunk_keys(world, region_min + offset, size / 2, camera_pos, out);
        }
    } else {
        out.push((region_min, size));
    }
}

/// Mesh a single leaf chunk `(region_min, size)` into its own render mesh. Meshes a
/// one-cell apron around the owned `CELLS_PER_CHUNK` cells for seam-consistent
/// normals; only owned cells' triangles are kept. Pure over `world`, so it runs off
/// the main thread.
pub fn mesh_one_chunk(world: &VoxelWorld, region_min: IVec3, size: i64) -> Mesh {
    let step = (size / CELLS_PER_CHUNK).max(1);
    let step_i = step as i32;
    let cells = (size / step) as i32;
    let mut builder = MeshBuilder::default();
    for lx in -1..=cells {
        for ly in -1..=cells {
            for lz in -1..=cells {
                let owned = lx >= 0 && lx < cells && ly >= 0 && ly < cells && lz >= 0 && lz < cells;
                let cell = IVec3::new(lx, ly, lz);
                let corner_base =
                    std::array::from_fn(|i| region_min + (cell + CORNER_OFFSETS[i]) * step_i);
                emit_cell(world, corner_base, step, owned, &mut builder);
            }
        }
    }
    builder.finish_mesh()
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
