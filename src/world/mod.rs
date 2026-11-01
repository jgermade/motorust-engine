//! El mundo alrededor de la carretera: el terreno, la carretera con lo que
//! la bordea y el decorado de la pista elegida ([`crate::circuits`]), bajo
//! un cielo de verano. Al cambiar de pista se vuelve a construir.

mod boosters;
pub mod cavern;
pub mod dam;
pub mod detail;
pub mod flowers;
mod foliage;
mod lamps;
pub mod landmarks;
mod lawn;
pub mod lettering;
mod lod;
pub mod mirror;
pub mod mountains;
pub mod petals;
pub mod portals;
pub mod preview;
pub mod prop_pack;
pub mod props;
pub mod quads;
pub mod realm;
pub mod relief;
pub mod road;
pub mod scenery;
pub mod sea;
mod shadow_fade;
pub mod shoulders;
pub mod sky;
pub mod structures;
pub mod terrain;
pub mod textures;
pub mod themes;
pub mod trees;
pub mod tufts;
pub mod waters;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::asset::embedded_asset;
use bevy::camera::primitives::MeshAabb;
use bevy::pbr::PbrPlugin;
use bevy::tasks::{Task, TaskPool, TaskPoolBuilder, block_on, poll_once};
use bevy::{
    camera::visibility::VisibilitySystems, light::NotShadowCaster, prelude::*,
    transform::TransformSystems,
};

use crate::circuits::reload::CircuitsReloaded;
use crate::circuits::{Circuit, Circuits, CurrentCircuit};
use crate::menu::GameState;
use crate::settings::{Settings, WEB};
use crate::track::{Surface, Track};
#[cfg(any(test, feature = "testing"))]
pub use preview::stretch as preview_stretch;
// For the benchmark, which leaves the ground out of the shadows for a turn.
#[cfg(not(target_arch = "wasm32"))]
pub use terrain::GroundPiece;
#[cfg(any(test, feature = "testing"))]
pub use terrain::is_earth_realm;
pub use textures::generate_mipmaps;
pub use trees::Dressed;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "sea.wgsl");
        embedded_asset!(app, "mirror.wgsl");
        // Without the renderer (in the tests), the sea's and the mirroring
        // water's materials are only assets.
        if app.is_plugin_added::<PbrPlugin>() {
            app.add_plugins((
                MaterialPlugin::<sea::SeaMaterial>::default(),
                MaterialPlugin::<mirror::MirrorMaterial>::default(),
            ));
        } else {
            app.init_asset::<sea::SeaMaterial>()
                .init_asset::<mirror::MirrorMaterial>();
        }
        // The modelled trees, if the content has them (a game's: the
        // engine's own has none, and plants the classic ones).
        if crate::content::try_file("models/trees/trees.bin").is_ok() {
            app.init_resource::<trees::ModelledTrees>();
        }
        // The circuits, read from their files once, as the game starts (or
        // those the app was given: a test's).
        let circuits = app
            .world()
            .get_resource::<Circuits>()
            .copied()
            .unwrap_or_else(Circuits::load);
        // The records saved under a circuit's former names, under its own.
        if let Some(mut settings) = app.world_mut().get_resource_mut::<Settings>() {
            settings.records.take_former(&circuits);
        }
        let current = CurrentCircuit::default();
        app.add_plugins((
            lamps::LampsPlugin,
            detail::DetailPlugin,
            shadow_fade::ShadowFadePlugin,
            realm::RealmPlugin,
            relief::ReliefPlugin,
            lawn::LawnPlugin,
        ));
        // In a desktop development build, read again when their files
        // change on the disk.
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        app.init_resource::<crate::circuits::reload::Watch>()
            .add_systems(
                PreUpdate,
                crate::circuits::reload::watch.before(forget_reloaded_circuits),
            );
        app.insert_resource(circuits)
            .insert_resource(current)
            .insert_resource(Track::new(&circuits.get(current).layout))
            .init_resource::<Roadsides>()
            .init_resource::<Preparing>()
            .init_resource::<Finishing>()
            .init_resource::<Previewing>()
            .init_resource::<PreviewRoots>()
            .init_resource::<CircuitLoading>()
            .init_resource::<textures::WorldTextures>()
            .init_resource::<prop_pack::PropPacks>()
            .init_resource::<Sea>()
            .add_systems(Startup, (build_world, sky::spawn_sky, spawn_sun))
            .add_message::<CircuitsReloaded>()
            .add_systems(PreUpdate, (forget_reloaded_circuits, rebuild_world).chain())
            .add_systems(
                Update,
                (
                    landmarks::spin,
                    landmarks::flow,
                    sky::drift_clouds,
                    sky::light_the_sky,
                    portals::spin_portals,
                    prop_pack::turn_wheels,
                    petals::drift_petals,
                    spawn_prepared_circuit,
                ),
            )
            .add_systems(
                Update,
                queue_circuits
                    .after(spawn_prepared_circuit)
                    .run_if(preparing_in_the_menus),
            )
            // The others go when a race starts (and in a championship,
            // which goes from track to track without choosing them, the one
            // left behind): the one raced on is there whole.
            .add_systems(
                OnEnter(GameState::Race),
                (unload_unused_circuits, complete_the_circuit),
            )
            .add_systems(
                PostUpdate,
                (
                    sea::dress_the_sea,
                    sky::follow_camera.before(TransformSystems::Propagate),
                    // With the trees where they are and whether they are
                    // seen, before what each camera sees is worked out.
                    trees::dress_trees
                        .after(TransformSystems::Propagate)
                        .after(VisibilitySystems::VisibilityPropagate)
                        .before(VisibilitySystems::CheckVisibility),
                ),
            );
    }
}

/// What lies beside the roads of each circuit built so far (see
/// [`Roadside`]), by its index in [`Circuits`]: it comes from the terrain,
/// which is only built once.
#[derive(Resource, Default)]
struct Roadsides(HashMap<usize, Roadside>);

/// What lies beside the roads of a circuit: where their solid edge is (see
/// [`Track::set_edges`]) and what their shoulders are made of (see
/// [`Track::set_shoulders`]).
#[derive(Clone)]
struct Roadside {
    edges: Vec<Vec<[f32; 2]>>,
    shoulders: Vec<Vec<[Surface; 2]>>,
}

impl Roadside {
    /// Gives the circuit's track what lies beside its roads.
    fn apply(&self, track: &mut Track) {
        track.set_edges(self.edges.clone());
        track.set_shoulders(self.shoulders.clone());
    }
}

/// Cada circuito cargado tiene su entidad raíz con su índice en [`Circuits`].
#[derive(Component, Debug, PartialEq, Eq)]
pub struct WorldRoot(pub usize);

/// The mesh of the sea, the same for every circuit.
#[derive(Resource)]
struct Sea(Handle<Mesh>);

impl FromWorld for Sea {
    fn from_world(world: &mut World) -> Self {
        Self(world.resource_mut::<Assets<Mesh>>().add(sea::sea_mesh()))
    }
}

/// What can be worked out for a circuit away from the main thread, which
/// is most of the work, first: enough to show it (its track, its terrain
/// and the terrain's meshes, where its shore is and what lies beside its
/// roads, the roads, tunnels and bridges, the lakes, and the sea, mirroring
/// only the sky as yet). The rest comes after it ([`Rest`]). The entities
/// and assets are made on the main thread.
struct Prepared {
    basis: Basis,
    ground: Vec<terrain::GroundMesh>,
    shore_image: Image,
    /// The sea mirroring the sky, unless the track's sea is always the
    /// cartoon one: what stands round it is mapped with the rest.
    sea_mirror: Option<mirror::PreparedMirror>,
    /// The meshes of the roads, tunnels and bridges, with their materials.
    pieces: Vec<(Mesh, StandardMaterial)>,
    /// The lakes behind its dams, which mirror what stands round them.
    lakes: Vec<dam::PreparedLake>,
    /// The lamps of its tunnels and mine (see [`lamps`]).
    lamps: Vec<lamps::Lamp>,
    /// The lawn in flower, for each kind of flower the ground is drawn in.
    meadows: Vec<(scenery::FlowerKind, Image)>,
}

