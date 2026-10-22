//! A friendly Maw standing in a route: small bodies walk round it where
//! there is room, a parked one pulls aside where there is none, and no body
//! is ever pushed inside its hull.
use crate::common;
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;
use common::{cmd, players, unit};
use oxide_sim::scenario::{PlayerSpec, ScenarioMode, UnitSpec};
use oxide_sim::stats::COLLISION_MAX_STEP;
use oxide_sim::{Command, Order, PlayerId, Scenario, State, Unit, UnitId, UnitKind};

const WIDTH: usize = 48;
const HEIGHT: usize = 21;
const MID: i32 = 10;
const GOAL: TilePos = TilePos::new(40, MID);
/// Every body east of this column has cleared the Maw's ground.
const THROUGH: i32 = 34;

/// Open ground, walled at the border.
fn field() -> Vec<String> {
    (0..HEIGHT)
        .map(|y| {
            (0..WIDTH)
                .map(|x| {
                    let edge = x == 0 || y == 0 || x == WIDTH - 1 || y == HEIGHT - 1;
                    if edge { '#' } else { '.' }
                })
                .collect()
        })
        .collect()
}

/// Two open fields joined by a pass `width` tiles wide, centred on `MID`.
fn pass(width: i32) -> Vec<String> {
    let top = MID - width / 2;
    field()
        .into_iter()
        .enumerate()
        .map(|(y, row)| {
            let y = i32::try_from(y).unwrap();
            row.chars()
                .enumerate()
                .map(|(x, c)| {
                    let x = i32::try_from(x).unwrap();
                    let walled = (14..THROUGH).contains(&x) && !(top..top + width).contains(&y);
                    if walled { '#' } else { c }
                })
                .collect()
        })
        .collect()
}

/// Three Lancers, two Wardens, two Sentinels and two Harvesters.
fn crowd(columns: [i32; 5]) -> Vec<UnitSpec> {
    let kinds = [UnitKind::Lancer; 3]
        .into_iter()
        .chain([UnitKind::Warden; 2])
        .chain([UnitKind::Sentinel; 2])
        .chain([UnitKind::Harvester; 2]);
    let spots = [9, 7, 5, 3]
        .into_iter()
        .flat_map(|x| columns.map(|y| (x, y)));
    kinds
        .zip(spots)
        .map(|(kind, (x, y))| unit(0, kind, x, y))
        .collect()
}

fn staged(map: Vec<String>, units: Vec<UnitSpec>) -> State {
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Sandbox,
        name: "standing-hulls".into(),
        map,
        players: players(0),
        units,
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .unwrap()
}

fn maw_of(state: &State, player: u8) -> UnitId {
    state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Maw && u.player == PlayerId(player))
        .unwrap()
        .id
}

fn movers_of(state: &State, player: u8) -> Vec<UnitId> {
    state
        .units()
        .iter()
        .filter(|u| u.kind != UnitKind::Maw && u.player == PlayerId(player))
        .map(|u| u.id)
        .collect()
}

/// Whether `body` stands inside `hull`'s body: deeper than the one
/// collision step that any two bodies in contact may overlap by.
fn inside(hull: &Unit, body: &Unit) -> bool {
    let clear = hull.kind.stats().radius + body.kind.stats().radius - COLLISION_MAX_STEP;
    hull.pos.dist_sq(body.pos) < clear * clear
}

/// Orders seat 0's movers to `GOAL` and runs until each has cleared the
/// Maw's ground, checking every tick that none stands inside the Maw.
/// Returns the Maw's displacement once all are through.
fn walk_past(state: &mut State, max_ticks: u64) -> Vec2Fx {
    let maw = maw_of(state, 0);
    let start = state.unit(maw).unwrap().pos;
    let movers = movers_of(state, 0);
    state.tick(&[cmd(
        0,
        Command::Run {
            units: movers.clone(),
            goal: GOAL,
            queue: false,
        },
    )]);
    let mut cleared = vec![false; movers.len()];
    for tick in 1..=max_ticks {
        let hull = state.unit(maw).unwrap();
        for (done, &id) in cleared.iter_mut().zip(&movers) {
            let body = state.unit(id).unwrap();
            assert!(!inside(hull, body), "tick {tick}: {id:?} is inside the Maw");
            *done |= TilePos::containing(body.pos).x >= THROUGH;
        }
        if cleared.iter().all(|&done| done) {
            return hull.pos - start;
        }
        state.tick(&[]);
    }
    panic!("not every body got past the Maw within {max_ticks} ticks: {cleared:?}");
}

