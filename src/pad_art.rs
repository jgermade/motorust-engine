//! The gamepads of the controls' mapping (see [`crate::mapping`]), drawn
//! from illustrations: a DualSense, a DualShock 4, a DualShock 3 and an Xbox
//! controller (`assets/ui/pads/`, from the anygamepad project). Each one is
//! an SVG whose controls carry data attributes:
//!
//! - `data-button="<button>"`: a control; its `.shape` elements are the
//!   button, its `.mark` elements the symbol or arrow printed on it. A
//!   `data-label` on a mark is the letter printed there (A, B, X, Y on the
//!   Xbox controller).
//! - `data-stick`: a stick, whose cap is its click button.
//! - `data-trigger` with `data-x`, `data-y`, `data-width`, `data-height`:
//!   where a trigger goes (the triggers are not in the front view).
//!
//! Bevy does not draw SVG: resvg does, on tiny-skia, its few words in the
//! game's font. Each illustration is drawn once per size, in the background
//! (it takes a tenth of a second or so): the whole controller, and every
//! button on its own, white with its marks dark, cropped to it, so the view
//! can tint it over the controller (tinted if it does something, lit while
//! held). To draw the buttons on their own the illustration is read twice
//! more, once without the marks and once with only the marks of the
//! buttons, both without shadows, and each button group gets an id to be
//! found in them.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, fontdb, roxmltree};

use crate::ui::ActiveDevice;

/// An illustration of a gamepad.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PadArt {
    DualSense,
    DualShock4,
    DualShock3,
    Xbox,
}

impl PadArt {
    /// The illustration for a gamepad of the kind `device` called `name`
    /// (if known): PlayStation pads by their generation; Xbox, Switch and
    /// any other, the Xbox controller.
    pub fn of(device: ActiveDevice, name: Option<&str>) -> Self {
        if device != ActiveDevice::PlayStation {
            return PadArt::Xbox;
        }
        let name = name.unwrap_or_default().to_ascii_lowercase();
        if name.contains("dualshock 3") || name.contains("playstation(r)3") || name.contains("ps3")
        {
            PadArt::DualShock3
        } else if name.contains("dualshock 4")
            || name.contains("ps4")
            || name == "wireless controller"
        {
            PadArt::DualShock4
        } else {
            PadArt::DualSense
        }
    }

    /// How big it is drawn `height` tall (in the units of `height`).
    pub fn size(self, height: f32) -> Vec2 {
        let [_, _, width, tall] = self.view_box();
        Vec2::new(width * height / tall, height)
    }

    /// The illustration's viewBox, the first in its text: left, top, width
    /// and height.
    fn view_box(self) -> [f32; 4] {
        let view: Vec<f32> = self
            .svg()
            .split_once("viewBox=\"")
            .and_then(|(_, rest)| rest.split_once('"'))
            .map_or("0 0 400 260", |(view, _)| view)
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        [view[0], view[1], view[2], view[3]]
    }

    fn svg(self) -> &'static str {
        match self {
            PadArt::DualSense => crate::content::text("ui/pads/dualsense.svg"),
            PadArt::DualShock4 => crate::content::text("ui/pads/dualshock4.svg"),
            PadArt::DualShock3 => crate::content::text("ui/pads/dualshock3.svg"),
            PadArt::Xbox => crate::content::text("ui/pads/xbox.svg"),
        }
    }
}

/// The button of Bevy an illustration's `data-button` (or `data-trigger`)
/// is, if the game uses it.
fn button_of(name: &str) -> Option<GamepadButton> {
    use GamepadButton as B;
    Some(match name {
        "south" => B::South,
        "east" => B::East,
        "west" => B::West,
        "north" => B::North,
        "left_shoulder" => B::LeftTrigger,
        "right_shoulder" => B::RightTrigger,
        "left_trigger" => B::LeftTrigger2,
        "right_trigger" => B::RightTrigger2,
        "select" => B::Select,
        "start" => B::Start,
        "home" => B::Mode,
        "left_stick" => B::LeftThumb,
        "right_stick" => B::RightThumb,
        "dpad_up" => B::DPadUp,
        "dpad_down" => B::DPadDown,
        "dpad_left" => B::DPadLeft,
        "dpad_right" => B::DPadRight,
        _ => return None,
    })
}

