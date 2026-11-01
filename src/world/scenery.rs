//! Cómo es el escenario de cada pista: el terreno (costa, colinas, peñascos y
//! paredes de roca), lo que bordea la carretera (muros, túneles, puentes,
//! arcos y carteles) y el decorado (árboles, casas, hoteles y palmeras).
//! Los datos de cada pista están en [`crate::circuits`].

use bevy::prelude::*;
use serde::Deserialize;

pub use super::mountains::Mountains;
pub use super::themes::Themes;
pub use super::waters::Waters;

use crate::geometry::smoothstep;

/// Un tramo de la vuelta, en metros desde la meta. Puede pasar por la meta:
/// de 2400 a 300, por ejemplo.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(from = "crate::circuits::data::SpanFile")]
pub struct Span {
    pub from: f32,
    pub to: f32,
}

pub const fn span(from: f32, to: f32) -> Span {
    Span { from, to }
}

/// Toda la vuelta.
pub const LAP: Span = span(0.0, f32::MAX);

impl Span {
    /// Cuánto se está dentro del tramo en el punto `along` de una vuelta de
    /// `lap` metros: 1 dentro, 0 fuera, y de uno a otro en los primeros y
    /// últimos `fade` metros.
    pub fn weight(&self, along: f32, lap: f32, fade: f32) -> f32 {
        if self.to - self.from >= lap {
            return 1.0;
        }
        let length = (self.to - self.from).rem_euclid(lap);
        let into = (along - self.from).rem_euclid(lap);
        if into > length {
            return 0.0;
        }
        if fade <= 0.0 {
            return 1.0;
        }
        smoothstep(0.0, fade, into) * smoothstep(0.0, fade, length - into)
    }

    /// Whether it and `other` share any of a lap `lap` metres long.
    pub fn overlaps(&self, other: &Span, lap: f32) -> bool {
        let middle = |span: &Span| span.from + span.length(lap) / 2.0;
        self.contains(other.from, lap)
            || other.contains(self.from, lap)
            || self.contains(middle(other), lap)
    }

    pub fn contains(&self, along: f32, lap: f32) -> bool {
        self.weight(along, lap, 0.0) > 0.0
    }

    /// Lo que mide el tramo en una vuelta de `lap` metros.
    pub fn length(&self, lap: f32) -> f32 {
        if self.to - self.from >= lap {
            lap
        } else {
            (self.to - self.from).rem_euclid(lap)
        }
    }
}

/// A qué lado de la carretera, mirando hacia donde se corre.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// El signo del desplazamiento lateral de ese lado (positivo a la izquierda).
    pub fn sign(self) -> f32 {
        match self {
            Side::Left => 1.0,
            Side::Right => -1.0,
        }
    }
}

/// Un peñasco: una masa de roca con forma irregular.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Massif {
    pub center: Vec2,
    pub radius: f32,
    pub height: f32,
    /// Whether the road climbs it: its smooth slopes are the ground the road
    /// is cut into, rather than rock standing up behind the walls.
    #[serde(default)]
    pub climbed: bool,
}

/// Una pared de roca a un lado de la carretera, como si la hubieran abierto
/// cortando la montaña.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cutting {
    pub span: Span,
    pub side: Side,
    pub height: f32,
}

/// The greens of a track's grass.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum Greens {
    /// The arcade lawn: a bright green, a little lighter or deeper here and
    /// there.
    Lawn,
    /// A countryside's: swathes of many greens, lusher by the water, drier
    /// up high and towards the sun, and dark conifer forest up the hills,
    /// far from the roads and on the mountains across the sea.
    Countryside,
    /// A Mediterranean island's in summer: straw-coloured dry grass, olive
    /// and grey-green sage, fresher green down the hollows and by the
    /// water, and dark scrub up the slopes before the rock, in patches of
    /// every size with ragged edges.
    Mediterranean,
    /// A lush green land's: the countryside's, but deeper
    /// and greener, not yellowed by the sun, with darker drifts where the
    /// grass grows thick.
    Lush,
}

