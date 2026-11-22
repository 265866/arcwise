//! Lineup libraries: each a `library.json` and the media files beside it, as described in the
//! arcwise-library repository's `docs/format.md`. Arcwise shows several at once, merged into one
//! [`Library`].

use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::{fs, io, mem};

use serde::Deserialize;
use thiserror::Error;

use crate::model::{
    Ability, AbilityId, Agent, AgentId, Lineup, LineupId, Map, MapId, Role, Side, Tag,
};

/// The `format` value this version reads.
pub const FORMAT: u32 = 1;

/// Why a library couldn't be loaded.
#[must_use]
#[derive(Debug, Error)]
pub enum LoadError {
    /// Its `library.json` couldn't be read.
    #[error("cannot read {path}")]
    Read {
        /// The file that couldn't be read.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// Its `library.json` isn't a library's JSON.
    #[error("{path} is not a lineup library")]
    Parse {
        /// The file that isn't a library.
        path: PathBuf,
        /// What the JSON reader found wrong.
        #[source]
        source: serde_json::Error,
    },
    /// It is written in a format this version doesn't read.
    #[error("library format {found} is not supported; this version reads format {FORMAT}")]
    Format {
        /// The format the library gives.
        found: u32,
    },
    /// A lineup refers to a map, agent or ability the library doesn't list.
    #[error("lineup {lineup} refers to {what}, which the library does not list")]
    Dangling {
        /// The lineup.
        lineup: LineupId,
        /// What it refers to, such as `map m-ascent`.
        what: String,
    },
    /// A map, agent, ability or lineup is listed twice.
    #[error("{0} is listed more than once")]
    Duplicate(String),
}

#[derive(Debug, Deserialize)]
struct File {
    format: u32,
    #[serde(default)]
    name: Option<String>,
    maps: Vec<Map>,
    agents: Vec<Agent>,
    lineups: Vec<Lineup>,
}

/// One library's `library.json`, checked, and the folder its media files are in.
#[derive(Debug)]
pub struct Part {
    root: PathBuf,
    file: File,
}

/// A map of one library and how many lineups the library has on it.
#[derive(Clone, Copy, Debug)]
pub struct MapLineups<'library> {
    /// The map.
    pub map: &'library Map,
    /// How many lineups the library has on it.
    pub lineups: usize,
}

impl Part {
    /// Checks a library on its own: its format, that nothing is listed twice, and that every
    /// lineup's map, agent and ability are listed. Its media files are under `root`.
    ///
    /// # Errors
    ///
    /// [`LoadError::Parse`] when `json` isn't a library, [`LoadError::Format`] for a format this
    /// version doesn't read, [`LoadError::Duplicate`] for a map, agent, ability or lineup listed
    /// twice, and [`LoadError::Dangling`] for a lineup whose map, agent or ability isn't listed.
    pub fn parse(root: &Path, json: &[u8]) -> Result<Part, LoadError> {
        let root = root.to_path_buf();
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

    /// The folder the library's media files are in.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// What the library calls itself, when it says.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.file.name.as_deref()
    }

    /// How many lineups the library lists.
    #[must_use]
    pub fn lineup_count(&self) -> usize {
        self.file.lineups.len()
    }

    /// The maps that have lineups, with how many each has.
    #[must_use]
    pub fn maps(&self) -> Vec<MapLineups<'_>> {
        self.file
            .maps
            .iter()
            .map(|map| MapLineups {
                map,
                lineups: self.file.lineups.iter().filter(|l| l.map == map.id).count(),
            })
            .filter(|counted| counted.lineups > 0)
            .collect()
    }

    /// The library's media files for the lineups on `maps` (every map when `None`), and its map,
    /// agent and ability pictures, as paths relative to its folder.
    #[must_use]
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

/// Where an ability is in the merged library: its agent, and its place among the agent's abilities.
#[derive(Clone, Copy, Debug)]
struct AbilityPlace {
    agent: usize,
    ability: usize,
}

/// One agent on one map, the key of the lineup counts.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct MapAgent {
    map: MapId,
    agent: AgentId,
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
    /// The library each map came from, by index into `roots`; `agent_root`, `ability_root` and
    /// `lineup_root` do the same for agents, abilities and lineups.
    map_root: Vec<usize>,
    agent_root: Vec<usize>,
    ability_root: HashMap<AbilityId, usize>,
    /// The id a renumbered ability has in its own library, which names its icon file.
    ability_files: HashMap<AbilityId, AbilityId>,
    lineup_root: Vec<usize>,
    map_index: HashMap<MapId, usize>,
    agent_index: HashMap<AgentId, usize>,
    ability_index: HashMap<AbilityId, AbilityPlace>,
    lineup_index: HashMap<LineupId, usize>,
    counts: HashMap<MapAgent, usize>,
    /// Per library, by index into `roots`: its lineups left out for an id already taken.
    skipped: Vec<usize>,
}

