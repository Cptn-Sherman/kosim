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

use avian3d::prelude::{Collider, RigidBody};
use bevy::prelude::*;

pub mod generation;
pub mod lod;
pub mod octree;
pub mod voxel;

use octree::OctNode;

/// Default edge length of the smallest voxel, in world units.
pub const DEFAULT_MIN_VOXEL_SIZE: f32 = 0.5;
/// Default octree depth. `2^6 = 64` voxels per axis → a 32-unit world at
/// 0.5-unit resolution (double the walkable extent of the original 16-unit map).
pub const DEFAULT_MAX_DEPTH: u32 = 6;

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
            // Centre the world horizontally on the origin and sink it below the
            // ground platform (which sits at y = 2): a 32-unit world whose top
            // face reaches y = 2, so all terrain sits beneath the platform.
            origin: Vec3::new(-16.0, -30.0, -16.0),
            lod_threshold: 0.08,
            rebuild_distance: 1.0,
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

/// Tracks the rendered terrain entity and the camera position it was built for.
#[derive(Resource)]
pub struct WorldRenderState {
    pub entity: Entity,
    pub mesh: Handle<Mesh>,
    pub last_camera_pos: Vec3,
}

/// Registers the voxel world: generates the sample scene on startup and keeps
/// its level of detail in sync with the camera.
pub struct KosimWorldPlugin;

impl Plugin for KosimWorldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldConfig>()
            .add_systems(Startup, setup_world)
            .add_systems(Update, update_lod);
    }
}

fn setup_world(
    mut commands: Commands,
    config: Res<WorldConfig>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let world = VoxelWorld::generate(config.clone());
    info!(
        "kosim_world: generated {dim}^3 voxel world ({size} units, {mvs}-unit voxels)",
        dim = world.dim,
        size = world.dim as f32 * config.min_voxel_size,
        mvs = config.min_voxel_size,
    );

    // Initial mesh from the world origin; the LOD system refines it once the
    // camera's transform has propagated.
    let mesh = meshes.add(lod::build_lod_mesh(&world, config.origin));

    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        metallic: 0.0,
        ..default()
    });

    let entity = commands
        .spawn((
            Name::new("VoxelWorld"),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
            Transform::IDENTITY,
        ))
        .id();

    // Full-resolution static collision. Unlike the visual mesh this does not
    // follow the camera LOD: it is generated once from every solid voxel so
    // physics is consistent everywhere. A parry `Voxels` collider places voxel
    // `(x, y, z)` centred at `((x, y, z) + 0.5) * voxel_size` in local space, so
    // translating the collider entity by the world origin aligns it exactly with
    // the rendered terrain.
    let coords = world.solid_voxel_coords();
    info!(
        "kosim_world: building voxel collider from {} solid voxels",
        coords.len()
    );
    commands.spawn((
        Name::new("VoxelWorldCollider"),
        RigidBody::Static,
        Collider::voxels(Vec3::splat(config.min_voxel_size), &coords),
        Transform::from_translation(config.origin),
    ));

    commands.insert_resource(WorldRenderState {
        entity,
        mesh,
        last_camera_pos: config.origin,
    });
    commands.insert_resource(world);
}

fn update_lod(
    world: Res<VoxelWorld>,
    mut state: ResMut<WorldRenderState>,
    mut meshes: ResMut<Assets<Mesh>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    let Ok(camera_transform) = camera.single() else {
        return;
    };
    let camera_pos = camera_transform.translation();

    if camera_pos.distance(state.last_camera_pos) < world.config.rebuild_distance {
        return;
    }
    state.last_camera_pos = camera_pos;

    let new_mesh = lod::build_lod_mesh(&world, camera_pos);
    if let Some(mesh) = meshes.get_mut(&state.mesh) {
        *mesh = new_mesh;
    }
}
