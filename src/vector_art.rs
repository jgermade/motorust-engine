//! Vector art of the start menu, drawn at the exact size it takes on the
//! screen, so it is sharp at any resolution:
//!
//! - The lettering of the buttons: the UI font (Russo One), slanted and a
//!   little heavier, like the lettering of the menu design. The UI font has
//!   no italic of its own, and Bevy does not slant text.
//! - The helmet: the curves of the menu design, as they come in
//!   `assets/ui/helmet.svg`. Bevy does not draw SVG; zeno, the rasterizer it
//!   already uses for the fonts, reads SVG path data.
//!
//! Each piece is drawn once per size into a white image that the menu tints.
//! The speedometer (see [`crate::speedometer`]) draws its figures with
//! [`draw_plain_text`].

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::shape::ShapeContext;
use swash::zeno::{Angle, Fill, Format, Mask, Transform};

use crate::world::generate_mipmaps;

/// The UI font's file in the content pack, the same one as
/// [`crate::hud::UiFont`].
pub const FONT_FILE: &str = "fonts/RussoOne-Regular.ttf";
/// How the letters are drawn: how much they lean forward (degrees), how
/// much heavier their strokes are and the extra space between them (as
/// fractions of the font size).
#[derive(Clone, Copy)]
struct Lettering {
    slant: f32,
    weight: f32,
    tracking: f32,
}

/// The lettering of the menu design.
const MENU: Lettering = Lettering {
    slant: 14.0,
    weight: 0.04,
    tracking: 0.07,
};
/// The font as it is.
const PLAIN: Lettering = Lettering {
    slant: 0.0,
    weight: 0.0,
    tracking: 0.0,
};

/// The helmet of the menu design (white, facing right), in the content
/// pack.
const HELMET_SVG: &str = "ui/helmet.svg";

fn font() -> FontRef<'static> {
    FontRef::from_index(crate::content::file(FONT_FILE), 0).expect("the UI font is a valid font")
}

/// Glyphs of `text` in the UI font at `size` px, with where each one starts
/// (px from the start of the line), `tracking` px apart. Without kerning:
/// with the heavier strokes, kerned letters would touch.
fn shape(text: &str, size: f32, tracking: f32) -> Vec<(u16, f32)> {
    let mut context = ShapeContext::new();
    let mut shaper = context
        .builder(font())
        .size(size)
        .features(&[("kern", 0)])
        .build();
    shaper.add_str(text);
    let mut glyphs = Vec::new();
    let mut pen = 0.0;
    shaper.shape_with(|cluster| {
        for glyph in cluster.glyphs {
            glyphs.push((glyph.id, pen + glyph.x));
            pen += glyph.advance + tracking;
        }
    });
    glyphs
}

/// How wide `text` is in the UI font at `size` px, as Bevy lays it out.
pub fn advance(text: &str, size: f32) -> f32 {
    let mut context = ShapeContext::new();
    let mut shaper = context.builder(font()).size(size).build();
    shaper.add_str(text);
    let mut width = 0.0;
    shaper.shape_with(|cluster| width += cluster.advance());
    width
}

/// Whether the UI font can draw this character.
#[cfg(any(test, feature = "testing"))]
pub fn has_char(c: char) -> bool {
    font().charmap().map(c) != 0
}