/// Una explanada: el terreno, llano a una altura (m sobre el mar) en un
/// círculo, para poner encima un edificio grande; or sloping, facing one
/// way.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terrace {
    pub center: Vec2,
    pub radius: f32,
    /// How high its middle stands.
    pub height: f32,
    /// How much higher it stands for each metre along x and along z: zero
    /// for level ground.
    #[serde(default)]
    pub tilt: Vec2,
    /// How far past its edge the ground blends back into the land (m):
    /// wider where the land round it stands far above or below it, so the
    /// bank stays grass instead of breaking into a cliff.
    pub skirt: f32,
}

/// Dónde está el mar, y los lagos y los ríos (see [`Waters`]).
#[derive(Deserialize)]
#[serde(from = "crate::circuits::data::CoastFile")]
pub enum Coast {
    /// The sea beyond its shores, and lakes and rivers if any: how far from
    /// them, positive on the land.
    Sea(Waters),
    /// Una isla: tierra dentro del circuito y, por fuera, hasta tantos metros
    /// de la línea central según el punto de la vuelta (pares de metros desde
    /// la meta y distancia; entre ellos se interpola). Con una distancia
    /// negativa el mar entra por debajo de la carretera: una ría.
    AroundTrack(&'static [(f32, f32)]),
    /// An island whose shore is a smooth closed curve through these points
    /// (x, z), drawn like the centre line of the track: the land is inside
    /// it, with its own bays, headlands and inlets. As with `AroundTrack`,
    /// the land drops to the sea right beyond the walls where the shore is
    /// near; an inlet drawn under a bridge brings the sea in under it.
    Outline(&'static [Vec2]),
    /// An island drawn as with `Outline`, with lakes (and rivers) inside
    /// it. The land drops to them right beyond the walls where their shores
    /// are near, as it does to the sea.
    OutlineWithLakes(&'static [Vec2], Waters),
    /// Inland, with no sea: its rivers and lakes, if any; but, as round an
    /// island's lakes, the land drops to them right beyond the walls where
    /// their shores are near.
    Inland(Waters),
}

/// Cómo se cierra la carretera por los lados.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum WallStyle {
    /// Muro de piedra.
    Stone,
    /// Barrera de hormigón blanca.
    Concrete,
    /// Barandilla verde de paseo marítimo.
    Railing,
    /// Seto sobre un murete.
    Hedge,
    /// Blocks of brown checkerboard earth, as on an arcade island.
    Checkered,
    /// A fence of heavy timber planks, as round a mine.
    Timber,
    /// A low vermilion fence of posts and rails, as round a shrine: like
    /// the railing, the ground beyond it shows through.
    ShrineFence,
    /// A low fence of weathered wooden posts and rails, as along the lanes
    /// of the countryside: the ground beyond it shows through.
    Fence,
    /// A low concrete parapet, as along a dam's crest on its lake's side:
    /// the water shows over it, a little way down.
    Parapet,
    /// The low border of a slot-car track, in red and white stripes.
    SlotBorder,
    /// The raised orange edge of the toy cars' track, with a little light
    /// every few metres along its top.
    OrangeEdge,
    /// The tall, rounded side of a slide's chute, of red plastic.
    ChuteSide,
    /// A row of wooden toy blocks, each painted a bright colour of its own
    /// with a letter on its face.
    Blocks,
    /// A low wall of plastic building bricks in bright colours, a course of
    /// bricks and a course of plates on it in a running bond, studs on top
    /// (see `world::bricks`).
    Bricks,
}

/// What the steep faces of the terrain (cliffs, rock walls and the rock
/// around the tunnels) are made of.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum Soil {
    /// Natural rock with tilted strata.
    Rock,
    /// Brown checkerboard earth under a grass top, as on an arcade island.
    Checkered,
    /// A desert canyon of red and ochre rock over dry, dusty earth; its
    /// tunnels are mine galleries propped with timber and lit by lanterns.
    Mine,
    /// Grey granite and schist, bare and hard: the rock of high,
    /// mountainous country.
    Granite,
    /// Grey lunar regolith and basaltic crater rock; its tunnels are
    /// pressurized space-station corridors lit by cool white-cyan strips.
    Lunar,
    /// The floor of a room, indoors: flat boards, with no rock, sand nor
    /// grass; its tunnels are what stands over the road (a table, another
    /// stretch of the road), which draw nothing of their own (the toy
    /// store's, a theme's).
    Floor,
}

/// The light of a track: a summer day; a clear Mediterranean summer
/// afternoon, graded as a photograph is; the last hours of a clear autumn
/// afternoon, the sun low and golden; a low sun setting in a warm, hazy
/// sky; or the black sky of the Moon with the Earth hanging above the
/// horizon.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum SkyStyle {
    Day,
    Mediterranean,
    GoldenHour,
    Dusk,
    /// A summer sunset in high country, the hills' long shadows over one
    /// another, graded as a film is: the light of New Zealand.
    HighCountry,
    Space,
    /// Indoors, under a ceiling: no sky, no sun and no clouds, but the
    /// light of the lamps from overhead, white and warm, softly filling the
    /// shade, as in a shop.
    Indoor,
}

