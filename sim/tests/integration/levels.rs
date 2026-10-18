//! Tiered maps: walkable levels joined by ramps and parted by cliffs.
//! Ground climbs only through a ramp, aircraft cross cliffs as open sky,
//! and a ramp holds walls and mines, never a foundation.

use crate::common;
use common::{building, cmd, players, run, run_until, unit};

use chassis::fx::Vec2Fx;
use chassis::grid::TilePos;
use oxide_sim::map::{Elevation, Terrain};
use oxide_sim::scenario::{ScenarioMode, UnitSpec};
use oxide_sim::stats::Domain;
use oxide_sim::{
    BuildingKind, Command, Event, PlaceRefusal, PlayerId, Scenario, State, Target, UnitKind,
};

const WIDTH: i32 = 24;

/// The first row of band `band`, counted from the top.
fn band_top(band: i32) -> i32 {
    1 + 4 * band
}

/// The cliff row under band `band`.
fn cliff_row(band: i32) -> i32 {
    band_top(band) + 3
}

/// The ramp's columns through the cliff under band `band`, alternating
/// sides so a climb zigzags.
fn ramp_columns(band: i32) -> std::ops::Range<i32> {
    if band % 2 == 0 { 10..13 } else { 15..18 }
}

/// `count` levels stacked as three-row bands, highest at the top, each
/// parted from the next by a cliff row with one three-wide ramp. Both
/// Foundries stand on the lowest band.
pub(crate) fn stair(count: i32, units: Vec<UnitSpec>) -> Scenario {
    let height = 4 * count + 1;
    let level_of_row = |y: i32| -> i32 {
        // Border and cliff rows take the level of the band above them.
        let band = ((y - 1).max(0) / 4).min(count - 1);
        count - 1 - band
    };
    let mut map = Vec::new();
    let mut levels = Vec::new();
    for y in 0..height {
        let mut row = String::new();
        let mut glyphs = String::new();
        let band = (y - 1) / 4;
        let cliff = y > 0 && y < height - 1 && (y - 1) % 4 == 3;
        for x in 0..WIDTH {
            let level = level_of_row(y);
            let (c, glyph_level) = if x == 0 || x == WIDTH - 1 || y == 0 || y == height - 1 {
                ('#', level)
            } else if cliff && ramp_columns(band).contains(&x) {
                ('/', level - 1)
            } else if cliff {
                ('=', level)
            } else if (x, y) == (1, band_top(count - 1)) {
                ('1', level)
            } else if (x, y) == (WIDTH - 3, band_top(count - 1)) {
                ('2', level)
            } else {
                ('.', level)
            };
            row.push(c);
            glyphs.push(char::from_digit(u32::try_from(glyph_level).unwrap(), 36).unwrap());
        }
        map.push(row);
        levels.push(glyphs);
    }
    Scenario {
        levels,
        mode: ScenarioMode::Match,
        name: "stair".into(),
        map,
        players: players(300),
        units,
        buildings: Vec::new(),
        meta: None,
    }
}

fn run_to(state: &mut State, walker: oxide_sim::UnitId, goal: TilePos) {
    state.tick(&[cmd(
        0,
        Command::Run {
            units: vec![walker],
            goal,
            queue: false,
        },
    )]);
}

#[test]
fn the_levels_layer_parses_and_round_trips() {
    let scenario = stair(2, vec![]);
    let state = scenario.build().unwrap();
    let map = state.map();
    assert!(map.tiered());
    assert_eq!(map.elevation(TilePos::new(5, 2)), Some(Elevation::Level(1)));
    assert_eq!(map.elevation(TilePos::new(5, 6)), Some(Elevation::Level(0)));
    let cliff = TilePos::new(5, cliff_row(0));
    assert_eq!(map.tile(cliff).unwrap().terrain, Terrain::Cliff);
    assert_eq!(map.elevation(cliff), Some(Elevation::Level(1)));
    assert_eq!(
        map.elevation(TilePos::new(10, cliff_row(0))),
        Some(Elevation::Ramp { low: 0 })
    );
    let terrain: Vec<String> = scenario
        .map
        .iter()
        .map(|row| row.replace(['1', '2'], "."))
        .collect();
    assert_eq!(map.ascii_rows(), terrain, "anchors print as ground");
    assert_eq!(map.level_rows(), scenario.levels);
}

