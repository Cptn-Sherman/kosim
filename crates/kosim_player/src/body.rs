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
    motion::apply_spring_force, stance::{Stance, StanceType},
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

pub fn compute_ray_length(
    entity: Entity,
    entities_to_ignore: Query<Entity, With<IgnoreRayCollision>>,
    ray_hits: &RayHits,
) -> f32 {
    // Compute the ray_length to a hit, if we don't hit anything we assume the ground is infinitly far away.
    let mut ray_length: f32 = f32::INFINITY;

    // Find the first ray hit which is not its own collider.
    for hit in ray_hits.iter_sorted() {
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
            ray_length = hit.distance;
            break;
        }
    }
    ray_length
}

pub fn apply_standing_spring_force(
    mut query: Query<(
        Entity,
        &LinearVelocity,
        &mut ConstantForce,
        &mut StandingSpringForce,
        &mut GravityScale,
        &RayHits,
    )>,
    config: Res<PlayerControlConfig>,
    ignored_entities: Query<Entity, With<IgnoreRayCollision>>,
    time: Res<Time>,
) {
    for (
        entity,
        linear_velocity,
        mut constant_force,
        mut standing_spring_force,
        mut gravity_scale,
        ray_hits,
    ) in &mut query
    {
        // Compute the ray_length to a hit, if we don't hit anything we assume the ground is infinitly far away.
        let ray_length: f32 = compute_ray_length(entity, ignored_entities, ray_hits);

        // Lerp current_ride_height to target_ride_height, this target_ride_height changes depending on the stance. Standing, Crouching, and Prone.
        standing_spring_force.length.current = exp_decay::<f32>(
            standing_spring_force.length.current,
            standing_spring_force.length.target,
            standing_spring_force.length.decay,
            time.delta_secs(),
        );

        // Todo: We should limit detecting landing unless the ray_length is now LESS than the current length (NOT INCLUDING MAX EXTENSION).

        let ride_height: f32 = standing_spring_force.length.current;
        let max_ray_length: f32 =
            standing_spring_force.length.current + standing_spring_force.extension;
        if ray_length <= max_ray_length {
            gravity_scale.0 = 0.0f32;
            apply_spring_force(
                &config,
                linear_velocity,
                &mut constant_force,
                ray_length,
                ride_height,
            );
        } else {
            constant_force.0 = Vec3::ZERO;
            gravity_scale.0 = 1.0f32;
        }
        trace!(
            "Constant Force: {}",
            format_value_vec3(constant_force.0, Some(3), true)
        );
        trace!("Gravity Scale: {}", gravity_scale.0);
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