/// The colour of the water: the blue sea, or the red water of a flooded pit
/// rich in iron, as at Rio Tinto.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum Water {
    Sea,
    Iron,
    /// The rivers and lakes of high country: deep green and grey, clearer
    /// over the shallows.
    River,
}

/// An open-pit mine round the water (see [`Scenery::pit`]): benches cut in
/// steps from its rim down to the water, following the shore, and its spoil
/// heaped round the rim.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pit {
    /// How many benches, how high each face steps up (m) and how wide each
    /// bench is (m).
    pub benches: u32,
    pub step: f32,
    pub tread: f32,
    /// How far past the rim the spoil heaped round it slopes down to the
    /// ground (m).
    pub spoil: f32,
}

/// A stretch of the lap carried by a bridge, and how it is built.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bridge {
    pub span: Span,
    pub style: BridgeStyle,
    /// How high a wooden bridge's fence stands over its deck (m): most at
    /// 1.2 m, a boardwalk low over the water lower. The other bridges have
    /// walls of their own, and leave it unused.
    #[serde(default)]
    pub fence: f32,
}

impl BridgeStyle {
    /// Whether it is of wood, old or new, with its own fence.
    pub fn wooden(self) -> bool {
        matches!(self, BridgeStyle::Wooden | BridgeStyle::OldWooden)
    }
}

/// How a bridge is built.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum BridgeStyle {
    /// A concrete deck under two red steel arches.
    RedArches,
    /// A wooden deck of planks on log piers, with a wooden fence on each side.
    Wooden,
    /// The same, old and weathered: its boards silver-grey, patched darker
    /// and mossy, its posts leaning a little and its rails sagging.
    OldWooden,
    /// A concrete deck between two steel through trusses, braced across
    /// overhead, on concrete piers.
    Truss,
    /// The crest of a dam: the concrete of the dam itself, down to the
    /// gorge below it, with the lake it holds on the other side (see
    /// [`Dam`], which says how).
    Dam,
    /// A slot-car track: its dark plastic with the slots and their rails,
    /// lying on the furniture under it, or carried over the floor on blue
    /// lattice pillars, between red and white borders.
    SlotTrack,
    /// The toy cars' orange track, with its chevrons, on blue pillars, between
    /// its raised edges.
    OrangeTrack,
    /// A slide's chute of yellow plastic between tall red sides.
    Slide,
}

