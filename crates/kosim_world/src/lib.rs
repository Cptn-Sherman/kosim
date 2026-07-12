//! `kosim_world` — the voxelised world state for Kosim.
//!
//! The world is a cubic volume of voxels stored in a compressed [`octree`]. The
//! smallest voxel is [`WorldConfig::min_voxel_size`] units (0.25 by default);
//! larger uniform regions are merged into single octree leaves. A camera-driven
//! [`lod`] pass re-meshes the world at a level of detail that follows the camera:
//! nearby terrain is drawn at full 0.25-unit resolution, distant terrain as
//! progressively coarser cubes.
//!
//! A sample scene is produced procedurally from fractal noise (see
//! [`generation`]).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use avian3d::prelude::{Collider, RigidBody};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};

pub mod generation;
pub mod lod;
pub mod octree;
pub mod tables;
pub mod voxel;

use octree::OctNode;
use voxel::VoxelMaterial;

/// Default edge length of the smallest voxel, in world units.
pub const DEFAULT_MIN_VOXEL_SIZE: f32 = 0.5;
/// Default octree depth. `2^8 = 256` voxels per axis → a 128-unit world at
/// 0.5-unit resolution. The world is large enough that camera-driven LOD
/// (Phase 2) actually engages and can be seen; the trade-off is a heavier
/// full-resolution startup mesh and collider.
pub const DEFAULT_MAX_DEPTH: u32 = 8;

/// Tunable parameters for the voxel world.
#[derive(Resource, Clone, Debug)]
pub struct WorldConfig {
    /// Edge length of a single leaf voxel, in world units.
    pub min_voxel_size: f32,
    /// Octree depth. The world spans `2^max_depth` voxels on each axis.
    pub max_depth: u32,
    /// World-space position of the root's minimum corner.
    pub origin: Vec3,
    /// Level-of-detail aggressiveness. A node is subdivided while
    /// `world_size / distance_to_camera > lod_threshold`; smaller values keep
    /// detail out to greater distances.
    pub lod_threshold: f32,
    /// The camera must move at least this far (world units) before the LOD mesh
    /// is rebuilt.
    pub rebuild_distance: f32,
    /// Seed for procedural generation.
    pub seed: u32,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            min_voxel_size: DEFAULT_MIN_VOXEL_SIZE,
            max_depth: DEFAULT_MAX_DEPTH,
            // Centre the world horizontally on the origin and place its top face
            // at y = 2 (just under the ground platform): a 128-unit world spanning
            // y ∈ [-126, 2], so terrain peaks sit a few units below the platform.
            origin: Vec3::new(-64.0, -126.0, -64.0),
            // Higher = LOD coarsens sooner (closer). At 0.5 the finest chunks
            // (8-unit) are used within ~32 units and terrain steps down through
            // coarser steps beyond that, so LOD is visible across the 128-unit world.
            lod_threshold: 0.5,
            // Rebuild the LOD mesh only after the camera moves this far, to bound
            // how often the (whole-world) remesh runs.
            rebuild_distance: 4.0,
            seed: 0,
        }
    }
}

/// The voxel world state: the octree plus the geometry needed to address it.
#[derive(Resource)]
pub struct VoxelWorld {
    pub root: OctNode,
    /// Voxels per axis (`2^max_depth`).
    pub dim: i64,
    pub config: WorldConfig,
}

impl VoxelWorld {
    /// Generate a fresh world from `config` using the procedural sample scene.
    pub fn generate(config: WorldConfig) -> Self {
        let dim = 1i64 << config.max_depth;
        let root = generation::generate_terrain(dim, config.seed);
        Self { root, dim, config }
    }

    /// Is the voxel at integer voxel coordinates `(x, y, z)` solid? Coordinates
    /// outside the world are empty.
    #[inline]
    pub fn is_solid_voxel(&self, x: i64, y: i64, z: i64) -> bool {
        self.root.is_solid(x, y, z, self.dim)
    }

    /// Material of the voxel at `(x, y, z)`, or `None` if it is empty/outside.
    #[inline]
    pub fn voxel_material(&self, x: i64, y: i64, z: i64) -> Option<VoxelMaterial> {
        self.root.voxel_at(x, y, z, self.dim).map(|v| v.material)
    }

    /// Is the grid-aligned region `[min, min + size)` entirely solid? Regions
    /// that are not fully within the world are treated as not solid, so the outer
    /// shell of the world renders its faces.
    pub fn region_full_solid(&self, min: IVec3, size: i64) -> bool {
        if min.x < 0
            || min.y < 0
            || min.z < 0
            || (min.x as i64 + size) > self.dim
            || (min.y as i64 + size) > self.dim
            || (min.z as i64 + size) > self.dim
        {
            return false;
        }
        self.root
            .region_full_solid(min.x as i64, min.y as i64, min.z as i64, size, self.dim)
    }

    /// Every solid minimum-voxel grid coordinate in the world, at leaf
    /// resolution. Feeds a parry `Voxels` collider (see [`Collider::voxels`]).
    pub fn solid_voxel_coords(&self) -> Vec<IVec3> {
        let mut coords = Vec::new();
        self.root.collect_solid(IVec3::ZERO, self.dim, &mut coords);
        coords
    }

    /// Is the point `p` (world space) inside solid matter?
    pub fn is_solid_world(&self, p: Vec3) -> bool {
        let local = (p - self.config.origin) / self.config.min_voxel_size;
        self.is_solid_voxel(
            local.x.floor() as i64,
            local.y.floor() as i64,
            local.z.floor() as i64,
        )
    }
}