/// A white drawing: how much of each pixel it covers (0..=255), row by row.
#[derive(Debug, Clone, PartialEq)]
pub struct Coverage {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

/// `text` in slanted heavy letters, with its ink `height` px tall (the
/// height of the capitals), cropped to the ink.
pub fn draw_text(text: &str, height: u32) -> Coverage {
    // The ink is a little taller than the capitals of the font: drawn once
    // to see how much, then at the size that gives the height wanted.
    let guess = height as f32 / 0.72;
    let first = draw_text_at(text, guess, MENU);
    if first.height <= 1 {
        return first;
    }
    draw_text_at(text, guess * height as f32 / first.height as f32, MENU)
}

/// `text` in the UI font as it is, at `size` px, cropped to the ink.
pub fn draw_plain_text(text: &str, size: f32) -> Coverage {
    draw_text_at(text, size, PLAIN)
}

/// `text` with the font at `size` px, drawn as `lettering` says.
fn draw_text_at(text: &str, size: f32, lettering: Lettering) -> Coverage {
    let font = font();
    let mut context = ScaleContext::new();
    let mut scaler = context.builder(font).size(size).hint(false).build();
    let mut render = Render::new(&[Source::Outline]);
    render.format(Format::Alpha);
    if lettering.weight > 0.0 {
        render.embolden(size * lettering.weight);
    }
    if lettering.slant != 0.0 {
        render.transform(Some(Transform::skew(
            Angle::from_degrees(lettering.slant),
            Angle::ZERO,
        )));
    }

    // Each glyph's mask and where its top left corner goes (px, y down from
    // the baseline).
    let mut masks = Vec::new();
    for (id, x) in shape(text, size, size * lettering.tracking) {
        let Some(image) = render.render(&mut scaler, id) else {
            continue;
        };
        let placement = image.placement;
        if placement.width == 0 || placement.height == 0 {
            continue;
        }
        let left = x.round() as i32 + placement.left;
        let top = -placement.top;
        masks.push((left, top, placement.width, placement.height, image.data));
    }
    if masks.is_empty() {
        return Coverage {
            width: 1,
            height: 1,
            alpha: vec![0],
        };
    }

    let min_x = masks.iter().map(|m| m.0).min().unwrap_or(0);
    let min_y = masks.iter().map(|m| m.1).min().unwrap_or(0);
    let max_x = masks.iter().map(|m| m.0 + m.2 as i32).max().unwrap_or(1);
    let max_y = masks.iter().map(|m| m.1 + m.3 as i32).max().unwrap_or(1);
    let (width, height) = ((max_x - min_x) as u32, (max_y - min_y) as u32);
    let mut alpha = vec![0u8; (width * height) as usize];
    for (left, top, mask_width, mask_height, data) in masks {
        for row in 0..mask_height {
            for column in 0..mask_width {
                let value = data[(row * mask_width + column) as usize];
                let x = (left - min_x) as u32 + column;
                let y = (top - min_y) as u32 + row;
                let pixel = &mut alpha[(y * width + x) as usize];
                // Where two letters touch, the ink is the union of both.
                *pixel = (*pixel).max(value);
            }
        }
    }
    crop(Coverage {
        width,
        height,
        alpha,
    })
}

/// The drawing without the empty rows and columns around the ink (the
/// glyph boxes are a little bigger than their ink).
fn crop(drawing: Coverage) -> Coverage {
    let Coverage {
        width,
        height,
        alpha,
    } = &drawing;
    let inked = |x: u32, y: u32| alpha[(y * width + x) as usize] > 0;
    let columns: Vec<u32> = (0..*width)
        .filter(|&x| (0..*height).any(|y| inked(x, y)))
        .collect();
    let rows: Vec<u32> = (0..*height)
        .filter(|&y| (0..*width).any(|x| inked(x, y)))
        .collect();
    let (Some(&left), Some(&right), Some(&top), Some(&bottom)) =
        (columns.first(), columns.last(), rows.first(), rows.last())
    else {
        return drawing;
    };
    let mut cropped = Vec::with_capacity(((right - left + 1) * (bottom - top + 1)) as usize);
    for y in top..=bottom {
        let row = (y * width) as usize;
        cropped.extend_from_slice(&alpha[row + left as usize..=row + right as usize]);
    }
    Coverage {
        width: right - left + 1,
        height: bottom - top + 1,
        alpha: cropped,
    }
}

/// The value of an attribute of the SVG (the first one with that name).
fn svg_attribute<'a>(svg: &'a str, name: &str) -> &'a str {
    let start = svg
        .find(&format!(" {name}=\""))
        .map(|at| at + name.len() + 3)
        .unwrap_or_else(|| panic!("the SVG has {name}"));
    let end = svg[start..].find('"').expect("attributes are closed") + start;
    &svg[start..end]
}

/// The helmet, `height` px tall (its view box, with its margin); nothing
/// (as wide as tall, clear) where the content has none (it is a game's: the
/// engine's own content has none).
pub fn draw_helmet(height: u32) -> Coverage {
    let Ok(svg) = crate::content::try_text(HELMET_SVG) else {
        return Coverage {
            width: height,
            height,
            alpha: vec![0; (height * height) as usize],
        };
    };
    let view: Vec<f32> = svg_attribute(svg, "viewBox")
        .split_whitespace()
        .map(|value| value.parse().expect("the view box is numbers"))
        .collect();
    let [left, top, view_width, view_height] = view[..] else {
        panic!("the view box has four numbers");
    };
    let scale = height as f32 / view_height;
    let width = (view_width * scale).round().max(1.0) as u32;
    let (alpha, _) = Mask::new(svg_attribute(svg, "d"))
        .style(Fill::EvenOdd)
        .transform(Some(
            Transform::translation(-left, -top).then(&Transform::scale(scale, scale)),
        ))
        .size(width, height)
        .render();
    Coverage {
        width,
        height,
        alpha,
    }
}

/// A white image with the coverage as its alpha. With mipmaps, it stays
/// smooth however small it is shown; without, it is meant to be shown at its
/// own size.
pub fn white_image(drawing: &Coverage, mipmaps: bool) -> Image {
    let data = drawing
        .alpha
        .iter()
        .flat_map(|&alpha| [255, 255, 255, alpha])
        .collect();
    let mut image = Image::new(
        Extent3d {
            width: drawing.width,
            height: drawing.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    if mipmaps {
        generate_mipmaps(&mut image);
    }
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..ImageSamplerDescriptor::default()
    });
    image
}

