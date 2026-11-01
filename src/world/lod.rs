//! Lighter versions of a mesh, to draw it far away (levels of detail): the
//! same shape with fewer triangles. Its edges are collapsed one by one, the
//! one that changes the shape least first, each into a vertex put where it
//! best keeps the faces round it (quadric error metrics, after Garland and
//! Heckbert): flat stretches go first, and trunks and outlines last. The top
//! of each part of the mesh stays where it is, so it is as tall, and no part
//! goes (a clump of leaves thins out to a triangle). The trees are drawn
//! with them far from the cameras ([`super::trees`]).

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

/// How far past the bound between two levels (m) something goes before it
/// changes level, either way, so that it does not flicker between them on
/// the bound.
pub const HYSTERESIS: f32 = 4.0;

/// The level of detail to draw something with when the nearest camera is
/// `distance` m away, having been drawn at `current`: 0 (the full mesh)
/// nearer than the first of `bounds` (m, nearest first), 1 between the first
/// and the second, and so on; it only changes once `distance` is
/// [`HYSTERESIS`] past a bound.
pub fn level_at(distance: f32, current: usize, bounds: &[f32]) -> usize {
    let past = |margin: f32| {
        bounds
            .iter()
            .filter(|&&bound| distance >= bound + margin)
            .count()
    };
    let coarser = past(HYSTERESIS);
    let finer = past(-HYSTERESIS);
    current.clamp(coarser, finer)
}

/// `mesh` (a list of triangles with positions, and colours in linear RGBA
/// if it has them) as at most `target` triangles, or as few as it can
/// without turning faces over. It keeps the colours, averaged, and its
/// normals, or works them out; and it stays within the box round it.
pub fn simplify(mesh: &Mesh, target: usize) -> Mesh {
    let source = Source::of(mesh);
    if source.indices.len() / 3 <= target {
        return mesh.clone();
    }
    let mut work = Work::new(&source);
    work.collapse_to(target);
    work.mesh()
}

/// `mesh`'s texture `image` (8-bit RGBA in sRGB, with its mipmaps; see
/// [`super::generate_mipmaps`]) painted in its vertices instead, in linear
/// RGBA: each vertex the average of the triangles round it, each triangle
/// the average of its texture at its middle and halfway to its corners, read
/// at mip `level`, as it is seen from afar. The texture coordinates go.
pub fn paint_from_texture(mesh: &Mesh, image: &Image, level: u32) -> Mesh {
    let source = Source::of(mesh);
    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(uvs)) => uvs.clone(),
        _ => vec![[0.0; 2]; source.positions.len()],
    };
    let texture = Texture::new(image, level);
    let mut sums = vec![(Vec4::ZERO, 0.0f32); source.positions.len()];
    for triangle in source.indices.as_chunks::<3>().0 {
        let corners = triangle.iter().map(|&i| Vec2::from(uvs[i as usize]));
        let corners: Vec<Vec2> = corners.collect();
        let middle = (corners[0] + corners[1] + corners[2]) / 3.0;
        let color = std::iter::once(middle)
            .chain(corners.iter().map(|&corner| middle.lerp(corner, 0.5)))
            .map(|uv| texture.at(uv))
            .sum::<Vec4>()
            / 4.0;
        for &i in triangle {
            sums[i as usize].0 += color;
            sums[i as usize].1 += 1.0;
        }
    }
    let colors: Vec<[f32; 4]> = sums
        .iter()
        .map(|&(sum, count)| (sum / count.max(1.0)).to_array())
        .collect();
    let mut painted = mesh.clone();
    painted.remove_attribute(Mesh::ATTRIBUTE_UV_0);
    painted.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
    painted.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    painted
}

/// A mip level of an 8-bit RGBA texture in sRGB, read nearest.
struct Texture<'a> {
    data: &'a [u8],
    width: u32,
    height: u32,
}

impl<'a> Texture<'a> {
    fn new(image: &'a Image, level: u32) -> Self {
        let data = image
            .data
            .as_deref()
            .expect("the texture has its pixels on the CPU");
        let level = level.min(image.texture_descriptor.mip_level_count.saturating_sub(1));
        let (mut width, mut height) = (image.width(), image.height());
        let mut start = 0;
        for _ in 0..level {
            start += (width * height * 4) as usize;
            (width, height) = ((width / 2).max(1), (height / 2).max(1));
        }
        Self {
            data: &data[start..],
            width,
            height,
        }
    }

