//! Plano de la pista: el trazado visto desde arriba (el norte, -Z, arriba),
//! dibujado a partir de la línea central cada vez que cambia la pista. Lo
//! usan la pantalla de elegir pista y el minimapa de la carrera, abajo a la
//! izquierda, con un punto por moto del color de cada una (with two players,
//! one map for each, bottom left of their half of the screen). Over the
//! race's map, if Options > Graphics > Performance HUD says so, where its
//! player is: how far round the lap and the coordinates of the ground, to
//! point a place of the track out.

use bevy::{
    asset::RenderAssetUsages,
    platform::collections::HashMap,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::bike::{BikeColor, BikeState, LocalPlayer, Player};
use crate::circuits::{Circuits, CurrentCircuit};
use crate::hud::UiFont;
use crate::locale::Language;
use crate::menu::GameState;
use crate::settings::Settings;
use crate::track::Track;
use crate::ui::text_font;
use crate::welcome::GameMode;

/// Lado del plano (píxeles) y margen alrededor del trazado.
pub const MAP_SIZE: u32 = 256;
const MAP_MARGIN: f32 = 18.0;
/// Grosor del trazado y de su borde (px).
const MAP_ROAD: f32 = 3.0;
const MAP_OUTLINE: f32 = 5.5;
/// How much thinner the roads off the lap are drawn (px).
const MAP_SIDE_ROAD: f32 = 1.0;

struct MarkerStyle {
    half_len: f32,
    radius: f32,
    color: [f32; 4],
}

/// Marca de la meta (línea amarilla) y de los checkpoints (líneas rojas más finas).
const START_STYLE: MarkerStyle = MarkerStyle {
    half_len: 5.5,
    radius: 1.6,
    color: [1.0, 0.85, 0.1, 1.0],
};

const CHECKPOINT_STYLE: MarkerStyle = MarkerStyle {
    half_len: 4.8,
    radius: 0.9,
    color: [0.95, 0.15, 0.15, 1.0],
};

/// Lado del minimapa de la carrera en pantalla y de los puntos de las motos
/// (px): el del jugador, más grande y con borde blanco. With two players,
/// each one's map in their half of the screen, a little smaller.
const HUD_SIZE: f32 = 190.0;
const SPLIT_HUD_SIZE: f32 = 160.0;
const RIVAL_DOT: f32 = 9.0;
const PLAYER_DOT: f32 = 14.0;
/// With two players their dots carry their number, so they are bigger: each
/// one's on their own map, and the other player's a little smaller.
const NUMBERED_DOT: f32 = 19.0;
const OTHER_PLAYER_DOT: f32 = 15.0;

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, create_map)
            .add_systems(Update, redraw_map)
            .add_systems(OnEnter(GameState::Race), spawn_hud_map)
            .add_systems(
                Update,
                (place_dots.after(redraw_map), show_coordinates).run_if(in_state(GameState::Race)),
            );
    }
}

/// El plano de la pista actual, siempre en la misma imagen: al cambiar de
/// pista se vuelve a dibujar encima.
#[derive(Resource)]
pub struct TrackMap {
    pub image: Handle<Image>,
    pub projection: Projection,
}

/// Cómo se pasa del suelo (m) al plano (px).
#[derive(Clone, Copy)]
pub struct Projection {
    pub min: Vec2,
    pub scale: f32,
    pub offset: Vec2,
}

impl Projection {
    pub fn new(track: &Track) -> Self {
        let (min, max) = track.bounds();
        let scale = (MAP_SIZE as f32 - 2.0 * MAP_MARGIN) / (max - min).max_element();
        let offset = (Vec2::splat(MAP_SIZE as f32) - (max - min) * scale) / 2.0;
        Self { min, scale, offset }
    }

    fn to_map(self, point: Vec2) -> Vec2 {
        (point - self.min) * self.scale + self.offset
    }
}

fn create_map(
    mut commands: Commands,
    circuit: Option<Res<CurrentCircuit>>,
    circuits: Option<Res<Circuits>>,
    track: Res<Track>,
    mut images: ResMut<Assets<Image>>,
) {
    let checkpoints = circuit
        .zip(circuits)
        .map(|(c, circuits)| circuits.get(*c).checkpoints())
        .unwrap_or_default();
    commands.insert_resource(TrackMap {
        image: images.add(draw(&track, &checkpoints)),
        projection: Projection::new(&track),
    });
}

