use arcwise_core::{
    AbilityId, AgentId, Bounces, Charge, Library, LineupId, LoadError, MapId, Part, Role, Side, Tag,
};

const FIXTURE: &[u8] = include_bytes!("fixtures/library.json");

fn fixture() -> Library {
    Library::from_json("lib", FIXTURE).expect("fixture loads")
}

#[test]
fn loads_and_indexes_the_fixture() {
    let library = fixture();
    assert_eq!(library.maps().len(), 3);
    assert_eq!(library.lineups().len(), 9);
    let lineup = library.lineup(LineupId(2)).unwrap();
    assert_eq!(lineup.side, Side::Attack);
    assert_eq!(lineup.stand.callout.as_deref(), Some("A Main"));
    assert_eq!(library.ability(AbilityId(11)).unwrap().name, "Recon Bolt");
    assert_eq!(
        library.map_by_name("ascent").unwrap().id,
        MapId::new("m-ascent")
    );
    assert_eq!(library.agent_by_name("SOVA").unwrap().role, Role::Initiator);
}

#[test]
fn sorts_abilities_by_slot() {
    let library = fixture();
    let sova = library.agent(&AgentId::new("a-sova")).unwrap();
    let slots: Vec<u8> = sova.abilities.iter().map(|a| a.slot).collect();
    assert_eq!(slots, [1, 2, 4]);
}

#[test]
fn drops_unknown_tags() {
    let library = fixture();
    assert_eq!(library.lineup(LineupId(1)).unwrap().tags, [Tag::Essential]);
    assert!(library.lineup(LineupId(3)).unwrap().tags.is_empty());
}

#[test]
fn counts_lineups_per_map_and_agent() {
    let library = fixture();
    let ascent = MapId::new("m-ascent");
    assert_eq!(library.count(&ascent, &AgentId::new("a-sova")), 6);
    assert_eq!(library.count(&ascent, &AgentId::new("a-jett")), 0);
    assert_eq!(library.map_total(&ascent), 7);
    let names: Vec<_> = library
        .maps_by_name()
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(
        names,
        ["Ascent", "Bind"],
        "maps without lineups are left out"
    );
}

#[test]
fn media_paths_follow_the_layout() {
    let library = fixture();
    let id = LineupId(4);
    assert_eq!(library.clip(id), std::path::Path::new("lib/lineups/4.mp4"));
    assert_eq!(
        library.end_still(id),
        std::path::Path::new("lib/lineups/4-end.webp")
    );
    assert_eq!(
        library.map_image(&MapId::new("m-bind"), Side::Defense),
        std::path::Path::new("lib/maps/m-bind-defense.svg")
    );
}

#[test]
fn reads_each_bolts_charge_and_bounces() {
    let library = fixture();
    let bolts = &library.lineup(LineupId(2)).unwrap().bolts;
    let read: Vec<_> = bolts
        .iter()
        .map(|b| (b.charge, b.bounces.count()))
        .collect();
    assert_eq!(read, [(Charge::Two, 1), (Charge::Full, 0)]);
    assert_eq!(bolts[1].charge.label(), "Full charge");
    assert_eq!(bolts[0].bounces.label(), "1 bounce");
    assert_eq!(bolts[0].aim, None);
    assert_eq!(bolts[1].aim.as_deref(), Some("Top of the antenna"));
    assert!(
        library.lineup(LineupId(3)).unwrap().bolts.is_empty(),
        "a lineup without bolts has none"
    );
}

