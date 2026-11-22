//! What the user is looking at: the map, agent, side and ability filter, the selected landing and
//! lineup, and which chooser is open. Every change goes through a method that leaves the state
//! consistent with the library.

use crate::landing::{self, Landing, LandingKey, Tally};
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

/// A chooser, which opens as a centered dialog on the first run and as a dropdown after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chooser {
    /// The map chooser.
    Map,
    /// The agent chooser.
    Agent,
}

/// Where to start, from the command line.
#[derive(Clone, Debug, Default)]
pub struct Start {
    /// The map to show.
    pub map: Option<MapId>,
    /// The agent to show.
    pub agent: Option<AgentId>,
    /// The side to show.
    pub side: Option<Side>,
    /// The lineup to select, which sets the map, agent and side too.
    pub lineup: Option<LineupId>,
    /// The view of the selected lineup.
    pub view: Option<View>,
}

/// A way to step through a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Toward the start of the list.
    Previous,
    /// Toward the end of the list.
    Next,
}

impl Direction {
    /// Steps `from` one place in this direction through `len` items, wrapping at either end.
    /// From no place, Next goes to the first item and Previous to the last. Nowhere to go when
    /// there are no items.
    fn step(self, from: Option<usize>, len: usize) -> Option<usize> {
        let last = len.checked_sub(1)?;
        Some(match (self, from) {
            (Direction::Next, None) => 0,
            (Direction::Previous, None) => last,
            (Direction::Next, Some(i)) => {
                if i >= last {
                    0
                } else {
                    i + 1
                }
            }
            (Direction::Previous, Some(i)) => {
                if i == 0 || i > last {
                    last
                } else {
                    i - 1
                }
            }
        })
    }
}

/// What the user is looking at, kept consistent with the library.
#[derive(Clone, Debug)]
pub struct Browse {
    map: Option<MapId>,
    agent: Option<AgentId>,
    side: Side,
    ability: Option<AbilityId>,
    /// The selected lineup; the selected landing is the one that holds it.
    lineup: Option<LineupId>,
    view: View,
    chooser: Option<Chooser>,
    /// The first choice is made in a centered dialog; after that the choosers open as dropdowns.
    dropdown: bool,
    landings: Vec<Landing>,
}

