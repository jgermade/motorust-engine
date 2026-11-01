//! Decorado: farolas, árboles (sueltos y en bosques), palmeras, casas,
//! hoteles altos de colores, un gran hotel de los de antes y boyas en el
//! mar, según el escenario de cada pista ([`Scenery`]). Todo se coloca al
//! azar, pero siempre igual, con [`random01`].

use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::mesh::{Indices, VertexAttributeValues};
use bevy::prelude::*;

use super::Assets3d;
use super::flowers::FlowerPlan;
use super::landmarks;
use super::mirror::MirrorPoint;
use super::quads::MeshBuilder;
use super::scenery::{
    Boat, BoatKind, Grandstand, HouseStyle, Houses, Lighthouse, ParasolCluster, Row, Scenery,
    Species, span,
};
use super::terrain::{Ground, SEA_LEVEL, Terrain};
use super::textures::WorldTextures;
use super::themes::Stage;
use super::trees::{LEVELS, Tree, TreeModel, lighter};
use crate::geometry::{heading_of, random01, smoothstep, value_noise};
use crate::shapes::{between, tube};
use crate::track::{Track, WALL_DISTANCE};

const LAMP_HEIGHT: f32 = 7.6;
/// Los árboles se reparten hasta esta distancia del circuito.
const TREE_MARGIN: f32 = 280.0;
const TREE_CANDIDATES: u32 = 32_000;
const MAX_TREES: usize = 2_200;
const HOUSE_SPACING: f32 = 40.0;
const FLOOR_HEIGHT: f32 = 3.1;
/// En los bosques, cuántos árboles en fondo a cada lado (one every
/// `Forest::every` metres along the road in each row).
const FOREST_DEPTH: usize = 4;

/// Un sitio ocupado por un edificio: su centro y su radio.
pub type Footprint = (Vec2, f32);

pub fn spawn_props(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    trees: &TreePlan,
    flowers: FlowerPlan,
    assets: &mut Assets3d,
) {
    let mut occupied: Vec<Footprint> = Vec::new();
    for grandstand in scenery.grandstands {
        occupied.push(spawn_grandstand(world, track, terrain, grandstand, assets));
    }
    for row in scenery.lamps {
        spawn_lamps(world, track, terrain, scenery, row, assets);
    }
    if let Some((along, lateral)) = scenery.grand_hotel {
        occupied.push(spawn_grand_hotel(
            world, track, terrain, along, lateral, assets,
        ));
    }
    for row in scenery.hotels {
        occupied.extend(spawn_hotels(world, track, terrain, row, assets));
    }
    // The circuit's flowers, for a theme to put among its own.
    assets.flowers = Some(flowers);
    // The portals between the realms, then what the circuit's themes put
    // by the road (see `super::themes`).
    occupied.extend(super::portals::spawn_portals(
        world,
        track,
        terrain,
        scenery.portals,
        assets,
    ));
    spawn_themes(
        world,
        track,
        terrain,
        scenery,
        Stage::BeforeHouses,
        &mut occupied,
        assets,
    );
    super::petals::spawn_petals(world, track, terrain, scenery.petals, assets);
    // The flowers no theme put among its own, clear of all put so far.
    if let Some(flowers) = assets.flowers.take() {
        let meadows = super::flowers::spawn_flowers(
            world,
            terrain,
            scenery.flowers,
            flowers,
            &occupied,
            assets,
        );
        occupied.extend(meadows);
    }
    let houses = place_houses(track, terrain, &scenery.houses, &occupied);
    for house in &houses {
        spawn_house(
            world,
            house,
            scenery.houses.style,
            assets.textures,
            assets.meshes,
            assets.materials,
        );
        occupied.push((house.position, house.width.max(house.depth)));
    }
    for row in scenery.palms {
        occupied.extend(spawn_palms(world, track, terrain, row, assets));
    }
    occupied.extend(landmarks::spawn_ridden_loops(
        world,
        track,
        scenery.curbs,
        scenery.soil == super::scenery::Soil::Floor,
        assets,
    ));
    super::boosters::spawn_boosters(world, track, assets);
    occupied.extend(landmarks::spawn_loops(
        world,
        track,
        terrain,
        scenery.loops,
        assets,
    ));
    occupied.extend(landmarks::spawn_totems(
        world,
        track,
        terrain,
        scenery.totems,
        assets,
    ));
    occupied.extend(landmarks::spawn_sunflowers(
        world,
        track,
        terrain,
        scenery.sunflowers,
        assets,
    ));
    spawn_themes(
        world,
        track,
        terrain,
        scenery,
        Stage::AfterHouses,
        &mut occupied,
        assets,
    );
    landmarks::spawn_rings(world, track, scenery.rings, assets);
    landmarks::spawn_waterfalls(world, track, terrain, scenery.waterfalls, assets);
    for lighthouse in scenery.lighthouses {
        spawn_lighthouse(world, terrain, lighthouse, assets);
        occupied.push(lighthouse_clearing(lighthouse));
    }
    spawn_trees(world, trees, &occupied, assets);
    spawn_buoys(world, scenery.buoys, assets);
    for boat in scenery.boats {
        spawn_boat(world, boat, assets);
    }
    for cluster in scenery.beach_parasols {
        spawn_parasol_cluster(world, terrain, cluster, assets);
    }
}

/// Puts in the world what the circuit's themes put at `stage`, and the
/// ground it takes among what is `occupied`.
fn spawn_themes(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    stage: Stage,
    occupied: &mut Vec<Footprint>,
    assets: &mut Assets3d,
) {
    for (_, theme) in scenery.themes.iter() {
        if theme.stage() == stage {
            occupied.extend(theme.spawn(world, track, terrain, scenery, assets));
        }
    }
}

/// Puntos de una fila a lo largo de la carretera: el punto del trazado y la
/// posición, cada `every` metros desde la mitad del primer hueco. None on a
/// road off the lap or next to its walls.
pub fn row_points(track: &Track, row: &Row) -> Vec<(usize, Vec2)> {
    let lap = track.lap_length();
    let length = row.span.length(lap);
    let count = (length / row.every).floor() as usize;
    (0..count)
        .map(|k| {
            let along = row.span.from + row.every / 2.0 + k as f32 * row.every;
            let index = track.index_at(along);
            (
                index,
                track.offset_point(index, row.side.sign() * row.lateral),
            )
        })
        .filter(|&(_, point)| {
            !track.on_other_road(0, point, |road, nearest| road.wall_at(nearest.index) + 2.0)
        })
        .collect()
}

/// Street lamps in a row, their arms over the road, as in the arcade
/// racers of the 90s.
fn spawn_lamps(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    scenery: &Scenery,
    row: &Row,
    assets: &mut Assets3d,
) {
    let metal = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.55, 0.57, 0.6),
        perceptual_roughness: 0.4,
        ..default()
    });
    let glass = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.95, 0.8),
        emissive: LinearRgba::rgb(1.5, 1.4, 1.1),
        ..default()
    });
    // El brazo sale hacia +X en local: hacia la carretera si la farola está a
    // la izquierda; a la derecha se gira media vuelta.
    let arm = [
        Vec3::new(0.0, LAMP_HEIGHT, 0.0),
        Vec3::new(0.5, LAMP_HEIGHT + 0.55, 0.0),
        Vec3::new(1.6, LAMP_HEIGHT + 0.85, 0.0),
        Vec3::new(2.7, LAMP_HEIGHT + 0.75, 0.0),
    ];
    let meshes = &mut *assets.meshes;
    let mut parts = vec![(
        meshes.add(tube(0.13, 0.09, LAMP_HEIGHT + 0.3)),
        between(Vec3::new(0.0, -0.3, 0.0), Vec3::new(0.0, LAMP_HEIGHT, 0.0)),
        metal.clone(),
    )];
    for pair in arm.windows(2) {
        parts.push((
            meshes.add(tube(0.08, 0.07, pair[0].distance(pair[1]))),
            between(pair[0], pair[1]),
            metal.clone(),
        ));
    }
    let head = meshes.add(Sphere::new(0.5).mesh().uv(16, 10));
    let tip = arm[3] + Vec3::new(0.35, -0.05, 0.0);
    parts.push((
        head.clone(),
        Transform::from_translation(tip).with_scale(Vec3::new(1.4, 0.4, 0.7)),
        metal,
    ));
    parts.push((
        head,
        Transform::from_translation(tip - Vec3::Y * 0.1).with_scale(Vec3::new(1.1, 0.2, 0.5)),
        glass,
    ));

    let lap = track.lap_length();
    for (index, point) in row_points(track, row) {
        // On a bridge or a dam, they stand on its deck.
        let on_deck = scenery.bridges.iter().any(|bridge| {
            bridge
                .span
                .contains(index as f32 * track.step_length(), lap)
        });
        let base = if on_deck {
            track.surface_point(index, row.side.sign() * row.lateral).y
        } else {
            terrain.height(point)
        };
        // Donde detrás del muro sube la roca no se ve bien una farola.
        if base - track.surface_point(index, row.side.sign() * row.lateral).y > 1.5 {
            continue;
        }
        let (_, heading) = track.pose_at(index);
        let facing = if row.side.sign() > 0.0 {
            heading
        } else {
            heading + std::f32::consts::PI
        };
        world
            .spawn((
                Transform::from_xyz(point.x, base, point.y)
                    .with_rotation(Quat::from_rotation_y(facing)),
                Visibility::default(),
            ))
            .with_children(|lamp| {
                for (mesh, transform, material) in &parts {
                    lamp.spawn((
                        Mesh3d(mesh.clone()),
                        MeshMaterial3d(material.clone()),
                        *transform,
                    ));
                }
            });
    }
}

/// Una casa ya colocada.
struct House {
    position: Vec2,
    /// Hacia dónde mira la fachada.
    heading: f32,
    width: f32,
    depth: f32,
    floors: u32,
    /// Altura del suelo de la planta baja.
    base: f32,
    paint: Color,
}

/// Casas en terrenos llanos cerca del circuito, mirando a la carretera: en
/// cualquier sitio o, si el escenario lo dice, junto a unos tramos.
fn place_houses(
    track: &Track,
    terrain: &Terrain,
    config: &Houses,
    occupied: &[Footprint],
) -> Vec<House> {
    let (min, max) = track.bounds();
    let lap = track.lap_length();
    let mut houses: Vec<House> = Vec::new();
    for seed in 0..4000u32 {
        if houses.len() >= config.max {
            break;
        }
        let random = |k: u32| random01(seed * 8 + k + 100_000);
        let position = if config.spans.is_empty() {
            Vec2::new(
                (min.x - 150.0).lerp(max.x + 150.0, random(0)),
                (min.y - 150.0).lerp(max.y + 150.0, random(1)),
            )
        } else {
            let span = config.spans[seed as usize % config.spans.len()];
            let along = span.from + span.length(lap) * random(0);
            let side = if random(6) < 0.5 { 1.0 } else { -1.0 };
            let lateral = side * (WALL_DISTANCE + 14.0 + 30.0 * random(1));
            track.offset_point(track.index_at(along), lateral)
        };
        let nearest = track.nearest(position);
        let (width, depth) = (9.0 + 5.0 * random(2), 7.0 + 3.0 * random(3));
        if !(WALL_DISTANCE + 14.0..WALL_DISTANCE + 70.0).contains(&nearest.distance)
            || houses
                .iter()
                .any(|h| h.position.distance(position) < HOUSE_SPACING)
            || occupied
                .iter()
                .any(|&(at, radius)| at.distance(position) < radius + width)
        {
            continue;
        }
        let reach = width.max(depth) / 2.0 + 1.0;
        let footprint = [Vec2::ZERO, Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y]
            .map(|offset| position + offset * reach);
        let heights = footprint.map(|p| terrain.height(p));
        let lowest = heights.iter().copied().fold(f32::INFINITY, f32::min);
        let highest = heights.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let all_grass = footprint
            .iter()
            .all(|&p| terrain.ground(p) == Ground::Grass);
        if highest - lowest > 2.0 || lowest < terrain.water_level(position) + 2.0 || !all_grass {
            continue;
        }
        let paints = config.paints;
        houses.push(House {
            position,
            heading: heading_of(nearest.point - position),
            width,
            depth,
            floors: 1 + (random(4) > 0.5) as u32,
            base: lowest,
            paint: paints[(random(5) * paints.len() as f32) as usize % paints.len()],
        });
    }
    houses
}

