//! Mallas procedurales para la moto y el piloto.
//!
//! Casi todas las piezas son *lofts*: una sucesión de secciones transversales
//! a lo largo del eje Z. Hay dos clases:
//!
//! - Redondeados ([`loft`], [`tube`]): entre cada par de secciones se
//!   intercalan otras interpoladas y el sombreado es suave.
//! - Facetados ([`faceted`], [`faceted_tube`]): la sección es un polígono, las
//!   secciones se unen con caras planas y cada cara tiene su propio sombreado,
//!   como los modelos *low poly*. Las aristas vivas dibujan la forma.
//!
//! El cuerpo del piloto es un [`SkinnedBody`]: tubos que siguen un camino
//! curvo, con colores en los vértices y pesos de huesos para deformarse sin
//! costuras en las articulaciones.

use std::f32::consts::TAU;

use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, VertexAttributeValues},
    prelude::*,
    render::render_resource::PrimitiveTopology,
};

/// Puntos del contorno de cada sección.
const PROFILE_POINTS: usize = 20;
/// Secciones interpoladas entre cada par de secciones dadas (incluida la primera).
const SUBDIVISIONS: usize = 4;
/// Forma del contorno de las piezas de carrocería: 2 es una elipse y, cuanto
/// más alto, más se parece a un rectángulo de esquinas redondeadas.
const PANEL_SQUARENESS: f32 = 2.6;
const ROUND: f32 = 2.0;

/// Sección transversal de un loft: un contorno en el plano XY a la altura `z`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub z: f32,
    pub y: f32,
    pub half_width: f32,
    pub half_height: f32,
    /// Cuánto se retrasa (+Z) el contorno por cada metro por encima de `y`:
    /// con un valor distinto de cero la sección va en diagonal, como el borde
    /// trasero de un carenado.
    pub slant: f32,
}

/// Atajo para escribir secciones de forma compacta: `(z, y, semiancho, semialto)`.
pub const fn section(z: f32, y: f32, half_width: f32, half_height: f32) -> Section {
    Section {
        z,
        y,
        half_width,
        half_height,
        slant: 0.0,
    }
}

impl Section {
    /// La misma sección en diagonal (ver [`Section::slant`]).
    pub const fn slanted(self, slant: f32) -> Section {
        Section { slant, ..self }
    }

    fn point(&self, p: Vec2) -> [f32; 3] {
        let up = p.y * self.half_height;
        [p.x * self.half_width, self.y + up, self.z + self.slant * up]
    }
}

/// Une las secciones (ordenadas por `z` creciente) en un sólido cerrado de
/// esquinas redondeadas, como un panel de carenado.
pub fn loft(sections: &[Section]) -> Mesh {
    let mut mesh = build(&smooth(sections), &profile(PANEL_SQUARENESS));
    mesh.compute_smooth_normals();
    mesh
}

/// Tubo redondo a lo largo de +Z, de `length` metros, que pasa de radio
/// `start` a `end`. Se coloca entre dos puntos con [`between`].
pub fn tube(start: f32, end: f32, length: f32) -> Mesh {
    let mut mesh = build(&tube_sections(start, end, length), &profile(ROUND));
    mesh.compute_smooth_normals();
    mesh
}

fn tube_sections(start: f32, end: f32, length: f32) -> [Section; 2] {
    [
        section(0.0, 0.0, start, start),
        section(length, 0.0, end, end),
    ]
}

/// Une las secciones (ordenadas por `z` creciente) con caras planas, sin
/// interpolar: cada sección es el polígono `outline` (puntos entre -1 y 1, en
/// sentido antihorario visto desde +Z y convexo) escalado a su tamaño.
pub fn faceted(outline: &[Vec2], sections: &[Section]) -> Mesh {
    flat_shaded(build(sections, outline))
}

/// Prisma de `sides` caras a lo largo de +Z, de `length` metros, que pasa de
/// radio `start` a `end`. Se coloca entre dos puntos con [`between`].
pub fn faceted_tube(sides: usize, start: f32, end: f32, length: f32) -> Mesh {
    let outline: Vec<Vec2> = (0..sides)
        .map(|i| {
            // Media cara de giro: así el prisma apoya una cara abajo, no una arista.
            let angle = (i as f32 + 0.5) / sides as f32 * TAU - std::f32::consts::FRAC_PI_2;
            Vec2::from_angle(angle)
        })
        .collect();
    flat_shaded(build(&tube_sections(start, end, length), &outline))
}