#[test]
fn rejects_a_charge_or_bounce_count_out_of_range() {
    for (from, to) in [
        (r#""charge": 3"#, r#""charge": 4"#),
        (r#""bounces": 1 }"#, r#""bounces": 3 }"#),
    ] {
        let err = Library::from_json("lib", &replace(from, to)).unwrap_err();
        assert!(
            matches!(err, LoadError::Parse { .. }),
            "{from} -> {to}: {err}"
        );
    }
    assert!(Bounces::try_from(2).is_ok());
}

fn replace(from: &str, to: &str) -> Vec<u8> {
    let json = std::str::from_utf8(FIXTURE).unwrap();
    assert!(json.contains(from), "fixture has {from:?}");
    json.replacen(from, to, 1).into_bytes()
}

#[test]
fn rejects_another_format() {
    let err = Library::from_json("lib", &replace(r#""format": 1"#, r#""format": 2"#)).unwrap_err();
    assert!(matches!(err, LoadError::Format { found: 2 }));
}

#[test]
fn rejects_a_lineup_with_an_unknown_map() {
    let json = replace(
        r#""id": 7, "map": "m-bind""#,
        r#""id": 7, "map": "m-haven""#,
    );
    let err = Library::from_json("lib", &json).unwrap_err();
    assert!(
        matches!(
            err,
            LoadError::Dangling {
                lineup: LineupId(7),
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn rejects_an_ability_of_another_agent() {
    let json = replace(
        r#""agent": "a-jett", "ability": 30"#,
        r#""agent": "a-jett", "ability": 20"#,
    );
    let err = Library::from_json("lib", &json).unwrap_err();
    assert_eq!(
        err.to_string(),
        "lineup 8 refers to ability 20 of Jett, which the library does not list"
    );
}

#[test]
fn rejects_a_duplicate_lineup() {
    let json = replace(r#""id": 9, "map""#, r#""id": 8, "map""#);
    assert_eq!(
        Library::from_json("lib", &json).unwrap_err().to_string(),
        "lineup 8 is listed more than once"
    );
}

#[test]
fn rejects_duplicate_maps_agents_and_abilities() {
    let message = |json: Vec<u8>| Library::from_json("lib", &json).unwrap_err().to_string();
    assert_eq!(
        message(replace(r#""id": "m-lotus""#, r#""id": "m-bind""#)),
        "map m-bind is listed more than once"
    );
    assert_eq!(
        message(replace(r#""id": "a-jett""#, r#""id": "a-brim""#)),
        "agent a-brim is listed more than once"
    );
    assert_eq!(
        message(replace(
            r#""id": 30, "name": "Cloudburst""#,
            r#""id": 20, "name": "Cloudburst""#
        )),
        "ability 20 is listed more than once"
    );
}

#[test]
fn rejects_a_bad_color() {
    let json = replace("#3D8BFF", "blue");
    assert!(matches!(
        Library::from_json("lib", &json).unwrap_err(),
        LoadError::Parse { .. }
    ));
}

#[test]
fn open_reports_a_missing_file() {
    let dir = std::env::temp_dir().join("arcwise-core-test-missing");
    let err = Library::open(&dir).unwrap_err();
    assert!(matches!(err, LoadError::Read { .. }));
}

/// Checks a real library: `ARCWISE_LIBRARY=<dir> cargo test -p arcwise-core -- --ignored`.
#[test]
#[ignore = "needs a library on disk"]
fn loads_a_real_library() {
    let root = std::env::var_os("ARCWISE_LIBRARY").expect("ARCWISE_LIBRARY is set");
    let library = Library::open(root).unwrap();
    assert!(!library.lineups().is_empty());
    for lineup in library.lineups() {
        assert!(library.clip(lineup.id).is_file(), "clip for {}", lineup.id);
    }
}

/// A second library: Ascent and Sova again, Haven new; its Shock Bolt is numbered 99, and its 10
/// is Owl Drone, which the fixture's 10 (Shock Bolt) is not.
const SECOND: &str = r##"{
  "format": 1, "name": "Second",
  "maps": [{ "id": "m-ascent", "name": "Ascent (second)" }, { "id": "m-haven", "name": "Haven" }],
  "agents": [{
    "id": "a-sova", "name": "Sova", "role": "initiator", "color": "#000000",
    "abilities": [{ "id": 99, "name": "shock bolt", "slot": 1 }, { "id": 10, "name": "Owl Drone", "slot": 3 }]
  }],
  "lineups": [
    { "id": 1, "map": "m-haven", "agent": "a-sova", "ability": 99, "side": "attack", "title": "Duplicate",
      "stand": { "x": 1, "y": 1, "callout": null }, "land": { "x": 2, "y": 2, "callout": null }, "steps": null },
    { "id": 100, "map": "m-haven", "agent": "a-sova", "ability": 99, "side": "attack", "title": "Haven shock",
      "stand": { "x": 1, "y": 1, "callout": null }, "land": { "x": 2, "y": 2, "callout": null }, "steps": null },
    { "id": 101, "map": "m-ascent", "agent": "a-sova", "ability": 10, "side": "defense", "title": "Ascent drone",
      "stand": { "x": 1, "y": 1, "callout": null }, "land": { "x": 2, "y": 2, "callout": null }, "steps": null }
  ]
}"##;

fn merged() -> Library {
    Library::merge(vec![
        Part::parse("first", FIXTURE).unwrap(),
        Part::parse("second", SECOND.as_bytes()).unwrap(),
    ])
}

#[test]
fn merges_libraries_into_one() {
    let library = merged();
    let names: Vec<_> = library.maps().iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        ["Ascent", "Bind", "Lotus", "Haven"],
        "the first library names a shared map"
    );
    assert_eq!(library.lineups().len(), 9 + 2);
    assert_eq!(
        library.skipped(),
        [0, 1],
        "the second library's lineup 1 is skipped"
    );
    assert_eq!(
        library.lineup(LineupId(1)).unwrap().title,
        "Shock A site from Lobby"
    );
    assert_eq!(
        library.count(&MapId::new("m-haven"), &AgentId::new("a-sova")),
        1
    );
    assert_eq!(
        library.count(&MapId::new("m-ascent"), &AgentId::new("a-sova")),
        7
    );
}

#[test]
fn an_ability_of_the_same_name_is_one_ability_and_a_clashing_id_is_renumbered() {
    let library = merged();
    assert_eq!(
        library.lineup(LineupId(100)).unwrap().ability,
        AbilityId(10),
        "Shock Bolt"
    );
    let drone = library.lineup(LineupId(101)).unwrap().ability;
    assert_ne!(drone, AbilityId(10));
    assert_eq!(library.ability(drone).unwrap().name, "Owl Drone");
    let sova = library.agent(&AgentId::new("a-sova")).unwrap();
    let names: Vec<_> = sova.abilities.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        ["Shock Bolt", "Recon Bolt", "Owl Drone", "Hunter's Fury"]
    );
    // The renumbered ability's icon keeps its own library's file name.
    assert_eq!(
        library.ability_icon(drone),
        std::path::Path::new("second/abilities/10.webp")
    );
    assert_eq!(
        library.ability_icon(AbilityId(10)),
        std::path::Path::new("first/abilities/10.webp")
    );
}

#[test]
fn media_comes_from_each_items_own_library() {
    let library = merged();
    let path = |p: &str| std::path::PathBuf::from(p);
    assert_eq!(library.clip(LineupId(1)), path("first/lineups/1.mp4"));
    assert_eq!(library.clip(LineupId(100)), path("second/lineups/100.mp4"));
    assert_eq!(
        library.map_card(&MapId::new("m-ascent")),
        path("first/maps/m-ascent.webp")
    );
    assert_eq!(
        library.map_image(&MapId::new("m-haven"), Side::Attack),
        path("second/maps/m-haven-attack.svg")
    );
    assert_eq!(library.roots(), [path("first"), path("second")]);
}

#[test]
fn a_part_lists_its_maps_and_files() {
    let part = Part::parse("second", SECOND.as_bytes()).unwrap();
    assert_eq!(part.name(), Some("Second"));
    assert_eq!(part.lineup_count(), 3);
    let maps: Vec<_> = part
        .maps()
        .iter()
        .map(|(m, n)| (m.name.as_str(), *n))
        .collect();
    assert_eq!(maps, [("Ascent (second)", 1), ("Haven", 2)]);
    let haven = part.files(Some(&[MapId::new("m-haven")]));
    assert!(haven.contains(&"lineups/100.mp4".to_owned()));
    assert!(!haven.contains(&"lineups/101.mp4".to_owned()));
    assert!(
        haven.contains(&"abilities/99.webp".to_owned()),
        "pictures for every map come too"
    );
    assert_eq!(part.files(None).len(), 6 + 3 + 3 * 4);
}
