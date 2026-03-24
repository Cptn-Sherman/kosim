use crate::Player;
use bevy::{
    camera::Camera3d,
    ecs::{
        component::Component,
        query::{With, Without},
        system::{Query, Res},
    },
    input::{ButtonInput, keyboard::KeyCode},
    math::{EulerRot, Quat},
    time::Time,
    transform::components::Transform,
};
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
        let (mut yaw, mut pitch, roll) =
            cam_transform.rotation.to_euler(EulerRot::YXZ);

        // Check for free look movement. Allowing the user to turn their head while maintaining a movement direction.
        if keys.pressed(key_bindings.action_enable_freelook.key) {
            // todo: add gamepad check.
            yaw -= input.focus_delta_smoothed.current.x.to_radians();
            yaw = yaw.clamp(-MAX_FREE_LOOK_ANGLE, MAX_FREE_LOOK_ANGLE);
        } else {
            yaw = exp_decay(yaw, 0.0, 8.0, time.delta_secs());
        }

        pitch -= input.focus_delta_smoothed.current.y.to_radians();
        // Prevent the Camera from wrapping over itself when looking up or down.
        pitch = pitch.clamp(-1.54, 1.54);
        // Order is important to prevent unintended roll.
        cam_transform.rotation =
            Quat::from_euler(EulerRot::default(), yaw, pitch, roll);
    }
}