/// Marks a rendered leaf-chunk entity with the chunk it represents.
#[derive(Component)]
pub struct TerrainChunk(pub lod::ChunkKey);

/// Owns the streamed voxel world and the currently-rendered set of LOD chunks.
///
/// Each visible leaf chunk is its own entity/mesh keyed by [`lod::ChunkKey`]. As the
/// camera moves, only chunks whose LOD changed are added or removed, and their
/// meshing runs on the async compute pool — so the main thread never blocks on a
/// remesh. The collider is separate and full-resolution (see [`setup_world`]).
#[derive(Resource)]
pub struct ChunkManager {
    world: Arc<VoxelWorld>,
    material: Handle<StandardMaterial>,
    /// Chunks currently spawned, by key.
    active: HashMap<lod::ChunkKey, Entity>,
    /// Chunks whose mesh is being built off-thread.
    pending: HashMap<lod::ChunkKey, Task<Mesh>>,
    /// Camera position the desired set was last computed for.
    last_camera_pos: Vec3,
}

/// Registers the voxel world: generates the sample scene and full-resolution static
/// collider on startup, then streams camera-driven LOD chunks in and out.
pub struct KosimWorldPlugin;

impl Plugin for KosimWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldConfig>()
            .add_systems(Startup, setup_world)
            .add_systems(Update, (schedule_chunk_meshing, apply_finished_chunks).chain());
    }
}

fn setup_world(
    mut commands: Commands,
    config: Res<WorldConfig>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let world = VoxelWorld::generate(config.clone());
    info!(
        "kosim_world: generated {dim}^3 voxel world ({size} units, {mvs}-unit voxels)",
        dim = world.dim,
        size = world.dim as f32 * config.min_voxel_size,
        mvs = config.min_voxel_size,
    );

    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        ..default()
    });

    // Static collider from the *full-resolution* isosurface (independent of the
    // visual LOD, so physics is consistent everywhere). Built from the same welded
    // buffers as the mesher, so it matches the rendered slopes; positions are
    // already world-space, so the collider entity sits at the origin.
    let collider_mesh = lod::build_terrain_mesh(&world);
    info!(
        "kosim_world: collider isosurface has {} vertices, {} triangles",
        collider_mesh.collider_vertices.len(),
        collider_mesh.collider_indices.len(),
    );
    match Collider::try_trimesh(collider_mesh.collider_vertices, collider_mesh.collider_indices) {
        Ok(collider) => {
            commands.spawn((
                Name::new("VoxelWorldCollider"),
                RigidBody::Static,
                collider,
                Transform::IDENTITY,
            ));
        }
        Err(err) => {
            error!("kosim_world: failed to build terrain trimesh collider: {err:?}");
        }
    }

    commands.insert_resource(ChunkManager {
        world: Arc::new(world),
        material,
        active: HashMap::new(),
        pending: HashMap::new(),
        // A sentinel far from any real camera forces a first pass on the first Update.
        last_camera_pos: Vec3::splat(f32::INFINITY),
    });
}

/// When the camera has moved far enough, diff the desired chunk set against what is
/// live: despawn chunks that are no longer wanted and spawn async meshing tasks for
/// newly wanted ones. Unchanged chunks are left untouched (the incremental win).
fn schedule_chunk_meshing(
    mut manager: ResMut<ChunkManager>,
    mut commands: Commands,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    let Ok(camera_transform) = camera.single() else {
        return;
    };
    let camera_pos = camera_transform.translation();
    if camera_pos.distance(manager.last_camera_pos) < manager.world.config.rebuild_distance {
        return;
    }
    manager.last_camera_pos = camera_pos;

    let desired: HashSet<lod::ChunkKey> = lod::desired_chunks(&manager.world, camera_pos)
        .into_iter()
        .collect();

    // Despawn chunks and drop in-flight work that is no longer wanted.
    let stale: Vec<lod::ChunkKey> = manager
        .active
        .keys()
        .filter(|k| !desired.contains(k))
        .copied()
        .collect();
    for key in stale {
        if let Some(entity) = manager.active.remove(&key) {
            commands.entity(entity).despawn();
        }
    }
    manager.pending.retain(|key, _| desired.contains(key));

    // Spawn async meshing for newly wanted chunks.
    let pool = AsyncComputeTaskPool::get();
    for key in desired {
        if manager.active.contains_key(&key) || manager.pending.contains_key(&key) {
            continue;
        }
        let world = manager.world.clone();
        let (region_min, size, sides) = key;
        let task = pool.spawn(async move { lod::mesh_one_chunk(&world, region_min, size, sides) });
        manager.pending.insert(key, task);
    }
}

/// Poll in-flight chunk meshes; spawn an entity for each one that finished this frame.
fn apply_finished_chunks(
    mut manager: ResMut<ChunkManager>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let mut finished: Vec<(lod::ChunkKey, Mesh)> = Vec::new();
    manager.pending.retain(|key, task| {
        match block_on(future::poll_once(&mut *task)) {
            Some(mesh) => {
                finished.push((*key, mesh));
                false
            }
            None => true,
        }
    });

    for (key, mesh) in finished {
        let handle = meshes.add(mesh);
        let material = manager.material.clone();
        let entity = commands
            .spawn((
                Name::new("TerrainChunk"),
                Mesh3d(handle),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                TerrainChunk(key),
            ))
            .id();
        // Replace any prior entity for this key (e.g. a re-requested chunk).
        if let Some(old) = manager.active.insert(key, entity) {
            commands.entity(old).despawn();
        }
    }
}
