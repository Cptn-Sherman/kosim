use bevy::{
    app::{App, FixedUpdate, Plugin, PreStartup, Startup, Update},
    ecs::resource::Resource,
};

pub mod binding;
pub mod cursor;
pub mod input;

#[derive(Resource)]
pub struct InputConfig {
    pub sensitivity: f32,
    pub gamepad_look_sensitivity: f32,
    pub mouse_look_sensitivity: f32,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            gamepad_look_sensitivity: 1.0,
            mouse_look_sensitivity: 0.1,
        }
    }
}

pub struct KosimInputPlugin;

impl Plugin for KosimInputPlugin {
    fn build(&self, app: &mut App) {
        
        app.insert_resource(InputConfig::default())
        .insert_resource(binding::Bindings::default())
        .add_systems(PreStartup, cursor::initial_grab_cursor)
        .add_systems(Startup, cursor::initial_cursor_center)
        .add_systems(
            FixedUpdate,
            (
                cursor::detect_toggle_cursor_system,
                input::update_input_resource,
            ),
        );
    }
}


