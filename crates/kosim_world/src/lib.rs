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
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, futures_lite::future};

pub mod fade;
pub mod generation;
pub mod lod;
pub mod voxel;

use fade::{ChunkFade, ChunkMaterial, FADE_SECONDS, Fade, RETIRE_SECONDS};
use voxel::VoxelMaterial;

/// Default edge length of the smallest voxel, in world units.
pub const DEFAULT_MIN_VOXEL_SIZE: f32 = 0.5;
/// Default octree depth. `2^11 = 2048` voxels per axis → a 1024-unit world at
/// 0.5-unit resolution (~8× the earlier 128-unit world). This is only feasible
/// because the world is sampled procedurally on demand (no eager octree) and
/// colliders/meshes are streamed per chunk near the camera — nothing is built over
/// the whole `dim³` volume.
pub const DEFAULT_MAX_DEPTH: u32 = 11;

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
            // Centre the cube on the world origin so the planet's centre is at
            // (0, 0, 0). A 2048-voxel cube at 0.5 units spans [-512, 512]; the planet
            // radius is 0.42 * 2048 * 0.5 ≈ 430 units, so its surface reaches ~y=430.
            origin: Vec3::new(-512.0, -512.0, -512.0),
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

/// The voxel world state: the procedural planet generator plus the geometry needed
/// to address it. Voxels are evaluated on demand (see [`generation`]) rather than
/// stored, so the world uses no memory proportional to its size.
#[derive(Resource)]
pub struct VoxelWorld {
    pub generator: generation::PlanetGenerator,
    /// Voxels per axis (`2^max_depth`).
    pub dim: i64,
    pub config: WorldConfig,
}

impl VoxelWorld {
    /// Create a fresh world from `config`. Nothing is generated eagerly.
    pub fn generate(config: WorldConfig) -> Self {
        let dim = 1i64 << config.max_depth;
        let generator = generation::PlanetGenerator::new(dim, config.seed);
        Self {
            generator,
            dim,
            config,
        }
    }

    /// Is the voxel at integer voxel coordinates `(x, y, z)` solid?
    #[inline]
    pub fn is_solid_voxel(&self, x: i64, y: i64, z: i64) -> bool {
        self.generator.is_solid(x, y, z)
    }

    /// Material of the voxel at `(x, y, z)`, or `None` if it is outside the planet.
    #[inline]
    pub fn voxel_material(&self, x: i64, y: i64, z: i64) -> Option<VoxelMaterial> {
        self.generator.material_at_voxel(x, y, z)
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
    /// The terrain texture array (one procedural layer per material).
    terrain_array: Handle<Image>,
    /// Chunks currently wanted and spawned, by key.
    active: HashMap<lod::ChunkKey, Entity>,
    /// Chunks whose mesh is being built off-thread.
    pending: HashMap<lod::ChunkKey, Task<Mesh>>,
    /// Chunks that have left the desired set and are dissolving out before despawn.
    retiring: HashMap<lod::ChunkKey, Entity>,
    /// Camera position the desired set was last computed for.
    last_camera_pos: Vec3,
}

/// Registers the voxel world: generates the sample scene and full-resolution static
/// collider on startup, then streams camera-driven LOD chunks in and out with a
/// dithered crossfade.
pub struct KosimWorldPlugin;

impl Plugin for KosimWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldConfig>()
            .add_plugins(MaterialPlugin::<ChunkMaterial>::default())
            .add_systems(Startup, setup_world)
            .add_systems(
                Update,
                (schedule_chunk_meshing, apply_finished_chunks, animate_fades).chain(),
            );
    }
}

fn setup_world(
    mut commands: Commands,
    config: Res<WorldConfig>,
    mut images: ResMut<Assets<Image>>,
) {
    let world = VoxelWorld::generate(config.clone());
    info!(
        "kosim_world: generated {dim}^3 voxel world ({size} units, {mvs}-unit voxels)",
        dim = world.dim,
        size = world.dim as f32 * config.min_voxel_size,
        mvs = config.min_voxel_size,
    );

    let terrain_array = images.add(fade::build_terrain_texture_array());

    // No whole-world collider: it was O(dim^3) to build and a giant static trimesh,
    // which caps the world size. Instead each *finest* streamed chunk near the player
    // gets its own trimesh collider (see `apply_finished_chunks`), so collision cost
    // is bounded regardless of how large the planet is.

    commands.insert_resource(ChunkManager {
        world: Arc::new(world),
        terrain_array,
        active: HashMap::new(),
        pending: HashMap::new(),
        retiring: HashMap::new(),
        // A sentinel far from any real camera forces a first pass on the first Update.
        last_camera_pos: Vec3::splat(f32::INFINITY),
    });
}

