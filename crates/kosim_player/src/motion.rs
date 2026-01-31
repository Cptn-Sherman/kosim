use avian3d::prelude::{ConstantForce, Forces, LinearVelocity, RayHits, RigidBodyForces, forces::ForcesItem};
use bevy_ecs::{component::Component, entity::Entity, query::With, system::{Query, Res}};
use bevy_input::{ButtonInput, keyboard::KeyCode};
use bevy_math::{EulerRot, Quat, Vec3};
use bevy_time::Time;
use bevy_transform::components::Transform;
use kosim_input::input::Input;
use kosim_utility::{exp_decay, interpolated_value::InterpolatedValue};

use crate::{Player, body::{Body, IgnoreRayCollision, Stance, StanceType, StandingSpringForce, compute_ray_length}, config::PlayerControlConfig};

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
        // warn!(
        //     "Player Motion System expected 1 player(s), recieved {}. Expect Instablity!",
        //     player_query.iter().len()
        // );
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

    // info!(
    //     "Movement Speed current: {}, target: {}",
    //     format_value_f32(motion.current_movement_speed, Some(4), true), format_value_f32(motion.target_movement_speed, Some(4), true)
    // );

    // * UPDATE MOVEMENT_VECTOR AND LERP

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

    // info!(
    //     "Current Movement Vector: {}",
    //     format_value_vec3(motion.movement_vector.current, Some(4), true),
    // );

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

        // info!(
        //     "final movement speed: {}, dot: {}, air scale: {}",
        //     format_value_f32(final_air_time, Some(3), true),
        //     format_value_f32(dot, Some(3), true),
        //     format_value_f32(air_time_scale, Some(3), true)
        // );

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

    linear_velocity.x = motion.linear_velocity_interp.current.x;
    linear_velocity.z = motion.linear_velocity_interp.current.z;

    // info!(
    //     "Interpolated Linear Velocity: current {}",
    //     format_value_vec3(motion.linear_velocity_interp.current, Some(3), true)
    // );
    // info!(
    //     "Interpolated Linear Velocity: target {}",
    //     format_value_vec3(motion.linear_velocity_interp.target, Some(3), true)
    // );
    // info!(
    //     "Linear Velocity: {}",
    //     format_value_vec3(linear_velocity.0, Some(3), true),
    // );

    // todo: move this to a new system that handles state changes.
    // * Detected and apply MOVING flag.
    // set the motion.moving when the magnituted of the movement_vector is greater than some arbitrary small threshold.
    motion.moving = motion.movement_vector.current.length() >= 0.01;
}

pub fn player_jump_system(
    mut player_query: Query<
        (
            Entity,
            Forces,
            &mut StandingSpringForce,
            &mut ConstantForce,
            &Motion,
            &mut Stance,
            &Body,
            &RayHits,
        ),
        With<Player>,
    >,
    ignored_entities: Query<Entity, With<IgnoreRayCollision>>,
    player_config: Res<PlayerControlConfig>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let (entity, mut forces, mut standing_spring, mut constant_force, motion, mut stance, body, ray_hits) =
        player_query.single_mut().expect("We do some errors");
    // * -   - JUMPING LOGIC   -

    if stance.current == StanceType::Standing
        && keys.pressed(KeyCode::Space)
        && stance.lockout <= 0.0
    {
        let ray_length: f32 = compute_ray_length(entity, ignored_entities, ray_hits);
        stance.lockout = player_config.stance_lockout;
        stance.current = StanceType::Airborne;
        constant_force.y = 0.0;

        apply_jump_force(
            &mut forces,
            &player_config,
            ray_length,
            &mut stance,
            &mut standing_spring,
            &motion,
            &body,
        );
    }
}

// This function and many of its helpers are ripped from, bevy_fly_cam.
pub fn player_rotation_system(
    mut player_query: Query<&mut Transform, With<Player>>,
    keys: Res<ButtonInput<KeyCode>>,
    input: Res<Input>,
) {
    for mut player_transform in player_query.iter_mut() {
        let (mut player_yaw, player_pitch, player_roll) =
            player_transform.rotation.to_euler(EulerRot::default());

        if !keys.pressed(KeyCode::AltLeft) {
            player_yaw -= (input.focus_delta.x).to_radians();
        }
        player_transform.rotation =
            Quat::from_euler(EulerRot::default(), player_yaw, player_pitch, player_roll);
    }
}

pub fn apply_jump_force(
    forces: &mut ForcesItem<'_, '_>,
    player_config: &Res<PlayerControlConfig>,
    ray_length: f32,
    stance: &mut Stance,
    standing_spring: &mut StandingSpringForce,
    motion: &Motion,
    body: &Body,
) {
    // Apply the stance cooldown now that we are jumping.
    stance.lockout = player_config.stance_lockout;

    let half_jump_strength: f32 = player_config.jump_strength / 2.0;
    let clamped_jump_force: f32 =
        compute_clamped_jump_force_factor(&body, &standing_spring, ray_length);

    // todo: make this value changable.
    let dynamic_jump_strength: f32 = half_jump_strength + (half_jump_strength * clamped_jump_force);

    // todo: right now we are applying this jump force directly up, this needs to consider the original movement velocities.
    // maybe instead of half the strength getting added to the up we added it directionally only so you always jump x height but can
    // use more of the timing to aid in forward momentum.

    // find the movement vector in the x and z direction.
    let jump_direction: Vec3 = motion
        .movement_vector
        .current
        .mul_add(Vec3::ONE, Vec3::from_array([0.0, 1.0, 0.0]))
        .normalize_or_zero();

    let impulse_vec: Vec3 = jump_direction * dynamic_jump_strength;

    // info!(
    //     "impulse_vec: {}",
    //     format_value_vec3(impulse_vec, Some(3), true)
    // );

    // apply the jump force.
    forces.apply_linear_impulse(impulse_vec.into());

    // info!(
    //     "Jumped with {}/{} due to distance to ground, /njump_factor {}, of ray length: {}",
    //     dynamic_jump_strength, player_config.jump_strength, clamped_jump_force, ray_length
    // );
}

fn compute_clamped_jump_force_factor(
    body: &Body,
    standing_spring: &StandingSpringForce,
    ray_length: f32,
) -> f32 {
    // Constants defined elsewhere in the code
    let full_standing_ray_length: f32 = standing_spring.length.current;
    let half_standing_ray_length: f32 =
        standing_spring.length.current - (body.current_body_height / 4.0);
    // This value represents the range of acceptable ray lengths for the player.
    let standing_ray_length_range: f32 = full_standing_ray_length - half_standing_ray_length;

    // Ensure the input is within the specified range
    let clamped_ray_length = f32::clamp(
        ray_length,
        half_standing_ray_length,
        standing_spring.length.current,
    );

    // Apply the linear transformation
    // Step 1: Normalize clamped_ray_length to a value between 0.0 and 1.0.
    let normalized_distance =
        (clamped_ray_length - half_standing_ray_length) / standing_ray_length_range;

    // Step 2: Subtract the normalized distance from CAPSULE_HEIGHT.
    let result: f32 = body.current_body_height - normalized_distance;

    // Ensure the output is within the range [0.0, 1.0].
    f32::clamp(result, 0.0, 1.0)
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

    // info!(
    //     "Applying Spring Force: {} (ray_length: {}, ride_height: {})",
    //     format_value_f32(spring_force, Some(3), true),
    //     format_value_f32(ray_length, Some(3), true),
    //     format_value_f32(ride_height, Some(3), true)
    // );
}