/// What the rest of a circuit is worked out from, kept from its first part
/// and shared with the work on the rest: which circuit, its track, its
/// terrain and its shore, and what lies beside its roads.
#[derive(Clone)]
struct Basis {
    index: usize,
    circuit: &'static Circuit,
    track: Arc<Track>,
    terrain: Arc<terrain::Terrain>,
    shore: Arc<sea::ShoreMap>,
    roadside: Roadside,
}

/// The rest of a circuit, worked out once it can be shown (see
/// [`Prepared`]): the sea's map of what stands round it, the grass beside
/// the roads, and where the trees and the flowers go (the houses and the
/// props are put in the world with them).
struct Rest {
    sea_mirror: Option<mirror::PreparedMirror>,
    /// Its grass beside the roads, drawn only near the camera.
    grass: Vec<tufts::GrassPiece>,
    trees: props::TreePlan,
    flowers: flowers::FlowerPlan,
}

/// How many threads a circuit's terrain is worked out on: all the cores
/// when it is needed now, and few in the background, so the game and its
/// menus keep their frame rate while the other circuits are prepared.
#[derive(Clone, Copy)]
enum Urgency {
    Now,
    Background,
}

impl Urgency {
    fn threads(self) -> usize {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        match self {
            Urgency::Now => cores,
            Urgency::Background => (cores / 2).saturating_sub(1).max(1),
        }
    }
}

/// The first part of circuit `index` (see [`Prepared`]).
fn prepare(
    circuits: Circuits,
    index: usize,
    textures: &textures::WorldTextures,
    urgency: Urgency,
) -> Prepared {
    let circuit = circuits.at(index);
    let scenery = &circuit.scenery;
    let track = Track::new(&circuit.layout);
    let terrain = terrain::Terrain::on_threads(&track, scenery, urgency.threads());
    let mut ground = if realm::splits(scenery) {
        realm_ground(&terrain, urgency.threads())
    } else {
        terrain.meshes(urgency.threads())
    };
    let shore = sea::ShoreMap::new(&terrain, &track);
    let shore_image = shore.image();
    let roadside = Roadside {
        edges: terrain.track_edges(&track),
        shoulders: shoulders::track_shoulders(&track, &terrain, scenery),
    };
    let mut pieces = road::road_pieces(&track, &terrain, scenery, textures);
    let (structures, lamps, tops) =
        structures::structure_pieces(&track, &terrain, scenery, textures, None);
    pieces.extend(structures);
    ground.extend(tops);
    // The tangents of what is drawn with relief, here rather than on the
    // main thread as it is put in the world.
    for piece in &mut ground {
        // The grass in flower has the lawn's.
        let texture = match piece.flowers {
            Some(_) => Some(&textures.grass),
            None => terrain::ground_look(piece.ground, terrain.soil(), textures).0,
        };
        if texture.is_some_and(|texture| textures.relief.of(texture).is_some()) {
            relief::add_tangents(&mut piece.mesh);
        }
    }
    for (mesh, material) in &mut pieces {
        if material
            .base_color_texture
            .as_ref()
            .is_some_and(|texture| textures.relief.of(texture).is_some())
        {
            relief::add_tangents(mesh);
        }
    }
    let lakes = dam::prepare_lakes(&track, &terrain, scenery, textures, urgency.threads());
    let sea_mirror = (!scenery.cartoon_water).then(|| {
        mirror::sky_mirror(
            scenery,
            &sea::mirror_reach(&track),
            mirror::deep(scenery.water),
        )
    });
    let meadows = preview::meadows(&ground, textures);
    structures::draw_signs(scenery, track.wall());
    Prepared {
        basis: Basis {
            index,
            circuit,
            track: Arc::new(track),
            terrain: Arc::new(terrain),
            shore: Arc::new(shore),
            roadside,
        },
        ground,
        shore_image,
        sea_mirror,
        pieces,
        lakes,
        lamps,
        meadows,
    }
}

/// The ground of a circuit split between the Earth and the Moon (see
/// [`realm`]): each realm's own, and the land beyond its border drawn as
/// its own, each on its layers.
fn realm_ground(terrain: &terrain::Terrain, threads: usize) -> Vec<terrain::GroundMesh> {
    use realm::Realm;
    [Realm::Moon, Realm::Earth]
        .into_iter()
        .flat_map(|style| {
            [style, style.other()]
                .into_iter()
                .flat_map(move |side| terrain.meshes_as(threads, Some(style), Some(side)))
        })
        .collect()
}

/// The rest of a circuit whose first part is `basis` (see [`Rest`]).
fn prepare_rest(basis: &Basis, textures: &textures::WorldTextures, urgency: Urgency) -> Rest {
    let scenery = &basis.circuit.scenery;
    let (track, terrain, shore) = (&*basis.track, &*basis.terrain, &*basis.shore);
    let sea_mirror = (!scenery.cartoon_water).then(|| {
        mirror::prepare_mirror(
            track,
            terrain,
            scenery,
            textures,
            &sea::mirror_reach(track),
            &|at| shore.distance_at(at) < -sea::MIRROR_INLAND,
            mirror::deep(scenery.water),
            urgency.threads(),
        )
    });
    let grass = tufts::tuft_pieces(
        track,
        terrain,
        &basis.roadside.shoulders,
        &basis.roadside.edges,
        textures,
    );
    Rest {
        sea_mirror,
        grass,
        trees: props::plan_trees(track, terrain, scenery),
        flowers: flowers::plan_flowers(track, terrain),
    }
}

/// The thread the circuits are prepared on in the background: their own,
/// not Bevy's async compute pool. The render pipelines are compiled there,
/// and behind a circuit being prepared (seconds of work) they would wait:
/// nothing new would be drawn until it was ready, the screenshots included.
fn circuit_pool() -> &'static TaskPool {
    static POOL: std::sync::OnceLock<TaskPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        TaskPoolBuilder::new()
            .num_threads(1)
            .thread_name("Circuits".into())
            .build()
    })
}

/// The thread the circuit chosen in the tracks menu is prepared on when it
/// is needed now: its own, so that it does not wait behind the work in the
/// background.
fn urgent_pool() -> &'static TaskPool {
    static POOL: std::sync::OnceLock<TaskPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        TaskPoolBuilder::new()
            .num_threads(1)
            .thread_name("Circuit chosen".into())
            .build()
    })
}

/// Work on a circuit on another thread, and whether it has begun (or still
/// waits its turn on its thread).
struct Work<T> {
    task: Task<T>,
    begun: Arc<AtomicBool>,
}

impl<T: Send + 'static> Work<T> {
    /// `work` done on another thread: the background one, or, if it is
    /// needed now, the one of its own.
    fn start(urgency: Urgency, work: impl FnOnce(Urgency) -> T + Send + 'static) -> Self {
        let begun = Arc::new(AtomicBool::new(false));
        let flag = begun.clone();
        let pool = match urgency {
            Urgency::Now => urgent_pool(),
            Urgency::Background => circuit_pool(),
        };
        let task = pool.spawn(async move {
            flag.store(true, Ordering::Relaxed);
            work(urgency)
        });
        Self { task, begun }
    }

    fn begun(&self) -> bool {
        self.begun.load(Ordering::Relaxed)
    }

    /// Done now: waited for, or, if it is not to be waited for (on the web
    /// the background work runs on this same thread), done here.
    fn wait(self, otherwise: impl FnOnce() -> T) -> T {
        if WEB {
            otherwise()
        } else {
            block_on(self.task)
        }
    }
}