    /// The colour at `uv` (linear RGBA).
    fn at(&self, uv: Vec2) -> Vec4 {
        let x = ((uv.x.rem_euclid(1.0) * self.width as f32) as u32).min(self.width - 1);
        let y = ((uv.y.rem_euclid(1.0) * self.height as f32) as u32).min(self.height - 1);
        let i = ((y * self.width + x) * 4) as usize;
        let [r, g, b, a] = [0, 1, 2, 3].map(|k| self.data[i + k]);
        Vec4::from_array(Color::srgba_u8(r, g, b, a).to_linear().to_f32_array())
    }
}

/// What [`simplify`] reads from a mesh.
struct Source {
    positions: Vec<Vec3>,
    normals: Option<Vec<Vec3>>,
    colors: Vec<Vec4>,
    indices: Vec<u32>,
}

impl Source {
    fn of(mesh: &Mesh) -> Self {
        assert_eq!(mesh.primitive_topology(), PrimitiveTopology::TriangleList);
        let positions: Vec<Vec3> = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(VertexAttributeValues::as_float3)
            .expect("a mesh to make lighter has positions")
            .iter()
            .map(|&p| Vec3::from(p))
            .collect();
        let normals = mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(VertexAttributeValues::as_float3)
            .map(|normals| normals.iter().map(|&n| Vec3::from(n)).collect());
        let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(colors)) => {
                colors.iter().map(|&c| Vec4::from(c)).collect()
            }
            _ => vec![Vec4::ONE; positions.len()],
        };
        let indices = match mesh.indices() {
            Some(indices) => indices.iter().map(|i| i as u32).collect(),
            None => (0..positions.len() as u32).collect(),
        };
        Self {
            positions,
            normals,
            colors,
            indices,
        }
    }

    fn bounds(&self) -> (Vec3, Vec3) {
        self.positions.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(min, max), &p| (min.min(p), max.max(p)),
        )
    }
}

/// The squared distances to some planes, weighted: xᵀAx + 2bᵀx + c.
#[derive(Clone, Copy)]
struct Quadric {
    a: Mat3,
    b: Vec3,
    c: f32,
}

impl Quadric {
    const ZERO: Self = Self {
        a: Mat3::ZERO,
        b: Vec3::ZERO,
        c: 0.0,
    };

    /// The plane through `at` facing `normal` (of length one), weighing
    /// `weight`.
    fn plane(normal: Vec3, at: Vec3, weight: f32) -> Self {
        let d = -normal.dot(at);
        Self {
            a: Mat3::from_cols(normal * normal.x, normal * normal.y, normal * normal.z) * weight,
            b: normal * (d * weight),
            c: d * d * weight,
        }
    }

    fn error(&self, x: Vec3) -> f32 {
        x.dot(self.a * x) + 2.0 * self.b.dot(x) + self.c
    }

    /// Where it is least, if that is settled (not along a flat face or a
    /// straight edge).
    fn least(&self) -> Option<Vec3> {
        let scale = self.a.x_axis.x + self.a.y_axis.y + self.a.z_axis.z;
        let determinant = self.a.determinant();
        (scale > 0.0 && determinant.abs() > 1e-6 * scale * scale * scale)
            .then(|| self.a.inverse() * -self.b)
            .filter(|x| x.is_finite())
    }
}

impl std::ops::Add for Quadric {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            a: self.a + other.a,
            b: self.b + other.b,
            c: self.c + other.c,
        }
    }
}

/// An edge to collapse, and what it would cost (see [`Work::cost`]), with the
/// versions of its ends it was worked out for: once either has changed, it
/// is out of date.
struct Candidate {
    cost: f32,
    to: Vec3,
    ends: [usize; 2],
    versions: [u32; 2],
}

impl PartialEq for Candidate {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Candidate {}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Candidate {
    /// The cheapest first out of a [`BinaryHeap`].
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.ends.cmp(&self.ends))
    }
}

