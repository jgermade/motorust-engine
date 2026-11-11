// The neon grid on the sea under a sunset of the 80s (see GridMaterial in
// src/world/sea.rs): on the band of sea under the sun, lines every so many
// metres either way, squared to the sun (one set running out to it, the
// other across), still on the water, worked out for
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
// xy: the way to the sun, level; zw: the circuit's middle (m).
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var<uniform> sun: vec4<f32>;
// x: the band's half width (m); y: how much of its sides fades (m).
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> band: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Squared to the sun, still on the water: one set of lines running out
    // to it, the other across, level with the horizon seen towards it, as
    // the 80s drew the ground under their sunsets.
    let to_sun = sun.xy;
    let across = vec2(to_sun.y, -to_sun.x);
    let p = in.world_position.xz;
    let coord = vec2(dot(p, across), dot(p, to_sun)) / lines.x;
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
    let average = min(lines.y * (step.x + step.y), 0.55);
    var alpha = mix(sharp, average, crowded);
    // Only on the band of sea under the sun, from the circuit out to it.
    let from_middle = p - sun.zw;
    let aside = abs(dot(from_middle, across));
    alpha *= 1.0 - smoothstep(band.x - band.y, band.x, aside);
    alpha *= smoothstep(-band.y, 0.0, dot(from_middle, to_sun));
    let distance = length(in.world_position.xyz - view.world_position.xyz);
    alpha *= 1.0 - smoothstep(lines.z, lines.w, distance);
    return vec4(color.rgb, color.a * alpha);
}
