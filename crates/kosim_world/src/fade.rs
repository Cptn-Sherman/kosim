//! Dithered LOD crossfade material and per-chunk fade state.
//!
//! Terrain chunks are drawn with [`ChunkMaterial`], a [`StandardMaterial`] extended
//! with a screen-door dither keyed to a per-chunk `fade` value (see
//! `assets/shaders/chunk_fade.wgsl`). A newly streamed chunk dissolves in
//! (`fade` 0 → 1) while the coarser/finer chunk it replaces dissolves out
//! (`fade` 1 → 0), so LOD changes cross-dissolve instead of popping.

use bevy::asset::Asset;
use bevy::ecs::component::Component;
use bevy::pbr::{ExtendedMaterial, MaterialExtension, StandardMaterial};
use bevy::reflect::Reflect;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

/// The terrain chunk material: `StandardMaterial` plus the dither-fade extension.
pub type ChunkMaterial = ExtendedMaterial<StandardMaterial, ChunkFade>;

/// Time in seconds for a new chunk to fully dither in.
pub const FADE_SECONDS: f32 = 0.4;
/// How long a replaced chunk stays (opaque, as a backing) before despawning. Must
/// comfortably exceed mesh latency + [`FADE_SECONDS`] so its replacements are solid
/// before it is removed.
pub const RETIRE_SECONDS: f32 = 0.8;

/// StandardMaterial extension carrying the per-chunk fade level in `[0, 1]`.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct ChunkFade {
    #[uniform(100)]
    pub fade: f32,
}

impl MaterialExtension for ChunkFade {
    fn fragment_shader() -> ShaderRef {
        "shaders/chunk_fade.wgsl".into()
    }
}

/// Per-chunk fade state. A fresh chunk dithers in (`value` 0 → 1). When replaced it
/// becomes `retiring`: it snaps to fully opaque and stays as a solid backing (so the
/// incoming chunk's dither holes reveal it, not the background) until `timer` reaches
/// [`RETIRE_SECONDS`], then it despawns.
#[derive(Component)]
pub struct Fade {
    pub value: f32,
    pub retiring: bool,
    pub timer: f32,
}
