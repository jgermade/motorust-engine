//! The speedometer of the race, bottom right, after the user's SVG design
//! (its measures and colours are the constants below): a see-through dial
//! that darkens towards its rim (from clear in the middle to translucent
//! black), the speeds from 0 to 360 km/h round it (from 180 a little bigger
//! each, and from 300 red, over a red arc), a blue pointer, and the speed in
//! figures on a translucent dark disc in the middle. The pointer glows, and
//! the figure of the scale it points at lights up, with no halo: blue, or
//! bright red from the red line up.
//!
//! Bevy does not draw SVG, so the shapes of the design are drawn with zeno
//! (the rasterizer Bevy already uses for the fonts, which reads SVG path
//! data) and its figures with the UI font, once per size, at the exact size
//! the dial takes on the screen. The pointer is another image the size of
//! the dial, turned with the node's [`UiTransform`]. The figures of the
//! scale lit up are drawn once each, on their own small images over the
//! dial, shown as much as each one is lit. Each frame, only the pointer's
//! angle, how lit each figure is and the figures in the middle change.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::LineHeight;
use bevy::ui::UiSystems;
use swash::zeno::{Mask, Stroke, Transform as Affine};

use crate::bike::{BikeState, LocalPlayer, Player};
use crate::geometry::smoothstep;
use crate::menu::GameState;
use crate::settings::Settings;
use crate::vector_art::{Coverage, draw_plain_text};

// The design, in its own units: its view box is 380 wide, with the dial
// centred on (190, 190).
const CENTER: f32 = 190.0;
/// How far from the centre the drawing goes: the dial's outer edge (150)
/// and one unit more for its smoothed border.
const EXTENT: f32 = 151.0;
/// The radii of the dial's outer edge, the middle of its rim (10 wide), its
/// face, where the figures stand, and the disc in the middle.
const OUTER: f32 = 150.0;
const RIM: f32 = 145.0;
const RIM_WIDTH: f32 = 10.0;
const FACE: f32 = 140.0;
const MARKS: f32 = 118.0;
const HUB: f32 = 52.0;
/// The thin lines round the edge, the face and the disc in the middle.
const LINE: f32 = 0.5;
/// The red arc along the edge of the face, from the red line to the top of
/// the scale.
const ARC_WIDTH: f32 = 6.0;
/// The pointer (pointing straight up): a triangle from the disc in the
/// middle out towards the figures.
const POINTER: [(f32, f32); 3] = [(180.0, 138.0), (200.0, 138.0), (190.0, 92.0)];
/// The figures in the middle: their sizes and the heights of their centres.
const SPEED_SIZE: f32 = 32.0;
const SPEED_AT: f32 = 187.0;
const UNIT_SIZE: f32 = 12.0;
const UNIT_AT: f32 = 214.0;

/// The scale: from 0 to 360 km/h, round three quarters of the dial (from
/// 135° left of the top to 135° right of it, degrees), marked every 30, red
/// from 300.
const TOP_SPEED: f32 = 360.0;
const START: f32 = -135.0;
const SWEEP: f32 = 270.0;
const MARK_STEP: u32 = 30;
const REDLINE: u32 = 300;

/// The colours (sRGB) of the design.
const EDGE_COLOR: [f32; 3] = rgb(0x88, 0x87, 0x80);
const FACE_EDGE_COLOR: [f32; 3] = rgb(0xb4, 0xb2, 0xa9);
const RED: [f32; 3] = rgb(0xe2, 0x4b, 0x4a);
/// The lit pointer: lighter than the design's blue (#378add), turning
/// almost white towards its tip, with a lit filament down its middle; its
/// outline as sharp as the design's.
const LIT_BLUE: [f32; 3] = rgb(0x4c, 0xa6, 0xff);
const POINTER_TIP: [f32; 3] = rgb(0xdc, 0xf2, 0xff);
/// A figure of the scale lit up: blue, or bright red from the red line up,
/// with nothing round it.
const LIT_FIGURE: [f32; 3] = rgb(0x3a, 0x9b, 0xff);
const LIT_RED: [f32; 3] = rgb(0xff, 0x24, 0x1c);
/// The box round each figure of the scale its lit image covers (units of
/// the design).
const LIT_BOX: Vec2 = Vec2::new(64.0, 40.0);
const BLACK: [f32; 3] = [0.0, 0.0, 0.0];
const WHITE: [f32; 3] = [1.0, 1.0, 1.0];
/// How dark the rim, the edge of the face (clear in the middle) and the
/// disc in the middle are.
const RIM_SHADE: f32 = 0.6;
const FACE_SHADE: f32 = 0.5;
const HUB_SHADE: f32 = 0.6;

