//! The Piledriver: a braced, instant heavy slug that picks the heaviest
//! machine in reach when nobody aims it.

use crate::common;

use chassis::grid::TilePos;
use oxide_sim::{Command, Event, Order, PlayerId, State, Target, UnitId, UnitKind};

use common::*;

/// West's `gun` at (5,5) facing east, with East's machines at the given
/// spots, all in its reach and sight.
fn range(gun: UnitKind, east: &[(UnitKind, i32, i32)]) -> (State, UnitId, Vec<UnitId>) {
    let mut units = vec![unit(0, gun, 5, 5)];
    units.extend(east.iter().map(|&(kind, x, y)| unit(1, kind, x, y)));
    let mut state = open_arena(30, 12, units).build().unwrap();
    let ids: Vec<UnitId> = state.units().iter().map(|unit| unit.id).collect();
    face_toward(&mut state, ids[0], TilePos::new(12, 5).center());
    (state, ids[0], ids[1..].to_vec())
}

/// Whom `id` is attacking, if anyone.
fn aimed_at(state: &State, id: UnitId) -> Option<Target> {
    let unit = state.unit(id).unwrap();
    match unit.order {
        Order::Attack { target, .. } => state
            .attack_view(unit.player, target)
            .and_then(|view| view.entity),
        _ => None,
    }
}

#[test]
fn a_piledriver_picks_the_heaviest_machine_in_reach() {
    let (mut state, gun, east) = range(
        UnitKind::Piledriver,
        &[(UnitKind::Sentinel, 8, 5), (UnitKind::Breaker, 12, 6)],
    );
    state.tick(&[]);
    assert_eq!(
        aimed_at(&state, gun),
        Some(Target::Unit(east[1])),
        "the Breaker over the nearer Sentinel"
    );
    // A gun that ranks by distance takes the Sentinel from the same spot.
    let (mut state, lancer, east) = range(
        UnitKind::Lancer,
        &[(UnitKind::Sentinel, 8, 5), (UnitKind::Breaker, 12, 6)],
    );
    state.tick(&[]);
    assert_eq!(aimed_at(&state, lancer), Some(Target::Unit(east[0])));
}

#[test]
fn with_only_light_machines_in_reach_a_piledriver_shoots_them() {
    let (mut state, gun, east) = range(UnitKind::Piledriver, &[(UnitKind::Sentinel, 8, 5)]);
    state.tick(&[]);
    assert_eq!(aimed_at(&state, gun), Some(Target::Unit(east[0])));
}

#[test]
fn a_piledriver_braces_before_its_slug_and_never_fires_on_the_move() {
    let (mut state, gun, east) = range(UnitKind::Piledriver, &[(UnitKind::Breaker, 12, 5)]);
    let breaker = east[0];
    let deploy = u64::from(
        UnitKind::Piledriver
            .stats()
            .brace
            .expect("the Piledriver braces")
            .deploy_ticks,
    );
    let start = state.current_tick();
    let slug = UnitKind::Piledriver.stats().weapons[0].damage;
    let max = UnitKind::Breaker.stats().max_hp;
    run_until(&mut state, 200, |s, _| s.unit(breaker).unwrap().hp < max);
    assert!(
        state.current_tick() - start > deploy,
        "the first slug waits for the spades"
    );
    assert_eq!(state.unit(breaker).unwrap().hp, max - slug);
    // Ordered to move, it stows and fires nothing until it stops.
    state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![gun],
            goal: TilePos::new(5, 9),
            queue: false,
        },
    )]);
    let hp = state.unit(breaker).unwrap().hp;
    let origin = state.unit(gun).unwrap().pos;
    let mut retractions = 0;
    for _ in 0..40 {
        let before = state.unit(gun).unwrap().braces();
        state.tick(&[]);
        let unit = state.unit(gun).unwrap();
        let after = unit.braces();
        if before != after {
            assert_eq!(after, before.saturating_sub(2));
            retractions += 1;
        }
        if after == 0 {
            break;
        }
        assert_eq!(unit.pos, origin, "the planted gun cannot drive");
    }
    assert_eq!(retractions, 10, "folding takes half a second");
    assert_eq!(state.unit(gun).unwrap().braces(), 0);
    assert_eq!(state.unit(breaker).unwrap().hp, hp);
    let moving = run_until(&mut state, 200, |s, _| {
        s.unit(gun).unwrap().tile() == TilePos::new(5, 9)
    });
    assert!(
        !moving.iter().any(
            |event| matches!(event, Event::DamageTaken { player, .. } if *player == PlayerId(1))
        ),
        "no slug lands while it walks"
    );
    assert_eq!(state.unit(breaker).unwrap().hp, hp);
}