/// Controls of the illustrations no pad of the game has: the back paddles
/// and the extra buttons of the Xbox controller.
const LEFT_OUT: [&str; 6] = [
    "left_paddle_1",
    "left_paddle_2",
    "right_paddle_1",
    "right_paddle_2",
    "extra_0",
    "extra_1",
];

/// The elements that draw something.
const DRAWING: [&str; 8] = [
    "path", "circle", "ellipse", "rect", "line", "polygon", "use", "text",
];

/// How an illustration is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Reading {
    /// The whole controller.
    Whole,
    /// The buttons' shapes, without their marks.
    Shapes,
    /// Only the buttons' marks.
    Marks,
}

/// The id a button group gets to be found in the tree.
fn button_id(name: &str) -> String {
    format!("pad-{name}")
}

fn has_class(node: roxmltree::Node, class: &str) -> bool {
    node.attribute("class")
        .is_some_and(|classes| classes.split_whitespace().any(|c| c == class))
}

/// The illustration's text made ready to be read as `reading` asks: without
/// the controls no pad of the game has, nor (`unlabelled`) the letters
/// printed on the face buttons; for a button on its own, without shadows
/// and without its marks, or with nothing but them. Each button group the
/// game uses gets its id ([`button_id`]).
fn prepared(document: &roxmltree::Document, reading: Reading, unlabelled: bool) -> String {
    let source = document.input_text();
    let mut cut: Vec<Range<usize>> = Vec::new();
    let mut ids: Vec<(usize, String)> = Vec::new();
    for node in document.descendants().filter(|n| n.is_element()) {
        let in_button = node
            .ancestors()
            .any(|a| a.attribute("data-button").and_then(button_of).is_some());
        let drawing = DRAWING.contains(&node.tag_name().name());
        let left_out = node
            .attribute("data-button")
            .is_some_and(|b| LEFT_OUT.contains(&b))
            || (unlabelled && node.attribute("data-label").is_some())
            || (reading == Reading::Shapes && in_button && has_class(node, "mark"))
            || (reading == Reading::Marks && in_button && drawing && !has_class(node, "mark"));
        if left_out {
            cut.push(node.range());
            continue;
        }
        if reading != Reading::Whole
            && let Some(filter) = node.attributes().find(|a| a.name() == "filter")
        {
            cut.push(filter.range());
        }
        if let Some(name) = node.attribute("data-button")
            && button_of(name).is_some()
            && node.attribute("id").is_none()
        {
            let at = node.range().start + 1 + node.tag_name().name().len();
            ids.push((at, format!(" id=\"{}\"", button_id(name))));
        }
    }
    cut.sort_by_key(|range| range.start);
    let mut text = String::with_capacity(source.len());
    let mut at = 0;
    let mut ids = ids.into_iter().peekable();
    for range in cut {
        if range.start < at {
            // Inside an element already cut.
            continue;
        }
        while let Some((place, id)) = ids.next_if(|(place, _)| *place <= range.start) {
            if place >= at {
                text.push_str(&source[at..place]);
                text.push_str(&id);
                at = place;
            }
        }
        text.push_str(&source[at..range.start]);
        at = range.end;
    }
    for (place, id) in ids {
        if place >= at {
            text.push_str(&source[at..place]);
            text.push_str(&id);
            at = place;
        }
    }
    text.push_str(&source[at..]);
    text
}

