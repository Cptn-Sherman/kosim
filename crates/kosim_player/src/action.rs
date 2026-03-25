use avian3d::prelude::{
    Collider, ConstantForce, Forces, RayHits, WriteRigidBodyForces, forces::ForcesItem,
};
use bevy::{
    ecs::{
        entity::Entity,
        query::With,
        system::{Query, Res, ResMut},
    },
    input::{ButtonInput, gamepad::Gamepad, keyboard::KeyCode},
    log::{info, trace},
    math::Vec3,
};

use bevy_enhanced_input::prelude::InputAction;
use kosim_input::binding::Bindings;
use kosim_utility::format_value::{format_value_f32, format_value_vec3};

use crate::{
    Player, PlayerControlConfig,
    body::{Body, IgnoreRayCollision, PlayerColliderFlag, StandingSpringForce, compute_ray_length},
    motion::Motion,
    stance::{Stance, StanceType},
};

//** -- JUMPING LOGIC -- */
pub fn detect_action_jumping(
    mut player_query: Query<
        (
            Entity,
            Forces,
            &mut StandingSpringForce,
            &mut ConstantForce,
            &Motion,
            &mut Stance,
            &Body,
            &RayHits,
        ),
        With<Player>,
    >,
    ignored_entities: Query<Entity, With<IgnoreRayCollision>>,
    player_config: Res<PlayerControlConfig>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    let (
        entity,
        mut forces,
        mut standing_spring,
        mut constant_force,
        motion,
        mut stance,
        body,
        ray_hits,
    ) = player_query.single_mut().expect("We do some errors");
    if stance.current == StanceType::Standing
        && keys.pressed(KeyCode::Space)
        && stance.lockout_timer <= 0.0
    {
        let ray_length: f32 = compute_ray_length(entity, ignored_entities, ray_hits);
        stance.lockout_timer = player_config.stance_lockout;
        stance.current = StanceType::Airborne;
        constant_force.y = 0.0;

        apply_jump_force(
            &mut forces,
            &player_config,
            ray_length,
            &mut stance,
            &mut standing_spring,
            &motion,
            &body,
        );
    }
}

pub fn apply_jump_force(
    forces: &mut ForcesItem<'_, '_>,
    player_config: &Res<PlayerControlConfig>,
    ray_length: f32,
    stance: &mut Stance,
    standing_spring: &mut StandingSpringForce,
    motion: &Motion,
    body: &Body,
) {
    // Apply the stance cooldown now that we are jumping.
    stance.lockout_timer = player_config.stance_lockout;

    let half_jump_strength: f32 = player_config.jump_strength / 2.0;
    let clamped_jump_force: f32 =
        compute_clamped_jump_force_factor(&body, &standing_spring, ray_length);

    // todo: make this value changable.

    let dynamic_jump_strength: f32 = half_jump_strength + (half_jump_strength * clamped_jump_force);

    // maybe instead of half the strength getting added to the up we added it directionally only so you always jump x height but can
    // use more of the timing to aid in forward momentum.

    // find the movement vector in the x and z direction.
    let jump_direction: Vec3 = motion
        .movement_vector
        .current
        .mul_add(Vec3::ONE, Vec3::from_array([0.0, 1.0, 0.0]))
        .normalize_or_zero();

    let impulse_vec: Vec3 = jump_direction * player_config.jump_strength;

    info!(
        "impulse_vec: {}",
        format_value_vec3(impulse_vec, Some(3), true)
    );

    // apply the jump force.
    forces.apply_local_linear_impulse(impulse_vec.into());

    trace!(
        "{{ Jump Strength: {}/{}, factor: {}, ray_length: {} }}",
        format_value_f32(dynamic_jump_strength, Some(3), true),
        player_config.jump_strength,
        format_value_f32(clamped_jump_force, Some(3), true),
        format_value_f32(ray_length, Some(3), true)
    );
}

fn compute_clamped_jump_force_factor(
    body: &Body,
    standing_spring: &StandingSpringForce,
    ray_length: f32,
) -> f32 {
    // Constants defined elsewhere in the code
    let full_standing_ray_length: f32 = standing_spring.length.current;
    let half_standing_ray_length: f32 =
        standing_spring.length.current - (body.current_body_height / 4.0);

    // This value represents the range of acceptable ray lengths for the player.
    let standing_ray_length_range: f32 = full_standing_ray_length - half_standing_ray_length;

    // Ensure the input is within the specified range
    let clamped_ray_length = f32::clamp(
        ray_length,
        half_standing_ray_length,
        standing_spring.length.current,
    );

    // Apply the linear transformation
    // Normalize clamped_ray_length to a value between 0.0 and 1.0.
    let normalized_distance =
        (clamped_ray_length - half_standing_ray_length) / standing_ray_length_range;

    // Subtract the normalized distance from CAPSULE_HEIGHT.
    let result: f32 = body.current_body_height - normalized_distance;

    // Ensure the output is within the range [0.0, 1.0].
    f32::clamp(result, 0.0, 1.0)
}