const fn rgb(r: u8, g: u8, b: u8) -> [f32; 3] {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0]
}

pub struct SpeedometerPlugin;

impl Plugin for SpeedometerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DialArt>()
            .add_systems(Update, show_speed.run_if(in_state(GameState::Race)))
            .add_systems(
                PostUpdate,
                draw_dials
                    .after(UiSystems::Layout)
                    .run_if(in_state(GameState::Race)),
            );
    }
}

/// A speedometer: its dial, with the pointer and the figures on it.
#[derive(Component)]
struct Speedometer;

/// The pointer of a speedometer, and whose speed it shows (0 for player 1,
/// 1 for player 2).
#[derive(Component)]
struct Pointer(usize);

/// The speed in figures in the middle of a speedometer, and whose speed it
/// is.
#[derive(Component)]
struct Figures(usize);

/// The words under the speed of a player's dial: the unit, and with the
/// gears on the gear.
#[derive(Component)]
struct Unit(usize);

/// A figure of the scale lit up while the pointer of `player`'s speedometer
/// points at it: the speed it marks (km/h).
#[derive(Component)]
struct LitFigure {
    player: usize,
    kmh: u32,
}

/// The speeds marked round the dial (km/h).
fn marks() -> impl Iterator<Item = u32> {
    (0..=TOP_SPEED as u32).step_by(MARK_STEP as usize)
}

/// The box of the design (units) a lit figure's image covers: round where
/// the figure stands.
fn lit_box(kmh: u32) -> Rect {
    Rect::from_center_size(on_dial(pointer_angle(kmh as f32), MARKS), LIT_BOX)
}

/// How lit the figure for `mark` km/h is at `kmh` (0..1): fully while it
/// is the figure nearest the pointer, fading into the next half way
/// between them.
pub fn figure_light(mark: u32, kmh: f32) -> f32 {
    let step = MARK_STEP as f32;
    1.0 - smoothstep(0.37 * step, 0.63 * step, (kmh - mark as f32).abs())
}

/// A speedometer `diameter` px wide for `player`, placed by `node`.
pub fn spawn_speedometer(
    commands: &mut Commands,
    player: usize,
    diameter: f32,
    node: Node,
    font: &Handle<Font>,
) {
    // Px on the screen per unit of the design.
    let scale = diameter / (2.0 * EXTENT);
    let figures = |size: f32, at: f32| {
        let line = size * scale * 1.25;
        (
            TextFont {
                font: font.clone().into(),
                font_size: FontSize::Px(size * scale),
                ..default()
            },
            LineHeight::Px(line),
            TextColor(Color::WHITE),
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                width: percent(100),
                top: px((at - (CENTER - EXTENT)) * scale - line / 2.0),
                ..default()
            },
        )
    };
    let lit: Vec<_> = marks()
        .map(|kmh| {
            let area = lit_box(kmh);
            let share = |units: f32| percent(units / (2.0 * EXTENT) * 100.0);
            (
                LitFigure { player, kmh },
                ImageNode {
                    color: Color::NONE,
                    ..default()
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: share(area.min.x - (CENTER - EXTENT)),
                    top: share(area.min.y - (CENTER - EXTENT)),
                    width: share(area.width()),
                    height: share(area.height()),
                    ..default()
                },
            )
        })
        .collect();
    let mut dial = commands.spawn((
        Speedometer,
        DespawnOnExit(GameState::Race),
        Node {
            width: px(diameter),
            height: px(diameter),
            ..node
        },
        ImageNode::default(),
    ));
    dial.with_children(|dial| {
        // The lit figures, under the pointer.
        for figure in lit {
            dial.spawn(figure);
        }
        dial.spawn((
            Pointer(player),
            ImageNode::default(),
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ));
        dial.spawn((
            Figures(player),
            Text::new("0"),
            figures(SPEED_SIZE, SPEED_AT),
        ));
        dial.spawn((Unit(player), Text::new("km/h"), figures(UNIT_SIZE, UNIT_AT)));
    });
}

