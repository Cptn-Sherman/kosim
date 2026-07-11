//! A sparse, compressed octree of voxels.
//!
//! Nodes are cubic and axis-aligned, addressed in integer *voxel* coordinates
//! (each step of 1 equals one leaf voxel, i.e. [`crate::WorldConfig::min_voxel_size`]
//! world units). The root spans `[0, dim)` on every axis where `dim` is a power
//! of two equal to `2^max_depth`.
//!
//! The tree is *compressed*: any cubic region that is uniformly empty collapses
//! to [`OctNode::Empty`], and any region that is uniformly one material collapses
//! to a single [`OctNode::Leaf`]. A `Leaf` therefore represents anything from a
//! single 0.25-unit voxel up to a large solid block — which is exactly what makes
//! the tree cheap to traverse for level-of-detail meshing.

use bevy::math::IVec3;

use crate::voxel::Voxel;

/// Child ordering: bit 0 = X, bit 1 = Y, bit 2 = Z.
pub const CHILD_COUNT: usize = 8;

/// A node in the voxel octree.
pub enum OctNode {
    /// A fully empty (air) cubic region.
    Empty,
    /// A cubic region uniformly filled with a single voxel material.
    Leaf(Voxel),
    /// A subdivided region with eight equally sized children.
    Branch(Box<[OctNode; CHILD_COUNT]>),
}

impl OctNode {
    /// Index of the child containing local coordinate `(x, y, z)` within a node
    /// whose children each have edge length `half`.
    #[inline]
    fn child_index(x: i64, y: i64, z: i64, half: i64) -> (usize, i64, i64, i64) {
        let ix = (x >= half) as usize;
        let iy = (y >= half) as usize;
        let iz = (z >= half) as usize;
        let idx = ix | (iy << 1) | (iz << 2);
        (
            idx,
            x - ix as i64 * half,
            y - iy as i64 * half,
            z - iz as i64 * half,
        )
    }

    /// Is the voxel at local coordinate `(x, y, z)` solid, given this node spans
    /// `[0, size)` on each axis? Coordinates outside `[0, size)` are treated as
    /// empty.
    pub fn is_solid(&self, x: i64, y: i64, z: i64, size: i64) -> bool {
        if x < 0 || y < 0 || z < 0 || x >= size || y >= size || z >= size {
            return false;
        }
        match self {
            OctNode::Empty => false,
            OctNode::Leaf(_) => true,
            OctNode::Branch(children) => {
                let half = size / 2;
                let (idx, cx, cy, cz) = Self::child_index(x, y, z, half);
                children[idx].is_solid(cx, cy, cz, half)
            }
        }
    }

    /// Is every voxel in this node solid (no `Empty` anywhere in the subtree)?
    pub fn is_full_solid(&self) -> bool {
        match self {
            OctNode::Empty => false,
            OctNode::Leaf(_) => true,
            OctNode::Branch(children) => children.iter().all(|c| c.is_full_solid()),
        }
    }

    /// Is the axis-aligned region whose minimum corner is `(qx, qy, qz)` and edge
    /// length `qsize` entirely solid, given this node spans `[0, size)` on each
    /// axis? The region must be grid-aligned to `qsize` (as LOD nodes always are),
    /// so it falls wholly within a single descendant at each level.
    ///
    /// Used for face culling: a cube face may only be dropped when the equally
    /// sized neighbour is *fully* solid and therefore certain to cover it. Testing
    /// a single sample voxel instead over-culls wherever the neighbour is solid at
    /// that point but hollow elsewhere (LOD seams, surface steps), leaving
    /// see-through holes.
    pub fn region_full_solid(&self, qx: i64, qy: i64, qz: i64, qsize: i64, size: i64) -> bool {
        match self {
            OctNode::Empty => false,
            OctNode::Leaf(_) => true,
            OctNode::Branch(children) => {
                if qsize >= size {
                    // The query covers this whole node.
                    return self.is_full_solid();
                }
                let half = size / 2;
                let (idx, cx, cy, cz) = Self::child_index(qx, qy, qz, half);
                children[idx].region_full_solid(cx, cy, cz, qsize, half)
            }
        }
    }

    /// Append the integer grid coordinates of every solid *minimum* voxel in this
    /// node to `out`. Merged leaves are expanded into their constituent unit
    /// voxels, so the result is the full solid volume at leaf resolution — the
    /// input a parry `Voxels` collider expects. `min` is this node's minimum
    /// corner and `size` its edge length, both in voxels.
    pub fn collect_solid(&self, min: IVec3, size: i64, out: &mut Vec<IVec3>) {
        match self {
            OctNode::Empty => {}
            OctNode::Leaf(_) => {
                for x in 0..size as i32 {
                    for y in 0..size as i32 {
                        for z in 0..size as i32 {
                            out.push(min + IVec3::new(x, y, z));
                        }
                    }
                }
            }
            OctNode::Branch(children) => {
                let half = (size / 2) as i32;
                for (i, child) in children.iter().enumerate() {
                    let offset = IVec3::new(
                        (i & 1) as i32 * half,
                        ((i >> 1) & 1) as i32 * half,
                        ((i >> 2) & 1) as i32 * half,
                    );
                    child.collect_solid(min + offset, size / 2, out);
                }
            }
        }
    }

    /// A representative voxel for this node, used when a `Branch` is rendered as a
    /// single coarse cube at a distant level of detail. Returns the first solid
    /// leaf encountered in child order, or `None` if the node is entirely empty.
    pub fn representative(&self) -> Option<Voxel> {
        match self {
            OctNode::Empty => None,
            OctNode::Leaf(v) => Some(*v),
            OctNode::Branch(children) => children.iter().find_map(|c| c.representative()),
        }
    }

    /// Collapse eight freshly built children into the most compact node that
    /// represents them: `Empty` if all empty, a single `Leaf` if all leaves share
    /// a material, otherwise a `Branch`.
    pub fn from_children(children: [OctNode; CHILD_COUNT]) -> OctNode {
        // All empty -> Empty.
        if children.iter().all(|c| matches!(c, OctNode::Empty)) {
            return OctNode::Empty;
        }

        // All leaves of the same material -> a single merged Leaf.
        let first_material = match &children[0] {
            OctNode::Leaf(v) => Some(v.material),
            _ => None,
        };
        if let Some(material) = first_material
            && children
                .iter()
                .all(|c| matches!(c, OctNode::Leaf(v) if v.material == material))
        {
            return OctNode::Leaf(Voxel::new(material));
        }

        OctNode::Branch(Box::new(children))
    }
}
