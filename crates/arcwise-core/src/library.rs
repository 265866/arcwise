//! A lineup library on disk: `library.json` and the media files beside it. The layout is described
//! in `docs/library-format.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::{
    Ability, AbilityId, Agent, AgentId, Lineup, LineupId, Map, MapId, Role, Side, Tag,
};

/// The `format` value this version reads.
pub const FORMAT: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is not a lineup library")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("library format {found} is not supported; this version reads format {FORMAT}")]
    Format { found: u32 },
    #[error("lineup {lineup} refers to {what}, which the library does not list")]
    Dangling { lineup: LineupId, what: String },
    #[error("{0} is listed more than once")]
    Duplicate(String),
}

#[derive(Deserialize)]
struct File {
    format: u32,
    maps: Vec<Map>,
    agents: Vec<Agent>,
    lineups: Vec<Lineup>,
}

#[derive(Debug)]
pub struct Library {
    root: PathBuf,
    maps: Vec<Map>,
    agents: Vec<Agent>,
    lineups: Vec<Lineup>,
    map_index: HashMap<MapId, usize>,
    agent_index: HashMap<AgentId, usize>,
    ability_index: HashMap<AbilityId, (usize, usize)>,
    lineup_index: HashMap<LineupId, usize>,
    counts: HashMap<(MapId, AgentId), usize>,
}

