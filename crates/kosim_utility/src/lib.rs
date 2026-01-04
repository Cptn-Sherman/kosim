
use std::ops::{Add, Mul, Sub};

pub mod format_value;
pub mod interpolated_value;

#[macro_export]
macro_rules! ternary {
    ($condition:expr, $true_expr:expr, $false_expr:expr) => {
        if $condition {
            $true_expr
        } else {
            $false_expr
        }
    };
}

// Pulled this from Freya Holmer's Lerp smoothing is broken talk. https://www.youtube.com/watch?v=LSNQuFEDOyQ
pub fn exp_decay<T>(a: T, b: T, decay: f32, delta_time: f32) -> T
where
    T: Copy + Sub<Output = T> + Mul<f32, Output = T> + Add<Output = T>,
{
    b + (a - b) * (-decay * delta_time).exp()
}

// * --- Valid File Extensions ---
const VALID_EXTENSIONS_VIDEO: [&str; 3] = ["mp4", "avi", "mkv"];
const VALID_EXTENSIONS_SCREENSHOT: [&str; 3] = ["png", "jpeg", "bmp"];

pub enum ExtensionType {
    Screenshot,
    _Video,
}

pub fn get_valid_extension<'a>(extension: &'a str, ext_type: ExtensionType) -> &'a str {
    let valid_extensions = match ext_type {
        ExtensionType::Screenshot => &VALID_EXTENSIONS_SCREENSHOT,
        ExtensionType::_Video => &VALID_EXTENSIONS_VIDEO,
    };

    let default_extension = match ext_type {
        ExtensionType::Screenshot => "png",
        ExtensionType::_Video => "mp4",
    };

    if valid_extensions.contains(&extension.to_lowercase().as_str()) {
        extension
    } else {
        default_extension
    }
}
