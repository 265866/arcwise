//! Landings: lineups of one ability that land in nearly the same place, shown as one marker.

use crate::model::{AbilityId, Lineup, LineupId, Point};

/// Landings closer than this, in percent of the map's width, share one marker.
pub const CLUSTER_RADIUS: f32 = 2.2;

/// Names a landing by its ability and its first lineup, so it keeps its identity while the
/// lineups around it are filtered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LandingKey {
    /// The ability every lineup of the landing throws.
    pub ability: AbilityId,
    /// The landing's first lineup.
    pub first: LineupId,
}

/// Lineups of one ability that land in nearly the same place, shown as one marker.
#[derive(Clone, Debug, PartialEq)]
pub struct Landing {
    /// What the landing is known by while the lineups around it are filtered.
    pub key: LandingKey,
    /// The mean landing point of its lineups.
    pub at: Point,
    /// In the order they were given; the stand markers are numbered in this order.
    pub lineups: Vec<LineupId>,
    /// The most common landing callout, ties going to the one seen first.
    pub name: Option<String>,
    /// Each stand callout and how many of the lineups throw from it, in first-seen order.
    pub stands: Vec<Tally<Option<String>>>,
}

/// One distinct item and how many times it was seen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tally<T> {
    /// The item.
    pub item: T,
    /// How many times it was seen.
    pub count: usize,
}

/// Groups lineups into landings. Each lineup joins the first landing of its ability whose first
/// lineup lands within [`CLUSTER_RADIUS`]; otherwise it starts a new one.
pub fn cluster<'a>(lineups: impl IntoIterator<Item = &'a Lineup>) -> Vec<Landing> {
    struct Group<'lineup> {
        anchor: Point,
        members: Vec<&'lineup Lineup>,
    }
    let mut groups: Vec<Group<'a>> = Vec::new();
    for lineup in lineups {
        let at = lineup.land.at();
        match groups.iter_mut().find(|g| {
            g.members[0].ability == lineup.ability && g.anchor.distance(at) < CLUSTER_RADIUS
        }) {
            Some(group) => group.members.push(lineup),
            None => groups.push(Group {
                anchor: at,
                members: vec![lineup],
            }),
        }
    }
    groups
        .into_iter()
        .map(|Group { members, .. }| {
            let first = members[0];
            Landing {
                key: LandingKey {
                    ability: first.ability,
                    first: first.id,
                },
                at: mean_landing(&members),
                lineups: members.iter().map(|l| l.id).collect(),
                name: first_most_common(members.iter().filter_map(|l| l.land.callout.clone()))
                    .map(|tally| tally.item),
                stands: tally(members.iter().map(|l| l.stand.callout.clone())),
            }
        })
        .collect()
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a landing groups a handful of lineups, far below the 2^24 where f32 loses whole numbers"
)]
fn mean_landing(members: &[&Lineup]) -> Point {
    let n = members.len() as f32;
    Point {
        x: members.iter().map(|l| l.land.x).sum::<f32>() / n,
        y: members.iter().map(|l| l.land.y).sum::<f32>() / n,
    }
}

/// Counts each distinct item, in the order each was first seen.
pub(crate) fn tally<T: PartialEq>(items: impl IntoIterator<Item = T>) -> Vec<Tally<T>> {
    let mut counts: Vec<Tally<T>> = Vec::new();
    for item in items {
        match counts.iter_mut().find(|seen| seen.item == item) {
            Some(seen) => seen.count += 1,
            None => counts.push(Tally { item, count: 1 }),
        }
    }
    counts
}

/// The most common item, the first seen of equal counts, which `max_by_key` would not give.
fn first_most_common<T: PartialEq>(items: impl IntoIterator<Item = T>) -> Option<Tally<T>> {
    tally(items)
        .into_iter()
        .fold(None, |best, next| match best {
            Some(seen) if seen.count >= next.count => Some(seen),
            _ => Some(next),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::{Library, MapId, Side};

    fn library() -> Library {
        Library::from_json(
            Path::new("lib"),
            include_bytes!("../tests/fixtures/library.json"),
        )
        .unwrap()
    }

    fn ascent_attack(library: &Library) -> Vec<Landing> {
        let ascent = MapId::new("m-ascent");
        cluster(
            library.lineups().iter().filter(|l| {
                l.map == ascent && l.side == Side::Attack && l.agent.as_str() == "a-sova"
            }),
        )
    }

    #[test]
    fn groups_nearby_landings_of_one_ability() {
        let library = library();
        let landings = ascent_attack(&library);
        let ids: Vec<Vec<u32>> = landings
            .iter()
            .map(|g| g.lineups.iter().map(|id| id.0).collect())
            .collect();
        assert_eq!(
            ids,
            [vec![1, 2, 9], vec![3], vec![4]],
            "1, 2 and 9 land on A site with Shock Bolt; 3 lands there too but with Recon Bolt"
        );
    }

    #[test]
    fn places_the_marker_at_the_mean_landing() {
        let library = library();
        let a_site = &ascent_attack(&library)[0];
        assert!(
            (a_site.at.x - 50.166_668).abs() < 1e-4,
            "x is {}",
            a_site.at.x
        );
        assert!(
            (a_site.at.y - 50.833_332).abs() < 1e-4,
            "y is {}",
            a_site.at.y
        );
        assert_eq!(
            a_site.key,
            LandingKey {
                ability: AbilityId(10),
                first: LineupId(1)
            },
            "the key names the first lineup"
        );
    }

    #[test]
    fn names_and_summarises_a_landing() {
        let library = library();
        let a_site = &ascent_attack(&library)[0];
        assert_eq!(a_site.name.as_deref(), Some("A Site"), "the landing's name");
        assert_eq!(
            a_site.stands,
            [
                Tally {
                    item: Some("A Lobby".to_owned()),
                    count: 1
                },
                Tally {
                    item: Some("A Main".to_owned()),
                    count: 2
                }
            ],
            "the stand callouts, in first-seen order"
        );
    }

    #[test]
    fn measures_distance_from_the_first_lineup_not_the_mean() {
        let library = library();
        let mut lineup = library.lineup(LineupId(1)).unwrap().clone();
        let mut far = lineup.clone();
        far.id = LineupId(100);
        far.land.x += CLUSTER_RADIUS - 0.1;
        let mut farther = lineup.clone();
        farther.id = LineupId(101);
        farther.land.x += 2.0 * CLUSTER_RADIUS - 0.2;
        lineup.land.callout = None;
        let landings = cluster([&lineup, &far, &farther]);
        assert_eq!(
            landings.len(),
            2,
            "farther is within reach of far, not of 1"
        );
        assert_eq!(
            landings[0].lineups,
            [LineupId(1), LineupId(100)],
            "far joins 1's landing"
        );
    }

    #[test]
    fn ties_go_to_the_first_callout() {
        assert_eq!(
            first_most_common(["b", "a", "a", "b"]),
            Some(Tally {
                item: "b",
                count: 2
            }),
            "b and a tie, and b came first"
        );
        assert_eq!(
            first_most_common(Vec::<&str>::new()),
            None,
            "nothing to count"
        );
    }
}
