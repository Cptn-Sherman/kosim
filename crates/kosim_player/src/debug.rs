use avian3d::prelude::LinearVelocity;
use bevy_asset::{AssetServer, Handle};
use bevy_camera::Camera3d;
use bevy_color::Color;
use bevy_ecs::{
    component::Component,
    query::{With, Without},
    system::{Commands, Query, Res},
};
use bevy_math::{EulerRot, Quat};
use bevy_text::{Font, TextColor, TextFont, TextSpan};
use bevy_transform::components::Transform;
use bevy_ui::{
    AlignItems, BackgroundColor, BorderColor, Display, FlexDirection, JustifyContent, Node,
    PositionType, UiRect, Val, widget::Text,
};
use bevy_utils::default;
use kosim_interface::{BORDER_COLOR, DEFAULT_DEBUG_FONT_PATH};
use kosim_utility::format_value::{format_value_f32, format_value_quat, format_value_vec3};

use crate::{Player, motion::Motion};

pub fn create_player_debug(mut commands: Commands, asset_server: Res<AssetServer>) {
    let default_font: Handle<Font> = asset_server.load(DEFAULT_DEBUG_FONT_PATH);
    
    let text_font: TextFont = TextFont {
        font: default_font,
        font_size: 11.0,
        ..Default::default()
    };

    commands
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexStart,
            align_items: AlignItems::FlexStart,
            position_type: PositionType::Absolute,
            ..default()
        })
        .with_children(|parent| {
            parent
                .spawn((
                    Node {
                        display: Display::Flex,
                        justify_content: JustifyContent::SpaceAround,
                        align_items: AlignItems::FlexStart,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        margin: UiRect::all(Val::Px(5.0)),
                        padding: UiRect::all(Val::Px(5.0)),
                        border: UiRect::all(Val::Px(2.0)),
                        ..Default::default()
                    },
                    BackgroundColor(Color::srgba(0.05, 0.05, 0.05, 0.75)),
                    BorderColor::all(BORDER_COLOR),
                ))
                .with_children(|parent| {
                    parent
                        .spawn((
                            Text::new("pos: "),
                            text_font.clone(),
                            TextColor(Color::WHITE),
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionPositionDebug,
                            ));
                        });

                    parent
                        .spawn((
                            Text::new("focus: "),
                            text_font.clone(),
                            TextColor(Color::WHITE),
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionRotationDebug,
                            ));
                        });

                    parent
                        .spawn((
                            Text::new("vel: "),
                            text_font.clone(),
                            TextColor(Color::WHITE),
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionVelocityDebug,
                            ));
                        });

                    parent
                        .spawn((
                            Text::new("Movement Vector | decay: "),
                            text_font.clone(),
                            TextColor(Color::WHITE),
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionMovementVectorDecayRateDebug,
                            ));
                        });

                    parent
                        .spawn((
                            Text::new("current: "),
                            text_font.clone(),
                            TextColor(Color::WHITE),
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionMovementVectorCurrentDebug,
                            ));
                        });

                    parent
                        .spawn((
                            Text::new("target:  "),
                            text_font.clone(),
                            TextColor(Color::WHITE),
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionMovementVectorTargetDebug,
                            ));
                        });

                    parent
                        .spawn((
                            Text::new("moving: "),
                            text_font.clone(),
                            TextColor(Color::WHITE),
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionMovementIsMovingDebug,
                            ));
                        })
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new(" | sprinting: "),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                            ));
                        })
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionMovementIsSprintingDebug,
                            ));
                        });

                    parent
                        .spawn((
                            Text::new("current speed: "),
                            text_font.clone(),
                            TextColor(Color::WHITE),
                        ))
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionMovementSpeedCurrentDebug,
                            ));
                        })
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new(" -> target: "),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                            ));
                        })
                        .with_children(|parent| {
                            parent.spawn((
                                TextSpan::new("000"),
                                text_font.clone(),
                                TextColor(Color::WHITE),
                                MotionMovementSpeedTargetDebug,
                            ));
                        });
                });
        });

    // info!("Created Player debug");
}

#[derive(Component)]
pub struct MotionPositionDebug;

