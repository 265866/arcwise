//! Lineup libraries: each a `library.json` and the media files beside it, as described in the
//! arcwise-library repository's `docs/format.md`. Arcwise shows several at once, merged into one
//! [`Library`].

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

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
    #[serde(default)]
    name: Option<String>,
    maps: Vec<Map>,
    agents: Vec<Agent>,
    lineups: Vec<Lineup>,
}

/// One library's `library.json`, checked, and the folder its media files are in.
pub struct Part {
    root: PathBuf,
    file: File,
}

impl Part {
    /// Checks a library on its own: its format, that nothing is listed twice, and that every
    /// lineup's map, agent and ability are listed.
    pub fn parse(root: impl Into<PathBuf>, json: &[u8]) -> Result<Part, LoadError> {
        let root = root.into();
        let mut file: File = serde_json::from_slice(json).map_err(|source| LoadError::Parse {
            path: root.join("library.json"),
            source,
        })?;
        if file.format != FORMAT {
            return Err(LoadError::Format { found: file.format });
        }
        index_unique(&file.maps, "map", |m| m.id.clone())?;
        let agents = index_unique(&file.agents, "agent", |a| a.id.clone())?;
        let mut owners = HashMap::new();
        for agent in &file.agents {
            for ability in &agent.abilities {
                if owners.insert(ability.id, agent.id.clone()).is_some() {
                    return Err(LoadError::Duplicate(format!("ability {}", ability.id)));
                }
            }
        }
        index_unique(&file.lineups, "lineup", |l| l.id)?;
        let maps: HashSet<MapId> = file.maps.iter().map(|m| m.id.clone()).collect();
        for lineup in &mut file.lineups {
            lineup.tags.retain(|t| *t != Tag::Other);
            let dangling = |what: String| LoadError::Dangling {
                lineup: lineup.id,
                what,
            };
            if !maps.contains(&lineup.map) {
                return Err(dangling(format!("map {}", lineup.map)));
            }
            let Some(&agent) = agents.get(&lineup.agent) else {
                return Err(dangling(format!("agent {}", lineup.agent)));
            };
            if owners.get(&lineup.ability) != Some(&lineup.agent) {
                return Err(dangling(format!(
                    "ability {} of {}",
                    lineup.ability, file.agents[agent].name
                )));
            }
        }
        Ok(Part { root, file })
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// What the library calls itself, when it says.
    pub fn name(&self) -> Option<&str> {
        self.file.name.as_deref()
    }

    pub fn lineup_count(&self) -> usize {
        self.file.lineups.len()
    }

    /// The maps that have lineups, with how many each has.
    pub fn maps(&self) -> Vec<(&Map, usize)> {
        self.file
            .maps
            .iter()
            .map(|m| {
                (
                    m,
                    self.file.lineups.iter().filter(|l| l.map == m.id).count(),
                )
            })
            .filter(|(_, n)| *n > 0)
            .collect()
    }

    /// The library's media files for the lineups on `maps` (every map when `None`), and its map,
    /// agent and ability pictures, as paths relative to its folder.
    pub fn files(&self, maps: Option<&[MapId]>) -> Vec<String> {
        let mut files = Vec::new();
        for map in &self.file.maps {
            files.push(format!("maps/{}.webp", map.id));
            files.push(format!("maps/{}-attack.svg", map.id));
            files.push(format!("maps/{}-defense.svg", map.id));
        }
        for agent in &self.file.agents {
            files.push(format!("agents/{}.webp", agent.id));
            for ability in &agent.abilities {
                files.push(format!("abilities/{}.webp", ability.id));
            }
        }
        for lineup in &self.file.lineups {
            if maps.is_some_and(|maps| !maps.contains(&lineup.map)) {
                continue;
            }
            for suffix in [".mp4", "-start.webp", "-end.webp", "-aim.webp"] {
                files.push(format!("lineups/{}{suffix}", lineup.id));
            }
        }
        files
    }
}

/// The lineups of every library shown, merged: a map or an agent listed by several is shown once,
/// and so is an ability of the same agent with the same name. The first library to list a lineup
/// id gives the lineup; the same id later is skipped.
#[derive(Debug)]
pub struct Library {
    /// Each library's folder, in the order they were merged.
    roots: Vec<PathBuf>,
    maps: Vec<Map>,
    agents: Vec<Agent>,
    lineups: Vec<Lineup>,
    /// The library each map, agent, ability and lineup came from, by index into `roots`.
    map_root: Vec<usize>,
    agent_root: Vec<usize>,
    ability_root: HashMap<AbilityId, usize>,
    /// The id a renumbered ability has in its own library, which names its icon file.
    ability_files: HashMap<AbilityId, AbilityId>,
    lineup_root: Vec<usize>,
    map_index: HashMap<MapId, usize>,
    agent_index: HashMap<AgentId, usize>,
    ability_index: HashMap<AbilityId, (usize, usize)>,
    lineup_index: HashMap<LineupId, usize>,
    counts: HashMap<(MapId, AgentId), usize>,
    /// Per library, by index into `roots`: its lineups left out for an id already taken.
    skipped: Vec<usize>,
}

impl Library {
    /// Reads `library.json` from `root`.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, LoadError> {
        let root = root.into();
        let path = root.join("library.json");
        let bytes = std::fs::read(&path).map_err(|source| LoadError::Read { path, source })?;
        Self::from_json(root, &bytes)
    }

    /// Parses a library whose media lives under `root`.
    pub fn from_json(root: impl Into<PathBuf>, json: &[u8]) -> Result<Self, LoadError> {
        Ok(Self::merge(vec![Part::parse(root, json)?]))
    }

