//! Constructor de mallas a base de cuadriláteros y triángulos planos, que se
//! orientan solos hacia donde se les pide.

use bevy::{
    asset::RenderAssetUsages, mesh::Indices, prelude::*, render::render_resource::PrimitiveTopology,
};

/// Acumula cuadriláteros con normal, coordenadas de textura y color.
#[derive(Default)]
pub struct MeshBuilder {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshBuilder {
    /// Añade un cuadrilátero dado por sus esquinas en orden (en un sentido o
    /// en otro): se orienta solo para que su cara visible mire hacia `facing`.
    pub fn quad(&mut self, corners: [(Vec3, Vec2); 4], facing: Vec3, color: Color) {
        let [a, b, c, _] = corners.map(|(p, _)| p);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        let corners = if normal.dot(facing) >= 0.0 {
            corners
        } else {
            [corners[0], corners[3], corners[2], corners[1]]
        };
        let normal = if normal.dot(facing) >= 0.0 {
            normal
        } else {
            -normal
        };
        let base = self.positions.len() as u32;
        for (position, uv) in corners {
            self.positions.push(position.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv.to_array());
            self.colors.push(color.to_linear().to_f32_array());
        }
        self.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Añade un triángulo, orientado igual que [`Self::quad`].
    pub fn triangle(&mut self, corners: [(Vec3, Vec2); 3], facing: Vec3, color: Color) {
        let [a, b, c] = corners.map(|(p, _)| p);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        let (corners, normal) = if normal.dot(facing) >= 0.0 {
            (corners, normal)
        } else {
            ([corners[0], corners[2], corners[1]], -normal)
        };
        let base = self.positions.len() as u32;
        for (position, uv) in corners {
            self.positions.push(position.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv.to_array());
            self.colors.push(color.to_linear().to_f32_array());
        }
        self.indices.extend([base, base + 1, base + 2]);
    }

    /// Adds a triangle like [`Self::triangle`], with a colour at each
    /// corner, blended across it.
    pub fn shaded_triangle(&mut self, corners: [(Vec3, Vec2, Color); 3], facing: Vec3) {
        let [a, b, c] = corners.map(|(p, ..)| p);
        let normal = (b - a).cross(c - a).normalize_or_zero();
        let (corners, normal) = if normal.dot(facing) >= 0.0 {
            (corners, normal)
        } else {
            ([corners[0], corners[2], corners[1]], -normal)
        };
        let base = self.positions.len() as u32;
        for (position, uv, color) in corners {
            self.positions.push(position.to_array());
            self.normals.push(normal.to_array());
            self.uvs.push(uv.to_array());
            self.colors.push(color.to_linear().to_f32_array());
        }
        self.indices.extend([base, base + 1, base + 2]);
    }

    /// Añade una viga: un prisma de sección rectangular de `a` a `b`, de
    /// `width` de ancho y `height` de alto (de canto, hacia arriba si la viga
    /// no es vertical), con sus dos extremos cerrados.
    pub fn beam(&mut self, a: Vec3, b: Vec3, width: f32, height: f32, color: Color) {
        let along = (b - a).normalize_or_zero();
        let side = if along.y.abs() > 0.99 {
            Vec3::X
        } else {
            along.cross(Vec3::Y).normalize()
        };
        let up = side.cross(along);
        let (w, h) = (side * width / 2.0, up * height / 2.0);
        let corner = |end: Vec3, s: f32, u: f32| end + w * s + h * u;
        let length = a.distance(b);
        for (facing, s0, u0, s1, u1) in [
            (side, 1.0, -1.0, 1.0, 1.0),
            (-side, -1.0, -1.0, -1.0, 1.0),
            (up, -1.0, 1.0, 1.0, 1.0),
            (-up, -1.0, -1.0, 1.0, -1.0),
        ] {
            self.quad(
                [
                    (corner(a, s0, u0), Vec2::ZERO),
                    (corner(b, s0, u0), Vec2::new(length, 0.0)),
                    (corner(b, s1, u1), Vec2::new(length, 1.0)),
                    (corner(a, s1, u1), Vec2::new(0.0, 1.0)),
                ],
                facing,
                color,
            );
        }
        for (end, facing) in [(a, -along), (b, along)] {
            self.quad(
                [
                    (corner(end, -1.0, -1.0), Vec2::ZERO),
                    (corner(end, 1.0, -1.0), Vec2::X),
                    (corner(end, 1.0, 1.0), Vec2::ONE),
                    (corner(end, -1.0, 1.0), Vec2::Y),
                ],
                facing,
                color,
            );
        }
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn build(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn beams_are_closed_boxes_facing_out() {
        let mut builder = MeshBuilder::default();
        builder.beam(
            Vec3::ZERO,
            Vec3::new(0.0, 0.0, -4.0),
            1.0,
            2.0,
            Color::WHITE,
        );
        // Seis caras de cuatro esquinas.
        assert_eq!(builder.positions.len(), 24);
        let center = Vec3::new(0.0, 0.0, -2.0);
        for face in 0..6 {
            let corner = Vec3::from(builder.positions[face * 4]);
            let normal = Vec3::from(builder.normals[face * 4]);
            assert!(
                (corner - center).dot(normal) > 0.0,
                "la cara {face} mira hacia dentro"
            );
        }
    }

    #[test]
    fn quads_face_where_asked_whatever_the_corner_order() {
        let corners = [
            (Vec3::ZERO, Vec2::ZERO),
            (Vec3::X, Vec2::X),
            (Vec3::new(1.0, 0.0, 1.0), Vec2::ONE),
            (Vec3::Z, Vec2::Y),
        ];
        for facing in [Vec3::Y, Vec3::NEG_Y] {
            let mut builder = MeshBuilder::default();
            builder.quad(corners, facing, Color::WHITE);
            let p = |i: u32| Vec3::from(builder.positions[builder.indices[i as usize] as usize]);
            let normal = (p(1) - p(0)).cross(p(2) - p(0));
            assert!(normal.dot(facing) > 0.0);
            assert!(Vec3::from(builder.normals[0]).dot(facing) > 0.0);
        }
    }
}
