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
use std::sync::{Arc, OnceLock};

use avian3d::prelude::{Collider, RigidBody};
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, TaskPoolBuilder, block_on, futures_lite::future};

pub mod fade;
pub mod generation;
pub mod lod;
pub mod voxel;

use fade::{ChunkFade, ChunkMaterial, DISSOLVE_SECONDS, FADE_SECONDS, Fade, RETIRE_SECONDS};
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
            // A region subdivides while `world_size / dist > lod_threshold`, so the
            // finest chunks (8-unit) reach out to ~16/threshold units. Lower = finest
            // detail extends farther (at the cost of more chunks/colliders).
            lod_threshold: 0.25,
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

    /// Might the voxel region `[region_min, region_min + size)` contain any surface?
    /// Used to prune empty air / solid-interior regions from the LOD walk.
    #[inline]
    pub fn region_has_surface(&self, region_min: IVec3, size: i64) -> bool {
        self.generator
            .region_has_surface(region_min.x as i64, region_min.y as i64, region_min.z as i64, size)
    }
}

/// Dedicated task pool for chunk meshing. Meshing must NOT share Bevy's
/// `AsyncComputeTaskPool`: avian spawns its collider-tree optimization there each
/// physics step and *blocks* on it at the end of the step — a wave of queued mesh
/// jobs in front of it stalled the main thread for tens of milliseconds (the
/// movement lag spikes). A separate pool keeps the two workloads from queueing
/// behind each other.
fn mesh_pool() -> &'static TaskPool {
    static POOL: OnceLock<TaskPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2))
            .unwrap_or(2)
            .clamp(1, 8);
        TaskPoolBuilder::new()
            .num_threads(threads)
            .thread_name("kosim-mesh".to_string())
            .build()
    })
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
    /// Chunks whose mesh (and, for finest chunks, collider) is being built off-thread.
    pending: HashMap<lod::ChunkKey, Task<(Mesh, Option<Collider>)>>,
    /// Chunks that have left the desired set and are dissolving out before despawn.
    retiring: HashMap<lod::ChunkKey, Entity>,
    /// Chunks that meshed to nothing (air / solid interior). Cached so they are never
    /// re-meshed or spawned as (invisible) entities — on a planet most chunks are
    /// empty, and rendering them was the bulk of the draw calls.
    empty: HashSet<lod::ChunkKey>,
    /// Colliders waiting to be attached to their (already spawned) chunk entities.
    /// Registering a static trimesh in the physics world is main-thread work that
    /// spikes badly in bulk, so they are drained a few per frame (see
    /// [`attach_queued_colliders`]).
    collider_queue: Vec<(Entity, Vec3, Collider)>,
    /// The desired-set computation runs off the main thread (it walks/balances the
    /// whole LOD tree). At most one is in flight; its result is diffed when ready.
    desired_task: Option<Task<Vec<lod::ChunkKey>>>,
    /// Camera position the desired set was last computed for.
    last_camera_pos: Vec3,
}

