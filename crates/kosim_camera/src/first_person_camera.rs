use bevy_camera::Camera3d;
use bevy_ecs::{component::Component, query::{With, Without}, system::{Query, Res}};
use bevy_math::{EulerRot, Quat, Vec3};
use bevy_time::Time;
use bevy_transform::components::Transform;
use kosim_input::input::Input;
use kosim_utility::{exp_decay, interpolated_value::InterpolatedValue};

#[derive(Component)]
pub struct SmoothedCamera {
    pub lean: InterpolatedValue<Vec3>,
    pub lock_lean: f32,
}
pub const ROTATION_AMOUNT: f32 = 4.0;
pub const LEAN_LOCKOUT_TIME: f32 = 0.15;

pub fn smooth_camera(
    mut camera_query: Query<
        (&mut Transform, &mut SmoothedCamera),
        (With<Camera3d>, Without<Player>),
    >,
    input: Res<Input>,
    time: Res<Time>,
) {
    let (mut camera_transform, mut smoothed_camera) = camera_query.single_mut().unwrap();

    // Update the Curent Lean
    let (yaw, pitch, _) = camera_transform.rotation.to_euler(EulerRot::default());
    //let pitch = input_vector.y * rotation_amount.to_radians();
    let roll: f32 = -1.0 * input.movement_raw.x * ROTATION_AMOUNT.to_radians();

    // Set the new target lean and lerp the current value at a constant rate
    // ! for now we will use the constant value 2.0 for lerping. We can probably replace this by just seeing how fast the camera is moving? check the velocity
    let lean_decay: f32 = 2.0; // ternary!(motion.sprinting, 2.0, 8.0);
    if smoothed_camera.lock_lean > 0.0 {
        smoothed_camera.lock_lean -= time.delta_secs();
    } else {
        smoothed_camera.lean.target = Vec3::from_array([yaw, pitch, roll]);
    }

    // Interpolate the smoothed camera lean.
    smoothed_camera.lean.current = exp_decay::<Vec3>(
        smoothed_camera.lean.current,
        smoothed_camera.lean.target,
        lean_decay,
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