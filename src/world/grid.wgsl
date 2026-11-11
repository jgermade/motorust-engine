// The neon grid on the sea under a sunset of the 80s (see GridMaterial in
// src/world/sea.rs): lines every so many metres either way, squared to the
// view, worked out for
// each pixel from how far apart they are on the screen there, so that they
// are as wide on the screen near and far (they face the eye wherever it
// is), smoothed at their edges; where the cells grow too small on the
// screen (far off, edge on, where they would crowd and shimmer) as much of
// the pixel as they would cover on average, on to the horizon.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::view,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> color: vec4<f32>;
// x: how far apart (m); y: how wide (pixels); z, w: fade from and to (m).
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> lines: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Squared to the view, as the 80s drew it: one set of lines across it,
    // level with the horizon, the other running away from the eye to its
    // vanishing point ahead; turned about the eye as it turns, so that going
    // on, the lines across come towards it.
    let ahead_3d = -view.world_from_view[2].xyz;
    let ahead = normalize(select(vec2(0.0, 1.0), ahead_3d.xz, length(ahead_3d.xz) > 1e-4));
    let side = vec2(ahead.y, -ahead.x);
    let eye = view.world_position.xz;
    let from_eye = in.world_position.xz - eye;
    // Along the view from a fixed point, so that the lines across move as the
    // eye goes on rather than with it.
    let along = dot(in.world_position.xz, ahead);
    let coord = vec2(dot(from_eye, side), along) / lines.x;
    let step = max(fwidth(coord), vec2(1e-5));
    // How many pixels from the nearest line, either way.
    let off = abs(fract(coord - 0.5) - 0.5) / step;
    let nearest = min(off.x, off.y);
    let sharp = 1.0 - clamp(nearest - lines.y * 0.5 + 0.5, 0.0, 1.0);
    // Where a cell is only a few pixels on the screen, the lines would crowd
    // and shimmer: there, as much of the pixel as they would cover on
    // average, so that far off they run on to the horizon as a haze of
    // their colour instead of stopping short of it.
    let crowded = smoothstep(0.08, 0.4, max(step.x, step.y));
    let average = min(lines.y * (step.x + step.y), 0.3);
    var alpha = mix(sharp, average, crowded);
    let distance = length(in.world_position.xyz - view.world_position.xyz);
    alpha *= 1.0 - smoothstep(lines.z, lines.w, distance);
    return vec4(color.rgb, color.a * alpha);
}
