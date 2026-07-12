// Terrain chunk shader: triplanar texture-array shading + screen-door LOD crossfade.
//
// Extends StandardMaterial. Each vertex's material (its texture-array layer) is
// carried in the red vertex-colour channel. The isosurface has no UVs, so the layer
// is sampled triplanar from world position. A per-chunk `fade` value drives an
// ordered-dither discard so LOD chunks cross-dissolve without popping.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, alpha_discard},
    forward_io::{VertexOutput, FragmentOutput},
}

struct ChunkFade {
    fade: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> chunk_fade: ChunkFade;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var terrain_tex: texture_2d_array<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var terrain_sampler: sampler;

// World units per texture tile (the 32px texture repeats every 1/scale units).
const TEX_SCALE: f32 = 0.5;

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
    if chunk_fade.fade < dither_threshold(in.position.xy) {
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