/// The words under the speed with the gears on: the gear `gear` is in (0
/// the first) and the unit.
fn unit_label(gear: usize, lang: crate::locale::Language) -> String {
    match lang {
        crate::locale::Language::Spanish => format!("{}ª · km/h", gear + 1),
        crate::locale::Language::English => format!("G{} · km/h", gear + 1),
    }
}

/// Where the pointer points at `kmh` (rad, clockwise from straight up).
pub fn pointer_angle(kmh: f32) -> f32 {
    (START + SWEEP * (kmh / TOP_SPEED).clamp(0.0, 1.0)).to_radians()
}

/// The pointers, the lit figures of the scale and the figures in the
/// middle follow each player's speed.
fn show_speed(
    player: Query<&BikeState, With<Player>>,
    others: Query<(&BikeState, &LocalPlayer), Without<Player>>,
    settings: Res<Settings>,
    mut pointers: Query<(&Pointer, &mut UiTransform)>,
    mut figures: Query<(&Figures, &mut Text), Without<Unit>>,
    mut units: Query<(&Unit, &mut Text), Without<Figures>>,
    mut lit: Query<(&LitFigure, &mut ImageNode)>,
) {
    let state = |index: usize| {
        if index == 0 {
            player.iter().next()
        } else {
            others
                .iter()
                .find(|(_, local)| local.0 == index)
                .map(|(state, _)| state)
        }
    };
    let kmh = |index: usize| state(index).map_or(0.0, |state| state.speed.abs() * 3.6);
    // With the gears on, the gear under the speed.
    for (unit, mut text) in &mut units {
        let label = match state(unit.0).filter(|_| settings.gears) {
            Some(state) => unit_label(state.gear, settings.language),
            None => "km/h".to_string(),
        };
        if text.0 != label {
            text.0 = label;
        }
    }
    for (pointer, mut transform) in &mut pointers {
        let rotation = Rot2::radians(pointer_angle(kmh(pointer.0)));
        if transform.rotation != rotation {
            transform.rotation = rotation;
        }
    }
    for (figures, mut text) in &mut figures {
        let label = format!("{:.0}", kmh(figures.0));
        if text.0 != label {
            text.0 = label;
        }
    }
    for (figure, mut image) in &mut lit {
        let color = Color::WHITE.with_alpha(figure_light(figure.kmh, kmh(figure.player)));
        if image.color != color {
            image.color = color;
        }
    }
}

/// The images of a dial drawn at one size: the dial, its pointer and each
/// figure of its scale lit up (in the order of [`marks`]).
struct DialImages {
    dial: Handle<Image>,
    pointer: Handle<Image>,
    lit: Vec<Handle<Image>>,
}

/// What draws on a dial: its pointer and its lit figures.
type DialPart = (
    &'static mut ImageNode,
    Has<Pointer>,
    Option<&'static LitFigure>,
);

/// The dials drawn so far, by size (px).
#[derive(Resource, Default)]
struct DialArt(HashMap<u32, DialImages>);