impl Browse {
    /// Starts where `start` says, keeping only what the library has. Unless the chosen agent has
    /// lineups on the chosen map, opens a chooser: the agent chooser when a map is chosen, and
    /// the map chooser otherwise.
    #[must_use]
    pub fn new(library: &Library, start: Start) -> Self {
        let mut browse = Browse {
            map: start.map.filter(|id| library.map(id).is_some()),
            agent: start.agent.filter(|id| library.agent(id).is_some()),
            side: start.side.unwrap_or(Side::Attack),
            ability: None,
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

    /// The chosen map.
    #[must_use]
    pub fn map(&self) -> Option<&MapId> {
        self.map.as_ref()
    }

    /// The chosen agent.
    #[must_use]
    pub fn agent(&self) -> Option<&AgentId> {
        self.agent.as_ref()
    }

    /// The side shown.
    #[must_use]
    pub fn side(&self) -> Side {
        self.side
    }

    /// The one ability shown, or `None` for every ability.
    #[must_use]
    pub fn ability(&self) -> Option<AbilityId> {
        self.ability
    }

    /// The selected lineup's view.
    #[must_use]
    pub fn view(&self) -> View {
        self.view
    }

    /// The chooser that is open.
    #[must_use]
    pub fn chooser(&self) -> Option<Chooser> {
        self.chooser
    }

    /// Whether the choosers open as dropdowns, which they do once the first choice is made.
    #[must_use]
    pub fn dropdown(&self) -> bool {
        self.dropdown
    }

    /// The landings shown on the map.
    #[must_use]
    pub fn landings(&self) -> &[Landing] {
        &self.landings
    }

    /// The landing shown on the map with this key.
    #[must_use]
    pub fn landing(&self, key: LandingKey) -> Option<&Landing> {
        self.landings.iter().find(|g| g.key == key)
    }

    /// The landing that holds the selected lineup.
    #[must_use]
    pub fn selected_landing(&self) -> Option<&Landing> {
        let id = self.lineup?;
        self.landings.iter().find(|g| g.lineups.contains(&id))
    }

    /// The selected lineup.
    #[must_use]
    pub fn selected_lineup(&self) -> Option<LineupId> {
        self.lineup
    }

    /// Lineups for the chosen agent on the chosen map.
    pub fn for_agent<'a>(
        &self,
        library: &'a Library,
    ) -> impl Iterator<Item = &'a Lineup> + use<'a> {
        let map = self.map.clone();
        let agent = self.agent.clone();
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
    #[must_use]
    pub fn side_count(&self, library: &Library, side: Side) -> usize {
        self.for_agent(library).filter(|l| l.side == side).count()
    }

    /// The abilities with lineups on the chosen side, in slot order, with their counts.
    #[must_use]
    pub fn abilities(&self, library: &Library) -> Vec<Tally<AbilityId>> {
        let mut counts = landing::tally(self.for_side(library).map(|l| l.ability));
        counts.sort_by_key(|counted| library.slot(counted.item));
        counts
    }

    /// Whether the chooser may close: the map has lineups for the chosen agent.
    #[must_use]
    pub fn can_close(&self, library: &Library) -> bool {
        match (&self.map, &self.agent) {
            (Some(map), Some(agent)) => library.count(map, agent) > 0,
            _ => false,
        }
    }

    /// Shows `map`, keeping the chosen agent. Closes the chooser when that agent has lineups
    /// there, and otherwise opens the agent chooser.
    pub fn choose_map(&mut self, library: &Library, map: MapId) {
        if self.map.as_ref() != Some(&map) {
            self.ability = None;
            self.clear_selection();
        }
        self.map = Some(map);
        if self.can_close(library) {
            self.chooser = None;
            self.dropdown = true;
        } else {
            self.chooser = Some(Chooser::Agent);
        }
        self.settle(library);
    }

    /// Shows `agent`, keeping the agent chooser open when the agent has no lineups on the map.
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

    /// Shows `side`, unless the agent has no lineups on it.
    pub fn set_side(&mut self, library: &Library, side: Side) {
        if self.side_count(library, side) == 0 {
            return;
        }
        self.side = side;
        self.clear_selection();
        self.settle(library);
    }

    /// Shows the other side, unless the agent has no lineups on it.
    pub fn toggle_side(&mut self, library: &Library) {
        self.set_side(library, self.side.other());
    }

    /// Shows one ability, or every ability with `None`.
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
            .map(|counted| counted.item)
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

    /// Selects a landing and its first stand spot.
    pub fn select_landing(&mut self, key: LandingKey) {
        if let Some(landing) = self.landing(key) {
            self.lineup = Some(landing.lineups[0]);
        }
    }

    /// Selects another stand spot of the selected landing.
    pub fn select_lineup(&mut self, id: LineupId) {
        if self
            .selected_landing()
            .is_some_and(|g| g.lineups.contains(&id))
        {
            self.lineup = Some(id);
        }
    }

    /// Selects the previous or next landing, wrapping at either end.
    pub fn step_landing(&mut self, direction: Direction) {
        let from = self
            .selected_landing()
            .and_then(|g| self.landings.iter().position(|l| l.key == g.key));
        if let Some(to) = direction.step(from, self.landings.len()) {
            self.lineup = Some(self.landings[to].lineups[0]);
        }
    }

    /// Steps through the selected landing's stand spots.
    pub fn step_lineup(&mut self, direction: Direction) {
        let Some(landing) = self.selected_landing() else {
            return;
        };
        let from = landing
            .lineups
            .iter()
            .position(|&id| Some(id) == self.lineup);
        if let Some(to) = direction.step(from, landing.lineups.len()) {
            self.lineup = Some(landing.lineups[to]);
        }
    }

    /// Switches the selected lineup's view.
    pub fn set_view(&mut self, view: View) {
        self.view = view;
    }

    /// Deselects the lineup and goes back to the clip view.
    pub fn clear_selection(&mut self) {
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
        self.landings = landing::cluster(
            self.for_side(library)
                .filter(|l| ability.is_none_or(|a| l.ability == a)),
        );
        if self.selected_landing().is_none() {
            self.lineup = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Direction;

    #[test]
    fn steps_wrap_at_either_end() {
        assert_eq!(Direction::Next.step(None, 3), Some(0), "next from nowhere");
        assert_eq!(
            Direction::Previous.step(None, 3),
            Some(2),
            "previous from nowhere"
        );
        assert_eq!(
            Direction::Next.step(Some(2), 3),
            Some(0),
            "next from the last"
        );
        assert_eq!(
            Direction::Previous.step(Some(0), 3),
            Some(2),
            "previous from the first"
        );
        assert_eq!(
            Direction::Next.step(Some(1), 3),
            Some(2),
            "next from the middle"
        );
    }

    #[test]
    fn there_is_nowhere_to_step_in_nothing() {
        assert_eq!(Direction::Next.step(None, 0), None, "next in nothing");
        assert_eq!(
            Direction::Previous.step(Some(4), 0),
            None,
            "previous in nothing"
        );
    }
}