/// A dam across a gorge, as El Atazar's, with the road along its crest (a
/// bridge of [`BridgeStyle::Dam`] over the same span): an arch of concrete
/// bowed towards the lake it holds, its water up near the crest; on the
/// other side its face drops nearly sheer to the river, which runs on down
/// the gorge to the sea.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dam {
    /// Where the road runs along the crest.
    pub crest: Span,
    /// Which side of the road the lake is on.
    pub lake_side: Side,
    /// The lake's shore: a smooth closed curve through these points (x,
    /// z), drawn as a coast's is, round a middle from which every point of
    /// it can be seen (the water is a fan from there). Where it meets the
    /// dam it runs along the crest, so the water reaches the dam's face.
    #[serde(deserialize_with = "crate::circuits::data::leak")]
    pub lake: &'static [Vec2],
    /// The height of the lake's water (m over the sea): a little under the
    /// crest.
    pub level: f32,
    /// The river below the dam: points (x, z) along its course, from
    /// under the dam (its first point past the crest, so the gorge runs on
    /// under it) down to the sea, each with how high the gorge's far
    /// wall stands there over the sea (m). Its water is the sea's, coming up
    /// the gorge to the foot of the dam.
    #[serde(deserialize_with = "crate::circuits::data::leak")]
    pub river: &'static [(Vec2, f32)],
    /// Which bank of the river (going downstream) the road up the gorge
    /// runs along: on that side the bank only drops from the road to the
    /// water; across the river, the gorge's far wall rises.
    pub road_bank: Side,
    /// Where a paved square stands by the end of the crest with a
    /// monument in the middle, if one does (x, z): on level ground, as a
    /// [`Terrace`] makes it.
    #[serde(default)]
    pub monument: Option<Vec2>,
}

/// Un tramo con otro muro distinto del de piedra a un lado.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WallRun {
    pub span: Span,
    pub side: Side,
    pub style: WallStyle,
}

/// Un arco sobre la carretera con un rótulo.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gantry {
    pub along: f32,
    #[serde(deserialize_with = "crate::circuits::data::color")]
    pub color: Color,
    /// El rótulo, o un tablero de ajedrez si no hay.
    #[serde(deserialize_with = "crate::circuits::data::optional_text", default)]
    pub text: Option<&'static str>,
}

/// Un cartel publicitario junto a la carretera.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Billboard {
    pub along: f32,
    pub side: Side,
    #[serde(deserialize_with = "crate::circuits::data::text")]
    pub text: &'static str,
    #[serde(deserialize_with = "crate::circuits::data::color")]
    pub color: Color,
    /// Desde qué letra, y en qué color, van las demás.
    #[serde(deserialize_with = "crate::circuits::data::accent", default)]
    pub accent: Option<(usize, Color)>,
}

/// Una fila de algo a lo largo de la carretera: en un tramo, a un lado, a
/// esta distancia de la línea central y cada tantos metros.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub span: Span,
    pub side: Side,
    pub lateral: f32,
    pub every: f32,
}

/// Especies de árbol.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum Species {
    UmbrellaPine,
    Cypress,
    Olive,
    /// Árbol alto de bosque, de copa alargada y oscura.
    Tall,
    /// Árbol de hoja ancha, de copa redonda y verde claro.
    Broadleaf,
    /// Maritime pine with tiered branches and coastal habit.
    MaritimePine,
    /// Slender poplar / aspen with upright crown and bright green foliage.
    Poplar,
    /// Almond / Mediterranean flowering tree with warm pink-tinted blossom.
    Almond,
    /// Japanese maple, crimson in autumn.
    Momiji,
    /// Ginkgo, golden yellow in autumn.
    Ginkgo,
    /// Japanese cherry in pale pink blossom.
    Sakura,
    /// Japanese black pine, windswept, its foliage in flat pads.
    BlackPine,
    /// Japanese cedar, tall and narrow.
    Sugi,
    /// A clump of tall bamboo.
    Bamboo,
}

/// A forest: a stretch with trees close together on both sides of the road,
/// of these species.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Forest {
    pub span: Span,
    #[serde(deserialize_with = "crate::circuits::data::leak")]
    pub species: &'static [Species],
    /// A tree every so many metres along the road, in each of its rows
    /// either side ([`FOREST_EVERY`], or more where it is more open).
    #[serde(default = "forest_every")]
    pub every: f32,
}

/// How far apart along the road a forest's trees are, in each row (m).
pub const FOREST_EVERY: f32 = 5.0;

