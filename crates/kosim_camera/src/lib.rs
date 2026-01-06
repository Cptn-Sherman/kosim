use avian3d::prelude::TransformInterpolation;
use bevy_asset::{AssetServer, Handle};
use bevy_camera::{Camera, Camera3d, Exposure};
use bevy_core_pipeline::tonemapping::Tonemapping;
use bevy_ecs::component::Component;
use bevy_ecs::event::Event;
use bevy_ecs::prelude::*;
use bevy_ecs::resource::Resource;
use bevy_input::{ButtonInput, keyboard::KeyCode};
use bevy_kira_audio::{Audio, AudioControl, AudioSource};
use bevy_light::VolumetricFog;
use bevy_math::Vec3;
use bevy_pbr::{Atmosphere, AtmosphereMode, AtmosphereSettings};
use bevy_render::view::screenshot::{Screenshot, save_to_disk};
use bevy_transform::components::Transform;
use bevy_utils::default;
use chrono::Local;
use kosim_input::binding::Bindings;
use kosim_utility::get_valid_extension;
use kosim_utility::interpolated_value::InterpolatedValue;

use crate::first_person_camera::SmoothedCamera;
use crate::freecam::FreeCamera;

pub mod first_person_camera;
pub mod freecam;
pub mod third_person_camera;

#[derive(Resource)]
pub struct CameraConfig {
    pub hdr: bool,
    pub fov: f32,
    pub screenshot_format: String,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self { hdr: true, fov: 75.0, screenshot_format: "png".into() }
    }
}

#[derive(Component)]
pub struct GameCamera;

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

#[derive(Message, Event, Clone)]
pub struct ToggleCameraEvent {
    mode: CameraMode,
}

#[derive(Clone)]
pub enum CameraMode {
    FirstPerson,
    ThirdPerson,
    FreeCam,
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
        CameraMode::ThirdPerson => {
            // TODO: No sound for third person camera for now.
        },
    }
}

/** This system was taken from the screenshot example: https://bevyengine.org/examples/Window/screenshot/ */
pub fn take_screenshot(
    mut commands: Commands,
    settings: Res<CameraConfig>,
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
            kosim_utility::ExtensionType::Screenshot
        )
    );

    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path));
}
