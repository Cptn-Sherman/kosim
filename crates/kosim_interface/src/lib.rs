use bevy::{
    app::{App, Plugin, Startup, Update},
    asset::{AssetServer, Handle},
    camera::visibility::Visibility,
    color::Color,
    ecs::{
        bundle::Bundle,
        component::Component,
        entity::Entity,
        query::{QuerySingleError, With},
        system::{Commands, Query, Res},
    },
    image::Image,
    log::info,
    text::{Font, FontFeatures, FontSmoothing, FontWeight, TextColor, TextFont},
    ui::{
        AlignItems, BackgroundColor, BorderColor, Display, FlexDirection, JustifyContent, Node,
        UiRect, Val,
        widget::{ImageNode, Text},
    },
    utils::default,
};
use kosim_player::focus::{FocusTarget, ObjectInformationComponent};

pub const DEFAULT_FONT_PATH: &str = "fonts/AshlanderPixel_fixed.ttf";
pub const DEFAULT_DEBUG_FONT_PATH: &str = "fonts/mononoki-Bold.ttf";
pub const DEFAULT_FONT_SIZE: f32 = 18.0;

#[allow(dead_code)]
pub const ORANGE_TEXT_COLOR: Color = Color::hsv(0.34, 1.0, 0.5);
#[allow(dead_code)]
pub const YELLOW_GREEN_TEXT_COLOR: Color = Color::hsv(0.9, 0.69, 0.58);
#[allow(dead_code)]
pub const RED_TEXT_COLOR: Color = Color::srgb(1.0, 0.0, 0.0);
pub const GREY_TEXT_COLOR: Color = Color::srgb(0.8, 0.8, 0.8);
#[allow(dead_code)]
pub const GOLD_TEXT_COLOR: Color = Color::srgb(1.0, 0.72, 0.0);
pub const BORDER_COLOR: Color = Color::srgb(0.6, 0.6, 0.6);
pub const HUD_BACKGROUND_COLOR: Color = Color::srgba(0.05, 0.05, 0.05, 0.75);

pub struct KosimInterfacePlugin;

impl Plugin for KosimInterfacePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_sample_hud)
            .add_systems(Update, update_focus_target_hud);
    }
}

#[allow(dead_code)]
pub fn gen_text_section(
    value: Option<String>,
    size: Option<f32>,
    color: Option<Color>,
    font: Handle<Font>,
) -> impl Bundle {
    (
        Text::new(value.unwrap_or_default()),
        TextFont {
            font,
            font_size: size.unwrap_or(DEFAULT_FONT_SIZE),
            font_smoothing: FontSmoothing::AntiAliased,
            weight: FontWeight::BOLD,
            font_features: FontFeatures::default(),
        },
        TextColor(color.unwrap_or(Color::WHITE)),
    )
}

#[allow(dead_code)]
pub fn vertical_stack_node(gap: Option<f32>) -> Node {
    Node {
        display: Display::Flex,
        align_items: AlignItems::Center,
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(gap.unwrap_or(8.0)),
        padding: UiRect::all(Val::Px(8.0)),
        border: UiRect::all(Val::Px(2.5)),
        ..Default::default()
    }
}

#[derive(Component)]
pub struct HudObjectInfoRootNode;

#[derive(Component)]
pub struct HudObjectInfoTextNode;

pub fn update_focus_target_hud(
    focus_target: Query<(Entity, &FocusTarget, &ObjectInformationComponent)>,
    mut root_node: Query<(&mut Node, &mut Visibility), With<HudObjectInfoRootNode>>,
    mut text_node: Query<&mut Text, With<HudObjectInfoTextNode>>,
) {
    let (_node, mut visibility) = match root_node.single_mut() {
        Ok(result) => result,
        Err(QuerySingleError::NoEntities(_)) => {
            info!("HUD root node not found, cannot show focus target information.");
            return;
        }
        Err(QuerySingleError::MultipleEntities(_)) => {
            info!(
                "Found {} root nodes, cannot determine which to show focus target information on.",
                root_node.iter().len()
            );
            return;
        }
    };

    let mut text = match text_node.single_mut() {
        Ok(result) => result,
        Err(QuerySingleError::NoEntities(_)) => {
            info!("HUD text node not found, cannot show focus target information.");
            return;
        }
        Err(QuerySingleError::MultipleEntities(_)) => {
            info!(
                "Found {} HUD text nodes, cannot determine which to show focus target information on.",
                text_node.iter().len()
            );
            return;
        }
    };

    let (_entity, _focus_target, object_info) = match focus_target.single() {
        Ok(result) => {
            *visibility = Visibility::Visible;
            result
        }
        Err(QuerySingleError::NoEntities(_)) => {
            // info!("No focus target, hiding HUD.");
            *visibility = Visibility::Hidden;
            return;
        }
        Err(QuerySingleError::MultipleEntities(_)) => {
            // info!("Multiple focus targets found, cannot determine which to display.");
            *visibility = Visibility::Hidden;
            return;
        }
    };

    text.0 = object_info.name.clone().into();
}

