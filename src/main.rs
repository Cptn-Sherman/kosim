use std::time::Duration;

use avian3d::{
    PhysicsPlugins,
    prelude::{Collider, Mass, PhysicsDebugPlugin, RigidBody},
};
use bevy::{
    color::palettes::tailwind::{AMBER_400, SKY_400, ZINC_200},
    dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin, FrameTimeGraphConfig},
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, SunDisk},
    prelude::*,
    render::render_asset::RenderAssetBytesPerFrame,
};
use bevy_infinite_grid::{InfiniteGridBundle, InfiniteGridPlugin};
use bevy_kira_audio::{Audio, AudioControl, AudioEasing, AudioPlugin, AudioTween};
use bevy_turborand::prelude::RngPlugin;
use kosim_camera::KosimCameraPlugin;
use kosim_input::{InputConfig, KosimInputPlugin, binding::Bindings, input::Input};
use kosim_interface::KosimInterfacePlugin;
use kosim_player::{PlayerPlugin, focus::ObjectInformationComponent};
use kosim_utility::mesh::generate_plane_mesh;

fn main() {
    App::new()
        .init_resource::<Bindings>()
        .insert_resource(RenderAssetBytesPerFrame::new(2_000_000_000))
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .insert_resource(InputConfig {
            sensitivity: 1.0,
            gamepad_look_sensitivity: 1.0,
            mouse_look_sensitivity: 1.0,
        })
        .insert_resource(Input::default())
        .add_plugins((
            DefaultPlugins,
            KosimInputPlugin,
            KosimCameraPlugin,
            KosimInterfacePlugin,
            AudioPlugin,
            RngPlugin::new().with_rng_seed(0),
            PhysicsDebugPlugin::default(),
            PhysicsPlugins::default(),
            PlayerPlugin,
            FpsOverlayPlugin {
                config: FpsOverlayConfig {
                    enabled: true,
                    refresh_interval: Duration::from_millis(1000),
                    text_config: TextFont {
                        font_size: 16.0,
                        ..default()
                    },
                    frame_time_graph_config: FrameTimeGraphConfig {
                        enabled: true,
                        min_fps: 30.0,
                        target_fps: 240.0,
                        ..default()
                    },
                    ..default()
                },
            },
            InfiniteGridPlugin,
        ))
        .add_systems(
            Startup,
            (setup, start_background_audio).chain(),
        )
        .add_systems(Update, (close_on_key,))
        .run();
}

fn start_background_audio(asset_server: Res<AssetServer>, audio: Res<Audio>) {
    // ! DO NOT DISTRIBUTE - This music file is for internal testing only!
    audio
        .into_inner()
        .play(asset_server.load("audio/liminal-spaces-ambient.ogg"))
        .fade_in(AudioTween::new(
            Duration::from_millis(8000),
            AudioEasing::InPowf(0.125),
        ))
        .with_volume(-20.0)
        .looped();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn(InfiniteGridBundle::default());

    let _cascade_shadow_config = CascadeShadowConfigBuilder {
        first_cascade_far_bound: 0.3,
        maximum_distance: 3.0,
        ..default()
    }
    .build();

    // create the 'Sun' with volumetric Lighting enabled.
    commands.spawn((
        DirectionalLight {
            illuminance: light_consts::lux::RAW_SUNLIGHT,
            shadows_enabled: true,
            ..default()
        },
        Transform::default().with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4)),
        SunDisk::default(),
    ));

    // Plane
    let plane_size: f32 = 128.0;
    let plane_thickness: f32 = 0.005;

    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(plane_size, plane_thickness, plane_size),
        Transform::from_xyz(0.0, 2.0, 0.0),
        get_sample_material(SKY_400.into(), &mut materials),
        Mesh3d(generate_plane_mesh(
            &mut meshes,
            plane_size,
            plane_size,
            1.0 / 16.0,
        )),
    ));

    // spawn a cube with physics and a material
    commands.spawn((
        RigidBody::Dynamic,
        Collider::cuboid(0.5, 0.5, 0.5),
        Mass(5.0),
        Mesh3d(meshes.add(Cuboid::from_length(0.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: AMBER_400.into(),
            ..default()
        })),
        Transform::from_xyz(2.0, 25.0, 2.0),
        ObjectInformationComponent {
            name: "Yellow Box".to_string(),
            description: "A yellow box for testing".to_string(),
        },
    ));

    for i in 0..6 {
        let size: f32 = 2.0 + (i as f32 * 2.0);
        let z_offset: f32 = (0..i)
            .map(|j: i32| 2.0 + (j as f32 * 2.0) + 2.0)
            .sum::<f32>()
            + (size / 2.0);

        commands.spawn((
            RigidBody::Static,
            Collider::cuboid(size, size, size),
            Mass(5.0),
            Mesh3d(meshes.add(Cuboid::from_length(size))),
            get_sample_material(ZINC_200.into(), &mut materials),
            Transform::from_xyz(16.0, (size / 2.0) + 2.0, -z_offset),
        ));
    }
}

pub fn get_sample_material(
    base_color: Color,
    standard_materials: &mut ResMut<Assets<StandardMaterial>>,
) -> MeshMaterial3d<StandardMaterial> {
    MeshMaterial3d(standard_materials.add(StandardMaterial {
        base_color,
        ..default()
    }))
}

// Close the focused window whenever the escape key (Esc) is pressed
// This is useful for examples or prototyping.
pub fn close_on_key(
    mut commands: Commands,
    focused_windows: Query<(Entity, &Window)>,
    input: Res<ButtonInput<KeyCode>>,
    key_bindings: Res<Bindings>,
) {
    for (window, focus) in focused_windows.iter() {
        if !focus.focused {
            continue;
        }

        if input.just_pressed(key_bindings.action_close_application) {
            commands.entity(window).despawn();
        }
    }
}