#[test]
fn ground_climbs_only_through_the_ramp() {
    let mut state = stair(2, vec![unit(0, UnitKind::Scuttler, 4, 6)])
        .build()
        .unwrap();
    let walker = state.units()[0].id;
    let goal = TilePos::new(4, 2);
    run_to(&mut state, walker, goal);
    let mut crossed = Vec::new();
    for _ in 0..400 {
        state.tick(&[]);
        let tile = state.unit(walker).unwrap().tile();
        if tile.y == cliff_row(0) {
            crossed.push(tile.x);
        }
    }
    assert_eq!(state.unit(walker).unwrap().tile(), goal);
    assert!(
        !crossed.is_empty(),
        "the climb passed through the cliff row"
    );
    assert!(
        crossed.iter().all(|x| ramp_columns(0).contains(x)),
        "every step through the cliff row was on the ramp, got {crossed:?}"
    );
}

#[test]
fn aircraft_cross_cliffs_as_open_sky() {
    let mut state = stair(2, vec![unit(0, UnitKind::Talon, 4, 7)])
        .build()
        .unwrap();
    let flyer = state.units()[0].id;
    let cliff = TilePos::new(4, cliff_row(0));
    assert!(!state.passable(cliff));
    assert!(state.passable_for(Domain::Air, cliff));
    let goal = TilePos::new(4, 1);
    run_to(&mut state, flyer, goal);
    let mut widest = 0;
    for _ in 0..200 {
        state.tick(&[]);
        widest = widest.max((state.unit(flyer).unwrap().tile().x - 4).abs());
    }
    assert_eq!(state.unit(flyer).unwrap().tile(), goal);
    assert!(
        widest <= 1,
        "the flyer went straight over, strayed {widest}"
    );
}

#[test]
fn a_ramp_takes_walls_and_mines_but_no_foundation() {
    let mut scenario = stair(
        2,
        vec![
            unit(0, UnitKind::Harvester, 11, 5),
            unit(0, UnitKind::Harvester, 11, 7),
        ],
    );
    scenario.buildings = vec![building(0, BuildingKind::Fabricator, 5, 6)];
    let state = scenario.build().unwrap();
    let ramp = TilePos::new(11, cliff_row(0));
    let cliff = TilePos::new(8, cliff_row(0));
    let me = PlayerId(0);
    for kind in [BuildingKind::Barricade, BuildingKind::ScuttleCharge] {
        assert_eq!(state.place_refusal(me, kind, ramp), None, "{kind:?}");
        assert_eq!(
            state.place_intent_refusal_replacing(me, kind, ramp, &[]),
            None
        );
    }
    for anchor in [ramp, TilePos::new(10, cliff_row(0) - 1)] {
        assert_eq!(
            state.place_refusal(me, BuildingKind::Turret, anchor),
            if anchor == ramp {
                Some(PlaceRefusal::Terrain)
            } else {
                None
            },
            "a turret beside the ramp is fine, on it is not"
        );
    }
    assert_eq!(
        state.place_intent_refusal_replacing(me, BuildingKind::Turret, ramp, &[]),
        Some(PlaceRefusal::Terrain)
    );
    assert_eq!(
        state.place_refusal(
            me,
            BuildingKind::Fabricator,
            TilePos::new(11, cliff_row(0) - 1)
        ),
        Some(PlaceRefusal::Terrain),
        "a footprint that only overlaps the ramp is refused too"
    );
    for kind in [BuildingKind::Barricade, BuildingKind::Turret] {
        assert_eq!(
            state.place_refusal(me, kind, cliff),
            Some(PlaceRefusal::Terrain)
        );
    }
}

#[test]
fn scenario_buildings_keep_the_ramp_rule() {
    let ramp = (11, cliff_row(0));
    let mut walled = stair(2, vec![]);
    walled.buildings = vec![building(0, BuildingKind::Barricade, ramp.0, ramp.1)];
    assert!(walled.build().is_ok());
    let mut turret = stair(2, vec![]);
    turret.buildings = vec![building(0, BuildingKind::Turret, ramp.0, ramp.1)];
    assert!(turret.build().is_err());
}

#[test]
fn a_plateau_without_a_ramp_still_builds_through_the_sky() {
    let mut scenario = stair(2, vec![]);
    for row in &mut scenario.map {
        *row = row.replace('/', "=");
    }
    // Every former ramp tile becomes cliff, which stores the upper level.
    let y = usize::try_from(cliff_row(0)).unwrap();
    scenario.levels[y] = scenario.levels[y].replace('0', "1");
    // Raise the second Foundry onto the cut-off plateau.
    let top = usize::try_from(band_top(0)).unwrap();
    let bottom = usize::try_from(band_top(1)).unwrap();
    scenario.map[bottom] = scenario.map[bottom].replace('2', ".");
    scenario.map[top].replace_range(5..6, "2");
    assert!(
        scenario.build().is_ok(),
        "aircraft reach the plateau, so the seats still connect"
    );
}

