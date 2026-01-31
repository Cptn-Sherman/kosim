use bevy_asset::Handle;
use bevy_color::Color;
use bevy_ecs::bundle::Bundle;
use bevy_text::{Font, FontSmoothing, LineHeight, TextColor, TextFont};
use bevy_ui::widget::Text;


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
pub const GOLD_TEXT_COLOR: Color = Color::srgb(1.0 , 0.72, 0.0);
pub const BORDER_COLOR: Color = Color::srgb(0.8 , 0.8, 0.8);


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
            line_height: LineHeight::RelativeToFont(1.2),
        },
        TextColor(color.unwrap_or(Color::WHITE)),
    )
}

// pub fn get_text_style() -> TextStyle {
//     TextStyle {
//         font: Handle::Weak(Font::default()),
//         font_size: DEFAULT_FONT_SIZE,
//         color: Color::WHITE,
//     }
// }
