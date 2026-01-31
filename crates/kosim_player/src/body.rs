use bevy_ecs::{bundle::Bundle, component::Component, entity::Entity, message::MessageWriter, query::With, system::{Query, Res}};
use avian3d::prelude::*;
use bevy_math::Vec3;
use bevy_time::Time;
use kosim_utility::{exp_decay, interpolated_value::InterpolatedValue};

use crate::{Player, actions::step::{DEFAULT_STEP_VOLUME, FootstepDirection, FootstepEvent}, config::PlayerControlConfig, motion::apply_spring_force};

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

#[derive(Debug, PartialEq, Clone)]
// each of these stance types needs to have a movement speed calculation, a
pub enum StanceType {
    Airborne,
    Standing,
    Landing,
}

#[derive(Component)]
pub struct Stance {
    pub current: StanceType,
    pub _grounded: bool,
    pub crouched: bool,
    pub lockout: f32,
}

pub trait Set {
    fn set<T>(&self, next_val: T);
}

pub trait SetWithLockout {
    fn try_set(&mut self, next_val: StanceType, lockout: Option<f32>);
}

impl SetWithLockout for Stance {
    fn try_set(&mut self, next_val: StanceType, lockout: Option<f32>) {
        if self.lockout <= 0.0 {
            self.current = next_val;
            self.lockout = lockout.unwrap_or(0.0f32);
        }
    }
}

pub fn update_player_stance(
    mut query: Query<(Entity, &StandingSpringForce, &mut Stance, &RayHits), With<Player>>,
    ignored_entities: Query<Entity, With<IgnoreRayCollision>>,
    mut ev_footstep: MessageWriter<FootstepEvent>,
    config: Res<PlayerControlConfig>,
    time: Res<Time>,
) {
    if query.is_empty() || query.iter().len() > 1 {
        // warn!(
        //     "Update Player Stance System found {} players, expected 1.",
        //     query.iter().len()
        // );
    }

    for (entity, standing_spring, mut stance, ray_hits) in &mut query {
        // Compute the next stance for the player.
        let previous_stance: StanceType = stance.current.clone();
        let mut next_stance: StanceType = stance.current.clone();

        let ray_length: f32 = compute_ray_length(entity, ignored_entities, ray_hits);

        // If your locked in you cannot change state.
        if stance.lockout <= 0.0 {
            // info!(
            //     "Standing Spring Length: {}",
            //     format_value_f32(standing_spring.length.current, Some(2), false)
            // );
            // info!(
            //     "Ray Length: {}",
            //     format_value_f32(ray_length, Some(2), false)
            // );

            if ray_length > standing_spring.length.current + config.ray_length_offset {
                next_stance = StanceType::Airborne;
            } else if ray_length < standing_spring.length.current {
                next_stance = StanceType::Standing;
            } else if previous_stance != StanceType::Standing
                && ray_length < standing_spring.length.current + standing_spring.extension
            {
                next_stance = StanceType::Landing;
            }
        } else if stance.lockout != 0.0 {
            stance.lockout -= time.delta_secs();
            stance.lockout = f32::max(stance.lockout, 0.0);
            if stance.lockout <= 0.0 {
                //info!("Stance lockout: RELEASED");
            } else {
                // info!(
                //     "Stance lockout: {}",
                //     format_value_f32(stance.lockout, Some(2), false)
                // );
            }
        }

        if next_stance != previous_stance {
            // info!(
            //     "Stance Changed: {:#?} -> {:#?}",
            //     previous_stance, next_stance
            // );
        }

        // handle footstep sound event when the state has changed and only then.
        if next_stance != stance.current {
            match next_stance {
                StanceType::Landing => {
                    // This is the sound effect that plays when the player has jumped or fallen and will land with both feet on the ground.
                    // this effect will play centered and will not pan in any direction.
                    ev_footstep.write(FootstepEvent {
                        dir: FootstepDirection::None,
                        volume: DEFAULT_STEP_VOLUME,
                    });
                }
                _ => (),
            }
        }

        // Update the current stance.
        stance.current = next_stance.clone();
    }
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
            gravity_scale.0 = 1.0f32;
        }
        // info!("External Force: {}", format_value_vec3(external_force.xyz(), Some(3), true));
        // info!("Gravity Scale: {}", gravity_scale.0);
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
