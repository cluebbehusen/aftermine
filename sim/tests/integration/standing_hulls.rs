//! A friendly Maw standing in a route: small bodies walk round it where
//! there is room, whatever their errand, a parked one pulls aside where
//! there is none, and no body is ever pushed inside its hull. A Harvester's
//! way round keeps to what its team knows of the ground and its dangers.
use crate::common;
use aftermine_sim::scenario::{BuildingSpec, PlayerSpec, ScenarioMode, UnitSpec};
use aftermine_sim::stats::{COLLISION_MAX_STEP, Domain};
use aftermine_sim::{
    AttackTarget, BuildingKind, Event, Order, PlayerCommand, PlayerId, Scenario, State, Unit,
    UnitId, UnitKind,
};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;
use common::{building, cmd, orders, players, unit, unit_id};

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

/// `map` with each tile `mark` names overwritten by the character it returns.
fn carved(map: Vec<String>, mark: impl Fn(TilePos) -> Option<char>) -> Vec<String> {
    map.into_iter()
        .enumerate()
        .map(|(y, row)| {
            row.chars()
                .enumerate()
                .map(|(x, c)| {
                    let tile = TilePos::new(i32::try_from(x).unwrap(), i32::try_from(y).unwrap());
                    mark(tile).unwrap_or(c)
                })
                .collect()
        })
        .collect()
}

fn staged(map: Vec<String>, units: Vec<UnitSpec>) -> State {
    scene(map, units, Vec::new(), 0)
}

/// [`staged`] with structures, each seat banking `scrap`.
fn scene(
    map: Vec<String>,
    units: Vec<UnitSpec>,
    buildings: Vec<BuildingSpec>,
    scrap: u32,
) -> State {
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Sandbox,
        name: "standing-hulls".into(),
        map,
        players: players(scrap),
        units,
        buildings,
        meta: None,
    }
    .build()
    .unwrap()
}

/// Rewrites one unit's serialized row, the way a save would carry it.
fn edit(state: &mut State, id: UnitId, poke: impl FnOnce(&mut serde_json::Value)) {
    let mut data = serde_json::to_value(&*state).unwrap();
    let row = data["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|u| u["id"] == serde_json::json!(id))
        .unwrap();
    poke(row);
    *state = serde_json::from_value(data).unwrap();
}

fn maw_of(state: &State, player: u8) -> UnitId {
    unit_id(state, |u| {
        u.kind == UnitKind::Maw && u.player == PlayerId(player)
    })
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
    state.tick(&[cmd(0, orders::run(movers.clone(), GOAL))]);
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

/// Gives `orders` and runs until `done` holds, checking every tick that no
/// ground body stands inside seat 0's Maw. Returns the Maw's displacement by
/// then.
fn run_past(
    state: &mut State,
    mut orders: Vec<PlayerCommand>,
    max_ticks: u64,
    mut done: impl FnMut(&State, &[Event]) -> bool,
) -> Vec2Fx {
    let maw = maw_of(state, 0);
    let start = state.unit(maw).unwrap().pos;
    for tick in 1..=max_ticks {
        let report = state.tick(&std::mem::take(&mut orders));
        let hull = state.unit(maw).unwrap();
        for body in state
            .units()
            .iter()
            .filter(|u| u.id != maw && u.domain() == Domain::Ground)
        {
            assert!(
                !inside(hull, body),
                "tick {tick}: {:?} is inside the Maw",
                body.id
            );
        }
        if done(state, &report.events) {
            return hull.pos - start;
        }
    }
    panic!("not done within {max_ticks} ticks");
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
        cmd(0, orders::run([own], TilePos::new(40, MID - 1))),
        cmd(1, orders::run([enemy], TilePos::new(40, MID + 1))),
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
        cmd(0, orders::run(west, GOAL)),
        cmd(1, orders::run(east, mirror(GOAL))),
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
    state.tick(&[cmd(0, orders::run([lancer], spot))]);
    common::run_until(&mut state, 500, |state, _| {
        let (hull, body) = (state.unit(maw).unwrap(), state.unit(lancer).unwrap());
        assert!(!inside(hull, body), "the Lancer is inside the Maw");
        body.order == Order::Idle
    });
    assert_eq!(state.unit(lancer).unwrap().tile(), spot);
    // It drove on, away from the Lancer, rather than back into it.
    assert!(state.unit(maw).unwrap().pos.x > spot.center().x);
}

#[test]
fn a_harvester_walks_round_its_own_maw_to_scrap_and_back() {
    let node = TilePos::new(29, MID);
    let mut state = scene(
        carved(field(), |tile| (tile == node).then_some('s')),
        vec![
            unit(0, UnitKind::Maw, 23, MID),
            unit(0, UnitKind::Harvester, 9, MID),
        ],
        vec![building(0, BuildingKind::Foundry, 3, MID)],
        0,
    );
    let harvester = movers_of(&state, 0)[0];
    let order = cmd(0, orders::harvest([harvester], node));
    let moved = run_past(&mut state, vec![order], 1_200, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::ScrapDeposited { unit, .. } if *unit == harvester))
    });
    assert_eq!(moved, Vec2Fx::ZERO, "it went round both ways, the Maw held");
}

