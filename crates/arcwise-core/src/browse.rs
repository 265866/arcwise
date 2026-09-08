//! What the user is looking at: the map, agent, side and ability filter, the selected landing and
//! lineup, and which chooser is open. Every change goes through a method that leaves the state
//! consistent with the library.

use crate::landing::{Landing, LandingKey, cluster};
use crate::library::Library;
use crate::model::{AbilityId, AgentId, Lineup, LineupId, MapId, Side};

/// What the lineup viewer shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum View {
    /// The full clip.
    #[default]
    Clip,
    /// The map around the stand spot.
    Stand,
    /// The crosshair at the throw.
    Aim,
    /// Where it lands.
    Lands,
}

impl View {
    pub const ALL: [View; 4] = [View::Clip, View::Stand, View::Aim, View::Lands];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chooser {
    Map,
    Agent,
}

/// Where to start, from the command line.
#[derive(Clone, Debug, Default)]
pub struct Start {
    pub map: Option<MapId>,
    pub agent: Option<AgentId>,
    pub side: Option<Side>,
    pub lineup: Option<LineupId>,
    pub view: Option<View>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Previous,
    Next,
}

impl Direction {
    /// Steps `from` one place in this direction through `len` items, wrapping at either end.
    /// From no place, Next goes to the first item and Previous to the last.
    fn step(self, from: Option<usize>, len: usize) -> usize {
        match (self, from) {
            (Direction::Next, None) => 0,
            (Direction::Previous, None) => len - 1,
            (Direction::Next, Some(i)) => (i + 1) % len,
            (Direction::Previous, Some(i)) => (i + len - 1) % len,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Browse {
    map: Option<MapId>,
    agent: Option<AgentId>,
    side: Side,
    ability: Option<AbilityId>,
    landing: Option<LandingKey>,
    lineup: Option<LineupId>,
    view: View,
    chooser: Option<Chooser>,
    /// The first choice is made in a centered dialog; after that the choosers open as dropdowns.
    dropdown: bool,
    landings: Vec<Landing>,
}

impl Browse {
    pub fn new(library: &Library, start: Start) -> Self {
        let mut browse = Browse {
            map: start.map.filter(|id| library.map(id).is_some()),
            agent: start.agent.filter(|id| library.agent(id).is_some()),
            side: start.side.unwrap_or(Side::Attack),
            ability: None,
            landing: None,
            lineup: None,
            view: start.view.unwrap_or_default(),
            chooser: None,
            dropdown: false,
            landings: Vec::new(),
        };
        if let Some(lineup) = start.lineup.and_then(|id| library.lineup(id)) {
            browse.map = Some(lineup.map.clone());
            browse.agent = Some(lineup.agent.clone());
            browse.side = lineup.side;
            browse.lineup = Some(lineup.id);
        }
        if browse.can_close(library) {
            browse.dropdown = true;
        } else {
            browse.chooser = Some(if browse.map.is_some() {
                Chooser::Agent
            } else {
                Chooser::Map
            });
        }
        browse.settle(library);
        browse
    }

    pub fn map(&self) -> Option<&MapId> {
        self.map.as_ref()
    }

    pub fn agent(&self) -> Option<&AgentId> {
        self.agent.as_ref()
    }

    pub fn side(&self) -> Side {
        self.side
    }

    pub fn ability(&self) -> Option<AbilityId> {
        self.ability
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn chooser(&self) -> Option<Chooser> {
        self.chooser
    }

    pub fn dropdown(&self) -> bool {
        self.dropdown
    }

    /// The landings shown on the map.
    pub fn landings(&self) -> &[Landing] {
        &self.landings
    }

    pub fn landing(&self, key: LandingKey) -> Option<&Landing> {
        self.landings.iter().find(|g| g.key == key)
    }

    pub fn selected_landing(&self) -> Option<&Landing> {
        self.landing.and_then(|key| self.landing(key))
    }

    pub fn selected_lineup(&self) -> Option<LineupId> {
        self.lineup
    }

    /// Lineups for the chosen agent on the chosen map.
    pub fn for_agent<'a>(
        &self,
        library: &'a Library,
    ) -> impl Iterator<Item = &'a Lineup> + use<'a> {
        let (map, agent) = (self.map.clone(), self.agent.clone());
        library
            .lineups()
            .iter()
            .filter(move |l| map.as_ref() == Some(&l.map) && agent.as_ref() == Some(&l.agent))
    }

    fn for_side<'a>(&self, library: &'a Library) -> impl Iterator<Item = &'a Lineup> + use<'a> {
        let side = self.side;
        self.for_agent(library).filter(move |l| l.side == side)
    }

    /// How many lineups the agent has on each side of the chosen map.
    pub fn side_count(&self, library: &Library, side: Side) -> usize {
        self.for_agent(library).filter(|l| l.side == side).count()
    }

    /// The abilities with lineups on the chosen side, in slot order, with their counts.
    pub fn abilities(&self, library: &Library) -> Vec<(AbilityId, usize)> {
        let mut counts: Vec<(AbilityId, usize)> = Vec::new();
        for lineup in self.for_side(library) {
            match counts.iter_mut().find(|(id, _)| *id == lineup.ability) {
                Some((_, n)) => *n += 1,
                None => counts.push((lineup.ability, 1)),
            }
        }
        counts.sort_by_key(|&(id, _)| library.slot(id));
        counts
    }

    /// Whether the chooser may close: the map has lineups for the chosen agent.
    pub fn can_close(&self, library: &Library) -> bool {
        match (&self.map, &self.agent) {
            (Some(map), Some(agent)) => library.count(map, agent) > 0,
            _ => false,
        }
    }

    pub fn choose_map(&mut self, library: &Library, map: MapId) {
        if self.map.as_ref() != Some(&map) {
            self.ability = None;
            self.clear_selection();
        }
        self.map = Some(map);
        // Keep the agent when they have lineups here; otherwise ask for one.
        if self.can_close(library) {
            self.chooser = None;
            self.dropdown = true;
        } else {
            self.chooser = Some(Chooser::Agent);
        }
        self.settle(library);
    }

    pub fn choose_agent(&mut self, library: &Library, agent: AgentId) {
        self.agent = Some(agent);
        self.ability = None;
        self.clear_selection();
        self.dropdown = true;
        self.chooser = (!self.can_close(library)).then_some(Chooser::Agent);
        self.settle(library);
    }

    /// Opens a chooser, or closes it when it is already open. Asking for the agent before a map
    /// has been chosen opens the map chooser.
    pub fn toggle_chooser(&mut self, library: &Library, which: Chooser) {
        let which = if self.map.is_none() {
            Chooser::Map
        } else {
            which
        };
        self.chooser = if self.chooser == Some(which) && self.can_close(library) {
            None
        } else {
            Some(which)
        };
    }

    /// Goes back from the agent step of the first-run dialog to the map step.
    pub fn back_to_maps(&mut self) {
        self.chooser = Some(Chooser::Map);
    }

    /// Closes the chooser if there is something to show behind it.
    pub fn dismiss_chooser(&mut self, library: &Library) {
        if self.can_close(library) {
            self.chooser = None;
        }
    }

    pub fn set_side(&mut self, library: &Library, side: Side) {
        if self.side_count(library, side) == 0 {
            return;
        }
        self.side = side;
        self.clear_selection();
        self.settle(library);
    }

    pub fn toggle_side(&mut self, library: &Library) {
        self.set_side(library, self.side.other());
    }

    pub fn set_ability(&mut self, library: &Library, ability: Option<AbilityId>) {
        self.ability = ability;
        self.clear_selection();
        self.settle(library);
    }

    /// The number keys: 1 shows every ability, 2 and up pick one in slot order. Does nothing
    /// while there is only one ability to show.
    pub fn ability_key(&mut self, library: &Library, n: usize) {
        let ids: Vec<AbilityId> = self
            .abilities(library)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        if ids.len() < 2 {
            return;
        }
        let next = match n {
            1 => None,
            n if n >= 2 && n - 2 < ids.len() => Some(ids[n - 2]),
            _ => return,
        };
        if next != self.ability {
            self.set_ability(library, next);
        }
    }

    pub fn select_landing(&mut self, library: &Library, key: LandingKey) {
        self.landing = Some(key);
        self.lineup = None;
        self.settle(library);
    }

    /// Selects a lineup of the selected landing.
    pub fn select_lineup(&mut self, library: &Library, id: LineupId) {
        self.lineup = Some(id);
        self.settle(library);
    }

    pub fn step_landing(&mut self, library: &Library, direction: Direction) {
        if self.landings.is_empty() {
            return;
        }
        let from = self
            .landing
            .and_then(|key| self.landings.iter().position(|g| g.key == key));
        let to = direction.step(from, self.landings.len());
        self.select_landing(library, self.landings[to].key);
    }

    /// Steps through the selected landing's stand spots.
    pub fn step_lineup(&mut self, library: &Library, direction: Direction) {
        let Some(landing) = self.selected_landing() else {
            return;
        };
        let from = landing
            .lineups
            .iter()
            .position(|&id| Some(id) == self.lineup);
        let to = landing.lineups[direction.step(from, landing.lineups.len())];
        self.select_lineup(library, to);
    }

    pub fn set_view(&mut self, view: View) {
        self.view = view;
    }

    pub fn clear_selection(&mut self) {
        self.landing = None;
        self.lineup = None;
        self.view = View::Clip;
    }

    /// Makes the side, ability filter and selection valid again after any change.
    fn settle(&mut self, library: &Library) {
        if !self.can_close(library) {
            self.landings.clear();
            return;
        }
        if self.side_count(library, self.side) == 0 {
            self.side = self.side.other();
        }
        if let Some(ability) = self.ability
            && !self.for_side(library).any(|l| l.ability == ability)
        {
            self.ability = None;
        }
        let ability = self.ability;
        self.landings = cluster(
            self.for_side(library)
                .filter(|l| ability.is_none_or(|a| l.ability == a)),
        );
        let landing = self
            .landing
            .and_then(|key| self.landings.iter().find(|g| g.key == key))
            .or_else(|| {
                let lineup = self.lineup?;
                self.landings.iter().find(|g| g.lineups.contains(&lineup))
            });
        match landing {
            None => {
                self.landing = None;
                self.lineup = None;
            }
            Some(landing) => {
                self.landing = Some(landing.key);
                if !self.lineup.is_some_and(|id| landing.lineups.contains(&id)) {
                    self.lineup = Some(landing.lineups[0]);
                }
            }
        }
    }
}