pub fn create_sample_hud(mut cmd: Commands, asset_server: Res<AssetServer>) {
    // Setup the default fonts
    let default_font: Handle<Font> = asset_server.load(DEFAULT_FONT_PATH);
    let default_debug_font: Handle<Font> = asset_server.load(DEFAULT_DEBUG_FONT_PATH);

    // Spawn in the crosshair
    let cursor_size: f32 = 24.0;
    let crosshair_texture_handle: Handle<Image> = asset_server.load("textures/crosshair007.png");

    // Center Look UI
    cmd.spawn(Node {
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        display: Display::Flex,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        flex_direction: FlexDirection::Column,
        ..default()
    })
    .with_children(|parent| {
        parent.spawn((
            Node {
                width: Val::Px(cursor_size),
                height: Val::Px(cursor_size),
                ..default()
            },
            ImageNode {
                image: crosshair_texture_handle.into(),
                ..default()
            },
        ));
    });

    // Focus Target UI
    cmd.spawn((Node {
        width: Val::Percent(100.0),
        height: Val::Percent(45.0),
        display: Display::Flex,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::FlexStart,
        flex_direction: FlexDirection::Column,
        top: Val::Percent(55.0),
        ..default()
    },))
        .with_children(|parent| {
            parent
                .spawn((
                    Node {
                        display: Display::Flex,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        padding: UiRect {
                            left: Val::Px(12.0),
                            right: Val::Px(12.0),
                            top: Val::Px(8.0),
                            bottom: Val::Px(8.0),
                        },
                        border: UiRect::all(Val::Px(2.5)),
                        ..default()
                    },
                    BackgroundColor(HUD_BACKGROUND_COLOR),
                    BorderColor::all(BORDER_COLOR),
                    HudObjectInfoRootNode,
                    Visibility::Hidden,
                ))
                .with_children(|parent| {
                    // -- COMMAND PROMPT SEGMENT --
                    parent.spawn((
                        Text::new("E: Take".to_string()),
                        TextFont {
                            font: default_font.clone(),
                            font_size: 14.0,
                            font_smoothing: FontSmoothing::AntiAliased,
                            weight: FontWeight::BOLD,
                            font_features: FontFeatures::default(),
                        },
                        TextColor(GOLD_TEXT_COLOR),
                        Visibility::Inherited,
                    ));

                    // -- TARGET NAME SEGMENT --
                    parent.spawn((
                        Text::new("NO TARGET".to_string()),
                        TextFont {
                            font: default_font.clone(),
                            font_size: DEFAULT_FONT_SIZE,
                            font_smoothing: FontSmoothing::AntiAliased,
                            weight: FontWeight::BOLD,
                            font_features: FontFeatures::default(),
                        },
                        TextColor(Color::WHITE),
                        HudObjectInfoTextNode,
                        Visibility::Inherited,
                    ));

                    // -- TARGET INFO SEGMENT --
                    parent.spawn((
                        Text::new("3.1 Kg".to_string()),
                        TextFont {
                            font: default_font.clone(),
                            font_size: 14.0,
                            font_smoothing: FontSmoothing::AntiAliased,
                            weight: FontWeight::BOLD,
                            font_features: FontFeatures::default(),
                        },
                        TextColor(GREY_TEXT_COLOR),
                        Visibility::Inherited,
                    ));
                });
        });
}