fn spawn_house(
    world: &mut ChildSpawnerCommands,
    house: &House,
    style: HouseStyle,
    textures: &WorldTextures,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let (w, d) = (house.width, house.depth);
    let walls_height = house.floors as f32 * FLOOR_HEIGHT + 0.4;
    // Las paredes bajan bajo tierra por si el suelo no es del todo llano.
    const FOOTING: f32 = 2.0;
    let plaster = materials.add(StandardMaterial {
        base_color: house.paint,
        base_color_texture: Some(textures.plaster.clone()),
        perceptual_roughness: 0.9,
        ..default()
    });
    let shutters = materials.add(StandardMaterial {
        base_color: Color::srgb(0.1, 0.35, 0.3),
        perceptual_roughness: 0.6,
        ..default()
    });
    let roof = materials.add(StandardMaterial {
        base_color_texture: Some(textures.roof.clone()),
        perceptual_roughness: 0.8,
        ..default()
    });

    let mut entity = world.spawn((
        Transform::from_xyz(house.position.x, house.base, house.position.y)
            .with_rotation(Quat::from_rotation_y(house.heading)),
        Visibility::default(),
    ));
    entity.with_children(|parent| {
        parent.spawn((
            Mesh3d(meshes.add(Cuboid::new(w, walls_height + FOOTING, d))),
            MeshMaterial3d(plaster.clone()),
            Transform::from_xyz(0.0, (walls_height - FOOTING) / 2.0, 0.0),
        ));
        let (slopes, gables) = gable_roof(w, d, walls_height);
        parent.spawn((Mesh3d(meshes.add(slopes)), MeshMaterial3d(roof)));
        parent.spawn((Mesh3d(meshes.add(gables)), MeshMaterial3d(plaster.clone())));

        // Ventanas con contraventanas en la fachada y en la parte de atrás, y
        // una puerta en la fachada (la cara -Z, que mira a la carretera).
        let window = meshes.add(Cuboid::new(1.0, 1.35, 0.12));
        let columns = ((w / 3.2) as u32).max(2);
        for floor in 0..house.floors {
            for column in 0..columns {
                let x = -w / 2.0 + w * (column as f32 + 0.5) / columns as f32;
                let y = floor as f32 * FLOOR_HEIGHT + 1.7;
                let door_here = floor == 0 && column == columns / 2;
                for z in [-d / 2.0 - 0.05, d / 2.0 + 0.05] {
                    if door_here && z < 0.0 {
                        continue;
                    }
                    parent.spawn((
                        Mesh3d(window.clone()),
                        MeshMaterial3d(shutters.clone()),
                        Transform::from_xyz(x, y, z),
                    ));
                }
            }
        }
        let door_x = -w / 2.0 + w * ((columns / 2) as f32 + 0.5) / columns as f32;
        let wood = materials.add(Color::srgb(0.35, 0.2, 0.1));
        parent.spawn((
            Mesh3d(meshes.add(Cuboid::new(1.3, 2.3, 0.12))),
            MeshMaterial3d(wood.clone()),
            Transform::from_xyz(door_x, 1.15, -d / 2.0 - 0.05),
        ));
        if style == HouseStyle::FalseFront {
            // The false front, standing up over the roof from the eaves,
            // with a cornice along its top, and a porch along the front
            // on posts (a balcony, with two floors).
            let rise = d * 0.32;
            let top = walls_height + rise + 1.4;
            let front = -d / 2.0 - 0.2;
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(w + 0.4, top - walls_height + 0.6, 0.3))),
                MeshMaterial3d(plaster.clone()),
                Transform::from_xyz(0.0, (walls_height - 0.6 + top) / 2.0, front),
            ));
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(w + 1.0, 0.4, 0.7))),
                MeshMaterial3d(wood.clone()),
                Transform::from_xyz(0.0, top + 0.2, front - 0.1),
            ));
            const PORCH: f32 = 2.6;
            const PORCH_HEIGHT: f32 = 3.0;
            parent.spawn((
                Mesh3d(meshes.add(Cuboid::new(w + 0.4, 0.18, PORCH))),
                MeshMaterial3d(wood.clone()),
                Transform::from_xyz(0.0, PORCH_HEIGHT, -d / 2.0 - PORCH / 2.0),
            ));
            let post = meshes.add(Cuboid::new(0.2, PORCH_HEIGHT + FOOTING, 0.2));
            let posts = ((w / 3.0) as u32).max(2);
            for k in 0..=posts {
                let x = -w / 2.0 + w * k as f32 / posts as f32;
                parent.spawn((
                    Mesh3d(post.clone()),
                    MeshMaterial3d(wood.clone()),
                    Transform::from_xyz(x, (PORCH_HEIGHT - FOOTING) / 2.0, -d / 2.0 - PORCH + 0.15),
                ));
            }
        }
    });
}

/// Tejado a dos aguas (con la cumbrera a lo largo del eje X) y los dos
/// hastiales triangulares que lo cierran.
fn gable_roof(width: f32, depth: f32, eaves: f32) -> (Mesh, Mesh) {
    const OVERHANG: f32 = 0.5;
    const TILE: f32 = 2.0;
    let rise = depth * 0.32;
    let (x, z) = (width / 2.0 + OVERHANG, depth / 2.0 + OVERHANG);
    // El alero sobresale y baja en proporción a la pendiente.
    let low = eaves - OVERHANG * rise / (depth / 2.0);
    let ridge = eaves + rise;
    let slope_length = Vec2::new(z, ridge - low).length() / TILE;
    let along = 2.0 * x / TILE;
    let mut slopes = MeshBuilder::default();
    for side in [-1.0, 1.0] {
        slopes.quad(
            [
                (Vec3::new(-x, low, side * z), Vec2::new(0.0, slope_length)),
                (Vec3::new(x, low, side * z), Vec2::new(along, slope_length)),
                (Vec3::new(x, ridge, 0.0), Vec2::new(along, 0.0)),
                (Vec3::new(-x, ridge, 0.0), Vec2::new(0.0, 0.0)),
            ],
            Vec3::new(0.0, 1.0, side),
            Color::WHITE,
        );
    }
    let mut gables = MeshBuilder::default();
    let half_width = width / 2.0;
    for side in [-1.0, 1.0] {
        let tip = Vec3::new(side * half_width, ridge, 0.0);
        gables.triangle(
            [
                (
                    Vec3::new(side * half_width, eaves, -depth / 2.0),
                    Vec2::ZERO,
                ),
                (Vec3::new(side * half_width, eaves, depth / 2.0), Vec2::X),
                (tip, Vec2::new(0.5, 0.4)),
            ],
            Vec3::X * side,
            Color::WHITE,
        );
    }
    (slopes.build(), gables.build())
}

/// Una variante de árbol: piezas de tronco/ramas y piezas de copa.
struct TreeVariant {
    wood: Vec<(Handle<Mesh>, Transform)>,
    crowns: Vec<(Handle<Mesh>, Transform)>,
}

/// Colores de la copa en sombra y al sol.
fn foliage(species: Species) -> (Color, Color) {
    match species {
        Species::UmbrellaPine => (Color::srgb(0.06, 0.18, 0.05), Color::srgb(0.28, 0.48, 0.14)),
        Species::Cypress => (Color::srgb(0.03, 0.12, 0.04), Color::srgb(0.14, 0.32, 0.11)),
        Species::Olive => (Color::srgb(0.18, 0.25, 0.12), Color::srgb(0.48, 0.58, 0.30)),
        Species::Tall => (Color::srgb(0.03, 0.11, 0.04), Color::srgb(0.12, 0.30, 0.10)),
        Species::Broadleaf => (Color::srgb(0.08, 0.24, 0.05), Color::srgb(0.35, 0.60, 0.18)),
        Species::MaritimePine => (Color::srgb(0.04, 0.16, 0.06), Color::srgb(0.22, 0.44, 0.16)),
        Species::Poplar => (Color::srgb(0.11, 0.28, 0.06), Color::srgb(0.42, 0.66, 0.20)),
        Species::Almond => (Color::srgb(0.40, 0.26, 0.30), Color::srgb(0.92, 0.74, 0.78)),
        Species::Momiji => (Color::srgb(0.32, 0.04, 0.03), Color::srgb(0.86, 0.16, 0.08)),
        Species::Ginkgo => (Color::srgb(0.42, 0.30, 0.03), Color::srgb(0.96, 0.78, 0.16)),
        Species::Sakura => (Color::srgb(0.52, 0.36, 0.42), Color::srgb(0.98, 0.80, 0.86)),
        Species::BlackPine => (Color::srgb(0.03, 0.12, 0.06), Color::srgb(0.16, 0.36, 0.18)),
        Species::Sugi => (Color::srgb(0.03, 0.10, 0.05), Color::srgb(0.13, 0.30, 0.12)),
        Species::Bamboo => (Color::srgb(0.12, 0.30, 0.06), Color::srgb(0.46, 0.70, 0.22)),
    }
}

/// Whether the leaves of `kind` are not green (in blossom, or turned in
/// autumn): the sea mirrors those trees, faintly (see [`crown_points`]).
fn in_colour(kind: Species) -> bool {
    let sunlit = foliage(kind).1.to_srgba();
    sunlit.green < sunlit.red.max(sunlit.blue)
}

/// The species whose shape a classic tree of `kind` takes (in its own
/// colours): the trees of Japan look like the nearest of the others.
fn classic_shape(kind: Species) -> Species {
    match kind {
        Species::Momiji => Species::Broadleaf,
        Species::Ginkgo | Species::Bamboo => Species::Poplar,
        Species::Sakura => Species::Almond,
        Species::BlackPine => Species::UmbrellaPine,
        Species::Sugi => Species::Tall,
        other => other,
    }
}

fn branch(
    meshes: &mut Assets<Mesh>,
    from: Vec3,
    to: Vec3,
    r_start: f32,
    r_end: f32,
) -> (Handle<Mesh>, Transform) {
    let len = (to - from).length();
    let mesh = meshes.add(tube(r_start, r_end, len));
    let tf = between(from, to);
    (mesh, tf)
}