/// The first part of circuit `index` worked out on another thread.
fn start_preparing(
    circuits: Circuits,
    index: usize,
    textures: &textures::WorldTextures,
    urgency: Urgency,
) -> Work<Prepared> {
    let textures = textures.clone();
    Work::start(urgency, move |urgency| {
        prepare(circuits, index, &textures, urgency)
    })
}

/// The rest of the circuit whose first part is `basis`, worked out on
/// another thread.
fn start_finishing(
    basis: &Basis,
    textures: &textures::WorldTextures,
    urgency: Urgency,
) -> Work<Rest> {
    let (basis, textures) = (basis.clone(), textures.clone());
    Work::start(urgency, move |urgency| {
        prepare_rest(&basis, &textures, urgency)
    })
}

/// The first parts of the circuits being prepared, by index in
/// [`Circuits`].
#[derive(Resource, Default)]
struct Preparing(HashMap<usize, Work<Prepared>>);

/// The circuits shown whose rest is still to come, by index in
/// [`Circuits`]: what it is put in the world with, and the work on it, if
/// it has started.
#[derive(Resource, Default)]
struct Finishing(HashMap<usize, (Shown, Option<Work<Rest>>)>);

/// The quick looks at the start of the circuits being prepared (see
/// [`preview`]), by index in [`Circuits`].
#[derive(Resource, Default)]
struct Previewing(HashMap<usize, Work<Prepared>>);

/// The quick looks at the start of the circuits in the world, by index in
/// [`Circuits`]: each hidden when its circuit's first part comes, and kept,
/// through the races too, for whenever its circuit is not there (see
/// [`unload_unused_circuits`]).
#[derive(Resource, Default)]
struct PreviewRoots(HashMap<usize, Entity>);

/// A quick look at the start of circuit `.0` in the world (see [`preview`]).
#[derive(Component, Debug, PartialEq, Eq)]
pub struct PreviewRoot(pub usize);

/// Whether the circuits are looked at first at their start before they
/// are ready (see [`preview`]): in the tracks menu seen from the road, and
/// not on the web, where the work in the background runs on the main
/// thread.
fn previews_wanted(settings: Option<&Settings>) -> bool {
    !WEB && settings.is_some_and(|settings| settings.track_view.at_the_road())
}

/// The quick look at the start of circuit `index` worked out on another
/// thread.
fn start_previewing(
    circuits: Circuits,
    index: usize,
    textures: &textures::WorldTextures,
    urgency: Urgency,
) -> Work<Prepared> {
    let textures = textures.clone();
    Work::start(urgency, move |urgency| {
        preview::prepare_preview(circuits.at(index), index, &textures, urgency)
    })
}

/// Whether the circuit chosen is still being prepared, and cannot be shown
/// yet: in the tracks menu it is not waited for (the menu would stop for
/// seconds), it comes when ready. Its rest may come later still: going on to
/// a race waits for it.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub struct CircuitLoading(pub bool);

/// How far apart two circuits are in the tracks menu of `count`, going
/// round it either way.
fn menu_distance(a: usize, b: usize, count: usize) -> usize {
    let apart = a.abs_diff(b);
    apart.min(count - apart)
}

/// Whether the circuits are prepared in the background now: in every menu,
/// from the welcome screen on, so that by the time the tracks are chosen
/// most are there; not in a race. On the web, only in the tracks menu, as
/// the work in the background runs on the main thread there and would stop
/// the welcome screen.
fn preparing_in_the_menus(state: Option<Res<State<GameState>>>) -> bool {
    match state.as_deref().map(State::get) {
        Some(GameState::Race) | None => false,
        Some(GameState::Tracks) => true,
        Some(_) => !WEB,
    }
}

/// In the menus (see [`preparing_in_the_menus`]), the circuits not yet built
/// are prepared in the background, one piece of work at a time (and none
/// while the chosen one is worked on now), so that going from one to the
/// next in the tracks menu is instant: seen from the road, a look at the
/// start of each first; the first part of each, those nearest the one
/// chosen first; then the rest of each; and last, seen from the road, a
/// look at the start of those built with none (the first circuit, built at
/// start-up), kept for after a race. Each is put in the world when it is
/// ready ([`spawn_prepared_circuit`]). The menus go on meanwhile.
#[allow(clippy::too_many_arguments)]
fn queue_circuits(
    current: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    settings: Option<Res<Settings>>,
    roots: Query<&WorldRoot>,
    textures: Res<textures::WorldTextures>,
    mut preparing: ResMut<Preparing>,
    mut finishing: ResMut<Finishing>,
    mut previewing: ResMut<Previewing>,
    previews: Res<PreviewRoots>,
) {
    // One at a time, and none while the one chosen is worked on now, so as
    // not to take its cores.
    let busy = !preparing.0.is_empty()
        || !previewing.0.is_empty()
        || finishing.0.values().any(|(_, work)| work.is_some());
    if busy {
        return;
    }
    let nearest = |indices: &mut dyn Iterator<Item = usize>| {
        indices.min_by_key(|&index| (menu_distance(index, current.0, circuits.len()), index))
    };
    let built: HashSet<usize> = roots.iter().map(|root| root.0).collect();
    // A look at the start of each first, seen from the road.
    if previews_wanted(settings.as_deref()) {
        let unseen = nearest(&mut (0..circuits.len()).filter(|index| {
            !built.contains(index)
                && !finishing.0.contains_key(index)
                && !previews.0.contains_key(index)
        }));
        if let Some(index) = unseen {
            let work = start_previewing(*circuits, index, &textures, Urgency::Background);
            previewing.0.insert(index, work);
            return;
        }
    }
    let unbuilt = nearest(&mut (0..circuits.len()).filter(|index| {
        !built.contains(index)
            && !preparing.0.contains_key(index)
            && !finishing.0.contains_key(index)
    }));
    if let Some(index) = unbuilt {
        let work = start_preparing(*circuits, index, &textures, Urgency::Background);
        preparing.0.insert(index, work);
        return;
    }
    let unfinished = nearest(
        &mut finishing
            .0
            .iter()
            .filter(|(_, (_, work))| work.is_none())
            .map(|(&index, _)| index),
    );
    if let Some(index) = unfinished
        && let Some((shown, work)) = finishing.0.get_mut(&index)
    {
        *work = Some(start_finishing(
            &shown.basis,
            &textures,
            Urgency::Background,
        ));
        return;
    }
    if previews_wanted(settings.as_deref()) {
        let unseen =
            nearest(&mut (0..circuits.len()).filter(|index| !previews.0.contains_key(index)));
        if let Some(index) = unseen {
            let work = start_previewing(*circuits, index, &textures, Urgency::Background);
            previewing.0.insert(index, work);
        }
    }
}

