use bevy_camera::Camera3d;
use bevy_ecs::{
    component::Component,
    query::{With, Without},
    system::{Query, Res},
};
use bevy_math::{EulerRot, Quat};
use bevy_time::Time;
use bevy_transform::components::Transform;
use kosim_input::{binding::Bindings, input::Input};
use kosim_utility::exp_decay;
use bevy_input::keyboard::KeyCode;
use bevy_input::ButtonInput;
use crate::Player;

#[derive(Component)]
pub struct Focus;

#[derive(Component)]
pub struct FocusTarget;

// This function and many of its helpers are ripped from, bevy_fly_cam.
pub fn camera_look_system(
    mut camera_query: Query<&mut Transform, (With<Camera3d>, Without<Player>)>,
    keys: Res<ButtonInput<KeyCode>>,
    key_bindings: Res<Bindings>,
    input: Res<Input>,
    time: Res<Time>,
) {
    for mut cam_transform in camera_query.iter_mut() {
        let (mut camera_yaw, mut camera_pitch, camera_roll) =
            cam_transform.rotation.to_euler(EulerRot::YXZ);

        // Check for free camera movement. Allowing the user to turn their head while maintaining a movement direction.
        if keys.pressed(key_bindings.action_enable_freelook.key) { // todo: add gamepad check.
            camera_yaw -= input.focus_delta.x.to_radians();
            let max_free_look_angle: f32 = 110.0f32.to_radians();
            camera_yaw = camera_yaw.clamp(-max_free_look_angle, max_free_look_angle);
            // info!("Camera Yaw: {}", camera_yaw);
        } else {
            camera_yaw = exp_decay(camera_yaw, 0.0, 8.0, time.delta_secs());
        }

        camera_pitch -= input.focus_delta.y.to_radians();
        // Prevent the Camera from wrapping over itself when looking up or down.
        camera_pitch = camera_pitch.clamp(-1.54, 1.54);
        // Order is important to prevent unintended roll.
        cam_transform.rotation =
            Quat::from_euler(EulerRot::default(), camera_yaw, camera_pitch, camera_roll);
    }
}