#[test]
fn a_lone_unit_walks_round_its_own_maw_standing_in_the_open() {
    // Each of these jammed against the Maw for good, or squeezed through
    // its body, while their route ran straight under it.
    for kind in [
        UnitKind::Lancer,
        UnitKind::Mantle,
        UnitKind::Sentinel,
        UnitKind::Warden,
        UnitKind::Harvester,
    ] {
        let mut state = staged(
            field(),
            vec![unit(0, UnitKind::Maw, 23, MID), unit(0, kind, 9, MID)],
        );
        let moved = walk_past(&mut state, 500);
        assert_eq!(moved, Vec2Fx::ZERO, "{kind:?} went round, the Maw held");
    }
}

#[test]
fn a_crowd_walks_round_its_own_maw_without_squeezing_through() {
    let mut units = vec![unit(0, UnitKind::Maw, 23, MID)];
    units.extend(crowd([8, 10, 12, 9, 11]));
    let mut state = staged(field(), units);
    assert_eq!(walk_past(&mut state, 500), Vec2Fx::ZERO);
}

#[test]
fn a_maw_holds_its_ground_in_a_pass_with_room_to_go_round() {
    for width in [4, 5] {
        let mut units = vec![unit(0, UnitKind::Maw, 23, MID)];
        units.extend(crowd([7, 9, 11, 13, 5]));
        let mut state = staged(pass(width), units);
        assert_eq!(walk_past(&mut state, 500), Vec2Fx::ZERO, "width {width}");
    }
}

#[test]
fn a_parked_maw_pulls_ahead_out_of_a_pass_too_narrow_to_go_round() {
    let mut units = vec![unit(0, UnitKind::Maw, 23, MID)];
    units.extend(crowd([7, 9, 11, 13, 5]));
    let mut state = staged(pass(3), units);
    let maw = maw_of(&state, 0);
    let moved = walk_past(&mut state, 600);
    // It leads the column out of the pass rather than backing into it.
    assert!(moved.x > Fx::from_num(5), "the Maw drove ahead: {moved:?}");
    common::run(&mut state, 200);
    let maw = state.unit(maw).unwrap();
    assert!(maw.path.is_none(), "the Maw stopped where it made way");
    assert_eq!(
        maw.order,
        Order::Idle,
        "making way leaves its orders as they were"
    );
}

#[test]
fn only_its_own_side_routes_round_a_maw() {
    let mut state = staged(
        field(),
        vec![
            unit(0, UnitKind::Maw, 23, MID),
            unit(0, UnitKind::Lancer, 9, MID - 1),
            unit(1, UnitKind::Lancer, 9, MID + 1),
        ],
    );
    let [own, enemy] = [0, 1].map(|player| movers_of(&state, player)[0]);
    state.tick(&[
        cmd(
            0,
            Command::Run {
                units: vec![own],
                goal: TilePos::new(40, MID - 1),
                queue: false,
            },
        ),
        cmd(
            1,
            Command::Run {
                units: vec![enemy],
                goal: TilePos::new(40, MID + 1),
                queue: false,
            },
        ),
    ]);
    let hull = state.unit(maw_of(&state, 0)).unwrap();
    let under_the_maw = |id: UnitId| {
        let body = state.unit(id).unwrap();
        let touch = hull.kind.stats().radius + body.kind.stats().radius;
        body.path
            .as_ref()
            .unwrap()
            .waypoints
            .iter()
            .any(|t| t.center().dist_sq(hull.pos) < touch * touch)
    };
    assert!(!under_the_maw(own), "its own Lancer steps round it");
    // Steering round an enemy before contact would leak where it stands.
    assert!(under_the_maw(enemy), "an enemy's route ignores it");
}