/// Puts in the world the first part or the rest of a circuit prepared on
/// another thread (one a frame: making its entities and meshes takes a
/// little while too), the chosen one's first: its first part shown, with
/// what lies beside its roads given to the track, and its rest started at
/// once; the others hidden, their rest left to [`queue_circuits`].
#[allow(clippy::too_many_arguments)]
fn spawn_prepared_circuit(
    mut commands: Commands,
    current: Res<CurrentCircuit>,
    mut preparing: ResMut<Preparing>,
    mut finishing: ResMut<Finishing>,
    mut previewing: ResMut<Previewing>,
    roots: Query<&WorldRoot>,
    mut builder: Builder,
    mut roadsides: ResMut<Roadsides>,
    mut track: ResMut<Track>,
    mut loading: ResMut<CircuitLoading>,
) {
    let chosen_first = |indices: Vec<usize>| {
        let mut indices = indices;
        indices.sort_by_key(|&index| index != current.0);
        indices
    };
    // A look at the start of a circuit: shown if it is the one chosen,
    // unless its first part has come; hidden otherwise, kept for later.
    // Put in before a first part that is ready too (a slow frame can find
    // both ready at once), which then hides it, so that a finished look is
    // never thrown away.
    let ready = chosen_first(previewing.0.keys().copied().collect())
        .into_iter()
        .find_map(|index| {
            let work = previewing.0.get_mut(&index)?;
            block_on(poll_once(&mut work.task)).map(|prepared| (index, prepared))
        });
    if let Some((index, prepared)) = ready {
        previewing.0.remove(&index);
        let built = roots.iter().any(|root| root.0 == index) || finishing.0.contains_key(&index);
        if !builder.previews.0.contains_key(&index) {
            let visibility = if index == current.0 && !built {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
            let stretch = preview::stretch(prepared.basis.track.lap_length());
            build(
                &mut commands,
                prepared,
                &mut builder,
                visibility,
                Some(stretch),
            );
        }
        return;
    }
    let ready = chosen_first(preparing.0.keys().copied().collect())
        .into_iter()
        .find_map(|index| {
            let work = preparing.0.get_mut(&index)?;
            block_on(poll_once(&mut work.task)).map(|prepared| (index, prepared))
        });
    if let Some((index, prepared)) = ready {
        preparing.0.remove(&index);
        let chosen = index == current.0;
        let visibility = if chosen {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        previewing.0.remove(&index);
        let shown = build(&mut commands, prepared, &mut builder, visibility, None);
        if chosen {
            shown.basis.roadside.apply(&mut track);
            loading.set_if_neq(CircuitLoading(false));
        }
        roadsides.0.insert(index, shown.basis.roadside.clone());
        let work = chosen.then(|| start_finishing(&shown.basis, &builder.textures, Urgency::Now));
        finishing.0.insert(index, (shown, work));
        return;
    }
    let ready = chosen_first(finishing.0.keys().copied().collect())
        .into_iter()
        .find_map(|index| {
            let work = finishing.0.get_mut(&index)?.1.as_mut()?;
            block_on(poll_once(&mut work.task)).map(|rest| (index, rest))
        });
    if let Some((index, rest)) = ready
        && let Some((shown, _)) = finishing.0.remove(&index)
    {
        finish(&mut commands, shown, rest, &mut builder);
    }
}

/// Puts the whole of circuit `index` in the world now, as far as it is not
/// yet, waiting for the work on it under way, or doing it here.
#[allow(clippy::too_many_arguments)]
fn complete_now(
    index: usize,
    built: bool,
    commands: &mut Commands,
    builder: &mut Builder,
    preparing: &mut Preparing,
    finishing: &mut Finishing,
    roadsides: &mut Roadsides,
) {
    if !built && !finishing.0.contains_key(&index) {
        let (textures, circuits) = (builder.textures.clone(), *builder.circuits);
        let prepared = match preparing.0.remove(&index) {
            Some(work) => work.wait(|| prepare(circuits, index, &textures, Urgency::Now)),
            None => prepare(circuits, index, &textures, Urgency::Now),
        };
        let shown = build(commands, prepared, builder, Visibility::Inherited, None);
        roadsides.0.insert(index, shown.basis.roadside.clone());
        finishing.0.insert(index, (shown, None));
    }
    if let Some((shown, work)) = finishing.0.remove(&index) {
        let textures = builder.textures.clone();
        let now = || prepare_rest(&shown.basis, &textures, Urgency::Now);
        let rest = match work {
            Some(work) => work.wait(now),
            None => now(),
        };
        finish(commands, shown, rest, builder);
    }
}

/// Going on to a race: the circuit chosen, whole, its rest waited for if it
/// has not come yet.
#[allow(clippy::too_many_arguments)]
fn complete_the_circuit(
    mut commands: Commands,
    current: Res<CurrentCircuit>,
    roots: Query<&WorldRoot>,
    mut builder: Builder,
    mut preparing: ResMut<Preparing>,
    mut finishing: ResMut<Finishing>,
    mut roadsides: ResMut<Roadsides>,
    mut track: ResMut<Track>,
    mut loading: ResMut<CircuitLoading>,
) {
    let built = roots.iter().any(|root| root.0 == current.0);
    if built && !finishing.0.contains_key(&current.0) {
        return;
    }
    complete_now(
        current.0,
        built,
        &mut commands,
        &mut builder,
        &mut preparing,
        &mut finishing,
        &mut roadsides,
    );
    if let Some(roadside) = roadsides.0.get(&current.0) {
        roadside.apply(&mut track);
    }
    loading.set_if_neq(CircuitLoading(false));
}

/// Going on to a race, the circuits prepared in the menus but the one
/// chosen go, to free their memory, and the work on them is let go. Their
/// looks at the start stay, hidden, if the tracks menu is seen from the
/// road: some 15 to 40 MB each, they show any circuit at once in the menus
/// after the race, while it is prepared again.
#[allow(clippy::too_many_arguments)]
fn unload_unused_circuits(
    mut commands: Commands,
    current: Res<CurrentCircuit>,
    settings: Option<Res<Settings>>,
    roots: Query<(Entity, &WorldRoot)>,
    mut preparing: ResMut<Preparing>,
    mut finishing: ResMut<Finishing>,
    mut previewing: ResMut<Previewing>,
    mut previews: ResMut<PreviewRoots>,
) {
    for (entity, root) in &roots {
        if root.0 != current.0 {
            commands.entity(entity).despawn();
        }
    }
    // The one chosen, if it is still on its way, comes all the same, its
    // look at the start shown meanwhile.
    preparing.0.retain(|&index, _| index == current.0);
    finishing.0.retain(|&index, _| index == current.0);
    previewing.0.retain(|&index, _| index == current.0);
    let looks_wanted = previews_wanted(settings.as_deref());
    previews.0.retain(|&index, &mut entity| {
        let kept = looks_wanted || index == current.0;
        if !kept {
            commands.entity(entity).despawn();
        }
        kept
    });
}

/// The circuits read again ([`CircuitsReloaded`], their files changed on
/// the disk) are forgotten: what was built of them goes, with their looks
/// at the start, what lies beside their roads and the work on them; and
/// the one chosen, if it is among them, is built again
/// ([`rebuild_world`]). The others are prepared again in the menus as
/// ever.
#[allow(clippy::too_many_arguments)]
fn forget_reloaded_circuits(
    mut commands: Commands,
    mut reloaded: MessageReader<CircuitsReloaded>,
    mut current: ResMut<CurrentCircuit>,
    roots: Query<(Entity, &WorldRoot)>,
    mut roadsides: ResMut<Roadsides>,
    mut preparing: ResMut<Preparing>,
    mut finishing: ResMut<Finishing>,
    mut previewing: ResMut<Previewing>,
    mut previews: ResMut<PreviewRoots>,
) {
    for CircuitsReloaded(changed) in reloaded.read() {
        for (entity, root) in &roots {
            if changed.contains(&root.0) {
                commands.entity(entity).despawn();
            }
        }
        for index in changed {
            roadsides.0.remove(index);
            preparing.0.remove(index);
            finishing.0.remove(index);
            previewing.0.remove(index);
            if let Some(entity) = previews.0.remove(index) {
                commands.entity(entity).despawn();
            }
        }
        if changed.contains(&current.0) {
            current.set_changed();
        }
    }
}

/// Al cambiar de pista: actualiza el trazado y conmuta la visibilidad entre
/// las pistas cargadas en caché, o, si no estuviera, la construye. In the
/// tracks menu it is not waited for: it is prepared on a thread of its own
/// if it has not begun yet, and comes when ready ([`CircuitLoading`],
/// [`spawn_prepared_circuit`]), its rest after it (started now, if it has
/// not begun); elsewhere (a championship going on to the next track) the
/// whole of it is waited for.
#[allow(clippy::too_many_arguments)]
fn rebuild_world(
    mut commands: Commands,
    current: Res<CurrentCircuit>,
    circuits: Res<Circuits>,
    state: Option<Res<State<GameState>>>,
    settings: Option<Res<Settings>>,
    mut roots: Query<(Entity, &WorldRoot, &mut Visibility)>,
    mut looks: Query<(&PreviewRoot, &mut Visibility), Without<WorldRoot>>,
    mut track: ResMut<Track>,
    mut builder: Builder,
    mut roadsides: ResMut<Roadsides>,
    mut preparing: ResMut<Preparing>,
    mut finishing: ResMut<Finishing>,
    mut previewing: ResMut<Previewing>,
    mut loading: ResMut<CircuitLoading>,
) {
    if !current.is_changed() || current.is_added() {
        return;
    }
    let mut found = false;
    for (_entity, root, mut visibility) in &mut roots {
        if root.0 == current.0 {
            *visibility = Visibility::Inherited;
            found = true;
        } else {
            *visibility = Visibility::Hidden;
        }
    }
    // Its look at the start, till it comes (not seen from above, where a
    // look kept from before the option changed would show its edges).
    let looks_wanted = previews_wanted(settings.as_deref());
    for (look, mut visibility) in &mut looks {
        *visibility = if look.0 == current.0 && !found && looks_wanted {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    *track = Track::new(&circuits.get(*current).layout);
    let in_menu = state.is_some_and(|state| *state.get() == GameState::Tracks);
    if in_menu && !WEB {
        if !found {
            // A look at its start first, if there is none, then the whole
            // of it, on the same thread, every core.
            if looks_wanted
                && !builder.previews.0.contains_key(&current.0)
                && previewing
                    .0
                    .get(&current.0)
                    .is_none_or(|work| !work.begun())
            {
                let work = start_previewing(
                    *builder.circuits,
                    current.0,
                    &builder.textures,
                    Urgency::Now,
                );
                previewing.0.insert(current.0, work);
            }
            if preparing.0.get(&current.0).is_none_or(|work| !work.begun()) {
                let work = start_preparing(
                    *builder.circuits,
                    current.0,
                    &builder.textures,
                    Urgency::Now,
                );
                preparing.0.insert(current.0, work);
            }
            loading.set_if_neq(CircuitLoading(true));
            return;
        }
        // Shown: its rest, if it has not begun, now.
        if let Some((shown, work)) = finishing.0.get_mut(&current.0)
            && work.as_ref().is_none_or(|work| !work.begun())
        {
            *work = Some(start_finishing(
                &shown.basis,
                &builder.textures,
                Urgency::Now,
            ));
        }
    } else {
        complete_now(
            current.0,
            found,
            &mut commands,
            &mut builder,
            &mut preparing,
            &mut finishing,
            &mut roadsides,
        );
    }
    loading.set_if_neq(CircuitLoading(false));
    if let Some(roadside) = roadsides.0.get(&current.0) {
        roadside.apply(&mut track);
    }
}

fn build_world(
    mut commands: Commands,
    current: Res<CurrentCircuit>,
    mut track: ResMut<Track>,
    mut builder: Builder,
    mut roadsides: ResMut<Roadsides>,
) {
    let prepared = prepare(
        *builder.circuits,
        current.0,
        &builder.textures,
        Urgency::Now,
    );
    let shown = build(
        &mut commands,
        prepared,
        &mut builder,
        Visibility::Inherited,
        None,
    );
    shown.basis.roadside.apply(&mut track);
    roadsides.0.insert(current.0, shown.basis.roadside.clone());
    let rest = prepare_rest(&shown.basis, &builder.textures, Urgency::Now);
    finish(&mut commands, shown, rest, &mut builder);
}

/// Lo que hace falta para construir el mundo.
#[derive(bevy::ecs::system::SystemParam)]
struct Builder<'w> {
    circuits: Res<'w, Circuits>,
    textures: Res<'w, textures::WorldTextures>,
    packs: Res<'w, prop_pack::PropPacks>,
    sea: Res<'w, Sea>,
    sea_materials: ResMut<'w, Assets<sea::SeaMaterial>>,
    mirror_materials: ResMut<'w, Assets<mirror::MirrorMaterial>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    materials: ResMut<'w, Assets<StandardMaterial>>,
    images: ResMut<'w, Assets<Image>>,
    previews: ResMut<'w, PreviewRoots>,
    relief: ResMut<'w, relief::SharedRelief>,
}

/// A circuit whose first part is in the world: where it is, the sea's
/// mirroring material (to be given what stands round it with the rest),
/// and what its rest is worked out and put in the world from.
struct Shown {
    root: Entity,
    sea_mirror: Option<Handle<mirror::MirrorMaterial>>,
    basis: Basis,
}

/// Puts the first part of a prepared circuit in the world (see
/// [`Prepared`]), its look at the start hidden; or, if it is a look at a
/// `stretch` of it (see [`preview`]), that, with the arches on the stretch.
fn build(
    commands: &mut Commands,
    prepared: Prepared,
    builder: &mut Builder,
    visibility: Visibility,
    stretch: Option<scenery::Span>,
) -> Shown {
    let Prepared {
        basis,
        ground,
        shore_image,
        sea_mirror,
        pieces,
        lakes,
        lamps,
        meadows,
    } = prepared;
    let meadows: Vec<_> = meadows
        .into_iter()
        .map(|(kind, meadow)| (kind, builder.images.add(meadow)))
        .collect();
    // The lawn in flower with the lawn's relief.
    for (_, meadow) in &meadows {
        builder.relief.share(meadow, &builder.textures.grass);
    }
    let shore_image = builder.images.add(shore_image);
    let scenery = &basis.circuit.scenery;
    let cartoon_sea = builder
        .sea_materials
        .add(basis.shore.material(shore_image, scenery.water));
    let lakes: Vec<_> = lakes
        .into_iter()
        .map(|lake| {
            let material = lake.mirror.into_material(&mut builder.images);
            (
                builder.meshes.add(lake.mesh),
                builder.mirror_materials.add(material),
            )
        })
        .collect();
    let sea_mirror = sea_mirror.map(|mirror| {
        let material = mirror.into_material(&mut builder.images);
        builder.mirror_materials.add(material)
    });
    let mut assets = Assets3d {
        textures: &builder.textures,
        packs: &builder.packs,
        flowers: None,
        meshes: &mut builder.meshes,
        materials: &mut builder.materials,
        images: &mut builder.images,
        mirrored: Vec::new(),
        mirrored_leaves: Vec::new(),
    };
    let (track, terrain) = (&*basis.track, &*basis.terrain);
    let split = realm::splits(scenery);
    let mut root = commands.spawn((lamps::Lamps(lamps), Transform::default(), visibility));
    match stretch {
        None => {
            root.insert(WorldRoot(basis.index));
        }
        Some(_) => {
            root.insert(PreviewRoot(basis.index));
        }
    }
    let root = root
        .with_children(|world| {
            terrain::spawn_terrain(
                world,
                terrain,
                ground,
                &meadows,
                assets.textures,
                assets.meshes,
                assets.materials,
            );
            for (mesh, material) in lakes {
                world.spawn((Mesh3d(mesh), MeshMaterial3d(material), NotShadowCaster));
            }
            // What the themes put round it all (a room's walls and
            // ceiling, indoors).
            for (_, theme) in scenery.themes.iter() {
                theme.spawn_around(world, &mut assets);
            }
            for (mesh, material) in pieces {
                // What is see-through or glows (light, dust) casts no
                // shadow, nor what lamps do not light (the daylight out of
                // a shop's window, its own sun in its colours).
                let shadowless = matches!(material.alpha_mode, AlphaMode::Add | AlphaMode::Blend)
                    || material.unlit;
                let material = assets.materials.add(material);
                // On a split circuit, the part of each in each realm.
                let parts: Vec<_> = if split {
                    [realm::Realm::Earth, realm::Realm::Moon]
                        .into_iter()
                        .filter_map(|realm| {
                            realm::part_in(&mesh, realm).map(|part| (part, Some(realm.layers())))
                        })
                        .collect()
                } else {
                    vec![(mesh, None)]
                };
                for (mesh, layers) in parts {
                    let mut piece = world.spawn((
                        Mesh3d(assets.meshes.add(mesh)),
                        MeshMaterial3d(material.clone()),
                    ));
                    if shadowless {
                        piece.insert(NotShadowCaster);
                    }
                    if let Some(layers) = layers {
                        piece.insert(layers);
                    }
                }
            }
            let mut put_up = |world: &mut ChildSpawnerCommands| {
                road::spawn_gantries(world, track, scenery, stretch, &mut assets);
                if stretch.is_none() {
                    structures::spawn_signs(world, track, terrain, scenery, &mut assets);
                }
            };
            if split {
                world
                    .spawn((
                        realm::RealmScenery,
                        Transform::default(),
                        Visibility::default(),
                    ))
                    .with_children(|scenery| put_up(scenery));
            } else {
                put_up(world);
            }
            // The sea: the cartoon one at first, mirroring, if it should,
            // before it is first drawn (see `sea::dress_the_sea`).
            let sea_water = sea::SeaWater {
                cartoon: cartoon_sea,
                mirror: sea_mirror.clone(),
            };
            let mut sea = world.spawn((
                Mesh3d(builder.sea.0.clone()),
                MeshMaterial3d(sea_water.cartoon.clone()),
                sea_water,
                Transform::from_xyz(0.0, terrain::SEA_LEVEL, 0.0),
            ));
            // The Moon has no seas: on a split circuit, the sea is the
            // Earth's.
            if split {
                sea.insert(realm::Realm::Earth.layers());
            }
        })
        .id();
    match stretch {
        None => {
            if let Some(&look) = builder.previews.0.get(&basis.index) {
                commands.entity(look).insert(Visibility::Hidden);
            }
        }
        Some(_) => {
            builder.previews.0.insert(basis.index, root);
        }
    }
    Shown {
        root,
        sea_mirror,
        basis,
    }
}

/// Puts the rest of a circuit in the world, its first part there already
/// (see [`Rest`]): its grass, its houses, trees, props and flowers, and
/// then the sea's map of what stands round it, with them standing.
fn finish(commands: &mut Commands, shown: Shown, rest: Rest, builder: &mut Builder) {
    let Shown {
        root,
        sea_mirror,
        basis,
    } = shown;
    let Ok(mut world_root) = commands.get_entity(root) else {
        return;
    };
    let scenery = &basis.circuit.scenery;
    let split = realm::splits(scenery);
    let mut assets = Assets3d {
        textures: &builder.textures,
        packs: &builder.packs,
        flowers: None,
        meshes: &mut builder.meshes,
        materials: &mut builder.materials,
        images: &mut builder.images,
        mirrored: Vec::new(),
        mirrored_leaves: Vec::new(),
    };
    world_root.with_children(|world| {
        for piece in rest.grass {
            let range = piece.range();
            // On a split circuit, the grass is drawn in the realm it grows
            // in: the Earth's, as the Moon has none.
            let layers = split.then(|| {
                piece
                    .mesh
                    .compute_aabb()
                    .map_or(realm::Realm::Earth, |aabb| {
                        realm::Realm::at(Vec2::new(aabb.center.x, aabb.center.z))
                    })
                    .layers()
            });
            let mut grass = world.spawn((
                Mesh3d(assets.meshes.add(piece.mesh)),
                MeshMaterial3d(assets.materials.add(piece.material)),
                range,
                NotShadowCaster,
            ));
            if let Some(layers) = layers {
                grass.insert(layers);
            }
        }
        let put_up = |world: &mut ChildSpawnerCommands| {
            props::spawn_props(
                world,
                &basis.track,
                &basis.terrain,
                scenery,
                &rest.trees,
                rest.flowers,
                &mut assets,
            );
        };
        // On a split circuit, each in the realm it stands in.
        if split {
            world
                .spawn((
                    realm::RealmScenery,
                    Transform::default(),
                    Visibility::default(),
                ))
                .with_children(put_up);
        } else {
            put_up(world);
        }
    });
    // The sea mirroring what stands round it too, now that it stands.
    if let (Some(handle), Some(mut mirror)) = (sea_mirror, rest.sea_mirror) {
        mirror.stand_up(&assets.mirrored, &assets.mirrored_leaves);
        let material = mirror.into_material(assets.images);
        if let Some(mut sea) = builder.mirror_materials.get_mut(&handle) {
            *sea = material;
        }
    }
}

/// Dónde guardar las mallas, los materiales y las imágenes del mundo.
pub struct Assets3d<'a> {
    pub textures: &'a textures::WorldTextures,
    /// The packs of props made with Meshy the themes place (see
    /// [`prop_pack`]).
    pub packs: &'a prop_pack::PropPacks,
    pub meshes: &'a mut Assets<Mesh>,
    pub materials: &'a mut Assets<StandardMaterial>,
    pub images: &'a mut Assets<Image>,
    /// Points over the surfaces of what is built round the water, and over
    /// the crowns of the trees in colour, as they are put in the world, for
    /// the sea to mirror (see [`mirror::PreparedMirror::stand_up`]).
    pub mirrored: Vec<mirror::MirrorPoint>,
    pub mirrored_leaves: Vec<mirror::MirrorPoint>,
    /// The circuit's fields of flowers, as they were prepared, while they
    /// are still to be put in the world: a theme puts them among its own,
    /// clear of what it put (see [`flowers::spawn_flowers`]), or the world
    /// does after the themes.
    pub flowers: Option<flowers::FlowerPlan>,
}

/// El sol, en el mismo sitio que en la foto del cielo, y la luz ambiente.
fn spawn_sun(mut commands: Commands) {
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.75, 0.82, 1.0),
        brightness: 700.0,
        ..default()
    });
    let sun = sky::Palette::of(scenery::SkyStyle::Day).sun_direction();
    commands.spawn((
        sky::Sunlight,
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::default().looking_to(-sun, Vec3::Y),
        // Shadows are only needed near the camera, which gives them more
        // resolution (with the screen split, nearer still: see [`detail`]).
        detail::Detail::default().cascades(),
    ));
}

