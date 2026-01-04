use bevy_ecs::{
    entity::Entity,
    query::With,
    resource::Resource,
    system::{Query, Res, ResMut},
};
use bevy_input::{
    ButtonInput,
    gamepad::{Gamepad, GamepadAxis},
    keyboard::KeyCode,
    mouse::AccumulatedMouseMotion,
};
use bevy_math::{Vec2, Vec3};
use bevy_window::{PrimaryWindow, Window};

use crate::binding::Bindings;
use crate::InputConfig;

// todo: make this adjustable in the config.
const ANALOGE_STICK_DEADZONE: f32 = 0.1;

#[derive(Resource)]
pub struct Input {
    pub movement_raw: Vec3,
    pub focus_delta: Vec2,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            movement_raw: Default::default(),
            focus_delta: Default::default(),
        }
    }
}

pub fn update_input_resource(
    mut input: ResMut<Input>,
    accumulated_mouse_motion: ResMut<AccumulatedMouseMotion>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    gamepads: Query<(Entity, &Gamepad)>,
    keys: Res<ButtonInput<KeyCode>>,
    config: Res<InputConfig>,
    key_bindings: Res<Bindings>,
) {
    // this is the raw input vector
    input.movement_raw = Vec3::ZERO.clone();
    input.focus_delta = Vec2::ZERO.clone();

    if keys.pressed(key_bindings.move_forward) {
        input.movement_raw.z = 1.0;
    }
    if keys.pressed(key_bindings.move_backward) {
        input.movement_raw.z = -1.0;
    }
    if keys.pressed(key_bindings.move_left) {
        input.movement_raw.x = -1.0;
    }
    if keys.pressed(key_bindings.move_right) {
        input.movement_raw.x = 1.0;
    }

    input.focus_delta.x = config.mouse_look_sensitivity * accumulated_mouse_motion.delta.x;
    input.focus_delta.y = config.mouse_look_sensitivity * accumulated_mouse_motion.delta.y;

    if let Ok((_entity, gamepad)) = gamepads.single() {
        let left_stick_x: f32 = gamepad.get(GamepadAxis::LeftStickX).unwrap_or_default();
        let left_stick_y: f32 = gamepad.get(GamepadAxis::LeftStickY).unwrap_or_default();
        let right_stick_x: f32 = gamepad.get(GamepadAxis::RightStickX).unwrap_or_default();
        let right_stick_y: f32 = gamepad.get(GamepadAxis::RightStickY).unwrap_or_default();

        if left_stick_x.abs() > ANALOGE_STICK_DEADZONE {
            input.movement_raw.x = left_stick_x;
        }

        if left_stick_y.abs() > ANALOGE_STICK_DEADZONE {
            input.movement_raw.y = left_stick_y;
        }

        if let Ok(window) = primary_window.single() {
            let window_scale: f32 = window.height().min(window.width());

            if right_stick_x.abs() > ANALOGE_STICK_DEADZONE {
                input.focus_delta.x = config.gamepad_look_sensitivity * right_stick_x * window_scale
            }

            if right_stick_y.abs() > ANALOGE_STICK_DEADZONE {
                input.focus_delta.y = config.gamepad_look_sensitivity * right_stick_y * window_scale
            }
        }
    }

    // info!(
    //     "Movement: {}, Direction: {}",
    //     format_value_vec3(input.movement, Some(2), true),
    //     format_value_vec2(input.direction, Some(2), true)
    // );
}
