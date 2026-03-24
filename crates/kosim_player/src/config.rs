use bevy::ecs::resource::Resource;


#[derive(Resource)]
pub struct PlayerControlConfig {
    pub(crate) capsule_height: f32,
    pub(crate) ride_height: f32,
    pub(crate) ride_height_step_offset: f32, // this is the amount we add when a step is taken to simulate head bob.
    pub(crate) ray_length_offset: f32,
    pub(crate) ride_spring_strength: f32,
    pub(crate) ride_spring_damper: f32,
    pub(crate) stance_lockout: f32,
    pub(crate) jump_strength: f32,
    pub(crate) default_movement_speed: f32,
    pub(crate) sprint_speed_factor: f32,
    pub(crate) _movement_decay: f32,
    pub(crate) _mouse_look_sensitivity: f32,
    pub(crate) _gamepad_look_sensitivity: f32,
    pub(crate) _enable_view_bobbing: bool,
    pub(crate) crouched_height_factor: f32,
}

impl Default for PlayerControlConfig {
    fn default() -> Self {
        Self {
            capsule_height: 1.0,
            ride_height: 1.5,
            ride_height_step_offset: 0.15,
            ray_length_offset: 0.15,
            ride_spring_strength: 3500.0,
            ride_spring_damper: 300.0,
            stance_lockout: 0.5,
            jump_strength: 400.0,
            default_movement_speed: 10.0,
            sprint_speed_factor: 2.0,
            _movement_decay: 0.90,
            _mouse_look_sensitivity: 0.0825,
            _gamepad_look_sensitivity: 0.0012, // This value was made up by me!
            _enable_view_bobbing: true,
            crouched_height_factor: 0.80,
        }
    }
}