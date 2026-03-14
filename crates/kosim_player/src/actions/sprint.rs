use bevy::{ecs::{entity::Entity, query::With, system::{Query, Res}}, input::{ButtonInput, gamepad::Gamepad, keyboard::KeyCode}};
use kosim_input::binding::Bindings;

use crate::{Player, body::{Stance, StanceType}, motion::Motion};


pub fn toggle_sprinting(
    mut player_query: Query<(&mut Motion, &Stance), With<Player>>,
    gamepad_query: Query<(Entity, &Gamepad)>,
    keys: Res<ButtonInput<KeyCode>>,
    bindings: Res<Bindings>,
) {
    for (mut motion, stance) in player_query.iter_mut() {
        if stance.current == StanceType::Airborne {
            return;
        }
        
        motion.sprinting = keys.pressed(bindings.action_sprint.key);

        if let Ok((_entity, gamepad)) = gamepad_query.single() {
            if !motion.sprinting {
                motion.sprinting = gamepad.pressed(bindings.action_sprint.button);
            }
        }
    }
}
