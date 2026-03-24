
use bevy::ecs::entity::Entity;
use bevy::ecs::query::With;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Query, Res, ResMut};
use bevy::input::ButtonInput;
use bevy::input::gamepad::{Gamepad, GamepadAxis};
use bevy::input::keyboard::KeyCode;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::log::trace;
use bevy::math::{Vec2, Vec3};
use bevy::platform::time;
use bevy::time::Time;
use bevy::window::{PrimaryWindow, Window};
use kosim_utility::exp_decay;
use kosim_utility::format_value::{format_value_vec2, format_value_vec3};
use kosim_utility::interpolated_value::InterpolatedValue;

use crate::binding::Bindings;
use crate::InputConfig;

// todo: make this adjustable in the config.
const ANALOGE_STICK_DEADZONE: f32 = 0.1;

#[derive(Resource)]
pub struct Input {
    pub movement_raw: Vec3,
    pub focus_delta_raw: Vec2,
    pub focus_delta_smoothed: InterpolatedValue<Vec2>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            movement_raw: Vec3::ZERO,
            focus_delta_raw: Vec2::ZERO,
            focus_delta_smoothed: InterpolatedValue::new(Vec2::ZERO, 24.0),
        }
    }
}

pub fn clear_input_resource(mut input: ResMut<Input>) {
    input.movement_raw = Vec3::ZERO;
    input.focus_delta_raw = Vec2::ZERO;
}

pub fn update_input_resource(
    mut input: ResMut<Input>,
    accumulated_mouse_motion: ResMut<AccumulatedMouseMotion>,
    primary_window: Query<&Window, With<PrimaryWindow>>,
    gamepads: Query<(Entity, &Gamepad)>,
    keys: Res<ButtonInput<KeyCode>>,
    config: Res<InputConfig>,
    key_bindings: Res<Bindings>,
    time: Res<Time>,
) {
    if keys.pressed(key_bindings.move_forward) {
        input.movement_raw.z += 1.0;
    }
    if keys.pressed(key_bindings.move_backward) {
        input.movement_raw.z += -1.0;
    }
    if keys.pressed(key_bindings.move_left) {
        input.movement_raw.x += -1.0;
    }
    if keys.pressed(key_bindings.move_right) {
        input.movement_raw.x += 1.0;
    }

    input.focus_delta_raw.x += config.mouse_look_sensitivity * accumulated_mouse_motion.delta.x;
    input.focus_delta_raw.y += config.mouse_look_sensitivity * accumulated_mouse_motion.delta.y;

    if let Ok((_entity, gamepad)) = gamepads.single() {
        let left_stick_x: f32 = gamepad.get(GamepadAxis::LeftStickX).unwrap_or_default();
        let left_stick_y: f32 = gamepad.get(GamepadAxis::LeftStickY).unwrap_or_default();
        let right_stick_x: f32 = gamepad.get(GamepadAxis::RightStickX).unwrap_or_default();
        let right_stick_y: f32 = gamepad.get(GamepadAxis::RightStickY).unwrap_or_default();

        if left_stick_x.abs() > ANALOGE_STICK_DEADZONE {
            input.movement_raw.x += left_stick_x;
        }

        if left_stick_y.abs() > ANALOGE_STICK_DEADZONE {
            input.movement_raw.y += left_stick_y;
        }

        if let Ok(window) = primary_window.single() {
            let window_scale: f32 = window.height().min(window.width());

            if right_stick_x.abs() > ANALOGE_STICK_DEADZONE {
                input.focus_delta_raw.x += config.gamepad_look_sensitivity * right_stick_x * window_scale
            }

            if right_stick_y.abs() > ANALOGE_STICK_DEADZONE {
                input.focus_delta_raw.y += config.gamepad_look_sensitivity * right_stick_y * window_scale
            }
        }
    }

    input.focus_delta_smoothed.current = exp_decay::<Vec2>(
        input.focus_delta_smoothed.current,
        input.focus_delta_raw,
        input.focus_delta_smoothed.decay,
        time.delta_secs(),
    );

    trace!(
        "Movement: {}, Direction: {}",
        format_value_vec3(input.movement_raw, Some(2), true),
        format_value_vec2(input.focus_delta_raw, Some(2), true)
    );
}
