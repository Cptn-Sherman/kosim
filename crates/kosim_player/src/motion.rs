use avian3d::{math::{AdjustPrecision, AsF32}, prelude::{
    Collider, ConstantForce, LinearVelocity, MoveAndSlide, MoveAndSlideConfig, MoveAndSlideHitResponse, MoveAndSlideOutput, SpatialQueryFilter
}};
use bevy::{
    color::palettes::tailwind, ecs::{
        component::Component, entity::{Entity, EntityHashSet}, query::With, system::{Query, Res}
    }, gizmos::gizmos::Gizmos, input::{ButtonInput, keyboard::KeyCode}, log::{trace, warn}, math::{EulerRot, Quat, Vec3}, prelude::{Deref, DerefMut}, time::Time, transform::components::Transform
};
use kosim_input::input::Input;
use kosim_utility::{
    exp_decay,
    format_value::{format_value_f32, format_value_vec3},
    interpolated_value::InterpolatedValue,
};

use crate::{
    Player,
    config::PlayerControlConfig, stance::{Stance, StanceType},
};

#[derive(Component)]
pub struct Motion {
    pub linear_velocity_interp: InterpolatedValue<Vec3>,
    pub movement_vector: InterpolatedValue<Vec3>,
    pub movement_speed: InterpolatedValue<f32>,
    pub sprinting: bool,
    pub moving: bool,
}

pub fn player_motion_system(
    mut player_query: Query<
        (&mut LinearVelocity, &mut Transform, &mut Motion, &Stance),
        With<Player>,
    >,
    player_config: Res<PlayerControlConfig>,
    input: Res<Input>,
    time: Res<Time>,
) {
    if player_query.is_empty() || player_query.iter().len() > 1 {
        warn!(
            "Player Motion System expected 1 player(s), recieved {}. Expect Instablity!",
            player_query.iter().len()
        );
        return;
    }

    let (mut linear_velocity, player_transform, mut motion, stance) =
        player_query.single_mut().expect("We do some errors");

    // * - COMPUTE CURRENT MOVEMENT SPEED AND LERP -

    if motion.sprinting == true {
        if stance.crouched == true {
            motion.movement_speed.target =
                player_config.default_movement_speed * 0.5 * player_config.sprint_speed_factor;
        } else {
            motion.movement_speed.target =
                player_config.default_movement_speed * player_config.sprint_speed_factor;
        }
    } else {
        if stance.crouched == false {
            motion.movement_speed.target = player_config.default_movement_speed;
        } else {
            motion.movement_speed.target = player_config.default_movement_speed * 0.5;
        }
    }

    // Apply lineaer interpolation to move the speed transition.
    motion.movement_speed.current = exp_decay(
        motion.movement_speed.current,
        motion.movement_speed.target,
        motion.movement_speed.decay,
        time.delta_secs(),
    );

    trace!(
        "Movement Speed current: {}, target: {}",
        format_value_f32(motion.movement_speed.current, Some(4), true),
        format_value_f32(motion.movement_speed.target, Some(4), true)
    );

    //* - UPDATE MOVEMENT_VECTOR AND LERP -

    let mut movement_vector: Vec3 = Vec3::ZERO.clone();
    
    // Apply the input_vector to the player to update the movement_vector.
    movement_vector += player_transform.forward().as_vec3() * input.movement_raw.z;
    movement_vector += player_transform.right().as_vec3() * input.movement_raw.x;

    // Update the target movement vector to be the normalized movement vector.
    motion.movement_vector.target = movement_vector.normalize_or_zero();

    // Lerp the current movement vector towards the target movement vector
    // updating the decay rate based on movement scale (based on being grounded or airborne)
    motion.movement_vector.current = exp_decay::<Vec3>(
        motion.movement_vector.current,
        motion.movement_vector.target,
        motion.movement_vector.decay,
        time.delta_secs(),
    );

    trace!(
        "Current Movement Vector: {}",
        format_value_vec3(motion.movement_vector.current, Some(4), true),
    );

    // * APPLY MOVEMENT_VECTOR TO PLAYER TRANSFORM LINEAR VELOCITY

    // We don't need to lerp here just setting the real value to as we already lerp the current_movement_vector and current_movement_speed.
    if stance.current == StanceType::Standing {
        motion.linear_velocity_interp.target.x =
            motion.movement_vector.current.x * motion.movement_speed.current;
        motion.linear_velocity_interp.target.z =
            motion.movement_vector.current.z * motion.movement_speed.current;
    } else {
        const PI: f32 = 3.1459;
        const SCALE: f32 = 0.2;
        const OFFSET: f32 = 0.3;

        let dot: f32 = motion
            .linear_velocity_interp
            .current
            .normalize_or_zero()
            .dot(motion.movement_vector.current);
        let air_time_scale: f32 =
            ((1f32 - f32::cos(0.5 * PI * dot - 0.5 * PI)) / (2.0 - SCALE)) + OFFSET;
        let final_air_time: f32 = motion.movement_speed.current * air_time_scale;

        trace!(
            "final air time movement speed: {}, dot: {}, air scale: {}",
            format_value_f32(final_air_time, Some(3), true),
            format_value_f32(dot, Some(3), true),
            format_value_f32(air_time_scale, Some(3), true)
        );

        motion.linear_velocity_interp.target.x +=
            motion.movement_vector.current.x * final_air_time * time.delta_secs();
        motion.linear_velocity_interp.target.z +=
            motion.movement_vector.current.z * final_air_time * time.delta_secs();
    }

    motion.linear_velocity_interp.current = exp_decay::<Vec3>(
        motion.linear_velocity_interp.current,
        motion.linear_velocity_interp.target,
        motion.linear_velocity_interp.decay,
        time.delta_secs(),
    );

    // We set the actual linear velocity to the current value of the interpolated linear velocity.
    linear_velocity.x = motion.linear_velocity_interp.current.x;
    linear_velocity.z = motion.linear_velocity_interp.current.z;

    trace!(
        "Interpolated Linear Velocity: {{ current {} -> target {} }}",
        format_value_vec3(motion.linear_velocity_interp.current, Some(3), true),
        format_value_vec3(motion.linear_velocity_interp.target, Some(3), true)
    );

    // todo: move this to a new system that handles state changes.
    // * Detected and apply MOVING flag.
    // set the motion.moving when the magnituted of the movement_vector is greater than some arbitrary small threshold.
    motion.moving = motion.movement_vector.current.length() >= 0.01;
}

