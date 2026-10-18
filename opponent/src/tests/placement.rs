use super::*;
use crate::placement::{Allowed, Layout, Refusal, check};
use std::collections::BTreeMap;

/// A west base with a frame, a scrap node, a sealed two-by-two pocket and a
/// visible hostile Sentinel; the east seat stays out of sight.
const YARD: [&str; 14] = [
    "####################",
    "#..................#",
    "#.......E..........#",
    "#..................#",
    "#..1.......####....#",
    "#..........#..#....#",
    "#..........#..#....#",
    "#..........####....#",
    "#.......s..........#",
    "#..................#",
    "#..................#",
    "#................2.#",
    "#..................#",
    "####################",
];

fn yard() -> Scenario {
    let mut scenario = arena(0);
    scenario.map = YARD.map(str::to_owned).to_vec();
    scenario.units = vec![
        harvester(0, 5, 6),
        harvester(0, 6, 6),
        UnitSpec {
            player: 0,
            kind: UnitKind::Sentinel,
            x: 10,
            y: 6,
        },
        UnitSpec {
            player: 1,
            kind: UnitKind::Sentinel,
            x: 6,
            y: 9,
        },
    ];
    scenario
}

fn verdict(state: &State, kind: BuildingKind, x: i32, y: i32) -> Result<Allowed, Refusal> {
    let observation = ObservationData::fog_honest(state, PlayerId(0));
    check(
        &map(&yard()),
        &observation,
        kind,
        TilePos::new(x, y),
        &[],
        Layout::Apart,
    )
}

#[test]
fn each_known_obstacle_refuses_a_footprint_for_its_own_reason() {
    let state = yard().build().unwrap();
    let fabricator = BuildingKind::Fabricator;
    assert_eq!(
        verdict(&state, BuildingKind::Airworks, 6, 11),
        Err(Refusal::Prerequisite)
    );
    assert_eq!(
        verdict(&state, fabricator, 15, 10),
        Err(Refusal::Unexplored)
    );
    assert_eq!(verdict(&state, fabricator, 8, 2), Err(Refusal::Frame));
    assert_eq!(
        verdict(&state, BuildingKind::Extractor, 5, 1),
        Err(Refusal::Frame)
    );
    assert!(verdict(&state, BuildingKind::Extractor, 8, 2).is_ok());
    assert_eq!(
        verdict(&state, fabricator, 8, 7),
        Err(Refusal::Terrain),
        "scrap"
    );
    assert_eq!(
        verdict(&state, fabricator, 10, 3),
        Err(Refusal::Terrain),
        "rock"
    );
    assert_eq!(verdict(&state, fabricator, 4, 5), Err(Refusal::Occupied));
    assert_eq!(verdict(&state, fabricator, 5, 3), Err(Refusal::Crowded));
    assert_eq!(verdict(&state, fabricator, 6, 9), Err(Refusal::HostileUnit));
    assert_eq!(verdict(&state, fabricator, 12, 5), Err(Refusal::NoEgress));
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let planned = [(fabricator, TilePos::new(5, 1))];
    assert_eq!(
        check(
            &map(&yard()),
            &observation,
            fabricator,
            TilePos::new(6, 1),
            &planned,
            Layout::Apart
        ),
        Err(Refusal::Claimed)
    );
    assert_eq!(
        verdict(&state, fabricator, 6, 1),
        Ok(Allowed { defer: false })
    );
}

#[test]
fn an_accepted_footprint_is_one_the_simulation_accepts() {
    // The yard plus pits, a peak and a standing Fabricator, so terrain the
    // yard lacks and tech-gated kinds are both compared.
    let mut scenario = yard();
    scenario.map[9] = "#.~~^..............#".to_owned();
    scenario.map[10] = "#.~~...............#".to_owned();
    scenario.buildings.push(BuildingSpec {
        player: 0,
        kind: BuildingKind::Fabricator,
        x: 12,
        y: 9,
    });
    let state = scenario.build().unwrap();
    let mut accepted = BTreeMap::new();
    for kind in BuildingKind::ALL {
        for y in 0..14 {
            for x in 0..20 {
                if verdict(&state, kind, x, y).is_ok() {
                    *accepted.entry(kind).or_insert(0) += 1;
                    let anchor = TilePos::new(x, y);
                    assert_eq!(
                        state.place_intent_refusal(PlayerId(0), kind, anchor),
                        None,
                        "{kind:?} at {anchor:?}"
                    );
                }
            }
        }
    }
    assert!(
        accepted.values().all(|count| *count > 0) && accepted.len() >= 8,
        "premise: most kinds find open ground: {accepted:?}"
    );
}