impl ChunkManager {
    /// Read access to the streamed voxel world (e.g. for probing terrain height).
    pub fn world(&self) -> &VoxelWorld {
        &self.world
    }
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
                (
                    schedule_chunk_meshing,
                    apply_finished_chunks,
                    attach_queued_colliders,
                    update_morph_factors,
                    animate_fades,
                )
                    .chain(),
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
        empty: HashSet::new(),
        collider_queue: Vec::new(),
        desired_task: None,
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
    mut fades: Query<(&mut Fade, &MeshMaterial3d<ChunkMaterial>)>,
    mut materials: ResMut<Assets<ChunkMaterial>>,
) {
    // 1. If an off-thread desired-set computation finished, diff it against the live
    // chunks. This is the only place the (large) desired set touches the main thread,
    // and it is just hash-set bookkeeping — the walk/balance itself ran on a task.
    let ready = manager
        .desired_task
        .as_mut()
        .and_then(|task| block_on(future::poll_once(task)));
    if let Some(desired_vec) = ready {
        manager.desired_task = None;
        let desired: HashSet<lod::ChunkKey> = desired_vec.into_iter().collect();

        // Forget cached-empty chunks that are no longer wanted, to bound memory.
        manager.empty.retain(|k| desired.contains(k));

        // Active chunks that are no longer wanted start dissolving out (kept alive
        // until fully faded so they cross-dissolve with their replacements).
        let stale: Vec<lod::ChunkKey> = manager
            .active
            .keys()
            .filter(|k| !desired.contains(k))
            .copied()
            .collect();
        for key in stale {
            if let Some(entity) = manager.active.remove(&key) {
                if let Ok((mut fade, _)) = fades.get_mut(entity) {
                    fade.retiring = true;
                    fade.timer = 0.0;
                }
                manager.retiring.insert(key, entity);
            }
        }

        // Chunks that were retiring but are wanted again: snap back to fully opaque
        // (they may have been mid-dissolve).
        let revived: Vec<lod::ChunkKey> = manager
            .retiring
            .keys()
            .filter(|k| desired.contains(k))
            .copied()
            .collect();
        for key in revived {
            if let Some(entity) = manager.retiring.remove(&key) {
                if let Ok((mut fade, material)) = fades.get_mut(entity) {
                    fade.retiring = false;
                    fade.value = 1.0;
                    if let Some(material) = materials.get_mut(&material.0) {
                        material.extension.params.x = 1.0;
                    }
                }
                manager.active.insert(key, entity);
            }
        }

        manager.pending.retain(|key, _| desired.contains(key));

        // Spawn async meshing for newly wanted chunks (on the dedicated mesh pool —
        // see `mesh_pool` for why not `AsyncComputeTaskPool`).
        let pool = mesh_pool();
        for key in desired {
            if manager.active.contains_key(&key)
                || manager.pending.contains_key(&key)
                || manager.empty.contains(&key)
            {
                continue;
            }
            let world = manager.world.clone();
            let (region_min, size, sides) = key;
            let task = pool.spawn(async move {
                let mesh = lod::mesh_one_chunk(&world, region_min, size, sides);
                // Build the collider here (off the main thread); only finest chunks,
                // which are next to the player, need one.
                let collider = if size == lod::CELLS_PER_CHUNK {
                    Collider::trimesh_from_mesh(&mesh)
                } else {
                    None
                };
                (mesh, collider)
            });
            manager.pending.insert(key, task);
        }
    }

    // 2. Start a new desired-set computation off-thread when the camera has moved far
    // enough and none is already running.
    if manager.desired_task.is_none() {
        let Ok(camera_transform) = camera.single() else {
            return;
        };
        let camera_pos = camera_transform.translation();
        if camera_pos.distance(manager.last_camera_pos) >= manager.world.config.rebuild_distance {
            manager.last_camera_pos = camera_pos;
            let world = manager.world.clone();
            manager.desired_task = Some(
                AsyncComputeTaskPool::get().spawn(async move { lod::desired_chunks(&world, camera_pos) }),
            );
        }
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
    // Apply at most this many finished chunks per frame. A fast flight can finish a
    // few hundred at once; handing them all to the renderer in one frame spikes the
    // GPU mesh upload/prepare. Spreading them over frames is invisible thanks to the
    // dither fade-in.
    const MAX_APPLY_PER_FRAME: usize = 24;

    let mut finished: Vec<(lod::ChunkKey, Mesh, Option<Collider>)> = Vec::new();
    let mut done_keys: Vec<lod::ChunkKey> = Vec::new();
    for (key, task) in manager.pending.iter_mut() {
        if finished.len() >= MAX_APPLY_PER_FRAME {
            break;
        }
        if let Some((mesh, collider)) = block_on(future::poll_once(&mut *task)) {
            finished.push((*key, mesh, collider));
            done_keys.push(*key);
        }
    }
    for key in done_keys {
        manager.pending.remove(&key);
    }

    for (key, mesh, collider) in finished {
        // Chunks with no surface (air / solid interior) render nothing: cache the key
        // so it is never re-meshed and never spawned as an invisible entity.
        if mesh.indices().map(|i| i.is_empty()).unwrap_or(true) {
            manager.empty.insert(key);
            continue;
        }

        // The collider (finest chunks only) was already built off-thread in the
        // meshing task, so there is no main-thread collision-build cost here.
        let handle = meshes.add(mesh);
        // Each chunk owns its material so it can fade independently. Vertex colours
        // carry the terrain material; base_color is white so they pass through.
        // `last_camera_pos` is close enough for the spawn-frame morph factor;
        // `update_morph_factors` keeps it current from then on.
        let morph = lod::morph_factor(&manager.world, key, manager.last_camera_pos);
        let material = materials.add(ChunkMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: 0.95,
                metallic: 0.0,
                // Mask (never actually cutting: alpha is always 1) so shadow
                // pipelines run the material's prepass fragment shader — that is
                // what lets the dither fade apply to shadows (see fade.rs).
                alpha_mode: AlphaMode::Mask(0.5),
                ..default()
            },
            extension: ChunkFade {
                params: Vec4::new(0.0, morph, 0.0, 0.0),
                array: Some(manager.terrain_array.clone()),
            },
        });
        // Shadows need no special handling across the fade: the prepass fragment
        // applies the same dither discard, so the chunk's shadow crossfades in
        // lockstep with its visible surface.
        let chunk = commands.spawn((
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
        ));
        let entity = chunk.id();
        if let Some(collider) = collider {
            let (region_min, size, _) = key;
            let center = manager.world.config.origin
                + (region_min.as_vec3() + Vec3::splat(size as f32 * 0.5))
                    * manager.world.config.min_voxel_size;
            manager.collider_queue.push((entity, center, collider));
        }
        // Replace any prior entity for this key (e.g. a re-requested chunk).
        if let Some(old) = manager.active.insert(key, entity) {
            commands.entity(old).despawn();
        }
    }
}