fn build_tree_variants(kind: Species, meshes: &mut Assets<Mesh>) -> Vec<TreeVariant> {
    const VARIANTS: u32 = 3;
    let mut variants = Vec::with_capacity(VARIANTS as usize);
    for variant in 0..VARIANTS {
        let seed = variant * 47 + kind as u32 * 17;
        let (wood, crowns) = match classic_shape(kind) {
            Species::UmbrellaPine => {
                let (dx, dz) = match variant {
                    0 => (0.3, 0.0),
                    1 => (-0.35, 0.2),
                    _ => (0.2, -0.3),
                };
                let trunk_top = Vec3::new(dx, 5.0, dz);
                let wood = vec![
                    branch(meshes, Vec3::new(0.0, -0.3, 0.0), trunk_top, 0.30, 0.22),
                    branch(
                        meshes,
                        trunk_top,
                        trunk_top + Vec3::new(1.6, 1.5, 0.6),
                        0.18,
                        0.12,
                    ),
                    branch(
                        meshes,
                        trunk_top,
                        trunk_top + Vec3::new(-1.5, 1.4, -0.6),
                        0.18,
                        0.12,
                    ),
                ];
                let crowns = vec![
                    (
                        meshes.add(crown_mesh(seed + 1, CrownStyle::Umbrella, foliage(kind))),
                        Transform::from_translation(trunk_top + Vec3::new(0.0, 2.1, 0.0))
                            .with_scale(Vec3::new(4.6, 1.4, 4.6)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 2, CrownStyle::Umbrella, foliage(kind))),
                        Transform::from_translation(trunk_top + Vec3::new(1.7, 1.8, 0.6))
                            .with_scale(Vec3::new(2.7, 1.1, 2.7)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 3, CrownStyle::Umbrella, foliage(kind))),
                        Transform::from_translation(trunk_top + Vec3::new(-1.6, 1.7, -0.6))
                            .with_scale(Vec3::new(2.5, 1.1, 2.5)),
                    ),
                ];
                (wood, crowns)
            }
            Species::Cypress => {
                let (scale_w, scale_h) = match variant {
                    0 => (1.15, 4.8),
                    1 => (1.05, 5.3),
                    _ => (1.25, 4.5),
                };
                let wood = vec![branch(
                    meshes,
                    Vec3::new(0.0, -0.3, 0.0),
                    Vec3::new(0.0, 1.4, 0.0),
                    0.20,
                    0.15,
                )];
                let crowns = vec![(
                    meshes.add(crown_mesh(seed + 1, CrownStyle::Spire, foliage(kind))),
                    Transform::from_xyz(0.0, 4.8, 0.0)
                        .with_scale(Vec3::new(scale_w, scale_h, scale_w)),
                )];
                (wood, crowns)
            }
            Species::Olive => {
                let (dx, dz) = match variant {
                    0 => (0.25, 0.0),
                    1 => (-0.2, 0.25),
                    _ => (0.1, -0.2),
                };
                let base_top = Vec3::new(dx, 1.8, dz);
                let wood = vec![
                    branch(meshes, Vec3::new(0.0, -0.3, 0.0), base_top, 0.28, 0.20),
                    branch(
                        meshes,
                        base_top,
                        base_top + Vec3::new(0.9, 1.0, 0.4),
                        0.16,
                        0.10,
                    ),
                    branch(
                        meshes,
                        base_top,
                        base_top + Vec3::new(-0.8, 0.9, -0.3),
                        0.16,
                        0.10,
                    ),
                ];
                let crowns = vec![
                    (
                        meshes.add(crown_mesh(seed + 1, CrownStyle::Cloud, foliage(kind))),
                        Transform::from_translation(base_top + Vec3::new(0.0, 1.5, 0.0))
                            .with_scale(Vec3::new(2.2, 1.6, 2.2)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 2, CrownStyle::Cloud, foliage(kind))),
                        Transform::from_translation(base_top + Vec3::new(1.0, 1.1, 0.4))
                            .with_scale(Vec3::new(1.8, 1.4, 1.8)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 3, CrownStyle::Cloud, foliage(kind))),
                        Transform::from_translation(base_top + Vec3::new(-0.9, 1.0, -0.3))
                            .with_scale(Vec3::new(1.7, 1.3, 1.7)),
                    ),
                ];
                (wood, crowns)
            }
            Species::Broadleaf => {
                let (dx, dz) = match variant {
                    0 => (0.15, 0.0),
                    1 => (-0.2, 0.15),
                    _ => (0.1, -0.15),
                };
                let trunk_top = Vec3::new(dx, 3.2, dz);
                let wood = vec![
                    branch(meshes, Vec3::new(0.0, -0.3, 0.0), trunk_top, 0.34, 0.24),
                    branch(
                        meshes,
                        trunk_top,
                        trunk_top + Vec3::new(1.3, 1.3, 0.5),
                        0.18,
                        0.11,
                    ),
                    branch(
                        meshes,
                        trunk_top,
                        trunk_top + Vec3::new(-1.2, 1.2, -0.4),
                        0.18,
                        0.11,
                    ),
                ];
                let crowns = vec![
                    (
                        meshes.add(crown_mesh(seed + 1, CrownStyle::Cloud, foliage(kind))),
                        Transform::from_translation(trunk_top + Vec3::new(0.0, 2.2, 0.0))
                            .with_scale(Vec3::new(3.2, 2.8, 3.2)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 2, CrownStyle::Cloud, foliage(kind))),
                        Transform::from_translation(trunk_top + Vec3::new(1.4, 1.4, 0.5))
                            .with_scale(Vec3::new(2.2, 2.0, 2.2)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 3, CrownStyle::Cloud, foliage(kind))),
                        Transform::from_translation(trunk_top + Vec3::new(-1.3, 1.3, -0.4))
                            .with_scale(Vec3::new(2.1, 1.9, 2.1)),
                    ),
                ];
                (wood, crowns)
            }
            Species::Tall => {
                let (h1, h2) = match variant {
                    0 => (8.2, 11.6),
                    1 => (8.8, 12.4),
                    _ => (7.8, 11.0),
                };
                let wood = vec![branch(
                    meshes,
                    Vec3::new(0.0, -0.3, 0.0),
                    Vec3::new(0.0, 9.8, 0.0),
                    0.35,
                    0.18,
                )];
                let crowns = vec![
                    (
                        meshes.add(crown_mesh(seed + 1, CrownStyle::ConicalTier, foliage(kind))),
                        Transform::from_xyz(0.0, h2, 0.0).with_scale(Vec3::new(2.5, 4.4, 2.5)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 2, CrownStyle::ConicalTier, foliage(kind))),
                        Transform::from_xyz(0.0, h1, 0.0).with_scale(Vec3::new(3.4, 4.0, 3.4)),
                    ),
                ];
                (wood, crowns)
            }
            Species::MaritimePine => {
                let (dx, dz) = match variant {
                    0 => (0.3, 0.0),
                    1 => (-0.3, 0.3),
                    _ => (0.2, -0.3),
                };
                let trunk_top = Vec3::new(dx, 6.8, dz);
                let wood = vec![
                    branch(meshes, Vec3::new(0.0, -0.3, 0.0), trunk_top, 0.32, 0.20),
                    branch(
                        meshes,
                        Vec3::new(dx * 0.7, 5.0, dz * 0.7),
                        Vec3::new(dx * 0.7 + 1.3, 5.8, dz * 0.7 + 0.5),
                        0.16,
                        0.10,
                    ),
                    branch(
                        meshes,
                        Vec3::new(dx * 0.8, 6.0, dz * 0.8),
                        Vec3::new(dx * 0.8 - 1.2, 6.7, dz * 0.8 - 0.4),
                        0.15,
                        0.09,
                    ),
                ];
                let crowns = vec![
                    (
                        meshes.add(crown_mesh(seed + 1, CrownStyle::Umbrella, foliage(kind))),
                        Transform::from_translation(trunk_top + Vec3::new(0.0, 1.2, 0.0))
                            .with_scale(Vec3::new(3.2, 1.5, 3.2)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 2, CrownStyle::Umbrella, foliage(kind))),
                        Transform::from_xyz(dx * 0.7 + 1.4, 6.0, dz * 0.7 + 0.5)
                            .with_scale(Vec3::new(2.3, 1.2, 2.3)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 3, CrownStyle::Umbrella, foliage(kind))),
                        Transform::from_xyz(dx * 0.8 - 1.3, 6.9, dz * 0.8 - 0.4)
                            .with_scale(Vec3::new(2.1, 1.1, 2.1)),
                    ),
                ];
                (wood, crowns)
            }
            Species::Poplar => {
                let (scale_w, scale_h) = match variant {
                    0 => (1.8, 5.4),
                    1 => (1.6, 5.8),
                    _ => (1.9, 5.0),
                };
                let wood = vec![branch(
                    meshes,
                    Vec3::new(0.0, -0.3, 0.0),
                    Vec3::new(0.0, 3.4, 0.0),
                    0.22,
                    0.14,
                )];
                let crowns = vec![(
                    meshes.add(crown_mesh(seed + 1, CrownStyle::Cloud, foliage(kind))),
                    Transform::from_xyz(0.0, 6.0, 0.0)
                        .with_scale(Vec3::new(scale_w, scale_h, scale_w)),
                )];
                (wood, crowns)
            }
            Species::Almond => {
                let (dx, dz) = match variant {
                    0 => (0.15, 0.0),
                    1 => (-0.15, 0.2),
                    _ => (0.1, -0.15),
                };
                let base_top = Vec3::new(dx, 1.8, dz);
                let wood = vec![
                    branch(meshes, Vec3::new(0.0, -0.3, 0.0), base_top, 0.24, 0.16),
                    branch(
                        meshes,
                        base_top,
                        base_top + Vec3::new(0.9, 0.9, 0.4),
                        0.13,
                        0.08,
                    ),
                    branch(
                        meshes,
                        base_top,
                        base_top + Vec3::new(-0.8, 0.8, -0.3),
                        0.13,
                        0.08,
                    ),
                ];
                let crowns = vec![
                    (
                        meshes.add(crown_mesh(seed + 1, CrownStyle::Cloud, foliage(kind))),
                        Transform::from_translation(base_top + Vec3::new(0.0, 1.4, 0.0))
                            .with_scale(Vec3::new(2.1, 1.5, 2.1)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 2, CrownStyle::Cloud, foliage(kind))),
                        Transform::from_translation(base_top + Vec3::new(1.0, 1.0, 0.4))
                            .with_scale(Vec3::new(1.7, 1.3, 1.7)),
                    ),
                    (
                        meshes.add(crown_mesh(seed + 3, CrownStyle::Cloud, foliage(kind))),
                        Transform::from_translation(base_top + Vec3::new(-0.9, 0.9, -0.3))
                            .with_scale(Vec3::new(1.6, 1.2, 1.6)),
                    ),
                ];
                (wood, crowns)
            }
            Species::Momiji
            | Species::Ginkgo
            | Species::Sakura
            | Species::BlackPine
            | Species::Sugi
            | Species::Bamboo => unreachable!("they take the shape of another"),
        };
        variants.push(TreeVariant { wood, crowns });
    }
    variants
}

/// A tree to plant: its species, where, the height of the ground there, and
/// the seed of its variant, size and turn.
struct PlannedTree {
    kind: Species,
    position: Vec2,
    height: f32,
    seed: u32,
}

/// Where the trees of a circuit go, in the order they are planted: the
/// scattered ones and those of the forests. Worked out away from the main
/// thread with the rest of the circuit (see [`super::prepare`]): finding
/// the road from tens of thousands of candidate spots takes a while. The
/// ones in the way of a building or a landmark are left out when they are
/// planted ([`spawn_trees`]).
pub struct TreePlan {
    scattered: Vec<PlannedTree>,
    forests: Vec<PlannedTree>,
    /// The variants of each species, each one mesh (see [`merged_variant`]),
    /// full and lighter (see [`lighter`]).
    models: Vec<(Species, Vec<[Mesh; LEVELS]>)>,
}

/// The colour of the bark (sRGB).
const BARK: Color = Color::srgb(0.32, 0.22, 0.15);

/// A tree variant as one mesh: its wood painted the colour of bark and its
/// crowns, so each tree is one entity to draw, not one per branch and
/// crown. Solid, and the same near and far: nothing changes as it comes
/// closer, so nothing pops in.
fn merged_variant(meshes: &Assets<Mesh>, variant: &TreeVariant) -> Mesh {
    let bark = BARK.to_linear().to_f32_array();
    let mut merged: Option<Mesh> = None;
    for (parts, wood) in [(&variant.wood, true), (&variant.crowns, false)] {
        for (handle, transform) in parts {
            let mut part = meshes
                .get(handle)
                .expect("the parts of a variant are in its assets")
                .clone()
                .transformed_by(*transform);
            part.remove_attribute(Mesh::ATTRIBUTE_UV_0);
            part.remove_attribute(Mesh::ATTRIBUTE_TANGENT);
            if wood {
                let count = part.count_vertices();
                part.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![bark; count]);
            }
            if let Some(Indices::U16(indices)) = part.indices() {
                let wide: Vec<u32> = indices.iter().map(|&i| u32::from(i)).collect();
                part.insert_indices(Indices::U32(wide));
            }
            match &mut merged {
                None => merged = Some(part),
                Some(mesh) => mesh
                    .merge(&part)
                    .expect("the parts of a tree have the same attributes"),
            }
        }
    }
    merged.expect("a tree has wood and a crown")
}

/// How many points over its crown of a tree in colour the sea mirrors at
/// most (see [`crown_points`]).
const CROWN_POINTS: usize = 48;

