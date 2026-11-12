//! Placement and targeted orders on a controller: A and X act at the
//! reticle while a mode is armed, a held A drags a placement row, and B
//! backs out, all through the real funnel against a staged sandbox.

use super::super::tests::pad_scene::{
    FRAME, PAD, down, feed, frame, left_stick, pad_input, press, push_until, rest_on, up,
};
use super::*;
use crate::input::{ClickVerb, Pointer, preview_spot, unit_pick_radius};
use crate::tests::support::{ZOOM, building_at, denied, left_down, scene, unit_at, wound_building};
use aftermine_protocol::RawEvent;
use aftermine_sim::{BuildingId, BuildingKind, Command, Rally};
use chassis::grid::TilePos;
use macroquad::prelude::vec2;

const BARRICADE: BuildingKind = BuildingKind::Barricade;
const FABRICATOR: BuildingKind = BuildingKind::Fabricator;

/// A Harvester below the middle of the map, selected, with `kind` armed
/// as its Build card arms it, and an own Turret at (31, 20) whose tile
/// refuses a site.
fn placing(kind: BuildingKind) -> (Game, InputState) {
    let mut game = scene(
        &[(0, UnitKind::Harvester, 32, 23)],
        &[(0, BuildingKind::Turret, 31, 20)],
    );
    let mut input = pad_input(&mut game);
    game.presentation
        .selection
        .set_units(vec![unit_at(&game, 32, 23)]);
    input.build_menu = true;
    input.build_category = Some(crate::action::building_category(kind));
    input.arm_placement(kind);
    (game, input)
}

/// Every staged build as (anchor, queued), in order.
fn builds(game: &Game) -> Vec<(TilePos, bool)> {
    game.pending
        .iter()
        .filter_map(|pc| match pc.command {
            Command::Build { anchor, queue, .. } => Some((anchor, queue)),
            _ => None,
        })
        .collect()
}

fn dispatch(game: &mut Game, input: &mut InputState, action: Action) {
    super::super::dispatch_action(game, input, &BindingMap::classic(), action);
}

/// Every staged command, in order.
fn commands(game: &Game) -> Vec<Command> {
    game.pending.iter().map(|pc| pc.command.clone()).collect()
}

#[test]
fn a_places_a_structure_centered_on_the_reticle_where_the_ghost_shows() {
    let (mut game, mut input) = placing(FABRICATOR);
    rest_on(&mut game, &mut input, vec2(35.0, 21.0));
    let spot = preview_spot(&game.view(), &input).expect("the ghost follows the reticle");
    // A 2x2 centered on the reticle, not hanging down and right of it.
    assert_eq!(spot.anchor, TilePos::new(34, 20));
    assert!(spot.verdict.builds(), "{:?}", spot.verdict);
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(builds(&game), [(spot.anchor, false)]);
    assert!(input.placing().is_none(), "one placement ends the mode");
    assert!(
        !input.construction_open(),
        "and closes the palette pad mode doesn't draw"
    );
    assert_eq!(
        game.presentation.selection.units(),
        vec![unit_at(&game, 32, 23)],
        "A placed rather than selected"
    );
}