fn forest_every() -> f32 {
    FOREST_EVERY
}

/// Las casas: cuántas como mucho, de qué colores y, si hace falta, solo en
/// unos tramos.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Houses {
    pub max: usize,
    #[serde(deserialize_with = "crate::circuits::data::colors")]
    pub paints: &'static [Color],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub spans: &'static [Span],
    pub style: HouseStyle,
}

/// How the houses are built.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum HouseStyle {
    /// Plastered walls under a tiled gable roof.
    Cottage,
    /// The same, with a wooden false front standing up over the roof at the
    /// front and a porch along it, as in the towns of the Old West.
    FalseFront,
}

/// Todo lo que hace falta para construir el escenario de una pista.
#[derive(Deserialize)]
// Its gantries' and billboards' texts are `&'static`, so a file is read
// from a text as lasting (see `crate::circuits::data::parse`).
#[serde(deny_unknown_fields, bound(deserialize = "'de: 'static"))]
pub struct Scenery {
    pub coast: Coast,
    /// Altura de las colinas (m).
    pub hills: f32,
    /// How the grass is coloured: one track's greens never change
    /// another's.
    pub greens: Greens,
    /// How light the grass is: 1 as painted, less for darker, less vivid
    /// greens (the grass in flower darkens half as much, the flowers being
    /// lighter; the hedges as much as the grass).
    pub grass_shade: f32,
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub massifs: &'static [Massif],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub cuttings: &'static [Cutting],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub terraces: &'static [Terrace],
    /// Montañas lejanas que cierran el horizonte: none if left out.
    #[serde(default)]
    pub mountains: Mountains,
    /// Colores de las franjas de los pianos.
    #[serde(deserialize_with = "crate::circuits::data::color_pair")]
    pub curbs: [Color; 2],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub walls: &'static [WallRun],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub tunnels: &'static [Span],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub bridges: &'static [Bridge],
    /// Dams, each with the road along its crest (see [`Dam`]).
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub dams: &'static [Dam],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub gantries: &'static [Gantry],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub billboards: &'static [Billboard],
    /// Farolas, cada `every` metros.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub lamps: &'static [Row],
    /// Especies de los árboles sueltos, y bosques: tramos con árboles altos
    /// muy juntos a los dos lados.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub trees: &'static [Species],
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub forests: &'static [Forest],
    pub houses: Houses,
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub palms: &'static [Row],
    /// Hoteles altos en fila, de colores.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub hotels: &'static [Row],
    /// Un gran hotel de los de antes: punto de la vuelta y distancia a la
    /// carretera (con signo, positiva a la izquierda).
    #[serde(default)]
    pub grand_hotel: Option<(f32, f32)>,
    /// Boyas de rayas en el mar.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub buoys: &'static [Vec2],
    /// Faros costeros en islotes o cabos.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub lighthouses: &'static [Lighthouse],
    /// Veleros y yates fondeados en la bahía o el mar abierto.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub boats: &'static [Boat],
    /// Flechas indicadoras de curva (<<< y >>>) en curvas cerradas.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub chevrons: &'static [ChevronSign],
    /// Tribunas cubiertas con público frente a la pista.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub grandstands: &'static [Grandstand],
    /// Sombrillas y tumbonas de colores en las playas.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub beach_parasols: &'static [ParasolCluster],
    /// Carteles de aviso de frenada (100 m, 50 m).
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub brake_markers: &'static [BrakeMarker],
    /// What cliffs and rock walls are made of.
    pub soil: Soil,
    /// Rows of tall sunflowers turned towards the road.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub sunflowers: &'static [Row],
    /// Carved totem poles beside the road.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub totems: &'static [Totem],
    /// Giant vertical loops standing beside the road.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub loops: &'static [Loop],
    /// Lines of spinning golden rings floating over the road.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub rings: &'static [Rings],
    /// Waterfalls pouring down the rock walls of the cuttings.
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub waterfalls: &'static [Waterfall],
    /// The light: day or dusk.
    pub sky: SkyStyle,
    /// The colour of the sea, the lakes and the rivers.
    pub water: Water,
    /// Whether the sea is always drawn as a cartoon, flat cartoon
    /// blues with waves rolling in (an arcade island's); on the
    /// other tracks it mirrors what stands round it unless Options >
    /// Graphics > Water says otherwise (see [`crate::world::sea`]).
    #[serde(default)]
    pub cartoon_water: bool,
    /// An open pit round the water, if there is one.
    #[serde(default)]
    pub pit: Option<Pit>,
    /// The portals between the realms of a split circuit, across the road
    /// (see [`super::realm`] and [`super::portals`]).
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub portals: &'static [Portal],
    /// The halls of an excavation, a mine's or great halls': where there are, its
    /// tunnels are dug through the rock (see [`Scenery::excavation`]).
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub halls: &'static [Hall],
    /// The colour of the halo the rear wheel leaves behind it on the road
    /// while the bike rides a wheelie, as a spin in a platformer, or
    /// `None` for none (see `crate::bike::wheelie_halo`).
    #[serde(deserialize_with = "crate::circuits::data::optional_color", default)]
    pub wheelie_halo: Option<Color>,
    /// Fields of flowers (see [`crate::world::flowers`]).
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub flowers: &'static [FlowerField],
    /// Petals, or leaves, drifting down (see [`crate::world::petals`]).
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub petals: &'static [Petals],
    /// Where the ground stays grass however steep it is (see [`Turf`]).
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub turf: &'static [Turf],
    /// Where the granite is of another colour (see [`RockTint`]).
    #[serde(deserialize_with = "crate::circuits::data::leak", default)]
    pub rock_tints: &'static [RockTint],
    /// The scenery a game adds of its own, by the names its themes were
    /// registered under (see [`crate::world::themes`]).
    #[serde(default)]
    pub themes: Themes,
}