/// Points over the crown of a tree variant's `mesh` (see [`merged_variant`];
/// in its own frame), for the sea to mirror: its vertices not painted the
/// colour of bark, at most [`CROWN_POINTS`] of them, spread over it.
fn crown_points(mesh: &Mesh) -> Vec<MirrorPoint> {
    let bark = Vec4::from(BARK.to_linear().to_f32_array());
    let (
        Some(positions),
        Some(VertexAttributeValues::Float32x3(normals)),
        Some(VertexAttributeValues::Float32x4(colors)),
    ) = (
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|positions| positions.as_float3()),
        mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
        mesh.attribute(Mesh::ATTRIBUTE_COLOR),
    )
    else {
        return Vec::new();
    };
    let crown: Vec<MirrorPoint> = positions
        .iter()
        .zip(normals)
        .zip(colors)
        .filter(|(_, color)| Vec4::from(**color).distance(bark) > 1e-3)
        .map(|((&at, &normal), &[r, g, b, _])| MirrorPoint {
            at: Vec3::from(at),
            normal: Vec3::from(normal).normalize_or_zero(),
            color: [r, g, b],
        })
        .collect();
    let every = crown.len().div_ceil(CROWN_POINTS).max(1);
    crown.into_iter().step_by(every).collect()
}

/// Plans the trees of a circuit (see [`TreePlan`]).
pub fn plan_trees(track: &Track, terrain: &Terrain, scenery: &Scenery) -> TreePlan {
    let lap = track.lap_length();
    // Si se puede plantar un árbol en un punto: hierba, no muy empinada, por
    // encima del mar y lejos de las bocas de los túneles (the buildings are
    // checked when planting).
    let near_tunnel = |position: Vec2| {
        let along = track.along(&track.nearest(position));
        scenery
            .tunnels
            .iter()
            .any(|tunnel| span(tunnel.from - 30.0, tunnel.to + 30.0).contains(along, lap))
    };
    let fits = |position: Vec2, distance: f32| {
        terrain.height(position) >= terrain.water_level(position) + 1.8
            && terrain.ground(position) == Ground::Grass
            && terrain.normal(position).y >= 0.88
            && !(distance < 35.0 && !scenery.tunnels.is_empty() && near_tunnel(position))
    };
    let planned = |kind: Species, position: Vec2, seed: u32| PlannedTree {
        kind,
        position,
        height: terrain.height(position),
        seed,
    };

    // Some spare ones, for those that end up in the way of a building.
    let mut scattered = Vec::new();
    let (min, max) = track.bounds();
    for seed in 0..TREE_CANDIDATES {
        if scattered.len() >= 2 * MAX_TREES || scenery.trees.is_empty() {
            break;
        }
        let random = |k: u32| random01(seed * 8 + k);
        let position = Vec2::new(
            (min.x - TREE_MARGIN).lerp(max.x + TREE_MARGIN, random(0)),
            (min.y - TREE_MARGIN).lerp(max.y + TREE_MARGIN, random(1)),
        );
        let distance = track.distance_to_centerline(position);
        // More trees near the track for sense of speed, while also populating hills and valleys
        let density = 0.40 + 0.60 * (1.0 - smoothstep(25.0, 160.0, distance));
        if distance < WALL_DISTANCE + 4.0 || random(2) > density || !fits(position, distance) {
            continue;
        }
        let kind =
            scenery.trees[(random(3) * scenery.trees.len() as f32) as usize % scenery.trees.len()];
        scattered.push(planned(kind, position, seed));
    }

    // Los bosques.
    let mut forests = Vec::new();
    let mut seed = 50_000;
    for forest in scenery.forests {
        let count = (forest.span.length(lap) / forest.every) as usize;
        for k in 0..count {
            for side in [1.0, -1.0] {
                for depth in 0..FOREST_DEPTH {
                    seed += 1;
                    let random = |n: u32| random01(seed * 8 + n);
                    let along = forest.span.from + (k as f32 + random(0)) * forest.every;
                    let lateral = side * (WALL_DISTANCE + 4.0 + 9.0 * (depth as f32 + random(1)));
                    let position = track.offset_point(track.index_at(along), lateral);
                    let distance = track.distance_to_centerline(position);
                    if distance < WALL_DISTANCE + 3.0 || !fits(position, distance) {
                        continue;
                    }
                    let kind = forest.species
                        [(random(2) * forest.species.len() as f32) as usize % forest.species.len()];
                    forests.push(planned(kind, position, seed));
                }
            }
        }
    }
    // The models of the species planted, built here too.
    let mut species: Vec<Species> = scenery.trees.to_vec();
    for &kind in scenery.forests.iter().flat_map(|forest| forest.species) {
        if !species.contains(&kind) {
            species.push(kind);
        }
    }
    let mut scratch = Assets::<Mesh>::default();
    let models = species
        .into_iter()
        .map(|kind| {
            let variants = build_tree_variants(kind, &mut scratch);
            let merged = variants
                .iter()
                .map(|variant| {
                    let full = merged_variant(&scratch, variant);
                    let [first, second] = lighter(&full);
                    [full, first, second]
                })
                .collect();
            (kind, merged)
        })
        .collect();
    TreePlan {
        scattered,
        forests,
        models,
    }
}

/// Plants the trees planned for a circuit, except those in the way of what
/// is `occupied`: up to [`MAX_TREES`] scattered ones, and the forests.
fn spawn_trees(
    world: &mut ChildSpawnerCommands,
    plan: &TreePlan,
    occupied: &[Footprint],
    assets: &mut Assets3d,
) {
    const VARIANTS: u32 = 3;
    // Bark and leaves are in the colours of the meshes.
    let material = assets.materials.add(StandardMaterial {
        perceptual_roughness: 0.95,
        ..default()
    });
    // Each variant's meshes, full and lighter, and its bounds.
    type Variant = ([Handle<Mesh>; LEVELS], Aabb);
    let models: Vec<(Species, Vec<Variant>)> = plan
        .models
        .iter()
        .map(|(kind, variants)| {
            (
                *kind,
                variants
                    .iter()
                    .map(|meshes| {
                        let bounds = meshes[0].compute_aabb().expect("a tree has vertices");
                        (meshes.clone().map(|mesh| assets.meshes.add(mesh)), bounds)
                    })
                    .collect(),
            )
        })
        .collect();
    let model = |kind: Species| {
        &models
            .iter()
            .find(|(k, _)| *k == kind)
            .expect("hay modelo para cada especie")
            .1
    };
    // The crowns of the variants of the trees in colour, for the sea to
    // mirror.
    let crowns: Vec<(Species, Vec<Vec<MirrorPoint>>)> = plan
        .models
        .iter()
        .filter(|(kind, _)| in_colour(*kind))
        .map(|(kind, variants)| {
            let crowns = variants
                .iter()
                .map(|meshes| crown_points(&meshes[0]))
                .collect();
            (*kind, crowns)
        })
        .collect();
    let free = |tree: &&PlannedTree| {
        !occupied
            .iter()
            .any(|&(at, radius)| at.distance(tree.position) < radius)
    };
    // Planted classic; dressed as the options say (see [`super::trees`]).
    let plant =
        |world: &mut ChildSpawnerCommands, leaves: &mut Vec<MirrorPoint>, tree: &PlannedTree| {
            let random = |k: u32| random01(tree.seed * 8 + k);
            let variants = model(tree.kind);
            let variant = (random(4) * VARIANTS as f32) as usize % VARIANTS as usize;
            let (meshes, bounds) = &variants[variant];
            let scale = 0.75 + 0.60 * random(5);
            let transform = Transform::from_xyz(tree.position.x, tree.height, tree.position.y)
                .with_rotation(Quat::from_rotation_y(random(6) * std::f32::consts::TAU))
                .with_scale(Vec3::splat(scale));
            if let Some((_, crowns)) = crowns.iter().find(|(kind, _)| *kind == tree.kind) {
                leaves.extend(crowns[variant].iter().map(|point| point.placed(&transform)));
            }
            world.spawn((
                Tree {
                    classic: meshes.clone(),
                    classic_material: material.clone(),
                    classic_bounds: *bounds,
                    model: TreeModel::of(tree.kind, random(7)),
                },
                Mesh3d(meshes[0].clone()),
                MeshMaterial3d(material.clone()),
                transform,
            ));
        };
    for tree in plan.scattered.iter().filter(free).take(MAX_TREES) {
        plant(world, &mut assets.mirrored_leaves, tree);
    }
    for tree in plan.forests.iter().filter(free) {
        plant(world, &mut assets.mirrored_leaves, tree);
    }
}