/// The buttons an illustration draws that the game has, with their
/// `data-button`.
fn buttons(document: &roxmltree::Document) -> Vec<(GamepadButton, String)> {
    let mut found: Vec<(GamepadButton, String)> = Vec::new();
    for node in document.descendants() {
        if let Some(name) = node.attribute("data-button")
            && let Some(button) = button_of(name)
            && !found.iter().any(|(b, _)| *b == button)
        {
            found.push((button, name.to_string()));
        }
    }
    found
}

/// Where each trigger goes, in the illustration's units.
fn triggers(document: &roxmltree::Document) -> Vec<(GamepadButton, [f32; 4])> {
    document
        .descendants()
        .filter_map(|node| {
            let button = button_of(node.attribute("data-trigger")?)?;
            let number = |name: &str| node.attribute(name)?.parse::<f32>().ok();
            Some((
                button,
                [
                    number("data-x")?,
                    number("data-y")?,
                    number("data-width")?,
                    number("data-height")?,
                ],
            ))
        })
        .collect()
}

/// How the illustrations are read: their few words in the game's font.
fn options() -> usvg::Options<'static> {
    let mut fonts = fontdb::Database::new();
    fonts.load_font_data(crate::content::file(crate::vector_art::FONT_FILE).to_vec());
    let family = fonts
        .faces()
        .next()
        .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
        .unwrap_or_default();
    fonts.set_sans_serif_family(family.clone());
    usvg::Options {
        font_family: family,
        fontdb: Arc::new(fonts),
        ..default()
    }
}

/// Draws `node` where it is in the illustration, `scale` px a unit, on
/// `pixmap`, whose top left is at `origin` (px) on the whole controller.
fn draw_in_place(node: &usvg::Node, scale: f32, origin: (f32, f32), pixmap: &mut Pixmap) {
    let Some(bounds) = node.abs_layer_bounding_box() else {
        return;
    };
    // resvg draws a group with its own transform: the ones of the groups
    // it is in come first, and it moves the group's box to the origin.
    let parent = match node {
        usvg::Node::Group(group) => group
            .transform()
            .invert()
            .map_or(group.abs_transform(), |own| {
                group.abs_transform().pre_concat(own)
            }),
        other => other.abs_transform(),
    };
    let transform = Transform::from_scale(scale, scale)
        .post_translate(-origin.0, -origin.1)
        .pre_concat(parent)
        .pre_translate(bounds.x(), bounds.y());
    resvg::render_node(node, transform, &mut pixmap.as_mut());
}

/// A button lit on its own, from its shapes and its marks drawn apart: the
/// shapes white, the marks dark on them. `None` if nothing is drawn.
fn lit(shapes: &Pixmap, marks: &Pixmap) -> Option<Vec<u8>> {
    const INK: f32 = 40.0;
    let mut drawn = false;
    let data = shapes
        .pixels()
        .iter()
        .zip(marks.pixels())
        .flat_map(|(shape, mark)| {
            let mark = mark.alpha() as f32 / 255.0;
            let alpha = shape.alpha().max((mark * 255.0).round() as u8);
            drawn |= alpha > 0;
            let gray = (255.0 + (INK - 255.0) * mark).round() as u8;
            [gray, gray, gray, alpha]
        })
        .collect();
    drawn.then_some(data)
}

/// A pixmap's pixels as straight (not premultiplied) RGBA.
fn straight(pixmap: &Pixmap) -> Vec<u8> {
    pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect()
}

