use crate::{
    config::Bindings, input::Input, player::Player, utils::{exp_decay, InterpolatedValue}
};
use avian3d::prelude::TransformInterpolation;
use bevy::{
    camera::Exposure, core_pipeline::tonemapping::Tonemapping, light::VolumetricFog, math::Vec3, pbr::{Atmosphere, AtmosphereMode, AtmosphereSettings}, prelude::*, utils::default
};
use bevy_kira_audio::{Audio, AudioControl, AudioSource};

use bevy::{
    input::ButtonInput,
    prelude::{Commands, Entity, KeyCode, Query, Res, With},
    render::view::screenshot::{save_to_disk, Screenshot},
};
use chrono::Local;

use crate::{
    config::EngineSettings,
    utils::{self, get_valid_extension},
};

#[derive(Component)]
pub struct GameCamera;

#[derive(Resource)]
pub struct CameraConfig {
    pub hdr: bool,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self { hdr: true }
    }
}

pub fn create_camera(mut commands: Commands, camera_config: Res<CameraConfig>) {
    let _ = camera_config;
    commands
        .spawn((
            Camera3d::default(),
            Camera::default(),
            Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::ZERO, Vec3::Y),
            Tonemapping::ReinhardLuminance,
            Atmosphere::EARTH,
            AtmosphereSettings {
                rendering_method: AtmosphereMode::Raymarched,
                aerial_view_lut_max_distance: 3.2e5,
                scene_units_to_m: 100.0,
                ..Default::default()
            },
            Exposure::SUNLIGHT,
            GameCamera,
            TransformInterpolation,
            SmoothedCamera {
                lean: InterpolatedValue::<Vec3>::new(Vec3::from_array([0.0, 0.0, 0.0]), 2.0),
                lock_lean: 0.0,
            },
        ))
        .insert(VolumetricFog {
            ambient_intensity: 0.0,
            ..default()
        });
}

#[derive(Component)]
pub struct FreeCamera;

pub fn create_free_camera(mut commands: Commands) {
    commands.spawn((
        Transform::from_xyz(0.0, 5.0, 0.0).looking_to(Vec3::ZERO, Vec3::Y),
        FreeCamera,
    ));
}

#[derive(Message, Event, Clone)]
pub struct ToggleCameraEvent {
    mode: CameraMode,
}

#[derive(Clone)]
pub enum CameraMode {
    FirstPerson,
    FreeCam,
}

pub fn move_free_camera(
    camera_query: Query<&mut Transform, (With<Camera3d>, Without<FreeCamera>)>,
    mut free_entity_query: Query<&mut Transform, With<FreeCamera>>,
    keys: Res<ButtonInput<KeyCode>>,
    key_bindings: Res<Bindings>,
    time: Res<Time>,
) {
    if camera_query.is_empty()
        || camera_query.iter().len() > 1
        || free_entity_query.is_empty()
        || free_entity_query.iter().len() > 1
    {
        warn!("Free Camera Motion System did not recieve expected 1 camera(s) recieved {}, and 1 player(s) recieved {}. Expect Instablity!", camera_query.iter().len(), free_entity_query.iter().len());
        return;
    }

    let camera_transform: &Transform = camera_query.iter().next().unwrap();

    for mut transform in free_entity_query.iter_mut() {
        let mut movement_vector: Vec3 = Vec3::ZERO.clone();
        let speed_vector: Vec3 = Vec3::from([20.0, 20.0, 20.0]);
        // WASD Movement
        if keys.pressed(key_bindings.move_forward) {
            movement_vector += camera_transform.forward().as_vec3();
        }
        if keys.pressed(key_bindings.move_backward) {
            movement_vector += camera_transform.back().as_vec3();
        }
        if keys.pressed(key_bindings.move_left) {
            movement_vector += camera_transform.left().as_vec3();
        }
        if keys.pressed(key_bindings.move_right) {
            movement_vector += camera_transform.right().as_vec3();
        }
        // Ascend and Descend
        if keys.pressed(key_bindings.move_ascend) {
            movement_vector += Vec3::Y;
        }
        if keys.pressed(key_bindings.move_descend) {
            movement_vector -= Vec3::Y;
        }

        // Scale the vector by the elapsed time.
        movement_vector *= speed_vector * time.delta_secs();
        transform.translation += movement_vector;
    }
}