/// Palmeras en fila: tronco algo inclinado con anillos y un penacho de hojas
/// que cuelgan. Devuelve el sitio que ocupan.
fn spawn_palms(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    row: &Row,
    assets: &mut Assets3d,
) -> Vec<Footprint> {
    const VARIANTS: u32 = 3;
    let material = assets.materials.add(StandardMaterial {
        perceptual_roughness: 0.85,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let models: Vec<Handle<Mesh>> = (0..VARIANTS)
        .map(|variant| assets.meshes.add(palm_mesh(variant)))
        .collect();
    let mut taken = Vec::new();
    for (k, (_, point)) in row_points(track, row).into_iter().enumerate() {
        let random = |n: u32| random01(k as u32 * 8 + n + 70_000 + row.lateral as u32);
        if terrain.height(point) < terrain.water_level(point) + 1.0 {
            continue;
        }
        world.spawn((
            Mesh3d(models[k % VARIANTS as usize].clone()),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(point.x, terrain.height(point) - 0.2, point.y)
                .with_rotation(Quat::from_rotation_y(random(0) * std::f32::consts::TAU))
                .with_scale(Vec3::splat(0.85 + 0.35 * random(1))),
        ));
        taken.push((point, 3.0));
    }
    taken
}

/// Una palmera: tronco de ocho caras con anillos más oscuros, que se
/// inclina y se curva, y nueve hojas largas que cuelgan desde arriba.
fn palm_mesh(variant: u32) -> Mesh {
    const HEIGHT: f32 = 9.0;
    const SIDES: usize = 8;
    const SEGMENTS: usize = 10;
    let random = |n: u32| random01(variant * 16 + n + 90_000);
    let lean = Vec3::new(1.0 + random(0), 0.0, 0.4 * (random(1) - 0.5));
    let spine = |t: f32| Vec3::Y * HEIGHT * t + lean * t * t;
    let mut mesh = MeshBuilder::default();
    let bark = |segment: usize| {
        if segment.is_multiple_of(2) {
            Color::srgb(0.45, 0.36, 0.24)
        } else {
            Color::srgb(0.36, 0.28, 0.18)
        }
    };
    for segment in 0..SEGMENTS {
        let (t0, t1) = (
            segment as f32 / SEGMENTS as f32,
            (segment + 1) as f32 / SEGMENTS as f32,
        );
        let (r0, r1) = (0.32 - 0.12 * t0, 0.32 - 0.12 * t1);
        for side in 0..SIDES {
            let angle = |s: usize| s as f32 / SIDES as f32 * std::f32::consts::TAU;
            let ring = |t: f32, r: f32, s: usize| {
                spine(t) + Vec3::new(angle(s).cos() * r, 0.0, angle(s).sin() * r)
            };
            let middle = angle(side) + std::f32::consts::PI / SIDES as f32;
            mesh.quad(
                [
                    (ring(t0, r0, side), Vec2::ZERO),
                    (ring(t0, r0, side + 1), Vec2::X),
                    (ring(t1, r1, side + 1), Vec2::ONE),
                    (ring(t1, r1, side), Vec2::Y),
                ],
                Vec3::new(middle.cos(), 0.0, middle.sin()),
                bark(segment),
            );
        }
    }
    // Las hojas: tiras que salen de lo alto, suben un poco y caen.
    let top = spine(1.0);
    const LEAVES: usize = 9;
    const LEAF_LENGTH: f32 = 4.2;
    const LEAF_STEPS: usize = 6;
    for leaf in 0..LEAVES {
        let angle = leaf as f32 / LEAVES as f32 * std::f32::consts::TAU + random(leaf as u32 + 2);
        let out = Vec3::new(angle.cos(), 0.0, angle.sin());
        let across = Vec3::new(-angle.sin(), 0.0, angle.cos());
        let droop = 2.2 + 1.2 * random(leaf as u32 + 20);
        let point = |t: f32| top + out * LEAF_LENGTH * t + Vec3::Y * (1.2 * t - droop * t * t);
        let width = |t: f32| 0.75 * (std::f32::consts::PI * t).sin().max(0.08);
        let green = Color::srgb(0.12 + 0.1 * random(leaf as u32 + 40), 0.42, 0.1);
        for step in 0..LEAF_STEPS {
            let (t0, t1) = (
                step as f32 / LEAF_STEPS as f32,
                (step + 1) as f32 / LEAF_STEPS as f32,
            );
            let (a, b) = (point(t0), point(t1));
            // Las hojas se doblan por el nervio: un poco en V.
            let fold = |t: f32| Vec3::Y * 0.25 * width(t);
            mesh.quad(
                [
                    (a - across * width(t0) - fold(t0), Vec2::ZERO),
                    (b - across * width(t1) - fold(t1), Vec2::X),
                    (b + across * width(t1) - fold(t1), Vec2::ONE),
                    (a + across * width(t0) - fold(t0), Vec2::Y),
                ],
                Vec3::Y,
                green,
            );
        }
    }
    mesh.build()
}

/// Colores de las fachadas de los hoteles, el de los cristales y el de los
/// balcones.
const HOTEL_PAINTS: [Color; 5] = [
    Color::srgb(0.97, 0.9, 0.7),
    Color::srgb(0.98, 0.8, 0.32),
    Color::srgb(0.95, 0.95, 0.94),
    Color::srgb(0.62, 0.8, 0.92),
    Color::srgb(0.95, 0.72, 0.62),
];
const GLASS: Color = Color::srgb(0.12, 0.25, 0.42);
const BALCONY: Color = Color::srgb(0.97, 0.97, 0.97);

/// Hoteles altos en fila, mirando a la carretera: cada uno con su color, una
/// franja de ventanas por planta y balcones por delante. Devuelve el sitio
/// que ocupan.
fn spawn_hotels(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    row: &Row,
    assets: &mut Assets3d,
) -> Vec<Footprint> {
    let material = assets.materials.add(StandardMaterial {
        base_color_texture: Some(assets.textures.plaster.clone()),
        perceptual_roughness: 0.8,
        ..default()
    });
    let mut taken = Vec::new();
    for (k, (index, point)) in row_points(track, row).into_iter().enumerate() {
        let random = |n: u32| random01(k as u32 * 8 + n + 80_000);
        let width = 18.0 + 8.0 * random(0);
        let depth = 13.0 + 5.0 * random(1);
        let floors = 6 + (random(2) * 6.0) as u32;
        // Uno de cada tres, todo de cristal azul.
        let glass_front = random(3) < 0.3;
        let paint =
            HOTEL_PAINTS[(random(4) * HOTEL_PAINTS.len() as f32) as usize % HOTEL_PAINTS.len()];
        let toward = track.offset_point(index, 0.0) - point;
        let reach = width.max(depth) / 2.0;
        let base = [Vec2::ZERO, Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y]
            .map(|offset| terrain.height(point + offset * reach))
            .into_iter()
            .fold(f32::INFINITY, f32::min);
        let mesh = hotel_mesh(width, depth, floors, paint, glass_front);
        world.spawn((
            Mesh3d(assets.meshes.add(mesh)),
            MeshMaterial3d(material.clone()),
            Transform::from_xyz(point.x, base, point.y)
                .with_rotation(Quat::from_rotation_y(heading_of(toward))),
        ));
        taken.push((point, reach + 3.0));
    }
    taken
}

/// Un hotel con la fachada en -Z: un bloque, una franja de ventanas por
/// planta en cada cara, balcones corridos por delante y una caseta en la
/// azotea.
fn hotel_mesh(width: f32, depth: f32, floors: u32, paint: Color, glass_front: bool) -> Mesh {
    const FOOTING: f32 = 3.0;
    let height = floors as f32 * FLOOR_HEIGHT + 1.0;
    let (w, d) = (width / 2.0, depth / 2.0);
    let mut mesh = MeshBuilder::default();
    // Las cuatro caras y la azotea.
    let faces = [
        (Vec3::NEG_Z, Vec3::new(-w, 0.0, -d), Vec3::new(w, 0.0, -d)),
        (Vec3::Z, Vec3::new(w, 0.0, d), Vec3::new(-w, 0.0, d)),
        (Vec3::NEG_X, Vec3::new(-w, 0.0, d), Vec3::new(-w, 0.0, -d)),
        (Vec3::X, Vec3::new(w, 0.0, -d), Vec3::new(w, 0.0, d)),
    ];
    for (facing, a, b) in faces {
        let color = if glass_front && facing == Vec3::NEG_Z {
            GLASS
        } else {
            paint
        };
        let span = a.distance(b) / 4.0;
        mesh.quad(
            [
                (a - Vec3::Y * FOOTING, Vec2::new(0.0, height / 4.0)),
                (b - Vec3::Y * FOOTING, Vec2::new(span, height / 4.0)),
                (b + Vec3::Y * height, Vec2::new(span, 0.0)),
                (a + Vec3::Y * height, Vec2::ZERO),
            ],
            facing,
            color,
        );
        // Ventanas: una franja por planta, un pelo por fuera de la pared.
        if !(glass_front && facing == Vec3::NEG_Z) {
            for floor in 0..floors {
                let y = floor as f32 * FLOOR_HEIGHT;
                let out = facing * 0.03;
                let inset = (b - a).normalize() * 0.8;
                mesh.quad(
                    [
                        (a + inset + Vec3::Y * (y + 0.9) + out, Vec2::ZERO),
                        (b - inset + Vec3::Y * (y + 0.9) + out, Vec2::X),
                        (b - inset + Vec3::Y * (y + 2.4) + out, Vec2::ONE),
                        (a + inset + Vec3::Y * (y + 2.4) + out, Vec2::Y),
                    ],
                    facing,
                    GLASS,
                );
            }
        }
    }
    mesh.quad(
        [
            (Vec3::new(-w, height, -d), Vec2::ZERO),
            (Vec3::new(w, height, -d), Vec2::X),
            (Vec3::new(w, height, d), Vec2::ONE),
            (Vec3::new(-w, height, d), Vec2::Y),
        ],
        Vec3::Y,
        paint,
    );
    // Balcones corridos por delante, en cada planta menos la baja.
    for floor in 1..floors {
        let y = floor as f32 * FLOOR_HEIGHT;
        mesh.beam(
            Vec3::new(-w + 0.5, y, -d - 0.6),
            Vec3::new(w - 0.5, y, -d - 0.6),
            1.2,
            0.2,
            BALCONY,
        );
        mesh.beam(
            Vec3::new(-w + 0.5, y + 1.0, -d - 1.15),
            Vec3::new(w - 0.5, y + 1.0, -d - 1.15),
            0.08,
            0.08,
            BALCONY,
        );
    }
    // La caseta de la azotea.
    mesh.beam(
        Vec3::new(-2.5, height + 1.25, 0.0),
        Vec3::new(2.5, height + 1.25, 0.0),
        4.0,
        2.5,
        paint,
    );
    mesh.build()
}

/// El gran hotel, sobre su explanada: un palacio de piedra clara de cuatro
/// plantas con un cuerpo central más alto, dos torres en las esquinas y
/// tejados de pizarra. Devuelve el sitio que ocupa.
fn spawn_grand_hotel(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    along: f32,
    lateral: f32,
    assets: &mut Assets3d,
) -> Footprint {
    let index = track.index_at(along);
    let point = track.offset_point(index, lateral);
    let toward = track.offset_point(index, 0.0) - point;
    let base = terrain.height(point);
    let material = assets.materials.add(StandardMaterial {
        base_color_texture: Some(assets.textures.plaster.clone()),
        perceptual_roughness: 0.85,
        ..default()
    });
    world.spawn((
        Mesh3d(assets.meshes.add(grand_hotel_mesh())),
        MeshMaterial3d(material),
        Transform::from_xyz(point.x, base, point.y)
            .with_rotation(Quat::from_rotation_y(heading_of(toward))),
    ));
    (point, 45.0)
}

fn grand_hotel_mesh() -> Mesh {
    const STONE: Color = Color::srgb(0.9, 0.82, 0.64);
    const WINDOW: Color = Color::srgb(0.16, 0.17, 0.22);
    const SLATE: Color = Color::srgb(0.32, 0.35, 0.4);
    let mut mesh = MeshBuilder::default();
    // Bloques: (centro x, centro z, ancho, fondo, plantas).
    let blocks = [
        (0.0, 0.0, 74.0, 16.0, 4u32),
        (0.0, -2.0, 22.0, 22.0, 5),
        (-33.0, 0.0, 12.0, 20.0, 5),
        (33.0, 0.0, 12.0, 20.0, 5),
    ];
    for (x, z, width, depth, floors) in blocks {
        let height = floors as f32 * 3.6;
        let center = Vec3::new(x, 0.0, z);
        // El bloque, con los cimientos bajo tierra.
        mesh.beam(
            center + Vec3::new(-width / 2.0, height / 2.0 - 1.5, 0.0),
            center + Vec3::new(width / 2.0, height / 2.0 - 1.5, 0.0),
            depth,
            height + 3.0,
            STONE,
        );
        // Ventanas en la fachada (-Z) y en la parte de atrás.
        let bays = (width / 4.0) as usize;
        for floor in 0..floors {
            for bay in 0..bays {
                let wx = x - width / 2.0 + (bay as f32 + 0.5) * width / bays as f32;
                let y = floor as f32 * 3.6 + 1.0;
                for (wz, facing) in [
                    (z - depth / 2.0 - 0.05, Vec3::NEG_Z),
                    (z + depth / 2.0 + 0.05, Vec3::Z),
                ] {
                    mesh.quad(
                        [
                            (Vec3::new(wx - 0.7, y, wz), Vec2::ZERO),
                            (Vec3::new(wx + 0.7, y, wz), Vec2::X),
                            (Vec3::new(wx + 0.7, y + 2.1, wz), Vec2::ONE),
                            (Vec3::new(wx - 0.7, y + 2.1, wz), Vec2::Y),
                        ],
                        facing,
                        WINDOW,
                    );
                }
            }
        }
        // Tejado de pizarra: un tronco de pirámide.
        let (hw, hd) = (width / 2.0 + 0.4, depth / 2.0 + 0.4);
        let (tw, td) = (width / 2.0 - 2.5, depth / 2.0 - 2.5);
        let (low, high) = (height, height + 3.5);
        let corner = |sx: f32, sz: f32, top: bool| {
            let (w, d, y) = if top { (tw, td, high) } else { (hw, hd, low) };
            center + Vec3::new(sx * w, y, sz * d)
        };
        for (facing, a, b) in [
            (Vec3::new(0.0, 1.0, -1.0), (-1.0, -1.0), (1.0, -1.0)),
            (Vec3::new(0.0, 1.0, 1.0), (1.0, 1.0), (-1.0, 1.0)),
            (Vec3::new(-1.0, 1.0, 0.0), (-1.0, 1.0), (-1.0, -1.0)),
            (Vec3::new(1.0, 1.0, 0.0), (1.0, -1.0), (1.0, 1.0)),
        ] {
            mesh.quad(
                [
                    (corner(a.0, a.1, false), Vec2::ZERO),
                    (corner(b.0, b.1, false), Vec2::X),
                    (corner(b.0, b.1, true), Vec2::ONE),
                    (corner(a.0, a.1, true), Vec2::Y),
                ],
                facing,
                SLATE,
            );
        }
        mesh.quad(
            [
                (corner(-1.0, -1.0, true), Vec2::ZERO),
                (corner(1.0, -1.0, true), Vec2::X),
                (corner(1.0, 1.0, true), Vec2::ONE),
                (corner(-1.0, 1.0, true), Vec2::Y),
            ],
            Vec3::Y,
            SLATE,
        );
    }
    mesh.build()
}

/// Boyas de rayas rojas y blancas flotando en el mar.
fn spawn_buoys(world: &mut ChildSpawnerCommands, buoys: &[Vec2], assets: &mut Assets3d) {
    if buoys.is_empty() {
        return;
    }
    let red = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.1, 0.08),
        perceptual_roughness: 0.5,
        ..default()
    });
    let white = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.95, 0.95),
        perceptual_roughness: 0.5,
        ..default()
    });
    let band = assets.meshes.add(Cylinder::new(0.9, 0.6));
    let cap = assets.meshes.add(Sphere::new(0.9).mesh().uv(16, 8));
    for &point in buoys {
        world.spawn((
            Transform::from_xyz(point.x, SEA_LEVEL - 0.3, point.y),
            Visibility::default(),
            children![
                (
                    Mesh3d(band.clone()),
                    MeshMaterial3d(red.clone()),
                    Transform::from_xyz(0.0, 0.3, 0.0)
                ),
                (
                    Mesh3d(band.clone()),
                    MeshMaterial3d(white.clone()),
                    Transform::from_xyz(0.0, 0.9, 0.0)
                ),
                (
                    Mesh3d(band.clone()),
                    MeshMaterial3d(red.clone()),
                    Transform::from_xyz(0.0, 1.5, 0.0)
                ),
                (
                    Mesh3d(cap.clone()),
                    MeshMaterial3d(white.clone()),
                    Transform::from_xyz(0.0, 1.8, 0.0).with_scale(Vec3::new(1.0, 0.5, 1.0)),
                ),
            ],
        ));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrownStyle {
    /// Broad, flattened umbrella canopy for stone and maritime pines
    Umbrella,
    /// Slender conical spire with vertical fluting for cypresses and poplars
    Spire,
    /// Organic cloud lobes for broadleaf trees, olive trees, and almond trees
    Cloud,
    /// Stepped tiered cone with foliage skirts for firs, conifers, and tall trees
    ConicalTier,
}

/// Tree crown sculpted according to species style, with volumetric shading
/// and directional sun plus ambient light illumination.
fn crown_mesh(seed: u32, style: CrownStyle, (shade, sunlit): (Color, Color)) -> Mesh {
    // Coarse: thousands of trees are drawn, and into the shadows too.
    let mut mesh = Sphere::new(1.0)
        .mesh()
        .ico(2)
        .expect("la subdivisión de la esfera es válida");
    let offset = Vec2::new(seed as f32 * 3.7, seed as f32 * 1.3);
    let positions: Vec<[f32; 3]> = mesh
        .attribute(Mesh::ATTRIBUTE_POSITION)
        .and_then(|p| p.as_float3())
        .expect("la esfera tiene posiciones")
        .to_vec();
    let mut colors = Vec::with_capacity(positions.len());
    let sun_dir = Vec3::new(0.55, 0.82, -0.35).normalize();
    let bumpy: Vec<[f32; 3]> = positions
        .iter()
        .map(|&[x, y, z]| {
            let p = Vec3::new(x, y, z);
            let angle = z.atan2(x);
            let r_xz = (x * x + z * z).sqrt();
            let lumps = value_noise(Vec2::new(x * 2.3 + y * 1.4, z * 2.3 - y * 0.9) + offset);
            let fine = value_noise(Vec2::new(x * 6.1 - z * 2.0, y * 6.1 + z) + offset);

            let shaped = match style {
                CrownStyle::Umbrella => {
                    let horiz = 1.0 + 0.35 * (1.0 - y.abs());
                    let new_y = if y > 0.15 {
                        0.15 + (y - 0.15) * 0.45
                    } else {
                        y * 0.85
                    };
                    let r_mod = 0.88 + 0.22 * lumps + 0.08 * fine;
                    Vec3::new(x * horiz * r_mod, new_y * r_mod, z * horiz * r_mod)
                }
                CrownStyle::Spire => {
                    let t = ((1.0 - y) * 0.5).clamp(0.0, 1.0);
                    let radius_factor =
                        t.powf(0.55) * (1.0 - (y.max(0.0) * 0.95).powf(2.0)).max(0.06);
                    let flute = (angle * 6.0).sin() * 0.10;
                    let r_mod = (radius_factor + flute) * (0.88 + 0.15 * fine);
                    Vec3::new(
                        x / r_xz.max(1e-4) * r_mod,
                        y * 1.15,
                        z / r_xz.max(1e-4) * r_mod,
                    )
                }
                CrownStyle::Cloud => {
                    let lobe = (angle * 3.0).sin() * (y * 2.5).cos() * 0.16;
                    let r_mod = 0.84 + 0.26 * lumps + 0.08 * fine + lobe;
                    p * r_mod
                }
                CrownStyle::ConicalTier => {
                    let tier_phase = ((y + 1.0) * 2.5).rem_euclid(1.0);
                    let skirt = 1.0 + 0.24 * (1.0 - tier_phase);
                    let taper = ((1.0 - y) * 0.5).clamp(0.15, 1.0);
                    let r_mod = taper * skirt * (0.85 + 0.15 * fine);
                    Vec3::new(x / r_xz.max(1e-4) * r_mod, y, z / r_xz.max(1e-4) * r_mod)
                }
            };

            let sun_dot = shaped.normalize().dot(sun_dir);
            let light = (0.5 + 0.5 * sun_dot) * 0.72 + 0.28 * fine;
            let ao = (0.35 + 0.65 * (shaped.y + 1.0) * 0.5).clamp(0.0, 1.0);
            let color = shade.mix(&sunlit, (light * ao).clamp(0.0, 1.0));
            colors.push(color.to_linear().to_f32_array());
            shaped.to_array()
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, bumpy);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    mesh.compute_smooth_normals();
    mesh
}

/// Tribunas cubiertas con gradas de hormigón y público animado.
fn spawn_grandstand(
    world: &mut ChildSpawnerCommands,
    track: &Track,
    terrain: &Terrain,
    grandstand: &Grandstand,
    assets: &mut Assets3d,
) -> Footprint {
    let index = track.index_at(grandstand.along);
    let lateral = grandstand.side.sign() * grandstand.lateral;
    let point = track.offset_point(index, lateral);
    let toward = track.offset_point(index, 0.0) - point;
    let base = terrain.height(point);
    let (structure_mesh, crowd_mesh, roof_mesh) = grandstand_meshes(grandstand.length);
    let concrete = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.82, 0.82, 0.8),
        base_color_texture: Some(assets.textures.plaster.clone()),
        perceptual_roughness: 0.9,
        ..default()
    });
    let crowd_mat = assets.materials.add(StandardMaterial {
        base_color_texture: Some(assets.textures.crowd.clone()),
        perceptual_roughness: 0.7,
        ..default()
    });
    let roof_mat = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.35, 0.65),
        perceptual_roughness: 0.5,
        ..default()
    });
    world.spawn((
        Transform::from_xyz(point.x, base, point.y)
            .with_rotation(Quat::from_rotation_y(heading_of(toward))),
        Visibility::default(),
        children![
            (
                Mesh3d(assets.meshes.add(structure_mesh)),
                MeshMaterial3d(concrete),
            ),
            (
                Mesh3d(assets.meshes.add(crowd_mesh)),
                MeshMaterial3d(crowd_mat),
            ),
            (
                Mesh3d(assets.meshes.add(roof_mesh)),
                MeshMaterial3d(roof_mat),
            ),
        ],
    ));
    (point, grandstand.length / 2.0 + 4.0)
}

