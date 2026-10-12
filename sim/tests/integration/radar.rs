//! Radar beyond masts: a Kestrel's ring is always up, and a Surveyor's
//! unfolds only after it has stood still for its deploy time.

use crate::common;
use common::{building, cmd, open_arena, unit};

use chassis::grid::TilePos;
use oxide_sim::scenario::{BuildingSpec, UnitSpec};
use oxide_sim::stats::{BuildingKind, HARVEST_RADAR_DANGER_RADIUS, UnitKind};
use oxide_sim::{Command, Order, PlayerId, State, UnitId};

fn field(units: Vec<UnitSpec>, buildings: Vec<BuildingSpec>) -> State {
    let mut scenario = open_arena(48, 24, units);
    scenario.buildings = buildings;
    scenario.build().unwrap()
}

fn contacts(state: &State) -> Vec<TilePos> {
    state.vision(PlayerId(0)).contacts().to_vec()
}

fn deploy_ticks() -> u8 {
    UnitKind::Surveyor.stats().radar.unwrap().deploy_ticks
}

fn idle(state: &mut State, ticks: u8) {
    for _ in 0..ticks {
        state.tick(&[]);
        state.validate_invariants().unwrap();
    }
}

#[test]
fn a_kestrels_ring_reaches_exactly_its_radius() {
    let radius = UnitKind::Kestrel.stats().radar.unwrap().radius;
    let ring = |dx: i32| {
        let mut state = field(
            vec![
                unit(0, UnitKind::Kestrel, 10, 10),
                unit(1, UnitKind::Harvester, 10 + dx, 10),
            ],
            Vec::new(),
        );
        state.tick(&[]);
        contacts(&state)
    };
    assert_eq!(ring(radius), [TilePos::new(10 + radius, 10)]);
    assert!(ring(radius + 1).is_empty());
}

#[test]
fn a_surveyor_unfolds_its_radar_after_standing_still_and_folds_to_drive() {
    let hidden = TilePos::new(21, 10);
    let mut state = field(
        vec![
            unit(0, UnitKind::Surveyor, 10, 10),
            unit(1, UnitKind::Harvester, hidden.x, hidden.y),
        ],
        Vec::new(),
    );
    let scout = state.units()[0].id;
    idle(&mut state, deploy_ticks() - 1);
    assert!(contacts(&state).is_empty(), "still unfolding");
    assert!(state.unit(scout).unwrap().radar().is_none());
    idle(&mut state, 1);
    assert_eq!(contacts(&state), [hidden], "the ring is up");
    for contact in contacts(&state) {
        assert!(
            contact.chebyshev(state.unit(scout).unwrap().tile()) > HARVEST_RADAR_DANGER_RADIUS,
            "a scout's own contacts lie beyond its danger ring"
        );
    }

    state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![scout],
            goal: TilePos::new(10, 14),
            queue: false,
        },
    )]);
    assert_eq!(
        state.unit(scout).unwrap().radar_deploy,
        0,
        "driving folds it"
    );
    assert!(contacts(&state).is_empty());
    let mut ticks = 0;
    while state.unit(scout).unwrap().radar().is_none() {
        state.tick(&[]);
        state.validate_invariants().unwrap();
        ticks += 1;
        assert!(ticks < 200, "the radar comes back up after the walk");
    }
    assert_eq!(state.unit(scout).unwrap().tile(), TilePos::new(10, 14));
    assert_eq!(contacts(&state), [hidden]);
}

#[test]
fn a_shove_from_a_passing_machine_leaves_the_radar_up() {
    let mut state = field(
        vec![
            unit(0, UnitKind::Surveyor, 10, 10),
            unit(0, UnitKind::Sentinel, 6, 10),
        ],
        Vec::new(),
    );
    let (scout, walker) = (state.units()[0].id, state.units()[1].id);
    idle(&mut state, deploy_ticks());
    assert!(state.unit(scout).unwrap().radar().is_some());
    let start = state.unit(scout).unwrap().pos;
    state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![walker],
            goal: TilePos::new(14, 10),
            queue: false,
        },
    )]);
    let mut shoved = false;
    for _ in 0..120 {
        state.tick(&[]);
        state.validate_invariants().unwrap();
        shoved |= state.unit(scout).unwrap().pos != start;
        assert!(
            state.unit(scout).unwrap().radar().is_some(),
            "a push is not driving"
        );
    }
    assert!(shoved, "test premise: the walker pushes the scout aside");
}