#[test]
fn a_welder_walks_round_its_own_maw_to_a_wounded_machine() {
    let mut state = scene(
        field(),
        vec![
            unit(0, UnitKind::Maw, 23, MID),
            unit(0, UnitKind::Harvester, 9, MID),
            unit(0, UnitKind::Sentinel, 30, MID),
        ],
        Vec::new(),
        500,
    );
    let [welder, patient] = movers_of(&state, 0)[..] else {
        unreachable!("a welder and a patient")
    };
    let wounded = UnitKind::Sentinel.stats().max_hp / 2;
    edit(&mut state, patient, |row| {
        row["hp"] = serde_json::json!(wounded);
    });
    let order = cmd(0, orders::repair_unit([welder], patient));
    let moved = run_past(&mut state, vec![order], 500, |state, _| {
        state.unit(patient).unwrap().hp > wounded
    });
    assert_eq!(moved, Vec2Fx::ZERO, "the welder went round, the Maw held");
}

#[test]
fn a_machine_walks_round_its_own_maw_to_board_a_transport() {
    let mut state = staged(
        field(),
        vec![
            unit(0, UnitKind::Maw, 23, MID),
            unit(0, UnitKind::Sentinel, 9, MID),
            unit(0, UnitKind::Skyhook, 31, MID),
        ],
    );
    let [rider, transport] = movers_of(&state, 0)[..] else {
        unreachable!("a rider and a transport")
    };
    let order = cmd(0, orders::load([rider], transport));
    let moved = run_past(&mut state, vec![order], 500, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::UnitBoarded { unit, .. } if *unit == rider))
    });
    assert_eq!(moved, Vec2Fx::ZERO, "the rider went round, the Maw held");
}

#[test]
fn a_ranged_unit_walks_round_its_own_maw_to_fire_on_a_structure() {
    let mut state = scene(
        field(),
        vec![
            unit(0, UnitKind::Maw, 23, MID),
            unit(0, UnitKind::Sentinel, 9, MID),
        ],
        vec![building(1, BuildingKind::Barricade, 29, MID)],
        0,
    );
    let sentinel = movers_of(&state, 0)[0];
    let barricade = state.buildings()[0].id;
    let full = state.building(barricade).unwrap().hp;
    let order = cmd(
        0,
        orders::attack([sentinel], AttackTarget::Building(barricade)),
    );
    let moved = run_past(&mut state, vec![order], 500, |state, _| {
        state.building(barricade).unwrap().hp < full
    });
    assert_eq!(moved, Vec2Fx::ZERO, "the Sentinel went round, the Maw held");
}

