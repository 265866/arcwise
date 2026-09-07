//! Landings: lineups of one ability that land in nearly the same place, shown as one marker.

use crate::model::{AbilityId, Lineup, LineupId, Point};

/// Landings closer than this, in percent of the map's width, share one marker.
pub const CLUSTER_RADIUS: f32 = 2.2;

/// Names a landing by its ability and its first lineup, so it keeps its identity while the
/// lineups around it are filtered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LandingKey {
    pub ability: AbilityId,
    pub first: LineupId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Landing {
    pub key: LandingKey,
    /// The mean landing point of its lineups.
    pub at: Point,
    pub ability: AbilityId,
    /// In the order they were given; the stand markers are numbered in this order.
    pub lineups: Vec<LineupId>,
    /// The most common landing callout, ties going to the one seen first.
    pub name: Option<String>,
    /// Each stand callout and how many of the lineups throw from it, in first-seen order.
    pub stands: Vec<(Option<String>, usize)>,
}

/// Groups lineups into landings. Each lineup joins the first landing of its ability whose first
/// lineup lands within [`CLUSTER_RADIUS`]; otherwise it starts a new one.
pub fn cluster<'a>(lineups: impl IntoIterator<Item = &'a Lineup>) -> Vec<Landing> {
    struct Group<'a> {
        anchor: Point,
        members: Vec<&'a Lineup>,
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
            let n = members.len() as f32;
            let at = Point {
                x: members.iter().map(|l| l.land.x).sum::<f32>() / n,
                y: members.iter().map(|l| l.land.y).sum::<f32>() / n,
            };
            let first = members[0];
            Landing {
                key: LandingKey {
                    ability: first.ability,
                    first: first.id,
                },
                at,
                ability: first.ability,
                lineups: members.iter().map(|l| l.id).collect(),
                name: most_common(members.iter().filter_map(|l| l.land.callout.clone()))
                    .map(|(name, _)| name),
                stands: tally(members.iter().map(|l| l.stand.callout.clone())),
            }
        })
        .collect()
}

fn tally<T: PartialEq>(items: impl IntoIterator<Item = T>) -> Vec<(T, usize)> {
    let mut counts: Vec<(T, usize)> = Vec::new();
    for item in items {
        match counts.iter_mut().find(|(seen, _)| *seen == item) {
            Some((_, n)) => *n += 1,
            None => counts.push((item, 1)),
        }
    }
    counts
}

fn most_common<T: PartialEq>(items: impl IntoIterator<Item = T>) -> Option<(T, usize)> {
    // Keeps the first of equal counts: max_by_key would keep the last.
    tally(items)
        .into_iter()
        .fold(None, |best, (item, n)| match best {
            Some((_, m)) if m >= n => best,
            _ => Some((item, n)),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Library, MapId, Side};

    fn library() -> Library {
        Library::from_json("lib", include_bytes!("../tests/fixtures/library.json")).unwrap()
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
        // 1, 2 and 9 land on A site with Shock Bolt; 3 lands there too but with Recon Bolt.
        assert_eq!(ids, [vec![1, 2, 9], vec![3], vec![4]]);
    }

    #[test]
    fn places_the_marker_at_the_mean_landing() {
        let library = library();
        let a_site = &ascent_attack(&library)[0];
        assert!((a_site.at.x - 50.166_668).abs() < 1e-4);
        assert!((a_site.at.y - 50.833_332).abs() < 1e-4);
        assert_eq!(
            a_site.key,
            LandingKey {
                ability: AbilityId(10),
                first: LineupId(1)
            }
        );
    }

    #[test]
    fn names_and_summarises_a_landing() {
        let library = library();
        let a_site = &ascent_attack(&library)[0];
        assert_eq!(a_site.name.as_deref(), Some("A Site"));
        assert_eq!(
            a_site.stands,
            [
                (Some("A Lobby".to_string()), 1),
                (Some("A Main".to_string()), 2)
            ]
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
        assert_eq!(landings.len(), 2);
        assert_eq!(landings[0].lineups, [LineupId(1), LineupId(100)]);
    }

    #[test]
    fn ties_go_to_the_first_callout() {
        assert_eq!(most_common(["b", "a", "a", "b"]), Some(("b", 2)));
        assert_eq!(most_common(Vec::<&str>::new()), None);
    }
}