/// An image drawn: its size in pixels and its pixels, straight RGBA.
struct Pixels {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Pixels {
    fn image(self) -> Image {
        Image::new(
            Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.data,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        )
    }
}

/// A gamepad drawn, before it is made images of (on the main thread).
struct Drawing {
    size: Vec2,
    whole: Pixels,
    buttons: Vec<(GamepadButton, Pixels, Rect)>,
    triggers: Vec<(GamepadButton, Rect)>,
}

impl Drawing {
    fn into_images(self, images: &mut Assets<Image>) -> DrawnPad {
        DrawnPad {
            size: self.size,
            whole: images.add(self.whole.image()),
            buttons: self
                .buttons
                .into_iter()
                .map(|(button, pixels, rect)| (button, images.add(pixels.image()), rect))
                .collect(),
            triggers: self.triggers,
        }
    }
}

/// A gamepad drawn for the screen.
#[derive(Clone, Debug)]
pub struct DrawnPad {
    /// Its size on the screen (logical px).
    pub size: Vec2,
    pub whole: Handle<Image>,
    /// Each button, white with its marks dark, and where it goes (logical
    /// px from the top left of the controller).
    pub buttons: Vec<(GamepadButton, Handle<Image>, Rect)>,
    /// Where each trigger goes (logical px).
    pub triggers: Vec<(GamepadButton, Rect)>,
}

/// An illustration at a size: whether its face buttons' letters are left
/// out, and its height in pixels.
type Key = (PadArt, bool, u32);

/// The gamepads drawn so far, and the ones being drawn. Drawing one takes
/// a tenth of a second or so (most of it their shadows), so it is done in
/// the background, not to hold a frame up.
#[derive(Resource, Default)]
pub struct PadArts {
    drawn: HashMap<Key, DrawnPad>,
    drawing: HashMap<Key, Task<Drawing>>,
}

impl PadArts {
    /// `art` drawn `height` logical px tall at `scale_factor` pixels each:
    /// `None` while it is being drawn, which this starts if it was not.
    pub fn get(
        &mut self,
        art: PadArt,
        unlabelled: bool,
        height: f32,
        scale_factor: f32,
    ) -> Option<&DrawnPad> {
        let pixels = (height * scale_factor).round().max(1.0) as u32;
        let key = (art, unlabelled, pixels);
        if !self.drawn.contains_key(&key) && !self.drawing.contains_key(&key) {
            let task = AsyncComputeTaskPool::get()
                .spawn(async move { draw(art, unlabelled, pixels, scale_factor) });
            self.drawing.insert(key, task);
        }
        self.drawn.get(&key)
    }

