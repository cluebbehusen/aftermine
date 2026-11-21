use super::*;
use aftermine_sim::{PlayerId, Scenario};

fn opening() -> (aftermine_sim::State, FogView) {
    let state = Scenario::skirmish().build().expect("skirmish builds");
    let view = FogView::capture(&state, PlayerId(0));
    (state, view)
}

#[test]
fn the_digest_lists_the_seat_s_own_forces_and_bank() {
    let (_, view) = opening();
    let text = render(&view, None);
    assert!(
        text.starts_with(&format!(
            "TICK 0 (0:00) | SEAT 0 | SCRAP {}",
            view.own_player.scrap
        )),
        "{text}"
    );
    for unit in view.units.iter().filter(|unit| unit.player == 0) {
        assert!(text.contains(&format!("    {} at ", unit.id)), "{text}");
    }
    for building in view
        .buildings
        .iter()
        .filter(|building| building.player == 0)
    {
        assert!(
            text.contains(&format!("  {} {} at ", building.id, building.kind.name())),
            "{text}"
        );
    }
    assert!(!text.contains("MAP:"), "the map is opt-in");
}

#[test]
fn the_digest_shows_no_other_seat_it_cannot_see() {
    let (state, view) = opening();
    let text = render(&view, Some(state.map()));
    for unit in state
        .units()
        .iter()
        .filter(|unit| unit.player != PlayerId(0))
    {
        let seen = view.units.iter().any(|known| known.id == unit.id.0);
        assert_eq!(
            text.contains(&format!("{} {} seat", unit.id.0, unit.kind.name())),
            seen,
            "{text}"
        );
    }
}

#[test]
fn the_map_draws_remembered_scrap_and_never_the_live_ground() {
    let (state, mut view) = opening();
    view.scrap.clear();
    view.wrecks.clear();
    let text = render(&view, Some(state.map()));
    let map = text
        .split("radar contact\n")
        .nth(1)
        .expect("the map is drawn");
    assert!(
        !map.contains('s'),
        "scrap the seat has no memory of leaked:\n{map}"
    );
    assert!(map.contains('O') && map.contains('o'), "{map}");
    let (state, view) = opening();
    let known = view
        .scrap
        .first()
        .expect("a seat remembers the scrap it starts beside");
    let text = render(&view, Some(state.map()));
    let map = text
        .split("radar contact\n")
        .nth(1)
        .expect("the map is drawn");
    let row = map
        .lines()
        .nth(2 + usize::try_from(known.tile[1]).unwrap())
        .expect("the remembered tile's row");
    let glyph = row
        .chars()
        .nth(4 + usize::try_from(known.tile[0]).unwrap())
        .expect("the remembered tile's column");
    assert!(matches!(glyph, 's' | 'o' | 'O'), "{glyph:?} in {row:?}");
}

#[test]
fn orders_read_as_a_tag_and_fields() {
    let tracks = Tracks {
        contacts: BTreeMap::new(),
        buildings: &[],
    };
    assert_eq!(
        flatten(
            &serde_json::json!({"order": "harvest", "node": {"x": 4, "y": 3}}),
            &tracks
        ),
        "harvest node=4,3"
    );
    assert_eq!(
        flatten(&serde_json::json!({"order": "idle"}), &tracks),
        "idle"
    );
    assert_eq!(flatten(&serde_json::json!([1, 2]), &tracks), "[1 2]");
    let run = aftermine_sim::Order::Run {
        goal: aftermine_sim::Goal {
            x: 44,
            y: 24,
            aim: aftermine_sim::Aim::Slot(TilePos::new(45, 24)),
            endpoint: None,
            march: Some(aftermine_sim::MarchId(2)),
        },
    };
    assert_eq!(
        compact(&run, &tracks),
        "run goal=44,24",
        "a goal reads as the tile it was given"
    );
}