//** -- SPRINTING LOGIC -- */
#[derive(InputAction)]
#[action_output(bool)]
pub struct Sprint;

// This is the system that detects if the player is sprinting and updates the motion.sprinting flag accordingly.
pub fn detect_action_sprinting(
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

//* -- CROUCHING -- */
#[derive(InputAction)]
#[action_output(bool)]
pub struct Crouch;

pub fn detect_action_crouching(
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

        info!(
            "Updated: Crouched -> {}, Collider scaled to: {:?}",
            stance.crouched,
            collider.scale()
        );
    }
}

//** -- FOOTSTEP LOGIC -- */
// todo: This should be moved later on.

use bevy::{
    asset::{AssetServer, Handle},
    ecs::{
        component::Component,
        event::Event,
        message::{Message, MessageReader, MessageWriter},
        resource::Resource,
        system::Commands,
    },
    time::Time,
};
use bevy_kira_audio::{Audio, AudioControl, AudioSource};
use bevy_turborand::{DelegatedRng, GlobalRng};
use kosim_utility::ternary;

const PLAYBACK_RANGE: f64 = 0.4;

#[derive(Message, Event, Clone)]
pub struct FootstepEvent {
    pub(crate) dir: FootstepDirection,
    pub(crate) volume: f32,
}

// this is the time in seconds between when the player takes a step. When running this is increased by the configured running speed multiplier.
// todo: When the ActionStep happens that is the point in time we apply a small impulse downward so the spring can have a lil' bump.

// This is the time in seconds between each footstep. When sprinting this value is multiplied.
pub const ACTION_STEP_DELTA_DEFAULT: f32 = 0.64;
const LOCKIN_ACTION_THRESHOLD_PERCENTAGE: f32 = 0.1;
const _BUMP_ACTION_THRESHOLD_PERCENTAGE: f32 = 0.70;
const _BUMP_REMAINING_ACTION_STEP: f32 =
    ACTION_STEP_DELTA_DEFAULT * (1.0 - _BUMP_ACTION_THRESHOLD_PERCENTAGE);
const LOCKIN_ACTION_STEP_DELTA: f32 =
    ACTION_STEP_DELTA_DEFAULT * (1.0 - LOCKIN_ACTION_THRESHOLD_PERCENTAGE);

#[derive(Component)]
pub struct ActionStep {
    pub(crate) dir: FootstepDirection,
    pub(crate) bumped: bool,
    pub(crate) delta: f32,
}

#[derive(Clone, PartialEq)]
pub enum FootstepDirection {
    None,
    Left,
    Right,
}

impl Default for FootstepDirection {
    fn default() -> Self {
        FootstepDirection::None
    }
}

// todo: update this to use constants so you can customize the offset from each ear.
// Maybe obsolete if a 3D sound implementation is used instead. Would be nice for ui.

const FOOTSTEP_CENTER: f32 = 0.5;
const FOOTSTEP_OFFSET: f32 = 0.05;

impl FootstepDirection {
    fn value(&self) -> f32 {
        match self {
            FootstepDirection::None => FOOTSTEP_CENTER,
            FootstepDirection::Left => FOOTSTEP_CENTER - FOOTSTEP_OFFSET,
            FootstepDirection::Right => FOOTSTEP_CENTER + FOOTSTEP_OFFSET,
        }
    }

    fn flip(&self) -> Self {
        match self {
            FootstepDirection::None => FootstepDirection::None,
            FootstepDirection::Left => FootstepDirection::Right,
            FootstepDirection::Right => FootstepDirection::Left,
        }
    }
}

#[derive(Resource)]
pub struct FootstepAudioHandle(Handle<AudioSource>);

pub fn load_footstep_sfx(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load("audio/Concrete20.wav");
    commands.insert_resource(FootstepAudioHandle(handle.clone()));
}

pub const DEFAULT_STEP_VOLUME: f32 = -12.0;
pub const UNSIGNED_STEP_VOLUME_SPRINT_BONUS: f32 = 2.0;
pub const UNSIGNED_STEP_VOLUME_UNMOVING_PENALTY: f32 = 4.0;