    /// Makes images of the gamepads drawn in the background since.
    fn finish(&mut self, images: &mut Assets<Image>) {
        let mut done = Vec::new();
        for (key, task) in &mut self.drawing {
            if let Some(drawing) = block_on(poll_once(task)) {
                done.push((*key, drawing));
            }
        }
        for (key, drawing) in done {
            self.drawing.remove(&key);
            self.drawn.insert(key, drawing.into_images(images));
        }
    }
}

/// Puts the gamepads drawn in the background on the screen's images.
pub fn finish_pad_arts(mut arts: ResMut<PadArts>, mut images: ResMut<Assets<Image>>) {
    if !arts.drawing.is_empty() {
        arts.finish(&mut images);
    }
}

/// Whether `node` is a group with nothing in it.
fn is_empty_group(node: &usvg::Node) -> bool {
    matches!(node, usvg::Node::Group(group) if !group.has_children())
}

/// Draws `art` `pixels` tall, at `scale_factor` pixels a logical one.
fn draw(art: PadArt, unlabelled: bool, pixels: u32, scale_factor: f32) -> Drawing {
    let document =
        roxmltree::Document::parse(art.svg()).expect("the pad illustrations are valid SVG");
    let [left, top, view_width, view_height] = art.view_box();
    let scale = pixels as f32 / view_height;
    let width = (view_width * scale).round() as u32;
    let opt = options();
    let read = |reading| {
        usvg::Tree::from_str(&prepared(&document, reading, unlabelled), &opt)
            .expect("the pad illustrations are valid SVG")
    };
    let mut whole = Pixmap::new(width, pixels).expect("a pad is drawn some pixels big");
    resvg::render(
        &read(Reading::Whole),
        Transform::from_scale(scale, scale),
        &mut whole.as_mut(),
    );
    let whole = Pixels {
        width,
        height: pixels,
        data: straight(&whole),
    };
    let (shapes, marks) = (read(Reading::Shapes), read(Reading::Marks));
    let logical = |px: f32| px / scale_factor;
    let buttons = buttons(&document)
        .into_iter()
        .filter_map(|(button, name)| {
            let id = button_id(&name);
            // A group left empty (its marks, without the letters) has a
            // box at the corner of the drawing: none.
            let nodes = [shapes.node_by_id(&id), marks.node_by_id(&id)]
                .map(|node| node.filter(|node| !is_empty_group(node)));
            // The box of the button on the controller (px), both parts in.
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for bounds in nodes
                .iter()
                .flatten()
                .filter_map(|n| n.abs_layer_bounding_box())
            {
                x0 = x0.min(bounds.left() * scale);
                y0 = y0.min(bounds.top() * scale);
                x1 = x1.max(bounds.right() * scale);
                y1 = y1.max(bounds.bottom() * scale);
            }
            let (x0, y0) = ((x0.floor() - 1.0).max(0.0), (y0.floor() - 1.0).max(0.0));
            let (x1, y1) = (
                (x1.ceil() + 1.0).min(width as f32),
                (y1.ceil() + 1.0).min(pixels as f32),
            );
            let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
            let mut parts = [Pixmap::new(w, h)?, Pixmap::new(w, h)?];
            for (node, pixmap) in nodes.iter().zip(&mut parts) {
                if let Some(node) = node {
                    draw_in_place(node, scale, (x0, y0), pixmap);
                }
            }
            let data = lit(&parts[0], &parts[1])?;
            let rect = Rect::new(logical(x0), logical(y0), logical(x1), logical(y1));
            Some((
                button,
                Pixels {
                    width: w,
                    height: h,
                    data,
                },
                rect,
            ))
        })
        .collect();
    let units = |value: f32| logical(value * scale);
    let triggers = triggers(&document)
        .into_iter()
        .map(|(button, [x, y, w, h])| {
            let (x, y) = (units(x - left), units(y - top));
            (button, Rect::new(x, y, x + units(w), y + units(h)))
        })
        .collect();
    Drawing {
        size: Vec2::new(logical(width as f32), logical(pixels as f32)),
        whole,
        buttons,
        triggers,
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use bevy::tasks::TaskPool;

    use super::*;

    const ARTS: [PadArt; 4] = [
        PadArt::DualSense,
        PadArt::DualShock4,
        PadArt::DualShock3,
        PadArt::Xbox,
    ];

    #[test]
    fn each_pad_gets_its_illustration() {
        let ps = ActiveDevice::PlayStation;
        assert_eq!(
            PadArt::of(ps, Some("DualSense Wireless Controller")),
            PadArt::DualSense
        );
        assert_eq!(
            PadArt::of(ps, Some("Wireless Controller")),
            PadArt::DualShock4
        );
        assert_eq!(PadArt::of(ps, Some("PS4 Controller")), PadArt::DualShock4);
        assert_eq!(
            PadArt::of(ps, Some("Sony PLAYSTATION(R)3 Controller")),
            PadArt::DualShock3
        );
        assert_eq!(PadArt::of(ps, None), PadArt::DualSense);
        assert_eq!(PadArt::of(ActiveDevice::Xbox, None), PadArt::Xbox);
        assert_eq!(
            PadArt::of(ActiveDevice::Switch, Some("Pro Controller")),
            PadArt::Xbox
        );
    }

    #[test]
    fn every_illustration_has_the_buttons_of_the_game_and_reads_each_way() {
        use GamepadButton as B;
        let opt = options();
        for art in ARTS {
            let document = roxmltree::Document::parse(art.svg()).unwrap();
            let found: Vec<GamepadButton> = buttons(&document).iter().map(|(b, _)| *b).collect();
            for button in [
                B::South,
                B::East,
                B::West,
                B::North,
                B::LeftTrigger,
                B::RightTrigger,
                B::Select,
                B::Start,
                B::Mode,
                B::LeftThumb,
                B::RightThumb,
                B::DPadUp,
                B::DPadDown,
                B::DPadLeft,
                B::DPadRight,
            ] {
                assert!(found.contains(&button), "{art:?}: {button:?}");
            }
            let found: Vec<GamepadButton> = triggers(&document).iter().map(|(b, _)| *b).collect();
            assert_eq!(found, [B::LeftTrigger2, B::RightTrigger2], "{art:?}");
            // Each reading is valid SVG with every button group in it.
            for reading in [Reading::Whole, Reading::Shapes, Reading::Marks] {
                let text = prepared(&document, reading, false);
                let tree = usvg::Tree::from_str(&text, &opt).unwrap();
                for (_, name) in buttons(&document) {
                    assert!(
                        tree.node_by_id(&button_id(&name)).is_some(),
                        "{art:?} {reading:?}: {name}"
                    );
                }
                if reading != Reading::Whole {
                    assert!(!text.contains(" filter=\""), "{art:?} {reading:?}");
                }
            }
        }
    }

    #[test]
    fn the_printed_letters_go_when_another_pad_marks_them_otherwise() {
        let document = roxmltree::Document::parse(PadArt::Xbox.svg()).unwrap();
        assert!(prepared(&document, Reading::Whole, false).contains("data-label=\""));
        assert!(!prepared(&document, Reading::Whole, true).contains("data-label=\""));
        // And the controls no pad of the game has are left out.
        assert!(!prepared(&document, Reading::Whole, false).contains("\"left_paddle_1\""));
    }

    #[test]
    fn every_illustration_is_drawn_a_light_controller_on_its_shadow() {
        let opt = options();
        for art in ARTS {
            let document = roxmltree::Document::parse(art.svg()).unwrap();
            let [_, _, view_width, view_height] = art.view_box();
            let height = 160;
            let scale = height as f32 / view_height;
            let width = (view_width * scale).round() as u32;
            let tree =
                usvg::Tree::from_str(&prepared(&document, Reading::Whole, false), &opt).unwrap();
            let mut pixmap = Pixmap::new(width, height).unwrap();
            resvg::render(
                &tree,
                Transform::from_scale(scale, scale),
                &mut pixmap.as_mut(),
            );
            let pixels = pixmap.pixels();
            let opaque =
                pixels.iter().filter(|p| p.alpha() > 230).count() as f32 / pixels.len() as f32;
            assert!((0.25..0.9).contains(&opaque), "{art:?}: {opaque}");
            let lightest = pixels
                .iter()
                .filter(|p| p.alpha() == 255)
                .map(|p| p.red() as u32 + p.green() as u32 + p.blue() as u32)
                .max()
                .unwrap_or(0);
            assert!(lightest > 660, "{art:?}: {lightest}");
        }
    }

    #[test]
    fn the_size_of_an_illustration_is_read_from_its_view() {
        for art in ARTS {
            let document = roxmltree::Document::parse(art.svg()).unwrap();
            let view: Vec<f32> = document
                .root_element()
                .attribute("viewBox")
                .unwrap()
                .split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect();
            assert_eq!(art.view_box().to_vec(), view, "{art:?}");
            let size = art.size(100.0);
            assert_eq!(size.y, 100.0);
            assert!((size.x - view[2] / view[3] * 100.0).abs() < 1e-3);
        }
    }

    #[test]
    fn the_face_buttons_are_where_they_are_with_their_letters_or_without() {
        // Without the letters (a Switch pad's are put on it apart), the
        // buttons' marks are none: their boxes are the buttons' alone, not
        // stretched to the corner of the drawing.
        let rects = |unlabelled| {
            let drawn = draw(PadArt::Xbox, unlabelled, 270, 1.0);
            drawn
                .buttons
                .into_iter()
                .filter(|(button, ..)| {
                    matches!(
                        button,
                        GamepadButton::South
                            | GamepadButton::East
                            | GamepadButton::West
                            | GamepadButton::North
                    )
                })
                .map(|(button, _, rect)| (button, rect))
                .collect::<Vec<_>>()
        };
        let (labelled, unlabelled) = (rects(false), rects(true));
        assert_eq!(labelled.len(), 4);
        for ((button, with), (_, without)) in labelled.iter().zip(&unlabelled) {
            assert!(
                with.center().distance(without.center()) < 2.0
                    && (with.width() - without.width()).abs() < 3.0,
                "{button:?}: {with:?} and {without:?}"
            );
        }
    }

    #[test]
    fn a_pad_is_drawn_with_each_button_on_it() {
        let drawn = draw(PadArt::DualSense, false, 120, 1.0);
        assert!((drawn.size.y - 120.0).abs() < 0.5);
        assert!((drawn.size.x - PadArt::DualSense.size(120.0).x).abs() < 1.0);
        assert_eq!(drawn.buttons.len(), 15);
        let pad = Rect::new(0.0, 0.0, drawn.size.x, drawn.size.y);
        for (button, pixels, rect) in &drawn.buttons {
            assert!(rect.width() > 1.0 && rect.height() > 1.0, "{button:?}");
            assert!(
                pad.contains(rect.min) && pad.contains(rect.max),
                "{button:?}: {rect:?}"
            );
            assert_eq!(
                pixels.data.len(),
                (pixels.width * pixels.height * 4) as usize
            );
        }
        let at = |button| {
            drawn
                .buttons
                .iter()
                .find(|(b, ..)| *b == button)
                .map(|(_, _, rect)| rect.center())
                .unwrap()
        };
        // The face buttons on the right, the D-pad on the left; the sticks
        // (in groups moved into place) where they are.
        assert!(at(GamepadButton::South).x > at(GamepadButton::DPadDown).x);
        assert!(at(GamepadButton::North).y < at(GamepadButton::South).y);
        assert!(at(GamepadButton::DPadUp).y < at(GamepadButton::DPadDown).y);
        assert!(at(GamepadButton::LeftThumb).x < at(GamepadButton::RightThumb).x);
        assert!(at(GamepadButton::LeftThumb).y > at(GamepadButton::DPadUp).y);
        // A button is white where it is, its mark dark on it.
        let (_, south, _) = drawn
            .buttons
            .iter()
            .find(|(b, ..)| *b == GamepadButton::South)
            .unwrap();
        assert!(south.data.chunks(4).any(|p| p[3] == 255 && p[0] == 255));
        assert!(south.data.chunks(4).any(|p| p[3] > 200 && p[0] < 120));
    }

    #[test]
    fn a_pad_is_drawn_in_the_background_once_for_each_size() {
        AsyncComputeTaskPool::get_or_init(TaskPool::default);
        let mut images = Assets::<Image>::default();
        let mut arts = PadArts::default();
        assert!(arts.get(PadArt::Xbox, false, 60.0, 2.0).is_none());
        assert_eq!(arts.drawing.len(), 1);
        // Asked again while it is being drawn: not drawn twice.
        assert!(arts.get(PadArt::Xbox, false, 60.0, 2.0).is_none());
        assert_eq!(arts.drawing.len(), 1);
        let waited = std::time::Instant::now();
        while !arts.drawing.is_empty() {
            assert!(waited.elapsed().as_secs() < 60, "never drawn");
            std::thread::sleep(std::time::Duration::from_millis(5));
            arts.finish(&mut images);
        }
        let drawn = arts.get(PadArt::Xbox, false, 60.0, 2.0).unwrap().clone();
        // 120 pixels tall, 60 logical px.
        assert!((drawn.size.y - 60.0).abs() < 0.5);
        assert_eq!(images.get(&drawn.whole).unwrap().height(), 120);
        assert_eq!(images.len(), 1 + drawn.buttons.len());
        // Another scale factor, another drawing.
        assert!(arts.get(PadArt::Xbox, false, 60.0, 1.0).is_none());
        assert_eq!(arts.drawing.len(), 1);
    }
}