impl Library {
    /// Reads `library.json` from `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, LoadError> {
        let root = root.into();
        let path = root.join("library.json");
        let bytes = std::fs::read(&path).map_err(|source| LoadError::Read {
            path: path.clone(),
            source,
        })?;
        Self::from_json(root, &bytes)
    }

    /// Parses a library whose media lives under `root`.
    pub fn from_json(root: impl Into<PathBuf>, json: &[u8]) -> Result<Self, LoadError> {
        let root = root.into();
        let file: File = serde_json::from_slice(json).map_err(|source| LoadError::Parse {
            path: root.join("library.json"),
            source,
        })?;
        if file.format != FORMAT {
            return Err(LoadError::Format { found: file.format });
        }
        let mut library = Library {
            root,
            map_index: index_unique(&file.maps, "map", |m| m.id.clone())?,
            agent_index: index_unique(&file.agents, "agent", |a| a.id.clone())?,
            ability_index: HashMap::new(),
            lineup_index: HashMap::new(),
            counts: HashMap::new(),
            maps: file.maps,
            agents: file.agents,
            lineups: file.lineups,
        };
        for (a, agent) in library.agents.iter_mut().enumerate() {
            agent.abilities.sort_by_key(|ab| ab.slot);
            for (b, ability) in agent.abilities.iter().enumerate() {
                if library.ability_index.insert(ability.id, (a, b)).is_some() {
                    return Err(LoadError::Duplicate(format!("ability {}", ability.id)));
                }
            }
        }
        for (i, lineup) in library.lineups.iter_mut().enumerate() {
            lineup.tags.retain(|t| *t != Tag::Other);
            if library.lineup_index.insert(lineup.id, i).is_some() {
                return Err(LoadError::Duplicate(format!("lineup {}", lineup.id)));
            }
        }
        for lineup in &library.lineups {
            library.check(lineup)?;
            *library
                .counts
                .entry((lineup.map.clone(), lineup.agent.clone()))
                .or_default() += 1;
        }
        Ok(library)
    }

    fn check(&self, lineup: &Lineup) -> Result<(), LoadError> {
        let dangling = |what: String| LoadError::Dangling {
            lineup: lineup.id,
            what,
        };
        if !self.map_index.contains_key(&lineup.map) {
            return Err(dangling(format!("map {}", lineup.map)));
        }
        let Some(&agent) = self.agent_index.get(&lineup.agent) else {
            return Err(dangling(format!("agent {}", lineup.agent)));
        };
        match self.ability_index.get(&lineup.ability) {
            Some(&(owner, _)) if owner == agent => Ok(()),
            _ => Err(dangling(format!(
                "ability {} of {}",
                lineup.ability, self.agents[agent].name
            ))),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn maps(&self) -> &[Map] {
        &self.maps
    }

    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    pub fn lineups(&self) -> &[Lineup] {
        &self.lineups
    }

    pub fn map(&self, id: &MapId) -> Option<&Map> {
        self.map_index.get(id).map(|&i| &self.maps[i])
    }

    pub fn agent(&self, id: &AgentId) -> Option<&Agent> {
        self.agent_index.get(id).map(|&i| &self.agents[i])
    }

    pub fn ability(&self, id: AbilityId) -> Option<&Ability> {
        self.ability_index
            .get(&id)
            .map(|&(a, b)| &self.agents[a].abilities[b])
    }

    pub fn lineup(&self, id: LineupId) -> Option<&Lineup> {
        self.lineup_index.get(&id).map(|&i| &self.lineups[i])
    }

    pub fn map_by_name(&self, name: &str) -> Option<&Map> {
        self.maps.iter().find(|m| m.name.eq_ignore_ascii_case(name))
    }

    pub fn agent_by_name(&self, name: &str) -> Option<&Agent> {
        self.agents
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
    }

    /// Lineups for one agent on one map.
    pub fn count(&self, map: &MapId, agent: &AgentId) -> usize {
        self.counts
            .get(&(map.clone(), agent.clone()))
            .copied()
            .unwrap_or(0)
    }

    pub fn map_total(&self, map: &MapId) -> usize {
        self.agents.iter().map(|a| self.count(map, &a.id)).sum()
    }

    /// Maps that have lineups, by name.
    pub fn maps_by_name(&self) -> Vec<&Map> {
        let mut maps: Vec<_> = self
            .maps
            .iter()
            .filter(|m| self.map_total(&m.id) > 0)
            .collect();
        maps.sort_by(|a, b| a.name.cmp(&b.name));
        maps
    }

    /// Agents of one role, by name.
    pub fn roster(&self, role: Role) -> Vec<&Agent> {
        let mut agents: Vec<_> = self.agents.iter().filter(|a| a.role == role).collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        agents
    }

    /// The slot order of abilities, for sorting ability ids.
    pub fn slot(&self, id: AbilityId) -> u8 {
        self.ability(id).map_or(0, |ab| ab.slot)
    }

    pub fn clip(&self, id: LineupId) -> PathBuf {
        self.root.join("lineups").join(format!("{id}.mp4"))
    }

    /// The clip's first frame: the view from the stand spot.
    pub fn start_still(&self, id: LineupId) -> PathBuf {
        self.root.join("lineups").join(format!("{id}-start.webp"))
    }

    /// The clip's last frame: where the utility landed.
    pub fn end_still(&self, id: LineupId) -> PathBuf {
        self.root.join("lineups").join(format!("{id}-end.webp"))
    }

    /// A screenshot of the crosshair at the moment of the throw.
    pub fn aim_image(&self, id: LineupId) -> PathBuf {
        self.root.join("lineups").join(format!("{id}-aim.webp"))
    }

    pub fn map_image(&self, id: &MapId, side: Side) -> PathBuf {
        let side = match side {
            Side::Attack => "attack",
            Side::Defense => "defense",
        };
        self.root.join("maps").join(format!("{id}-{side}.svg"))
    }

    pub fn map_card(&self, id: &MapId) -> PathBuf {
        self.root.join("maps").join(format!("{id}.webp"))
    }

    pub fn portrait(&self, id: &AgentId) -> PathBuf {
        self.root.join("agents").join(format!("{id}.webp"))
    }

    pub fn ability_icon(&self, id: AbilityId) -> PathBuf {
        self.root.join("abilities").join(format!("{id}.webp"))
    }
}

/// Each item's position by its key, refusing a key that appears twice.
fn index_unique<T, K: std::hash::Hash + Eq + std::fmt::Display>(
    items: &[T],
    kind: &str,
    key: impl Fn(&T) -> K,
) -> Result<HashMap<K, usize>, LoadError> {
    let mut index = HashMap::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let k = key(item);
        if index.contains_key(&k) {
            return Err(LoadError::Duplicate(format!("{kind} {k}")));
        }
        index.insert(k, i);
    }
    Ok(index)
}