#[cfg(any(test, feature = "testing"))]
pub mod tests {
    use super::*;

    pub fn circuits_are_prepared_in_the_menus_and_let_go_for_a_race_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_state::<GameState>()
            .insert_resource(*circuits)
            .add_plugins(WorldPlugin);

        app.update();

        // Al inicio, solo la pista inicial (0) está cargada y visible; on
        // the welcome screen, the next one is prepared already.
        let mut roots_query = app.world_mut().query::<(&WorldRoot, &Visibility)>();
        let roots: Vec<_> = roots_query.iter(app.world()).collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].0.0, 0);
        assert_eq!(*roots[0].1, Visibility::Inherited);
        assert!(app.world().resource::<Preparing>().0.contains_key(&1));

        // Entramos al menú de elección de pista: the menu does not wait for
        // the other circuits, which are prepared in the background, one at
        // a time, the nearest in the menu first...
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Tracks);
        app.update();
        assert_eq!(roots_query.iter(app.world()).count(), 1);
        let preparing: Vec<usize> = app
            .world()
            .resource::<Preparing>()
            .0
            .keys()
            .copied()
            .collect();
        assert_eq!(preparing, [1]);

        // ...and arrive, hidden, as they are ready.
        let started = std::time::Instant::now();
        while roots_query.iter(app.world()).count() < circuits.len() {
            assert!(
                started.elapsed().as_secs() < 120,
                "the circuits never arrive"
            );
            app.update();
        }
        assert!(app.world().resource::<Preparing>().0.is_empty());
        let roots: Vec<_> = roots_query.iter(app.world()).collect();
        for (root, vis) in &roots {
            if root.0 == 0 {
                assert_eq!(*vis, Visibility::Inherited);
            } else {
                assert_eq!(*vis, Visibility::Hidden);
            }
        }

        // Cambiamos de pista a la 1: debe conmutar visibilidad instantáneamente.
        app.world_mut().resource_mut::<CurrentCircuit>().0 = 1;
        app.update();

        let roots: Vec<_> = roots_query.iter(app.world()).collect();
        assert_eq!(roots.len(), circuits.len());
        for (root, vis) in &roots {
            if root.0 == 1 {
                assert_eq!(*vis, Visibility::Inherited);
            } else {
                assert_eq!(*vis, Visibility::Hidden);
            }
        }

        // On to choosing a bike, they all stay, for a way back to the
        // tracks; on to the race, only the one chosen (1) is left.
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Select);
        app.update();
        assert_eq!(roots_query.iter(app.world()).count(), circuits.len());
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Race);
        app.update();
        app.update();
        let roots: Vec<_> = roots_query.iter(app.world()).collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].0.0, 1);
        assert_eq!(*roots[0].1, Visibility::Inherited);
    }

    pub fn a_circuit_chosen_before_it_is_ready_comes_without_stopping_the_menu_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_state::<GameState>()
            .insert_resource(*circuits)
            .add_plugins(WorldPlugin);
        app.update();
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Tracks);
        app.update();

        // Straight to the last circuit, behind all the others in the
        // background: the menu goes on at once, the circuit on its way.
        let last = circuits.len() - 1;
        app.world_mut().resource_mut::<CurrentCircuit>().0 = last;
        app.update();
        // The menu goes on at once: the circuit is on its way, not waited
        // for. (A wall-clock bound here only measured how loaded the machine
        // was when the whole suite ran in parallel.)
        assert_eq!(
            *app.world().resource::<CircuitLoading>(),
            CircuitLoading(true)
        );
        assert!(app.world().resource::<Preparing>().0.contains_key(&last));
        let started = std::time::Instant::now();
        // It comes, shown alone, with what lies beside its roads.
        let mut roots = app.world_mut().query::<(&WorldRoot, &Visibility)>();
        while app.world().resource::<CircuitLoading>().0 {
            assert!(started.elapsed().as_secs() < 120, "it never comes");
            app.update();
        }
        app.update();
        let shown: Vec<usize> = roots
            .iter(app.world())
            .filter(|(_, vis)| **vis == Visibility::Inherited)
            .map(|(root, _)| root.0)
            .collect();
        assert_eq!(shown, [last]);
        assert!(app.world().resource::<Roadsides>().0.contains_key(&last));
        assert!(!app.world().resource::<Preparing>().0.contains_key(&last));
    }

    pub fn a_circuit_is_shown_before_its_rest_comes_and_raced_on_whole_on(
        circuits: &'static crate::circuits::Circuits,
        name: &str,
    ) {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_state::<GameState>()
            .insert_resource(*circuits)
            .add_plugins(WorldPlugin);
        app.update();
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Tracks);
        app.update();
        // The named circuit, chosen at once: shown first, its rest (its
        // houses, trees, props, grass and flowers) still to come.
        let last = circuits
            .iter()
            .position(|circuit| circuit.name == name)
            .unwrap_or_else(|| panic!("{name} is a circuit"));
        app.world_mut().resource_mut::<CurrentCircuit>().0 = last;
        app.update();
        let started = std::time::Instant::now();
        while app.world().resource::<CircuitLoading>().0 {
            assert!(started.elapsed().as_secs() < 120, "it never comes");
            app.update();
        }
        app.update();
        let mut roots = app.world_mut().query::<(&WorldRoot, &Children)>();
        let mut pieces = |app: &mut App| {
            roots
                .iter(app.world())
                .find(|(root, _)| root.0 == last)
                .map_or(0, |(_, children)| children.len())
        };
        let shown = pieces(&mut app);
        assert!(shown > 0);
        assert!(app.world().resource::<Finishing>().0.contains_key(&last));
        // Going on to the race, it is there whole, its rest waited for.
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Race);
        app.update();
        app.update();
        assert!(app.world().resource::<Finishing>().0.is_empty());
        assert!(
            pieces(&mut app) > shown + 100,
            "{shown} {}",
            pieces(&mut app)
        );
    }

    #[test]
    fn a_circuit_read_again_is_built_again() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_state::<GameState>();
        crate::testing::demo_content(&mut app).add_plugins(WorldPlugin);
        app.update();
        let mut roots = app.world_mut().query::<(Entity, &WorldRoot)>();
        let built: Vec<(Entity, usize)> = roots
            .iter(app.world())
            .map(|(entity, root)| (entity, root.0))
            .collect();
        assert_eq!(built.len(), 1);
        assert_eq!(built[0].1, 0);

        // The demo circuit's file edited (its name), and read again.
        let text = crate::testing::demo_file("circuits/bahia.ron");
        let files = [(
            "circuits/bahia.ron".to_string(),
            &*String::leak(text.replacen("name: \"Bahía GP\"", "name: \"Bahía GP 2\"", 1)),
        )];
        let edited = Circuits::parse(&files).unwrap();
        app.insert_resource(edited)
            .world_mut()
            .write_message(CircuitsReloaded(vec![0]));
        app.update();
        app.update();
        let rebuilt: Vec<(Entity, usize)> = roots
            .iter(app.world())
            .map(|(entity, root)| (entity, root.0))
            .collect();
        assert_eq!(rebuilt.len(), 1, "built again, once");
        assert_eq!(rebuilt[0].1, 0);
        assert_ne!(rebuilt[0].0, built[0].0, "a new one");
        assert!(app.world().resource::<Roadsides>().0.contains_key(&0));
        assert_eq!(app.world().resource::<Circuits>().at(0).name, "Bahía GP 2");

        // One that was not built is forgotten with nothing to do.
        app.world_mut().write_message(CircuitsReloaded(vec![3]));
        app.update();
        let kept: Vec<Entity> = roots.iter(app.world()).map(|(entity, _)| entity).collect();
        assert_eq!(kept, [rebuilt[0].0], "the one chosen stays");
    }

    /// A world with the tracks menu seen as `view`.
    fn menu_app(
        circuits: &'static crate::circuits::Circuits,
        view: crate::settings::TrackView,
    ) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_state::<GameState>()
            .insert_resource(Settings {
                track_view: view,
                ..default()
            })
            .insert_resource(*circuits)
            .add_plugins(WorldPlugin);
        app.update();
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Tracks);
        app.update();
        app
    }

    pub fn seen_from_the_road_a_circuit_shows_its_start_first_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut app = menu_app(circuits, crate::settings::TrackView::Trackside);
        // The first work in the background: a look at the start of the
        // nearest circuit.
        let keys = |app: &App| -> Vec<usize> {
            app.world()
                .resource::<Previewing>()
                .0
                .keys()
                .copied()
                .collect()
        };
        assert_eq!(keys(&app), [1]);
        assert!(app.world().resource::<Preparing>().0.is_empty());
        // The last one chosen at once: its start shown while it is on its
        // way, and then the circuit in its place.
        let last = circuits.len() - 1;
        app.world_mut().resource_mut::<CurrentCircuit>().0 = last;
        app.update();
        let mut looks = app.world_mut().query::<(&PreviewRoot, &Visibility)>();
        let started = std::time::Instant::now();
        loop {
            assert!(started.elapsed().as_secs() < 120, "no look at its start");
            let shown: Vec<_> = looks
                .iter(app.world())
                .filter(|(look, _)| look.0 == last)
                .map(|(_, visibility)| *visibility)
                .collect();
            if !shown.is_empty() {
                assert_eq!(shown, [Visibility::Inherited]);
                break;
            }
            app.update();
        }
        assert!(app.world().resource::<CircuitLoading>().0);
        while app.world().resource::<CircuitLoading>().0 {
            assert!(started.elapsed().as_secs() < 240, "it never comes");
            app.update();
        }
        app.update();
        // Its look stays, hidden, for when the circuit has gone.
        let kept: Vec<_> = looks
            .iter(app.world())
            .filter(|(look, _)| look.0 == last)
            .map(|(_, visibility)| *visibility)
            .collect();
        assert_eq!(kept, [Visibility::Hidden]);
        let mut roots = app.world_mut().query::<(&WorldRoot, &Visibility)>();
        assert!(
            roots
                .iter(app.world())
                .any(|(root, visibility)| root.0 == last && *visibility == Visibility::Inherited)
        );
    }

    /// A world on the welcome screen, the tracks menu seen as `view`, and
    /// every circuit but the first (built at start-up) looked at by its
    /// start already, and built too if `built`: stand-ins, empty, so as not
    /// to wait for them all.
    pub fn app_with_looks(
        circuits: &'static crate::circuits::Circuits,
        view: crate::settings::TrackView,
        built: bool,
    ) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::state::app::StatesPlugin)
            .add_plugins(bevy::asset::AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<Image>()
            .init_state::<GameState>()
            .insert_resource(Settings {
                track_view: view,
                ..default()
            })
            .insert_resource(*circuits)
            .add_plugins(WorldPlugin);
        for index in 1..circuits.len() {
            if built {
                app.world_mut()
                    .spawn((WorldRoot(index), Visibility::Hidden));
            }
            let look = app
                .world_mut()
                .spawn((PreviewRoot(index), Visibility::Hidden))
                .id();
            app.world_mut()
                .resource_mut::<PreviewRoots>()
                .0
                .insert(index, look);
        }
        app.update();
        app
    }

    /// The looks at the start in the world, by circuit, with how they are
    /// seen.
    fn looks(app: &mut App) -> Vec<(usize, Visibility)> {
        let mut looks = app.world_mut().query::<(&PreviewRoot, &Visibility)>();
        let mut seen: Vec<_> = looks
            .iter(app.world())
            .map(|(look, visibility)| (look.0, *visibility))
            .collect();
        seen.sort_by_key(|&(index, _)| index);
        seen
    }

    pub fn the_looks_at_the_starts_stay_through_a_race_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut app = app_with_looks(circuits, crate::settings::TrackView::Trackside, true);
        // Every circuit built, and every one looked at but the first: a
        // look at its start comes last, and is kept hidden, as it is
        // there whole.
        let previewing: Vec<usize> = app
            .world()
            .resource::<Previewing>()
            .0
            .keys()
            .copied()
            .collect();
        assert_eq!(previewing, [0]);
        let started = std::time::Instant::now();
        while !app.world().resource::<PreviewRoots>().0.contains_key(&0) {
            assert!(started.elapsed().as_secs() < 120, "no look at its start");
            app.update();
        }
        app.update();
        let hidden: Vec<_> = (0..circuits.len())
            .map(|index| (index, Visibility::Hidden))
            .collect();
        assert_eq!(looks(&mut app), hidden);

        // On to a race on the first: the other circuits go, the looks
        // stay, hidden.
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Race);
        app.update();
        app.update();
        let mut roots = app.world_mut().query::<&WorldRoot>();
        let built: Vec<usize> = roots.iter(app.world()).map(|root| root.0).collect();
        assert_eq!(built, [0]);
        assert_eq!(looks(&mut app), hidden);

        // Back in the tracks menu, another circuit is seen by its start at
        // once, while it is prepared again.
        for state in [GameState::Welcome, GameState::Tracks] {
            app.world_mut()
                .resource_mut::<NextState<GameState>>()
                .set(state);
            app.update();
        }
        app.world_mut().resource_mut::<CurrentCircuit>().0 = 2;
        app.update();
        assert!(app.world().resource::<CircuitLoading>().0);
        let seen: Vec<usize> = looks(&mut app)
            .into_iter()
            .filter(|&(_, visibility)| visibility == Visibility::Inherited)
            .map(|(index, _)| index)
            .collect();
        assert_eq!(seen, [2]);
    }

    pub fn seen_from_above_the_looks_at_the_starts_go_with_a_race_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        // Looks from when the menu was seen from the road: none shown, and
        // all gone with a race.
        let mut app = app_with_looks(circuits, crate::settings::TrackView::Aerial, false);
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Tracks);
        app.update();
        app.world_mut().resource_mut::<CurrentCircuit>().0 = 2;
        app.update();
        assert!(
            looks(&mut app)
                .iter()
                .all(|&(_, visibility)| visibility == Visibility::Hidden)
        );
        app.world_mut()
            .resource_mut::<NextState<GameState>>()
            .set(GameState::Race);
        app.update();
        app.update();
        // The one raced on keeps its own, as before, hidden as it comes
        // whole.
        assert_eq!(looks(&mut app), [(2, Visibility::Hidden)]);
    }

    pub fn seen_from_above_no_circuit_is_looked_at_by_its_start_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        let mut app = menu_app(circuits, crate::settings::TrackView::Aerial);
        assert!(app.world().resource::<Previewing>().0.is_empty());
        let preparing: Vec<usize> = app
            .world()
            .resource::<Preparing>()
            .0
            .keys()
            .copied()
            .collect();
        assert_eq!(preparing, [1]);
        app.world_mut().resource_mut::<CurrentCircuit>().0 = circuits.len() - 1;
        app.update();
        assert!(app.world().resource::<Previewing>().0.is_empty());
    }

    pub fn the_circuits_nearest_in_the_menu_come_first_on(
        circuits: &'static crate::circuits::Circuits,
    ) {
        assert_eq!(menu_distance(0, 1, circuits.len()), 1);
        assert_eq!(menu_distance(0, circuits.len() - 1, circuits.len()), 1);
        assert_eq!(menu_distance(1, 3, circuits.len()), 2);
        assert_eq!(menu_distance(3, 1, circuits.len()), 2);
    }
}
