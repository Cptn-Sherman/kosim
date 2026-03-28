use bevy::{
    asset::{AssetServer, Handle},
    color::Color,
    ecs::{
        bundle::Bundle, component::Component, entity::Entity, query::With, system::{Commands, Query, Res}
    },
    image::Image,
    text::{Font, FontFeatures, FontSmoothing, FontWeight, TextColor, TextFont},
    ui::{
        AlignItems, Display, FlexDirection, JustifyContent, Node, UiRect, Val,
        widget::{ImageNode, Text},
    },
    utils::default,
};
use kosim_player::focus::{FocusTarget, ObjectInformationComponent};

pub const DEFAULT_FONT_PATH: &str = "fonts/AshlanderPixel_fixed.ttf";
pub const DEFAULT_DEBUG_FONT_PATH: &str = "fonts/Monocraft.ttf";
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

#[allow(dead_code)]
pub const ORANGE_TEXT_COLOR: Color = Color::hsv(0.34, 1.0, 0.5);
#[allow(dead_code)]
pub const YELLOW_GREEN_TEXT_COLOR: Color = Color::hsv(0.9, 0.69, 0.58);
#[allow(dead_code)]
pub const RED_TEXT_COLOR: Color = Color::srgb(1.0, 0.0, 0.0);
#[allow(dead_code)]
pub const GOLD_TEXT_COLOR: Color = Color::srgb(1.0, 0.72, 0.0);
pub const BORDER_COLOR: Color = Color::srgb(0.6, 0.6, 0.6);
pub const HUD_BACKGROUND_COLOR: Color = Color::srgba(0.05, 0.05, 0.05, 0.75);

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

pub fn update_focus_target_hud(
    focus_target: Query<(Entity, &FocusTarget, &ObjectInformationComponent)>,
    mut hud_query: Query<&mut Text, With<ObjectInformationComponent>>,
    mut node_query: Query<&mut Node, With<HudObjectInfoRootNode>>,
) {
    let (entity, focus_target, object_info) = match focus_target.single() {
        Ok(result) => result,
        Err(_) => {
            if let Ok(mut node) = node_query.single_mut() {
                node.display = Display::None;
            }
        }, // No focus target, exit early.
    };

    let mut hud_text = match hud_query.single_mut() {
        Ok(text) => text,
        Err(_) => return, // No HUD text entity, exit early.
    };

    // Update the HUD text with the focused entity's information.
    let entity_info = format!("Focused Entity: {:?}", entity);
    hud_text.0 = object_info.name.clone().into();
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
    cmd.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            display: Display::Flex,
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            padding: UiRect::all(Val::Px(8.0)),
            border: UiRect::all(Val::Px(2.5)),
            ..Default::default()
        },
        BackgroundColor(HUD_BACKGROUND_COLOR),
        BorderColor::all(BORDER_COLOR),
    ))
    .with_children(|parent| {
        parent.spawn((
            gen_text_section(
                Some("Yellow Box".to_string()),
                Some(DEFAULT_FONT_SIZE),
                None,
                default_font.clone(),
            ),
            ObjectInformationComponent {
                name: "Yellow Box".to_string(),
                description: "A yellow box for testing".to_string(),
            },
        ));
        parent.spawn(gen_text_section(
            Some("E: Take".to_string()),
            Some(10.0),
            None,
            default_debug_font.clone(),
        ));
    });
}