#[test]
fn an_attack_names_its_target_as_the_seat_knows_it() {
    use aftermine_sim::{AttackTarget, ContactId, Order, UnitId};
    let seen = ContactTrack {
        id: ContactId(126),
        tile: TilePos::new(30, 12),
        domain: aftermine_sim::stats::Domain::Ground,
        visible_unit: Some(UnitId(72)),
        history: Vec::new(),
    };
    let dark = ContactTrack {
        id: ContactId(127),
        tile: TilePos::new(40, 9),
        visible_unit: None,
        ..seen.clone()
    };
    let (_, view) = opening();
    let tracks = Tracks {
        contacts: [(126, &seen), (127, &dark)].into_iter().collect(),
        buildings: &view.buildings,
    };
    let attack = |target| {
        compact(
            &Order::Attack {
                target,
                pursue: false,
                resume: None,
            },
            &tracks,
        )
    };
    assert_eq!(
        attack(AttackTarget::Contact(ContactId(126))),
        "attack target=unit 72"
    );
    assert_eq!(
        attack(AttackTarget::Contact(ContactId(127))),
        "attack target=contact at (40,9)"
    );
    assert_eq!(
        attack(AttackTarget::Contact(ContactId(9))),
        "attack target=a contact no longer tracked"
    );
    assert_eq!(
        attack(AttackTarget::RememberedBuilding(
            aftermine_sim::RememberedBuilding {
                owner: aftermine_sim::PlayerId(1),
                building_kind: aftermine_sim::BuildingKind::Turret,
                anchor: TilePos::new(21, 9),
            }
        )),
        "attack target=remembered turret at (21,9)"
    );
    let foundry = &view.buildings[0];
    assert_eq!(
        attack(AttackTarget::RememberedBuilding(
            aftermine_sim::RememberedBuilding {
                owner: aftermine_sim::PlayerId(foundry.player),
                building_kind: foundry.kind,
                anchor: TilePos::new(foundry.anchor[0], foundry.anchor[1]),
            }
        )),
        format!(
            "attack target=structure {} {} at ({},{})",
            foundry.id,
            foundry.kind.tier_name(foundry.tier),
            foundry.anchor[0],
            foundry.anchor[1]
        ),
        "a structure in sight is named, not remembered"
    );
}

#[test]
fn an_advance_reads_as_what_the_seat_saw_happen() {
    use aftermine_protocol::SeenEvent;
    use aftermine_sim::{BuildingId, BuildingKind, Event, GameResult, UnitId, UnitKind};
    use chassis::fx::{Fx, Vec2Fx};
    let at = |x: i32, y: i32| Vec2Fx::new(Fx::from_num(x), Fx::from_num(y));
    let view = PlayedView {
        player: PlayerId(0),
        team: 0,
        ticks: 1200,
        tick: 2400,
        income: 300,
        log: vec![
            SeenEvent {
                tick: 1300,
                event: Event::UnitDied {
                    unit: UnitId(3),
                    kind: UnitKind::Harvester,
                    player: PlayerId(0),
                    pos: at(10, 4),
                    grounded: true,
                },
                note: None,
            },
            SeenEvent {
                tick: 2399,
                event: Event::BuildingDestroyed {
                    building: BuildingId(1),
                    kind: BuildingKind::Foundry,
                    player: PlayerId(1),
                    tier: 0,
                    pos: at(53, 39),
                },
                note: None,
            },
            SeenEvent {
                tick: 2399,
                event: Event::GameOver {
                    result: GameResult::Victory { team: 0 },
                },
                note: None,
            },
        ],
        result: Some(GameResult::Victory { team: 0 }),
        tick_limit: Some(24_000),
    };
    let text = played(&view);
    assert!(
        text.starts_with("ADVANCED 1200 ticks to 2400 (2:00) | income 300 scrap, 300 a minute"),
        "{text}"
    );
    assert!(
        text.contains("1300 (1:05) your harvester 3 died at (10,4)"),
        "{text}"
    );
    assert!(
        text.contains("seat 1's foundry 1 was destroyed at (53,39)"),
        "{text}"
    );
    assert!(
        text.contains("MATCH OVER at tick 2400 (2:00): you won"),
        "{text}"
    );
}

