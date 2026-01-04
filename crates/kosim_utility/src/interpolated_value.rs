use std::ops::{Add, Mul, Sub};


pub struct InterpolatedValue<T>
where
    T: Copy + Sub<Output = T> + Mul<f32, Output = T> + Add<Output = T>,
{
    pub current: T,
    pub target: T,
    pub decay: f32,
}

impl<T> InterpolatedValue<T>
where
    T: Copy + Sub<Output = T> + Mul<f32, Output = T> + Add<Output = T>,
{
    pub fn new(initial: T, decay: f32) -> Self {
        Self {
            current: initial,
            target: initial,
            decay,
        }
    }
}