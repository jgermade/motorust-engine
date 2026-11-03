// The world drawn as pixel art, as the 8- and 16-bit games drew theirs (see
// PixelArt in src/render_scale.rs): shown over the whole window, each of the
// pixels it was drawn at a sharp square, its colours cut to a few shades of
// red, green and blue, with ordered (Bayer) dithering between one shade and
// the next, so that gradients such as the sky's break up into the patterns of
// dots of the old consoles instead of bands.

#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}

#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif
#ifdef SRGB_OUTPUT
#import bevy_render::color_operations::linear_to_srgb
#endif

struct PixelLook {
    // The image's size (pixels).
    size: vec2<f32>,
    // How many shades each of red, green and blue has.
    levels: f32,
    // How much the dithering breaks the bands between them (0 to 1).
    dither: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> look: PixelLook;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var world: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var world_sampler: sampler;

// The threshold of the 4 × 4 Bayer matrix at a pixel (0 to 15): the bits of
// x ^ y and y interleaved, the other way round.
fn bayer(pixel: vec2<u32>) -> f32 {
    let x = pixel.x & 3u;
    let y = pixel.y & 3u;
    let xy = x ^ y;
    let value = ((xy & 1u) << 3u) | ((y & 1u) << 2u) | (((xy >> 1u) & 1u) << 1u) | ((y >> 1u) & 1u);
    return f32(value);
}

fn to_srgb(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3(0.0031308));
}

fn to_linear(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + 0.055) / 1.055, vec3(2.4));
    return select(high, low, srgb <= vec3(0.04045));
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    // The pixel of the world drawn under this one of the window: read as it
    // is, not blended with its neighbours.
    let size = vec2<u32>(look.size);
    let pixel = min(vec2<u32>(mesh.uv * look.size), size - vec2(1u));
    let linear = textureLoad(world, pixel, 0).rgb;
    // Cut to the shades, evenly apart as the screen shows them (in sRGB),
    // each pixel nudged up or down by its place in the dithering's pattern.
    let steps = look.levels - 1.0;
    let nudge = ((bayer(pixel) + 0.5) / 16.0 - 0.5) * look.dither;
    let srgb = clamp(floor(to_srgb(clamp(linear, vec3(0.0), vec3(1.0))) * steps + 0.5 + nudge) / steps, vec3(0.0), vec3(1.0));
    var color = vec4(to_linear(srgb), 1.0);
#ifdef TONEMAP_IN_SHADER
    color = tonemapping::tone_mapping(color, view.color_grading);
#endif
#ifdef SRGB_OUTPUT
    color = vec4(linear_to_srgb(color.rgb), color.a);
#endif
    return color;
}