/// A mesh being made lighter: its vertices (those at the same spot welded
/// into one, so that it does not tear along its seams), what each one
/// gathers, its faces and which are left.
struct Work {
    positions: Vec<Vec3>,
    /// The colours and normals of the vertices each one gathers, summed, and
    /// how many.
    colors: Vec<Vec4>,
    normals: Vec<Vec3>,
    gathered: Vec<f32>,
    quadrics: Vec<Quadric>,
    versions: Vec<u32>,
    faces: Vec<[usize; 3]>,
    alive: Vec<bool>,
    left: usize,
    /// The faces round each vertex (some may be gone).
    around: Vec<Vec<usize>>,
    /// Which part of the mesh (of faces joined by their corners) each vertex
    /// is in, and how many faces each part has left: none goes below
    /// [`LEAST_OF_A_PART`], so a part (the top tier of a fir) never goes.
    part: Vec<usize>,
    part_faces: Vec<usize>,
    /// The top of each part, which stays where it is, so that the mesh is
    /// as tall far away: a thin spike (the tip of a cypress) is cheap to
    /// shorten, as its vertices slide along its own faces.
    pinned: Vec<bool>,
    /// The box round the mesh, which the vertices stay in.
    bounds: (Vec3, Vec3),
}

impl Work {
    fn new(source: &Source) -> Self {
        // Weld the vertices at the same spot.
        let mut spots: HashMap<[u32; 3], usize> = HashMap::new();
        let mut positions = Vec::new();
        let mut colors = Vec::new();
        let mut normals = Vec::new();
        let mut gathered = Vec::new();
        let welded: Vec<usize> = source
            .positions
            .iter()
            .enumerate()
            .map(|(i, &p)| {
                let index = *spots
                    .entry(p.to_array().map(f32::to_bits))
                    .or_insert_with(|| {
                        positions.push(p);
                        colors.push(Vec4::ZERO);
                        normals.push(Vec3::ZERO);
                        gathered.push(0.0);
                        positions.len() - 1
                    });
                colors[index] += source.colors[i];
                if let Some(source_normals) = &source.normals {
                    normals[index] += source_normals[i];
                }
                gathered[index] += 1.0;
                index
            })
            .collect();
        let faces: Vec<[usize; 3]> = source
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| [0, 1, 2].map(|k| welded[t[k] as usize]))
            .filter(|&[a, b, c]| a != b && b != c && a != c)
            .collect();
        // Each vertex's quadric: the planes of the faces round it, by their
        // areas; and along an open edge, a plane across it, so the outline
        // keeps its place.
        let mut quadrics = vec![Quadric::ZERO; positions.len()];
        let mut around = vec![Vec::new(); positions.len()];
        let mut edges: HashMap<[usize; 2], (usize, usize)> = HashMap::new();
        for (f, face) in faces.iter().enumerate() {
            let [a, b, c] = face.map(|v| positions[v]);
            let cross = (b - a).cross(c - a);
            let area = cross.length();
            for (k, &v) in face.iter().enumerate() {
                around[v].push(f);
                if area > 1e-12 {
                    quadrics[v] = quadrics[v] + Quadric::plane(cross / area, a, area);
                }
                let w = face[(k + 1) % 3];
                edges.entry([v.min(w), v.max(w)]).or_insert((0, f)).0 += 1;
            }
        }
        for (&[v, w], &(count, f)) in &edges {
            let [a, b, c] = faces[f].map(|v| positions[v]);
            let normal = (b - a).cross(c - a).normalize_or_zero();
            let along = positions[w] - positions[v];
            let across = along.cross(normal).normalize_or_zero();
            if count == 1 && across != Vec3::ZERO {
                let edge = Quadric::plane(across, positions[v], 10.0 * along.length_squared());
                quadrics[v] = quadrics[v] + edge;
                quadrics[w] = quadrics[w] + edge;
            }
        }
        // The parts: the faces joined by their corners.
        let mut parent: Vec<usize> = (0..positions.len()).collect();
        fn root(parent: &mut [usize], mut v: usize) -> usize {
            while parent[v] != v {
                parent[v] = parent[parent[v]];
                v = parent[v];
            }
            v
        }
        for face in &faces {
            for &v in &face[1..] {
                let (a, b) = (root(&mut parent, v), root(&mut parent, face[0]));
                parent[a] = b;
            }
        }
        let part: Vec<usize> = (0..positions.len()).map(|v| root(&mut parent, v)).collect();
        let mut part_faces = vec![0; positions.len()];
        for face in &faces {
            part_faces[part[face[0]]] += 1;
        }
        let mut tops: HashMap<usize, usize> = HashMap::new();
        for face in &faces {
            for &v in face {
                let top = tops.entry(part[v]).or_insert(v);
                if positions[v].y > positions[*top].y {
                    *top = v;
                }
            }
        }
        let mut pinned = vec![false; positions.len()];
        for &top in tops.values() {
            pinned[top] = true;
        }
        let left = faces.len();
        Self {
            part,
            part_faces,
            pinned,
            versions: vec![0; positions.len()],
            bounds: source.bounds(),
            positions,
            colors,
            normals,
            gathered,
            quadrics,
            alive: vec![true; faces.len()],
            faces,
            left,
            around,
        }
    }

    /// What collapsing the edge between `v` and `w` costs, and where their
    /// vertex goes: the best point for both, or one of them, or between.
    fn cost(&self, v: usize, w: usize) -> Candidate {
        let quadric = self.quadrics[v] + self.quadrics[w];
        let (p, q) = (self.positions[v], self.positions[w]);
        let middle = (p + q) / 2.0;
        let (cost, to) = match (self.pinned[v], self.pinned[w]) {
            (true, true) => (f32::INFINITY, middle),
            (true, false) => (quadric.error(p), p),
            (false, true) => (quadric.error(q), q),
            (false, false) => quadric
                .least()
                .filter(|x| x.distance(middle) <= 2.0 * p.distance(q))
                .map(|x| x.clamp(self.bounds.0, self.bounds.1))
                .into_iter()
                .chain([p, q, middle])
                .map(|x| (quadric.error(x), x))
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap_or((0.0, middle)),
        };
        Candidate {
            cost,
            to,
            ends: [v, w],
            versions: [self.versions[v], self.versions[w]],
        }
    }

    /// The vertices joined to `v` by an edge.
    fn neighbours(&self, v: usize) -> Vec<usize> {
        let mut neighbours: Vec<usize> = self.around[v]
            .iter()
            .filter(|&&f| self.alive[f])
            .flat_map(|&f| self.faces[f])
            .filter(|&w| w != v)
            .collect();
        neighbours.sort_unstable();
        neighbours.dedup();
        neighbours
    }

    /// Whether moving `v` and `w` to `to` would turn a face round them over
    /// (or flatten it to nothing).
    fn folds(&self, v: usize, w: usize, to: Vec3) -> bool {
        self.around[v].iter().chain(&self.around[w]).any(|&f| {
            let face = self.faces[f];
            if !self.alive[f] || (face.contains(&v) && face.contains(&w)) {
                return false;
            }
            let corners = face.map(|u| self.positions[u]);
            let moved = face.map(|u| {
                if u == v || u == w {
                    to
                } else {
                    self.positions[u]
                }
            });
            let normal = |[a, b, c]: [Vec3; 3]| (b - a).cross(c - a);
            let (before, after) = (normal(corners), normal(moved));
            after.length_squared() <= 1e-14 || before.dot(after) <= 0.0
        })
    }

    /// Whether collapsing the edge between `v` and `w` would leave their
    /// part with fewer than [`LEAST_OF_A_PART`] faces.
    fn thins_out(&self, v: usize, w: usize) -> bool {
        let gone = self.around[v]
            .iter()
            .filter(|&&f| self.alive[f] && self.faces[f].contains(&w))
            .count();
        self.part_faces[self.part[v]] < LEAST_OF_A_PART + gone
    }

    fn collapse_to(&mut self, target: usize) {
        let mut queue = BinaryHeap::new();
        for v in 0..self.positions.len() {
            for w in self.neighbours(v) {
                if v < w {
                    queue.push(self.cost(v, w));
                }
            }
        }
        while self.left > target {
            let Some(candidate) = queue.pop() else {
                break;
            };
            let [v, w] = candidate.ends;
            if candidate.versions != [self.versions[v], self.versions[w]]
                || candidate.cost.is_infinite()
                || self.folds(v, w, candidate.to)
                || self.thins_out(v, w)
            {
                continue;
            }
            self.join(v, w, candidate.to);
            for u in self.neighbours(v) {
                queue.push(self.cost(v, u));
            }
        }
    }

    /// Joins `w` into `v`, at `to`.
    fn join(&mut self, v: usize, w: usize, to: Vec3) {
        self.positions[v] = to;
        self.pinned[v] |= self.pinned[w];
        self.quadrics[v] = self.quadrics[v] + self.quadrics[w];
        let (color, normal, gathered) = (self.colors[w], self.normals[w], self.gathered[w]);
        self.colors[v] += color;
        self.normals[v] += normal;
        self.gathered[v] += gathered;
        self.versions[v] += 1;
        self.versions[w] += 1;
        let moved = std::mem::take(&mut self.around[w]);
        for f in moved {
            if !self.alive[f] {
                continue;
            }
            let face = self.faces[f].map(|u| if u == w { v } else { u });
            self.faces[f] = face;
            let [a, b, c] = face;
            // Gone if it had both, or if it is now the same as another
            // (facing the same way).
            let twin = self.around[v]
                .iter()
                .any(|&g| g != f && self.alive[g] && same_face(self.faces[g], face));
            if a == b || b == c || a == c || twin {
                self.alive[f] = false;
                self.left -= 1;
                self.part_faces[self.part[v]] -= 1;
            } else {
                self.around[v].push(f);
            }
        }
        let alive = &self.alive;
        self.around[v].retain(|&f| alive[f]);
        self.around[v].sort_unstable();
        self.around[v].dedup();
    }

    /// The mesh left: its vertices with their average colour, and their
    /// normal (that of the vertices gathered, or else of the faces round
    /// it, or else pointing out from the middle).
    fn mesh(self) -> Mesh {
        let mut index = vec![u32::MAX; self.positions.len()];
        let mut kept = Vec::new();
        let mut indices = Vec::new();
        for (face, _) in self
            .faces
            .iter()
            .zip(&self.alive)
            .filter(|(_, alive)| **alive)
        {
            for &v in face {
                if index[v] == u32::MAX {
                    index[v] = kept.len() as u32;
                    kept.push(v);
                }
                indices.push(index[v]);
            }
        }
        let positions: Vec<Vec3> = kept.iter().map(|&v| self.positions[v]).collect();
        let mut faces = vec![Vec3::ZERO; kept.len()];
        for triangle in indices.as_chunks::<3>().0 {
            let [a, b, c] = [0, 1, 2].map(|k| positions[triangle[k] as usize]);
            let cross = (b - a).cross(c - a);
            for &i in triangle {
                faces[i as usize] += cross;
            }
        }
        let middle = positions.iter().copied().sum::<Vec3>() / positions.len().max(1) as f32;
        let normals: Vec<[f32; 3]> = kept
            .iter()
            .zip(&faces)
            .zip(&positions)
            .map(|((&v, &face), &position)| {
                self.normals[v]
                    .try_normalize()
                    .or_else(|| face.try_normalize())
                    .unwrap_or_else(|| (position - middle).normalize_or(Vec3::Y))
                    .to_array()
            })
            .collect();
        let colors: Vec<[f32; 4]> = kept
            .iter()
            .map(|&v| (self.colors[v] / self.gathered[v].max(1.0)).to_array())
            .collect();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            positions.iter().map(|p| p.to_array()).collect::<Vec<_>>(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
    }
}

