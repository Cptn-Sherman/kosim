use avian3d::{math::{AdjustPrecision, AsF32}, prelude::{
    Collider, ConstantForce, LinearVelocity, MoveAndSlide, MoveAndSlideConfig, MoveAndSlideHitResponse, MoveAndSlideOutput, SpatialQueryFilter
}};
use bevy::{
    color::palettes::tailwind, ecs::{
        component::Component, entity::{Entity, EntityHashSet}, query::With, system::{Query, Res}
    }, gizmos::gizmos::Gizmos, input::{ButtonInput, keyboard::KeyCode}, log::{trace, warn}, math::{Quat, Vec3}, prelude::{Deref, DerefMut}, time::Time, transform::components::Transform
};
use kosim_input::input::Input;
use kosim_utility::{
    exp_decay,
    format_value::{format_value_f32, format_value_vec3},
    interpolated_value::InterpolatedValue,
};

use crate::{
    Player,
    config::PlayerControlConfig,
    gravity::{PlanetGravity, up_at},
    stance::{Stance, StanceType},
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
    gravity: Res<PlanetGravity>,
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

    // * APPLY MOVEMENT_VECTOR TO PLAYER LINEAR VELOCITY (tangent to the planet)

    // `movement_vector` is built from the player's forward/right, which lie on the
    // tangent plane now that the capsule is radially aligned, so it is already a
    // surface (tangential) velocity direction. We keep the radial velocity (owned by
    // the ride spring / gravity) and only drive the tangential part.
    let up = up_at(player_transform.translation, gravity.center);
    let radial_speed = linear_velocity.0.dot(up);

    if stance.current == StanceType::Standing {
        let target = motion.movement_vector.current * motion.movement_speed.current;
        motion.linear_velocity_interp.target = target;
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

        let delta = motion.movement_vector.current * final_air_time * time.delta_secs();
        motion.linear_velocity_interp.target += delta;
    }

    motion.linear_velocity_interp.current = exp_decay::<Vec3>(
        motion.linear_velocity_interp.current,
        motion.linear_velocity_interp.target,
        motion.linear_velocity_interp.decay,
        time.delta_secs(),
    );

    // Strip any radial drift from the interpolated (tangential) velocity, then
    // recombine with the preserved radial speed.
    let tangential = motion.linear_velocity_interp.current
        - up * motion.linear_velocity_interp.current.dot(up);
    linear_velocity.0 = up * radial_speed + tangential;

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
    gravity: Res<PlanetGravity>,
) {
    for mut player_transform in player_query.iter_mut() {
        // The capsule's up is radial; its facing is a heading on the tangent plane.
        let up = up_at(player_transform.translation, gravity.center);

        // Re-project the current facing onto the (possibly re-oriented) tangent plane.
        let mut forward = player_transform.forward().as_vec3();
        forward -= up * forward.dot(up);
        if forward.length_squared() < 1.0e-6 {
            // Facing was along `up`; fall back to the current right vector.
            forward = player_transform.right().as_vec3();
            forward -= up * forward.dot(up);
        }
        forward = forward.normalize_or(Vec3::Z);

        // Yaw about `up` from mouse look (unless free-look is held).
        if !keys.pressed(KeyCode::AltLeft) {
            let yaw = -input.focus_delta_raw.x.to_radians();
            forward = Quat::from_axis_angle(up, yaw) * forward;
        }

        // Build a rotation whose local up is `up` and whose -Z is `forward`.
        player_transform.rotation = Transform::IDENTITY.looking_to(forward, up).rotation;
    }
}

pub fn apply_spring_force(
    config: &Res<PlayerControlConfig>,
    linear_velocity: &LinearVelocity,
    constant_force: &mut ConstantForce,
    ray_length: f32,
    ride_height: f32,
    up: Vec3,
) {
    // Difference between how far the capsule floats above the surface and its target
    // ride height, along the radial "up".
    let spring_offset: f32 = ray_length - ride_height;
    // Velocity component along `up` (away from the planet), damped so the body settles.
    let radial_velocity: f32 = linear_velocity.0.dot(up);
    let spring_force: f32 =
        spring_offset * config.ride_spring_strength + radial_velocity * config.ride_spring_damper;

    // Push the body back toward its ride height along `up`.
    constant_force.0 = -up * spring_force;

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
            &Stance,
            &Motion,
        ),
        With<Player>,
    >,
    move_and_slide: MoveAndSlide,
    gravity: Res<PlanetGravity>,
    time: Res<Time>,
    mut gizmos: Gizmos,
) {
    for (entity, mut transform, mut lin_vel, mut touched, collider, stance, motion) in &mut query {
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

        // Update transform.
        transform.translation = position.f32();

        // * GROUND FRICTION -
        //
        // `move_and_slide` projects the incoming velocity ALONG the contact
        // surface. On a tilted voxel face that projection redirects the vertical
        // ride-spring / landing velocity into a horizontal downhill component,
        // so writing `projected_velocity` back verbatim makes the character
        // creep (and, before the spring was damped, orbit) down every slope even
        // when the player isn't moving. Nothing else removes that component.
        //
        // Emulate static ground friction: while grounded (Standing/Landing) and
        // not moving under the player's own input, drop the horizontal part of
        // the projected velocity so the character comes to rest on slopes. The
        // vertical component is kept so the ride spring still settles the body to
        // its ride height. When the player IS moving we leave the projection
        // intact so they travel along the slope surface as intended.
        let grounded: bool =
            matches!(stance.current, StanceType::Standing | StanceType::Landing);
        let up = up_at(transform.translation, gravity.center);
        let mut resolved_velocity = projected_velocity;
        if grounded && !motion.moving {
            // Keep only the radial component so the ride spring still settles the
            // body; drop the tangential downhill creep the surface projection adds.
            resolved_velocity = up * resolved_velocity.dot(up);
        }
        lin_vel.0 = resolved_velocity;
    }
}
