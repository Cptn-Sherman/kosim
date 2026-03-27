use crate::{Player, body::IgnoreRayCollision};
use avian3d::prelude::RayHits;
use bevy::{
    camera::Camera3d,
    ecs::{
        component::Component,
        entity::Entity,
        query::{With, Without},
        system::{Query, Res},
    },
    input::{ButtonInput, keyboard::KeyCode},
    log::info,
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

pub fn apply_target_system(
    focus: Query<(Entity, &RayHits), With<GameCamera>>,
    ignored_entities: Query<Entity, With<IgnoreRayCollision>>,
) {
    // Compute the ray_length to a hit, if we don't hit anything we assume the ground is infinitly far away.
    let (entity, ray_hits) = focus.single().unwrap();
    let mut ray_length: f32 = f32::INFINITY;

    for hit in ray_hits.iter_sorted() {
            // First we ensure that the hit is not an ignored entity.
            for child_entity in ignored_entities {
                if hit.entity == child_entity {
                    continue;
                }
            }
            // Next we ensure the hit is not the entity,
            // if true this is the ray_length.
            if hit.entity != entity {
                ray_length = hit.distance;
                break;
            }
    }
    info!("Ray length to focus hit is {}.", ray_length);

    
}
