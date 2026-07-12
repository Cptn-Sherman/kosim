// Terrain chunk shader: triplanar texture-array shading, geomorphing LOD, and a
// screen-door crossfade.
//
// Extends StandardMaterial. Per-vertex terrain data travels in the vertex colour
// (the isosurface has no UVs): `r` = texture-array layer, `gba` = the geomorph
// displacement vector to the parent-LOD surface (already edge-pinned at chunk faces
// so Transvoxel stitching stays watertight). The vertex stage slides each vertex
// onto the parent-LOD surface by the chunk's morph factor, so a chunk being swapped
// for its parent (or vice versa) shows identical geometry — the LOD pop becomes a
// continuous slide. The factor is a CPU-computed per-chunk uniform (not a per-pixel
// view distance) so the shadow prepass (`chunk_prepass.wgsl`) — whose `view` is the
// light, not the camera — displaces identically. The fragment stage
// triplanar-samples the layer and applies a per-chunk ordered-dither fade so
// streamed chunks cross-dissolve.
//
// chunk_params: x = dither fade (0..1), y = geomorph factor (0 full detail →
// 1 parent surface), z/w = unused.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, alpha_discard},
    forward_io::{Vertex, VertexOutput, FragmentOutput},
    mesh_functions,
    view_transformations::position_world_to_clip,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> chunk_params: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var terrain_tex: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var terrain_sampler: sampler;

@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;

    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_normal = mesh_functions::mesh_normal_local_to_world(
        vertex.normal,
        vertex.instance_index,
    );
    var world_position = mesh_functions::mesh_position_local_to_world(
        world_from_local,
        vec4<f32>(vertex.position, 1.0),
    );

    // Geomorph: slide toward the parent-LOD surface by the chunk's morph factor.
    world_position = vec4<f32>(
        world_position.xyz + vertex.color.yzw * chunk_params.y,
        1.0,
    );

    out.world_position = world_position;
    out.position = position_world_to_clip(world_position.xyz);
    out.color = vertex.color;
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
    return out;
}

// UV scale: the 32x32 texture repeats every 1/scale world units. 1.0 = one 32x32
// tile per unit.
const TEX_SCALE: f32 = 1.0;

// 4x4 ordered (Bayer) dither threshold in (0, 1) for the given framebuffer pixel.
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

// Triplanar sample of one array layer, projected from world position and blended by
// the (squared, sharpened) world normal.
fn triplanar(world_pos: vec3<f32>, world_normal: vec3<f32>, layer: i32) -> vec4<f32> {
    var w = pow(abs(world_normal), vec3<f32>(4.0));
    w = w / (w.x + w.y + w.z);
    let cx = textureSample(terrain_tex, terrain_sampler, world_pos.yz * TEX_SCALE, layer);
    let cy = textureSample(terrain_tex, terrain_sampler, world_pos.zx * TEX_SCALE, layer);
    let cz = textureSample(terrain_tex, terrain_sampler, world_pos.xy * TEX_SCALE, layer);
    return cx * w.x + cy * w.y + cz * w.z;
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    // Screen-door fade: drop pixels the current fade level hasn't "reached" yet.
    if chunk_params.x < dither_threshold(in.position.xy) {
        discard;
    }

    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // The material layer travels in the red vertex-colour channel.
    let layer = i32(round(in.color.r));
    let albedo = triplanar(in.world_position.xyz, normalize(in.world_normal), layer);
    pbr_input.material.base_color = vec4<f32>(albedo.rgb, 1.0);

    pbr_input.material.base_color =
        alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