/// A piece of vector art.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Art {
    Helmet,
    /// Slanted heavy letters.
    Text(String),
}

/// The art drawn so far, by piece and height (px), with its size (px).
#[derive(Resource, Default)]
pub struct VectorArt(HashMap<(Art, u32), (Handle<Image>, UVec2)>);

impl VectorArt {
    /// The image of `art`, `height` px tall, drawn the first time it is asked
    /// for, and its size (px).
    pub fn get(
        &mut self,
        art: Art,
        height: u32,
        images: &mut Assets<Image>,
    ) -> (Handle<Image>, UVec2) {
        let height = height.max(1);
        self.0
            .entry((art.clone(), height))
            .or_insert_with(|| {
                let drawing = match &art {
                    Art::Helmet => draw_helmet(height),
                    Art::Text(text) => draw_text(text, height),
                };
                let size = UVec2::new(drawing.width, drawing.height);
                (images.add(white_image(&drawing, false)), size)
            })
            .clone()
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    #[test]
    fn labels_are_cropped_to_their_ink_at_the_height_asked() {
        let letters = draw_text("1P", 72);
        let at = |x: u32, y: u32| letters.alpha[(y * letters.width + x) as usize];
        // Some ink touches each edge.
        assert!((0..letters.width).any(|x| at(x, 0) > 0));
        assert!((0..letters.width).any(|x| at(x, letters.height - 1) > 0));
        assert!((0..letters.height).any(|y| at(0, y) > 0));
        assert!((0..letters.height).any(|y| at(letters.width - 1, y) > 0));
        assert!(letters.height.abs_diff(72) <= 1, "{}", letters.height);
    }

    #[test]
    fn the_letters_lean_forward() {
        // The top of an "I" is further right than its foot.
        let letters = draw_text("I", 70);
        let ink_center = |y: u32| {
            let row = &letters.alpha[(y * letters.width) as usize..][..letters.width as usize];
            let total: f32 = row.iter().map(|&v| v as f32).sum();
            row.iter()
                .enumerate()
                .map(|(x, &v)| x as f32 * v as f32)
                .sum::<f32>()
                / total
        };
        assert!(ink_center(2) > ink_center(letters.height - 3) + 5.0);
    }

    #[test]
    fn longer_labels_are_wider() {
        assert!(advance("SALIR DEL JUEGO", 50.0) > advance("OPCIONES", 50.0));
        assert!(draw_text("EXIT GAME", 40).width > draw_text("OPTIONS", 40).width);
        assert_eq!(advance("", 50.0), 0.0);
    }

    #[test]
    fn with_no_helmet_in_the_content_none_is_drawn() {
        if crate::content::try_text(HELMET_SVG).is_ok() {
            return;
        }
        let helmet = draw_helmet(200);
        assert_eq!((helmet.width, helmet.height), (200, 200));
        assert!(helmet.alpha.iter().all(|&a| a == 0));
    }

    pub fn the_helmet_is_drawn_from_its_svg_on() {
        let helmet = draw_helmet(200);
        assert_eq!(helmet.height, 200);
        // As wide as its view box says (261.88 by 269.62).
        assert_eq!(helmet.width, 194);
        let at = |x: f32, y: f32| {
            let (x, y) = (
                (x * helmet.width as f32) as u32,
                (y * helmet.height as f32) as u32,
            );
            helmet.alpha[(y * helmet.width + x) as usize]
        };
        // Solid shell at the back, nothing in the corners, and the visor
        // opening (even-odd fill) in front.
        assert_eq!(at(0.2, 0.5), 255);
        assert_eq!(at(0.01, 0.01), 0);
        assert_eq!(at(0.99, 0.99), 0);
        let filled = helmet.alpha.iter().filter(|&&a| a > 127).count();
        let share = filled as f32 / helmet.alpha.len() as f32;
        assert!((0.3..0.7).contains(&share), "{share}");
    }

    /// Every character the game shows (the non-ASCII ones in the code,
    /// outside comments) must exist in the UI font: the font has no arrows,
    /// ticks nor PlayStation symbols, and those show as gaps. Arrows and
    /// symbols are drawn as icons instead (see [`crate::legend`]).
    #[test]
    fn every_character_shown_exists_in_the_ui_font() {
        let mut missing = Vec::new();
        let mut stack = vec![std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src"
        ))];
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                for entry in std::fs::read_dir(&path).unwrap() {
                    stack.push(entry.unwrap().path());
                }
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            for (number, line) in source.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                for c in code.chars().filter(|c| !c.is_ascii()) {
                    if !has_char(c) {
                        missing.push(format!("{}:{}: {c:?}", path.display(), number + 1));
                    }
                }
            }
        }
        assert!(missing.is_empty(), "missing in the UI font: {missing:#?}");
    }
}