fn redraw_map(
    circuit: Option<Res<CurrentCircuit>>,
    circuits: Option<Res<Circuits>>,
    track: Res<Track>,
    mut map: ResMut<TrackMap>,
    mut images: ResMut<Assets<Image>>,
) {
    let circuit_changed = circuit.as_ref().is_some_and(|c| c.is_changed());
    if (!track.is_changed() && !circuit_changed) || track.is_added() {
        return;
    }
    let checkpoints = circuit
        .zip(circuits)
        .map(|(c, circuits)| circuits.get(*c).checkpoints())
        .unwrap_or_default();
    map.projection = Projection::new(&track);
    images
        .insert(&map.image, draw(&track, &checkpoints))
        .expect("la imagen del plano sigue existiendo");
}

/// El trazado en blanco con un borde oscuro, el inicio con una línea amarilla
/// transversal y los checkpoints con líneas rojas más finas, sobre fondo
/// transparente.
pub fn draw(track: &Track, checkpoints: &[f32]) -> Image {
    let size = MAP_SIZE as usize;
    let mut pixels = vec![[0.0f32; 4]; size * size];
    let projection = Projection::new(track);
    let centerline = track.centerline();
    let outline = [0.02, 0.04, 0.1, 0.95];
    let road = [1.0, 1.0, 1.0, 1.0];

    // The lap, and thinner, the roads off it: all the outlines first, so
    // that where they meet the roads are one.
    for (radius, color) in [(MAP_OUTLINE, outline), (MAP_ROAD, road)] {
        for line in std::iter::once(track).chain(track.side_roads()) {
            let points = line.centerline();
            let thinner = if line.is_closed() { 0.0 } else { MAP_SIDE_ROAD };
            for index in 0..line.segment_count() {
                let next = points[(index + 1) % points.len()];
                let (a, b) = (projection.to_map(points[index]), projection.to_map(next));
                // Un círculo cada medio píxel a lo largo del tramo.
                let steps = (a.distance(b) * 2.0).ceil().max(1.0) as usize;
                for step in 0..steps {
                    let at = a.lerp(b, step as f32 / steps as f32);
                    stamp(&mut pixels, size, at, radius - thinner, color);
                }
            }
        }
    }

    // Checkpoints: líneas rojas más finas transversales a la pista.
    for &along in checkpoints {
        let index = track.index_at(along);
        let (cp_center, cp_tangent) = marker_tangent(centerline, index, projection);
        draw_track_marker(
            &mut pixels,
            size,
            cp_center,
            cp_tangent,
            CHECKPOINT_STYLE,
            outline,
        );
    }

    // Inicio: línea amarilla transversal a la pista.
    let (goal, start_tangent) = marker_tangent(centerline, 0, projection);
    draw_track_marker(&mut pixels, size, goal, start_tangent, START_STYLE, outline);

    let data = pixels
        .iter()
        .flat_map(|pixel| pixel.map(|channel| (channel * 255.0).round() as u8))
        .collect();
    Image::new(
        Extent3d {
            width: MAP_SIZE,
            height: MAP_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

fn marker_tangent(centerline: &[Vec2], index: usize, projection: Projection) -> (Vec2, Vec2) {
    let n = centerline.len();
    let center = projection.to_map(centerline[index % n]);
    let prev = projection.to_map(centerline[(index + n - 1) % n]);
    let next = projection.to_map(centerline[(index + 1) % n]);
    let tangent = (next - prev).normalize_or_zero();
    (center, tangent)
}

fn draw_track_marker(
    pixels: &mut [[f32; 4]],
    size: usize,
    center: Vec2,
    tangent: Vec2,
    style: MarkerStyle,
    outline: [f32; 4],
) {
    let normal = Vec2::new(-tangent.y, tangent.x);
    let a = center - normal * style.half_len;
    let b = center + normal * style.half_len;
    draw_line(pixels, size, a, b, style.radius + 1.0, outline);
    draw_line(pixels, size, a, b, style.radius, style.color);
}

fn draw_line(pixels: &mut [[f32; 4]], size: usize, a: Vec2, b: Vec2, radius: f32, color: [f32; 4]) {
    let steps = (a.distance(b) * 2.0).ceil().max(1.0) as usize;
    for step in 0..=steps {
        let at = a.lerp(b, step as f32 / steps as f32);
        stamp(pixels, size, at, radius, color);
    }
}

/// Pinta un círculo de `color` con el borde suavizado.
fn stamp(pixels: &mut [[f32; 4]], size: usize, center: Vec2, radius: f32, color: [f32; 4]) {
    let low = (center - radius - 1.0).max(Vec2::ZERO);
    let high = (center + radius + 1.0).min(Vec2::splat(size as f32 - 1.0));
    for y in low.y as usize..=high.y as usize {
        for x in low.x as usize..=high.x as usize {
            let pixel_center = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let cover = (radius + 0.5 - pixel_center.distance(center)).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let pixel = &mut pixels[y * size + x];
            for channel in 0..3 {
                pixel[channel] += (color[channel] - pixel[channel]) * cover;
            }
            pixel[3] = pixel[3].max(color[3] * cover);
        }
    }
}

/// A minimap of the race, bottom left of the screen, or of each player's
/// half of it with two players: whose it is (`None` with one player), and
/// how wide it is (px).
#[derive(Component)]
struct HudMap {
    viewer: Option<usize>,
    size: f32,
}

/// The dot of a bike on a minimap.
#[derive(Component)]
struct Dot {
    bike: Entity,
    map: Entity,
}

/// How a bike's dot is drawn on the map of `viewer` (with two players,
/// each one's): how wide (px), the number in it, and whether it is a
/// player's (a white border) and how far over the rest (z).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DotStyle {
    pub size: f32,
    pub number: Option<usize>,
    pub human: bool,
    pub z: i32,
}

/// With two players, each one's dot on their own map is the biggest and
/// over the rest, and the other player's a little smaller under it, each
/// with its number; with one, the player's is bigger than the rivals'.
pub fn dot_style(viewer: Option<usize>, player: bool, local: Option<usize>) -> DotStyle {
    match local {
        Some(index) if Some(index) == viewer || viewer.is_none() => DotStyle {
            size: NUMBERED_DOT,
            number: Some(index + 1),
            human: true,
            z: 2,
        },
        Some(index) => DotStyle {
            size: OTHER_PLAYER_DOT,
            number: Some(index + 1),
            human: true,
            z: 1,
        },
        None if player => DotStyle {
            size: PLAYER_DOT,
            number: None,
            human: true,
            z: 2,
        },
        None => DotStyle {
            size: RIVAL_DOT,
            number: None,
            human: false,
            z: 0,
        },
    }
}

/// Over a minimap, if the options say so, where its player is (see
/// [`Coordinates`]), as wide as the map: the container, of the map of
/// `viewer` (see [`HudMap`]), and each of its texts.
#[derive(Component)]
struct MapCoordinates {
    viewer: Option<usize>,
}

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum CoordinateText {
    Along,
    Road,
    X,
    Z,
    Y,
}

/// How big the coordinates are against the map's width: the widest line
/// ("X -1033", "Z -1033" and "Y 120", 10 of the font's sizes, with room
/// between them) nearly as wide as the map.
const COORDINATES_SIZE: f32 = 0.085;
/// How big how far round the lap is, over them: half as big again, twice
/// (so "8888 m" takes three quarters of the map's width).
const ALONG_SIZE: f32 = 2.25 * COORDINATES_SIZE;

/// Where a bike is on a track, in whole metres: how far round the lap (from
/// the line, as the circuits measure their stretches), on which road off it
/// if it is on one (see [`crate::track::Nearest::road`]), and the
/// coordinates of the ground (x, z, and how high, y).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coordinates {
    pub along: i32,
    pub road: usize,
    pub x: i32,
    pub z: i32,
    pub y: i32,
}

impl Coordinates {
    /// Those of a bike at `position` on the ground, `elevation` m up, on
    /// `track`.
    pub fn of(track: &Track, position: Vec2, elevation: f32) -> Self {
        let nearest = track.nearest_at(position, elevation);
        // Whole metres, with no "-0".
        let whole = |metres: f32| metres.round() as i32;
        Self {
            along: whole(track.along(&nearest)),
            road: nearest.road,
            x: whole(position.x),
            z: whole(position.y),
            y: whole(elevation),
        }
    }

    /// One of them as it is shown: "1234 m", "ramal 1" (nothing on the
    /// lap), "X 412", "Z -1033", "Y 12".
    fn said(&self, text: CoordinateText, lang: Language) -> String {
        match (text, self.road, lang) {
            (CoordinateText::Along, ..) => format!("{} m", self.along),
            (CoordinateText::Road, 0, _) => String::new(),
            (CoordinateText::Road, road, Language::Spanish) => format!("ramal {road}"),
            (CoordinateText::Road, road, Language::English) => format!("branch {road}"),
            (CoordinateText::X, ..) => format!("X {}", self.x),
            (CoordinateText::Z, ..) => format!("Z {}", self.z),
            (CoordinateText::Y, ..) => format!("Y {}", self.y),
        }
    }
}

/// Over each minimap, if the options say so, where its player is.
#[allow(clippy::type_complexity)]
fn show_coordinates(
    settings: Res<Settings>,
    track: Res<Track>,
    bikes: Query<(&BikeState, Has<Player>, Option<&LocalPlayer>)>,
    mut boxes: Query<(&MapCoordinates, &mut Node, &Children)>,
    rows: Query<&Children, Without<MapCoordinates>>,
    mut texts: Query<(&CoordinateText, &mut Text)>,
) {
    for (label, mut node, children) in &mut boxes {
        let display = if settings.show_coordinates {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if !settings.show_coordinates {
            continue;
        }
        let viewer = label.viewer.unwrap_or(0);
        let Some((state, ..)) = bikes.iter().find(|(_, player, local)| {
            local.map_or(usize::from(!*player), |local| local.0) == viewer
        }) else {
            continue;
        };
        let coordinates = Coordinates::of(&track, state.position, state.elevation);
        for row in children.iter().filter_map(|row| rows.get(row).ok()) {
            for entity in row.iter() {
                let Ok((kind, mut text)) = texts.get_mut(entity) else {
                    continue;
                };
                let wanted = coordinates.said(*kind, settings.language);
                if text.0 != wanted {
                    text.0 = wanted;
                }
            }
        }
    }
}

fn spawn_hud_map(
    mut commands: Commands,
    map: Res<TrackMap>,
    game_mode: Res<GameMode>,
    font: Res<UiFont>,
) {
    // Bottom left: of the screen, or of each player's half, over the
    // bottom of player 1's half and the bottom of the screen.
    let maps: Vec<(Option<usize>, f32, Val)> = if *game_mode == GameMode::SplitScreen {
        vec![
            (Some(0), SPLIT_HUD_SIZE, percent(50.0)),
            (Some(1), SPLIT_HUD_SIZE, px(0.0)),
        ]
    } else {
        vec![(None, HUD_SIZE, px(0.0))]
    };
    for (viewer, size, bottom) in maps {
        commands.spawn((
            HudMap { viewer, size },
            DespawnOnExit(GameState::Race),
            Node {
                position_type: PositionType::Absolute,
                left: px(16),
                bottom,
                margin: UiRect::bottom(px(16)),
                width: px(size),
                height: px(size),
                border_radius: BorderRadius::all(px(14)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.2, 0.25, 0.35, 0.4)),
            GlobalZIndex(10),
            BackgroundColor(Color::srgba(0.02, 0.04, 0.1, 0.65)),
            children![(
                ImageNode::new(map.image.clone()),
                Node {
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
            ),],
        ));
        // Just above the map, as wide as it: the branch, if on one; under
        // it how far round the lap, bigger; and under them x, z and y from
        // one edge to the other.
        let text = |kind, share: f32| {
            (
                kind,
                Text::new(""),
                text_font(&font, size * share),
                TextColor(Color::WHITE),
                TextShadow {
                    offset: Vec2::splat(1.0),
                    color: Color::srgba(0.0, 0.0, 0.0, 0.9),
                },
            )
        };
        let row = || Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        };
        commands
            .spawn((
                MapCoordinates { viewer },
                DespawnOnExit(GameState::Race),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(16),
                    bottom,
                    margin: UiRect::bottom(px(16.0 + size + 4.0)),
                    width: px(size),
                    flex_direction: FlexDirection::Column,
                    display: Display::None,
                    ..default()
                },
                GlobalZIndex(10),
            ))
            .with_children(|coordinates| {
                coordinates.spawn(row()).with_children(|row| {
                    row.spawn(text(CoordinateText::Road, COORDINATES_SIZE));
                });
                coordinates.spawn(row()).with_children(|row| {
                    row.spawn(text(CoordinateText::Along, ALONG_SIZE));
                });
                coordinates.spawn(row()).with_children(|row| {
                    for kind in [CoordinateText::X, CoordinateText::Z, CoordinateText::Y] {
                        row.spawn(text(kind, COORDINATES_SIZE));
                    }
                });
            });
    }
}

/// Un punto por moto en cada minimapa, donde está, del color de la moto;
/// the player's (on their own map) over the rest. Each dot is made the
/// first time its bike is seen.
#[allow(clippy::type_complexity)]
fn place_dots(
    mut commands: Commands,
    map: Res<TrackMap>,
    font: Res<UiFont>,
    huds: Query<(Entity, &HudMap)>,
    bikes: Query<(
        Entity,
        &BikeState,
        &BikeColor,
        Has<Player>,
        Option<&LocalPlayer>,
    )>,
    mut dots: Query<(&Dot, &mut Node)>,
) {
    let mut placed: HashMap<(Entity, Entity), Mut<Node>> = dots
        .iter_mut()
        .map(|(dot, node)| ((dot.map, dot.bike), node))
        .collect();
    for (hud, hud_map) in &huds {
        for (bike, state, color, player, local) in &bikes {
            let style = dot_style(hud_map.viewer, player, local.map(|local| local.0));
            let at = map.projection.to_map(state.position) / MAP_SIZE as f32 * hud_map.size
                - style.size / 2.0;
            match placed.get_mut(&(hud, bike)) {
                Some(node) => {
                    // Solo si se ha movido: tocar el nodo obliga a recolocarlo.
                    if node.left != px(at.x) || node.top != px(at.y) {
                        node.left = px(at.x);
                        node.top = px(at.y);
                    }
                }
                None => {
                    let dot = commands
                        .spawn((
                            Dot { bike, map: hud },
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(at.x),
                                top: px(at.y),
                                width: px(style.size),
                                height: px(style.size),
                                border: UiRect::all(px(if style.human { 2.5 } else { 1.0 })),
                                border_radius: BorderRadius::MAX,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                            BackgroundColor(color.0),
                            BorderColor::all(if style.human {
                                Color::WHITE
                            } else {
                                Color::srgba(0.0, 0.0, 0.0, 0.8)
                            }),
                            ZIndex(style.z),
                        ))
                        .id();
                    if let Some(number) = style.number {
                        commands.entity(dot).with_child((
                            Text::new(number.to_string()),
                            text_font(&font, style.size * 0.63),
                            TextColor(Color::WHITE),
                            TextShadow {
                                offset: Vec2::splat(1.0),
                                color: Color::srgba(0.0, 0.0, 0.0, 0.9),
                            },
                        ));
                    }
                    commands.entity(hud).add_child(dot);
                }
            }
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn the_map_shows_the_whole_track_inside_its_margins() {
        let image = draw(&Track::default(), &[780.0]);
        let data = image.data.as_ref().unwrap();
        let size = MAP_SIZE as usize;
        let alpha = |x: usize, y: usize| data[(y * size + x) * 4 + 3];
        // Hay trazado dibujado...
        let drawn = (0..size * size).filter(|i| data[i * 4 + 3] > 200).count();
        assert!(drawn > size * 4, "{drawn} píxeles");
        // ...y no toca los bordes.
        for i in 0..size {
            for (x, y) in [(i, 0), (i, size - 1), (0, i), (size - 1, i)] {
                assert_eq!(alpha(x, y), 0, "borde en ({x}, {y})");
            }
        }
    }

    #[test]
    fn the_map_draws_start_line_and_checkpoints() {
        let track = Track::default();
        let image = draw(&track, &[780.0]);
        let data = image.data.as_ref().unwrap();
        let size = MAP_SIZE as usize;

        let mut yellow_pixels = 0;
        let mut red_pixels = 0;

        for i in 0..(size * size) {
            let r = data[i * 4];
            let g = data[i * 4 + 1];
            let b = data[i * 4 + 2];
            let a = data[i * 4 + 3];

            if a > 150 {
                // Línea de inicio amarilla: rojo y verde altos, azul bajo.
                if r > 180 && g > 150 && b < 100 {
                    yellow_pixels += 1;
                }
                // Checkpoint rojo: rojo alto, verde y azul bajos.
                if r > 180 && g < 100 && b < 100 {
                    red_pixels += 1;
                }
            }
        }

        assert!(
            yellow_pixels > 0,
            "Debe haber píxeles amarillos para la línea de inicio ({yellow_pixels})"
        );
        assert!(
            red_pixels > 0,
            "Debe haber píxeles rojos para los checkpoints ({red_pixels})"
        );
    }

    #[test]
    fn each_player_stands_out_on_their_own_map() {
        // Two players: each one's dot the biggest and on top on their map,
        // the other's smaller under it, both numbered.
        for viewer in [0, 1] {
            let own = dot_style(Some(viewer), false, Some(viewer));
            let other = dot_style(Some(viewer), false, Some(1 - viewer));
            assert_eq!(own.number, Some(viewer + 1));
            assert_eq!(other.number, Some(2 - viewer));
            assert!(own.size > other.size && own.z > other.z);
            let rival = dot_style(Some(viewer), false, None);
            assert!(other.size > rival.size && other.z > rival.z);
            assert!(!rival.human && rival.number.is_none());
        }
        // One player: theirs bigger than the rivals', with no number.
        let player = dot_style(None, true, None);
        assert!(player.human && player.number.is_none());
        assert!(player.size > dot_style(None, false, None).size);
    }

    #[test]
    fn the_map_says_how_far_round_the_lap_and_where_on_the_ground() {
        let track = Track::default();
        let index = track.centerline().len() / 3;
        let point = track.centerline()[index];
        let along = index as f32 * track.step_length();
        let at = Coordinates::of(&track, point, 12.4);
        assert!((at.along as f32 - along).abs() <= 1.0, "{at:?}");
        assert_eq!(at.road, 0);
        let whole = |metres: f32| metres.round() as i32;
        assert_eq!((at.x, at.z, at.y), (whole(point.x), whole(point.y), 12));
        let said = |kind| at.said(kind, Language::Spanish);
        assert_eq!(said(CoordinateText::Along), format!("{} m", at.along));
        assert_eq!(said(CoordinateText::Road), "");
        assert_eq!(said(CoordinateText::Z), format!("Z {}", at.z));
        // A branch named; never "-0".
        let branch = Coordinates { road: 1, ..at };
        assert_eq!(
            branch.said(CoordinateText::Road, Language::Spanish),
            "ramal 1"
        );
        assert_eq!(
            branch.said(CoordinateText::Road, Language::English),
            "branch 1"
        );
        let near_zero = Coordinates::of(&track, Vec2::new(-0.3, point.y), 0.0);
        assert_eq!(near_zero.said(CoordinateText::X, Language::English), "X 0");
        // The widest line nearly as wide as the map at its size, with room
        // between its three: in Russo One, "X -1033", "Z -1033" and "Y 120"
        // are 10.035 of its size wide.
        let widest = 10.035 * COORDINATES_SIZE;
        assert!((0.8..0.95).contains(&widest), "{widest}");
        // How far round the lap, half as big again twice, on a line of its
        // own (the branch over it), fits too: "8888 m" is 3.905 of its size
        // wide.
        assert_eq!(ALONG_SIZE, 2.25 * COORDINATES_SIZE);
        let along = 3.905 * ALONG_SIZE;
        assert!((0.7..0.8).contains(&along), "{along}");
    }

    #[test]
    fn the_projection_keeps_the_track_inside_the_map() {
        let track = Track::default();
        let projection = Projection::new(&track);
        for &point in track.centerline() {
            let at = projection.to_map(point);
            assert!(at.cmpge(Vec2::splat(MAP_MARGIN - 0.5)).all());
            assert!(
                at.cmple(Vec2::splat(MAP_SIZE as f32 - MAP_MARGIN + 0.5))
                    .all()
            );
        }
    }
}
