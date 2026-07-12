// Screen-door dither fade for terrain LOD crossfades.
//
// An extension to StandardMaterial: before running the normal PBR fragment, discard
// the pixel when the per-chunk `fade` value is below this pixel's ordered-dither
// threshold. Animating `fade` 0 -> 1 dissolves a chunk in; 1 -> 0 dissolves it out.
// Two overlapping LOD chunks fading opposite ways cross-dissolve with no visible pop
// and no transparency sorting.

#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing, alpha_discard},
    forward_io::{VertexOutput, FragmentOutput},
}

struct ChunkFade {
    fade: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100)
var<uniform> chunk_fade: ChunkFade;

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
    pbr_input.material.base_color =
        alpha_discard(pbr_input.material, pbr_input.material.base_color);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