/// Each dial, its pointer and its lit figures get the images drawn at the
/// size the dial takes on the screen; a new size (the window was resized)
/// is drawn when it is first needed, and the sizes no longer shown are let
/// go.
fn draw_dials(
    mut art: ResMut<DialArt>,
    mut images: ResMut<Assets<Image>>,
    mut dials: Query<(&ComputedNode, &mut ImageNode, &Children), With<Speedometer>>,
    mut parts: Query<DialPart, Without<Speedometer>>,
) {
    let size = |node: &ComputedNode| node.size().x.round() as u32;
    let shown: Vec<u32> = dials
        .iter()
        .map(|(node, ..)| size(node))
        .filter(|&size| size > 0)
        .collect();
    if shown.iter().any(|size| !art.0.contains_key(size)) {
        art.0.retain(|size, _| shown.contains(size));
        for &size in &shown {
            art.0.entry(size).or_insert_with(|| DialImages {
                dial: images.add(dial(size)),
                pointer: images.add(pointer(size)),
                lit: marks()
                    .map(|kmh| images.add(lit_figure(size, kmh)))
                    .collect(),
            });
        }
    }
    let show = |image: &mut ImageNode, wanted: &Handle<Image>| {
        if image.image != *wanted {
            image.image = wanted.clone();
        }
    };
    for (node, mut image, children) in &mut dials {
        let Some(drawn) = art.0.get(&size(node)) else {
            continue;
        };
        show(&mut image, &drawn.dial);
        for &child in children {
            let Ok((mut image, is_pointer, lit)) = parts.get_mut(child) else {
                continue;
            };
            if is_pointer {
                show(&mut image, &drawn.pointer);
            } else if let Some(lit) = lit {
                show(&mut image, &drawn.lit[(lit.kmh / MARK_STEP) as usize]);
            }
        }
    }
}

/// The point of the design `radius` from the centre, `angle` rad clockwise
/// from straight up.
fn on_dial(angle: f32, radius: f32) -> Vec2 {
    Vec2::new(CENTER + radius * angle.sin(), CENTER - radius * angle.cos())
}

/// A circle as SVG path data.
fn circle(radius: f32) -> String {
    let (left, right) = (CENTER - radius, CENTER + radius);
    format!(
        "M {left} {CENTER} A {radius} {radius} 0 1 0 {right} {CENTER} \
         A {radius} {radius} 0 1 0 {left} {CENTER} Z"
    )
}

/// A drawing in the making, `size` px square, covering the design out to
/// [`EXTENT`]: plain (not premultiplied) sRGB colour and opacity per pixel,
/// from 0 to 1, row by row. Shapes are laid over it one after the other, as
/// a browser draws an SVG.
struct Canvas {
    size: u32,
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    /// An empty drawing. `color` is the colour of its clear pixels: where
    /// the image is shown smaller or turned, they blend into its edges.
    fn new(size: u32, color: [f32; 3]) -> Self {
        let [r, g, b] = color;
        Self {
            size,
            pixels: vec![[r, g, b, 0.0]; (size * size) as usize],
        }
    }

    /// Px per unit of the design.
    fn scale(&self) -> f32 {
        self.size as f32 / (2.0 * EXTENT)
    }

    /// The point of the design at the centre of pixel (`x`, `y`).
    fn design_point(&self, x: u32, y: u32) -> Vec2 {
        Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / self.scale() + (CENTER - EXTENT)
    }

    /// How much of each pixel a shape of the design covers (0..=255).
    fn mask(&self, path: &str, stroke: Option<Stroke>) -> Vec<u8> {
        let scale = self.scale();
        let mut mask = Mask::new(path);
        mask.transform(Some(
            Affine::translation(EXTENT - CENTER, EXTENT - CENTER)
                .then(&Affine::scale(scale, scale)),
        ))
        .size(self.size, self.size);
        if let Some(stroke) = stroke {
            mask.style(stroke);
        }
        mask.render().0
    }

    /// Lays `color` over the drawing where `coverage` covers, as opaque as
    /// `opacity` says at each point of the design.
    fn paint(&mut self, coverage: &[u8], color: [f32; 3], opacity: impl Fn(Vec2) -> f32) {
        for y in 0..self.size {
            for x in 0..self.size {
                let index = (y * self.size + x) as usize;
                if coverage[index] == 0 {
                    continue;
                }
                let alpha = coverage[index] as f32 / 255.0 * opacity(self.design_point(x, y));
                self.pixels[index] = over(self.pixels[index], color, alpha);
            }
        }
    }

