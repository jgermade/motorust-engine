// The curtain of an interdimensional portal as a rider sees it: the other
// realm, drawn from the same place by the rider's portal view (see
// PortalCurtain in src/world/realm.rs), looked up where the curtain is on the
// screen, rippling a little and swirling with light at the rim. Unlit, and not
// tone mapped again: what it shows is lit and tone mapped already.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{globals, view},
}

// x: 1 while the other realm is drawn into `beyond`, 0 otherwise.
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> look: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var beyond: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var beyond_sampler: sampler;

// The swirl's light, cyan (linear), and from how far out (of the radius) it
// covers the view.
const SWIRL: vec3<f32> = vec3(0.22, 0.75, 1.0);
const RIM: vec2<f32> = vec2(0.62, 1.0);

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let centred = in.uv * 2.0 - 1.0;
    let r = length(centred);
    let angle = atan2(centred.y, centred.x);
    let t = globals.time;
    let swirl = 0.5 + 0.5 * sin(angle * 5.0 + r * 9.0 - t * 2.6);
    let rim = smoothstep(RIM.x, RIM.y, r);
    let glow = SWIRL * (0.35 + 0.65 * swirl);

    // Where on the screen this is, rippled a little more towards the rim.
    let ripple = vec2(sin(r * 14.0 - t * 3.0), cos(angle * 3.0 + t * 1.7)) * 0.004 * r;
    let uv = clamp((in.position.xy - view.viewport.xy) / view.viewport.zw + ripple, vec2(0.0), vec2(1.0));
    let seen = textureSampleLevel(beyond, beyond_sampler, uv, 0.0).rgb;

    let open = mix(seen, glow, rim * 0.75) + SWIRL * 0.05 * swirl;
    // Nothing drawn beyond (too far to tell): the swirl alone.
    let closed = mix(SWIRL * 0.35, glow, 0.5 + 0.5 * rim);
    return vec4(select(closed, open, look.x > 0.5), 1.0);
}
