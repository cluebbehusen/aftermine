use super::*;
use crate::map::Map;
use crate::state::{Player, Recovery};
use crate::stats::UnitKind;
use chassis::fx::Fx;

/// The pass [`detonate_under_units`] replaced: every unit tested against
/// every live charge, in slot order.
fn scanned_detonate_under_units(state: &mut State, events: &mut Vec<Event>) {
    for slot in 0..state.buildings.len() {
        let b = &state.buildings[slot];
        if b.kind != BuildingKind::ScuttleCharge || !b.built() || b.hp == 0 {
            continue;
        }
        let tripped = state.units.iter().any(|u| {
            u.hp > 0
                && state.hostile(b.player, u.player)
                && u.domain() == Domain::Ground
                && u.pos.dist_sq(b.center()) <= {
                    let trigger =
                        CHARGE_TRIGGER_RADIUS + crate::stats::hull_excess(u.kind.stats().radius);
                    trigger * trigger
                }
                && blast_reaches(
                    &state.map,
                    b.center(),
                    b.anchor,
                    ShotEnd::new(u.pos, Domain::Ground),
                )
        });
        if tripped {
            detonate(state, slot, events);
        }
    }
}

/// A 32-tile floor with a short wall, for seats on the given teams.
fn field(teams: &[u8]) -> State {
    let mut rows = vec![".".repeat(32); 32];
    rows[14] = format!("{}{}{}", ".".repeat(9), "#".repeat(5), ".".repeat(18));
    let (map, _) = Map::parse(&rows).unwrap();
    let seat = |team: u8| Player {
        name: "p".into(),
        team,
        scrap: 0,
        recovery: Recovery::Ready,
        resigned: false,
        eliminated_at: None,
    };
    State::assemble(map, teams.iter().map(|&team| seat(team)).collect())
}

#[test]
fn the_trigger_window_holds_every_body_a_charge_trips() {
    let sixteenth = Fx::lit("0.0625");
    for kind in UnitKind::ALL {
        let mut state = field(&[0, 1]);
        state.place_building(
            PlayerId(1),
            BuildingKind::ScuttleCharge,
            TilePos::new(15, 16),
        );
        let center = state.buildings[0].center();
        state.spawn_unit(PlayerId(0), kind, center);
        let (low, high) = trigger_window(center);
        let mut tripped = 0;
        for dy in -64..=64 {
            for dx in -64..=64 {
                let offset = Vec2Fx::new(Fx::from_num(dx), Fx::from_num(dy)) * sixteenth;
                state.units[0].pos = center + offset;
                if trips(&state, &state.buildings[0], &state.units[0]) {
                    tripped += 1;
                    let tile = state.units[0].tile();
                    assert!(
                        (low.x..=high.x).contains(&tile.x) && (low.y..=high.y).contains(&tile.y),
                        "{kind:?} at {offset:?} trips from outside the window"
                    );
                }
            }
        }
        assert_eq!(
            tripped > 0,
            kind.stats().domain == Domain::Ground,
            "{kind:?} trips {tripped} times"
        );
    }
}

#[test]
fn detonating_matches_the_full_unit_scan() {
    let kinds: Vec<UnitKind> = UnitKind::ALL.into_iter().collect();
    let mut detonations = 0;
    let mut chains = 0;
    for shift in 0..64 {
        let mut state = field(&[0, 1, 2, 0]);
        // Hostile charges in pairs inside each other's blast ring, so one
        // blast can kill a neighbor before its turn, and a lattice of them
        // across the wall.
        for (n, y) in (3..30).step_by(4).enumerate() {
            for (m, x) in (2..30).step_by(5).enumerate() {
                let owner = PlayerId::from_index((n + m) % 3);
                let neighbor = PlayerId::from_index((n + m + 1) % 3);
                state.place_building(owner, BuildingKind::ScuttleCharge, TilePos::new(x, y));
                state.place_building(
                    neighbor,
                    BuildingKind::ScuttleCharge,
                    TilePos::new(x + 1, y),
                );
            }
        }
        // A crowd that slides a sixteenth of a tile per shift, so bodies
        // cross every trigger edge.
        let step = Fx::lit("0.0625") * Fx::from_num(shift);
        for (i, y) in (2..30).step_by(3).enumerate() {
            for (j, x) in (1..31).step_by(4).enumerate() {
                let kind = kinds[(i * 7 + j * 3) % kinds.len()];
                let owner = PlayerId::from_index((i + j) % 4);
                let pos = TilePos::new(x, y).center() + Vec2Fx::new(step, step / 2);
                let unit = state.spawn_unit(owner, kind, pos);
                if (i + j) % 11 == 0 {
                    state.unit_mut(unit).unwrap().hp = 0;
                }
            }
        }
        let mut reference = state.clone();
        let (mut fast_events, mut scanned_events) = (Vec::new(), Vec::new());
        detonate_under_units(&mut state, &mut fast_events);
        scanned_detonate_under_units(&mut reference, &mut scanned_events);
        assert_eq!(fast_events, scanned_events, "shift {shift}");
        assert_eq!(state.hash(), reference.hash(), "shift {shift}");
        let fired = fast_events
            .iter()
            .filter(|e| matches!(e, Event::ChargeDetonated { .. }))
            .count();
        detonations += fired;
        chains += usize::from(fired > 1);
    }
    assert!(
        detonations > 0 && chains > 0,
        "{detonations} blasts, {chains} chains"
    );
}
