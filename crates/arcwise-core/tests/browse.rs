use arcwise_core::{
    AbilityId, AgentId, Browse, Chooser, Direction, Library, LineupId, MapId, Side, Start, View,
};

fn library() -> Library {
    Library::from_json("lib", include_bytes!("fixtures/library.json")).unwrap()
}

fn ascent() -> MapId {
    MapId::new("m-ascent")
}

fn sova() -> AgentId {
    AgentId::new("a-sova")
}

/// Sova on Ascent, as if both were picked in the first-run dialog.
fn sova_on_ascent(library: &Library) -> Browse {
    let mut browse = Browse::new(library, Start::default());
    browse.choose_map(library, ascent());
    browse.choose_agent(library, sova());
    browse
}

fn landing_ids(browse: &Browse) -> Vec<Vec<u32>> {
    browse
        .landings()
        .iter()
        .map(|g| g.lineups.iter().map(|id| id.0).collect())
        .collect()
}

#[test]
fn first_run_asks_for_a_map_then_an_agent() {
    let library = library();
    let mut browse = Browse::new(&library, Start::default());
    assert_eq!(browse.chooser(), Some(Chooser::Map));
    assert!(!browse.dropdown());
    browse.dismiss_chooser(&library);
    assert_eq!(
        browse.chooser(),
        Some(Chooser::Map),
        "nothing to show behind it yet"
    );

    browse.choose_map(&library, ascent());
    assert_eq!(browse.chooser(), Some(Chooser::Agent));
    assert!(browse.landings().is_empty());

    browse.choose_agent(&library, sova());
    assert_eq!(browse.chooser(), None);
    assert!(browse.dropdown());
    assert_eq!(landing_ids(&browse), [vec![1, 2, 9], vec![3], vec![4]]);
}

#[test]
fn asking_for_an_agent_before_a_map_opens_the_maps() {
    let library = library();
    let mut browse = Browse::new(&library, Start::default());
    browse.toggle_chooser(&library, Chooser::Agent);
    assert_eq!(browse.chooser(), Some(Chooser::Map));
}

#[test]
fn choosers_toggle_once_something_is_shown() {
    let library = library();
    let mut browse = sova_on_ascent(&library);
    browse.toggle_chooser(&library, Chooser::Agent);
    assert_eq!(browse.chooser(), Some(Chooser::Agent));
    browse.toggle_chooser(&library, Chooser::Map);
    assert_eq!(browse.chooser(), Some(Chooser::Map));
    browse.toggle_chooser(&library, Chooser::Map);
    assert_eq!(browse.chooser(), None);
}

#[test]
fn changing_to_a_map_without_the_agent_asks_for_another() {
    let library = library();
    let mut browse = Browse::new(&library, Start::default());
    browse.choose_map(&library, MapId::new("m-bind"));
    browse.choose_agent(&library, AgentId::new("a-jett"));
    assert_eq!(browse.chooser(), None);

    browse.choose_map(&library, ascent());
    assert_eq!(browse.chooser(), Some(Chooser::Agent));
    browse.dismiss_chooser(&library);
    assert_eq!(browse.chooser(), Some(Chooser::Agent));
}

#[test]
fn keeps_the_agent_on_a_map_where_they_have_lineups() {
    let library = library();
    let mut browse = sova_on_ascent(&library);
    browse.choose_map(&library, MapId::new("m-bind"));
    assert_eq!(browse.chooser(), None);
    assert_eq!(browse.agent(), Some(&sova()));
    assert_eq!(
        browse.side(),
        Side::Defense,
        "Sova has only defense lineups on Bind"
    );
}

#[test]
fn selecting_a_landing_selects_its_first_lineup() {
    let library = library();
    let mut browse = sova_on_ascent(&library);
    let key = browse.landings()[0].key;
    browse.select_landing(key);
    assert_eq!(browse.selected_lineup(), Some(LineupId(1)));
    browse.select_lineup(LineupId(9));
    assert_eq!(browse.selected_lineup(), Some(LineupId(9)));
    browse.select_lineup(LineupId(4));
    assert_eq!(
        browse.selected_landing().unwrap().lineups,
        [LineupId(1), LineupId(2), LineupId(9)],
        "a lineup outside the landing does not move the selection"
    );
    assert_eq!(
        browse.selected_lineup(),
        Some(LineupId(9)),
        "and keeps the stand spot"
    );
}

