//! The lineup library Arcwise browses and the selection logic behind its screens, kept free of any
//! UI so it can be tested on its own.

mod args;
mod browse;
mod landing;
mod library;
mod model;
mod version_line;

pub use args::Args;
pub use browse::{Browse, Chooser, Direction, Start, View};
pub use landing::{CLUSTER_RADIUS, Landing, LandingKey, Tally, cluster};
pub use library::{FORMAT, Library, LoadError, MapLineups, Part};
pub use model::{
    Ability, AbilityId, Agent, AgentId, Bolt, Bounces, Charge, Lineup, LineupId, Map, MapId, Point,
    Rgb, Role, Side, Spot, Steps, Tag,
};
pub use version_line::version_line;
