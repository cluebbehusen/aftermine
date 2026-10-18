//! Tiered maps: walkable levels joined by ramps and parted by cliffs.
//! Ground climbs only through a ramp, aircraft cross cliffs as open sky,
//! and a ramp holds walls and mines, never a foundation.

use crate::common;
use common::{building, cmd, players, run, unit};

use chassis::grid::TilePos;
use oxide_sim::map::{Elevation, Terrain};
use oxide_sim::scenario::{ScenarioMode, UnitSpec};
use oxide_sim::stats::Domain;
use oxide_sim::{BuildingKind, Command, PlaceRefusal, PlayerId, Scenario, State, UnitKind};

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
        // Border rows take the level of the band above them.
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
                ('=', level - 1)
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
    assert_eq!(map.elevation(cliff), Some(Elevation::Level(0)));
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
    // Every former ramp tile becomes cliff, which stores the lower level a
    // ramp already carries.
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