#[test]
fn six_levels_climb_end_to_end() {
    let count = 6;
    let start = TilePos::new(4, band_top(count - 1) + 2);
    let mut state = stair(count, vec![unit(0, UnitKind::Scuttler, start.x, start.y)])
        .build()
        .unwrap();
    assert_eq!(
        state.map().elevation(TilePos::new(4, 2)),
        Some(Elevation::Level(5))
    );
    let walker = state.units()[0].id;
    let goal = TilePos::new(4, 2);
    run_to(&mut state, walker, goal);
    run(&mut state, 1500);
    assert_eq!(state.unit(walker).unwrap().tile(), goal);
}

#[test]
fn a_tiered_map_reproduces_bit_identically() {
    let scenario = stair(
        3,
        vec![
            unit(0, UnitKind::Sentinel, 4, 2),
            unit(0, UnitKind::Talon, 6, 3),
            unit(1, UnitKind::Sentinel, 16, 10),
        ],
    );
    let mut a = scenario.build().unwrap();
    let mut b = scenario.build().unwrap();
    let hunt = |state: &State| {
        vec![cmd(
            0,
            Command::Hunt {
                units: state
                    .units()
                    .iter()
                    .filter(|u| u.player == PlayerId(0))
                    .map(|u| u.id)
                    .collect(),
                goal: TilePos::new(16, 10),
                queue: false,
            },
        )]
    };
    let first = hunt(&a);
    a.tick(&first);
    let second = hunt(&b);
    b.tick(&second);
    for _ in 0..300 {
        a.tick(&[]);
        b.tick(&[]);
    }
    assert_eq!(a.hash(), b.hash());
    let restored: State = serde_json::from_str(&serde_json::to_string(&a).unwrap()).unwrap();
    assert_eq!(restored.hash(), a.hash());
}

fn attack(
    state: &mut State,
    player: u8,
    units: Vec<oxide_sim::UnitId>,
    target: Target,
) -> Vec<Event> {
    state
        .tick(&[cmd(
            player,
            Command::Attack {
                units,
                target: target.into(),
                queue: false,
            },
        )])
        .events
}

/// Whether bodies at the two points share a level.
fn level_mates(state: &State, a: Vec2Fx, b: Vec2Fx) -> bool {
    state
        .map()
        .shares_level(TilePos::containing(a), TilePos::containing(b))
}

/// Every shell `gun` launched lands no higher than the gun stands.
fn launches_fall(state: &State, events: &[Event], gun: oxide_sim::UnitId) -> usize {
    let mut launched = 0;
    for event in events {
        if let Event::ShellLaunched {
            shooter, from, to, ..
        } = event
            && *shooter == Target::Unit(gun)
        {
            let (_, gun_high) = state
                .map()
                .elevation(TilePos::containing(*from))
                .unwrap()
                .span();
            let (impact_low, _) = state
                .map()
                .elevation(TilePos::containing(*to))
                .unwrap()
                .span();
            assert!(
                impact_low <= gun_high,
                "a shell from {from:?} rose to {to:?}"
            );
            launched += 1;
        }
    }
    launched
}

#[test]
fn a_chase_climbs_the_ramp_and_fires_from_it() {
    // The victim stands at the cliff's foot, two tiles below the gun on the
    // rim and in range of it, but the rim is another level. The first shot comes
    // from the ramp, the nearest ground that shares the victim's level.
    let mut state = stair(
        2,
        vec![
            unit(0, UnitKind::Sentinel, 8, 3),
            unit(1, UnitKind::Harvester, 8, 5),
        ],
    )
    .build()
    .unwrap();
    let (gun, victim) = (state.units()[0].id, state.units()[1].id);
    attack(&mut state, 0, vec![gun], Target::Unit(victim));
    let events = run_until(&mut state, 400, |_, events| {
        events
            .iter()
            .any(|e| matches!(e, Event::AttackHit { attacker, .. } if *attacker == gun))
    });
    let Some(Event::AttackHit { attacker_pos, .. }) = events
        .iter()
        .find(|e| matches!(e, Event::AttackHit { attacker, .. } if *attacker == gun))
    else {
        unreachable!("run_until stopped on the hit");
    };
    assert!(
        state.map().is_ramp(TilePos::containing(*attacker_pos)),
        "the first shot left the ramp, not {attacker_pos:?}"
    );
}