#[test]
fn scout_radar_never_reveals_a_buried_charge() {
    let charge = TilePos::new(18, 10);
    let mut state = field(
        vec![unit(0, UnitKind::Surveyor, 10, 10)],
        vec![building(1, BuildingKind::ScuttleCharge, charge.x, charge.y)],
    );
    idle(&mut state, deploy_ticks());
    let scout = state.units()[0].id;
    assert!(state.unit(scout).unwrap().radar().is_some());
    assert!(!contacts(&state).contains(&charge));
    assert!(state.vision(PlayerId(0)).ghosts().is_empty());
}

#[test]
fn a_scout_waiting_for_a_safe_view_unfolds_its_radar() {
    // A Bastion line it cannot see past safely, kept in sight by a Kestrel
    // the Bastion cannot shoot.
    let mut state = field(
        vec![
            unit(0, UnitKind::Surveyor, 8, 9),
            unit(0, UnitKind::Kestrel, 24, 2),
        ],
        vec![building(1, BuildingKind::Bastion, 24, 8)],
    );
    let scout: UnitId = state.units()[0].id;
    let goal = TilePos::new(23, 9);
    state.tick(&[cmd(
        0,
        Command::Scout {
            units: vec![scout],
            goal,
            queue: false,
        },
    )]);
    idle(&mut state, deploy_ticks() + 1);
    let unit = state.unit(scout).unwrap();
    assert_eq!(unit.order, Order::Scout { tile: goal }, "still waiting");
    assert!(unit.radar().is_some(), "a held scout is a picket");
}

#[test]
fn a_parked_surveyor_spots_for_an_avalanche() {
    // The Reclaimer lies beyond both machines' sight but inside the
    // Surveyor's radar and the Avalanche's reach.
    let target = TilePos::new(25, 10);
    let shelled = |spotter: bool| {
        let mut units = vec![unit(0, UnitKind::Avalanche, 12, 10)];
        if spotter {
            units.push(unit(0, UnitKind::Surveyor, 14, 10));
        }
        let mut state = field(
            units,
            vec![building(1, BuildingKind::Reclaimer, target.x, target.y)],
        );
        let reclaimer = state
            .buildings()
            .iter()
            .find(|b| b.kind == BuildingKind::Reclaimer)
            .unwrap()
            .id;
        let full = state.building(reclaimer).unwrap().hp;
        idle(&mut state, 200);
        idle(&mut state, 200);
        assert!(!state.vision(PlayerId(0)).visible(target));
        state.building(reclaimer).is_none_or(|b| b.hp < full)
    };
    assert!(shelled(true), "the radar contact draws blind fire");
    assert!(
        !shelled(false),
        "without the scout there is nothing to shoot at"
    );
}

#[test]
fn a_saved_unfolding_radar_round_trips_and_forgeries_are_refused() {
    let mut state = field(vec![unit(0, UnitKind::Surveyor, 10, 10)], Vec::new());
    idle(&mut state, deploy_ticks() / 2);
    let json = serde_json::to_value(&state).unwrap();
    let restored: State = serde_json::from_value(json.clone()).unwrap();
    restored.validate_invariants().unwrap();
    assert_eq!(restored.hash(), state.hash());

    let refused = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut forged = json.clone();
        edit(&mut forged);
        serde_json::from_value::<State>(forged)
            .unwrap_err()
            .to_string()
    };
    let over = u64::from(deploy_ticks()) + 1;
    assert!(
        refused(&|d| d["units"][0]["radar_deploy"] = serde_json::json!(over))
            .contains("impossible radar deployment")
    );
    assert!(
        refused(&|d| {
            d["units"][0]["path"] = serde_json::json!({
                "goal": {"x": 12, "y": 10},
                "waypoints": [{"x": 11, "y": 10}, {"x": 12, "y": 10}],
                "next": 0,
            });
        })
        .contains("impossible radar deployment"),
        "a radar unfolds only at rest"
    );
}
