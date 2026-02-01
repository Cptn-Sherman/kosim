use std::time::Duration;

use avian3d::{
    PhysicsPlugins,
    prelude::{Collider, Mass, PhysicsDebugPlugin, RigidBody},
};
use bevy::{
    asset::RenderAssetUsages,
    color::palettes::tailwind::{AMBER_400, SKY_400, ZINC_200},
    dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin, FrameTimeGraphConfig},
    light::{CascadeShadowConfigBuilder, DirectionalLightShadowMap, SunDisk},
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
    render::render_asset::RenderAssetBytesPerFrame,
};
use bevy_kira_audio::{Audio, AudioControl, AudioEasing, AudioPlugin, AudioTween};
use bevy_turborand::prelude::RngPlugin;
use kosim_camera::{
    CameraConfig, ToggleCameraEvent, create_camera,
    freecam::{create_free_camera, move_free_camera},
    load_toggle_camera_soundfxs, play_toggle_camera_soundfx, swap_camera_target, take_screenshot,
};
use kosim_input::{
    InputConfig,
    binding::Bindings,
    cursor::{detect_toggle_cursor_system, initial_cursor_center, initial_grab_cursor},
    input::{Input, update_input_resource},
};
use kosim_player::PlayerPlugin;

#[derive(Component)]
struct Sun;

fn main() {
    App::new()
        .init_resource::<Bindings>()
        .insert_resource(RenderAssetBytesPerFrame::new(2_000_000_000))
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .insert_resource(CameraConfig {
            hdr: true,
            fov: 75.0,
            screenshot_format: "png".into(),
        })
        .insert_resource(InputConfig {
            sensitivity: 1.0,
            gamepad_look_sensitivity: 1.0,
            mouse_look_sensitivity: 1.0,
        })
        .insert_resource(Input::default())
        .add_plugins((
            DefaultPlugins,
            RngPlugin::new().with_rng_seed(0),
            PhysicsDebugPlugin::default(),
            PhysicsPlugins::default(),
            // DebugInterfacePlugin,
            PlayerPlugin,
            AudioPlugin,
            FpsOverlayPlugin {
                config: FpsOverlayConfig {
                    enabled: false, // !Bug: will be fixed in 0.18 release.
                    frame_time_graph_config: FrameTimeGraphConfig {
                        enabled: true,
                        target_fps: 60.0,
                        ..default()
                    },
                    ..default()
                },
            },
            // bevy_panic_handler::PanicHandler::new().build(),
            // TemporalAntiAliasPlugin,
            // AtmospherePlugin,
            // BlockoutPlugin,
            // InfiniteGridPlugin,
            // SunMovePlugin,
            // RandomStarsPlugin,
        ))
        .add_systems(PreStartup, (create_camera, create_free_camera))
        .add_systems(
            Startup,
            (
                setup,
                start_background_audio,
                load_toggle_camera_soundfxs,
                initial_grab_cursor,
                initial_cursor_center, // ! Bug: "cursor position can be set only for locked cursor" however, window is locked.
            )
                .chain(),
        )
        .add_systems(
            Update,
            (
                update_input_resource,
                detect_toggle_cursor_system,
                swap_camera_target,
                move_free_camera,
                play_toggle_camera_soundfx,
                take_screenshot,
                close_on_key,
            ),
        )
        .add_message::<ToggleCameraEvent>()
        .run();
}

