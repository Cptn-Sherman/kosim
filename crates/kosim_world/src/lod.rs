//! Camera-driven level-of-detail traversal and cube meshing.
//!
//! The octree is walked from the root. For each node we compute a screen-relative
//! size (`world_size / distance_to_camera`). Nodes that are large relative to
//! their distance (i.e. close to the camera) are subdivided for more detail;
//! nodes that are small (far away) are emitted as a single coarse cube. A merged
//! `Leaf` is always emitted whole, since it holds no finer detail.
//!
//! Cube faces are culled against the world: a face is skipped when the equally
//! sized neighbour on that side is solid, which removes the interior of the
//! terrain and leaves only the visible shell.

use bevy::asset::RenderAssetUsages;
use bevy::math::{IVec3, Vec3};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};

use crate::VoxelWorld;
use crate::octree::OctNode;
use crate::voxel::Voxel;

/// The six axis-aligned face directions as `(normal, four CCW corner offsets)`
/// in unit-cube space. Corners are wound counter-clockwise when viewed from
/// outside so the default front-face culling keeps them.
struct Face {
    normal: [f32; 3],
    corners: [[f32; 3]; 4],
}

const FACES: [Face; 6] = [
    // +X
    Face {
        normal: [1.0, 0.0, 0.0],
        corners: [
            [1.0, 0.0, 1.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
        ],
    },
    // -X
    Face {
        normal: [-1.0, 0.0, 0.0],
        corners: [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [0.0, 1.0, 0.0],
        ],
    },
    // +Y
    Face {
        normal: [0.0, 1.0, 0.0],
        corners: [
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
    },
    // -Y
    Face {
        normal: [0.0, -1.0, 0.0],
        corners: [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
        ],
    },
    // +Z
    Face {
        normal: [0.0, 0.0, 1.0],
        corners: [
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ],
    },
    // -Z
    Face {
        normal: [0.0, 0.0, -1.0],
        corners: [
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
        ],
    },
];

/// Accumulates geometry for the terrain mesh.
#[derive(Default)]
struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    /// Emit the visible faces of one cube. `min_world` is the cube's minimum
    /// corner in world space, `world_size` its edge length. `cull` reports
    /// whether the neighbouring cell in a given direction is solid (and hence
    /// the shared face should be skipped).
    fn push_cube(
        &mut self,
        min_world: Vec3,
        world_size: f32,
        voxel: Voxel,
        mut cull: impl FnMut([f32; 3]) -> bool,
    ) {
        let color = voxel.material.linear_rgba();
        for face in &FACES {
            if cull(face.normal) {
                continue;
            }
            let base = self.positions.len() as u32;
            for corner in &face.corners {
                self.positions.push([
                    min_world.x + corner[0] * world_size,
                    min_world.y + corner[1] * world_size,
                    min_world.z + corner[2] * world_size,
                ]);
                self.normals.push(face.normal);
                self.colors.push(color);
            }
            self.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }

    fn into_mesh(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colors);
        mesh.insert_indices(Indices::U32(self.indices));
        mesh
    }
}

/// Build a level-of-detail terrain mesh for `world` as seen from `camera_pos`
/// (world space).
pub fn build_lod_mesh(world: &VoxelWorld, camera_pos: Vec3) -> Mesh {
    let mut builder = MeshBuilder::default();
    collect(world, &world.root, IVec3::ZERO, world.dim, camera_pos, &mut builder);
    builder.into_mesh()
}

/// Should a node of `world_size` at `center` (world space) be subdivided for
/// more detail, rather than drawn as a single cube?
fn should_subdivide(world: &VoxelWorld, center: Vec3, world_size: f32, camera_pos: Vec3) -> bool {
    let dist = center.distance(camera_pos).max(1.0e-3);
    world_size / dist > world.config.lod_threshold
}

/// Recursively gather visible cubes for `node`, whose minimum corner is `min`
/// (voxel coords) and whose edge length is `size` voxels.
fn collect(
    world: &VoxelWorld,
    node: &OctNode,
    min: IVec3,
    size: i64,
    camera_pos: Vec3,
    builder: &mut MeshBuilder,
) {
    match node {
        OctNode::Empty => {}
        OctNode::Leaf(voxel) => emit_cube(world, min, size, *voxel, builder),
        OctNode::Branch(children) => {
            let mvs = world.config.min_voxel_size;
            let world_size = size as f32 * mvs;
            let center = world.config.origin
                + (min.as_vec3() + Vec3::splat(size as f32 * 0.5)) * mvs;

            if size > 1 && should_subdivide(world, center, world_size, camera_pos) {
                let half = (size / 2) as i32;
                for (i, child) in children.iter().enumerate() {
                    let offset = IVec3::new(
                        (i & 1) as i32 * half,
                        ((i >> 1) & 1) as i32 * half,
                        ((i >> 2) & 1) as i32 * half,
                    );
                    collect(world, child, min + offset, size / 2, camera_pos, builder);
                }
            } else if let Some(voxel) = node.representative() {
                // Far enough away (or unsplittable): draw the whole branch as one
                // coarse cube.
                emit_cube(world, min, size, voxel, builder);
            }
        }
    }
}

/// Emit a single cube spanning voxels `[min, min+size)`, culling a face only when
/// the equally sized neighbour on that side is *fully* solid (and therefore
/// certain to cover it). Culling against a single sample voxel instead leaves
/// see-through holes at LOD seams and surface steps.
fn emit_cube(world: &VoxelWorld, min: IVec3, size: i64, voxel: Voxel, builder: &mut MeshBuilder) {
    let mvs = world.config.min_voxel_size;
    let min_world = world.config.origin + min.as_vec3() * mvs;
    let world_size = size as f32 * mvs;

    builder.push_cube(min_world, world_size, voxel, |normal| {
        // The equally sized neighbour block on this side.
        let neighbour = min
            + IVec3::new(
                normal[0] as i32 * size as i32,
                normal[1] as i32 * size as i32,
                normal[2] as i32 * size as i32,
            );
        world.region_full_solid(neighbour, size)
    });
}
