use avian3d::prelude::*;
use bevy::{
    app::{App, FixedUpdate, Plugin, Startup, Update},
    asset::Assets,
    camera::{Camera, Camera3d},
    color::Color,
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        hierarchy::ChildOf,
        query::{With, Without},
        schedule::IntoScheduleConfigs,
        system::{Commands, Query, Res, ResMut},
    },
    input::{gamepad::GamepadButton, keyboard::KeyCode},
    log::{info, warn},
    math::{Dir3, Vec3, primitives::Sphere},
    mesh::{Mesh, Mesh3d, Meshable},
    pbr::{MeshMaterial3d, StandardMaterial},
    transform::components::Transform,
    utils::default,
};
use bevy_enhanced_input::{
    EnhancedInputPlugin, action::Action, actions, bindings, prelude::InputContextAppExt,
};
use kosim_camera::GameCamera;
use kosim_utility::interpolated_value::InterpolatedValue;

use crate::{
    action::{
        ACTION_STEP_DELTA_DEFAULT, ActionStep, Crouch, FootstepDirection, FootstepEvent, Sprint,
        detect_action_crouching, detect_action_jumping, detect_action_sprinting, load_footstep_sfx,
        play_footstep_sfx, tick_footstep,
    },
    body::{
        Body, IgnoreRayCollision, StandingSpringForce,
        apply_standing_spring_force, lock_angular_velocity,
    },
    config::PlayerControlConfig,
    debug::{
        create_player_debug, update_debug_is_moving, update_debug_is_sprinting,
        update_debug_linear_velocity, update_debug_movement_speed_current,
        update_debug_movement_speed_target, update_debug_movement_vector_current,
        update_debug_movement_vector_decay, update_debug_movement_vector_target,
        update_debug_position, update_debug_rotation,
    },
    focus::{Focus, camera_look_system},
    motion::{Motion, TouchedEntities, player_motion_system, player_rotation_system, run_move_and_slide},
    stance::{Stance, StanceType, compute_next_stance},
};

pub mod action;
pub mod body;
pub mod config;
pub mod debug;
pub mod focus;
pub mod motion;
pub mod stance;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(PlayerControlConfig::default()); // later we will load from some toml file
        app.add_plugins(EnhancedInputPlugin)
            .add_input_context::<Player>();
        app.add_systems(
            Startup,
            (
                spawn_player,
                load_footstep_sfx,
                attached_camera_system,
                create_player_debug,
            )
                .chain(),
        );
        app.add_systems(
            FixedUpdate,
            (
                camera_look_system,
                player_rotation_system,
                player_motion_system,
                run_move_and_slide,
                compute_next_stance,
                detect_action_jumping,
                detect_action_crouching,
                detect_action_sprinting,
                apply_standing_spring_force,
                lock_angular_velocity,
                play_footstep_sfx,
                tick_footstep,
            )
                .chain(),
        );
        app.add_systems(
            Update,
            (
                // BUG: having this in Update causes physics bugs but its choppy in FixedUpdate.
                update_debug_movement_vector_decay,
                update_debug_movement_vector_current,
                update_debug_movement_vector_target,
                update_debug_movement_speed_current,
                update_debug_movement_speed_target,
                update_debug_linear_velocity,
                update_debug_is_sprinting,
                update_debug_is_moving,
                update_debug_rotation,
                update_debug_position,
            )
                .chain(),
        );
        app.add_message::<FootstepEvent>();
        // info!("Initialized Player plugin");
    }
}

#[derive(Component)]
pub struct Player;

#[derive(Bundle)]
pub struct PlayerBundle {
    constant_force: ConstantForce,
    linear_velocity: LinearVelocity,
    impulse_force: ConstantLinearAcceleration,
    downward_ray: RayCaster,
    ray_hits: RayHits,
    body: Body,
    motion: Motion,
    focus: Focus,
    stance: Stance,
    standing_spring_force: StandingSpringForce,
    action_step: ActionStep,
    mass: Mass,
    locked_axes: LockedAxes,
    gravity_scale: GravityScale,
    transform: Transform,
    rigid_body: RigidBody,
}

