// The ground as a world of small voxels (see src/world/voxel.rs): each
// point of it drawn as the face of a cube `size` metres on a side that a
// staircase of cubes over the ground would show there. A top where the
// ground is level; over a slope, treads and risers, as many risers as the
// slope asks, facing down it; a cliff, all risers. Each cube is of one
// colour, its texture read once, at its middle, a little lighter or
// darker than the next, and cut to a palette of a few shades if asked.
// Otherwise the ground's own material.

#import bevy_pbr::{
    forward_io::{FragmentOutput, VertexOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{alpha_discard, apply_pbr_lighting, main_pass_post_lighting_processing},
}

// Must match VoxelLook in src/world/voxel.rs.
struct VoxelLook {
    // How big each voxel is (m).
    size: f32,
    // How many metres each repetition of the texture covers.
    tile: f32,
    // How many shades each of red, green and blue a voxel's colour is cut
    // to (0: as it is).
    shades: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> look: VoxelLook;

// A random number from 0 to 1 for a cube.
fn hash(cube: vec3<i32>) -> f32 {
    var v = bitcast<vec3<u32>>(cube) * vec3(1664525u, 22695477u, 2891336453u);
    var h = v.x ^ (v.y >> 3u) ^ (v.z << 5u);
    h = h * 747796405u + 2891336453u;
    h = ((h >> ((h >> 28u) + 4u)) ^ h) * 277803737u;
    return f32((h >> 22u) ^ h) * (1.0 / 4294967296.0);
}

@fragment
fn fragment(vertex: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var in = vertex;
    let s = look.size;
    let p = in.world_position.xyz;
    let n = normalize(in.world_normal);

    // The ground's slope: how fast it rises across (m a m), and the column
    // of cubes the point is over, its middle.
    let up = max(n.y, 1e-3);
    let rise = -n.xz / up;
    let column = (floor(p.xz / s) + 0.5) * s;
    // The ground's height at the column's middle, as the slope has it, and
    // the step of the staircase there: where this point is on the same
    // step, it is on a tread, the top of a cube; on another, a riser, the
    // side of a cube, facing down the slope along whichever way it falls
    // most.
    let middle = p.y + dot(rise, column - p.xz);
    let tread = floor(middle / s) == floor(p.y / s);
    var face = vec3(0.0, 1.0, 0.0);
    if !tread {
        if abs(n.x) >= abs(n.z) {
            face = vec3(sign(n.x), 0.0, 0.0);
        } else {
            face = vec3(0.0, 0.0, sign(n.z));
        }
    }

    // The cube this point is on, and its texture read once, at its middle:
    // the texture moved by as much as the point is from it.
    let cube = vec3<i32>(floor(vec3(p.x, select(p.y, middle, tread), p.z) / s));
    let centre = (vec3<f32>(cube) + 0.5) * s;
#ifdef VERTEX_UVS_A
    in.uv = in.uv + (centre.xz - p.xz) / look.tile;
#endif

    var pbr_input = pbr_input_from_standard_material(in, is_front);
    // Each cube a little lighter or darker than the next, the sides a
    // little darker than the tops, as a voxel world's.
    let shade = 0.88 + 0.24 * hash(cube) - select(0.1, 0.0, tread);
    var color = pbr_input.material.base_color.rgb * shade;
    // Cut to a palette of a few shades, evenly apart as the screen shows
    // them, as pixel art's.
    if look.shades >= 2.0 {
        let steps = look.shades - 1.0;
        let srgb = pow(clamp(color, vec3(0.0), vec3(1.0)), vec3(1.0 / 2.2));
        color = pow(round(srgb * steps) / steps, vec3(2.2));
    }
    pbr_input.material.base_color = vec4(color, pbr_input.material.base_color.a);
    // Lit as the cube's face, not as the smooth ground.
    pbr_input.N = face;
    pbr_input.world_normal = face;

    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
