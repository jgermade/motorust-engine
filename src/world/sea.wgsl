// The sea, drawn as a cel-shaded cartoon sea: flat,
// saturated blues with no lighting, white crests rolling in towards the
// coast and breaking on it, lighter water by the coast, and white foam
// hugging the shore (see src/world/sea.rs).

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{globals, view},
    pbr_functions::main_pass_post_lighting_processing,
    pbr_types,
}

// xy: corner of the shore map (m, x and z); zw: its size (m).
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> shore_area: vec4<f32>;
// Distance to the shore, signed: red, close up (SHORE_NEAR to SHORE_FAR m);
// green, as far as WAVES_REACH m out to sea. Blue and alpha: the way to the
// nearest shore (x and z, from -1 to 1).
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var shore_map: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var shore_sampler: sampler;
// xy: where the swell heads (x and z). Waves only roll in to the shores that
// face it, so they all go one way.
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var<uniform> swell: vec4<f32>;

// What the shore map holds. Must match src/world/sea.rs.
const SHORE_NEAR: f32 = -10.0;
const SHORE_FAR: f32 = 40.0;
const WAVES_REACH: f32 = 400.0;

// The colours (sRGB): far out, at sea, by the coast, and the foam (see
// SeaColors in src/world/sea.rs).
struct SeaColors {
    deep: vec4<f32>,
    open: vec4<f32>,
    shallow: vec4<f32>,
    foam: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var<uniform> colors: SeaColors;

// The waves: one crest every WAVE_SPACING m, each taking WAVE_PERIOD s to
// come in by one spacing, broken into pieces about WAVE_PIECE m long.
const WAVE_SPACING: f32 = 18.0;
const WAVE_PERIOD: f32 = 6.0;
const WAVE_PIECE: f32 = 16.0;
// How far from the camera they fade out (m), where they would shimmer.
const WAVE_FADE: vec2<f32> = vec2(170.0, 400.0);

fn hash1(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(41.3, 289.1))) * 15731.743);
}

// Smooth value noise, from 0 to 1.
fn noise(p: vec2<f32>) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash1(cell);
    let b = hash1(cell + vec2(1.0, 0.0));
    let c = hash1(cell + vec2(0.0, 1.0));
    let d = hash1(cell + vec2(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// How much of a wave covers this point, `offshore` m from the coast. The
// crests run along the coast (lines at the same distance from it) and roll
// in towards it, broken into pieces: few, short and thin out at sea, more,
// longer and thicker as they come in, until they break on the shore.
fn waves(at: vec2<f32>, offshore: f32, t: f32) -> f32 {
    let phase = offshore / WAVE_SPACING + t / WAVE_PERIOD;
    // The nearest crest, which keeps its number as it comes in.
    let crest = round(phase);
    let from_crest = abs(phase - crest) * WAVE_SPACING;
    // 1 by the coast, 0 out at sea.
    let coming_in = 1.0 - smoothstep(6.0, 70.0, offshore);
    // Each crest is broken its own way; more of it shows near the coast.
    let piece = noise(at / WAVE_PIECE + vec2(crest * 7.31, crest * 3.17));
    let threshold = mix(0.66, 0.36, coming_in);
    let body = clamp((piece - threshold) / (1.0 - threshold), 0.0, 1.0);
    // Thick in the middle of each piece and thin at its ends; it melts
    // into the foam on the shore, and fades out far out at sea.
    let thickness = mix(0.3, 1.3, coming_in) * sqrt(body)
        * smoothstep(0.8, 3.5, offshore)
        * (1.0 - smoothstep(0.6 * WAVES_REACH, 0.95 * WAVES_REACH, offshore));
    let width = fwidth(from_crest) + 1e-3;
    return 1.0 - smoothstep(thickness - width, thickness + width, from_crest);
}

fn to_linear(srgb: vec3<f32>) -> vec3<f32> {
    return pow(srgb, vec3(2.2));
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    let t = globals.time;
    let at = in.world_position.xz;

    // Distance to the shore (m): far out at sea outside the map.
    // (Sampled everywhere: textures cannot be sampled under a branch.)
    let uv = (at - shore_area.xy) / shore_area.zw;
    let stored = textureSample(shore_map, shore_sampler, clamp(uv, vec2(0.0), vec2(1.0)));
    let inside = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    let shore = select(SHORE_FAR, mix(SHORE_NEAR, SHORE_FAR, stored.r), inside);
    let offshore = select(WAVES_REACH, stored.g * WAVES_REACH, inside);
    // The coast wobbles a little, and the foam with it.
    let wobble = (noise(at * 0.18 + t * 0.4) - 0.5) * 1.2;
    let near = shore + wobble;

    // Broad patches of deeper and lighter blue out at sea.
    let patches = noise(at / 260.0 + vec2(3.7, -1.9));
    var color = mix(colors.deep.rgb, colors.open.rgb, smoothstep(0.25, 0.75, patches));
    color = mix(colors.shallow.rgb, color, smoothstep(2.0, 30.0, near));

    // White crests coming in, fading out with distance (they would shimmer).
    let distance = length(in.world_position.xyz - view.world_position.xyz);
    let fade = 1.0 - smoothstep(WAVE_FADE.x, WAVE_FADE.y, distance);
    // Close up the fine distance (with the wobble), further out the coarse.
    let to_shore = mix(near, offshore, smoothstep(30.0, 38.0, shore));
    // Only where the shore faces the swell (none outside the map).
    let toward = select(vec2(0.0), stored.ba * 2.0 - 1.0, inside);
    let exposed = smoothstep(0.15, 0.6, dot(toward, swell.xy));
    var foam = waves(at, to_shore, t) * 0.92 * fade * exposed;

    // Foam along the shore, at least a pixel wide however far: seen from
    // afar it stays a line.
    let pixel = fwidth(near);
    foam = max(foam, 1.0 - smoothstep(0.4, 1.6 + pixel, near));

    let srgb = mix(color, colors.foam.rgb, clamp(foam, 0.0, 1.0));

    // Fog and tone mapping like everything else, with no lighting.
    var pbr_input = pbr_types::pbr_input_new();
    pbr_input.frag_coord = in.position;
    pbr_input.world_position = in.world_position;
    pbr_input.material.flags = pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT
        | pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT;
    return main_pass_post_lighting_processing(pbr_input, vec4(to_linear(srgb), 1.0));
}
