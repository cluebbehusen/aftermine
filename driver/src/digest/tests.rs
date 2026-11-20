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
    let tracks = Tracks::new();
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
    let tracks: Tracks = [(126, &seen), (127, &dark)].into_iter().collect();
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
        "attack target=a lost contact"
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
}