pub fn spawn_player(
    player_config: Res<PlayerControlConfig>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut collider = Collider::capsule(0.5, 1.0);
    collider.set_scale(Vec3::from([1.0, 1.0, 1.0]), 10);

    commands
        .spawn((
            PlayerBundle {
                constant_force: ConstantForce::new(0.0, 0.0, 0.0),
                linear_velocity: LinearVelocity::from(Vec3::ZERO),
                impulse_force: ConstantLinearAcceleration::new(0.0, 0.0, 0.0),
                gravity_scale: GravityScale(1.0),
                transform: Transform::from_xyz(0.0, 16.0, 0.0),
                downward_ray: RayCaster::new(Vec3::ZERO, Dir3::NEG_Y),
                ray_hits: RayHits::default(),
                rigid_body: RigidBody::Dynamic,
                locked_axes: LockedAxes::new()
                    .lock_rotation_z()
                    .lock_rotation_x()
                    .lock_rotation_y(),
                mass: Mass(20.0),
                body: Body {
                    current_body_height: 1.0,
                },
                motion: Motion {
                    linear_velocity_interp: InterpolatedValue::new(
                        Vec3::from_array([0.0, 0.0, 0.0]),
                        16.0,
                    ),
                    movement_vector: InterpolatedValue::new(
                        Vec3::from_array([0.0, 0.0, 0.0]),
                        16.0,
                    ),
                    movement_speed: InterpolatedValue::new(
                        player_config.default_movement_speed,
                        4.0,
                    ),
                    sprinting: false,
                    moving: false,
                },
                stance: Stance {
                    current: StanceType::Standing,
                    crouched: false,
                    lockout_timer: 0.0,
                },
                focus: Focus {},
                action_step: ActionStep {
                    dir: FootstepDirection::Right,
                    delta: ACTION_STEP_DELTA_DEFAULT,
                    bumped: false,
                },
                standing_spring_force: StandingSpringForce {
                    length: InterpolatedValue::new(player_config.ride_height, 6.0),
                    extension: player_config.ray_length_offset,
                },
            },
            Mesh3d(meshes.add(Sphere::new(0.2).mesh().ico(8).unwrap())),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 200.0 / 256.0, 0.0),
                ..default()
            })),
            TransformInterpolation,
            CustomPositionIntegration,
            TouchedEntities::default(),
            CollidingEntities::default(),
            collider.clone(),
            IgnoreRayCollision,
            Player,
            actions!(Player[(Action::<Crouch>::new(), bindings![KeyCode::ControlLeft, GamepadButton::LeftThumb]),(
                Action::<Sprint>::new(), bindings![KeyCode::ShiftLeft, GamepadButton::South])])
        ));
        info!("Spawned Player Actor");
}

fn attached_camera_system(
    mut commands: Commands,
    mut player_query: Query<(Entity, &mut Transform), (With<Player>, Without<Camera>)>,
    mut camera_query: Query<
        (Entity, &mut Transform, Option<&ChildOf>),
        (With<Camera3d>, With<GameCamera>, Without<Player>),
    >,
) {
    if camera_query.is_empty()
        || camera_query.iter().len() > 1
        || player_query.is_empty()
        || player_query.iter().len() > 1
    {
        warn!(
            "The Camera attach system did not recieve 1 player and 1 camera. Found {} cameras, and {} players",
            camera_query.iter().len(),
            player_query.iter().len()
        );
    }

    for (player_entity, _player_transform) in &mut player_query {
        for (camera_entity, mut camera_transform, camera_parent) in &mut camera_query {
            camera_transform.translation = Vec3::from_array([0.0, 1.0, 0.0]);
            if camera_parent.is_none() {
                commands
                    .entity(player_entity)
                    .add_children(&[camera_entity]);
                info!("Attached Camera to player character as child");
            } else {
                info!("Camera parent already exists, will not set player as parent!");
            }
        }
    }
}