/// Open ground with walls north and south of the Maw's lane, leaving one way
/// round it: a loop north along the edge of the ground a way round may cover.
fn chamber() -> Vec<String> {
    carved(field(), |tile| {
        let walled = (18..28).contains(&tile.x) && ((5..8).contains(&tile.y) || tile.y >= 12);
        walled.then_some('#')
    })
}

#[test]
fn a_harvesters_way_round_its_maw_bends_only_for_structures_its_team_has_seen() {
    let node = TilePos::new(29, MID);
    let unseen = TilePos::new(17, 6);
    let seen = TilePos::new(23, 4);
    // The Harvester's planned path after its first tick, and whether it
    // runs under the Maw, with a seat-1 Barricade on `barricade`.
    let plan = |barricade: Option<TilePos>| {
        let mut state = scene(
            carved(chamber(), |tile| (tile == node).then_some('s')),
            vec![
                unit(0, UnitKind::Maw, 23, MID),
                unit(0, UnitKind::Harvester, 9, MID),
            ],
            barricade
                .map(|at| building(1, BuildingKind::Barricade, at.x, at.y))
                .into_iter()
                .collect(),
            0,
        );
        let maw = maw_of(&state, 0);
        let harvester = movers_of(&state, 0)[0];
        // On a tile corner the Maw's footprint is four tiles wide, and the
        // ground a way round it may cover reaches past its own sight.
        let corner = Vec2Fx::new(Fx::from_num(23), Fx::from_num(MID));
        edit(&mut state, maw, |row| {
            row["pos"] = serde_json::to_value(corner).unwrap();
        });
        let sight = state.vision(PlayerId(0));
        assert!(!sight.visible(unseen) && sight.visible(seen));
        let order = cmd(0, orders::harvest([harvester], node));
        run_past(&mut state, vec![order], 1, |_, _| true);
        let (hull, body) = (state.unit(maw).unwrap(), state.unit(harvester).unwrap());
        let touch = hull.kind.stats().radius + body.kind.stats().radius;
        let waypoints = body.path.as_ref().unwrap().waypoints.clone();
        let under = waypoints
            .iter()
            .any(|t| t.center().dist_sq(hull.pos) < touch * touch);
        (waypoints, under)
    };
    let (open, under) = plan(None);
    assert!(
        !under && open.contains(&unseen),
        "the way round passes {unseen:?}"
    );
    assert_eq!(
        plan(Some(unseen)),
        (open.clone(), false),
        "a Barricade its team has not seen leaves the way round as it was"
    );
    let (blocked, under) = plan(Some(seen));
    assert_ne!(blocked, open);
    assert!(under, "one it has seen closes the only way round");
}

/// Sends seat 0's Maw to park across the lane at `(23, MID)` on the tick
/// `order` is given, so `mover` plans its route while the Maw still drives
/// and that route runs straight under the spot where it parks. Runs until
/// the Maw has parked, before `mover` reaches it.
fn park_across_a_fresh_route(state: &mut State, mover: UnitId, order: PlayerCommand) {
    let maw = maw_of(state, 0);
    let park = cmd(0, orders::run([maw], TilePos::new(23, MID)));
    run_past(state, vec![park, order], 300, |state, _| {
        let hull = state.unit(maw).unwrap();
        hull.path.is_none() && hull.drive_speed() == Fx::ZERO
    });
    assert!(
        TilePos::containing(state.unit(mover).unwrap().pos).x < 20,
        "the Maw parked before {mover:?} reached it"
    );
}

#[test]
fn a_maw_parking_across_a_route_planned_while_it_drove_holds_its_spot() {
    let mut state = staged(
        field(),
        vec![
            unit(0, UnitKind::Maw, 23, MID - 6),
            unit(0, UnitKind::Lancer, 9, MID),
        ],
    );
    let lancer = movers_of(&state, 0)[0];
    let order = cmd(0, orders::run([lancer], GOAL));
    park_across_a_fresh_route(&mut state, lancer, order);
    let moved = run_past(&mut state, Vec::new(), 500, |state, _| {
        TilePos::containing(state.unit(lancer).unwrap().pos).x >= THROUGH
    });
    assert_eq!(moved, Vec2Fx::ZERO, "the Lancer went round, the Maw held");
}

