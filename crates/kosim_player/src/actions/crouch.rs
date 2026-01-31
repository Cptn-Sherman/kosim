use avian3d::prelude::Collider;
use bevy_ecs::{entity::Entity, query::With, system::{Query, Res, ResMut}};
use bevy_input::{ButtonInput, gamepad::Gamepad, keyboard::KeyCode};
use bevy_math::Vec3;
use kosim_input::binding::Bindings;

use crate::{Player, PlayerControlConfig, body::{Body, PlayerColliderFlag, Stance}};



pub fn toggle_crouching(
    mut player_query: Query<(&mut Body, &mut Stance), With<Player>>,
    mut player_collider_query: Query<&mut Collider, With<PlayerColliderFlag>>, // , (With<PlayerCollider>, With<PlayerColliderFlag>, Without<Player>)
    player_config: ResMut<PlayerControlConfig>,
    gamepad_query: Query<(Entity, &Gamepad)>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
) {
    for (mut body, mut stance) in player_query.iter_mut() {
        let mut pressed: bool = false;
        if let Ok((_entity, gamepad)) = gamepad_query.single() {
            if gamepad.just_pressed(bindings.action_toggle_crouched.button)
                || keys.just_pressed(bindings.action_toggle_crouched.key)
            {
                pressed = true;
            }
        } else {
            if keys.just_pressed(bindings.action_toggle_crouched.key) {
                pressed = true;
            }
        }

        if !pressed {
            return;
        }

        let mut collider = player_collider_query.single_mut().unwrap();

        // Toggle crouching flag
        stance.crouched = !stance.crouched;

        if stance.crouched == true {
            // Update the collider scale
            let crouched_height: f32 =
                player_config.capsule_height * player_config.crouched_height_factor;
            collider.set_scale(Vec3::from([1.0, crouched_height, 1.0]), 10);
            // stance.ride_height.target =
            //     player_config.ride_height * player_config.crouched_height_factor;
            body.current_body_height = crouched_height;
        } else {
            // Reset the collider scale to One
            collider.set_scale(Vec3::from([1.0, 1.0, 1.0]), 10);
            //stance.ride_height.target = player_config.ride_height;
            body.current_body_height = player_config.capsule_height;
        }

        // info!(
        //     "Updated: Crouched -> {}, Collider scaled to: {:?}",
        //     stance.crouched,
        //     collider.scale()
        // );
    }
}
