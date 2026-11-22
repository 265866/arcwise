//! The records in `library.json`.

use std::fmt::{self, Display, Formatter};
use std::iter;

use serde::{Deserialize, Serialize};

/// A map's id, a UUID string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MapId(String);

impl MapId {
    /// The id written `id`.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(id.to_owned())
    }

    /// The id as written in the library.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for MapId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An agent's id, a UUID string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    /// The id written `id`.
    #[must_use]
    pub fn new(id: &str) -> Self {
        Self(id.to_owned())
    }

    /// The id as written in the library.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for AgentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An ability's number, unique within its library.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(transparent)]
pub struct AbilityId(pub u32);

impl Display for AbilityId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A lineup's number, unique within its library; its clip and pictures are named after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(transparent)]
pub struct LineupId(pub u32);

impl Display for LineupId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A map the library has lineups on.
#[derive(Clone, Debug, Deserialize)]
pub struct Map {
    /// The map's id, which lineups refer to it by.
    pub id: MapId,
    /// The name the map chooser shows.
    pub name: String,
}

/// An agent's role, which groups agents in the agent chooser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Takes fights first.
    Duelist,
    /// Gathers information and sets up fights.
    Initiator,
    /// Blocks sight lines with smokes.
    Controller,
    /// Holds ground and watches flanks.
    Sentinel,
}

impl Role {
    /// The order the agent chooser lists roles in.
    pub const ALL: [Role; 4] = [
        Role::Duelist,
        Role::Initiator,
        Role::Controller,
        Role::Sentinel,
    ];

    /// The role's heading in the agent chooser, such as `Duelists`.
    #[must_use]
    pub fn plural(self) -> &'static str {
        match self {
            Role::Duelist => "Duelists",
            Role::Initiator => "Initiators",
            Role::Controller => "Controllers",
            Role::Sentinel => "Sentinels",
        }
    }
}

/// A color written `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct Rgb(pub u32);

impl TryFrom<String> for Rgb {
    type Error = String;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.strip_prefix('#')
            .filter(|hex| hex.len() == 6)
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .map(Rgb)
            .ok_or_else(|| format!("expected a #rrggbb color, found {s:?}"))
    }
}

/// An agent and the agent's abilities.
#[derive(Clone, Debug, Deserialize)]
pub struct Agent {
    /// The agent's id, which lineups refer to it by.
    pub id: AgentId,
    /// The agent's name.
    pub name: String,
    /// The agent's role.
    pub role: Role,
    /// The agent's color, as the library gives it.
    pub color: Rgb,
    /// The agent's abilities, as the library lists them.
    pub abilities: Vec<Ability>,
}

/// One of an agent's abilities.
#[derive(Clone, Debug, Deserialize)]
pub struct Ability {
    /// The ability's number, which lineups refer to it by.
    pub id: AbilityId,
    /// The ability's name.
    pub name: String,
    /// Position on the agent's ability bar, 1 to 4; abilities are listed in this order.
    pub slot: u8,
}

/// The side of the round a lineup is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// The side that plants the spike.
    Attack,
    /// The side that defuses it.
    Defense,
}

impl Side {
    /// The opposite side.
    #[must_use]
    pub fn other(self) -> Side {
        match self {
            Side::Attack => Side::Defense,
            Side::Defense => Side::Attack,
        }
    }

    /// The side's name as the header shows it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Side::Attack => "Attack",
            Side::Defense => "Defense",
        }
    }
}

/// A position on the map image, in percent of its width (`x`) and height (`y`).
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(from = "[f32; 2]")]
pub struct Point {
    /// Percent of the map image's width from its left edge.
    pub x: f32,
    /// Percent of the map image's height from its top edge.
    pub y: f32,
}

impl Point {
    /// The point written `[x, y]` in the library.
    #[must_use]
    pub fn from_pair([x, y]: [f32; 2]) -> Self {
        Point { x, y }
    }

    /// The straight-line distance to `other`, in percent of the map image.
    #[must_use]
    pub fn distance(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

impl From<[f32; 2]> for Point {
    fn from(pair: [f32; 2]) -> Self {
        Point::from_pair(pair)
    }
}

/// A named place on the map: where the player stands, or where the utility lands.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Spot {
    /// Percent of the map image's width from its left edge.
    pub x: f32,
    /// Percent of the map image's height from its top edge.
    pub y: f32,
    /// The place's name in the game, such as `A Main`, when the library gives one.
    pub callout: Option<String>,
}

impl Spot {
    /// Where the spot is on the map image.
    #[must_use]
    pub fn at(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }
}

/// A label the library puts on a lineup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    /// One of the few lineups worth learning first.
    Essential,
    /// One professional players use.
    Pro,
    /// A tag this version of Arcwise doesn't know, which it ignores.
    #[serde(other)]
    Other,
}