impl Scenery {
    /// The halls of the tunnels where they are excavations, a mine's or
    /// great halls' (see [`super::cavern`]), rather than tunnels lined with
    /// concrete panels.
    pub fn excavation(&self) -> Option<&'static [Hall]> {
        (self.soil == Soil::Mine || !self.halls.is_empty()).then_some(self.halls)
    }
}

/// A field of flowers (see [`crate::world::flowers`]): the ground in
/// flower, of one kind or, in patches, of several, with gaps of grass among
/// them and a ragged edge, and, near the road, clumps standing up. Where
/// fields overlap, the first one decides.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlowerField {
    pub area: FlowerArea,
    #[serde(deserialize_with = "crate::circuits::data::leak")]
    pub kinds: &'static [FlowerKind],
}

/// Where a field of flowers grows: beside a stretch of the lap, on both
/// sides, from `near` to `far` metres from its centre line; on the
/// shoulders of the roads along a stretch of the lap, between the asphalt
/// and the walls (in patches, with as much grass between them: bare earth
/// worn by the wheels, or fallen leaves), which the other fields keep off;
/// round a point, from `inner` to `outer` metres from it (a ring round a
/// lake, or, from 0, a round meadow, which nothing else is put in); or
/// everywhere else.
#[derive(Deserialize)]
pub enum FlowerArea {
    Road {
        span: Span,
        near: f32,
        far: f32,
    },
    Shoulders {
        span: Span,
    },
    Round {
        center: Vec2,
        inner: f32,
        outer: f32,
    },
    Everywhere,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum FlowerKind {
    /// Red spider lilies (higanbana): bare stems crowned with curling
    /// petals and long stamens.
    SpiderLily,
    /// White flowers with yellow hearts among their leaves.
    White,
    /// Yellow rape blossom (nanohana), in clusters on branching stems.
    Yellow,
    /// Purple irises among their sword-shaped leaves, as by the water.
    Iris,
    /// Susuki, the pampas grass of the autumn hills: arching blades under
    /// silvery gold plumes, taller than the others.
    Susuki,
    /// Shibazakura, moss phlox: a carpet of tiny flowers hugging the ground,
    /// in bands of magenta, pink, lavender and white, as round the lakes at
    /// the foot of Mount Fuji.
    Phlox,
    /// Not flowers, but the grass worn down to bare earth, brown and dusty,
    /// with tufts of grass and pebbles in it, as round a town or a wood.
    BareEarth,
    /// Not flowers, but leaves fallen over the grass, brown, rust, ochre,
    /// gold and red, as under the trees in autumn.
    FallenLeaves,
}

/// Petals, or leaves, drifting down through the air over an area, falling
/// and swaying, over and over.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Petals {
    pub area: PetalArea,
    pub kind: PetalKind,
}

