use crate::{Player, body::IgnoreRayCollision};
use avian3d::prelude::RayHits;
use bevy::{
    camera::Camera3d,
    ecs::{
        component::Component,
        entity::Entity,
        query::{With, Without},
        system::{Commands, Query, Res},
    },
    input::{ButtonInput, keyboard::KeyCode},
    math::{EulerRot, Quat},
    time::Time,
    transform::components::Transform,
};
use kosim_camera::GameCamera;
use kosim_input::{binding::Bindings, input::Input};
use kosim_utility::exp_decay;

#[derive(Component)]
pub struct Focus;

#[derive(Component)]
pub struct FocusTarget;

pub const MAX_FREE_LOOK_ANGLE: f32 = 110.0f32.to_radians();

// This function and many of its helpers are ripped from, bevy_fly_cam.
pub fn camera_look_system(
    mut camera_query: Query<&mut Transform, (With<Camera3d>, Without<Player>)>,
    keys: Res<ButtonInput<KeyCode>>,
    key_bindings: Res<Bindings>,
    input: Res<Input>,
    time: Res<Time>,
) {
    for mut cam_transform in camera_query.iter_mut() {
        let (mut yaw, mut pitch, roll) = cam_transform.rotation.to_euler(EulerRot::YXZ);

        // Check for free look movement. Allowing the user to turn their head while maintaining a movement direction.
        if keys.pressed(key_bindings.action_enable_freelook.key) {
            // todo: add gamepad check.
            yaw -= input.focus_delta_smoothed.current.x.to_radians();
            yaw = yaw.clamp(-MAX_FREE_LOOK_ANGLE, MAX_FREE_LOOK_ANGLE);
        } else {
            yaw = exp_decay(yaw, 0.0, 8.0, time.delta_secs());
        }

        pitch -= input.focus_delta_raw.y.to_radians();
        // Prevent the Camera from wrapping over itself when looking up or down.
        pitch = pitch.clamp(-1.54, 1.54);
        // Order is important to prevent unintended roll.
        cam_transform.rotation = Quat::from_euler(EulerRot::default(), yaw, pitch, roll);
    }
}


pub fn update_focus_target(
    focus: Query<(Entity, &RayHits), With<GameCamera>>,
    previous_focus: Query<Entity, (With<FocusTarget>, Without<GameCamera>)>,
    ignored_entities: Query<Entity, With<IgnoreRayCollision>>,
    mut commands: Commands,
) {
    // Compute the ray_length to a hit, if we don't hit anything we assume the ground is infinitly far away.
    let (entity, ray_hits) = focus.single().unwrap();
    let mut ray_length: f32 = f32::INFINITY;
    let mut hit_entity: Option<Entity> = None;

    for hit in ray_hits.iter_sorted() {
            // First we ensure that the hit is not an ignored entity.
            let mut can_skip: bool = false;
            for ignorable_entity in ignored_entities {
                if hit.entity == ignorable_entity {
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
                hit_entity = Some(hit.entity);
                break;
            }
    }

    let previous_focus_entity: Option<Entity> = previous_focus.iter().next();

    if ray_length.is_infinite() {
        // Distance is infinite, remove FocusTarget from previous_focus entity
        if let Some(prev_entity) = previous_focus_entity {
            commands.entity(prev_entity).remove::<FocusTarget>();
        }
    } else {
        // We have a hit, check if it's the same as previous_focus
        if let Some(hit_ent) = hit_entity {
            if let Some(prev_entity) = previous_focus_entity {
                if hit_ent == prev_entity {
                    // Same entity, do nothing.
                } else {
                    // Different entities: apply FocusTarget to new hit entity and remove from previous.
                    commands.entity(hit_ent).insert(FocusTarget);
                    commands.entity(prev_entity).remove::<FocusTarget>();
                }
            } else {
                // No previous focus, apply FocusTarget to new hit entity.
                commands.entity(hit_ent).insert(FocusTarget);
            }
        }
    }
}
