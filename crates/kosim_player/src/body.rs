use avian3d::prelude::*;

use bevy::{
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        query::With,
        system::{Query, Res},
    },
    log::trace,
    math::Vec3,
    transform::components::Transform,
    time::Time,
};
use kosim_utility::{
    exp_decay,
    format_value::{format_value_vec3},
    interpolated_value::InterpolatedValue,
};

use crate::{
    Player,
    config::PlayerControlConfig,
    gravity::{PlanetGravity, up_at},
    motion::apply_spring_force,
    stance::{Stance, StanceType},
};

#[derive(Component)]
pub struct PlayerColliderFlag;

#[derive(Bundle)]
pub struct PlayerColliderBundle {
    pub(crate) collider: Collider,
}

#[derive(Component)]
pub struct Body {
    pub current_body_height: f32,
}

#[derive(Component)]
pub struct StandingSpringForce {
    pub length: InterpolatedValue<f32>,
    pub extension: f32,
}

#[derive(Component)]
pub struct IgnoreRayCollision;

/// Radius of the sphere used to probe the ground beneath the player.
///
/// This matches the player capsule radius so the probe samples the same
/// footprint the capsule occupies. Probing with a thin ray instead lets the body
/// float based on a single point directly below its origin, which on uneven
/// (e.g. voxel) terrain leaves the capsule intersecting taller neighbouring
/// geometry — `move_and_slide` then depenetrates it sideways and the character
/// zips around. A shape cast reports the nearest ground under the whole
/// footprint, so the spring floats the capsule clear of every surface below it.
pub const GROUND_PROBE_RADIUS: f32 = 0.5;

pub fn compute_ray_length(
    entity: Entity,
    entities_to_ignore: Query<Entity, With<IgnoreRayCollision>>,
    shape_hits: &ShapeHits,
) -> f32 {
    // Compute the ray_length to a hit, if we don't hit anything we assume the ground is infinitly far away.
    let mut ray_length: f32 = f32::INFINITY;

    // Find the first hit which is not its own collider.
    for hit in shape_hits.iter_sorted() {
        // First we ensure that the hit is not an ignored entity.
        let mut can_skip: bool = false;
        for child_entity in entities_to_ignore {
            if hit.entity == child_entity {
                can_skip = true;
            }
        }
        if can_skip {
            continue;
        }
        // Next we ensure the hit is not the entity,
        // if true this is the ray_length.
        if hit.entity != entity {
            // A shape cast reports how far the sphere centre travelled before
            // contact; add the probe radius so this matches the origin->ground
            // distance the spring is tuned around (identical to the old thin-ray
            // value on flat ground).
            ray_length = hit.distance + GROUND_PROBE_RADIUS;
            break;
        }
    }
    ray_length
}

pub fn apply_standing_spring_force(
    mut query: Query<(
        Entity,
        &Transform,
        &LinearVelocity,
        &mut ConstantForce,
        &mut StandingSpringForce,
        &Mass,
        &ShapeHits,
    )>,
    config: Res<PlayerControlConfig>,
    gravity: Res<PlanetGravity>,
    ignored_entities: Query<Entity, With<IgnoreRayCollision>>,
    time: Res<Time>,
) {
    for (
        entity,
        transform,
        linear_velocity,
        mut constant_force,
        mut standing_spring_force,
        mass,
        ray_hits,
    ) in &mut query
    {
        // Distance to the ground along the (radial) probe; infinite if nothing hit.
        let ray_length: f32 = compute_ray_length(entity, ignored_entities, ray_hits);

        // Lerp current_ride_height to target_ride_height, this target_ride_height changes depending on the stance. Standing, Crouching, and Prone.
        standing_spring_force.length.current = exp_decay::<f32>(
            standing_spring_force.length.current,
            standing_spring_force.length.target,
            standing_spring_force.length.decay,
            time.delta_secs(),
        );

        let ride_height: f32 = standing_spring_force.length.current;
        let max_ray_length: f32 =
            standing_spring_force.length.current + standing_spring_force.extension;

        // Everything is relative to the direction away from the planet centre.
        let up = up_at(transform.translation, gravity.center);
        if ray_length <= max_ray_length {
            // Grounded: the ride spring pushes the body to its float height along `up`.
            apply_spring_force(
                &config,
                linear_velocity,
                &mut constant_force,
                ray_length,
                ride_height,
                up,
            );
        } else {
            // Airborne: pull toward the planet centre (F = m * g * -up).
            constant_force.0 = -up * (mass.0 * gravity.strength);
        }
        trace!(
            "Constant Force: {}",
            format_value_vec3(constant_force.0, Some(3), true)
        );
    }
}

pub fn lock_angular_velocity(mut query: Query<(&mut AngularVelocity, &Stance), With<Player>>) {
    for (mut angular_velocity, stance) in &mut query {
        match stance.current {
            StanceType::Standing | StanceType::Landing => {
                angular_velocity.0 = Vec3::ZERO;
            }
            _ => (),
        }
    }
}
