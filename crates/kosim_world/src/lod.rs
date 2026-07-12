//! Isosurface meshing of the binary voxel world (Phase 1: single full-resolution
//! LOD via grid-aligned marching cubes).
//!
//! Each *marching cell* spans eight neighbouring voxel *centres*; its 8-bit corner
//! occupancy mask indexes [`crate::tables`] to pick the triangulation. Every output
//! vertex sits at the exact midpoint of the cell edge it lies on -- the `t = 0.5`
//! case, never interpolated by a density value -- so the surface is grid-aligned to
//! the half-voxel lattice and passes cleanly through the faces between solid and
//! empty voxels.
//!
//! Because the placement is exact, a vertex is identified by an integer lattice key
//! (`voxel_a + voxel_b`, the sum of the two corner coordinates it bridges). That
//! welds shared vertices with no floating-point tolerance and, later, gives the
//! exact fine/coarse correspondence the HVT geomorph (Phase 3) needs.
//!
//! Camera-driven level of detail and Transvoxel transition cells return in Phase 2;
//! for now the whole world is meshed once at leaf resolution.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::math::{IVec3, Vec3};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

use crate::VoxelWorld;
use crate::tables::{CORNER_OFFSETS, EDGE_CORNERS, EDGE_TABLE, TRI_TABLE};

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
    /// Area-weighted face-normal sums; normalised in [`Self::finish`].
    normals: Vec<Vec3>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
    /// Integer lattice key -> index into the vertex arrays.
    vertex_map: HashMap<IVec3, u32>,
}

impl MeshBuilder {
    /// Fetch or create the welded vertex at lattice `key`, positioned at `pos`
    /// with surface `color`. The colour is taken from the first cell to emit the
    /// vertex; that is good enough while terrain is vertex-coloured (texturing is
    /// Phase 5).
    fn vertex(&mut self, key: IVec3, pos: Vec3, color: [f32; 4]) -> u32 {
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
    fn triangle(&mut self, a: (IVec3, Vec3, [f32; 4]), b: (IVec3, Vec3, [f32; 4]), c: (IVec3, Vec3, [f32; 4])) {
        let ia = self.vertex(a.0, a.1, a.2);
        let ib = self.vertex(b.0, b.1, b.2);
        let ic = self.vertex(c.0, c.1, c.2);
        // Degenerate triangles never occur: the three edges of a marching-cubes
        // case are always distinct, and distinct edges have distinct midpoints.
        let face_normal = (b.1 - a.1).cross(c.1 - a.1);
        self.normals[ia as usize] += face_normal;
        self.normals[ib as usize] += face_normal;
        self.normals[ic as usize] += face_normal;
        self.indices.extend_from_slice(&[ia, ib, ic]);
    }

    fn finish(self) -> TerrainMesh {
        let MeshBuilder {
            positions,
            normals,
            colors,
            indices,
            ..
        } = self;

        let normals: Vec<[f32; 3]> = normals
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
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.insert_indices(Indices::U32(indices));

        TerrainMesh {
            mesh,
            collider_vertices,
            collider_indices,
        }
    }
}

/// Build the full-resolution isosurface mesh (and matching collider buffers) for
/// `world`.
pub fn build_terrain_mesh(world: &VoxelWorld) -> TerrainMesh {
    let mut builder = MeshBuilder::default();
    let dim = world.dim;

    // Marching cells sit *between* voxel centres, so a cell at `c` samples voxels
    // `c + corner`. Ranging `c` over `-1..dim` lets the outermost cells reach the
    // shell voxels (0 and dim-1); out-of-range samples read as empty, closing the
    // surface at the world boundary.
    for cx in -1..dim {
        for cy in -1..dim {
            for cz in -1..dim {
                mesh_cell(world, IVec3::new(cx as i32, cy as i32, cz as i32), &mut builder);
            }
        }
    }

    builder.finish()
}

/// Mesh a single marching cell whose minimum-corner voxel is `cell`.
fn mesh_cell(world: &VoxelWorld, cell: IVec3, builder: &mut MeshBuilder) {
    // Corner occupancy -> case mask. Bit `i` is set when corner `i` is solid.
    let mut mask = 0u8;
    let mut corner_voxel = [IVec3::ZERO; 8];
    for i in 0..8 {
        let v = cell + CORNER_OFFSETS[i];
        corner_voxel[i] = v;
        if world.is_solid_voxel(v.x as i64, v.y as i64, v.z as i64) {
            mask |= 1 << i;
        }
    }

    let edges = EDGE_TABLE[mask as usize];
    if edges == 0 {
        return;
    }

    let mvs = world.config.min_voxel_size;
    let origin = world.config.origin;
    // Build the (key, position, colour) for the vertex on cube edge `e`.
    let edge_vertex = |e: usize| -> (IVec3, Vec3, [f32; 4]) {
        let [ca, cb] = EDGE_CORNERS[e];
        let va = corner_voxel[ca];
        let vb = corner_voxel[cb];
        // Exact integer key: the sum of the two bridged voxel coordinates.
        let key = va + vb;
        // World midpoint of the two voxel *centres* (`centre = origin + (v+0.5)*mvs`).
        let pos = origin + (key.as_vec3() * 0.5 + Vec3::splat(0.5)) * mvs;
        // Colour from whichever corner is solid (a crossed edge has exactly one).
        let solid = if (mask & (1 << ca)) != 0 { va } else { vb };
        let color = world
            .voxel_material(solid.x as i64, solid.y as i64, solid.z as i64)
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
        builder.triangle(a, b, c);
        i += 3;
    }
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