#[test]
fn direct_fire_never_crosses_between_levels() {
    let mut forces = Vec::new();
    for x in [4, 7, 14, 17] {
        forces.push(unit(0, UnitKind::Sentinel, x, 3));
        forces.push(unit(1, UnitKind::Sentinel, x, 5));
    }
    let mut state = stair(2, forces).build().unwrap();
    let hunt = |player: u8, state: &State, goal: TilePos| {
        cmd(
            player,
            Command::Hunt {
                units: state
                    .units()
                    .iter()
                    .filter(|u| u.player == PlayerId(player))
                    .map(|u| u.id)
                    .collect(),
                goal,
                queue: false,
            },
        )
    };
    let orders = [
        hunt(0, &state, TilePos::new(11, 6)),
        hunt(1, &state, TilePos::new(11, 2)),
    ];
    let mut events = state.tick(&orders).events;
    events.extend(run(&mut state, 900));
    let mut hits = 0;
    for event in &events {
        if let Event::AttackHit {
            attacker_pos,
            target_pos,
            target: Some(Target::Unit(_)),
            ..
        } = event
        {
            assert!(
                level_mates(&state, *attacker_pos, *target_pos),
                "a shot from {attacker_pos:?} reached {target_pos:?} on another level"
            );
            hits += 1;
        }
    }
    assert!(hits > 0, "the armies met through the ramp");
}

#[test]
fn artillery_fires_down_and_climbs_to_fire_up() {
    let mut high = stair(
        2,
        vec![
            unit(0, UnitKind::Bombard, 5, 2),
            unit(1, UnitKind::Harvester, 5, 6),
        ],
    )
    .build()
    .unwrap();
    let (gun, victim) = (high.units()[0].id, high.units()[1].id);
    let mut events = attack(&mut high, 0, vec![gun], Target::Unit(victim));
    events.extend(run(&mut high, 60));
    assert!(
        launches_fall(&high, &events, gun) > 0,
        "the plateau gun fires down"
    );

    let mut low = stair(
        2,
        vec![
            unit(0, UnitKind::Bombard, 5, 6),
            unit(1, UnitKind::Harvester, 5, 2),
        ],
    )
    .build()
    .unwrap();
    let (gun, victim) = (low.units()[0].id, low.units()[1].id);
    let mut events = attack(&mut low, 0, vec![gun], Target::Unit(victim));
    events.extend(run(&mut low, 30));
    assert_eq!(
        launches_fall(&low, &events, gun),
        0,
        "the floor gun holds fire"
    );
    events.extend(run(&mut low, 400));
    assert!(
        launches_fall(&low, &events, gun) > 0,
        "the gun climbs until its shells fall"
    );
}

#[test]
fn a_lead_never_carries_a_shell_onto_higher_ground() {
    // A harvester walks up the ramp under fire from the floor. Leading it
    // would aim at the plateau ahead; each shell lands at its current spot
    // instead, or is not fired.
    let mut state = stair(
        2,
        vec![
            unit(0, UnitKind::Bombard, 4, 7),
            unit(0, UnitKind::Talon, 9, 5),
            unit(1, UnitKind::Harvester, 11, 7),
        ],
    )
    .build()
    .unwrap();
    let (gun, victim) = (state.units()[0].id, state.units()[2].id);
    state.tick(&[cmd(
        1,
        Command::Run {
            units: vec![victim],
            goal: TilePos::new(11, 1),
            queue: false,
        },
    )]);
    let mut events = attack(&mut state, 0, vec![gun], Target::Unit(victim));
    events.extend(run(&mut state, 200));
    assert!(
        launches_fall(&state, &events, gun) > 0,
        "the gun fired during the climb"
    );
}

