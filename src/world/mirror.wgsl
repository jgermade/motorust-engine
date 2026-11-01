// Still, dark water that mirrors what stands round it, the hills, the
// roads, the dams and the sky, more the lower one looks across it, stirred by
// a faint ripple: a dam's lake, and the sea unless it is the cartoon one
// (see MirrorMaterial in src/world/mirror.rs). What is built by it and the
// trees in colour show in it faintly, broken up by drifting patches.

#import bevy_pbr::{
    forward_io::VertexOutput,
    mesh_view_bindings::{globals, view},
    pbr_functions::main_pass_post_lighting_processing,
    pbr_types,
}

// xy: corner of the map of the water's surroundings (m, x and z); zw: its
// size (m).
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> area: vec4<f32>;
// x: the water's level (m); y: how high over it the maps reach at most
// (m); z: how far a reflected line of sight is followed (m); w: 1 if
// anything stands round it (see `standing`).
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var<uniform> water: vec4<f32>;
// What stands round the water: rgb, its colour as lit (linear); a, how high
// it stands over the water (m, less than 0 under it).
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var surroundings: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var surroundings_sampler: sampler;
// The sky's colour each way: across, round from the north (-Z) towards the
// east (+X); down, from overhead to the horizon.
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var sky: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var sky_sampler: sampler;
struct LakeLook {
    // The water's own colour, deep down (linear).
    deep: vec4<f32>,
    // xyz: towards the sun; w: how bright its glint is.
    sun: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var<uniform> look: LakeLook;
// What stands on the ground round the water, apart from it (buildings,
// torii, the crowns of the trees in colour), over the same area: rgb, its
// colour as lit (linear); a, how high its top is over the water (m, less
// than 0 where nothing stands).
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var standing: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(8) var standing_sampler: sampler;
// r: how high over the water its solid part begins (m); g: how much of it
// the water shows (1 for what is built, less for leaves).
@group(#{MATERIAL_BIND_GROUP}) @binding(9) var standing_base: texture_2d<f32>;

const PI: f32 = 3.14159265;
// The reflected ray is followed in at most this many steps, closer together
// near the water, about one every STEP_LENGTH m where it could meet
// something (but never fewer than LEAST_STEPS), and its hit narrowed down in
// a few more.
const STEPS: i32 = 64;
const LEAST_STEPS: i32 = 8;
const STEP_LENGTH: f32 = 3.0;
const REFINE: i32 = 6;
// The ripple: how much it tilts the water, how big its swells are (m), and
// from how far (m) it fades out, where it would shimmer.
const RIPPLE_TILT: f32 = 0.035;
const RIPPLE_SIZE: f32 = 3.5;
const RIPPLE_FADE: vec2<f32> = vec2(40.0, 260.0);
// What stands round the water shows in it this much at most, and this
// much of that where the patches drifting across the water break it up;
// the patches are about this big (m).
const STANDING_SEEN: f32 = 0.6;
const STANDING_BROKEN: f32 = 0.2;
const PATCH_SIZE: f32 = 2.5;

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

// How the ripple tilts the water at `at` (x and z slopes): two faint sets
// of swells drifting slowly across each other.
fn ripple(at: vec2<f32>, t: f32) -> vec2<f32> {
    let p = at / RIPPLE_SIZE;
    let a = vec2(noise(p + vec2(t * 0.21, t * 0.07)), noise(p + vec2(5.2, 1.3) - vec2(t * 0.05, t * 0.18)));
    let b = vec2(noise(p * 2.3 + vec2(9.1, -t * 0.3)), noise(p * 2.3 + vec2(t * 0.26, 4.7)));
    return (a + 0.5 * b - 0.75) * RIPPLE_TILT;
}

// Where a ray from `p` along `r` (on the ground, x and z) is over the map:
// from and to how far along it (m); from after to, never.
fn over_the_map(p: vec2<f32>, r: vec2<f32>) -> vec2<f32> {
    let inverse = 1.0 / select(r, vec2(1e-6), abs(r) < vec2(1e-6));
    let a = (area.xy - p) * inverse;
    let b = (area.xy + area.zw - p) * inverse;
    let low = min(a, b);
    let high = max(a, b);
    return vec2(max(low.x, low.y), min(high.x, high.y));
}

// What the map holds at `at`: off it, nothing, down at the water.
fn surroundings_at(at: vec2<f32>) -> vec4<f32> {
    let uv = (at - area.xy) / area.zw;
    let stored = textureSampleLevel(surroundings, surroundings_sampler, clamp(uv, vec2(0.0), vec2(1.0)), 0.0);
    let inside = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    return select(vec4(0.0), stored, inside);
}

// Whether a point is under the ground round the lake.
fn under(q: vec3<f32>) -> bool {
    return q.y - water.x < surroundings_at(q.xz).a;
}

// What stands at a point over the map, if anything: rgb, its colour; a,
// how much of it the water shows (0 where nothing stands there).
fn standing_at(q: vec3<f32>) -> vec4<f32> {
    let uv = clamp((q.xz - area.xy) / area.zw, vec2(0.0), vec2(1.0));
    let over = q.y - water.x;
    let top = textureSampleLevel(standing, standing_sampler, uv, 0.0);
    if over >= top.a {
        return vec4(0.0);
    }
    let base = textureSampleLevel(standing_base, standing_sampler, uv, 0.0);
    return select(vec4(0.0), vec4(top.rgb, base.g), over > base.r);
}

// How much of what stands round the water shows at `at` on it: patches
// drifting slowly across it, where it shows less.
fn unbroken(at: vec2<f32>, t: f32) -> f32 {
    let p = at / PATCH_SIZE;
    let patches = 0.65 * noise(p + vec2(t * 0.11, -t * 0.07)) + 0.35 * noise(p * 2.7 + vec2(-t * 0.19, 3.1));
    return mix(STANDING_BROKEN, 1.0, smoothstep(0.35, 0.65, patches));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.world_position.xyz;
    let to_eye = view.world_position.xyz - p;
    let distance = length(to_eye);
    let v = to_eye / distance;

    let fade = 1.0 - smoothstep(RIPPLE_FADE.x, RIPPLE_FADE.y, distance);
    let slope = ripple(p.xz, globals.time) * fade;
    let n = normalize(vec3(-slope.x, 1.0, -slope.y));
    var r = reflect(-v, n);
    r.y = max(r.y, 0.001);

    // Follow the reflected ray up from the water until it runs into the
    // ground round it, or leaves the map, or rises over the highest of it,
    // for the sky; and note the first thing standing it passes through on
    // the way.
    let span = over_the_map(p.xz, r.xz);
    let start = max(span.x, 0.0);
    let end = min(min(span.y, water.z), (water.y + 0.5 - (p.y - water.x)) / r.y);
    var hit = false;
    var near = start;
    var far = start;
    var stands = vec4(0.0);
    if end > start {
        let steps = clamp(i32(ceil((end - start) / STEP_LENGTH)), LEAST_STEPS, STEPS);
        for (var i = 1; i <= steps; i++) {
            let f = f32(i) / f32(steps);
            let t = start + (end - start) * f * f;
            let q = p + r * t;
            if water.w > 0.5 && stands.a == 0.0 {
                stands = standing_at(q);
            }
            if under(q) {
                hit = true;
                far = t;
                break;
            }
            near = t;
        }
    }
    for (var k = 0; k < REFINE; k++) {
        let t = 0.5 * (near + far);
        let below = under(p + r * t);
        far = select(far, t, below);
        near = select(t, near, below);
    }
    let ground = surroundings_at((p + r * far).xz).rgb;

    let elevation = asin(clamp(r.y, 0.0, 1.0));
    let azimuth = atan2(r.x, -r.z);
    let sky_uv = vec2(fract(azimuth / (2.0 * PI)), 1.0 - elevation / (0.5 * PI));
    let sky_color = textureSampleLevel(sky, sky_sampler, sky_uv, 0.0).rgb;
    let glint = pow(max(dot(r, look.sun.xyz), 0.0), 600.0) * look.sun.w;
    var reflected = select(sky_color + vec3(glint), ground, hit);
    // What stands shows faintly over what is behind it, broken up.
    reflected = mix(reflected, stands.rgb, STANDING_SEEN * stands.a * unbroken(p.xz, globals.time));

    // Water reflects little looking down into it and more and more the
    // lower one looks across it (Schlick's Fresnel).
    let facing = clamp(dot(v, n), 0.0, 1.0);
    let fresnel = 0.02 + 0.98 * pow(1.0 - facing, 5.0);
    let color = mix(look.deep.rgb, reflected, fresnel);

    // Fog and tone mapping like everything else, with no lighting: what it
    // mirrors is lit already.
    var pbr_input = pbr_types::pbr_input_new();
    pbr_input.frag_coord = in.position;
    pbr_input.world_position = in.world_position;
    pbr_input.material.flags = pbr_types::STANDARD_MATERIAL_FLAGS_FOG_ENABLED_BIT
        | pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT;
    return main_pass_post_lighting_processing(pbr_input, vec4(color, 1.0));
}
