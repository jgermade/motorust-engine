// A button of the start menu (see src/welcome.rs): a card with rounded
// corners and a solid fill and, when it is chosen, a very faint slanted
// chequered flag that slides along, as in the menu design.

#import bevy_render::globals::Globals
#import bevy_ui::ui_vertex_output::UiVertexOutput

@group(0) @binding(1) var<uniform> globals: Globals;

// Fill colour (sRGB, with alpha): the flag is mixed in sRGB, like the design.
@group(1) @binding(0) var<uniform> color: vec4<f32>;
// xy: how far the flag slides in one period (design units); z: width of the
// card (design units); w: how much the flag shows (0 to 1).
@group(1) @binding(1) var<uniform> flow: vec4<f32>;

// The squares of the flag (design units), slanted 16 degrees (tan 16°).
const CELL: f32 = 90.0;
const SLANT: f32 = 0.28674539;
// Seconds the flag takes to slide one period (two squares).
const PERIOD: f32 = 3.0;

@fragment
fn fragment(in: UiVertexOutput) -> @location(0) vec4<f32> {
    let point = in.uv * in.size;

    // Rounded corners: signed distance to the edge of the card (px).
    let radius = in.border_radius.x;
    let half = in.size * 0.5;
    let q = abs(point - half) - (half - vec2(radius));
    let edge = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    let coverage = clamp(0.5 - edge, 0.0, 1.0);

    var fill = color;
    if flow.w > 0.0 {
        let unit = in.size.x / flow.z;
        let p = point / unit - flow.xy * fract(globals.time / PERIOD);
        let column = i32(floor((p.x + p.y * SLANT) / CELL));
        let row = i32(floor(p.y / CELL));
        let dark = (column + row) % 2 == 0;
        // Dark squares slightly darken the card, light ones lighten it less.
        let square = select(vec4(1.0, 1.0, 1.0, 0.03), vec4(0.0, 0.0, 0.0, 0.055), dark);
        let a = square.a * flow.w;
        let alpha = a + fill.a * (1.0 - a);
        let rgb = (square.rgb * a + fill.rgb * fill.a * (1.0 - a)) / max(alpha, 0.0001);
        fill = vec4(rgb, alpha);
    }
    return vec4(to_linear(fill.rgb), fill.a * coverage);
}

fn to_linear(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + 0.055) / 1.055, vec3(2.4));
    return select(high, low, srgb <= vec3(0.04045));
}