    fn fill(&mut self, path: &str, color: [f32; 3], opacity: f32) {
        let coverage = self.mask(path, None);
        self.paint(&coverage, color, |_| opacity);
    }

    fn stroke(&mut self, path: &str, width: f32, color: [f32; 3], opacity: f32) {
        let coverage = self.mask(path, Some(Stroke::new(width)));
        self.paint(&coverage, color, |_| opacity);
    }

    /// `text` in the UI font, `size` units of the design, centred on `at`.
    fn letters(&mut self, text: &str, size: f32, at: Vec2, color: [f32; 3]) {
        let ink = self.letter_coverage(text, size, at);
        self.paint_coverage(&ink, color, 1.0);
    }

    /// How much of each pixel `text` in the UI font covers (0..1), `size`
    /// units of the design, centred on `at`.
    fn letter_coverage(&self, text: &str, size: f32, at: Vec2) -> Vec<f32> {
        let mut ink = vec![0.0; (self.size * self.size) as usize];
        let Coverage {
            width,
            height,
            alpha,
        } = draw_plain_text(text, size * self.scale());
        let center = (at - (CENTER - EXTENT)) * self.scale();
        let left = (center.x - width as f32 / 2.0).round() as i32;
        let top = (center.y - height as f32 / 2.0).round() as i32;
        for row in 0..height {
            for column in 0..width {
                let (x, y) = (left + column as i32, top + row as i32);
                if !(0..self.size as i32).contains(&x) || !(0..self.size as i32).contains(&y) {
                    continue;
                }
                let index = (y as u32 * self.size + x as u32) as usize;
                ink[index] = alpha[(row * width + column) as usize] as f32 / 255.0;
            }
        }
        ink
    }

    /// Lays `color` over the drawing as much as `coverage` (0..1) covers
    /// each pixel, times `opacity`.
    fn paint_coverage(&mut self, coverage: &[f32], color: [f32; 3], opacity: f32) {
        for (pixel, &cover) in self.pixels.iter_mut().zip(coverage) {
            if cover > 0.0 {
                *pixel = over(*pixel, color, (cover * opacity).min(1.0));
            }
        }
    }

    /// The drawing as an sRGB image, shown at its own size.
    fn image(self) -> Image {
        let size = self.size;
        self.image_of(UVec2::ZERO, UVec2::splat(size))
    }

    /// The pixels of the drawing from `from`, `size` of them, as an sRGB
    /// image.
    fn image_of(&self, from: UVec2, size: UVec2) -> Image {
        let data = (from.y..from.y + size.y)
            .flat_map(|y| (from.x..from.x + size.x).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                let pixel = self.pixels[(y * self.size + x) as usize];
                pixel.map(|channel| (channel * 255.0).round() as u8)
            })
            .collect();
        let mut image = Image::new(
            Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..ImageSamplerDescriptor::default()
        });
        image
    }
}

/// `color`, `alpha` opaque, laid over `below`.
fn over(below: [f32; 4], color: [f32; 3], alpha: f32) -> [f32; 4] {
    let opacity = alpha + below[3] * (1.0 - alpha);
    if opacity <= 0.0 {
        return below;
    }
    let mix = |c: usize| (color[c] * alpha + below[c] * below[3] * (1.0 - alpha)) / opacity;
    [mix(0), mix(1), mix(2), opacity]
}