fn start_background_audio(asset_server: Res<AssetServer>, audio: Res<Audio>) {
    // ! DO NOT DISTRIBUTE - This music file is for internal testing only!
    audio
        .into_inner()
        .play(asset_server.load("audio/liminal-spaces-ambient.ogg"))
        .fade_in(AudioTween::new(
            Duration::from_millis(18000),
            AudioEasing::InPowf(0.125),
        ))
        .with_volume(-20.0)
        .looped();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard_materials: ResMut<Assets<StandardMaterial>>,
    //mut extended_materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, BlockoutMaterialExt>>>,
) {
    // info!("Percentage Test: {}", format_percentage::<f32>(120.0f32));

    // commands.spawn(InfiniteGridBundle::default());

    let _cascade_shadow_config = CascadeShadowConfigBuilder {
        first_cascade_far_bound: 0.3,
        maximum_distance: 3.0,
        ..default()
    }
    .build();

    // create the 'Sun' with volumetric Lighting enabled.
    let _sun_id = commands
        .spawn((
            DirectionalLight {
                illuminance: light_consts::lux::RAW_SUNLIGHT,
                shadows_enabled: true,
                ..default()
            },
            Transform::default().with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4)),
            SunDisk::default(),
        ))
        .id();

    // Plane
    let plane_size: f32 = 128.0;
    let plane_thickness: f32 = 0.005;

    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(plane_size, plane_thickness, plane_size),
        Transform::from_xyz(0.0, 2.0, 0.0),
        get_sample_material(SKY_400.into(), &mut standard_materials),
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
        MeshMaterial3d(standard_materials.add(StandardMaterial {
            base_color: AMBER_400.into(),
            ..default()
        })),
        Transform::from_xyz(2.0, 25.0, 2.0),
    ));

    // spawn a cube with physics and a material
    let mini_plateform_cube_size: f32 = 2.0;
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(
            mini_plateform_cube_size,
            mini_plateform_cube_size,
            mini_plateform_cube_size,
        ),
        Mass(5.0),
        Mesh3d(meshes.add(Cuboid::from_length(mini_plateform_cube_size))),
        get_sample_material(ZINC_200.into(), &mut standard_materials),
        Transform::from_xyz(4.0, (mini_plateform_cube_size / 2.0) + 2.0, 8.0),
    ));

    // spawn a cube with physics and a material
    let small_plateform_cube_size: f32 = 4.0;
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(
            small_plateform_cube_size,
            small_plateform_cube_size,
            small_plateform_cube_size,
        ),
        Mass(5.0),
        Mesh3d(meshes.add(Cuboid::from_length(small_plateform_cube_size))),
        get_sample_material(ZINC_200.into(), &mut standard_materials),
        Transform::from_xyz(8.0, (small_plateform_cube_size / 2.0) + 2.0, 8.0),
    ));

    // spawn a cube with physics and a material
    let medium_plateform_cube_size: f32 = 6.0;
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(
            medium_plateform_cube_size,
            medium_plateform_cube_size,
            medium_plateform_cube_size,
        ),
        Mass(5.0),
        Mesh3d(meshes.add(Cuboid::from_length(medium_plateform_cube_size))),
        get_sample_material(ZINC_200.into(), &mut standard_materials),
        Transform::from_xyz(16.0, (medium_plateform_cube_size / 2.0) + 2.0, 8.0),
    ));

    // spawn a cube with physics and a material
    let large_plateform_cube_size: f32 = 8.0;
    commands.spawn((
        RigidBody::Static,
        Collider::cuboid(
            large_plateform_cube_size,
            large_plateform_cube_size,
            large_plateform_cube_size,
        ),
        Mass(5.0),
        Mesh3d(meshes.add(Cuboid::from_length(large_plateform_cube_size))),
        get_sample_material(ZINC_200.into(), &mut standard_materials),
        Transform::from_xyz(24.0, (large_plateform_cube_size / 2.0) + 2.0, 8.0),
    ));
}

pub fn get_sample_material(
    base_color: Color,
    standard_materials: &mut ResMut<Assets<StandardMaterial>>,
) -> MeshMaterial3d<StandardMaterial> {
    MeshMaterial3d(standard_materials.add(StandardMaterial {
        base_color,
        ..default()
    }))

    /*
           MeshMaterial3d(extended_materials.add(ExtendedMaterial {
           base: StandardMaterial {
               base_color: ZINC_200.into(),
               ..default()
           },
           extension: BlockoutMaterialExt::default(),
       })),
    */
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

// * --- Generate Meshes ---
pub fn generate_plane_mesh(
    meshes: &mut ResMut<Assets<Mesh>>,
    width: f32,
    length: f32,
    uv_scale: f32,
) -> Handle<Mesh> {
    let half_width = width / 2.0;
    let half_length = length / 2.0;

    let vertices = vec![
        // Top face
        (
            [-half_width, 0.0, half_length],
            [0.0, 1.0, 0.0],
            [0.0, uv_scale * length],
        ), // Top-left
        (
            [half_width, 0.0, half_length],
            [0.0, 1.0, 0.0],
            [uv_scale * width, uv_scale * length],
        ), // Top-right
        (
            [half_width, 0.0, -half_length],
            [0.0, 1.0, 0.0],
            [uv_scale * width, 0.0],
        ), // Bottom-right
        (
            [-half_width, 0.0, -half_length],
            [0.0, 1.0, 0.0],
            [0.0, 0.0],
        ), // Bottom-left
    ];

    let indices = vec![
        0, 1, 2, 2, 3, 0, // top face
    ];

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();

    for (position, normal, uv) in vertices {
        positions.push(position);
        normals.push(normal);
        uvs.push(uv);
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        VertexAttributeValues::from(positions),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, VertexAttributeValues::from(normals));
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, VertexAttributeValues::from(uvs));
    mesh.insert_indices(Indices::U32(indices));

    meshes.add(
        mesh.with_generated_tangents()
            .expect("Failed to generate tangents for the mesh"),
    )
}
