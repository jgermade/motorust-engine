//! Rótulos con letras de píxeles gordos, como los de los juegos de los 90:
//! para los arcos de meta, las pancartas y los carteles.

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use super::textures::generate_mipmaps;

/// Letras de 5 × 7 píxeles: una fila por byte, el bit 4 a la izquierda.
pub(super) fn glyph(letter: char) -> [u8; 7] {
    match letter.to_ascii_uppercase() {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        '!' => [4, 4, 4, 4, 4, 0, 4],
        '.' => [0, 0, 0, 0, 0, 0, 4],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '<' => [3, 6, 12, 24, 12, 6, 3],
        '>' => [24, 12, 6, 3, 6, 12, 24],
        _ => [0; 7],
    }
}

/// Píxeles de imagen por cada píxel de letra.
const SCALE: u32 = 8;
/// Margen alrededor del texto, en píxeles de letra.
const PADDING: u32 = 2;

/// Un rótulo: `text` sobre `paper`, centrado en una imagen que mide como
/// mínimo `aspect` veces más de ancho que de alto, con un borde de `border`
/// si hay. Las letras van en `ink` y, desde la `accent.0`, en `accent.1`.
///
/// Each sign is drawn once and kept: the threads that prepare the circuits
/// draw theirs ahead (see [`super::prepare`]), so building a circuit on the
/// main thread only copies them, and the circuits share their checkpoints.
pub fn sign_image(
    text: &str,
    ink: Color,
    accent: Option<(usize, Color)>,
    paper: Color,
    border: Option<Color>,
    aspect: f32,
) -> Image {
    static DRAWN: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, Image>>> =
        std::sync::LazyLock::new(Default::default);
    let key = format!("{text}|{ink:?}|{accent:?}|{paper:?}|{border:?}|{aspect}");
    if let Some(image) = DRAWN.lock().ok().and_then(|drawn| drawn.get(&key).cloned()) {
        return image;
    }
    let image = draw_sign(text, ink, accent, paper, border, aspect);
    if let Ok(mut drawn) = DRAWN.lock() {
        drawn.insert(key, image.clone());
    }
    image
}

/// Draws a sign (see [`sign_image`]).
fn draw_sign(
    text: &str,
    ink: Color,
    accent: Option<(usize, Color)>,
    paper: Color,
    border: Option<Color>,
    aspect: f32,
) -> Image {
    let letters = text.chars().count() as u32;
    let height = 7 + 2 * PADDING;
    let text_width = letters * 6 - 1;
    let width = (text_width + 2 * PADDING).max((aspect * height as f32).ceil() as u32);
    let left = (width - text_width) / 2;
    let to_bytes = |color: Color| {
        let srgba = color.to_srgba();
        [srgba.red, srgba.green, srgba.blue, srgba.alpha].map(|c| (c * 255.0).round() as u8)
    };
    let (ink, paper) = (to_bytes(ink), to_bytes(paper));
    let accent = accent.map(|(from, color)| (from as u32, to_bytes(color)));
    let border = border.map(to_bytes);
    // El rótulo en píxeles de letra.
    let small = |x: u32, y: u32| -> [u8; 4] {
        if let Some(border) = border
            && (x == 0 || y == 0 || x == width - 1 || y == height - 1)
        {
            return border;
        }
        let (Some(column), Some(row)) = (x.checked_sub(left), y.checked_sub(PADDING)) else {
            return paper;
        };
        let (letter, column) = (column / 6, column % 6);
        if row >= 7 || column >= 5 || letter >= letters {
            return paper;
        }
        let bits = glyph(text.chars().nth(letter as usize).unwrap_or(' '))[row as usize];
        if bits & (1 << (4 - column)) != 0 {
            match accent {
                Some((from, color)) if letter >= from => color,
                _ => ink,
            }
        } else {
            paper
        }
    };
    let (big_width, big_height) = (width * SCALE, height * SCALE);
    let data = (0..big_width * big_height)
        .flat_map(|i| small(i % big_width / SCALE, i / big_width / SCALE))
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: big_width,
            height: big_height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    generate_mipmaps(&mut image);
    image
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn letters_are_drawn_in_ink_on_paper() {
        let image = sign_image("I", Color::BLACK, None, Color::WHITE, None, 0.0);
        let width = image.width();
        let data = image.data.as_ref().unwrap();
        let pixel = |x: u32, y: u32| data[((y * width + x) * 4) as usize];
        // La I: el centro de su palo, en tinta; la esquina, papel.
        let middle = (PADDING + 2) * SCALE + SCALE / 2;
        assert_eq!(pixel(middle, (PADDING + 3) * SCALE), 0);
        assert_eq!(pixel(1, 1), 255);
        // Con proporción, se ensancha.
        let wide = sign_image("I", Color::BLACK, None, Color::WHITE, None, 6.0);
        assert!(wide.width() >= 6 * wide.height());
        // Desde la segunda letra, en otro color.
        let two = sign_image(
            "II",
            Color::BLACK,
            Some((1, Color::WHITE)),
            Color::BLACK,
            None,
            0.0,
        );
        let width = two.width();
        let data = two.data.as_ref().unwrap();
        let pixel = |x: u32, y: u32| data[((y * width + x) * 4) as usize];
        let row = (PADDING + 3) * SCALE;
        assert_eq!(pixel(middle, row), 0);
        assert_eq!(pixel(middle + 6 * SCALE, row), 255);
    }
}