/// The dial, `size` px wide: everything but the pointer and the figures in
/// the middle.
fn dial_canvas(size: u32) -> Canvas {
    let mut canvas = Canvas::new(size, BLACK);
    // A thin grey edge round the dial, and the rim inside it.
    canvas.stroke(&circle(OUTER), LINE, EDGE_COLOR, 1.0);
    canvas.stroke(&circle(RIM), RIM_WIDTH, BLACK, RIM_SHADE);
    // The face goes from clear in the middle to translucent black at its
    // edge: the race shows through it.
    let face = canvas.mask(&circle(FACE), None);
    canvas.paint(&face, BLACK, |at| {
        FACE_SHADE * (at.distance(Vec2::splat(CENTER)) / FACE).min(1.0)
    });
    canvas.stroke(&circle(FACE), LINE, FACE_EDGE_COLOR, 1.0);
    // The red arc, from the red line to the top of the scale.
    let (from, to) = (
        on_dial(pointer_angle(REDLINE as f32), FACE),
        on_dial(pointer_angle(TOP_SPEED), FACE),
    );
    let arc = format!(
        "M {} {} A {FACE} {FACE} 0 0 1 {} {}",
        from.x, from.y, to.x, to.y
    );
    canvas.stroke(&arc, ARC_WIDTH, RED, 1.0);
    // The speeds round the dial: from 180 up, each a little bigger.
    for kmh in marks() {
        let color = if kmh >= REDLINE { RED } else { WHITE };
        let at = on_dial(pointer_angle(kmh as f32), MARKS);
        canvas.letters(&kmh.to_string(), figure_size(kmh), at, color);
    }
    // The darker disc in the middle, under the figures.
    canvas.fill(&circle(HUB), BLACK, HUB_SHADE);
    canvas.stroke(&circle(HUB), LINE, EDGE_COLOR, 1.0);
    canvas
}

fn dial(size: u32) -> Image {
    dial_canvas(size).image()
}

/// How big the figure for `kmh` is (units of the design): from 180 up, each
/// a little bigger.
fn figure_size(kmh: u32) -> f32 {
    14.0 + (kmh.saturating_sub(180) / MARK_STEP) as f32
}

/// The figure for `kmh` lit up, on a drawing the size of the dial: blue,
/// or from the red line up bright red, with no halo.
fn lit_figure_canvas(size: u32, kmh: u32) -> Canvas {
    let color = if kmh >= REDLINE { LIT_RED } else { LIT_FIGURE };
    let mut canvas = Canvas::new(size, color);
    let at = on_dial(pointer_angle(kmh as f32), MARKS);
    let ink = canvas.letter_coverage(&kmh.to_string(), figure_size(kmh), at);
    canvas.paint_coverage(&ink, color, 1.0);
    canvas
}

/// The figure for `kmh` lit up, for a dial `size` px wide: just the box
/// round it (see [`lit_box`]).
fn lit_figure(size: u32, kmh: u32) -> Image {
    let canvas = lit_figure_canvas(size, kmh);
    let area = lit_box(kmh);
    let pixel = |units: Vec2| ((units - (CENTER - EXTENT)) * canvas.scale()).round();
    let (from, to) = (
        pixel(area.min),
        pixel(area.max).min(Vec2::splat(size as f32)),
    );
    canvas.image_of(from.as_uvec2(), (to - from).max(Vec2::ONE).as_uvec2())
}

/// The pointer, `size` px wide like the dial, pointing straight up: lit
/// from within, lighter towards its tip, with a filament down its middle,
/// and as sharp as the design's.
fn pointer_canvas(size: u32) -> Canvas {
    let mut canvas = Canvas::new(size, LIT_BLUE);
    let [a, b, c] = POINTER;
    let triangle = format!("M {} {} L {} {} L {} {} Z", a.0, a.1, b.0, b.1, c.0, c.1);
    let shape = canvas.mask(&triangle, None);
    canvas.paint(&shape, LIT_BLUE, |_| 1.0);
    // Lighter towards the tip.
    canvas.paint(&shape, POINTER_TIP, |at| 0.8 * smoothstep(a.1, c.1, at.y));
    let core = format!("M {} {} L {} {}", CENTER, a.1 - 4.0, c.0, c.1 + 12.0);
    canvas.stroke(&core, 1.6, WHITE, 0.7);
    canvas
}