#[test]
fn a_maw_parking_across_a_harvest_route_planned_while_it_drove_holds_its_spot() {
    // In the Maw's sight from where it sets off.
    let node = TilePos::new(28, MID);
    let mut state = scene(
        carved(field(), |tile| (tile == node).then_some('s')),
        vec![
            unit(0, UnitKind::Maw, 23, MID - 3),
            unit(0, UnitKind::Harvester, 6, MID),
        ],
        vec![building(0, BuildingKind::Foundry, 3, MID)],
        0,
    );
    let harvester = movers_of(&state, 0)[0];
    let order = cmd(0, orders::harvest([harvester], node));
    park_across_a_fresh_route(&mut state, harvester, order);
    let moved = run_past(&mut state, Vec::new(), 1_200, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::ScrapDeposited { unit, .. } if *unit == harvester))
    });
    assert_eq!(
        moved,
        Vec2Fx::ZERO,
        "the Harvester went round, the Maw held"
    );
}

#[test]
fn a_maw_parking_across_a_scout_route_planned_while_it_drove_holds_its_spot() {
    let mut state = staged(
        field(),
        vec![
            unit(0, UnitKind::Maw, 23, MID - 3),
            unit(0, UnitKind::Surveyor, 1, MID),
        ],
    );
    let surveyor = movers_of(&state, 0)[0];
    let goal = TilePos::new(36, MID);
    let order = cmd(0, orders::scout([surveyor], goal));
    park_across_a_fresh_route(&mut state, surveyor, order);
    let moved = run_past(&mut state, Vec::new(), 400, |state, _| {
        state.unit(surveyor).unwrap().order == Order::Idle
    });
    assert_eq!(state.unit(surveyor).unwrap().tile(), goal);
    assert_eq!(moved, Vec2Fx::ZERO, "the Surveyor went round, the Maw held");
}

#[test]
fn a_maw_pulls_aside_for_a_harvester_whose_way_round_runs_through_known_fire() {
    let node = TilePos::new(29, MID);
    let mut state = scene(
        carved(field(), |tile| (tile == node).then_some('s')),
        vec![
            unit(0, UnitKind::Maw, 23, MID),
            unit(0, UnitKind::Harvester, 9, MID),
            // Hovering over the Maw, it shows its team both Turrets.
            unit(0, UnitKind::Kestrel, 23, MID),
        ],
        vec![
            building(0, BuildingKind::Foundry, 3, MID),
            // Their known reach covers the ground beside the Maw on both
            // sides but not the lane through it.
            building(1, BuildingKind::Turret, 23, MID - 8),
            building(1, BuildingKind::Turret, 23, MID + 8),
        ],
        0,
    );
    let harvester = movers_of(&state, 0)[0];
    let order = cmd(0, orders::harvest([harvester], node));
    let moved = run_past(&mut state, vec![order], 1_200, |_, events| {
        events
            .iter()
            .any(|event| matches!(event, Event::ScrapDeposited { unit, .. } if *unit == harvester))
    });
    assert_ne!(moved, Vec2Fx::ZERO, "the Maw made way");
}

#[test]
fn a_surveyor_scouts_round_its_own_maw() {
    let mut state = staged(
        field(),
        vec![
            unit(0, UnitKind::Maw, 23, MID),
            unit(0, UnitKind::Surveyor, 9, MID),
        ],
    );
    let surveyor = movers_of(&state, 0)[0];
    let goal = TilePos::new(36, MID);
    let order = cmd(0, orders::scout([surveyor], goal));
    let moved = run_past(&mut state, vec![order], 400, |state, _| {
        state.unit(surveyor).unwrap().order == Order::Idle
    });
    assert_eq!(state.unit(surveyor).unwrap().tile(), goal);
    assert_eq!(moved, Vec2Fx::ZERO, "the Surveyor went round, the Maw held");
}