#[test]
fn hidden_enemies_never_change_a_verdict() {
    let quiet = yard().build().unwrap();
    let sight = ObservationData::fog_honest(&quiet, PlayerId(0));
    let unseen = (1..19)
        .flat_map(|x| (1..10).map(move |y| TilePos::new(x, y)))
        .find(|tile| !sight.visible(*tile) && quiet.map().terrain_passable(*tile))
        .expect("premise: part of the yard is out of sight");
    let mut hidden = yard();
    hidden.units.push(UnitSpec {
        player: 1,
        kind: UnitKind::Sentinel,
        x: unseen.x,
        y: unseen.y,
    });
    let hidden = hidden.build().unwrap();
    for y in 0..14 {
        for x in 0..20 {
            assert_eq!(
                verdict(&quiet, BuildingKind::Fabricator, x, y),
                verdict(&hidden, BuildingKind::Fabricator, x, y),
                "({x}, {y})"
            );
        }
    }
}

#[test]
fn explored_ground_out_of_sight_is_claimed_as_a_scaffold() {
    let state = yard().build().unwrap();
    let mut observation = ObservationData::fog_honest(&state, PlayerId(0));
    let anchor = TilePos::new(6, 1);
    assert_eq!(
        check(
            &map(&yard()),
            &observation,
            BuildingKind::Fabricator,
            anchor,
            &[],
            Layout::Apart
        ),
        Ok(Allowed { defer: false })
    );
    let index = usize::try_from(anchor.y * observation.map_width + anchor.x).unwrap();
    observation.visible[index] = false;
    assert_eq!(
        check(
            &map(&yard()),
            &observation,
            BuildingKind::Fabricator,
            anchor,
            &[],
            Layout::Apart
        ),
        Ok(Allowed { defer: true })
    );
}

#[test]
fn a_tiered_base_never_lays_a_foundation_on_a_ramp() {
    // A plateau across the field's north joins the floor by a ramp across
    // a block of the west Foundry's base layout, where a building would go.
    let mut scenario = field();
    let ramps = 1..=5;
    for y in 1..=7 {
        let row = &mut scenario.map[y];
        let cells: String = row
            .chars()
            .enumerate()
            .map(|(x, c)| match (x, y) {
                (0 | 47, _) => c,
                (x, 7) if ramps.contains(&x) => '/',
                (_, 7) => '=',
                _ => c,
            })
            .collect();
        *row = cells;
    }
    scenario.levels = (0..scenario.map.len())
        .map(|y| {
            (0..48)
                .map(|_| if y <= 6 { '2' } else { '1' })
                .collect()
        })
        .collect();
    scenario.players[0].scrap = 3_000;
    scenario.units = (5..=7)
        .flat_map(|x| [13, 14].map(|y| harvester(0, x, y)))
        .collect();
    let mut state = scenario.build().unwrap();
    let mut bot = seat(&scenario, 0);
    let mut built = Vec::new();
    while state.current_tick() < 3_000 {
        let commands = bot.act(&state, &mut OwnEvents::default());
        built.extend(builds(&commands));
        state.tick(&commands);
    }
    assert!(!built.is_empty(), "the base grew");
    for (kind, anchor) in built {
        let (w, h) = kind.size();
        let on_ramp = (0..h).any(|dy| (0..w).any(|dx| state.map().is_ramp(anchor.offset(dx, dy))));
        assert!(
            !on_ramp || kind.fits_ramp(),
            "{kind:?} ordered onto the ramp at {anchor:?}"
        );
    }
}