/// Keep every chunk's geomorph factor tracking its camera distance. The factor is a
/// per-chunk material uniform (see [`lod::morph_factor`] for why it is not computed
/// in the shader from the view). It is quantised, and a material is only marked
/// modified when its factor actually changes, so a stationary camera re-uploads
/// nothing and a moving one only touches the thin shell of chunks inside their
/// morph band.
fn update_morph_factors(
    manager: Res<ChunkManager>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    mut materials: ResMut<Assets<ChunkMaterial>>,
    chunks: Query<(&TerrainChunk, &MeshMaterial3d<ChunkMaterial>)>,
) {
    let Ok(camera_transform) = camera.single() else {
        return;
    };
    let camera_pos = camera_transform.translation();
    for (chunk, material) in &chunks {
        let morph = lod::morph_factor(&manager.world, chunk.0, camera_pos);
        // Compare through `get` first: `get_mut` flags the asset as modified (a GPU
        // re-prepare) even when nothing changed.
        if materials
            .get(&material.0)
            .is_some_and(|m| m.extension.params.y != morph)
            && let Some(m) = materials.get_mut(&material.0)
        {
            m.extension.params.y = morph;
        }
    }
}

/// Drain a few queued colliders per frame, nearest to the camera first. Registering
/// a static trimesh collider costs the physics world real main-thread time (~1 ms
/// each); attaching a whole wave of freshly meshed chunks in one frame was the main
/// source of movement lag spikes. Latency here is invisible: colliders only matter
/// right next to the player, and those chunks sort to the front.
fn attach_queued_colliders(
    mut manager: ResMut<ChunkManager>,
    mut commands: Commands,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    const MAX_PER_FRAME: usize = 4;
    if manager.collider_queue.is_empty() {
        return;
    }
    // Sort farthest-first so the nearest chunks pop off the tail.
    if let Ok(camera_transform) = camera.single() {
        let camera_pos = camera_transform.translation();
        manager.collider_queue.sort_unstable_by(|a, b| {
            let da = a.1.distance_squared(camera_pos);
            let db = b.1.distance_squared(camera_pos);
            db.total_cmp(&da)
        });
    }
    let take = manager.collider_queue.len().min(MAX_PER_FRAME);
    let at = manager.collider_queue.len() - take;
    for (entity, _, collider) in manager.collider_queue.split_off(at) {
        // The chunk may have been despawned (retired/replaced) while queued.
        if let Ok(mut chunk) = commands.get_entity(entity) {
            chunk.insert((RigidBody::Static, collider));
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
            // Opaque backing: snap to fully visible once (it's the crossfade's
            // solid geometry) while the replacement fades in.
            if fade.timer == 0.0
                && let Some(material) = materials.get_mut(&material.0)
            {
                material.extension.params.x = 1.0;
            }
            fade.timer += time.delta_secs();
            if fade.timer < RETIRE_SECONDS {
                continue;
            }
            // Backing period over: the replacement is fully opaque underneath, so
            // dither *out* to hand pixels over gradually — any residual geometry
            // mismatch (geomorph approximation, pinned border rings) resolves
            // pixel-by-pixel instead of popping on the despawn frame. The shadow
            // dissolves with it (dither applies in the shadow pass too).
            let remaining = 1.0 - (fade.timer - RETIRE_SECONDS) / DISSOLVE_SECONDS;
            if remaining <= 0.0 {
                commands.entity(entity).despawn();
                manager.retiring.remove(&chunk.0);
                continue;
            }
            if let Some(material) = materials.get_mut(&material.0) {
                material.extension.params.x = remaining;
            }
            continue;
        }
        if fade.value >= 1.0 {
            continue; // fully dithered in — nothing to animate
        }
        fade.value = (fade.value + step).min(1.0);
        if let Some(material) = materials.get_mut(&material.0) {
            material.extension.params.x = fade.value;
        }
    }
}
