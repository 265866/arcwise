//! The lineup library Arcwise browses and the selection logic behind its screens, kept free of any
//! UI so it can be tested on its own.

mod landing;
mod library;
mod model;

pub use landing::{CLUSTER_RADIUS, Landing, LandingKey, cluster};
pub use library::{FORMAT, Library, LoadError};
pub use model::{
    Ability, AbilityId, Agent, AgentId, Lineup, LineupId, Map, MapId, Point, Rgb, Role, Side, Spot,
    Steps, Tag,
};
