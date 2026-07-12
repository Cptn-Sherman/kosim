//! Point (radial) gravity toward a planet centre, and the helpers the controller
//! uses to work relative to the local "up" direction instead of world +Y.

use bevy::prelude::*;

/// The planet the player is bound to. Gravity, the ground probe, the ride spring and
/// the capsule's orientation are all taken relative to the direction from `center`.
#[derive(Resource)]
pub struct PlanetGravity {
    /// Planet centre in world space. Must match the world's planet centre
    /// (`kosim_world` centres it on the origin).
    pub center: Vec3,
    /// Gravitational acceleration toward the centre (units/s²).
    pub strength: f32,
}

impl Default for PlanetGravity {
    fn default() -> Self {
        Self {
            center: Vec3::ZERO,
            // Match Avian's old global gravity, which the ride spring/damping were
            // tuned against; stronger gravity makes the spring overshoot on landing.
            strength: 9.81,
        }
    }
}

/// The local "up" (away from the planet centre) at world position `pos`. Falls back
/// to world +Y exactly at the centre.
///
/// The ground probe needs no separate re-orienting system: its `ShapeCaster`
/// direction is *local* (relative to the entity rotation), and the capsule is aligned
/// so its local `-Y` already points radially down toward the planet centre.
#[inline]
pub fn up_at(pos: Vec3, center: Vec3) -> Vec3 {
    (pos - center).normalize_or(Vec3::Y)
}