#[test]
fn landing_keys_step_and_wrap() {
    let library = library();
    let mut browse = sova_on_ascent(&library);
    browse.step_landing(Direction::Previous);
    assert_eq!(
        browse.selected_lineup(),
        Some(LineupId(4)),
        "up from nothing picks the last"
    );
    browse.step_landing(Direction::Next);
    assert_eq!(
        browse.selected_lineup(),
        Some(LineupId(1)),
        "and wraps to the first"
    );

    browse.clear_selection();
    browse.step_landing(Direction::Next);
    assert_eq!(
        browse.selected_lineup(),
        Some(LineupId(1)),
        "down from nothing picks the first"
    );
}

#[test]
fn lineup_keys_step_through_a_landing() {
    let library = library();
    let mut browse = sova_on_ascent(&library);
    browse.step_lineup(Direction::Next);
    assert_eq!(
        browse.selected_lineup(),
        None,
        "no landing, nothing to step through"
    );

    browse.step_landing(Direction::Next);
    browse.step_lineup(Direction::Next);
    assert_eq!(browse.selected_lineup(), Some(LineupId(2)));
    browse.step_lineup(Direction::Previous);
    browse.step_lineup(Direction::Previous);
    assert_eq!(browse.selected_lineup(), Some(LineupId(9)));
}

#[test]
fn the_ability_filter_narrows_the_landings() {
    let library = library();
    let mut browse = sova_on_ascent(&library);
    assert_eq!(
        browse.abilities(&library),
        [(AbilityId(10), 4), (AbilityId(11), 1)]
    );
    browse.ability_key(&library, 3);
    assert_eq!(browse.ability(), Some(AbilityId(11)));
    assert_eq!(landing_ids(&browse), [vec![3]]);
    browse.ability_key(&library, 9);
    assert_eq!(browse.ability(), Some(AbilityId(11)), "no ninth ability");
    browse.ability_key(&library, 1);
    assert_eq!(browse.ability(), None);
}

#[test]
fn the_ability_keys_do_nothing_with_one_ability() {
    let library = library();
    let mut browse = sova_on_ascent(&library);
    browse.set_side(&library, Side::Defense);
    browse.ability_key(&library, 2);
    assert_eq!(browse.ability(), None);
}

#[test]
fn changing_side_clears_the_selection_and_resets_a_missing_ability() {
    let library = library();
    let mut browse = sova_on_ascent(&library);
    browse.set_ability(&library, Some(AbilityId(10)));
    browse.step_landing(Direction::Next);
    browse.set_view(View::Aim);
    browse.toggle_side(&library);
    assert_eq!(browse.side(), Side::Defense);
    assert_eq!(browse.selected_lineup(), None);
    assert_eq!(browse.view(), View::Clip);
    assert_eq!(browse.ability(), None, "no Shock Bolt lineups on defense");
    assert_eq!(landing_ids(&browse), [vec![5]]);
}

#[test]
fn cannot_switch_to_a_side_without_lineups() {
    let library = library();
    let mut browse = Browse::new(&library, Start::default());
    browse.choose_map(&library, MapId::new("m-bind"));
    browse.choose_agent(&library, AgentId::new("a-jett"));
    browse.toggle_side(&library);
    assert_eq!(browse.side(), Side::Attack);
    assert_eq!(browse.side_count(&library, Side::Defense), 0);
}

#[test]
fn starts_from_a_lineup() {
    let library = library();
    let browse = Browse::new(
        &library,
        Start {
            lineup: Some(LineupId(5)),
            view: Some(View::Lands),
            ..Start::default()
        },
    );
    assert_eq!(browse.chooser(), None);
    assert_eq!(browse.map(), Some(&ascent()));
    assert_eq!(browse.side(), Side::Defense);
    assert_eq!(browse.selected_lineup(), Some(LineupId(5)));
    assert_eq!(browse.view(), View::Lands);
}

#[test]
fn starts_on_a_map_and_asks_for_the_agent() {
    let library = library();
    let browse = Browse::new(
        &library,
        Start {
            map: Some(ascent()),
            agent: Some(AgentId::new("a-jett")),
            ..Start::default()
        },
    );
    assert_eq!(
        browse.chooser(),
        Some(Chooser::Agent),
        "Jett has nothing on Ascent"
    );
}

#[test]
fn ignores_unknown_start_values() {
    let library = library();
    let browse = Browse::new(
        &library,
        Start {
            map: Some(MapId::new("m-nowhere")),
            lineup: Some(LineupId(404)),
            ..Start::default()
        },
    );
    assert_eq!(browse.map(), None);
    assert_eq!(browse.chooser(), Some(Chooser::Map));
}