/// How to throw: what to do before aiming, where to aim, and the release.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Steps {
    /// What to do before aiming, in order, such as walking to a corner.
    pub setup: Vec<String>,
    /// What the crosshair lines up with, when the lineup says.
    pub aim: Option<String>,
    /// How to release the throw, such as a left click.
    pub release: String,
    /// Shown over the clip when the throw needs more than a plain click, such as a jump.
    pub note: Option<String>,
}

impl Steps {
    /// The steps in order, the aim prefixed with `Aim:`.
    pub fn lines(&self) -> impl Iterator<Item = String> + '_ {
        self.setup
            .iter()
            .cloned()
            .chain(self.aim.iter().map(|aim| format!("Aim: {aim}")))
            .chain(iter::once(self.release.clone()))
    }
}

/// How far Sova's bow is drawn when a bolt leaves: the section of the three-section charge bar the
/// fill has reached, or a full bar, which turns red. Written 0 to 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
pub enum Charge {
    /// Released while the fill is in the first section.
    None,
    /// Released while the fill is in the second section.
    One,
    /// Released while the fill is in the third section.
    Two,
    /// Released with the bar full and red.
    Full,
}

impl TryFrom<u8> for Charge {
    type Error = String;

    fn try_from(n: u8) -> Result<Self, Self::Error> {
        match n {
            0 => Ok(Charge::None),
            1 => Ok(Charge::One),
            2 => Ok(Charge::Two),
            3 => Ok(Charge::Full),
            _ => Err(format!("a charge is 0 to 3, not {n}")),
        }
    }
}

impl Charge {
    /// The sections the fill has passed: 0 to 3.
    #[must_use]
    pub fn sections(self) -> u8 {
        match self {
            Charge::None => 0,
            Charge::One => 1,
            Charge::Two => 2,
            Charge::Full => 3,
        }
    }

    /// The charge as the panel names it, such as `1 charge`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Charge::None => "No charge",
            Charge::One => "1 charge",
            Charge::Two => "2 charge",
            Charge::Full => "Full charge",
        }
    }
}

/// How many times a bolt bounces before it lands, chosen with right click: 0 to 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
pub struct Bounces(u8);

impl TryFrom<u8> for Bounces {
    type Error = String;

    fn try_from(n: u8) -> Result<Self, Self::Error> {
        if n <= 2 {
            Ok(Bounces(n))
        } else {
            Err(format!("a bolt bounces 0 to 2 times, not {n}"))
        }
    }
}

impl Bounces {
    /// The number of bounces: 0 to 2.
    #[must_use]
    pub fn count(self) -> u8 {
        self.0
    }

    /// The bounces as the panel names them, such as `1 bounce`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self.0 {
            0 => "No bounce",
            1 => "1 bounce",
            _ => "2 bounces",
        }
    }
}

/// One of Sova's bolts, as his bow's HUD shows it when it leaves.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Bolt {
    /// How far the bow is drawn.
    pub charge: Charge,
    /// How many times the bolt bounces.
    pub bounces: Bounces,
    /// What the crosshair lines up with for this bolt, when the bolts of a lineup aim apart.
    #[serde(default)]
    pub aim: Option<String>,
}

/// One way to throw one ability to one place.
#[derive(Clone, Debug, Deserialize)]
pub struct Lineup {
    /// The lineup's number; its clip and pictures are named after it.
    pub id: LineupId,
    /// The map it is on.
    pub map: MapId,
    /// The agent who throws it.
    pub agent: AgentId,
    /// The ability thrown, one of the agent's.
    pub ability: AbilityId,
    /// The side of the round it is for.
    pub side: Side,
    /// The lineup's name, as the list shows it.
    pub title: String,
    /// The library's labels for it.
    #[serde(default)]
    pub tags: Vec<Tag>,
    /// Where to stand.
    pub stand: Spot,
    /// The throw's path from the stand spot to the landing.
    #[serde(default)]
    pub path: Vec<Point>,
    /// Where the utility lands.
    pub land: Spot,
    /// How to throw, when the library says.
    pub steps: Option<Steps>,
    /// Sova's bolts, in the order they are fired: one, or two for Double Shock Bolt. Empty for
    /// other abilities.
    #[serde(default)]
    pub bolts: Vec<Bolt>,
}