fn pointer(size: u32) -> Image {
    pointer_canvas(size).image()
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn with_the_gears_the_dial_says_which_one() {
        use crate::locale::Language;
        assert_eq!(unit_label(0, Language::Spanish), "1ª · km/h");
        assert_eq!(unit_label(5, Language::English), "G6 · km/h");
    }

    /// The colour (0..=255) of the drawing at a point of the design.
    fn at(canvas: &Canvas, point: Vec2) -> [u8; 4] {
        let pixel = (point - (CENTER - EXTENT)) * canvas.scale();
        let index = (pixel.y as u32 * canvas.size + pixel.x as u32) as usize;
        canvas.pixels[index].map(|channel| (channel * 255.0).round() as u8)
    }

    #[test]
    fn the_scale_runs_round_three_quarters_of_the_dial() {
        assert_eq!(pointer_angle(0.0), (-135.0f32).to_radians());
        assert!(pointer_angle(180.0).abs() < 1e-6, "180 straight up");
        assert_eq!(pointer_angle(360.0), 135.0f32.to_radians());
        // 240 points as in the design: 45° right of the top.
        assert!((pointer_angle(240.0) - 45.0f32.to_radians()).abs() < 1e-6);
        // It stops at both ends.
        assert_eq!(pointer_angle(-20.0), pointer_angle(0.0));
        assert_eq!(pointer_angle(500.0), pointer_angle(360.0));
    }

    #[test]
    fn the_dial_is_drawn_as_in_the_design() {
        let canvas = dial_canvas(302);
        let center = Vec2::splat(CENTER);
        // Clear outside the dial.
        assert_eq!(at(&canvas, Vec2::new(45.0, 45.0))[3], 0);
        // The face is black, clear in the middle and more opaque towards
        // its edge, and the rim more opaque still.
        let alpha = |point: Vec2| {
            let pixel = at(&canvas, point);
            assert_eq!(pixel[..3], [0, 0, 0], "black at {point}");
            pixel[3]
        };
        let middle = alpha(center + Vec2::new(-80.0, 0.0));
        let near_edge = alpha(center + Vec2::new(-130.0, -20.0));
        let rim = alpha(center + Vec2::new(0.0, RIM));
        assert!(middle > 30 && middle < near_edge && near_edge < rim);
        assert_eq!(rim, (RIM_SHADE * 255.0).round() as u8);
        // The red arc on the right, below the middle.
        let arc = at(&canvas, on_dial(pointer_angle(330.0), FACE));
        assert!(arc[0] > 200 && arc[1] < 100, "{arc:?}");
        // The disc in the middle is translucent black, darker than the face
        // round it (away from where the figures go).
        let hub = at(&canvas, center + Vec2::new(-40.0, -20.0));
        let face = at(&canvas, center + Vec2::new(-60.0, -20.0));
        assert!(hub[3] >= (HUB_SHADE * 255.0) as u8 && hub[3] < 255);
        assert!(hub[3] > face[3] + 60, "{hub:?} {face:?}");
    }

    #[test]
    fn the_speeds_are_marked_round_the_dial() {
        let canvas = dial_canvas(604);
        // The ink of a figure at a speed's place: white up to 270, red from 300.
        let ink = |kmh: f32| {
            let center = on_dial(pointer_angle(kmh), MARKS);
            let mut most: Option<[u8; 4]> = None;
            for dy in -6..=6 {
                for dx in -6..=6 {
                    let pixel = at(&canvas, center + Vec2::new(dx as f32, dy as f32));
                    let lightness = |p: [u8; 4]| p[0] as u32 + p[1] as u32 + p[2] as u32;
                    let distinct = |p: [u8; 4]| p[0].abs_diff(p[1]) as u32;
                    if most.is_none_or(|m| {
                        lightness(pixel) + 2 * distinct(pixel) > lightness(m) + 2 * distinct(m)
                    }) {
                        most = Some(pixel);
                    }
                }
            }
            most.unwrap()
        };
        for kmh in [0.0, 90.0, 180.0, 270.0] {
            let pixel = ink(kmh);
            assert!(pixel.iter().take(3).all(|&c| c > 230), "{kmh}: {pixel:?}");
        }
        for kmh in [300.0, 360.0] {
            let pixel = ink(kmh);
            assert!(pixel[0] > 200 && pixel[1] < 120, "{kmh}: {pixel:?}");
        }
    }

    #[test]
    fn the_pointer_points_up_from_the_middle() {
        let canvas = pointer_canvas(302);
        let blue = at(&canvas, Vec2::new(CENTER, 120.0));
        assert_eq!(blue[3], 255);
        assert!(blue[2] > blue[0]);
        // Nothing in the middle, nor below it.
        assert_eq!(at(&canvas, Vec2::splat(CENTER))[3], 0);
        assert_eq!(at(&canvas, Vec2::new(CENTER, 250.0))[3], 0);
        // Its clear pixels are blue too, so its edges stay blue when it turns.
        assert_eq!(
            at(&canvas, Vec2::new(60.0, 60.0))[2],
            (LIT_BLUE[2] * 255.0).round() as u8
        );
        // Sharp: clear right outside it, with no halo.
        assert_eq!(at(&canvas, Vec2::new(199.0, 120.0))[3], 0);
        // Lit from within: lighter towards its tip.
        let base = at(&canvas, Vec2::new(184.0, 136.0));
        let tip = at(&canvas, Vec2::new(190.5, 97.0));
        assert!(tip[0] > base[0] + 60, "{base:?} {tip:?}");
    }

    #[test]
    fn the_figure_the_pointer_is_at_lights_up() {
        assert_eq!(figure_light(120, 120.0), 1.0);
        assert_eq!(figure_light(120, 128.0), 1.0);
        assert!((figure_light(120, 135.0) - 0.5).abs() < 1e-5);
        assert!((figure_light(150, 135.0) - 0.5).abs() < 1e-5);
        assert_eq!(figure_light(120, 145.0), 0.0);
        assert_eq!(figure_light(0, 0.0), 1.0);
        // Always one at least half lit, all the way round.
        for kmh in 0..=360 {
            let most = marks()
                .map(|mark| figure_light(mark, kmh as f32))
                .fold(0.0, f32::max);
            assert!(most >= 0.5, "{kmh}");
        }
    }

    #[test]
    fn a_lit_figure_is_blue_and_red_from_the_red_line() {
        let canvas = lit_figure_canvas(302, 120);
        let center = on_dial(pointer_angle(120.0), MARKS);
        let bluest = (-5..=5)
            .flat_map(|dy| (-8..=8).map(move |dx| Vec2::new(dx as f32, dy as f32)))
            .map(|d| at(&canvas, center + d))
            .max_by_key(|p| p[3] as u32 * p[2] as u32)
            .unwrap();
        assert!(bluest[2] > 230 && bluest[2] > bluest[0] + 40, "{bluest:?}");
        let canvas = lit_figure_canvas(302, 300);
        let center = on_dial(pointer_angle(300.0), MARKS);
        let mut reddest = [0u8; 4];
        for dy in -5..=5 {
            for dx in -8..=8 {
                let pixel = at(&canvas, center + Vec2::new(dx as f32, dy as f32));
                if pixel[3] as u32 * pixel[0] as u32 > reddest[3] as u32 * reddest[0] as u32 {
                    reddest = pixel;
                }
            }
        }
        assert!(reddest[0] > 230 && reddest[3] > 230, "{reddest:?}");
        // Nothing round it.
        assert_eq!(at(&canvas, center + Vec2::new(0.0, 9.0))[3], 0);
        assert_eq!(at(&canvas, center + Vec2::new(0.0, 40.0))[3], 0);
        let image = lit_figure(302, 300);
        assert!(image.width() < 80 && image.height() < 50);
    }

    #[test]
    fn laying_colour_over_a_clear_pixel_gives_that_colour() {
        let clear = [0.2, 0.3, 0.4, 0.0];
        assert_eq!(over(clear, RED, 0.5), [RED[0], RED[1], RED[2], 0.5]);
        // Half black over white: middle grey, opaque.
        let grey = over([1.0, 1.0, 1.0, 1.0], BLACK, 0.5);
        assert_eq!(grey, [0.5, 0.5, 0.5, 1.0]);
    }
}
