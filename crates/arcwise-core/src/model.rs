//! The records in `library.json`.

use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A map's id, a UUID string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MapId(Arc<str>);

/// An agent's id, a UUID string.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentId(Arc<str>);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(transparent)]
pub struct AbilityId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(transparent)]
pub struct LineupId(pub u32);

macro_rules! string_id {
    ($($id:ident),*) => {$(
        impl $id {
            pub fn new(id: impl Into<Arc<str>>) -> Self {
                Self(id.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $id {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    )*};
}
string_id!(MapId, AgentId);

impl fmt::Display for AbilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl fmt::Display for LineupId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Map {
    pub id: MapId,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Duelist,
    Initiator,
    Controller,
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

#[derive(Clone, Debug, Deserialize)]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    pub role: Role,
    pub color: Rgb,
    pub abilities: Vec<Ability>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Ability {
    pub id: AbilityId,
    pub name: String,
    /// Position on the agent's ability bar, 1 to 4; abilities are listed in this order.
    pub slot: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Attack,
    Defense,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Attack => Side::Defense,
            Side::Defense => Side::Attack,
        }
    }

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
    pub x: f32,
    pub y: f32,
}

impl From<[f32; 2]> for Point {
    fn from([x, y]: [f32; 2]) -> Self {
        Point { x, y }
    }
}

impl Point {
    pub fn distance(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// A named place on the map: where the player stands, or where the utility lands.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Spot {
    pub x: f32,
    pub y: f32,
    pub callout: Option<String>,
}

impl Spot {
    pub fn at(&self) -> Point {
        Point {
            x: self.x,
            y: self.y,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    Essential,
    Pro,
    #[serde(other)]
    Other,
}

/// How to throw: what to do before aiming, where to aim, and the release.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Steps {
    pub setup: Vec<String>,
    pub aim: Option<String>,
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
            .chain(std::iter::once(self.release.clone()))
    }
}

/// How far Sova's bow is drawn when a bolt leaves: the section of the three-section charge bar the
/// fill has reached, or a full bar, which turns red. Written 0 to 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u8")]
pub enum Charge {
    /// Released while the fill is in the first section.
    None,
    One,
    Two,
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
    pub fn sections(self) -> u8 {
        match self {
            Charge::None => 0,
            Charge::One => 1,
            Charge::Two => 2,
            Charge::Full => 3,
        }
    }

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
    pub fn count(self) -> u8 {
        self.0
    }

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
    pub charge: Charge,
    pub bounces: Bounces,
    /// What the crosshair lines up with for this bolt, when the bolts of a lineup aim apart.
    #[serde(default)]
    pub aim: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Lineup {
    pub id: LineupId,
    pub map: MapId,
    pub agent: AgentId,
    pub ability: AbilityId,
    pub side: Side,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<Tag>,
    pub stand: Spot,
    /// The throw's path from the stand spot to the landing.
    #[serde(default)]
    pub path: Vec<Point>,
    pub land: Spot,
    pub steps: Option<Steps>,
    /// Sova's bolts, in the order they are fired: one, or two for Double Shock Bolt. Empty for
    /// other abilities.
    #[serde(default)]
    pub bolts: Vec<Bolt>,
}