pub fn swap_camera_target(
    mut commands: Commands,
    mut ev_toggle_cam: MessageWriter<ToggleCameraEvent>,
    keys: Res<ButtonInput<KeyCode>>,
    key_bindings: Res<Bindings>,
    mut camera_query: Query<(Entity, &mut Transform, Option<&ChildOf>), With<GameCamera>>,
    player_query: Query<Entity, With<Player>>,
    free_camera_query: Query<Entity, With<FreeCamera>>,
) {
    if !keys.just_pressed(key_bindings.action_toggle_camera_mode) {
        return;
    }

    let mut valid_queries: bool = true;
    if player_query.is_empty() {
        warn!("Player Query was empty, cannot swap camera parent target!");
        valid_queries = false;
    }

    if free_camera_query.is_empty() {
        warn!("Fly Camera Query was empty, cannot swap camera parent target!");
        valid_queries = false;
    }

    if camera_query.is_empty() {
        warn!("Camera Query was empty, cannot swap camera parent target!");
        valid_queries = false;
    }

    if !valid_queries {
        return;
    }

    // this is not safe, should handle none option
    // we first ensure that each of these entities has only one instance
    let player = player_query.iter().next().unwrap();
    let free_camera = free_camera_query.iter().next().unwrap();
    let (camera, mut camera_transform, camera_parent) = camera_query.iter_mut().next().unwrap();
    let camera_parent_unwrapped = camera_parent.unwrap();

    // check the camera to see what its parented to.
    // If its parented to the player, then we want to parent it to the fly camera.
    // else it is parented to the fly camera, and we want it parented to the player.
    if camera_parent_unwrapped.parent() == player {
        camera_transform.translation = Vec3::from_array([0.0, 0.0, 0.0]);
        commands.entity(free_camera).add_children(&[camera]);
        info!("Attached camera to fly_camera entity.");
        ev_toggle_cam.write(ToggleCameraEvent {
            mode: CameraMode::FreeCam,
        });
    } else {
        camera_transform.translation = Vec3::from_array([0.0, 1.0, 0.0]);
        commands.entity(player).add_children(&[camera]);
        info!("Attached camera to player entity.");
        ev_toggle_cam.write(ToggleCameraEvent {
            mode: CameraMode::FirstPerson,
        });
    }
}

#[derive(Component)]
pub struct SmoothedCamera {
    pub lean: InterpolatedValue<Vec3>,
    pub lock_lean: f32,
}
pub const ROTATION_AMOUNT: f32 = 4.0;
pub const LEAN_LOCKOUT_TIME: f32 = 0.15;

pub fn smooth_camera(
    mut camera_query: Query<
        (&mut Transform, &mut SmoothedCamera),
        (With<Camera3d>, Without<Player>),
    >,
    input: Res<Input>,
    time: Res<Time>,
) {
    let (mut camera_transform, mut smoothed_camera) = camera_query.single_mut().unwrap();

    // Update the Curent Lean
    let (yaw, pitch, _) = camera_transform.rotation.to_euler(EulerRot::default());
    //let pitch = input_vector.y * rotation_amount.to_radians();
    let roll: f32 = -1.0 * input.movement_raw.x * ROTATION_AMOUNT.to_radians();
    
    // Set the new target lean and lerp the current value at a constant rate
    // ! for now we will use the constant value 2.0 for lerping. We can probably replace this by just seeing how fast the camera is moving? check the velocity
    let lean_decay: f32 = 2.0; // ternary!(motion.sprinting, 2.0, 8.0);
    if smoothed_camera.lock_lean > 0.0 {
        smoothed_camera.lock_lean -= time.delta_secs();
    } else {
        smoothed_camera.lean.target = Vec3::from_array([yaw, pitch, roll]);
    }

    // Interpolate the smoothed camera lean.
    smoothed_camera.lean.current = exp_decay::<Vec3>(
        smoothed_camera.lean.current,
        smoothed_camera.lean.target,
        lean_decay,
        time.delta_secs(),
    );

    // Apply the lean to the camera transformation.
    camera_transform.rotation = Quat::from_euler(
        EulerRot::default(),
        yaw, // we dont change the yaw.
        pitch,
        smoothed_camera.lean.current.z,
    );
}

#[derive(Resource)]
pub struct ToggleCameraFreeModeAudioHandle(Handle<AudioSource>);

#[derive(Resource)]
pub struct ToggleCameraFirstModeAudioHandle(Handle<AudioSource>);

pub fn load_toggle_camera_soundfxs(mut commands: Commands, asset_server: Res<AssetServer>) {
    let free_handle = asset_server.load("audio/Blip-003.wav");
    let first_handle = asset_server.load("audio/Blip-004.wav");
    commands.insert_resource(ToggleCameraFreeModeAudioHandle(free_handle.clone()));
    commands.insert_resource(ToggleCameraFirstModeAudioHandle(first_handle.clone()));
}

pub fn play_toggle_camera_soundfx(
    first_handle: Res<ToggleCameraFirstModeAudioHandle>,
    free_handle: Res<ToggleCameraFreeModeAudioHandle>,
    mut _ev_footstep: MessageReader<ToggleCameraEvent>,
    audio: Res<Audio>,
) {
    let mut mode: CameraMode = CameraMode::FreeCam;
    let mut should_play: bool = false;
    let volume: f32 = 0.15;

    for _ev in _ev_footstep.read() {
        should_play = true;
        mode = _ev.mode.clone();
    }

    if !should_play {
        return;
    }

    match mode {
        CameraMode::FirstPerson => {
            audio
                .into_inner()
                .play(first_handle.0.clone())
                .with_volume(volume);
        }
        CameraMode::FreeCam => {
            audio
                .into_inner()
                .play(free_handle.0.clone())
                .with_volume(volume);
        }
    }
}

/** This system was taken from the screenshot example: https://bevyengine.org/examples/Window/screenshot/ */
pub fn take_screenshot(
    mut commands: Commands,
    settings: Res<EngineSettings>,
    bindings: Res<Bindings>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if !keys.just_pressed(bindings.action_screenshot.key) {
        return;
    }

    let path: String = format!(
        "./kosim-{}.{}",
        Local::now().format("%Y-%m-%d_%H-%M-%S%.3f").to_string(),
        get_valid_extension(
            &settings.screenshot_format,
            utils::ExtensionType::Screenshot
        )
    );

    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}