// todo: move this somewhere more appropriate.
// ! This should ideally not take in and load a new sound ever time and should be loaded once. ALSO, remove the inability to iterate over all the events this should be solved with an update.
// ! ALSO GENERALIZE THIS TO ANY SOUND.
// ! You should only need to send panning, volume and a sound effect tag to get the right one and it looks up from asset map or some shit...
pub fn play_footstep_sfx(
    mut ev_footstep: MessageReader<FootstepEvent>,
    mut global_rng: ResMut<GlobalRng>,
    audio: Res<Audio>,
    my_audio_handle: Res<FootstepAudioHandle>,
) {
    let mut should_play: bool = false;
    let mut panning: f32 = 0.5;
    let mut volume: f32 = DEFAULT_STEP_VOLUME;

    for ev in ev_footstep.read() {
        should_play = true;
        panning = ev.dir.value();
        volume = ev.volume;
    }

    if should_play {
        // info!("Playing footstep with volume: {}", volume);
        let random_playback_rate: f64 = global_rng.f64() * PLAYBACK_RANGE + 0.8;
        audio
            .into_inner()
            .play(my_audio_handle.0.clone())
            .with_panning(panning)
            .with_playback_rate(random_playback_rate)
            .with_volume(volume);
    }
}

pub fn tick_footstep(
    mut ev_footstep: MessageWriter<FootstepEvent>,
    mut query: Query<(&mut ActionStep, &mut Stance, &Motion), With<Player>>,
    // mut camera_query: Query<
    //     (&mut Transform, &mut SmoothedCamera),
    //     (With<Camera3d>, Without<Player>),
    // >,
    player_config: Res<PlayerControlConfig>,
    config: Res<PlayerControlConfig>,
    time: Res<Time>,
) {
    for (mut action, stance, motion) in query.iter_mut() {
        // you must be on the ground for this sound to play.
        if stance.current != StanceType::Standing && stance.current != StanceType::Landing {
            continue;
        }
        // if you are not moving and need to take more than 85% of your remaining step we play no sound.
        if motion.moving == false && action.delta >= LOCKIN_ACTION_STEP_DELTA {
            continue;
        }

        // scale the speed based on if you are sprinting or if you are not moving and are resting your foot.
        // when this value is higher you finish your step sooner.

        let step_speed_scale: f32 =
            motion.movement_speed.current / player_config.default_movement_speed;

        // info!("Step Speed Scale: {}", step_speed_scale);

        let mut _ride_height_offset: f32 = ternary!(
            motion.sprinting,
            config.ride_height_step_offset,
            -config.ride_height_step_offset
        );

        if motion.sprinting == true || motion.moving == false {
            _ride_height_offset *= 1.4; // this is kinda arbitrary. but this little bit of kick is applied when you start sprinting from a stand still.
        }

        // reduce the time by elaspsed times the scale.
        action.delta -= time.delta_secs() * step_speed_scale;
        let mut vol: f32 = ternary!(
            motion.moving,
            DEFAULT_STEP_VOLUME,
            DEFAULT_STEP_VOLUME - UNSIGNED_STEP_VOLUME_UNMOVING_PENALTY
        );
        if motion.sprinting {
            vol += UNSIGNED_STEP_VOLUME_SPRINT_BONUS;
        }

        // bump the riding height when the delta is less than the bump threshold.
        // todo: this needs to be moved to another component that handles view bobbing separately.
        // if config.enable_view_bobbing
        //     && action.delta <= BUMP_REMAINING_ACTION_STEP
        //     && action.bumped == false
        // {
        //     // ! DISABLING HEADBOB RIGHT NOW
        //     // stance.ride_height.current =
        //     //     config.ride_height + (ride_height_offset * current_ride_height_offset_scaler);
        //     action.bumped = true;
        //     let (camera_transform, mut _smoothed_camera) = camera_query.single_mut().unwrap();
        //     let (_yaw, _pitch, _) = camera_transform.rotation.to_euler(EulerRot::default());
        //     let dir: f32 = ternary!(action.dir == FootstepDirection::Left, 1.0, -1.0);
        //     let sprinting_scale: f32 = ternary!(motion.sprinting, 0.2, 0.15);
        //     let roll: f32 = dir * ROTATION_AMOUNT.to_radians() * sprinting_scale;
        //     // smoothed_camera.lean.target = Vec3::from_array([yaw, pitch, roll]);
        //     // smoothed_camera.lock_lean = LEAN_LOCKOUT_TIME;
        //     // info!(
        //     //     "Leaning: {}, with roll: {}",
        //     //     format_value_vec3(_smoothed_camera.lean.current, Some(2), true),
        //     //     format_value_f32(roll, Some(6), true)
        //     // );
        // }

        // if the inter step delta has elapsed increase the delta, flip the dir, reset the bump, and queue the sound event.
        if action.delta <= 0.0 {
            // send the play sound event.
            ev_footstep.write(FootstepEvent {
                dir: action.dir.clone(),
                volume: vol,
            });
            // reset the delta.
            action.delta += ACTION_STEP_DELTA_DEFAULT;
            // reset the bumped flag.
            action.bumped = false;
            // flip the direction of the footstep panning.
            action.dir = action.dir.flip();
        }
    }
}