pub fn player_rotation_system(
    mut player_query: Query<&mut Transform, With<Player>>,
    keys: Res<ButtonInput<KeyCode>>,
    input: Res<Input>,
) {
    for mut player_transform in player_query.iter_mut() {
        // Get the current rotation components.
        let (mut player_yaw, player_pitch, player_roll) =
            player_transform.rotation.to_euler(EulerRot::default());

        // Ensure the player is not holding down the free look key.
        if !keys.pressed(KeyCode::AltLeft) {
            player_yaw -= (input.focus_delta_raw.x).to_radians();
        }

        // Apply the current rotation.
        player_transform.rotation =
            Quat::from_euler(EulerRot::default(), player_yaw, player_pitch, player_roll);
    }
}

pub fn apply_spring_force(
    config: &Res<PlayerControlConfig>,
    linear_velocity: &LinearVelocity,
    constant_force: &mut ConstantForce,
    ray_length: f32,
    ride_height: f32,
) {
    // Find the diference between how close the capsule is to the surface beneath it.
    // Compute this value by subtracting the ray length from the set ride height
    // to find the diference in position.
    let spring_offset: f32 = f32::abs(ray_length) - ride_height;
    let spring_force: f32 = (spring_offset * config.ride_spring_strength)
        - (-linear_velocity.0.y * config.ride_spring_damper);

    /* Now we apply our spring force vector in the direction to return the bodies distance from the ground towards RIDE_HEIGHT. */
    constant_force.0.y = -spring_force;

    trace!(
        "Applying Spring Force: {} (ray_length: {}, ride_height: {})",
        format_value_f32(spring_force, Some(3), true),
        format_value_f32(ray_length, Some(3), true),
        format_value_f32(ride_height, Some(3), true)
    );
}

/// The entities touched during the last `move_and_slide` call. Stored for debug printing.
#[derive(Component, Default, Deref, DerefMut)]
pub struct TouchedEntities(EntityHashSet);

/// System to run the move and slide algorithm, updating the player's transform and velocity.
///
/// This replaces Avian's default "position integration" that moves kinematic bodies based on their
/// velocity without any collision handling.
pub fn run_move_and_slide(
    mut query: Query<
        (
            Entity,
            &mut Transform,
            &mut LinearVelocity,
            &mut TouchedEntities,
            &Collider,
        ),
        With<Player>,
    >,
    move_and_slide: MoveAndSlide,
    time: Res<Time>,
    mut gizmos: Gizmos,
) {
    for (entity, mut transform, mut lin_vel, mut touched, collider) in &mut query {
        touched.clear();
        // Perform move and slide
        let MoveAndSlideOutput {
            position,
            projected_velocity,
        } = move_and_slide.move_and_slide(
            collider,
            transform.translation.adjust_precision(),
            transform.rotation.adjust_precision(),
            lin_vel.0,
            time.delta(),
            &MoveAndSlideConfig::default(),
            &SpatialQueryFilter::from_excluded_entities([entity]),
            |hit| {
                // For each collision, draw debug gizmos
                if hit.intersects() {
                    gizmos.circle(transform.translation, 33.0, tailwind::RED_600);
                } else {
                    gizmos.arrow(
                        hit.point.f32(),
                        (hit.point
                            + hit.normal.adjust_precision() * hit.collision_distance
                                / time.delta_secs().adjust_precision())
                        .f32(),
                        tailwind::EMERALD_400,
                    );
                }
                touched.insert(hit.entity);
                MoveAndSlideHitResponse::Accept
            },
        );

        // Update transform and velocity
        transform.translation = position.f32();
        lin_vel.0 = projected_velocity;
    }
}