/// When the camera has moved far enough, diff the desired chunk set against what is
/// live: despawn chunks that are no longer wanted and spawn async meshing tasks for
/// newly wanted ones. Unchanged chunks are left untouched (the incremental win).
fn schedule_chunk_meshing(
    mut manager: ResMut<ChunkManager>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut fades: Query<&mut Fade>,
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

    // Active chunks that are no longer wanted start dissolving out (kept alive until
    // fully faded so they cross-dissolve with their replacements).
    let stale: Vec<lod::ChunkKey> = manager
        .active
        .keys()
        .filter(|k| !desired.contains(k))
        .copied()
        .collect();
    for key in stale {
        if let Some(entity) = manager.active.remove(&key) {
            if let Ok(mut fade) = fades.get_mut(entity) {
                fade.retiring = true;
                fade.timer = 0.0;
            }
            manager.retiring.insert(key, entity);
        }
    }

    // Chunks that were retiring but are wanted again: they are already opaque, so
    // just make them active again.
    let revived: Vec<lod::ChunkKey> = manager
        .retiring
        .keys()
        .filter(|k| desired.contains(k))
        .copied()
        .collect();
    for key in revived {
        if let Some(entity) = manager.retiring.remove(&key) {
            if let Ok(mut fade) = fades.get_mut(entity) {
                fade.retiring = false;
                fade.value = 1.0;
            }
            manager.active.insert(key, entity);
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

/// Poll in-flight chunk meshes; spawn an entity for each one that finished this
/// frame, starting it dissolving in from transparent.
fn apply_finished_chunks(
    mut manager: ResMut<ChunkManager>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ChunkMaterial>>,
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
        // Finest chunks are the ones next to the player, so only they get a physics
        // collider (built from the same mesh). This bounds collision cost by view
        // distance, not world size. Build it before the mesh is moved into Assets.
        let (_, size, _) = key;
        let collider = if size == lod::CELLS_PER_CHUNK {
            Collider::trimesh_from_mesh(&mesh)
        } else {
            None
        };

        let handle = meshes.add(mesh);
        // Each chunk owns its material so it can fade independently. Vertex colours
        // carry the terrain material; base_color is white so they pass through.
        let material = materials.add(ChunkMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.95,
                metallic: 0.0,
                ..default()
            },
            extension: ChunkFade {
                fade: 0.0,
                array: Some(manager.terrain_array.clone()),
            },
        });
        let mut chunk = commands.spawn((
            Name::new("TerrainChunk"),
            Mesh3d(handle),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            TerrainChunk(key),
            Fade {
                value: 0.0,
                retiring: false,
                timer: 0.0,
            },
            // Don't cast a shadow *while dithering in*: the shadow pass ignores the
            // dither, so a fading-in chunk would pop a full shadow (a flash). Removed
            // once fully opaque (see `animate_fades`), so solid terrain self-shadows.
            NotShadowCaster,
        ));
        if let Some(collider) = collider {
            chunk.insert((RigidBody::Static, collider));
        }
        let entity = chunk.id();
        // Replace any prior entity for this key (e.g. a re-requested chunk).
        if let Some(old) = manager.active.insert(key, entity) {
            commands.entity(old).despawn();
        }
    }
}

/// Advance every chunk's dither fade. Chunks that finish fading out are despawned.
fn animate_fades(
    time: Res<Time>,
    mut manager: ResMut<ChunkManager>,
    mut commands: Commands,
    mut materials: ResMut<Assets<ChunkMaterial>>,
    mut chunks: Query<(Entity, &TerrainChunk, &mut Fade, &MeshMaterial3d<ChunkMaterial>)>,
) {
    let step = time.delta_secs() / FADE_SECONDS;
    for (entity, chunk, mut fade, material) in &mut chunks {
        if fade.retiring {
            // Opaque backing: snap to fully visible once and let it cast shadows
            // (it's the crossfade's solid geometry), then just count down.
            if fade.timer == 0.0 {
                if let Some(material) = materials.get_mut(&material.0) {
                    material.extension.fade = 1.0;
                }
                commands.entity(entity).remove::<NotShadowCaster>();
            }
            fade.timer += time.delta_secs();
            if fade.timer >= RETIRE_SECONDS {
                commands.entity(entity).despawn();
                manager.retiring.remove(&chunk.0);
            }
            continue;
        }
        if fade.value >= 1.0 {
            continue; // fully dithered in — nothing to animate
        }
        fade.value = (fade.value + step).min(1.0);
        if fade.value >= 1.0 {
            // Fully opaque now: start casting a shadow (self-shadowing terrain).
            commands.entity(entity).remove::<NotShadowCaster>();
        }
        if let Some(material) = materials.get_mut(&material.0) {
            material.extension.fade = fade.value;
        }
    }
}