/// The fewest faces a part of a mesh is left with: a triangle seen from
/// either side, so that a clump of leaves far away thins out but stays.
const LEAST_OF_A_PART: usize = 2;

/// Whether two faces are the same, facing the same way.
fn same_face(a: [usize; 3], b: [usize; 3]) -> bool {
    (0..3).any(|turn| [0, 1, 2].map(|k| a[(k + turn) % 3]) == b)
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    fn triangles(mesh: &Mesh) -> usize {
        mesh.indices().map_or(0, |indices| indices.len() / 3)
    }

    fn bounds(mesh: &Mesh) -> (Vec3, Vec3) {
        Source::of(mesh).bounds()
    }

    /// A lumpy ball, red below and green above.
    fn ball() -> Mesh {
        let mut mesh = Sphere::new(2.0).mesh().ico(7).unwrap();
        let colors: Vec<[f32; 4]> = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(VertexAttributeValues::as_float3)
            .unwrap()
            .iter()
            .map(|p| {
                if p[1] > 0.0 {
                    [0.0, 1.0, 0.0, 1.0]
                } else {
                    [1.0, 0.0, 0.0, 1.0]
                }
            })
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh
    }

    #[test]
    fn a_lighter_mesh_keeps_its_shape_and_colours_with_fewer_triangles() {
        let full = ball();
        assert_eq!(triangles(&full), 1280);
        for target in [300, 120, 40] {
            let light = simplify(&full, target);
            let count = triangles(&light);
            assert!(count <= target && count >= target / 4, "{target}: {count}");
            // As big as it was: the vertices kept round the outside, not
            // pulled in to the middles of their cells (coarse, a little
            // outside, so the faces between them are not inside).
            let (min, max) = bounds(&light);
            let (full_min, full_max) = bounds(&full);
            assert!(min.cmpge(full_min - 1e-4).all() && max.cmple(full_max + 1e-4).all());
            let radius = Source::of(&light)
                .positions
                .iter()
                .map(|p| p.length())
                .sum::<f32>()
                / Source::of(&light).positions.len() as f32;
            assert!(radius > 1.9 && radius < 2.2, "{target}: {radius}");
            // Green above and red below, still.
            let source = Source::of(&light);
            for (p, c) in source.positions.iter().zip(&source.colors) {
                if p.y > 1.0 {
                    assert!(c.y > 0.9 && c.x < 0.1, "{p}: {c}");
                }
                if p.y < -1.0 {
                    assert!(c.x > 0.9 && c.y < 0.1, "{p}: {c}");
                }
            }
            // Normals out, of length one.
            for (p, n) in source
                .positions
                .iter()
                .zip(source.normals.as_ref().unwrap())
            {
                assert!((n.length() - 1.0).abs() < 1e-3);
                assert!(n.dot(p.normalize()) > 0.7, "{p}: {n}");
            }
        }
    }

    #[test]
    fn a_mesh_already_light_enough_stays_as_it_is() {
        let full = ball();
        assert_eq!(triangles(&simplify(&full, 5000)), 1280);
    }

    #[test]
    fn the_two_sides_of_a_thin_sheet_are_both_kept() {
        // A square drawn from both sides, of many small triangles.
        let mut mesh = Plane3d::default()
            .mesh()
            .size(4.0, 4.0)
            .subdivisions(7)
            .build();
        let front: Vec<u32> = mesh.indices().unwrap().iter().map(|i| i as u32).collect();
        let back: Vec<u32> = front
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|t| [t[0], t[2], t[1]])
            .collect();
        mesh.insert_indices(Indices::U32([front, back].concat()));
        let light = simplify(&mesh, 40);
        let source = Source::of(&light);
        let facing = |up: bool| {
            source.indices.as_chunks::<3>().0.iter().any(|t| {
                let [a, b, c] = [0, 1, 2].map(|k| source.positions[t[k] as usize]);
                ((b - a).cross(c - a).y > 0.0) == up
            })
        };
        assert!(triangles(&light) <= 40);
        assert!(facing(true) && facing(false));
    }

    #[test]
    fn a_texture_is_painted_in_the_vertices_as_seen_from_afar() {
        // A texture red on its left half and blue on its right, with
        // mipmaps.
        let size = 16;
        let pixels: Vec<u8> = (0..size * size)
            .flat_map(|i| {
                if i % size < size / 2 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 255]
                }
            })
            .collect();
        let mut image = Image::new(
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        super::super::generate_mipmaps(&mut image);
        // Two triangles, one on each half.
        let mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 6])
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_UV_0,
            vec![
                [0.05f32, 0.1],
                [0.4, 0.1],
                [0.2, 0.9],
                [0.6, 0.1],
                [0.95, 0.1],
                [0.8, 0.9],
            ],
        )
        .with_inserted_indices(Indices::U32(vec![0, 1, 2, 3, 4, 5]));
        let painted = paint_from_texture(&mesh, &image, 1);
        assert!(painted.attribute(Mesh::ATTRIBUTE_UV_0).is_none());
        let colors = match painted.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(colors)) => colors.clone(),
            _ => panic!("painted"),
        };
        for (i, color) in colors.iter().enumerate() {
            let red = i < 3;
            assert_eq!(color[0] > 0.9, red, "{i}: {color:?}");
            assert_eq!(color[2] > 0.9, !red, "{i}: {color:?}");
        }
    }

    #[test]
    fn the_level_changes_only_past_each_bound() {
        let bounds = [60.0, 200.0];
        assert_eq!(level_at(10.0, 0, &bounds), 0);
        // Just past the bound: as it was, either way.
        assert_eq!(level_at(62.0, 0, &bounds), 0);
        assert_eq!(level_at(58.0, 1, &bounds), 1);
        // Well past it.
        assert_eq!(level_at(65.0, 0, &bounds), 1);
        assert_eq!(level_at(55.0, 1, &bounds), 0);
        // Far, from anywhere; and back near, from far.
        assert_eq!(level_at(500.0, 0, &bounds), 2);
        assert_eq!(level_at(5.0, 2, &bounds), 0);
        assert_eq!(level_at(150.0, 2, &bounds), 1);
    }
}