fn grandstand_meshes(length: f32) -> (Mesh, Mesh, Mesh) {
    let mut structure = MeshBuilder::default();
    let mut crowd = MeshBuilder::default();
    let mut roof = MeshBuilder::default();
    let hl = length / 2.0;
    const TIERS: usize = 4;
    const TIER_HEIGHT: f32 = 0.65;
    const TIER_DEPTH: f32 = 1.25;

    // Peldaños de hormigón
    for k in 0..TIERS {
        let (y0, y1) = (k as f32 * TIER_HEIGHT, (k + 1) as f32 * TIER_HEIGHT);
        let (z0, z1) = (k as f32 * TIER_DEPTH, (k + 1) as f32 * TIER_DEPTH);
        // Huella
        structure.quad(
            [
                (Vec3::new(-hl, y1, z0), Vec2::ZERO),
                (Vec3::new(hl, y1, z0), Vec2::new(length / 4.0, 0.0)),
                (Vec3::new(hl, y1, z1), Vec2::new(length / 4.0, 1.0)),
                (Vec3::new(-hl, y1, z1), Vec2::new(0.0, 1.0)),
            ],
            Vec3::Y,
            Color::srgb(0.85, 0.85, 0.83),
        );
        // Contrahuella
        structure.quad(
            [
                (Vec3::new(-hl, y0, z0), Vec2::ZERO),
                (Vec3::new(hl, y0, z0), Vec2::new(length / 4.0, 0.0)),
                (Vec3::new(hl, y1, z0), Vec2::new(length / 4.0, 0.5)),
                (Vec3::new(-hl, y1, z0), Vec2::new(0.0, 0.5)),
            ],
            Vec3::NEG_Z,
            Color::srgb(0.75, 0.75, 0.73),
        );
        // Espectadores sobre cada fila
        let cy = y1 + 0.02;
        crowd.quad(
            [
                (Vec3::new(-hl, cy, z0 + 0.1), Vec2::ZERO),
                (Vec3::new(hl, cy, z0 + 0.1), Vec2::new(length / 4.0, 0.0)),
                (Vec3::new(hl, cy, z1 - 0.1), Vec2::new(length / 4.0, 1.0)),
                (Vec3::new(-hl, cy, z1 - 0.1), Vec2::new(0.0, 1.0)),
            ],
            Vec3::Y,
            Color::WHITE,
        );
    }
    // Pared trasera y cerramientos laterales
    let total_depth = TIERS as f32 * TIER_DEPTH;
    let total_height = TIERS as f32 * TIER_HEIGHT;
    structure.quad(
        [
            (Vec3::new(-hl, 0.0, total_depth), Vec2::ZERO),
            (
                Vec3::new(hl, 0.0, total_depth),
                Vec2::new(length / 4.0, 0.0),
            ),
            (
                Vec3::new(hl, total_height, total_depth),
                Vec2::new(length / 4.0, 1.0),
            ),
            (
                Vec3::new(-hl, total_height, total_depth),
                Vec2::new(0.0, 1.0),
            ),
        ],
        Vec3::Z,
        Color::srgb(0.7, 0.7, 0.68),
    );
    for side in [-hl, hl] {
        let normal = Vec3::X * side.signum();
        structure.triangle(
            [
                (Vec3::new(side, 0.0, 0.0), Vec2::ZERO),
                (Vec3::new(side, 0.0, total_depth), Vec2::X),
                (Vec3::new(side, total_height, total_depth), Vec2::ONE),
            ],
            normal,
            Color::srgb(0.75, 0.75, 0.73),
        );
    }
    // Columnas metálicas para la cubierta
    const PILLARS: usize = 5;
    for p in 0..PILLARS {
        let x = -hl + (p as f32 / (PILLARS - 1) as f32) * length;
        structure.beam(
            Vec3::new(x, total_height, total_depth),
            Vec3::new(x, 5.2, total_depth),
            0.18,
            0.18,
            Color::srgb(0.4, 0.42, 0.45),
        );
        structure.beam(
            Vec3::new(x, 0.0, -0.1),
            Vec3::new(x, 4.2, -0.1),
            0.15,
            0.15,
            Color::srgb(0.4, 0.42, 0.45),
        );
    }
    // Cubierta inclinada
    roof.quad(
        [
            (Vec3::new(-hl - 0.5, 4.4, -0.5), Vec2::ZERO),
            (Vec3::new(hl + 0.5, 4.4, -0.5), Vec2::new(length / 4.0, 0.0)),
            (
                Vec3::new(hl + 0.5, 5.4, total_depth + 0.6),
                Vec2::new(length / 4.0, 1.5),
            ),
            (
                Vec3::new(-hl - 0.5, 5.4, total_depth + 0.6),
                Vec2::new(0.0, 1.5),
            ),
        ],
        Vec3::new(0.0, 1.0, -0.2).normalize(),
        Color::WHITE,
    );
    (structure.build(), crowd.build(), roof.build())
}

/// Faro costero con bandas rojas y blancas, balcón y linterna luminosa.
/// How far round a lighthouse no tree grows, for each metre of its height:
/// it stands on a bare, windswept cape, seen whole from far off.
const LIGHTHOUSE_CLEARING: f32 = 1.5;

/// The bare ground round a lighthouse, where no tree grows.
fn lighthouse_clearing(lighthouse: &Lighthouse) -> Footprint {
    (lighthouse.position, lighthouse.height * LIGHTHOUSE_CLEARING)
}

