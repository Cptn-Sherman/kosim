use avian3d::prelude::RayHits;
use bevy::{
    ecs::{
        component::Component,
        entity::Entity,
        message::MessageWriter,
        query::With,
        system::{Query, Res},
    },
    log::{info, trace, warn},
    time::Time,
};
use kosim_utility::format_value::format_value_f32;

use crate::{
    Player, action::{DEFAULT_STEP_VOLUME, FootstepDirection, FootstepEvent}, body::{IgnoreRayCollision, StandingSpringForce, compute_ray_length}, config::PlayerControlConfig
};

#[derive(Debug, PartialEq, Clone)]
// each of these stance types needs to have a movement speed calculation, a
pub enum StanceType {
    Airborne,
    Standing,
    Landing,
}

#[derive(Component)]
pub struct Stance {
    pub lockout_timer: f32,
    pub current: StanceType,
    pub crouched: bool,
}

pub trait ForceSet {
    fn force_set(&mut self, next_val: StanceType);
}

impl ForceSet for Stance {
    fn force_set(&mut self, next_val: StanceType) {
        self.current = next_val;
    }
}

pub trait SetWithLockout {
    fn try_set(&mut self, next_val: StanceType, lockout: Option<f32>) -> Result<(), String>;
}

impl SetWithLockout for Stance {
    fn try_set(&mut self, next_val: StanceType, lockout: Option<f32>) -> Result<(), String> {
        if self.lockout_timer <= 0.0 {
            self.current = next_val;
            self.lockout_timer = lockout.unwrap_or(0.0f32);
            Ok(())
        } else {
            Err(format!(
                "Cannot change stance to {:?} because lockout is still active for {} seconds.",
                next_val, self.lockout_timer
            ))
        }
    }
}

pub fn compute_next_stance(
    mut query: Query<(Entity, &StandingSpringForce, &mut Stance, &RayHits), With<Player>>,
    ignored_entities: Query<Entity, With<IgnoreRayCollision>>,
    mut ev_footstep: MessageWriter<FootstepEvent>,
    config: Res<PlayerControlConfig>,
    time: Res<Time>,
) {
    if query.is_empty() || query.iter().len() > 1 {
        warn!(
            "Update Player Stance System found {} players, expected 1.",
            query.iter().len()
        );
    }

    for (entity, standing_spring_height, mut stance, ray_hits) in &mut query {
        // Compute the next stance for the player.
        let previous_stance: StanceType = stance.current.clone();
        let mut next_stance: StanceType = stance.current.clone();

        let ray_length: f32 = compute_ray_length(entity, ignored_entities, ray_hits);

        // If your locked in you cannot change state.
        if stance.lockout_timer <= 0.0 {
            if ray_length > standing_spring_height.length.current + config.ray_length_offset {
                next_stance = StanceType::Airborne;
            } else if ray_length < standing_spring_height.length.current {
                next_stance = StanceType::Standing;
            } else if previous_stance != StanceType::Standing
                && ray_length
                    < standing_spring_height.length.current + standing_spring_height.extension
            {
                next_stance = StanceType::Landing;
            }
        } else if stance.lockout_timer != 0.0 {
            stance.lockout_timer -= time.delta_secs();
            stance.lockout_timer = f32::max(stance.lockout_timer, 0.0);
            if stance.lockout_timer <= 0.0 {
                trace!("Stance lockout: RELEASED");
            } else {
                trace!(
                    "Stance lockout: {}",
                    format_value_f32(stance.lockout_timer, Some(2), false)
                );
            }
        }

        if next_stance != previous_stance {
            info!(
                "Stance Changed: {:#?} -> {:#?}",
                previous_stance, next_stance
            );
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