    pub fn merge(parts: Vec<Part>) -> Self {
        let mut library = Library {
            roots: Vec::with_capacity(parts.len()),
            maps: Vec::new(),
            agents: Vec::new(),
            lineups: Vec::new(),
            map_root: Vec::new(),
            agent_root: Vec::new(),
            ability_root: HashMap::new(),
            ability_files: HashMap::new(),
            lineup_root: Vec::new(),
            map_index: HashMap::new(),
            agent_index: HashMap::new(),
            ability_index: HashMap::new(),
            lineup_index: HashMap::new(),
            counts: HashMap::new(),
            skipped: Vec::with_capacity(parts.len()),
        };
        for (source, part) in parts.into_iter().enumerate() {
            library.roots.push(part.root);
            library.skipped.push(0);
            library.add(source, part.file);
        }
        for agent in &mut library.agents {
            agent.abilities.sort_by_key(|ab| ab.slot);
        }
        for (a, agent) in library.agents.iter().enumerate() {
            for (b, ability) in agent.abilities.iter().enumerate() {
                library.ability_index.insert(ability.id, (a, b));
            }
        }
        for lineup in &library.lineups {
            *library
                .counts
                .entry((lineup.map.clone(), lineup.agent.clone()))
                .or_default() += 1;
        }
        library
    }

    fn add(&mut self, source: usize, file: File) {
        for map in file.maps {
            if !self.map_index.contains_key(&map.id) {
                self.map_index.insert(map.id.clone(), self.maps.len());
                self.maps.push(map);
                self.map_root.push(source);
            }
        }
        // What this library's ability ids are in the merged library.
        let mut ability_ids = HashMap::new();
        for mut agent in file.agents {
            let abilities = std::mem::take(&mut agent.abilities);
            let a = match self.agent_index.get(&agent.id) {
                Some(&a) => a,
                None => {
                    self.agent_index.insert(agent.id.clone(), self.agents.len());
                    self.agents.push(agent);
                    self.agent_root.push(source);
                    self.agents.len() - 1
                }
            };
            for ability in abilities {
                let same = self.agents[a]
                    .abilities
                    .iter()
                    .find(|known| known.name.eq_ignore_ascii_case(&ability.name))
                    .map(|known| known.id);
                let id = same.unwrap_or_else(|| {
                    // An id another library already uses for something else gets the next free one.
                    let id = if self.ability_root.contains_key(&ability.id) {
                        let id = AbilityId(
                            self.ability_root.keys().map(|id| id.0).max().unwrap_or(0) + 1,
                        );
                        self.ability_files.insert(id, ability.id);
                        id
                    } else {
                        ability.id
                    };
                    self.ability_root.insert(id, source);
                    self.agents[a].abilities.push(Ability {
                        id,
                        ..ability.clone()
                    });
                    id
                });
                ability_ids.insert(ability.id, id);
            }
        }
        for mut lineup in file.lineups {
            if self.lineup_index.contains_key(&lineup.id) {
                self.skipped[source] += 1;
                continue;
            }
            lineup.ability = ability_ids[&lineup.ability];
            self.lineup_index.insert(lineup.id, self.lineups.len());
            self.lineups.push(lineup);
            self.lineup_root.push(source);
        }
    }

    /// Each merged library's folder, in order.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Per library, in the order merged: how many of its lineups are left out because a library
    /// earlier in the list has the same id.
    pub fn skipped(&self) -> &[usize] {
        &self.skipped
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

    /// The folder of the library a lineup came from; the first library's for one it does not have.
    fn lineup_dir(&self, id: LineupId) -> PathBuf {
        let source = self
            .lineup_index
            .get(&id)
            .map_or(0, |&i| self.lineup_root[i]);
        self.root(source).join("lineups")
    }

    fn root(&self, source: usize) -> &std::path::Path {
        self.roots
            .get(source)
            .map_or(std::path::Path::new(""), |r| r)
    }

    pub fn clip(&self, id: LineupId) -> PathBuf {
        self.lineup_dir(id).join(format!("{id}.mp4"))
    }

    /// The clip's first frame: the view from the stand spot.
    pub fn start_still(&self, id: LineupId) -> PathBuf {
        self.lineup_dir(id).join(format!("{id}-start.webp"))
    }

    /// The clip's last frame: where the utility landed.
    pub fn end_still(&self, id: LineupId) -> PathBuf {
        self.lineup_dir(id).join(format!("{id}-end.webp"))
    }

    /// A screenshot of the crosshair at the moment of the throw.
    pub fn aim_image(&self, id: LineupId) -> PathBuf {
        self.lineup_dir(id).join(format!("{id}-aim.webp"))
    }

    fn map_dir(&self, id: &MapId) -> PathBuf {
        let source = self.map_index.get(id).map_or(0, |&i| self.map_root[i]);
        self.root(source).join("maps")
    }

    pub fn map_image(&self, id: &MapId, side: Side) -> PathBuf {
        let side = match side {
            Side::Attack => "attack",
            Side::Defense => "defense",
        };
        self.map_dir(id).join(format!("{id}-{side}.svg"))
    }

    pub fn map_card(&self, id: &MapId) -> PathBuf {
        self.map_dir(id).join(format!("{id}.webp"))
    }

    pub fn portrait(&self, id: &AgentId) -> PathBuf {
        let source = self.agent_index.get(id).map_or(0, |&i| self.agent_root[i]);
        self.root(source).join("agents").join(format!("{id}.webp"))
    }

    pub fn ability_icon(&self, id: AbilityId) -> PathBuf {
        let source = self.ability_root.get(&id).copied().unwrap_or(0);
        // A renumbered ability keeps its library's own file name.
        let file = self.ability_files.get(&id).copied().unwrap_or(id);
        self.root(source)
            .join("abilities")
            .join(format!("{file}.webp"))
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