impl Library {
    /// Reads `library.json` from `root`.
    ///
    /// # Errors
    ///
    /// [`LoadError::Read`] when the file can't be read, and the errors of [`Part::parse`].
    pub fn open(root: &Path) -> Result<Self, LoadError> {
        let path = root.join("library.json");
        let bytes = fs::read(&path).map_err(|source| LoadError::Read { path, source })?;
        Self::from_json(root, &bytes)
    }

    /// Parses a library whose media lives under `root`.
    ///
    /// # Errors
    ///
    /// The errors of [`Part::parse`].
    pub fn from_json(root: &Path, json: &[u8]) -> Result<Self, LoadError> {
        Ok(Self::merge(vec![Part::parse(root, json)?]))
    }

    /// Merges libraries checked on their own, in the order given; see [`Library`] for what
    /// merging keeps.
    #[must_use]
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
                library.ability_index.insert(
                    ability.id,
                    AbilityPlace {
                        agent: a,
                        ability: b,
                    },
                );
            }
        }
        for lineup in &library.lineups {
            *library
                .counts
                .entry(MapAgent {
                    map: lineup.map.clone(),
                    agent: lineup.agent.clone(),
                })
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
        let mut merged_ability_ids = HashMap::new();
        for mut agent in file.agents {
            let abilities = mem::take(&mut agent.abilities);
            let a = if let Some(&a) = self.agent_index.get(&agent.id) {
                a
            } else {
                self.agent_index.insert(agent.id.clone(), self.agents.len());
                self.agents.push(agent);
                self.agent_root.push(source);
                self.agents.len() - 1
            };
            for ability in abilities {
                let same = self.agents[a]
                    .abilities
                    .iter()
                    .find(|known| known.name.eq_ignore_ascii_case(&ability.name))
                    .map(|known| known.id);
                let id = same.unwrap_or_else(|| {
                    let id = self.free_ability_id(ability.id);
                    self.ability_root.insert(id, source);
                    self.agents[a].abilities.push(Ability {
                        id,
                        ..ability.clone()
                    });
                    id
                });
                merged_ability_ids.insert(ability.id, id);
            }
        }
        for mut lineup in file.lineups {
            if self.lineup_index.contains_key(&lineup.id) {
                self.skipped[source] += 1;
                continue;
            }
            lineup.ability = merged_ability_ids[&lineup.ability];
            self.lineup_index.insert(lineup.id, self.lineups.len());
            self.lineups.push(lineup);
            self.lineup_root.push(source);
        }
    }

    /// The id a new ability takes in the merged library: its own, or the next free one when
    /// another library already uses its id for something else. A renumbered ability keeps its own
    /// id for its icon file.
    fn free_ability_id(&mut self, own: AbilityId) -> AbilityId {
        if !self.ability_root.contains_key(&own) {
            return own;
        }
        let id = AbilityId(self.ability_root.keys().map(|id| id.0).max().unwrap_or(0) + 1);
        self.ability_files.insert(id, own);
        id
    }

    /// Each merged library's folder, in order.
    #[must_use]
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Per library, in the order merged: how many of its lineups are left out because a library
    /// earlier in the list has the same id.
    #[must_use]
    pub fn skipped(&self) -> &[usize] {
        &self.skipped
    }

    /// Every map, in the order the libraries list them.
    #[must_use]
    pub fn maps(&self) -> &[Map] {
        &self.maps
    }

    /// Every agent, in the order the libraries list them, each one's abilities in slot order.
    #[must_use]
    pub fn agents(&self) -> &[Agent] {
        &self.agents
    }

    /// Every lineup, in the order the libraries list them.
    #[must_use]
    pub fn lineups(&self) -> &[Lineup] {
        &self.lineups
    }

    /// The map with this id.
    #[must_use]
    pub fn map(&self, id: &MapId) -> Option<&Map> {
        self.map_index.get(id).map(|&i| &self.maps[i])
    }

    /// The agent with this id.
    #[must_use]
    pub fn agent(&self, id: &AgentId) -> Option<&Agent> {
        self.agent_index.get(id).map(|&i| &self.agents[i])
    }

    /// The ability with this id, as numbered in the merged library.
    #[must_use]
    pub fn ability(&self, id: AbilityId) -> Option<&Ability> {
        self.ability_index
            .get(&id)
            .map(|place| &self.agents[place.agent].abilities[place.ability])
    }

    /// The lineup with this id.
    #[must_use]
    pub fn lineup(&self, id: LineupId) -> Option<&Lineup> {
        self.lineup_index.get(&id).map(|&i| &self.lineups[i])
    }

    /// The map with this name, ignoring ASCII case.
    #[must_use]
    pub fn map_by_name(&self, name: &str) -> Option<&Map> {
        self.maps.iter().find(|m| m.name.eq_ignore_ascii_case(name))
    }

    /// The agent with this name, ignoring ASCII case.
    #[must_use]
    pub fn agent_by_name(&self, name: &str) -> Option<&Agent> {
        self.agents
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
    }

    /// Lineups for one agent on one map.
    #[must_use]
    pub fn count(&self, map: &MapId, agent: &AgentId) -> usize {
        self.counts
            .get(&MapAgent {
                map: map.clone(),
                agent: agent.clone(),
            })
            .copied()
            .unwrap_or(0)
    }

    /// Lineups on one map, for every agent.
    #[must_use]
    pub fn map_total(&self, map: &MapId) -> usize {
        self.agents.iter().map(|a| self.count(map, &a.id)).sum()
    }

    /// Maps that have lineups, by name.
    #[must_use]
    pub fn maps_by_name(&self) -> Vec<&Map> {
        let mut maps: Vec<&Map> = self
            .maps
            .iter()
            .filter(|m| self.map_total(&m.id) > 0)
            .collect();
        maps.sort_by(|a, b| a.name.cmp(&b.name));
        maps
    }

    /// Agents of one role, by name.
    #[must_use]
    pub fn roster(&self, role: Role) -> Vec<&Agent> {
        let mut agents: Vec<&Agent> = self.agents.iter().filter(|a| a.role == role).collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));
        agents
    }

    /// The slot order of abilities, for sorting ability ids.
    #[must_use]
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

    fn root(&self, source: usize) -> &Path {
        self.roots.get(source).map_or(Path::new(""), |r| r)
    }

    /// The lineup's clip.
    #[must_use]
    pub fn clip(&self, id: LineupId) -> PathBuf {
        self.lineup_dir(id).join(format!("{id}.mp4"))
    }

    /// The clip's first frame: the view from the stand spot.
    #[must_use]
    pub fn start_still(&self, id: LineupId) -> PathBuf {
        self.lineup_dir(id).join(format!("{id}-start.webp"))
    }

    /// The clip's last frame: where the utility landed.
    #[must_use]
    pub fn end_still(&self, id: LineupId) -> PathBuf {
        self.lineup_dir(id).join(format!("{id}-end.webp"))
    }

    /// A screenshot of the crosshair at the moment of the throw.
    #[must_use]
    pub fn aim_image(&self, id: LineupId) -> PathBuf {
        self.lineup_dir(id).join(format!("{id}-aim.webp"))
    }

    fn map_dir(&self, id: &MapId) -> PathBuf {
        let source = self.map_index.get(id).map_or(0, |&i| self.map_root[i]);
        self.root(source).join("maps")
    }

    /// The map's picture for one side.
    #[must_use]
    pub fn map_image(&self, id: &MapId, side: Side) -> PathBuf {
        let side = match side {
            Side::Attack => "attack",
            Side::Defense => "defense",
        };
        self.map_dir(id).join(format!("{id}-{side}.svg"))
    }

    /// The map's card in the map chooser.
    #[must_use]
    pub fn map_card(&self, id: &MapId) -> PathBuf {
        self.map_dir(id).join(format!("{id}.webp"))
    }

    /// The agent's portrait.
    #[must_use]
    pub fn portrait(&self, id: &AgentId) -> PathBuf {
        let source = self.agent_index.get(id).map_or(0, |&i| self.agent_root[i]);
        self.root(source).join("agents").join(format!("{id}.webp"))
    }

    /// The ability's icon. A renumbered ability keeps its own library's file name.
    #[must_use]
    pub fn ability_icon(&self, id: AbilityId) -> PathBuf {
        let source = self.ability_root.get(&id).copied().unwrap_or(0);
        let file = self.ability_files.get(&id).copied().unwrap_or(id);
        self.root(source)
            .join("abilities")
            .join(format!("{file}.webp"))
    }
}

/// Each item's position by its key, refusing a key that appears twice.
fn index_unique<T, K: Hash + Eq + Display>(
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