/// Where petals drift: over a stretch of the lap, out to `reach` metres
/// either side of its centre line, or round a point.
#[derive(Deserialize)]
pub enum PetalArea {
    Road { span: Span, reach: f32 },
    Round { center: Vec2, radius: f32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum PetalKind {
    /// Red maple leaves.
    Maple,
    /// White cherry petals.
    Cherry,
    /// The golden leaves of the mallorns.
    Gold,
}

/// What a small prop by the road stands on, so that it is seen over the
/// road's wall: a block of stone, or a deck of planks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Plinth {
    Stone,
    Wood,
}

/// A prop and where it stands, facing: by the road, `along` metres from the
/// finish line and `lateral` metres to the left of its centre line (to the
/// right if negative), facing the road; over the road, facing along it; or
/// anywhere, at `position` (x, z) facing `heading` (see
/// [`crate::geometry`]). It stands on the ground, or in the water, with its
/// foot a little under it; `scale` times as big as made, turned `turn`
/// radians from that about its foot (anticlockwise seen from above: one by
/// the road turned to face the riders coming along it), and leaning `lean`
/// radians towards its side that was -X as made (a tree leaning out over
/// what its crown spreads over). Its model is one of a theme's.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prop<M> {
    pub model: M,
    pub spot: Spot,
    #[serde(default = "as_made")]
    pub scale: f32,
    #[serde(default)]
    pub turn: f32,
    #[serde(default)]
    pub lean: f32,
    /// Whether it faces the riders coming up the road at it, instead of
    /// the road beside it: a gate on a hilltop reads as a gate from far
    /// down the road, not as two posts edge on.
    #[serde(default)]
    pub facing_riders: bool,
}

/// A [`Prop`]'s scale where a circuit's file leaves it out: as made.
fn as_made() -> f32 {
    1.0
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub enum Spot {
    Road {
        along: f32,
        lateral: f32,
    },
    /// Over the middle of the road, as a gate the road runs through.
    Over {
        along: f32,
    },
    At {
        position: Vec2,
        heading: f32,
    },
}

/// A prop by the road (see [`Prop`]).
pub const fn by_road<M: Copy>(model: M, along: f32, lateral: f32) -> Prop<M> {
    Prop {
        model,
        spot: Spot::Road { along, lateral },
        scale: 1.0,
        turn: 0.0,
        lean: 0.0,
        facing_riders: false,
    }
}

/// A round stretch of land whose steep slopes are grass, not rock (see
/// [`Scenery::turf`]): grass up to `steepest`, the upward part of the
/// ground's normal under which it is rock (0.45: all but sheer faces; more,
/// grass on the ledges and gentler slopes of a rock face, rock between); and
/// its granite green with moss over much of it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Turf {
    pub center: Vec2,
    pub radius: f32,
    pub steepest: f32,
}

/// A round stretch of land whose granite is of another colour (see
/// [`Scenery::rock_tints`]): `color`, as light as the grey it stands
/// for, fading back into the grey over the outer quarter of `radius`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RockTint {
    pub center: Vec2,
    pub radius: f32,
    #[serde(deserialize_with = "crate::circuits::data::color")]
    pub color: Color,
}

