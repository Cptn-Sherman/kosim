//! Detached free-fly camera for inspecting the world.
//!
//! Pressing [`Bindings::action_toggle_camera_mode`] (F3) toggles free-cam mode. On
//! entering, the [`GameCamera`] is un-parented from the player and keeps its current
//! world pose; [`free_cam_control`] then flies it directly with the usual
//! move/look input (WASD + mouse, Space/Shift to rise/fall, Ctrl to boost). On
//! exiting, the camera is re-parented to the player and the normal first-person
//! systems resume.
//!
//! While free-cam is active the player-control systems ([`camera_look_system`],
//! rotation, motion, move-and-slide) are gated off via [`player_control_active`] so
//! the shared input does not drive the player at the same time — the player simply
//! parks in place.

use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::With;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::log::info;
use bevy::math::{EulerRot, Quat, Vec3};
use bevy::prelude::{Commands, Entity, GlobalTransform, Query, Res, ResMut, Resource};
use bevy::time::Time;
use bevy::transform::components::Transform;
use kosim_camera::GameCamera;
use kosim_input::binding::Bindings;
use kosim_input::input::Input;

use crate::Player;

/// Base fly speed in units/second, and the multiplier applied while boosting.
const FREE_CAM_SPEED: f32 = 25.0;
const FREE_CAM_BOOST: f32 = 4.0;

/// State for the detached free-fly camera. `yaw`/`pitch` are tracked here rather
/// than read back from the transform so look stays stable and gimbal-free.
#[derive(Resource, Default)]
pub struct FreeCam {
    pub active: bool,
    pub yaw: f32,
    pub pitch: f32,
}

/// Run condition: player-control systems run only while the free cam is *off*.
pub fn player_control_active(free: Res<FreeCam>) -> bool {
    !free.active
}

/// Run condition: the free-cam controller runs only while it is *on*.
pub fn free_cam_active(free: Res<FreeCam>) -> bool {
    free.active
}

/// Toggle free-cam mode on the toggle key: detach/reattach the camera and seed the
/// look angles so there is no jump.
pub fn toggle_free_cam(
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    mut free: ResMut<FreeCam>,
    mut commands: Commands,
    player_query: Query<Entity, With<Player>>,
    mut camera_query: Query<(Entity, &mut Transform, &GlobalTransform), With<GameCamera>>,
) {
    if !keys.just_pressed(bindings.action_toggle_camera_mode) {
        return;
    }
    let Ok((camera, mut transform, global)) = camera_query.single_mut() else {
        return;
    };

    if !free.active {
        // Enter: detach and keep the exact world pose the camera had as a child.
        let world = global.compute_transform();
        *transform = world;
        let (yaw, pitch, _) = world.rotation.to_euler(EulerRot::YXZ);
        free.yaw = yaw;
        free.pitch = pitch;
        commands.entity(camera).remove::<ChildOf>();
        free.active = true;
        info!("Free cam ON (F3 to exit)");
    } else {
        // Exit: re-parent to the player and reset to the first-person offset.
        if let Ok(player) = player_query.single() {
            commands.entity(player).add_children(&[camera]);
            transform.translation = Vec3::new(0.0, 1.0, 0.0);
            transform.rotation = Quat::IDENTITY;
        }
        free.active = false;
        info!("Free cam OFF");
    }
}

/// Fly the detached camera: mouse look (free yaw + clamped pitch) and WASD/Space/
/// Shift movement in the camera's own basis, with Ctrl to boost. Runs in
/// `FixedUpdate` so it reads the input resource before it is cleared.
pub fn free_cam_control(
    mut free: ResMut<FreeCam>,
    input: Res<Input>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
    time: Res<Time>,
    mut camera_query: Query<&mut Transform, With<GameCamera>>,
) {
    let Ok(mut transform) = camera_query.single_mut() else {
        return;
    };

    // Look: accumulate raw mouse delta into stored yaw/pitch.
    free.yaw -= input.focus_delta_raw.x.to_radians();
    free.pitch -= input.focus_delta_raw.y.to_radians();
    free.pitch = free.pitch.clamp(-1.54, 1.54);
    let rotation = Quat::from_euler(EulerRot::YXZ, free.yaw, free.pitch, 0.0);
    transform.rotation = rotation;

    // Move in the camera's basis. `movement_raw.z` is forward (+W), `.x` strafe (+D).
    let mut direction = rotation * Vec3::NEG_Z * input.movement_raw.z
        + rotation * Vec3::X * input.movement_raw.x;
    if keys.pressed(bindings.move_ascend) {
        direction += Vec3::Y;
    }
    if keys.pressed(bindings.move_descend) {
        direction -= Vec3::Y;
    }
    if direction != Vec3::ZERO {
        let boost = if keys.pressed(KeyCode::ControlLeft) {
            FREE_CAM_BOOST
        } else {
            1.0
        };
        transform.translation += direction.normalize() * FREE_CAM_SPEED * boost * time.delta_secs();
    }
}
