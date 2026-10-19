use super::*;
use crate::scenario::{PlayerSpec, ScenarioMode};
use chassis::fx::Fx;

/// A sandbox over `rows`, shooting along row 2 from column 1 to column 7.
fn state(rows: &[&str]) -> State {
    crate::Scenario {
        mode: ScenarioMode::Sandbox,
        name: "fire".into(),
        map: rows.iter().map(|row| (*row).to_string()).collect(),
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