/// La misma malla con cada triángulo sombreado plano, con sus propios vértices.
pub fn flat_shaded(mut mesh: Mesh) -> Mesh {
    mesh.duplicate_vertices();
    mesh.compute_flat_normals();
    mesh
}

/// Transform que lleva el eje +Z de una pieza de `from` a `to`.
pub fn between(from: Vec3, to: Vec3) -> Transform {
    Transform::from_translation(from)
        .with_rotation(Quat::from_rotation_arc(Vec3::Z, (to - from).normalize()))
}

/// Contorno de superelipse, en sentido antihorario visto desde +Z.
fn profile(squareness: f32) -> Vec<Vec2> {
    let exponent = 2.0 / squareness;
    (0..PROFILE_POINTS)
        .map(|i| {
            let (sin, cos) = (i as f32 / PROFILE_POINTS as f32 * TAU).sin_cos();
            Vec2::new(
                cos.signum() * cos.abs().powf(exponent),
                sin.signum() * sin.abs().powf(exponent),
            )
        })
        .collect()
}

/// Intercala secciones entre las dadas con una spline Catmull-Rom, para que
/// las piezas se curven con suavidad a lo largo.
fn smooth(sections: &[Section]) -> Vec<Section> {
    assert!(
        sections.len() >= 2,
        "un loft necesita al menos dos secciones"
    );
    let last = sections.len() - 1;
    let at = |i: isize| sections[i.clamp(0, last as isize) as usize];
    let mut smoothed = Vec::with_capacity(last * SUBDIVISIONS + 1);
    for i in 0..last as isize {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        for step in 0..SUBDIVISIONS {
            let t = step as f32 / SUBDIVISIONS as f32;
            let spline = |f: fn(&Section) -> f32| catmull_rom(f(&p0), f(&p1), f(&p2), f(&p3), t);
            smoothed.push(Section {
                // La posición a lo largo va lineal: así nunca retrocede.
                z: p1.z.lerp(p2.z, t),
                y: spline(|s| s.y),
                half_width: spline(|s| s.half_width).max(0.002),
                half_height: spline(|s| s.half_height).max(0.002),
                slant: p1.slant.lerp(p2.slant, t),
            });
        }
    }
    smoothed.push(sections[last]);
    smoothed
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t2 * t)
}

