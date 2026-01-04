use std::fmt::{Display, Write};

use bevy_math::{EulerRot, Quat, Vec2, Vec3};

pub const NO_PERCENTAGE: &str = "---.-%";

// todo: Add bool for restrict range to keep between 0.0-100.0.
// todo: Add config for number of decimal places.
// todo: Add config for number of numerial places, the number of digits to the right side of the dot.
// todo: Replace "None" with something like nan% or better.
pub fn format_percentage<T>(value: T) -> String
where
    T: Display + PartialOrd + Into<f64>,
{
    let v: f64 = value.into();
    if v >= 0.0 && v <= 100.0 {
        format!("{: >5.1}%", v)
    } else {
        NO_PERCENTAGE.to_owned()
    }
}

pub fn format_value_f32(
    value: f32,
    decimal_digits: Option<usize>,
    format_negative_space: bool,
) -> String {
    let mut buffer: String = String::new();

    let rounded_value: i32 = value as i32;

    let num_digits: usize = if rounded_value == 0 {
        1 // Account for the single digit zero
    } else {
        rounded_value.to_string().len() // Calculate the number of digits
    };

    let width: usize = num_digits + decimal_digits.unwrap_or(0);

    if format_negative_space && value >= 0.0 {
        write!(
            &mut buffer,
            " {:>width$.decimal_width$}", // <--- this line has a single extra space, its hard to see :)
            value,
            width = width,
            decimal_width = decimal_digits.unwrap_or(0)
        )
    } else {
        write!(
            &mut buffer,
            "{:>width$.decimal_width$}",
            value,
            width = width,
            decimal_width = decimal_digits.unwrap_or(0)
        )
    }
    .expect("Failed to write to buffer while formatting value as string!");

    buffer
}

pub fn format_value_vec3(
    vec: Vec3,
    decimal_digits: Option<usize>,
    format_negative_space: bool,
) -> String {
    return format!(
        "[{}, {}, {}]",
        format_value_f32(vec.x, decimal_digits, format_negative_space),
        format_value_f32(vec.y, decimal_digits, format_negative_space),
        format_value_f32(vec.z, decimal_digits, format_negative_space)
    );
}

#[allow(dead_code)]
pub fn format_value_vec2(
    vec: Vec2,
    decimal_digits: Option<usize>,
    format_negative_space: bool,
) -> String {
    return format!(
        "[{}, {}]",
        format_value_f32(vec.x, decimal_digits, format_negative_space),
        format_value_f32(vec.y, decimal_digits, format_negative_space)
    );
}

pub fn format_value_quat(
    quat: Quat,
    decimal_digits: Option<usize>,
    format_negative_space: bool,
    output_euler: Option<EulerRot>,
) -> String {
    match output_euler {
        None => {
            return format!(
                "[{}, {}, {}, {}]",
                format_value_f32(quat.x, decimal_digits, format_negative_space),
                format_value_f32(quat.y, decimal_digits, format_negative_space),
                format_value_f32(quat.z, decimal_digits, format_negative_space),
                format_value_f32(quat.w, decimal_digits, format_negative_space)
            );
        }
        _ => {
            let (yaw, pitch, roll) = quat.to_euler(output_euler.unwrap());
            return format!(
                "[{}, {}, {}]",
                format_value_f32(yaw, decimal_digits, format_negative_space),
                format_value_f32(pitch, decimal_digits, format_negative_space),
                format_value_f32(roll, decimal_digits, format_negative_space),
            );
        }
    }
}
