use super::*;
use crate::scenario::{PlayerSpec, ScenarioMode};
use chassis::fx::Fx;

/// A sandbox over `rows`, shooting along row 2 from column 1 to column 7.
fn state(rows: &[&str]) -> State {
    tiered(rows, &[])
}

/// A sandbox over `rows` with the levels layer `levels`.
fn tiered(rows: &[&str], levels: &[&str]) -> State {
    crate::Scenario {
        mode: ScenarioMode::Sandbox,
        name: "fire".into(),
        map: rows.iter().map(|row| (*row).to_string()).collect(),
        levels: levels.iter().map(|row| (*row).to_string()).collect(),
        players: vec![PlayerSpec {
            name: "solo".into(),
            team: None,
            scrap: 0,
            bot: false,
            bot_config: None,
        }],
        units: Vec::new(),
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .expect("fire scene builds")
}

fn at(x: i32, y: i32) -> Vec2Fx {
    TilePos::new(x, y).center()
}

fn shot(state: &State, fire: Fire, shooter: Domain, victim: Domain, landing: Landing) -> bool {
    clear_line(
        state,
        fire,
        ShotEnd::new(at(1, 2), shooter),
        ShotEnd::new(at(7, 2), victim),
        landing,
    )
}

#[test]
fn rock_covers_only_direct_fire_between_ground_parties() {
    let walled = state(&[
        ".........",
        ".........",
        "....#....",
        ".........",
        ".........",
    ]);
    let (ground, air) = (Domain::Ground, Domain::Air);
    assert!(!shot(
        &walled,
        Fire::Direct,
        ground,
        ground,
        Landing::Checked
    ));
    assert!(shot(
        &walled,
        Fire::Indirect,
        ground,
        ground,
        Landing::Checked
    ));
    assert!(shot(&walled, Fire::Direct, air, ground, Landing::Checked));
    assert!(shot(&walled, Fire::Direct, ground, air, Landing::Checked));
}

#[test]
fn peaks_wall_every_shot_and_pits_none() {
    let peaked = state(&[
        "....^....",
        "....^....",
        "....^....",
        "....^....",
        "....^....",
    ]);
    for fire in [Fire::Direct, Fire::Indirect] {
        for shooter in [Domain::Ground, Domain::Air] {
            assert!(!shot(
                &peaked,
                fire,
                shooter,
                Domain::Ground,
                Landing::Unchecked
            ));
        }
    }
    let pitted = state(&[
        "....~....",
        "....~....",
        "....~....",
        "....~....",
        "....~....",
    ]);
    assert!(shot(
        &pitted,
        Fire::Direct,
        Domain::Ground,
        Domain::Ground,
        Landing::Checked
    ));
}

#[test]
fn a_checked_landing_refuses_a_shot_into_a_peak() {
    let peaked = state(&[
        ".........",
        ".........",
        ".......^.",
        ".........",
        ".........",
    ]);
    let into = |landing| {
        clear_line(
            &peaked,
            Fire::Direct,
            ShotEnd::new(at(1, 2), Domain::Ground),
            ShotEnd::new(
                TilePos::new(7, 2).center() - chassis::fx::Vec2Fx::new(Fx::lit("0.4"), Fx::ZERO),
                Domain::Ground,
            ),
            landing,
        )
    };
    assert!(!into(Landing::Checked));
    assert!(into(Landing::Unchecked), "the trace itself skips end tiles");
}

#[test]
fn fire_follows_the_weapon() {
    let mut weapon = crate::UnitKind::Lancer.stats().weapons[0];
    assert_eq!(Fire::of(&weapon), Fire::Direct);
    weapon.indirect = true;
    assert_eq!(Fire::of(&weapon), Fire::Indirect);
}

fn ground(x: i32, y: i32) -> ShotEnd {
    ShotEnd::new(at(x, y), Domain::Ground)
}

/// A plateau at level 2 over a floor at level 1, a ramp at column 3
/// joining them.
fn ledge() -> State {
    tiered(
        &[
            ".........",
            ".........",
            "===/=====",
            ".........",
            ".........",
        ],
        &[
            "222222222",
            "222222222",
            "111111111",
            "111111111",
            "111111111",
        ],
    )
}

#[test]
fn direct_ground_fire_needs_a_shared_level() {
    let state = ledge();
    let (high, ramp, low) = (ground(3, 0), ground(3, 2), ground(3, 4));
    let direct = |from, to| clear_line(&state, Fire::Direct, from, to, Landing::Checked);
    assert!(
        !direct(high, low) && !direct(low, high),
        "a cliff parts the levels"
    );
    assert!(
        direct(high, ramp) && direct(ramp, high),
        "a ramp shares the level above"
    );
    assert!(
        direct(low, ramp) && direct(ramp, low),
        "and the level below"
    );
    assert!(!levels_allow(&state, Fire::Direct, high, low));
    assert!(levels_allow(&state, Fire::Direct, low, ramp));
    let air = |x, y| ShotEnd::new(at(x, y), Domain::Air);
    assert!(
        direct(air(3, 0), low) && direct(high, air(3, 4)),
        "aircraft ignore levels"
    );
}

#[test]
fn arcs_fall_from_any_height_but_never_rise() {
    let state = ledge();
    let (high, ramp, low) = (ground(3, 0), ground(3, 2), ground(3, 4));
    let arc = |from, to| clear_line(&state, Fire::Indirect, from, to, Landing::Checked);
    assert!(arc(high, low), "artillery fires down");
    assert!(!arc(low, high), "and never up");
    assert!(arc(low, ramp), "a ramp's foot is on the shooter's level");
    assert!(arc(ramp, high), "a ramp reaches the level above it");
    assert!(!levels_allow(&state, Fire::Indirect, low, high));
}

#[test]
fn higher_ground_between_blocks_direct_fire_but_not_arcs() {
    let rows = ["...=...=...", "...=...=...", "...=...=..."];
    let mesa = tiered(&rows, &["11112221111"; 3]);
    let (west, east) = (ground(1, 1), ground(9, 1));
    assert!(!clear_line(
        &mesa,
        Fire::Direct,
        west,
        east,
        Landing::Checked
    ));
    assert!(clear_line(
        &mesa,
        Fire::Indirect,
        west,
        east,
        Landing::Checked
    ));
    assert!(clear_line(
        &mesa,
        Fire::Direct,
        ShotEnd::new(at(1, 1), Domain::Air),
        east,
        Landing::Checked
    ));
    assert!(
        levels_allow(&mesa, Fire::Direct, west, east),
        "the ends share a level; only the line is blocked"
    );
    let basin = tiered(&rows, &["22211111222"; 3]);
    assert!(
        clear_line(&basin, Fire::Direct, west, east, Landing::Checked),
        "fire crosses lower ground"
    );
}

#[test]
fn a_tall_cliff_parts_levels_two_apart() {
    let state = tiered(&["...", "===", "..."], &["333", "111", "111"]);
    let (high, low) = (ground(1, 0), ground(1, 2));
    assert!(!clear_line(
        &state,
        Fire::Direct,
        high,
        low,
        Landing::Checked
    ));
    assert!(clear_line(
        &state,
        Fire::Indirect,
        high,
        low,
        Landing::Checked
    ));
    assert!(!clear_line(
        &state,
        Fire::Indirect,
        low,
        high,
        Landing::Checked
    ));
}

#[test]
fn a_building_stands_on_its_anchor_not_its_aim_tile() {
    let state = tiered(&[".....=...", ".....=...", ".....=..."], &["222221111"; 3]);
    // A plateau building's east edge floors into the cliff beside it.
    let edge = ShotEnd::new(Vec2Fx::new(Fx::from_num(5), at(4, 1).y), Domain::Ground);
    let gun = ground(1, 1);
    assert!(
        !levels_allow(&state, Fire::Direct, gun, edge),
        "the edge alone reads as the cliff's level"
    );
    assert!(levels_allow(
        &state,
        Fire::Direct,
        gun,
        edge.on(TilePos::new(4, 1))
    ));
}

#[test]
fn levels_change_nothing_where_nothing_stands_higher() {
    // Rock, peaks, and pits on a level-1 field whose only plateau lies past
    // the cliff on row 13. Lines between ends above it never reach the
    // plateau, so every verdict matches the flat map with the cliff paved.
    let mut rows: Vec<String> = Vec::new();
    let mut levels: Vec<String> = Vec::new();
    let mut rng = chassis::rng::Pcg32::new(0x1E7E_1505, 7);
    for y in 0..16 {
        let mut row = String::new();
        for _ in 0..16 {
            row.push(match y {
                13 => '=',
                14 | 15 => '.',
                _ => ['.', '.', '.', '.', '.', '#', '^', '~'][rng.next_index(8)],
            });
        }
        levels.push(if y >= 14 { "2" } else { "1" }.repeat(16));
        rows.push(row);
    }
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    let levels: Vec<&str> = levels.iter().map(String::as_str).collect();
    let flat_rows: Vec<String> = rows.iter().map(|row| row.replace('=', ".")).collect();
    let flat_rows: Vec<&str> = flat_rows.iter().map(String::as_str).collect();
    let (tiered, flat) = (tiered(&rows, &levels), state(&flat_rows));
    assert!(tiered.map().tiered() && !flat.map().tiered());
    let domains = [Domain::Ground, Domain::Air];
    for _ in 0..2_000 {
        let mut end = || {
            let x = i32::try_from(rng.next_below(16)).unwrap();
            let y = i32::try_from(rng.next_below(13)).unwrap();
            ShotEnd::new(at(x, y), domains[rng.next_index(2)])
        };
        let (from, to) = (end(), end());
        for fire in [Fire::Direct, Fire::Indirect] {
            for landing in [Landing::Checked, Landing::Unchecked] {
                assert_eq!(
                    clear_line(&tiered, fire, from, to, landing),
                    clear_line(&flat, fire, from, to, landing),
                    "{fire:?} {landing:?} from {from:?} to {to:?}"
                );
            }
        }
    }
}
