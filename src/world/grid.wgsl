// The neon grid on the sea under a sunset of the 80s (see GridMaterial in
// src/world/sea.rs): lines every so many metres either way, worked out for
// each pixel from how far apart they are on the screen there, so that they
// are as wide on the screen near and far (they face the eye wherever it
// is), smoothed at their edges; faded where the cells grow too small on the
// screen (far off, edge on, where they would crowd and shimmer) and away
// from the camera.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> color: vec4<f32>;
// x: how far apart (m); y: how wide (pixels); z, w: fade from and to (m).
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> lines: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let coord = in.world_position.xz / lines.x;
    let step = max(fwidth(coord), vec2(1e-5));
    // How many pixels from the nearest line, either way.
    let off = abs(fract(coord - 0.5) - 0.5) / step;
    let nearest = min(off.x, off.y);
    var alpha = 1.0 - clamp(nearest - lines.y * 0.5 + 0.5, 0.0, 1.0);
    // Where a cell is only a few pixels on the screen, the lines would crowd.
    alpha *= 1.0 - smoothstep(0.06, 0.2, max(step.x, step.y));
    let distance = length(in.world_position.xyz - view.world_position.xyz);
    alpha *= 1.0 - smoothstep(lines.z, lines.w, distance);
    return vec4(color.rgb, color.a * alpha);
}