#[test]
fn a_bomb_at_the_cliff_foot_spares_the_level_above() {
    // Both bystanders stand two tiles from the impact, inside the bomb's
    // splash; only the one on the impact's level is caught.
    let mut state = stair(
        2,
        vec![
            unit(0, UnitKind::Condor, 2, 6),
            unit(1, UnitKind::Harvester, 8, 5),
            unit(1, UnitKind::Harvester, 8, 3),
            unit(1, UnitKind::Harvester, 6, 5),
        ],
    )
    .build()
    .unwrap();
    let ids: Vec<_> = state.units().iter().map(|u| u.id).collect();
    let (bomber, victim, above, beside) = (ids[0], ids[1], ids[2], ids[3]);
    let full = UnitKind::Harvester.stats().max_hp;
    attack(&mut state, 0, vec![bomber], Target::Unit(victim));
    run_until(&mut state, 900, |_, events| {
        events
            .iter()
            .any(|e| matches!(e, Event::ShellLanded { .. }))
    });
    let hp = |id| state.unit(id).map_or(0, |u| u.hp);
    assert!(
        hp(victim) < full && hp(beside) < full,
        "the floor took the blast"
    );
    assert_eq!(hp(above), full, "the plateau did not");
}

#[test]
fn a_rim_turret_ignores_the_floor_but_covers_the_ramp() {
    let scene = |x: i32, y: i32| {
        let mut scenario = stair(2, vec![unit(1, UnitKind::Harvester, x, y)]);
        scenario.buildings = vec![building(0, BuildingKind::Turret, 9, 3)];
        let mut state = scenario.build().unwrap();
        let events = run(&mut state, 60);
        events
            .iter()
            .any(|e| matches!(e, Event::TurretFired { .. }))
    };
    assert!(!scene(9, 5), "the floor below the rim is another level");
    assert!(scene(10, 4), "the ramp shares the rim's level");
}

#[test]
fn shelled_from_above_a_unit_climbs_to_answer() {
    let mut state = stair(
        2,
        vec![
            unit(1, UnitKind::Bombard, 3, 2),
            unit(0, UnitKind::Sentinel, 3, 6),
        ],
    )
    .build()
    .unwrap();
    let (gun, answer) = (state.units()[0].id, state.units()[1].id);
    let mut events = attack(&mut state, 1, vec![gun], Target::Unit(answer));
    events.extend(run_until(&mut state, 900, |_, events| {
        events.iter().any(|e| {
            matches!(e, Event::AttackHit { attacker, target: Some(Target::Unit(t)), .. }
                if *attacker == answer && *t == gun)
        })
    }));
    assert!(
        events.iter().any(
            |e| matches!(e, Event::ShellLaunched { shooter, .. } if *shooter == Target::Unit(gun))
        ),
        "the answer came after the shelling"
    );
}

/// Two plateaus at level 1 flanking a floor at level 0, each joined to it
/// by a ramp; the whole map is its own half-turn.
fn mesas(units: Vec<UnitSpec>) -> Scenario {
    let (width, height) = (24, 11);
    let mut map = Vec::new();
    let mut levels = Vec::new();
    for y in 0..height {
        let mut row = String::new();
        let mut glyphs = String::new();
        for x in 0..width {
            let raised = x <= 7 || x >= 16;
            let ramp = (x == 7 && (2..=4).contains(&y)) || (x == 16 && (6..=8).contains(&y));
            row.push(if x == 0 || x == width - 1 || y == 0 || y == height - 1 {
                '#'
            } else if ramp {
                '/'
            } else if x == 7 || x == 16 {
                '='
            } else if (x, y) == (1, 1) {
                '1'
            } else if (x, y) == (21, 8) {
                '2'
            } else {
                '.'
            });
            glyphs.push(if raised && !ramp { '1' } else { '0' });
        }
        map.push(row);
        levels.push(glyphs);
    }
    Scenario {
        levels,
        mode: ScenarioMode::Match,
        name: "mesas".into(),
        map,
        players: players(300),
        units,
        buildings: Vec::new(),
        meta: None,
    }
}

#[test]
fn mirrored_ramp_duels_end_in_mutual_annihilation() {
    let mut state = mesas(vec![
        unit(0, UnitKind::Sentinel, 3, 5),
        unit(1, UnitKind::Sentinel, 20, 5),
    ])
    .build()
    .unwrap();
    let (a, b) = (state.units()[0].id, state.units()[1].id);
    state.tick(&[
        cmd(
            0,
            Command::Hunt {
                units: vec![a],
                goal: TilePos::new(20, 5),
                queue: false,
            },
        ),
        cmd(
            1,
            Command::Hunt {
                units: vec![b],
                goal: TilePos::new(3, 5),
                queue: false,
            },
        ),
    ]);
    run_until(&mut state, 900, |s, _| {
        s.unit(a).is_none() || s.unit(b).is_none()
    });
    assert!(
        state.unit(a).is_none() && state.unit(b).is_none(),
        "mirrored climbs meet as equals"
    );
}
