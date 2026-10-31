//! Shared scaffolding used by the focused behavior suites.
#![allow(dead_code)]

use aftermine_sim::scenario::{BuildingSpec, PlayerSpec, ScenarioMode, UnitSpec};
use aftermine_sim::{
    BuildingKind, Command, Event, Goal, Order, PlayerCommand, PlayerId, Scenario, State, UnitKind,
};

/// A small arena: two Foundries in opposite corners, open ground between.
pub fn arena(units: Vec<UnitSpec>) -> Scenario {
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "test-arena".into(),
        map: vec![
            "################".into(),
            "#1.............#".into(),
            "#..............#".into(),
            "#.....##.......#".into(),
            "#.....##...s...#".into(),
            "#..........s...#".into(),
            "#............2.#".into(),
            "#..............#".into(),
            "################".into(),
        ],
        players: players(200),
        units,
        buildings: Vec::new(),
        meta: None,
    }
}

/// Two human seats, each banking `scrap`.
pub fn players(scrap: u32) -> Vec<PlayerSpec> {
    vec![
        PlayerSpec {
            name: "West".into(),
            team: None,
            scrap,
            bot: false,
            bot_config: None,
        },
        PlayerSpec {
            name: "East".into(),
            team: None,
            scrap,
            bot: false,
            bot_config: None,
        },
    ]
}

pub fn unit(player: u8, kind: UnitKind, x: i32, y: i32) -> UnitSpec {
    UnitSpec { player, kind, x, y }
}

/// A radar blip on the ground at `tile`.
pub fn ground_blip(tile: chassis::grid::TilePos) -> aftermine_sim::RadarContact {
    aftermine_sim::RadarContact {
        tile,
        domain: aftermine_sim::stats::Domain::Ground,
    }
}

/// A radar blip in the air over `tile`.
pub fn air_blip(tile: chassis::grid::TilePos) -> aftermine_sim::RadarContact {
    aftermine_sim::RadarContact {
        tile,
        domain: aftermine_sim::stats::Domain::Air,
    }
}

pub fn building(player: u8, kind: BuildingKind, x: i32, y: i32) -> BuildingSpec {
    BuildingSpec { player, kind, x, y }
}

/// A wide open arena — Foundries tucked into opposite corners, out of
/// every lane — for movement and pursuit scenarios that need real
/// distances. `carve` edits the char grid (walls, doors) after the
/// open fill, before the anchors land.
pub fn open_arena_with(
    width: usize,
    height: usize,
    units: Vec<UnitSpec>,
    carve: impl Fn(&mut Vec<Vec<char>>),
) -> Scenario {
    let mut rows = vec![vec!['#'; width]; height];
    for row in rows.iter_mut().take(height - 1).skip(1) {
        for cell in row.iter_mut().take(width - 1).skip(1) {
            *cell = '.';
        }
    }
    carve(&mut rows);
    rows[1][1] = '1';
    rows[height - 3][width - 3] = '2';
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "open-arena".into(),
        map: rows.into_iter().map(|r| r.into_iter().collect()).collect(),
        players: vec![
            PlayerSpec {
                name: "West".into(),
                team: None,
                scrap: 0,
                bot: false,
                bot_config: None,
            },
            PlayerSpec {
                name: "East".into(),
                team: None,
                scrap: 0,
                bot: false,
                bot_config: None,
            },
        ],
        units,
        buildings: Vec::new(),
        meta: None,
    }
}

/// [`open_arena_with`] without terrain edits.
pub fn open_arena(width: usize, height: usize, units: Vec<UnitSpec>) -> Scenario {
    open_arena_with(width, height, units, |_| {})
}

pub fn cmd(player: u8, command: Command) -> PlayerCommand {
    PlayerCommand {
        player: PlayerId(player),
        command,
    }
}

/// `order` with every walking goal reduced to its commanded tile, so a test
/// compares the program a player wrote without depending on how far the
/// sim has resolved each goal.
pub fn commanded(order: Order) -> Order {
    let tile = |goal: Goal| Goal::at(goal.tile());
    match order {
        Order::Run { goal } => Order::Run { goal: tile(goal) },
        Order::Hunt { goal } => Order::Hunt { goal: tile(goal) },
        Order::Advance { goal } => Order::Advance { goal: tile(goal) },
        Order::Unload { at, reverse } => Order::Unload {
            at: tile(at),
            reverse,
        },
        Order::Attack {
            target,
            pursue,
            resume,
        } => Order::Attack {
            target,
            pursue,
            resume: resume.map(|resume| match resume {
                aftermine_sim::Resume::Hunt(goal) => aftermine_sim::Resume::Hunt(tile(goal)),
                escort @ aftermine_sim::Resume::Accompany(_) => escort,
            }),
        },
        other => other,
    }
}