#[test]
fn mirrored_maws_make_mirrored_way() {
    // The narrow pass above for seat 0 and its half-turn below for seat 1.
    let upper = pass(3);
    let map: Vec<String> = upper
        .iter()
        .cloned()
        .chain(upper.iter().rev().map(|row| row.chars().rev().collect()))
        .collect();
    let (width, height) = (
        i32::try_from(WIDTH).unwrap(),
        i32::try_from(map.len()).unwrap(),
    );
    let mirror = |tile: TilePos| TilePos::new(width - 1 - tile.x, height - 1 - tile.y);
    let mirror_pos =
        |pos: Vec2Fx| Vec2Fx::new(Fx::from_num(width) - pos.x, Fx::from_num(height) - pos.y);
    let mut units = Vec::new();
    for west in std::iter::once(unit(0, UnitKind::Maw, 23, MID)).chain(crowd([7, 9, 11, 13, 5])) {
        let east = mirror(TilePos::new(west.x, west.y));
        units.push(UnitSpec {
            player: 1,
            x: east.x,
            y: east.y,
            ..west
        });
        units.push(west);
    }
    let seat = |player: u8| PlayerSpec {
        name: format!("Seat {player}"),
        team: None,
        scrap: 0,
        bot: false,
        bot_config: None,
    };
    let mut state = Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Sandbox,
        name: "mirrored-passes".into(),
        map,
        players: vec![seat(0), seat(1)],
        units,
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .unwrap();
    let pairs: Vec<(UnitId, UnitId)> = state
        .units()
        .chunks(2)
        .map(|pair| (pair[1].id, pair[0].id))
        .collect();
    let [west, east] = [0, 1].map(|player| movers_of(&state, player));
    state.tick(&[
        cmd(
            0,
            Command::Run {
                units: west,
                goal: GOAL,
                queue: false,
            },
        ),
        cmd(
            1,
            Command::Run {
                units: east,
                goal: mirror(GOAL),
                queue: false,
            },
        ),
    ]);
    let maw = maw_of(&state, 0);
    let start = state.unit(maw).unwrap().pos;
    for tick in 1..=600 {
        for &(w, e) in &pairs {
            let (a, b) = (state.unit(w).unwrap(), state.unit(e).unwrap());
            assert_eq!(mirror_pos(a.pos), b.pos, "tick {tick}: {w:?} and {e:?}");
            assert_eq!(a.path.is_some(), b.path.is_some(), "tick {tick}: {w:?}");
        }
        state.tick(&[]);
    }
    assert!(
        state.unit(maw).unwrap().pos.x - start.x > Fx::from_num(5),
        "the Maws made way"
    );
}

#[test]
fn a_parked_maw_pulls_off_the_spot_its_own_unit_is_sent_to() {
    let mut state = staged(
        field(),
        vec![
            unit(0, UnitKind::Maw, 23, MID),
            unit(0, UnitKind::Lancer, 9, MID),
        ],
    );
    let maw = maw_of(&state, 0);
    let lancer = movers_of(&state, 0)[0];
    let spot = TilePos::new(23, MID);
    state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![lancer],
            goal: spot,
            queue: false,
        },
    )]);
    common::run_until(&mut state, 500, |state, _| {
        let (hull, body) = (state.unit(maw).unwrap(), state.unit(lancer).unwrap());
        assert!(!inside(hull, body), "the Lancer is inside the Maw");
        body.order == Order::Idle
    });
    assert_eq!(state.unit(lancer).unwrap().tile(), spot);
    // It drove on, away from the Lancer, rather than back into it.
    assert!(state.unit(maw).unwrap().pos.x > spot.center().x);
}
