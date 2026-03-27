use bevy::{
    camera::Camera3d,
    ecs::{
        component::Component,
        query::With,
        system::{Query, Res},
    },
    math::{EulerRot, Quat, Vec3},
    time::Time,
    transform::components::Transform,
};
use kosim_input::input::Input;
use kosim_utility::{exp_decay, interpolated_value::InterpolatedValue};

const ROTATION_AMOUNT: f32 = 1.0;
const LEAN_DECAY: f32 = 4.0;

#[derive(Component)]
pub struct DynamicCameraMovement {
    pub lean: InterpolatedValue<Vec3>,
    pub lock_lean: f32,
}

pub fn camera_lean(
    mut camera_query: Query<(&mut Transform, &mut DynamicCameraMovement), With<Camera3d>>,
    input: Res<Input>,
    time: Res<Time>,
) {
    let (mut camera_transform, mut smoothed_camera) = camera_query.single_mut().unwrap();

    // Get the current yaw and pitch from the camera's transform.
    let (yaw, pitch, _) = camera_transform.rotation.to_euler(EulerRot::default());
    // Calculate the target roll based in the inverse input movement on the x-axis, multiplied by the rotation amount.
    let roll: f32 = -1.0 * input.movement_raw.x * ROTATION_AMOUNT.to_radians();
    // Set the target lean to the current yaw and pitch, and the calculated roll.
    smoothed_camera.lean.target = Vec3::from_array([yaw, pitch, roll]);
    // Interpolate the smoothed camera lean.

    smoothed_camera.lean.current = exp_decay::<Vec3>(
        smoothed_camera.lean.current,
        smoothed_camera.lean.target,
        LEAN_DECAY,
        time.delta_secs(),
    );

    // Apply the lean to the camera transformation.
    camera_transform.rotation = Quat::from_euler(
        EulerRot::default(),
        yaw, // we dont change the yaw.
        pitch,
        smoothed_camera.lean.current.z,
    );
}