#[test]
fn a_warhead_reads_in_tiles_and_ticks() {
    use aftermine_sim::strike::{KnownWarhead, WarheadLine};
    use aftermine_sim::{BuildingId, WarheadId};
    use chassis::fx::{Fx, Vec2Fx};
    let at = |x: &str, y: &str| Vec2Fx::new(Fx::lit(x), Fx::lit(y));
    let (_, mut view) = opening();
    view.tick = 13_800;
    view.warheads = vec![
        KnownWarhead {
            id: WarheadId(0),
            player: Some(PlayerId(1)),
            hostile: true,
            position: at("47.3", "10.6"),
            line: Some(WarheadLine {
                launch: at("26.0", "5.0"),
                impact: at("47.5", "10.5"),
                arrival: 13_855,
            }),
            exposure: Some(60),
            interceptor: Some(BuildingId(66)),
        },
        KnownWarhead {
            id: WarheadId(1),
            player: None,
            hostile: true,
            position: at("30.9", "8.1"),
            line: None,
            exposure: Some(3),
            interceptor: None,
        },
        KnownWarhead {
            id: WarheadId(2),
            player: Some(PlayerId(0)),
            hostile: false,
            position: at("26.0", "5.0"),
            line: Some(WarheadLine {
                launch: at("26.0", "5.0"),
                impact: at("50.5", "30.5"),
                arrival: 13_900,
            }),
            exposure: None,
            interceptor: None,
        },
        KnownWarhead {
            id: WarheadId(3),
            player: Some(PlayerId(2)),
            hostile: false,
            position: at("12.0", "30.0"),
            line: Some(WarheadLine {
                launch: at("12.0", "30.0"),
                impact: at("50.5", "30.5"),
                arrival: 13_950,
            }),
            exposure: None,
            interceptor: None,
        },
    ];
    let text = render(&view, None);
    assert!(
        text.contains("  2 own at (26,5) lands at (50,30)"),
        "{text}"
    );
    assert!(
        text.contains("  3 allied from seat 2 at (12,30) lands at (50,30)"),
        "a teammate's warhead is not the seat's own: {text}"
    );
    assert!(
        text.contains(
            "  0 hostile from seat 1 at (47,10) lands at (47,10) on tick 13855 (in 55 ticks), \
             launched from (26,5) | held 60 ticks | interceptor structure 66 expected"
        ),
        "{text}"
    );
    assert!(
        text.contains("  1 hostile at (30,8) landing point not yet tracked | held 3 ticks"),
        "{text}"
    );
    assert!(!text.contains("bits"), "{text}");
    view.tick = 13_855;
    assert!(
        render(&view, None).contains("lands at (47,10) this tick"),
        "{}",
        render(&view, None)
    );
}

#[test]
fn took_fire_names_what_was_hit_when_the_seat_can() {
    use aftermine_protocol::SeenEvent;
    use aftermine_sim::Event;
    use chassis::fx::{Fx, Vec2Fx};
    let fire = |note: Option<&str>| SeenEvent {
        tick: 1300,
        event: Event::DamageTaken {
            player: PlayerId(0),
            pos: Vec2Fx::new(Fx::from_num(12), Fx::from_num(7)),
        },
        note: note.map(str::to_string),
    };
    let view = PlayedView {
        player: PlayerId(0),
        team: 0,
        ticks: 200,
        tick: 1400,
        income: 0,
        log: vec![fire(Some("lancer 72")), fire(None)],
        result: None,
        tick_limit: None,
    };
    let text = played(&view);
    assert!(
        text.contains("1300 (1:05) your lancer 72 took fire at (12,7)"),
        "{text}"
    );
    assert!(
        text.contains("1300 (1:05) your units or structures took fire near (12,7)"),
        "{text}"
    );
}