/// Malla con índices que une las secciones con el contorno `profile`, con
/// tapas en los extremos. Le faltan las normales.
fn build(sections: &[Section], profile: &[Vec2]) -> Mesh {
    let n = profile.len() as u32;
    let rings = sections.len() as u32;

    let mut positions: Vec<[f32; 3]> = sections
        .iter()
        .flat_map(|s| profile.iter().map(move |&p| s.point(p)))
        .collect();
    let mut indices = Vec::new();
    for ring in 0..rings - 1 {
        for k in 0..n {
            let a = ring * n + k;
            let b = ring * n + (k + 1) % n;
            let c = (ring + 1) * n + (k + 1) % n;
            let d = (ring + 1) * n + k;
            indices.extend([a, b, c, a, c, d]);
        }
    }

    // Tapas con sus propios vértices: así sus normales no se mezclan con las
    // de los lados y el borde queda definido.
    for (cap, outwards) in [(&sections[0], false), (&sections[sections.len() - 1], true)] {
        let center = positions.len() as u32;
        let middle = profile.iter().sum::<Vec2>() / n as f32;
        positions.push(cap.point(middle));
        positions.extend(profile.iter().map(|&p| cap.point(p)));
        for k in 0..n {
            let (a, b) = (center + 1 + k, center + 1 + (k + 1) % n);
            if outwards {
                indices.extend([center, a, b]);
            } else {
                indices.extend([center, b, a]);
            }
        }
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_indices(Indices::U32(indices))
}

/// Huesos que mueven un punto de un barrido, con su peso (hasta dos).
pub type Bones = [(u16, f32); 2];

/// Un solo hueso, con todo el peso.
pub const fn bone(index: u16) -> Bones {
    [(index, 1.0), (index, 0.0)]
}

/// Mezcla de dos huesos; `weight` es lo que pesa el segundo.
pub const fn bones(first: u16, second: u16, weight: f32) -> Bones {
    [(first, 1.0 - weight), (second, weight)]
}

/// Punto del camino de un barrido.
#[derive(Clone, Copy, Debug)]
pub struct SweepPoint {
    pub center: Vec3,
    /// Semiejes de la sección: a lo ancho y a lo alto.
    pub radius: Vec2,
    pub color: LinearRgba,
    pub bones: Bones,
}

/// Una sección ya interpolada de un barrido.
struct Ring {
    center: Vec3,
    radius: Vec2,
    color: LinearRgba,
    joints: [u16; 4],
    weights: [f32; 4],
}

/// Malla deformable por huesos hecha de barridos: tubos que siguen un camino
/// curvo, con color y huesos por vértice.
#[derive(Default)]
pub struct SkinnedBody {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl SkinnedBody {
    /// Añade un tubo cerrado que pasa por `points`, con el semieje alto de
    /// cada sección orientado hacia `up`. Dos puntos seguidos en el mismo
    /// sitio hacen un corte nítido de color.
    pub fn sweep(&mut self, points: &[SweepPoint], up: Vec3) {
        let rings = subdivide(points);
        let frames = frames(&rings, up);
        let n = PROFILE_POINTS as u32;
        let first = self.positions.len() as u32;
        for (ring, frame) in rings.iter().zip(&frames) {
            self.push_ring(ring, *frame);
        }
        for r in 0..rings.len() as u32 - 1 {
            for k in 0..n {
                let a = first + r * n + k;
                let b = first + r * n + (k + 1) % n;
                let c = first + (r + 1) * n + (k + 1) % n;
                let d = first + (r + 1) * n + k;
                self.indices.extend([a, b, c, a, c, d]);
            }
        }

        // Tapas con sus propios vértices, para que no alteren el sombreado
        // de los lados: un extremo abierto dejaría ver el interior.
        let ends = [(0, false), (rings.len() - 1, true)];
        for (index, outwards) in ends {
            let (ring, frame) = (&rings[index], frames[index]);
            let center = self.positions.len() as u32;
            self.push_vertex(ring.center, ring);
            self.push_ring(ring, frame);
            for k in 0..n {
                let (a, b) = (center + 1 + k, center + 1 + (k + 1) % n);
                if outwards {
                    self.indices.extend([center, a, b]);
                } else {
                    self.indices.extend([center, b, a]);
                }
            }
        }
    }

    fn push_ring(&mut self, ring: &Ring, (side, up): (Vec3, Vec3)) {
        for p in profile(ROUND) {
            let offset = side * (p.x * ring.radius.x) + up * (p.y * ring.radius.y);
            self.push_vertex(ring.center + offset, ring);
        }
    }

    fn push_vertex(&mut self, position: Vec3, ring: &Ring) {
        self.positions.push(position.to_array());
        self.colors.push(ring.color.to_f32_array());
        self.joints.push(ring.joints);
        self.weights.push(ring.weights);
    }

    pub fn build(self) -> Mesh {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(self.joints),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, self.weights)
        .with_inserted_indices(Indices::U32(self.indices));
        mesh.compute_smooth_normals();
        mesh.with_generated_skinned_mesh_bounds()
            .expect("la malla tiene huesos y posiciones")
    }
}

/// Intercala secciones a lo largo del camino (Catmull-Rom) para que las
/// curvas, como un codo, queden redondeadas.
fn subdivide(points: &[SweepPoint]) -> Vec<Ring> {
    assert!(points.len() >= 2, "un barrido necesita al menos dos puntos");
    let last = points.len() - 1;
    let at = |i: isize| points[i.clamp(0, last as isize) as usize];
    let ring = |a: &SweepPoint, b: &SweepPoint, t: f32, center: Vec3| {
        let (joints, weights) = blend(&a.bones, &b.bones, t);
        Ring {
            center,
            radius: a.radius.lerp(b.radius, t),
            color: a.color.mix(&b.color, t),
            joints,
            weights,
        }
    };

    let mut rings = vec![ring(&points[0], &points[0], 0.0, points[0].center)];
    for i in 0..last {
        let (p1, p2) = (points[i], points[i + 1]);
        if p1.center.distance(p2.center) < 1e-5 {
            rings.push(ring(&p2, &p2, 0.0, p2.center));
            continue;
        }
        let (p0, p3) = (at(i as isize - 1), at(i as isize + 2));
        for step in 1..=SUBDIVISIONS {
            let t = step as f32 / SUBDIVISIONS as f32;
            let axis = |f: fn(Vec3) -> f32| {
                catmull_rom(f(p0.center), f(p1.center), f(p2.center), f(p3.center), t)
            };
            let center = Vec3::new(axis(|v| v.x), axis(|v| v.y), axis(|v| v.z));
            rings.push(ring(&p1, &p2, t, center));
        }
    }
    rings
}

/// Pesos de huesos interpolados entre dos puntos, sumados por hueso y normalizados.
fn blend(a: &Bones, b: &Bones, t: f32) -> ([u16; 4], [f32; 4]) {
    let mut merged: Vec<(u16, f32)> = Vec::with_capacity(4);
    let scaled = a.iter().map(|&(j, w)| (j, w * (1.0 - t)));
    for (joint, weight) in scaled.chain(b.iter().map(|&(j, w)| (j, w * t))) {
        if weight <= 0.0 {
            continue;
        }
        match merged.iter_mut().find(|(j, _)| *j == joint) {
            Some(entry) => entry.1 += weight,
            None => merged.push((joint, weight)),
        }
    }
    let total: f32 = merged.iter().map(|(_, w)| w).sum();
    let (mut joints, mut weights) = ([0; 4], [0.0; 4]);
    for (slot, (joint, weight)) in merged.into_iter().enumerate() {
        joints[slot] = joint;
        weights[slot] = weight / total;
    }
    (joints, weights)
}

/// Ejes de cada sección (lateral y "arriba"), transportados a lo largo del
/// camino sin giros bruscos.
fn frames(rings: &[Ring], up: Vec3) -> Vec<(Vec3, Vec3)> {
    let distinct = |from: usize, to: usize| rings[from].center.distance(rings[to].center) > 1e-5;
    let tangent = |i: usize| {
        let previous = (0..i).rev().find(|&j| distinct(j, i)).unwrap_or(i);
        let next = (i + 1..rings.len()).find(|&j| distinct(i, j)).unwrap_or(i);
        (rings[next].center - rings[previous].center).normalize()
    };
    let mut frames = Vec::with_capacity(rings.len());
    let mut last_tangent = tangent(0);
    let mut normal = (up - last_tangent * up.dot(last_tangent)).normalize();
    for i in 0..rings.len() {
        let tangent = tangent(i);
        normal = Quat::from_rotation_arc(last_tangent, tangent) * normal;
        normal = (normal - tangent * normal.dot(tangent)).normalize();
        last_tangent = tangent;
        // (lateral, arriba, tangente) forman una base dextrógira: así las
        // caras miran hacia fuera.
        frames.push((normal.cross(tangent), normal));
    }
    frames
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    fn triangles(mesh: &Mesh) -> Vec<[Vec3; 3]> {
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .expect("posiciones");
        let indices: Vec<u32> = match mesh.indices() {
            Some(Indices::U32(indices)) => indices.clone(),
            Some(_) => panic!("índices de 16 bits"),
            None => (0..positions.len() as u32).collect(),
        };
        indices
            .chunks(3)
            .map(|t| [t[0], t[1], t[2]].map(|i| Vec3::from(positions[i as usize])))
            .collect()
    }

    fn assert_faces_point_away_from(mesh: &Mesh, center: Vec3) {
        for [a, b, c] in triangles(mesh) {
            let normal = (b - a).cross(c - a);
            if normal.length() < 1e-9 {
                continue;
            }
            let centroid = (a + b + c) / 3.0;
            assert!(
                normal.dot(centroid - center) > 0.0,
                "cara hacia dentro en {centroid}"
            );
        }
    }

    const OCTAGON: [Vec2; 8] = [
        Vec2::new(1.0, -0.4),
        Vec2::new(1.0, 0.4),
        Vec2::new(0.4, 1.0),
        Vec2::new(-0.4, 1.0),
        Vec2::new(-1.0, 0.4),
        Vec2::new(-1.0, -0.4),
        Vec2::new(-0.4, -1.0),
        Vec2::new(0.4, -1.0),
    ];

    #[test]
    fn faceted_faces_point_outwards_and_are_flat() {
        let mesh = faceted(
            &OCTAGON,
            &[
                section(-1.0, 0.5, 0.3, 0.2),
                section(0.0, 0.5, 0.5, 0.4).slanted(-0.5),
                section(1.0, 0.5, 0.3, 0.2),
            ],
        );
        assert_faces_point_away_from(&mesh, Vec3::new(0.0, 0.5, 0.0));
        // Sombreado plano: los tres vértices de cada triángulo comparten normal.
        let normals = mesh
            .attribute(Mesh::ATTRIBUTE_NORMAL)
            .and_then(|a| a.as_float3())
            .expect("normales");
        for t in normals.chunks(3) {
            assert_eq!(t[0], t[1]);
            assert_eq!(t[1], t[2]);
        }
    }

    #[test]
    fn slanted_section_leans_its_top_forward() {
        let s = section(0.0, 1.0, 0.5, 0.5).slanted(-0.4);
        assert_eq!(s.point(Vec2::new(0.0, 1.0)), [0.0, 1.5, -0.2]);
        assert_eq!(s.point(Vec2::new(0.0, -1.0)), [0.0, 0.5, 0.2]);
    }

    #[test]
    fn faceted_tube_rests_on_a_face() {
        let mesh = faceted_tube(6, 0.1, 0.1, 1.0);
        assert_faces_point_away_from(&mesh, Vec3::new(0.0, 0.0, 0.5));
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|a| a.as_float3())
            .expect("posiciones");
        let lowest = positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        let on_bottom = positions.iter().filter(|p| (p[1] - lowest).abs() < 1e-5);
        assert!(on_bottom.count() > 2, "una cara entera abajo");
    }

    #[test]
    fn loft_faces_point_outwards() {
        // Una forma convexa: todas las caras deben mirar lejos de su centro.
        let mesh = loft(&[
            section(-1.0, 0.5, 0.3, 0.2),
            section(0.0, 0.5, 0.5, 0.4),
            section(1.0, 0.5, 0.3, 0.2),
        ]);
        assert_faces_point_away_from(&mesh, Vec3::new(0.0, 0.5, 0.0));
    }

    #[test]
    fn smoothing_keeps_the_given_sections() {
        let given = [
            section(-1.0, 0.2, 0.3, 0.2),
            section(0.0, 0.6, 0.5, 0.4),
            section(1.0, 0.4, 0.1, 0.1),
        ];
        let smoothed = smooth(&given);
        assert_eq!(smoothed.len(), 2 * SUBDIVISIONS + 1);
        for (i, section) in given.iter().enumerate() {
            let s = smoothed[i * SUBDIVISIONS];
            assert!((s.y - section.y).abs() < 1e-5 && (s.z - section.z).abs() < 1e-5);
        }
        assert!(smoothed.windows(2).all(|w| w[0].z < w[1].z));
    }

    fn point(z: f32, color: LinearRgba, bones: Bones) -> SweepPoint {
        SweepPoint {
            center: Vec3::new(0.0, 0.0, z),
            radius: Vec2::splat(0.1),
            color,
            bones,
        }
    }

    #[test]
    fn sweep_faces_point_outwards_and_weights_add_up() {
        let mut body = SkinnedBody::default();
        body.sweep(
            &[
                point(0.0, LinearRgba::RED, bone(0)),
                point(0.5, LinearRgba::RED, bones(0, 1, 0.5)),
                point(1.0, LinearRgba::RED, bone(1)),
            ],
            Vec3::Y,
        );
        for w in &body.weights {
            assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        }
        // Un cilindro cerrado es convexo: toda cara mira lejos de su centro.
        let mesh = body.build();
        let center = Vec3::new(0.0, 0.0, 0.5);
        for [a, b, c] in triangles(&mesh) {
            let normal = (b - a).cross(c - a);
            assert!(normal.dot((a + b + c) / 3.0 - center) > 0.0);
        }
    }

    #[test]
    fn repeated_point_makes_a_sharp_color_change() {
        let mut body = SkinnedBody::default();
        body.sweep(
            &[
                point(0.0, LinearRgba::RED, bone(0)),
                point(0.5, LinearRgba::RED, bone(0)),
                point(0.5, LinearRgba::BLUE, bone(0)),
                point(1.0, LinearRgba::BLUE, bone(0)),
            ],
            Vec3::Y,
        );
        // Ningún vértice queda a medio camino entre los dos colores.
        assert!(
            body.colors
                .iter()
                .all(|c| *c == LinearRgba::RED.to_f32_array()
                    || *c == LinearRgba::BLUE.to_f32_array())
        );
    }

    #[test]
    fn blend_merges_shared_bones() {
        let (joints, weights) = blend(&bones(0, 1, 0.5), &bone(1), 0.5);
        assert_eq!(joints[..2], [0, 1]);
        assert!((weights[0] - 0.25).abs() < 1e-5 && (weights[1] - 0.75).abs() < 1e-5);
    }

    #[test]
    fn tube_goes_from_one_point_to_the_other() {
        let (from, to) = (Vec3::new(1.0, 2.0, 3.0), Vec3::new(-1.0, 0.5, 2.0));
        let transform = between(from, to);
        let end = transform.transform_point(Vec3::Z * from.distance(to));
        assert!(end.abs_diff_eq(to, 1e-5));
    }
}