/// A stretch of an excavation, a mine's or great halls', in metres along the lap (a branch off
/// the lap counts as the stretch it stands in for): the rock it is cut
/// through and how high its roof rises above the road in the middle (m).
/// Where there is none, the excavation is a gallery propped with timber.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hall {
    pub span: Span,
    pub stone: Stone,
    pub crown: f32,
    /// How much wider than the road between its walls the hall is, either
    /// side (m): where it is, the road runs through it between low walls of
    /// its stone (none in a mine).
    #[serde(default)]
    pub width: f32,
}

/// What the rock of a stretch of a mine's excavation is like, and the
/// ground beside its roads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Stone {
    /// A gallery propped with timber sets, with planks beside the road.
    Timbered,
    /// Red and cream sandstone in layers, over red gravel.
    Sandstone,
    /// Dark slate with veins of gold, over black gravel.
    Slate,
    /// Ochre clay, its walls lined with planks, over beaten earth.
    Clay,
    /// Masonry, as in great halls dug under a mountain: walls and vaults of dressed grey
    /// blocks over flagstones, great pillars in the widest halls.
    Masonry,
    /// A natural cave in grey granite, over grit.
    Cave,
}

/// A carved totem pole beside the road, facing it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Totem {
    pub along: f32,
    pub side: Side,
}

/// A giant vertical loop on a block of earth beside the road, in the plane
/// of the road: from the road it is seen side on, as in a 2D platformer.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Loop {
    pub along: f32,
    pub side: Side,
    /// Distance from the centre line to the middle of the loop (m).
    pub lateral: f32,
    /// Radius of the running surface inside the loop (m).
    pub radius: f32,
}

/// A line of `count` golden rings over the road from `along` on, `lateral`
/// metres to the left of the centre line and `height` metres above the road.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rings {
    pub along: f32,
    pub count: u32,
    pub lateral: f32,
    pub height: f32,
}

/// A waterfall `width` metres wide down the rock wall of a cutting.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Waterfall {
    pub along: f32,
    pub side: Side,
    pub width: f32,
}

/// Un faro costero sobre un peñasco o cabo.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lighthouse {
    pub position: Vec2,
    pub height: f32,
}

/// Embarcaciones fondeadas en el mar.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize)]
pub enum BoatKind {
    Sailboat,
    Yacht,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Boat {
    pub position: Vec2,
    pub heading: f32,
    pub kind: BoatKind,
}

/// Dirección de la flecha de curva.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Deserialize)]
pub enum ChevronDirection {
    Left,
    Right,
}

/// Cartel de curva con flechas reflectantes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChevronSign {
    pub along: f32,
    pub side: Side,
    pub direction: ChevronDirection,
}

/// Tribuna de espectadores cubierta.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grandstand {
    pub along: f32,
    pub side: Side,
    pub lateral: f32,
    pub length: f32,
}

/// Grupo de sombrillas y tumbonas en la arena.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParasolCluster {
    pub position: Vec2,
    pub count: usize,
}

/// Cartel de distancia a curva (100 m, 50 m).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrakeMarker {
    pub along: f32,
    pub side: Side,
    pub distance: u32,
}

/// An interdimensional stargate portal spanning over the road at `along`
/// metres from the finish line, with twin energy pylons, counter-rotating
/// plasma rings, and a translucent glowing vortex curtain.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Portal {
    pub along: f32,
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    #[test]
    fn spans_can_wrap_around_the_finish_line() {
        let lap = 1000.0;
        let wrapped = span(900.0, 100.0);
        assert!(wrapped.contains(950.0, lap) && wrapped.contains(50.0, lap));
        assert!(!wrapped.contains(500.0, lap));
        assert_eq!(wrapped.length(lap), 200.0);
        // Se entra poco a poco.
        assert!(wrapped.weight(905.0, lap, 20.0) < 0.5);
        assert_eq!(wrapped.weight(0.0, lap, 20.0), 1.0);
        assert_eq!(LAP.weight(500.0, lap, 20.0), 1.0);
        assert_eq!(LAP.length(lap), lap);
    }
}