fn spawn_lighthouse(
    world: &mut ChildSpawnerCommands,
    terrain: &Terrain,
    lighthouse: &Lighthouse,
    assets: &mut Assets3d,
) {
    let base = terrain.height(lighthouse.position);
    let (body_mesh, lantern_mesh, lamp_mesh) = lighthouse_meshes(lighthouse.height);
    let plaster = assets.materials.add(StandardMaterial {
        base_color_texture: Some(assets.textures.plaster.clone()),
        perceptual_roughness: 0.85,
        ..default()
    });
    let metal = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.2, 0.22),
        metallic: 0.5,
        perceptual_roughness: 0.4,
        ..default()
    });
    let beacon = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.96, 0.7),
        emissive: LinearRgba::rgb(8.0, 7.0, 3.0),
        ..default()
    });
    world.spawn((
        Transform::from_xyz(lighthouse.position.x, base, lighthouse.position.y),
        Visibility::default(),
        children![
            (
                Mesh3d(assets.meshes.add(body_mesh)),
                MeshMaterial3d(plaster),
            ),
            (
                Mesh3d(assets.meshes.add(lantern_mesh)),
                MeshMaterial3d(metal),
            ),
            (Mesh3d(assets.meshes.add(lamp_mesh)), MeshMaterial3d(beacon),),
        ],
    ));
}

fn lighthouse_meshes(height: f32) -> (Mesh, Mesh, Mesh) {
    const SIDES: usize = 12;
    const TIERS: usize = 4;
    let mut body = MeshBuilder::default();
    let mut lantern = MeshBuilder::default();
    let mut lamp = MeshBuilder::default();

    let r_bottom = 4.2;
    let r_top = 2.4;
    let colors = [
        Color::srgb(0.85, 0.12, 0.1),
        Color::srgb(0.95, 0.95, 0.95),
        Color::srgb(0.85, 0.12, 0.1),
        Color::srgb(0.95, 0.95, 0.95),
    ];
    // Torre troncocónica con bandas
    for tier in 0..TIERS {
        let t0 = tier as f32 / TIERS as f32;
        let t1 = (tier + 1) as f32 / TIERS as f32;
        let y0 = t0 * height;
        let y1 = t1 * height;
        let r0 = r_bottom.lerp(r_top, t0);
        let r1 = r_bottom.lerp(r_top, t1);
        let color = colors[tier % colors.len()];
        for side in 0..SIDES {
            let a0 = side as f32 / SIDES as f32 * std::f32::consts::TAU;
            let a1 = (side + 1) as f32 / SIDES as f32 * std::f32::consts::TAU;
            let p0 = Vec3::new(a0.cos() * r0, y0, a0.sin() * r0);
            let p1 = Vec3::new(a1.cos() * r0, y0, a1.sin() * r0);
            let p2 = Vec3::new(a1.cos() * r1, y1, a1.sin() * r1);
            let p3 = Vec3::new(a0.cos() * r1, y1, a0.sin() * r1);
            let mid = (a0 + a1) / 2.0;
            let normal = Vec3::new(mid.cos(), 0.1, mid.sin()).normalize();
            body.quad(
                [
                    (p0, Vec2::ZERO),
                    (p1, Vec2::X),
                    (p2, Vec2::ONE),
                    (p3, Vec2::Y),
                ],
                normal,
                color,
            );
        }
    }
    // Plataforma del balcón superior
    let r_balcony = 3.3;
    for side in 0..SIDES {
        let a0 = side as f32 / SIDES as f32 * std::f32::consts::TAU;
        let a1 = (side + 1) as f32 / SIDES as f32 * std::f32::consts::TAU;
        let p0 = Vec3::new(a0.cos() * r_top, height, a0.sin() * r_top);
        let p1 = Vec3::new(a1.cos() * r_top, height, a1.sin() * r_top);
        let p2 = Vec3::new(a1.cos() * r_balcony, height, a1.sin() * r_balcony);
        let p3 = Vec3::new(a0.cos() * r_balcony, height, a0.sin() * r_balcony);
        body.quad(
            [
                (p0, Vec2::ZERO),
                (p1, Vec2::X),
                (p2, Vec2::ONE),
                (p3, Vec2::Y),
            ],
            Vec3::Y,
            Color::srgb(0.9, 0.9, 0.9),
        );
        // Barandilla del balcón
        let b0 = Vec3::new(a0.cos() * r_balcony, height + 1.1, a0.sin() * r_balcony);
        let b1 = Vec3::new(a1.cos() * r_balcony, height + 1.1, a1.sin() * r_balcony);
        lantern.beam(p2, b1, 0.08, 0.08, Color::srgb(0.2, 0.2, 0.2));
        lantern.beam(b0, b1, 0.08, 0.08, Color::srgb(0.2, 0.2, 0.2));
    }
    // Cúpula / techo rojo de la linterna
    let r_lantern = 2.0;
    let lantern_top = height + 3.0;
    let spire_top = lantern_top + 2.4;
    for side in 0..SIDES {
        let a0 = side as f32 / SIDES as f32 * std::f32::consts::TAU;
        let a1 = (side + 1) as f32 / SIDES as f32 * std::f32::consts::TAU;
        let p0 = Vec3::new(a0.cos() * r_lantern, lantern_top, a0.sin() * r_lantern);
        let p1 = Vec3::new(a1.cos() * r_lantern, lantern_top, a1.sin() * r_lantern);
        let tip = Vec3::new(0.0, spire_top, 0.0);
        body.triangle(
            [(p0, Vec2::ZERO), (p1, Vec2::X), (tip, Vec2::new(0.5, 1.0))],
            Vec3::new(a0.cos(), 0.5, a0.sin()).normalize(),
            Color::srgb(0.85, 0.12, 0.1),
        );
        // Montantes de metal de la linterna
        let base_p = Vec3::new(a0.cos() * r_lantern, height, a0.sin() * r_lantern);
        lantern.beam(base_p, p0, 0.1, 0.1, Color::srgb(0.2, 0.2, 0.2));
    }
    // Foco emisor
    lamp.beam(
        Vec3::new(0.0, height + 1.2, 0.0),
        Vec3::new(0.0, height + 2.2, 0.0),
        1.3,
        1.3,
        Color::WHITE,
    );
    (body.build(), lantern.build(), lamp.build())
}

/// Embarcaciones fondeadas en el agua: veleros con velas blancas y yates modernos.
fn spawn_boat(world: &mut ChildSpawnerCommands, boat: &Boat, assets: &mut Assets3d) {
    let position = Vec3::new(boat.position.x, SEA_LEVEL - 0.2, boat.position.y);
    let rotation = Quat::from_rotation_y(boat.heading);
    let hull_mat = assets.materials.add(StandardMaterial {
        base_color: Color::srgb(0.96, 0.96, 0.96),
        perceptual_roughness: 0.35,
        ..default()
    });
    match boat.kind {
        BoatKind::Sailboat => {
            let (hull, sails) = sailboat_meshes();
            let sail_mat = assets.materials.add(StandardMaterial {
                base_color: Color::srgb(0.98, 0.98, 0.95),
                cull_mode: None,
                double_sided: true,
                perceptual_roughness: 0.7,
                ..default()
            });
            world.spawn((
                Transform::from_translation(position).with_rotation(rotation),
                Visibility::default(),
                children![
                    (Mesh3d(assets.meshes.add(hull)), MeshMaterial3d(hull_mat),),
                    (Mesh3d(assets.meshes.add(sails)), MeshMaterial3d(sail_mat),),
                ],
            ));
        }
        BoatKind::Yacht => {
            let hull = yacht_mesh();
            world.spawn((
                Mesh3d(assets.meshes.add(hull)),
                MeshMaterial3d(hull_mat),
                Transform::from_translation(position).with_rotation(rotation),
            ));
        }
    }
}

fn sailboat_meshes() -> (Mesh, Mesh) {
    let mut hull = MeshBuilder::default();
    let mut sails = MeshBuilder::default();
    const LENGTH: f32 = 11.0;
    const WIDTH: f32 = 3.6;
    const DEPTH: f32 = 2.0;

    let bow = Vec3::new(0.0, 0.6, -LENGTH / 2.0);
    let stern_l = Vec3::new(-WIDTH / 2.5, 0.6, LENGTH / 2.0);
    let stern_r = Vec3::new(WIDTH / 2.5, 0.6, LENGTH / 2.0);
    let mid_l = Vec3::new(-WIDTH / 2.0, 0.6, 0.0);
    let mid_r = Vec3::new(WIDTH / 2.0, 0.6, 0.0);
    let keel = Vec3::new(0.0, -DEPTH, 0.5);

    // Casco del velero
    let wood = Color::srgb(0.72, 0.58, 0.42);
    let white = Color::srgb(0.95, 0.95, 0.96);
    let stripe = Color::srgb(0.85, 0.15, 0.12);
    let (u0, u1, u2) = (Vec2::ZERO, Vec2::X, Vec2::ONE);

    // Cubierta de madera
    hull.triangle([(bow, u0), (mid_l, u1), (stern_l, u2)], Vec3::Y, wood);
    hull.triangle([(bow, u0), (stern_l, u1), (stern_r, u2)], Vec3::Y, wood);
    hull.triangle([(bow, u0), (stern_r, u1), (mid_r, u2)], Vec3::Y, wood);

    // Costados del casco
    hull.triangle(
        [(bow, u0), (keel, u1), (mid_l, u2)],
        Vec3::new(-1.0, 0.0, -0.5).normalize(),
        white,
    );
    hull.triangle(
        [(bow, u0), (mid_r, u1), (keel, u2)],
        Vec3::new(1.0, 0.0, -0.5).normalize(),
        white,
    );
    hull.triangle(
        [(mid_l, u0), (keel, u1), (stern_l, u2)],
        Vec3::NEG_X,
        stripe,
    );
    hull.triangle([(mid_r, u0), (stern_r, u1), (keel, u2)], Vec3::X, stripe);
    hull.triangle([(stern_l, u0), (keel, u1), (stern_r, u2)], Vec3::Z, white);

    // Mástil
    const MAST_H: f32 = 12.5;
    let mast_base = Vec3::new(0.0, 0.6, -0.8);
    let mast_top = Vec3::new(0.0, MAST_H, -1.0);
    hull.beam(mast_base, mast_top, 0.18, 0.18, Color::srgb(0.8, 0.8, 0.85));

    // Botavara y vela mayor
    let boom_end = Vec3::new(0.0, 1.4, LENGTH / 2.3);
    hull.beam(
        mast_base + Vec3::Y * 0.8,
        boom_end,
        0.12,
        0.12,
        Color::srgb(0.8, 0.8, 0.85),
    );

    sails.triangle(
        [
            (mast_base + Vec3::Y * 0.9, u0),
            (boom_end, u1),
            (mast_top, u2),
        ],
        Vec3::X,
        Color::WHITE,
    );
    // Foque / génova en proa
    sails.triangle(
        [
            (bow + Vec3::Y * 0.2, u0),
            (mast_base + Vec3::Y * 0.5, u1),
            (mast_top - Vec3::Y * 2.0, u2),
        ],
        Vec3::NEG_X,
        Color::srgb(0.94, 0.96, 0.98),
    );
    (hull.build(), sails.build())
}