#[test]
fn a_held_a_drags_a_row_that_skips_a_refused_cell_and_stages_on_release() {
    let (mut game, mut input) = placing(BARRICADE);
    rest_on(&mut game, &mut input, vec2(28.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    push_until(&mut game, &mut input, (0.6, 0.0), FRAME, |g| g.x >= 33.5);
    assert_eq!(
        input.planned_stamps(),
        [28, 29, 30, 32, 33].map(|x| TilePos::new(x, 20)),
        "the row skips the Turret's tile"
    );
    assert!(game.pending.is_empty(), "nothing stages while A is held");
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(
        builds(&game),
        [
            (TilePos::new(28, 20), false),
            (TilePos::new(29, 20), true),
            (TilePos::new(30, 20), true),
            (TilePos::new(32, 20), true),
            (TilePos::new(33, 20), true),
        ]
    );
    assert!(input.placing().is_none());
    assert!(!input.construction_open());
}

#[test]
fn a_camera_jump_under_a_held_a_lays_nothing_on_the_way() {
    let (mut game, mut input) = placing(BARRICADE);
    rest_on(&mut game, &mut input, vec2(28.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    push_until(&mut game, &mut input, (0.6, 0.0), FRAME, |g| g.x >= 30.5);
    assert_eq!(
        input.planned_stamps(),
        [28, 29, 30].map(|x| TilePos::new(x, 20))
    );
    rest_on(&mut game, &mut input, vec2(10.5, 10.5));
    for stamp in input.planned_stamps() {
        assert!(
            (stamp.y == 20 && (28..=30).contains(&stamp.x)) || *stamp == TilePos::new(10, 10),
            "{stamp:?} lies on the way"
        );
    }
}

#[test]
fn a_held_left_trigger_queues_and_a_plain_a_after_it_ends_placement() {
    let (mut game, mut input) = placing(BARRICADE);
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    rest_on(&mut game, &mut input, vec2(28.5, 18.5));
    press(&mut game, &mut input, PadButton::South);
    rest_on(&mut game, &mut input, vec2(30.5, 18.5));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        builds(&game),
        [(TilePos::new(28, 18), true), (TilePos::new(30, 18), true)]
    );
    assert!(input.placing().is_some(), "a held LT keeps placing");
    assert!(input.build_menu);
    feed(&mut game, &mut input, &[up(PadButton::LeftTrigger)]);
    assert!(input.placing().is_some(), "letting go keeps placing");
    assert!(input.build_menu);

    rest_on(&mut game, &mut input, vec2(26.5, 18.5));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(builds(&game)[2..], [(TilePos::new(26, 18), false)]);
    assert!(input.placing().is_none(), "a plain A ends placement");
    assert!(!input.construction_open(), "and closes construction");
}

#[test]
fn a_on_a_refused_spot_is_denied_and_placement_stands() {
    let (mut game, mut input) = placing(BARRICADE);
    rest_on(&mut game, &mut input, vec2(31.5, 20.5));
    press(&mut game, &mut input, PadButton::South);
    assert!(denied(&game));
    assert!(game.pending.is_empty());
    assert_eq!(input.placing_kind(), Some(BARRICADE));
    assert!(input.build_menu, "nothing placed, so construction stays");
    rest_on(&mut game, &mut input, vec2(29.5, 20.5));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(builds(&game), [(TilePos::new(29, 20), false)]);
}

#[test]
fn x_places_one_structure_but_waits_while_a_lays_a_row() {
    let (mut game, mut input) = placing(BARRICADE);
    rest_on(&mut game, &mut input, vec2(29.5, 20.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(builds(&game), [(TilePos::new(29, 20), false)]);
    assert!(input.placing().is_none(), "X is a tap of A");
    assert!(!input.construction_open());

    let (mut game, mut input) = placing(BARRICADE);
    rest_on(&mut game, &mut input, vec2(29.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    press(&mut game, &mut input, PadButton::West);
    assert!(denied(&game));
    assert!(game.pending.is_empty());
    assert_eq!(input.stroke_by(), Some(Presser::Pad(PAD)));
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(builds(&game), [(TilePos::new(29, 20), false)]);
}

#[test]
fn b_drops_a_held_row_then_walks_out_of_placement() {
    let (mut game, mut input) = placing(BARRICADE);
    rest_on(&mut game, &mut input, vec2(27.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    push_until(&mut game, &mut input, (0.6, 0.0), FRAME, |g| g.x >= 29.5);
    press(&mut game, &mut input, PadButton::East);
    assert!(input.planned_stamps().is_empty(), "B dropped the row");
    assert_eq!(input.placing_kind(), Some(BARRICADE), "and kept placing");
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert!(game.pending.is_empty(), "the release builds nothing");

    // Out of placement, B goes back to the build ring the structure came
    // from, as Esc goes back to the palette, then walks back ring by ring.
    let heading = |game: &Game, input: &InputState| {
        publish(game, input);
        super::super::pad_ring::current(&game.view(), input).map(|ring| ring.heading)
    };
    publish(&game, &input);
    press(&mut game, &mut input, PadButton::East);
    assert!(input.placing().is_none());
    assert!(input.build_menu);
    assert_eq!(heading(&game, &input).as_deref(), Some("DEFENSE"));
    press(&mut game, &mut input, PadButton::East);
    assert_eq!(input.build_category, None);
    assert_eq!(heading(&game, &input).as_deref(), Some("BUILD"));
    press(&mut game, &mut input, PadButton::East);
    assert!(!input.construction_open());
    assert_eq!(heading(&game, &input).as_deref(), Some("COMMANDS"));
    press(&mut game, &mut input, PadButton::East);
    assert!(input.ring.is_none());
    assert_eq!(game.presentation.selection.units().len(), 1);
    press(&mut game, &mut input, PadButton::East);
    assert!(game.presentation.selection.units().is_empty());
}

#[test]
fn b_out_of_a_queued_chain_returns_to_the_selection() {
    let (mut game, mut input) = placing(BARRICADE);
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    rest_on(&mut game, &mut input, vec2(28.5, 18.5));
    press(&mut game, &mut input, PadButton::South);
    feed(&mut game, &mut input, &[up(PadButton::LeftTrigger)]);
    assert_eq!(builds(&game), [(TilePos::new(28, 18), true)]);
    assert!(input.placing().is_some(), "premise: still placing");

    // The build ring has a card to show, yet B leaves it shut.
    publish(&game, &input);
    press(&mut game, &mut input, PadButton::East);
    assert!(input.placing().is_none());
    assert!(input.ring.is_none(), "no build ring opens");
    assert!(!input.construction_open(), "construction closes");
    assert_eq!(game.presentation.selection.units().len(), 1);
    assert_eq!(builds(&game).len(), 1, "and the chain stands");
}

#[test]
fn b_out_of_a_placement_with_no_build_ring_left_closes_construction() {
    // Nothing published a panel, so no ring has a card to show.
    let (mut game, mut input) = placing(BARRICADE);
    press(&mut game, &mut input, PadButton::East);
    assert!(input.ring.is_none());
    assert!(!input.construction_open());
    assert_eq!(game.presentation.selection.units().len(), 1);
}

/// Publishes the panel as the HUD's layout pass does before input.
fn publish(game: &Game, input: &InputState) {
    *game.presentation.panel_model.borrow_mut() =
        crate::panel::build_for_input(&game.view(), &BindingMap::classic(), input);
}

#[test]
fn another_pointer_a_second_pad_or_a_disconnect_leaves_no_stray_row() {
    // A mouse that only moves leaves the row be; one that clicks takes
    // over and ends the row unbuilt.
    let (mut game, mut input) = placing(BARRICADE);
    rest_on(&mut game, &mut input, vec2(28.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    feed(
        &mut game,
        &mut input,
        &[RawEvent::MouseMove { x: 5.0, y: 5.0 }],
    );
    assert!(
        input.stroke_by().is_some(),
        "a nudged mouse changes nothing"
    );
    feed(&mut game, &mut input, &[left_down(vec2(5.0, 5.0))]);
    assert_eq!(input.stroke_by(), None);
    assert_eq!(input.placing_kind(), Some(BARRICADE));
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert!(game.pending.is_empty());

    // A second pad's A neither opens a row nor ends the first pad's.
    let (mut game, mut input) = placing(BARRICADE);
    let other = |button| RawEvent::PadDown {
        pad: PAD + 1,
        button,
    };
    rest_on(&mut game, &mut input, vec2(28.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    feed(
        &mut game,
        &mut input,
        &[
            other(PadButton::South),
            RawEvent::PadUp {
                pad: PAD + 1,
                button: PadButton::South,
            },
        ],
    );
    assert!(game.pending.is_empty());
    assert_eq!(input.stroke_by(), Some(Presser::Pad(PAD)));
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(builds(&game), [(TilePos::new(28, 20), false)]);

    // A pad that goes takes its row with it.
    let (mut game, mut input) = placing(BARRICADE);
    rest_on(&mut game, &mut input, vec2(28.5, 20.5));
    feed(
        &mut game,
        &mut input,
        &[
            down(PadButton::South),
            RawEvent::PadDisconnected { pad: PAD },
        ],
    );
    assert_eq!(input.stroke_by(), None);
    assert!(game.pending.is_empty());
}

#[test]
fn the_reticle_neither_snaps_nor_slows_while_placing() {
    let (mut game, mut input) = placing(BARRICADE);
    let beside = vec2(32.5, 23.9);
    rest_on(&mut game, &mut input, beside);
    assert_eq!(input.reticle.snap(), None, "no bracket while placing");
    let travel = |game: &mut Game, input: &mut InputState| {
        let from = input.reticle.ground(&game.presentation.camera);
        feed(game, input, &[left_stick(0.5, 0.0)]);
        frame(game, input, FRAME);
        feed(game, input, &[left_stick(0.0, 0.0)]);
        input.reticle.ground(&game.presentation.camera).x - from.x
    };
    let placing_travel = travel(&mut game, &mut input);

    input.stop_placing();
    rest_on(&mut game, &mut input, beside);
    assert!(
        input.reticle.snap().is_some(),
        "premise: it snaps otherwise"
    );
    let snapped_travel = travel(&mut game, &mut input);
    assert!(
        snapped_travel < placing_travel * 0.9,
        "{snapped_travel} slows where {placing_travel} does not"
    );
}

/// A Sentinel at (30, 20), selected.
fn sentinel() -> (Game, InputState, UnitId) {
    let mut game = scene(&[(0, UnitKind::Sentinel, 30, 20)], &[]);
    let input = pad_input(&mut game);
    let unit = unit_at(&game, 30, 20);
    game.presentation.selection.set_units(vec![unit]);
    (game, input, unit)
}

#[test]
fn a_or_x_confirms_hunt_run_and_patrol_at_the_reticle() {
    let (mut game, mut input, unit) = sentinel();
    dispatch(&mut game, &mut input, Action::Hunt);
    rest_on(&mut game, &mut input, vec2(36.5, 24.5));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        commands(&game),
        [Command::Hunt {
            units: vec![unit],
            goal: TilePos::new(36, 24),
            queue: false,
        }]
    );
    assert!(input.armed_mode().is_none(), "the order spent the mode");
    assert_eq!(game.presentation.selection.units(), vec![unit]);

    let (mut game, mut input, unit) = sentinel();
    dispatch(&mut game, &mut input, Action::Run);
    rest_on(&mut game, &mut input, vec2(26.5, 14.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        commands(&game),
        [Command::Run {
            units: vec![unit],
            goal: TilePos::new(26, 14),
            queue: false,
        }]
    );

    let (mut game, mut input, unit) = sentinel();
    dispatch(&mut game, &mut input, Action::Patrol);
    rest_on(&mut game, &mut input, vec2(36.5, 24.5));
    press(&mut game, &mut input, PadButton::South);
    rest_on(&mut game, &mut input, vec2(26.5, 24.5));
    press(&mut game, &mut input, PadButton::West);
    assert!(game.pending.is_empty(), "waypoints collect until Patrol");
    dispatch(&mut game, &mut input, Action::Patrol);
    assert_eq!(
        commands(&game),
        [Command::Patrol {
            units: vec![unit],
            waypoints: vec![TilePos::new(36, 24), TilePos::new(26, 24)],
        }]
    );
}

#[test]
fn a_held_left_trigger_keeps_a_targeted_order_armed() {
    let (mut game, mut input, unit) = sentinel();
    dispatch(&mut game, &mut input, Action::Hunt);
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    for x in [36, 40] {
        rest_on(&mut game, &mut input, vec2(x as f32 + 0.5, 24.5));
        press(&mut game, &mut input, PadButton::South);
    }
    assert_eq!(
        commands(&game),
        [36, 40].map(|x| Command::Hunt {
            units: vec![unit],
            goal: TilePos::new(x, 24),
            queue: true,
        })
    );
    assert!(input.is_armed(ClickVerb::Hunt));
}

/// Loads one warhead into Spire `id` through the state's own
/// serialization.
fn load_warhead(game: &mut Game, id: BuildingId) {
    let mut data = serde_json::to_value(&*game.state).expect("state serializes");
    let spire = data["buildings"]
        .as_array_mut()
        .expect("buildings")
        .iter_mut()
        .find(|b| b["id"] == serde_json::json!(id))
        .expect("the Spire is listed");
    spire["magazine"]["loaded"] = 1.into();
    *game.state = serde_json::from_value(data).expect("the loaded state loads");
}

#[test]
fn a_launches_at_the_reticle() {
    let mut game = scene(&[], &[(0, BuildingKind::Spire, 20, 10)]);
    let mut input = pad_input(&mut game);
    let spire = building_at(&game, 20, 10);
    load_warhead(&mut game, spire);
    game.presentation.selection.set_buildings(vec![spire]);
    dispatch(&mut game, &mut input, Action::Launch);
    rest_on(&mut game, &mut input, vec2(40.5, 30.5));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        commands(&game),
        [Command::Launch {
            building: spire,
            target: TilePos::new(40, 30),
        }]
    );
    assert!(input.striking().is_none(), "its one warhead is spent");
}

#[test]
fn launch_takes_the_bumpers_over_to_fire_fewer_or_more() {
    let mut game = scene(
        &[(0, UnitKind::Sentinel, 30, 30)],
        &[
            (0, BuildingKind::Spire, 20, 10),
            (0, BuildingKind::Spire, 26, 10),
        ],
    );
    let mut input = pad_input(&mut game);
    let spires = vec![building_at(&game, 20, 10), building_at(&game, 26, 10)];
    for &spire in &spires {
        load_warhead(&mut game, spire);
    }
    game.presentation.selection.set_buildings(spires.clone());
    dispatch(&mut game, &mut input, Action::Launch);
    let salvo = |game: &Game, input: &InputState| {
        crate::input::salvo_readout(&game.view(), input).map(|(fires, _)| fires)
    };
    assert_eq!(salvo(&game, &input), Some(1), "one fires to begin with");
    for (bumper, fires) in [
        (PadButton::RightShoulder, 2),
        (PadButton::RightShoulder, 2),
        (PadButton::LeftShoulder, 1),
        (PadButton::RightShoulder, 2),
    ] {
        press(&mut game, &mut input, bumper);
        assert_eq!(salvo(&game, &input), Some(fires), "{bumper:?}");
    }
    assert!(
        game.presentation.selection.units().is_empty(),
        "the bumpers selected no army while Launch was armed"
    );
    rest_on(&mut game, &mut input, vec2(40.5, 30.5));
    press(&mut game, &mut input, PadButton::South);
    let target = TilePos::new(40, 30);
    assert_eq!(
        commands(&game),
        spires
            .iter()
            .map(|&building| Command::Launch { building, target })
            .collect::<Vec<_>>()
    );
    assert!(input.striking().is_none(), "both warheads are spent");
    press(&mut game, &mut input, PadButton::LeftShoulder);
    assert_eq!(
        game.presentation.selection.units(),
        vec![unit_at(&game, 30, 30)],
        "with Launch done, LB selects the army again"
    );
}

#[test]
fn x_sets_a_rally_on_the_ground_or_onto_a_unit_the_reticle_reaches() {
    let mut game = scene(
        &[(0, UnitKind::Sentinel, 30, 20)],
        &[(0, FABRICATOR, 20, 25)],
    );
    let mut input = pad_input(&mut game);
    let fabricator = building_at(&game, 20, 25);
    let escort = unit_at(&game, 30, 20);
    game.presentation.selection.set_buildings(vec![fabricator]);
    let rallies = |game: &Game| -> Vec<Option<Rally>> {
        game.pending
            .iter()
            .filter_map(|pc| match pc.command {
                Command::SetRally { rally, .. } => Some(rally),
                _ => None,
            })
            .collect()
    };

    input.armed = Some(crate::input::Armed::Rally(vec![fabricator]));
    rest_on(&mut game, &mut input, vec2(36.5, 24.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(rallies(&game), [Some(Rally::Tile(TilePos::new(36, 24)))]);
    assert!(input.rallying().is_empty(), "a rally takes one pick");

    // Off the Sentinel's body but inside the reticle's reach, as X's
    // contextual order reaches.
    let reach = 24.0 / ZOOM;
    let off = f32::midpoint(unit_pick_radius(UnitKind::Sentinel), reach);
    assert!(unit_pick_radius(UnitKind::Sentinel) < off && off < reach);
    game.pending.clear();
    input.armed = Some(crate::input::Armed::Rally(vec![fabricator]));
    rest_on(&mut game, &mut input, vec2(30.5 + off, 20.5));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(rallies(&game), [Some(Rally::Unit(escort))]);
    assert_eq!(input.last_pointer, Pointer::Pad);
}

/// Takes `hp` off unit `id` through the state's own serialization.
fn wound_unit(game: &mut Game, id: UnitId, hp: u64) {
    let mut data = serde_json::to_value(&*game.state).expect("state serializes");
    let unit = data["units"]
        .as_array_mut()
        .expect("units")
        .iter_mut()
        .find(|u| u["id"] == serde_json::json!(id))
        .expect("the unit is listed");
    unit["hp"] = serde_json::json!(unit["hp"].as_u64().expect("hp") - hp);
    *game.state = serde_json::from_value(data).expect("the wounded state loads");
}

#[test]
fn repair_escort_and_salvage_take_what_the_reticle_brackets() {
    let mut game = scene(
        &[
            (0, UnitKind::Harvester, 26, 20),
            (0, UnitKind::Lancer, 34, 20),
        ],
        &[(0, BuildingKind::Turret, 30, 24)],
    );
    let mut input = pad_input(&mut game);
    let harvester = unit_at(&game, 26, 20);
    let lancer = unit_at(&game, 34, 20);
    let turret = building_at(&game, 30, 24);
    wound_unit(&mut game, lancer, 10);
    wound_building(&mut game, turret, 10);
    game.presentation.selection.set_units(vec![harvester]);
    // Off the Lancer's body but inside the reticle's reach.
    let reach = 24.0 / ZOOM;
    let off = f32::midpoint(unit_pick_radius(UnitKind::Lancer), reach);
    assert!(unit_pick_radius(UnitKind::Lancer) < off && off < reach);
    let beside = vec2(34.5 + off, 20.5);

    dispatch(&mut game, &mut input, Action::RepairUnit);
    rest_on(&mut game, &mut input, beside);
    assert!(input.reticle.snap().is_some(), "the reticle brackets it");
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        commands(&game),
        [Command::RepairUnit {
            units: vec![harvester],
            target: lancer,
            queue: false,
        }]
    );

    game.pending.clear();
    dispatch(&mut game, &mut input, Action::Accompany);
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        commands(&game),
        [Command::Accompany {
            units: vec![harvester],
            target: lancer,
            queue: false,
        }]
    );

    // Off the Turret's footprint corner but inside the bracket's reach.
    game.pending.clear();
    dispatch(&mut game, &mut input, Action::Salvage);
    rest_on(&mut game, &mut input, vec2(29.7, 23.7));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        commands(&game),
        [Command::Salvage {
            units: vec![harvester],
            building: turret,
            queue: false,
        }]
    );
}

#[test]
fn a_controller_taking_over_from_a_finger_drops_its_ghost_and_aim() {
    let (mut game, mut input) = placing(BARRICADE);
    rest_on(&mut game, &mut input, vec2(28.5, 20.5));
    super::super::drop_ghost_in_view(&game.view(), &mut input);
    assert!(input.ghost_anchor().is_some(), "premise: a finger's ghost");
    feed(&mut game, &mut input, &[left_stick(0.3, 0.0)]);
    assert_eq!(input.ghost_anchor(), None, "the reticle takes over");
    feed(&mut game, &mut input, &[left_stick(0.0, 0.0)]);
    rest_on(&mut game, &mut input, vec2(28.5, 20.5));
    super::super::drop_ghost_in_view(&game.view(), &mut input);
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(builds(&game), [(TilePos::new(28, 20), false)]);

    let mut game = scene(&[], &[(0, BuildingKind::Spire, 20, 10)]);
    let mut input = pad_input(&mut game);
    let spire = building_at(&game, 20, 10);
    load_warhead(&mut game, spire);
    game.presentation.selection.set_buildings(vec![spire]);
    dispatch(&mut game, &mut input, Action::Launch);
    rest_on(&mut game, &mut input, vec2(40.5, 30.5));
    super::super::drop_aim_in_view(&game.view(), &mut input);
    assert!(
        input
            .striking()
            .and_then(crate::input::Strike::aim)
            .is_some()
    );
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        commands(&game),
        [Command::Launch {
            building: spire,
            target: TilePos::new(40, 30),
        }]
    );
}

#[test]
fn salvage_refuses_the_foundry_the_reticle_rests_on_rather_than_strip_its_neighbor() {
    let mut game = scene(
        &[(0, UnitKind::Harvester, 26, 20)],
        &[
            (0, BuildingKind::Foundry, 30, 24),
            (0, BuildingKind::Barricade, 32, 24),
        ],
    );
    let mut input = pad_input(&mut game);
    let harvester = unit_at(&game, 26, 20);
    let barricade = building_at(&game, 32, 24);
    game.presentation.selection.set_units(vec![harvester]);
    dispatch(&mut game, &mut input, Action::Salvage);
    // On the Foundry, with the Barricade inside the reticle's reach.
    rest_on(&mut game, &mut input, vec2(31.6, 24.5));
    press(&mut game, &mut input, PadButton::South);
    assert!(denied(&game));
    assert!(game.pending.is_empty(), "{:?}", commands(&game));
    assert!(input.is_armed(ClickVerb::Salvage), "the mode stands");

    rest_on(&mut game, &mut input, vec2(32.5, 24.5));
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        commands(&game),
        [Command::Salvage {
            units: vec![harvester],
            building: barricade,
            queue: false,
        }]
    );
}