pub fn update_debug_position(
    player_query: Query<&Transform, With<Player>>,
    mut query: Query<&mut TextSpan, With<MotionPositionDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let player_transform = player_query.single().unwrap();
    text.0 = format_value_vec3(player_transform.translation, Some(4), true);
}

#[derive(Component)]
pub struct MotionRotationDebug;

pub fn update_debug_rotation(
    player_query: Query<&Transform, With<Player>>,
    camera_query: Query<&Transform, (With<Camera3d>, Without<Player>)>,
    mut query: Query<&mut TextSpan, With<MotionRotationDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let camera_transform = camera_query.single().unwrap();
    let player_transform = player_query.single().unwrap();
    let (player_yaw, _player_pitch, _player_roll) =
        player_transform.rotation.to_euler(EulerRot::default());
    let (_camera_yaw, camera_pitch, camera_roll) =
        camera_transform.rotation.to_euler(EulerRot::default());
    let quat: Quat = Quat::from_euler(EulerRot::default(), player_yaw, camera_pitch, camera_roll);
    text.0 = format_value_quat(quat, Some(4), true, Some(EulerRot::default()));
}

#[derive(Component)]
pub struct MotionVelocityDebug;

pub fn update_debug_linear_velocity(
    player_query: Query<&mut LinearVelocity, With<Player>>,
    mut query: Query<&mut TextSpan, With<MotionVelocityDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let player_linear_velocity = player_query.single().unwrap();
    text.0 = format_value_vec3(player_linear_velocity.0, Some(4), true);
}

#[derive(Component)]
pub struct MotionMovementVectorCurrentDebug;

pub fn update_debug_movement_vector_current(
    player_query: Query<&Motion, With<Player>>,
    mut query: Query<&mut TextSpan, With<MotionMovementVectorCurrentDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let player_motion = player_query.single().unwrap();
    text.0 = format_value_vec3(player_motion.movement_vector.current, Some(4), true);
}

#[derive(Component)]
pub struct MotionMovementVectorTargetDebug;

pub fn update_debug_movement_vector_target(
    player_query: Query<&Motion, With<Player>>,
    mut query: Query<&mut TextSpan, With<MotionMovementVectorTargetDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let player_motion = player_query.single().unwrap();
    text.0 = format_value_vec3(player_motion.movement_vector.target, Some(4), true);
}

#[derive(Component)]
pub struct MotionMovementVectorDecayRateDebug;

pub fn update_debug_movement_vector_decay(
    player_query: Query<&Motion, With<Player>>,
    mut query: Query<&mut TextSpan, With<MotionMovementVectorDecayRateDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let player_motion = player_query.single().unwrap();
    text.0 = format_value_f32(player_motion.movement_vector.decay, Some(2), true);
}

#[derive(Component)]
pub struct MotionMovementIsMovingDebug;

pub fn update_debug_is_moving(
    player_query: Query<&Motion, With<Player>>,
    mut query: Query<&mut TextSpan, With<MotionMovementIsMovingDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let player_motion = player_query.single().unwrap();
    text.0 = player_motion.moving.to_string();
}

#[derive(Component)]
pub struct MotionMovementIsSprintingDebug;

pub fn update_debug_is_sprinting(
    player_query: Query<&Motion, With<Player>>,
    mut query: Query<&mut TextSpan, With<MotionMovementIsSprintingDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let player_motion = player_query.single().unwrap();
    text.0 = player_motion.sprinting.to_string();
}

#[derive(Component)]
pub struct MotionMovementSpeedCurrentDebug;

pub fn update_debug_movement_speed_current(
    player_query: Query<&Motion, With<Player>>,
    mut query: Query<&mut TextSpan, With<MotionMovementSpeedCurrentDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let player_motion = player_query.single().unwrap();
    text.0 = format_value_f32(player_motion.movement_speed.current, Some(2), true);
}

#[derive(Component)]
pub struct MotionMovementSpeedTargetDebug;

pub fn update_debug_movement_speed_target(
    player_query: Query<&Motion, With<Player>>,
    mut query: Query<&mut TextSpan, With<MotionMovementSpeedTargetDebug>>,
) {
    let mut text = query.single_mut().unwrap();
    let player_motion = player_query.single().unwrap();
    text.0 = format_value_f32(player_motion.movement_speed.target, Some(2), true);
}
