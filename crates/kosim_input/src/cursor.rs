use crate::binding::Bindings;
use bevy::{ecs::{query::With, system::{Query, Res, Single}}, input::{ButtonInput, keyboard::KeyCode}, log::{info, warn}, math::Vec2, window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window}};
use kosim_utility::ternary;


// * --- Cursor Grab ---
// Start up system used to capture the mouse.
// ! There is currently a bug in the x11 implementation which causes this to fail on linux and sets the window to monitor 0.
// ! The initial cursor lock will succeed but the center will fail.
pub fn initial_lock_cursor(cursor_options: Single<&mut CursorOptions>) {
    set_cursor_grab_mode(cursor_options, CursorGrabMode::Locked);
}

pub fn initial_cursor_center(mut primary_window: Query<&mut Window, With<PrimaryWindow>>) {
    if let Ok(mut window) = primary_window.single_mut() {
        center_cursor(&mut window);
    } else {
        warn!("Primary window not found for `initial_cursor_center`!");
    }
}

pub fn detect_toggle_cursor_system(
    mut primary_window: Query<&mut Window, With<PrimaryWindow>>,
    cursor_options: Single<&mut CursorOptions>,
    keys: Res<ButtonInput<KeyCode>>,
    key_bindings: Res<Bindings>,
) {
    if let Ok(mut window) = primary_window.single_mut() {
        if keys.just_pressed(key_bindings.action_toggle_cursor_focus) {
            toggle_cursor_grab_mode(&mut window, cursor_options);
        }
    } else {
        // FIXME: warn!("Primary window not found for `detect_toggle_cursor`!");
    }
}

fn set_cursor_grab_mode(mut cursor_options: Single<&mut CursorOptions>, grab_mode: CursorGrabMode) {
    cursor_options.grab_mode = grab_mode;
    cursor_options.visible = ternary!(grab_mode == CursorGrabMode::None, true, false);
    info!(
        "CursorGrabMode: {{ {} }}",
        grab_mode_stringified(&grab_mode)
    );
}

// Sets the cursor to the center of the window.
pub fn center_cursor(window: &mut Window) {
    let center: Vec2 = Vec2 {
        x: window.width() / 2.,
        y: window.height() / 2.,
    };
    window.set_cursor_position(Some(center));
}

fn toggle_cursor_grab_mode(window: &mut Window, cursor_options: Single<&mut CursorOptions>) {
    match cursor_options.grab_mode {
        CursorGrabMode::None => {
            set_cursor_grab_mode(cursor_options, CursorGrabMode::Locked);
            center_cursor(window);
        }
        _ => {
            set_cursor_grab_mode(cursor_options, CursorGrabMode::None);
        }
    }
}

fn grab_mode_stringified(grab_mode: &CursorGrabMode) -> String {
    match grab_mode {
        CursorGrabMode::Confined => "Confined".to_string(),
        CursorGrabMode::Locked => "Locked".to_string(),
        CursorGrabMode::None => "None".to_string(),
    }
}