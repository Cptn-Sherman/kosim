pub mod binding;
pub mod cursor;
pub mod input;

use bevy_ecs::resource::Resource;

#[derive(Resource)]
pub struct InputConfig {
    pub sensitivity: f32,
    pub gamepad_look_sensitivity: f32,
    pub mouse_look_sensitivity: f32,
}