/// `state` with idle welding switched off for each of `workers`, for a
/// test whose premise is a worker standing idle beside a wounded machine or
/// a damaged building.
/// The switch rides in the snapshot, so no tick passes.
pub fn idle_welds_off(state: &State, workers: &[aftermine_sim::UnitId]) -> State {
    let mut data = serde_json::to_value(state).unwrap();
    for row in data["units"].as_array_mut().unwrap() {
        if workers.iter().any(|id| row["id"] == serde_json::json!(id)) {
            row["worker"]["idle_weld_off"] = serde_json::json!(true);
        }
    }
    serde_json::from_value(data).unwrap()
}

/// Runs exactly `ticks` idle ticks, collecting every event.
pub fn run(state: &mut State, ticks: u64) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..ticks {
        all.extend(state.tick(&[]).events);
    }
    all
}

/// Runs until `stop` returns true or `max_ticks` elapse, collecting every
/// event. Panics if the condition never holds — behavior tests should state
/// exactly how long something may take.
pub fn run_until(
    state: &mut State,
    max_ticks: u64,
    mut stop: impl FnMut(&State, &[Event]) -> bool,
) -> Vec<Event> {
    let mut all = Vec::new();
    for _ in 0..max_ticks {
        let report = state.tick(&[]);
        let done = stop(state, &report.events);
        all.extend(report.events);
        if done {
            return all;
        }
    }
    panic!("condition not reached within {max_ticks} ticks");
}

/// Establish an already-aimed fixture when testing same-tick resolution.
pub fn face_toward(state: &mut State, id: aftermine_sim::UnitId, point: chassis::fx::Vec2Fx) {
    let unit = state.unit(id).unwrap();
    let direction = point - unit.pos;
    let heading = (0..=255u8)
        .max_by_key(|&step| {
            let d = chassis::compass::dir(step);
            (
                d.x * direction.x + d.y * direction.y,
                std::cmp::Reverse(step),
            )
        })
        .unwrap();
    let mut value = serde_json::to_value(&*state).unwrap();
    let row = value["units"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|u| u["id"] == serde_json::json!(id))
        .unwrap();
    row["heading"] = serde_json::json!(heading);
    if unit.kind.has_ground_turret() {
        row["turret_heading"] = serde_json::json!(heading);
    }
    *state = serde_json::from_value(value).unwrap();
}

/// Points the gun of the defense `id` at `point`, for a staged scene that
/// starts with the gun already on its target: a new defense faces the map
/// centre.
pub fn aim_defense(state: &mut State, id: aftermine_sim::BuildingId, point: chassis::fx::Vec2Fx) {
    let heading = chassis::compass::heading_of(point - state.building(id).unwrap().center());
    let mut value = serde_json::to_value(&*state).unwrap();
    let row = value["buildings"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|b| b["id"] == serde_json::json!(id))
        .unwrap();
    row["gun_heading"] = serde_json::json!(heading);
    *state = serde_json::from_value(value).unwrap();
}

/// [`aim_defense`] at a unit's position or a building's closest point.
pub fn aim_defense_at(
    state: &mut State,
    id: aftermine_sim::BuildingId,
    target: aftermine_sim::Target,
) {
    let center = state.building(id).unwrap().center();
    let point = match target {
        aftermine_sim::Target::Unit(target) => state.unit(target).unwrap().pos,
        aftermine_sim::Target::Building(target) => {
            state.building(target).unwrap().closest_point_to(center)
        }
    };
    aim_defense(state, id, point);
}

pub fn face_target(state: &mut State, id: aftermine_sim::UnitId, target: aftermine_sim::Target) {
    let unit = state.unit(id).unwrap();
    let point = match target {
        aftermine_sim::Target::Unit(target) => state.unit(target).unwrap().pos,
        aftermine_sim::Target::Building(target) => {
            state.building(target).unwrap().closest_point_to(unit.pos)
        }
    };
    face_toward(state, id, point);
}

pub(crate) fn wide_open_map() -> Vec<String> {
    vec![
        "########################".into(),
        "#1.....................#".into(),
        "#......................#".into(),
        "#......................#".into(),
        "#......................#".into(),
        "#......................#".into(),
        "#......................#".into(),
        "#...................2..#".into(),
        "#......................#".into(),
        "########################".into(),
    ]
}

pub(crate) fn drip_credits(state: &aftermine_sim::State) -> u32 {
    let period = aftermine_sim::stats::FOUNDRY_DRIP_PERIOD;
    let start = aftermine_sim::stats::FOUNDRY_DRIP_START_TICK;
    let credits_by = |tick: u64| {
        if tick < start {
            0
        } else {
            tick / period - (start / period - 1)
        }
    };
    u32::try_from(credits_by(state.current_tick())).unwrap()
}
