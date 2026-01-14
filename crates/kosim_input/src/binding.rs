use bevy_input::{gamepad::GamepadButton, keyboard::KeyCode};
use bevy_ecs::resource::Resource;

pub enum Action {
    Jump,
    Interact,
    Crouch,
}

pub struct Binding {
    pub key: KeyCode,
    pub button: GamepadButton,
}

// Key configuration
#[derive(Resource)]
pub struct Bindings {
    pub move_forward: KeyCode,
    pub move_backward: KeyCode,
    pub move_left: KeyCode,
    pub move_right: KeyCode,
    pub move_ascend: KeyCode,
    pub move_descend: KeyCode,
    pub action_sprint: Binding,
    pub action_interact: Binding,
    pub action_toggle_crouched: Binding,
    pub action_enable_freelook: Binding,
    pub action_screenshot: Binding,
    pub action_toggle_cursor_focus: KeyCode,
    pub action_toggle_camera_mode: KeyCode,
    pub action_close_application: KeyCode,
}

impl Default for Bindings {
    fn default() -> Self {
        Self {
            move_forward: KeyCode::KeyW,
            move_backward: KeyCode::KeyS,
            move_left: KeyCode::KeyA,
            move_right: KeyCode::KeyD,
            move_ascend: KeyCode::Space,
            move_descend: KeyCode::ShiftLeft,
            action_sprint: Binding {
                key: KeyCode::ShiftLeft,
                button: GamepadButton::LeftTrigger,
            },
            action_toggle_crouched: Binding {
                key: KeyCode::ControlLeft,
                button: GamepadButton::LeftThumb,
            },
            action_toggle_cursor_focus: KeyCode::Tab,
            action_interact: Binding {
                key: KeyCode::KeyE,
                button: GamepadButton::East,
            },
            action_screenshot: Binding {
                key: KeyCode::Equal,
                button: GamepadButton::Start,
            },
            action_toggle_camera_mode: KeyCode::F3,
            action_close_application: KeyCode::Escape,
            action_enable_freelook: Binding {
                key: KeyCode::AltLeft,
                button: GamepadButton::Z, // <-- idk what key to use here.
            },
        }
    }
}