fn yacht_mesh() -> Mesh {
    let mut mesh = MeshBuilder::default();
    const LENGTH: f32 = 15.0;
    const WIDTH: f32 = 5.0;

    let bow = Vec3::new(0.0, 1.0, -LENGTH / 2.0);
    let mid_l = Vec3::new(-WIDTH / 2.0, 0.9, 0.0);
    let mid_r = Vec3::new(WIDTH / 2.0, 0.9, 0.0);
    let stern_l = Vec3::new(-WIDTH / 2.2, 0.8, LENGTH / 2.0);
    let stern_r = Vec3::new(WIDTH / 2.2, 0.8, LENGTH / 2.0);
    let bottom = Vec3::new(0.0, -1.4, 1.0);

    let white = Color::srgb(0.96, 0.96, 0.96);
    let blue_stripe = Color::srgb(0.12, 0.35, 0.72);
    let (u0, u1, u2) = (Vec2::ZERO, Vec2::X, Vec2::ONE);

    // Casco
    mesh.triangle(
        [(bow, u0), (bottom, u1), (mid_l, u2)],
        Vec3::new(-1.0, 0.0, -0.5).normalize(),
        white,
    );
    mesh.triangle(
        [(bow, u0), (mid_r, u1), (bottom, u2)],
        Vec3::new(1.0, 0.0, -0.5).normalize(),
        white,
    );
    mesh.triangle(
        [(mid_l, u0), (bottom, u1), (stern_l, u2)],
        Vec3::NEG_X,
        blue_stripe,
    );
    mesh.triangle(
        [(mid_r, u0), (stern_r, u1), (bottom, u2)],
        Vec3::X,
        blue_stripe,
    );
    mesh.triangle([(stern_l, u0), (bottom, u1), (stern_r, u2)], Vec3::Z, white);

    // Cubierta
    mesh.triangle([(bow, u0), (mid_l, u1), (stern_l, u2)], Vec3::Y, white);
    mesh.triangle([(bow, u0), (stern_l, u1), (stern_r, u2)], Vec3::Y, white);
    mesh.triangle([(bow, u0), (stern_r, u1), (mid_r, u2)], Vec3::Y, white);

    // Cabina y flybridge
    mesh.beam(
        Vec3::new(0.0, 2.0, -1.0),
        Vec3::new(0.0, 2.0, 3.5),
        WIDTH * 0.65,
        1.8,
        white,
    );
    // Parabrisas tintado oscuro
    mesh.quad(
        [
            (Vec3::new(-WIDTH * 0.3, 1.2, -1.6), Vec2::ZERO),
            (Vec3::new(WIDTH * 0.3, 1.2, -1.6), Vec2::X),
            (Vec3::new(WIDTH * 0.28, 2.6, -0.9), Vec2::ONE),
            (Vec3::new(-WIDTH * 0.28, 2.6, -0.9), Vec2::Y),
        ],
        Vec3::new(0.0, 0.6, -1.0).normalize(),
        GLASS,
    );
    mesh.build()
}

/// Grupo de sombrillas y tumbonas de playa en la arena.
fn spawn_parasol_cluster(
    world: &mut ChildSpawnerCommands,
    terrain: &Terrain,
    cluster: &ParasolCluster,
    assets: &mut Assets3d,
) {
    let parasol_mesh = assets.meshes.add(parasol_mesh());
    let cloth_mat = assets.materials.add(StandardMaterial {
        cull_mode: None,
        double_sided: true,
        perceptual_roughness: 0.75,
        ..default()
    });
    for i in 0..cluster.count {
        let r0 = random01(i as u32 * 7 + 101);
        let r1 = random01(i as u32 * 11 + 203);
        let angle = r0 * std::f32::consts::TAU;
        let dist = 3.0 + 8.0 * r1;
        let p = cluster.position + Vec2::new(angle.cos() * dist, angle.sin() * dist);
        let base = terrain.height(p);
        world.spawn((
            Mesh3d(parasol_mesh.clone()),
            MeshMaterial3d(cloth_mat.clone()),
            Transform::from_xyz(p.x, base, p.y).with_rotation(Quat::from_rotation_y(angle)),
        ));
    }
}

fn parasol_mesh() -> Mesh {
    let mut mesh = MeshBuilder::default();
    const SIDES: usize = 8;
    const RADIUS: f32 = 1.6;
    const HEIGHT: f32 = 2.2;
    // Mástil de madera inclinado
    mesh.beam(
        Vec3::ZERO,
        Vec3::new(0.15, HEIGHT, 0.0),
        0.06,
        0.06,
        Color::srgb(0.65, 0.52, 0.38),
    );
    // Cúpula octogonal a rayas de colores vivos
    let top = Vec3::new(0.15, HEIGHT + 0.4, 0.0);
    let rim_y = HEIGHT - 0.2;
    for s in 0..SIDES {
        let a0 = s as f32 / SIDES as f32 * std::f32::consts::TAU;
        let a1 = (s + 1) as f32 / SIDES as f32 * std::f32::consts::TAU;
        let p0 = Vec3::new(a0.cos() * RADIUS + 0.15, rim_y, a0.sin() * RADIUS);
        let p1 = Vec3::new(a1.cos() * RADIUS + 0.15, rim_y, a1.sin() * RADIUS);
        let color = if s % 2 == 0 {
            Color::srgb(0.92, 0.15, 0.12) // Rojo
        } else {
            Color::srgb(0.96, 0.96, 0.96) // Blanco
        };
        let norm = Vec3::new(a0.cos(), 0.6, a0.sin()).normalize();
        mesh.triangle(
            [(p0, Vec2::ZERO), (p1, Vec2::X), (top, Vec2::new(0.5, 1.0))],
            norm,
            color,
        );
    }
    // Tumbona de madera
    mesh.beam(
        Vec3::new(0.4, 0.15, 0.3),
        Vec3::new(0.4, 0.15, 1.8),
        0.55,
        0.12,
        Color::srgb(0.85, 0.75, 0.6),
    );
    mesh.beam(
        Vec3::new(0.4, 0.35, 1.5),
        Vec3::new(0.4, 0.55, 1.9),
        0.55,
        0.08,
        Color::srgb(0.85, 0.75, 0.6),
    );
    mesh.build()
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    pub fn no_tree_stands_in_a_dam_s_lake_on(circuits: &'static crate::circuits::Circuits) {
        use crate::track::closed_curve;
        use crate::world::terrain::signed_distance;
        let mut near = 0;
        for circuit in circuits
            .iter()
            .filter(|circuit| !circuit.scenery.dams.is_empty())
        {
            let scenery = &circuit.scenery;
            let track = Track::new(&circuit.layout);
            let terrain = Terrain::new(&track, scenery);
            let plan = plan_trees(&track, &terrain, scenery);
            for dam in scenery.dams {
                let shore = closed_curve(dam.lake, 4.0);
                // The water stands at the lake's level in it, the sea's far
                // from it.
                let middle = dam.lake.iter().copied().sum::<Vec2>() / dam.lake.len() as f32;
                assert!(signed_distance(&shore, middle) > 0.0);
                assert_eq!(terrain.water_level(middle), dam.level);
                assert_eq!(terrain.water_level(track.centerline()[0]), SEA_LEVEL);
                // Every tree stands out of its water, and on the rim round it
                // over it (further off, down the gorge under the dam, the
                // ground is lower than the lake); some of them by it.
                for tree in plan.scattered.iter().chain(&plan.forests) {
                    let out = -signed_distance(&shore, tree.position);
                    if out < 40.0 {
                        near += 1;
                        assert!(
                            out > 0.0 && tree.height > dam.level + 1.5,
                            "{}: a tree in the lake at {} ({out:.1} m out, {:.1} m up)",
                            circuit.name,
                            tree.position,
                            tree.height
                        );
                    }
                }
            }
        }
        assert!(near > 0, "no tree by a lake");
    }

    #[test]
    fn no_tree_grows_round_a_lighthouse() {
        let lighthouse = Lighthouse {
            position: Vec2::new(480.0, -1046.0),
            height: 28.0,
        };
        let (at, radius) = lighthouse_clearing(&lighthouse);
        assert_eq!(at, lighthouse.position);
        // Wider than the tower is tall, so the whole of it is seen.
        assert!(radius > lighthouse.height);
    }
    use bevy::mesh::VertexAttributeValues;

    #[test]
    fn the_sea_mirrors_the_crowns_of_the_trees_in_colour() {
        let mut meshes = Assets::<Mesh>::default();
        let bark = Vec4::from(BARK.to_linear().to_f32_array());
        for (species, colour) in [
            (Species::Almond, true),
            (Species::Momiji, true),
            (Species::Ginkgo, true),
            (Species::Sakura, true),
            (Species::Broadleaf, false),
            (Species::Olive, false),
            (Species::Poplar, false),
            (Species::Sugi, false),
            (Species::Bamboo, false),
        ] {
            assert_eq!(in_colour(species), colour, "{species:?}");
            if !colour {
                continue;
            }
            for variant in build_tree_variants(species, &mut meshes) {
                let crown = crown_points(&merged_variant(&meshes, &variant));
                // A few dozen points over it, over the ground (a poplar's
                // crown, the ginkgo's shape, comes down to half a metre),
                // in the colours of its leaves.
                assert!(
                    crown.len() >= CROWN_POINTS / 2 && crown.len() <= CROWN_POINTS,
                    "{species:?}: {}",
                    crown.len()
                );
                for point in &crown {
                    assert!(point.at.y > 0.2, "{species:?}: {point:?}");
                    assert!(Vec3::from(point.color).extend(1.0).distance(bark) > 1e-3);
                    assert!((point.normal.length() - 1.0).abs() < 0.01);
                }
                // Spread round it, not all to one side.
                let (low, high) = crown.iter().fold(
                    (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
                    |(low, high), point| (low.min(point.at), high.max(point.at)),
                );
                assert!(
                    (high - low).xz().min_element() > 1.5,
                    "{species:?}: {low} {high}"
                );
            }
        }
    }

    #[test]
    fn each_tree_is_one_solid_mesh_of_bark_and_foliage() {
        let mut meshes = Assets::<Mesh>::default();
        let bark = BARK.to_linear().to_f32_array();
        for species in [
            Species::UmbrellaPine,
            Species::Cypress,
            Species::Olive,
            Species::Tall,
            Species::Broadleaf,
            Species::MaritimePine,
            Species::Poplar,
            Species::Almond,
            Species::Momiji,
            Species::Ginkgo,
            Species::Sakura,
            Species::BlackPine,
            Species::Sugi,
            Species::Bamboo,
        ] {
            for variant in build_tree_variants(species, &mut meshes) {
                let tree = merged_variant(&meshes, &variant);
                let positions = tree
                    .attribute(Mesh::ATTRIBUTE_POSITION)
                    .and_then(|p| p.as_float3())
                    .unwrap();
                let Some(VertexAttributeValues::Float32x4(colors)) =
                    tree.attribute(Mesh::ATTRIBUTE_COLOR)
                else {
                    panic!("{species:?}: a tree is painted in its vertices");
                };
                assert_eq!(colors.len(), positions.len());
                // Its wood is bark and its crowns are not, and they stand
                // over the ground.
                let wood = colors.iter().filter(|c| **c == bark).count();
                assert!(wood > 0 && wood < colors.len(), "{species:?}");
                let top = positions.iter().map(|p| p[1]).fold(f32::MIN, f32::max);
                assert!(top > 3.0, "{species:?}: {top}");
                // Lighter far away, with as tall a crown, still in the
                // colours of bark and leaves.
                let triangles = |mesh: &Mesh| mesh.indices().unwrap().len() / 3;
                let [first, second] = lighter(&tree);
                assert!(
                    triangles(&first) <= 200
                        && triangles(&second) <= 60
                        && triangles(&second) < triangles(&first)
                        && triangles(&first) < triangles(&tree) / 2,
                    "{species:?}: {} {} {}",
                    triangles(&tree),
                    triangles(&first),
                    triangles(&second)
                );
                for light in [&first, &second] {
                    let height = light
                        .attribute(Mesh::ATTRIBUTE_POSITION)
                        .and_then(|p| p.as_float3())
                        .unwrap()
                        .iter()
                        .map(|p| p[1])
                        .fold(f32::MIN, f32::max);
                    assert!(height > 0.9 * top, "{species:?}: {height} of {top}");
                }
            }
        }
    }

    pub fn every_circuit_gets_its_houses_on(circuits: &'static crate::circuits::Circuits) {
        for circuit in circuits.iter() {
            let track = Track::new(&circuit.layout);
            let terrain = Terrain::new(&track, &circuit.scenery);
            let houses = place_houses(&track, &terrain, &circuit.scenery.houses, &[]);
            assert!(
                houses.len() >= circuit.scenery.houses.max / 2,
                "{}: {} casas",
                circuit.name,
                houses.len()
            );
        }
    }

    #[test]
    fn every_circuit_gets_its_houses() {
        every_circuit_gets_its_houses_on(&crate::testing::DEMO);
    }
}
