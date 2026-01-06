use bevy_camera::Camera3d;
use bevy_ecs::{component::Component, query::{With, Without}, system::{Commands, Query, Res}};
use bevy_input::{ButtonInput, keyboard::KeyCode};
use bevy_math::Vec3;
use bevy_time::Time;
use bevy_transform::components::Transform;
use kosim_input::binding::Bindings;

#[derive(Component)]
pub struct FreeCamera;

pub fn create_free_camera(mut commands: Commands) {
    commands.spawn((
        Transform::from_xyz(0.0, 5.0, 0.0).looking_to(Vec3::ZERO, Vec3::Y),
        FreeCamera,
    ));
}

pub fn move_free_camera(
    camera_query: Query<&mut Transform, (With<Camera3d>, Without<FreeCamera>)>,
    mut free_entity_query: Query<&mut Transform, With<FreeCamera>>,
    keys: Res<ButtonInput<KeyCode>>,
    key_bindings: Res<Bindings>,
    time: Res<Time>,
) {
    if camera_query.is_empty()
        || camera_query.iter().len() > 1
        || free_entity_query.is_empty()
        || free_entity_query.iter().len() > 1
    {
        warn!(
            "Free Camera Motion System did not recieve expected 1 camera(s) recieved {}, and 1 player(s) recieved {}. Expect Instablity!",
            camera_query.iter().len(),
            free_entity_query.iter().len()
        );
        return;
    }

    let camera_transform: &Transform = camera_query.iter().next().unwrap();

    for mut transform in free_entity_query.iter_mut() {
        let mut movement_vector: Vec3 = Vec3::ZERO.clone();
        let speed_vector: Vec3 = Vec3::from([20.0, 20.0, 20.0]);
        // WASD Movement
        if keys.pressed(key_bindings.move_forward) {
            movement_vector += camera_transform.forward().as_vec3();
        }
        if keys.pressed(key_bindings.move_backward) {
            movement_vector += camera_transform.back().as_vec3();
        }
        if keys.pressed(key_bindings.move_left) {
            movement_vector += camera_transform.left().as_vec3();
        }
        if keys.pressed(key_bindings.move_right) {
            movement_vector += camera_transform.right().as_vec3();
        }
        // Ascend and Descend
        if keys.pressed(key_bindings.move_ascend) {
            movement_vector += Vec3::Y;
        }
        if keys.pressed(key_bindings.move_descend) {
            movement_vector -= Vec3::Y;
        }

        // Scale the vector by the elapsed time.
        movement_vector *= speed_vector * time.delta_secs();
        transform.translation += movement_vector;
    }
}