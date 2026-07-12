// Prepass/shadow stages for terrain chunks.
//
// Vertex: applies the exact same geomorph displacement as the main pass in
// `chunk_fade.wgsl` — the per-vertex parent-LOD displacement vector in the vertex
// colour's gba channels, scaled by the chunk's CPU-computed morph factor
// (`chunk_params.y`). Without this, shadow maps render the unmorphed surface and
// shadows swim against the morphing terrain. The factor being a uniform (not a view
// distance) matters here: in the shadow pass `view` is the light, not the camera.
//
// Fragment: applies the same ordered-dither discard as the main pass, so a chunk's
// *shadow* crossfades in and out in lockstep with its visible surface instead of
// popping on the frame a fade completes. (This runs for shadows because the chunk
// material uses `AlphaMode::Mask` — depth-only pipelines skip material fragment
// shaders for opaque materials.)

#import bevy_pbr::{
    mesh_functions,
    prepass_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> chunk_params: vec4<f32>;

// 4x4 ordered (Bayer) dither threshold in (0, 1); matches `chunk_fade.wgsl`. In the
// shadow pass the pattern is anchored to shadow-map texels (stable, since Bevy
// snaps cascades to texel increments), and PCF smooths the dithered depth into a
// soft partial shadow.
fn dither_threshold(frag: vec2<f32>) -> f32 {
    let x = u32(frag.x) % 4u;
    let y = u32(frag.y) % 4u;
    var bayer = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0,
        12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0,
        15.0, 7.0, 13.0, 5.0,
    );
    return (bayer[y * 4u + x] + 0.5) / 16.0;
}

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    var world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );

#ifdef VERTEX_COLORS
    // Geomorph: identical displacement to the main pass.
    world_position = vec4<f32>(
        world_position.xyz + vertex.color.yzw * chunk_params.y,
        1.0,
    );
    out.color = vertex.color;
#endif

    out.world_position = world_position;
    out.position = position_world_to_clip(world_position.xyz);

#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif

#ifdef MOTION_VECTOR_PREPASS
    // Terrain chunks are static; report no motion.
    out.previous_world_position = world_position;
#endif

#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
#endif
#endif

#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif

    return out;
}

// Dither-discard so shadows crossfade with the surface. `FragmentOutput` only
// exists when the pipeline declares outputs (e.g. emulated unclipped depth); plain
// shadow depth passes use the bare variant.
#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput

@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
    if chunk_params.x < dither_threshold(in.position.xy) {
        discard;
    }
    var out: FragmentOutput;
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
    if chunk_params.x < dither_threshold(in.position.xy) {
        discard;
    }
}
#endif
