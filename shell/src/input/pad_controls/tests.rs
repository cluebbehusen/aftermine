//! Controller presses through the real funnel: pad events as the frame
//! loop feeds them, against a staged sandbox.

use super::super::tests::pad_scene::{
    CENTER, FRAME, PAD, building_at, denied, down, feed, pad_input, pos, press, push_until,
    rest_on, scene, unit_at, units_of, up, wound_building,
};
use super::*;
use aftermine_protocol::RawEvent;
use aftermine_sim::{BuildingKind, Command, PlayerCommand};
use macroquad::prelude::vec2;

const SENTINEL: UnitKind = UnitKind::Sentinel;
const LANCER: UnitKind = UnitKind::Lancer;
const HARVESTER: UnitKind = UnitKind::Harvester;

fn staged(game: &Game) -> Vec<PlayerCommand> {
    game.pending.iter().cloned().collect()
}

/// The one command staged.
fn only_command(game: &Game) -> Command {
    match staged(game).as_slice() {
        [one] => one.command.clone(),
        other => panic!("expected one staged command, got {other:?}"),
    }
}

#[test]
fn a_selects_what_the_reticle_brackets() {
    let mut game = scene(&[(0, SENTINEL, 30, 20), (0, SENTINEL, 34, 20)], &[]);
    let mut input = pad_input(&mut game);
    let near = unit_at(&game, 30, 20);
    // Beside it, inside the reticle's reach but off its body.
    rest_on(&mut game, &mut input, vec2(31.1, 20.5));
    assert_eq!(
        input.reticle.snap(),
        Some(Picked::Unit(near, aftermine_sim::PlayerId(0)))
    );
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(game.presentation.selection.units, vec![near]);
}

#[test]
fn a_tap_selects_what_was_bracketed_as_a_went_down() {
    let mut game = scene(&[(0, SENTINEL, 30, 20), (0, SENTINEL, 31, 20)], &[]);
    let mut input = pad_input(&mut game);
    let first = unit_at(&game, 30, 20);
    rest_on(&mut game, &mut input, vec2(30.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    // Most of the way to the next unit, but inside the tap's slop.
    push_until(&mut game, &mut input, (0.4, 0.0), FRAME, |g| g.x >= 31.1);
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(game.presentation.selection.units, vec![first]);
}

#[test]
fn held_queue_toggles_a_pick_and_a_miss_clears() {
    let mut game = scene(&[(0, SENTINEL, 30, 20), (0, SENTINEL, 33, 20)], &[]);
    let mut input = pad_input(&mut game);
    let (a, b) = (unit_at(&game, 30, 20), unit_at(&game, 33, 20));
    let (at_a, at_b) = (pos(&game, a), pos(&game, b));
    rest_on(&mut game, &mut input, at_a);
    press(&mut game, &mut input, PadButton::South);
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    assert!(input.queue_held(), "a held left trigger queues");
    rest_on(&mut game, &mut input, at_b);
    input.now += 1.0;
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(game.presentation.selection.units, vec![a, b]);
    input.now += 1.0;
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        game.presentation.selection.units,
        vec![a],
        "toggled back out"
    );
    feed(&mut game, &mut input, &[up(PadButton::LeftTrigger)]);
    assert!(!input.queue_held());
    rest_on(&mut game, &mut input, vec2(30.5, 30.5));
    input.now += 1.0;
    press(&mut game, &mut input, PadButton::South);
    assert!(
        game.presentation.selection.units.is_empty(),
        "a miss clears"
    );
}

/// Two Sentinels and a Lancer in view, and a Sentinel far out of it.
fn kinds_scene() -> Game {
    scene(
        &[
            (0, SENTINEL, 30, 20),
            (0, SENTINEL, 34, 22),
            (0, LANCER, 32, 18),
            (0, SENTINEL, 60, 20),
        ],
        &[],
    )
}

#[test]
fn a_second_tap_selects_that_kind_in_view() {
    let mut game = kinds_scene();
    let mut input = pad_input(&mut game);
    rest_on(&mut game, &mut input, vec2(30.5, 20.5));
    press(&mut game, &mut input, PadButton::South);
    input.now += 0.2;
    press(&mut game, &mut input, PadButton::South);
    let in_view = vec![unit_at(&game, 30, 20), unit_at(&game, 34, 22)];
    assert_eq!(game.presentation.selection.units, in_view);
}

#[test]
fn a_late_second_tap_or_one_under_queue_stays_a_tap() {
    let mut game = kinds_scene();
    let mut input = pad_input(&mut game);
    let one = unit_at(&game, 30, 20);
    rest_on(&mut game, &mut input, vec2(30.5, 20.5));
    press(&mut game, &mut input, PadButton::South);
    input.now += 0.5;
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(game.presentation.selection.units, vec![one], "too late");
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    input.now += 0.1;
    press(&mut game, &mut input, PadButton::South);
    assert!(
        game.presentation.selection.units.is_empty(),
        "queue toggles the unit out instead"
    );
}

#[test]
fn a_double_tap_reaches_a_structure_the_reticle_brackets() {
    let mut game = scene(
        &[],
        &[
            (0, BuildingKind::Array, 30, 20),
            (0, BuildingKind::Array, 36, 20),
        ],
    );
    let mut input = pad_input(&mut game);
    let (min, _) = select::footprint(game.state.building(building_at(&game, 30, 20)).unwrap());
    // Just off the footprint's corner, inside the reticle's reach.
    rest_on(&mut game, &mut input, min - vec2(0.3, 0.3));
    press(&mut game, &mut input, PadButton::South);
    input.now += 0.2;
    press(&mut game, &mut input, PadButton::South);
    assert_eq!(
        game.presentation.selection.buildings,
        vec![building_at(&game, 30, 20), building_at(&game, 36, 20)]
    );
}

#[test]
fn x_moves_the_selection_to_the_ground_under_the_reticle() {
    let mut game = scene(&[(0, SENTINEL, 30, 20)], &[]);
    let mut input = pad_input(&mut game);
    let unit = unit_at(&game, 30, 20);
    game.presentation.selection.units = vec![unit];
    rest_on(&mut game, &mut input, vec2(36.5, 24.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Advance {
            units: vec![unit],
            goal: chassis::grid::TilePos::new(36, 24),
            queue: false,
        }
    );
}

#[test]
fn x_attacks_an_enemy_in_reach_even_beside_your_own_unit() {
    let mut game = scene(
        &[
            (0, SENTINEL, 26, 20),
            (0, SENTINEL, 30, 20),
            (1, SENTINEL, 31, 20),
        ],
        &[],
    );
    let mut input = pad_input(&mut game);
    let (army, own, enemy) = (
        unit_at(&game, 26, 20),
        unit_at(&game, 30, 20),
        unit_at(&game, 31, 20),
    );
    game.presentation.selection.units = vec![army];
    // Nearer your own Sentinel, which the bracket prefers.
    rest_on(&mut game, &mut input, vec2(30.9, 20.5));
    assert_eq!(
        input.reticle.snap(),
        Some(Picked::Unit(own, aftermine_sim::PlayerId(0)))
    );
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Attack {
            units: vec![army],
            target: aftermine_sim::Target::Unit(enemy).into(),
            queue: false,
        }
    );
}

#[test]
fn x_sends_harvesters_to_scrap_and_welds_a_wounded_structure() {
    let mut game = scene(
        &[(0, HARVESTER, 37, 20)],
        &[(0, BuildingKind::Array, 30, 24)],
    );
    let mut input = pad_input(&mut game);
    let harvester = unit_at(&game, 37, 20);
    let array = building_at(&game, 30, 24);
    game.presentation.selection.units = vec![harvester];
    // The drill's scrap lies at (40, 20).
    rest_on(&mut game, &mut input, vec2(40.5, 20.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Harvest {
            units: vec![harvester],
            node: chassis::grid::TilePos::new(40, 20),
            queue: false,
        }
    );
    game.pending.clear();
    wound_building(&mut game, array, 10);
    // Off the footprint's corner but inside the bracket's reach.
    let (min, _) = select::footprint(game.state.building(array).unwrap());
    rest_on(&mut game, &mut input, min - vec2(0.3, 0.3));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Repair {
            units: vec![harvester],
            building: array,
            queue: false,
        }
    );
}

#[test]
fn x_under_a_held_left_trigger_queues_the_order() {
    let mut game = scene(&[(0, SENTINEL, 30, 20)], &[]);
    let mut input = pad_input(&mut game);
    let unit = unit_at(&game, 30, 20);
    game.presentation.selection.units = vec![unit];
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    rest_on(&mut game, &mut input, vec2(36.5, 24.5));
    press(&mut game, &mut input, PadButton::West);
    assert!(matches!(
        only_command(&game),
        Command::Advance { queue: true, .. }
    ));
}

#[test]
fn x_collects_a_patrol_waypoint_while_a_route_is_armed() {
    let mut game = scene(&[(0, SENTINEL, 30, 20)], &[]);
    let mut input = pad_input(&mut game);
    game.presentation.selection.units = vec![unit_at(&game, 30, 20)];
    super::super::dispatch_action(
        &mut game,
        &mut input,
        &BindingMap::classic(),
        Action::Patrol,
    );
    rest_on(&mut game, &mut input, vec2(36.5, 24.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(input.patrol_route.as_ref().map(Vec::len), Some(1));
    assert!(game.pending.is_empty());
}

#[test]
fn chrome_under_the_reticle_takes_neither_a_nor_x() {
    let mut game = scene(&[(0, SENTINEL, 32, 20)], &[]);
    let mut input = pad_input(&mut game);
    game.presentation.selection.units = vec![unit_at(&game, 32, 20)];
    let mut layout = game.presentation.layout.get();
    layout.performance = macroquad::prelude::Rect::new(0.0, 0.0, 1280.0, 800.0);
    game.presentation.layout.set(layout);
    rest_on(&mut game, &mut input, CENTER);
    press(&mut game, &mut input, PadButton::West);
    assert!(game.pending.is_empty());
    assert!(denied(&game));
    game.presentation.sounds_pending.clear();
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    assert!(input.pad_press.is_none());
    assert!(denied(&game));
}

#[test]
fn b_cancels_the_mode_then_clears_the_selection() {
    let mut game = scene(&[(0, SENTINEL, 30, 20)], &[]);
    let mut input = pad_input(&mut game);
    let unit = unit_at(&game, 30, 20);
    game.presentation.selection.units = vec![unit];
    super::super::dispatch_action(&mut game, &mut input, &BindingMap::classic(), Action::Hunt);
    press(&mut game, &mut input, PadButton::East);
    assert!(input.armed_mode().is_none());
    assert_eq!(game.presentation.selection.units, vec![unit]);
    press(&mut game, &mut input, PadButton::East);
    assert!(game.presentation.selection.units.is_empty());
    press(&mut game, &mut input, PadButton::East);
    assert!(!input.take_menu_request(), "B never opens the menu");
}

#[test]
fn the_bumpers_select_the_army_and_the_army_in_view() {
    let mut game = scene(
        &[
            (0, SENTINEL, 30, 20),
            (0, HARVESTER, 31, 20),
            (0, SENTINEL, 60, 20),
        ],
        &[],
    );
    let mut input = pad_input(&mut game);
    press(&mut game, &mut input, PadButton::LeftShoulder);
    assert_eq!(
        game.presentation.selection.units,
        units_of(&game, 0, SENTINEL)
    );
    press(&mut game, &mut input, PadButton::RightShoulder);
    assert_eq!(
        game.presentation.selection.units,
        vec![unit_at(&game, 30, 20)]
    );
}

/// Two Harvesters, a Lancer and two Sentinels, all selected.
fn mixed() -> (Game, InputState) {
    let mut game = scene(
        &[
            (0, SENTINEL, 30, 20),
            (0, LANCER, 31, 20),
            (0, HARVESTER, 32, 20),
            (0, SENTINEL, 33, 20),
            (0, HARVESTER, 34, 20),
        ],
        &[],
    );
    let input = pad_input(&mut game);
    let mut all: Vec<UnitId> = game.state.units().iter().map(|u| u.id).collect();
    all.sort_unstable();
    game.presentation.selection.units = all;
    (game, input)
}

#[test]
fn rt_walks_the_kinds_in_roster_order_then_back_to_the_group() {
    let (mut game, mut input) = mixed();
    let all = game.presentation.selection.units.clone();
    for kind in [HARVESTER, LANCER, SENTINEL] {
        press(&mut game, &mut input, PadButton::RightTrigger);
        assert_eq!(
            game.presentation.selection.units,
            units_of(&game, 0, kind),
            "{kind:?}"
        );
    }
    press(&mut game, &mut input, PadButton::RightTrigger);
    assert_eq!(game.presentation.selection.units, all);
    press(&mut game, &mut input, PadButton::RightTrigger);
    assert_eq!(
        game.presentation.selection.units,
        units_of(&game, 0, HARVESTER)
    );
}

#[test]
fn rt_forgets_its_group_once_the_selection_changes_otherwise() {
    let (mut game, mut input) = mixed();
    press(&mut game, &mut input, PadButton::RightTrigger);
    let fresh = vec![unit_at(&game, 31, 20), unit_at(&game, 33, 20)];
    game.presentation.selection.units = fresh;
    press(&mut game, &mut input, PadButton::RightTrigger);
    assert_eq!(
        game.presentation.selection.units,
        units_of(&game, 0, LANCER)
    );
}

/// Publishes the panel as the HUD's layout pass does before input, so RT
/// reads the All button the frame shows.
fn publish(game: &Game, input: &InputState) {
    *game.presentation.panel_model.borrow_mut() =
        crate::panel::build_for_input(&game.view(), &BindingMap::classic(), input);
}

#[test]
fn rt_widens_one_kind_to_every_one_of_it_as_the_all_button_does() {
    let mut game = scene(
        &[
            (0, SENTINEL, 30, 20),
            (0, SENTINEL, 31, 20),
            (0, SENTINEL, 60, 36),
            (0, LANCER, 32, 20),
            (1, SENTINEL, 40, 30),
        ],
        &[],
    );
    let mut input = pad_input(&mut game);
    game.presentation.selection.units = vec![unit_at(&game, 30, 20), unit_at(&game, 31, 20)];
    publish(&game, &input);
    press(&mut game, &mut input, PadButton::RightTrigger);
    assert_eq!(
        game.presentation.selection.units,
        units_of(&game, 0, SENTINEL),
        "every own Sentinel, the far one too, and no enemy"
    );
    assert!(!denied(&game));
}

#[test]
fn rt_refuses_one_kind_once_every_one_is_selected() {
    let mut game = scene(&[(0, SENTINEL, 30, 20), (0, SENTINEL, 31, 20)], &[]);
    let mut input = pad_input(&mut game);
    let both = units_of(&game, 0, SENTINEL);
    game.presentation.selection.units = both.clone();
    publish(&game, &input);
    press(&mut game, &mut input, PadButton::RightTrigger);
    assert!(denied(&game));
    assert_eq!(game.presentation.selection.units, both);
}

#[test]
fn rt_walks_structure_kinds_in_roster_order_then_widens_one_kind() {
    let mut game = scene(
        &[],
        &[
            (0, BuildingKind::Turret, 26, 16),
            (0, BuildingKind::Array, 30, 16),
            (0, BuildingKind::Turret, 36, 16),
        ],
    );
    let mut input = pad_input(&mut game);
    let turret = building_at(&game, 26, 16);
    let array = building_at(&game, 30, 16);
    let other = building_at(&game, 36, 16);
    let mut mixed = vec![turret, array];
    mixed.sort_unstable();
    game.presentation.selection.buildings = mixed.clone();
    for step in [vec![array], vec![turret], mixed] {
        press(&mut game, &mut input, PadButton::RightTrigger);
        assert_eq!(game.presentation.selection.buildings, step);
    }
    game.presentation.selection.buildings = vec![turret];
    publish(&game, &input);
    press(&mut game, &mut input, PadButton::RightTrigger);
    let mut turrets = vec![turret, other];
    turrets.sort_unstable();
    assert_eq!(game.presentation.selection.buildings, turrets);
}

#[test]
fn a_held_left_trigger_drops_with_a_transition_or_its_pad() {
    let mut game = scene(&[], &[]);
    let mut input = pad_input(&mut game);
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    input.reset_transient();
    assert!(!input.queue_held());
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    feed(
        &mut game,
        &mut input,
        &[RawEvent::PadDisconnected { pad: PAD }],
    );
    assert!(!input.queue_held());
}

#[test]
fn a_new_session_forgets_the_last_tap_and_the_narrowed_group() {
    let (mut game, mut input) = mixed();
    press(&mut game, &mut input, PadButton::RightTrigger);
    press(&mut game, &mut input, PadButton::South);
    assert!(input.narrowed.is_some() && input.last_pad_tap.is_some());
    input.reset_session();
    assert!(input.narrowed.is_none() && input.last_pad_tap.is_none());
}

#[test]
fn a_disconnect_later_in_the_batch_drops_what_the_pad_held() {
    let mut game = scene(&[(0, SENTINEL, 36, 20)], &[]);
    let mut input = pad_input(&mut game);
    feed(
        &mut game,
        &mut input,
        &[
            down(PadButton::LeftTrigger),
            down(PadButton::South),
            RawEvent::PadDisconnected { pad: PAD },
        ],
    );
    assert!(!input.queue_held(), "the trigger went with its pad");
    assert!(input.pad_press.is_none(), "so did the press");
}

#[test]
fn a_press_after_another_in_the_batch_hit_tests_the_layout_it_brought_up() {
    let mut game = scene(&[(0, SENTINEL, 36, 20)], &[]);
    let mut input = pad_input(&mut game);
    rest_on(&mut game, &mut input, CENTER);
    // A stand-in HUD whose chrome covers the reticle once anything is
    // selected.
    let relayout = |game: &Game, _: &InputState| {
        let mut layout = game.presentation.layout.get();
        layout.performance = if game.presentation.selection.units.is_empty() {
            macroquad::prelude::Rect::default()
        } else {
            macroquad::prelude::Rect::new(0.0, 0.0, 1280.0, 800.0)
        };
        game.presentation.layout.set(layout);
    };
    super::super::apply_events(
        &mut game,
        &mut input,
        &BindingMap::classic(),
        &[
            down(PadButton::LeftShoulder),
            up(PadButton::LeftShoulder),
            down(PadButton::West),
        ],
        relayout,
    );
    assert_eq!(game.presentation.selection.units.len(), 1);
    assert!(game.pending.is_empty(), "X met the chrome LB brought up");
    assert!(denied(&game));
}

#[test]
fn x_from_workers_names_their_own_structure_before_an_enemy_beside_it() {
    let mut game = scene(
        &[
            (0, HARVESTER, 26, 20),
            (0, SENTINEL, 26, 22),
            (1, SENTINEL, 31, 20),
        ],
        &[(0, BuildingKind::Array, 30, 20)],
    );
    let mut input = pad_input(&mut game);
    let (harvester, sentinel) = (unit_at(&game, 26, 20), unit_at(&game, 26, 22));
    let (array, enemy) = (building_at(&game, 30, 20), unit_at(&game, 31, 20));
    wound_building(&mut game, array, 10);
    // On the Array's tile, with the enemy inside the reticle's reach.
    rest_on(&mut game, &mut input, vec2(30.9, 20.5));
    game.presentation.selection.units = vec![harvester];
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Repair {
            units: vec![harvester],
            building: array,
            queue: false,
        }
    );
    game.pending.clear();
    // An army there still takes the enemy, as a right-click does.
    game.presentation.selection.units = vec![sentinel];
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Attack {
            units: vec![sentinel],
            target: aftermine_sim::Target::Unit(enemy).into(),
            queue: false,
        }
    );
}

#[test]
fn x_from_riders_boards_the_transport_bracketed_before_an_enemy_beside_it() {
    let mut game = scene(
        &[
            (0, LANCER, 26, 20),
            (0, UnitKind::Skyhook, 30, 20),
            (1, SENTINEL, 32, 20),
        ],
        &[],
    );
    // Zoomed out, the reticle reaches both from between them, while the
    // enemy stands beyond what a click on it would count as the Skyhook.
    game.presentation.camera.show(CENTER, 16.0);
    let mut input = pad_input(&mut game);
    let (lancer, skyhook) = (unit_at(&game, 26, 20), unit_at(&game, 30, 20));
    rest_on(&mut game, &mut input, vec2(31.5, 20.5));
    assert_eq!(
        input.reticle.snap(),
        Some(Picked::Unit(skyhook, aftermine_sim::PlayerId(0)))
    );
    game.presentation.selection.units = vec![lancer];
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Load {
            units: vec![lancer],
            transport: skyhook,
            queue: false,
        }
    );
}

#[test]
fn a_press_and_the_last_disconnect_in_one_batch_hand_the_pointer_back() {
    let mut game = scene(&[(0, SENTINEL, 36, 20)], &[]);
    let mut input = pad_input(&mut game);
    feed(
        &mut game,
        &mut input,
        &[
            down(PadButton::South),
            RawEvent::PadDisconnected { pad: PAD },
        ],
    );
    assert_eq!(
        input.last_pointer,
        crate::platform::Hands::BUILD.pointer,
        "no reticle is left without a controller"
    );
}

#[test]
fn the_minimap_under_the_reticle_takes_neither_a_nor_x() {
    let mut game = scene(&[(0, SENTINEL, 36, 20)], &[]);
    let mut input = pad_input(&mut game);
    game.presentation.selection.units = vec![unit_at(&game, 36, 20)];
    let mut layout = game.presentation.layout.get();
    layout.minimap = macroquad::prelude::Rect::new(0.0, 0.0, 1280.0, 800.0);
    game.presentation.layout.set(layout);
    rest_on(&mut game, &mut input, CENTER);
    press(&mut game, &mut input, PadButton::West);
    assert!(game.pending.is_empty());
    assert!(denied(&game));
    game.presentation.sounds_pending.clear();
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    assert!(input.pad_press.is_none());
    assert!(denied(&game));
}

/// Zoomed out, so the reticle's reach spans a tile and a half.
fn wide_reach(game: &mut Game) {
    game.presentation.camera.show(CENTER, 16.0);
}

#[test]
fn x_from_workers_on_scrap_beside_their_structure_harvests() {
    let mut game = scene(
        &[(0, HARVESTER, 36, 20)],
        &[(0, BuildingKind::Array, 39, 20)],
    );
    wide_reach(&mut game);
    let mut input = pad_input(&mut game);
    let harvester = unit_at(&game, 36, 20);
    let array = building_at(&game, 39, 20);
    wound_building(&mut game, array, 10);
    game.presentation.selection.units = vec![harvester];
    // The drill's scrap lies at (40, 20), right beside the wounded Array.
    rest_on(&mut game, &mut input, vec2(40.5, 20.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Harvest {
            units: vec![harvester],
            node: chassis::grid::TilePos::new(40, 20),
            queue: false,
        }
    );
}

#[test]
fn x_moves_to_the_ground_beside_your_own_unit_not_onto_it() {
    let mut game = scene(&[(0, SENTINEL, 30, 20), (0, HARVESTER, 37, 25)], &[]);
    wide_reach(&mut game);
    let mut input = pad_input(&mut game);
    let sentinel = unit_at(&game, 30, 20);
    game.presentation.selection.units = vec![sentinel];
    // Inside the reticle's reach of the Harvester, which it brackets.
    rest_on(&mut game, &mut input, vec2(36.5, 24.5));
    assert!(matches!(input.reticle.snap(), Some(Picked::Unit(..))));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Advance {
            units: vec![sentinel],
            goal: chassis::grid::TilePos::new(36, 24),
            queue: false,
        }
    );
}

#[test]
fn x_lays_patrol_waypoints_and_rallies_on_the_ground() {
    let mut game = scene(
        &[(0, SENTINEL, 30, 20), (1, SENTINEL, 34, 24)],
        &[(0, BuildingKind::Fabricator, 26, 18)],
    );
    wide_reach(&mut game);
    let mut input = pad_input(&mut game);
    let fabricator = building_at(&game, 26, 18);
    // Beside the enemy, inside the reticle's reach of it.
    let ground = vec2(33.5, 24.5);
    game.presentation.selection.units = vec![unit_at(&game, 30, 20)];
    super::super::dispatch_action(
        &mut game,
        &mut input,
        &BindingMap::classic(),
        Action::Patrol,
    );
    rest_on(&mut game, &mut input, ground);
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        input.patrol_route.as_deref(),
        Some(&[chassis::grid::TilePos::new(33, 24)][..]),
        "the waypoint stays on the ground"
    );
    input.patrol_route = None;
    game.presentation.selection.units.clear();
    game.presentation.selection.buildings = vec![fabricator];
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::SetRally {
            building: fabricator,
            rally: Some(aftermine_sim::Rally::Tile(chassis::grid::TilePos::new(
                33, 24
            ))),
        }
    );
}

#[test]
fn a_double_tap_over_salvage_keeps_the_pile() {
    // A Sentinel beside the drill's scrap, inside the reticle's reach of it
    // but off its body, and another in view.
    let mut game = scene(&[(0, SENTINEL, 41, 21), (0, SENTINEL, 34, 24)], &[]);
    wide_reach(&mut game);
    let mut input = pad_input(&mut game);
    rest_on(&mut game, &mut input, vec2(40.5, 20.5));
    press(&mut game, &mut input, PadButton::South);
    input.now += 0.2;
    press(&mut game, &mut input, PadButton::South);
    assert!(game.presentation.selection.units.is_empty());
    assert_eq!(
        game.presentation.selection.pile,
        Some(chassis::grid::TilePos::new(40, 20))
    );
}

#[test]
fn x_from_defenses_takes_the_nearest_enemy_they_can_fire_at() {
    let mut game = scene(
        &[(1, UnitKind::Skyhook, 31, 20), (1, SENTINEL, 32, 20)],
        &[(0, BuildingKind::Turret, 28, 20)],
    );
    wide_reach(&mut game);
    let mut input = pad_input(&mut game);
    let turret = building_at(&game, 28, 20);
    let sentinel = unit_at(&game, 32, 20);
    game.presentation.selection.buildings = vec![turret];
    // Nearer the aircraft, which a Turret cannot fire at, with the
    // Sentinel also in reach.
    rest_on(&mut game, &mut input, vec2(31.3, 20.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::FocusFire {
            buildings: vec![turret],
            target: aftermine_sim::Target::Unit(sentinel).into(),
        }
    );
}

#[test]
fn a_tap_keeps_the_unit_it_bracketed_after_that_unit_moves_on() {
    let mut game = scene(&[(0, SENTINEL, 30, 20), (0, SENTINEL, 30, 26)], &[]);
    let mut input = pad_input(&mut game);
    let mover = unit_at(&game, 30, 20);
    rest_on(&mut game, &mut input, vec2(30.5, 20.5));
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    // The Sentinel walks out of the reticle's reach before A comes up.
    let run = PlayerCommand {
        player: aftermine_sim::PlayerId(0),
        command: Command::Run {
            units: vec![mover],
            goal: chassis::grid::TilePos::new(36, 20),
            queue: false,
        },
    };
    let _ = game.state.tick(&[run]);
    for _ in 0..40 {
        let _ = game.state.tick(&[]);
    }
    assert!(
        pos(&game, mover).distance(vec2(30.5, 20.5)) > 1.0,
        "it left the reticle's reach"
    );
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert_eq!(game.presentation.selection.units, vec![mover]);
}

#[test]
fn x_never_aims_at_what_only_the_debug_overlay_sees() {
    // An enemy far beyond your Sentinel's sight.
    let mut game = scene(&[(0, SENTINEL, 14, 10), (1, SENTINEL, 50, 30)], &[]);
    wide_reach(&mut game);
    let mut input = pad_input(&mut game);
    let sentinel = unit_at(&game, 14, 10);
    game.presentation.overlay = true;
    game.presentation.selection.units = vec![sentinel];
    // Inside the reticle's reach of the hidden enemy.
    rest_on(&mut game, &mut input, vec2(49.3, 30.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Advance {
            units: vec![sentinel],
            goal: chassis::grid::TilePos::new(49, 30),
            queue: false,
        }
    );
}

#[test]
fn x_from_workers_on_scrap_harvests_beside_an_enemy_an_army_attacks() {
    let mut game = scene(
        &[
            (0, HARVESTER, 36, 20),
            (0, SENTINEL, 34, 24),
            (1, SENTINEL, 41, 21),
        ],
        &[],
    );
    wide_reach(&mut game);
    let mut input = pad_input(&mut game);
    let (harvester, army) = (unit_at(&game, 36, 20), unit_at(&game, 34, 24));
    let enemy = unit_at(&game, 41, 21);
    // On the drill's scrap at (40, 20), inside the reticle's reach of the
    // enemy but off its body.
    rest_on(&mut game, &mut input, vec2(40.5, 20.5));
    game.presentation.selection.units = vec![harvester];
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Harvest {
            units: vec![harvester],
            node: chassis::grid::TilePos::new(40, 20),
            queue: false,
        }
    );
    game.pending.clear();
    game.presentation.selection.units = vec![army];
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::Attack {
            units: vec![army],
            target: aftermine_sim::Target::Unit(enemy).into(),
            queue: false,
        }
    );
}

#[test]
fn x_rallies_onto_your_own_unit_in_reach() {
    let mut game = scene(
        &[(0, SENTINEL, 34, 24)],
        &[(0, BuildingKind::Fabricator, 26, 18)],
    );
    wide_reach(&mut game);
    let mut input = pad_input(&mut game);
    let (sentinel, fabricator) = (unit_at(&game, 34, 24), building_at(&game, 26, 18));
    game.presentation.selection.buildings = vec![fabricator];
    // A tile from the Sentinel, off its body.
    rest_on(&mut game, &mut input, vec2(33.5, 24.5));
    press(&mut game, &mut input, PadButton::West);
    assert_eq!(
        only_command(&game),
        Command::SetRally {
            building: fabricator,
            rally: Some(aftermine_sim::Rally::Unit(sentinel)),
        }
    );
}

#[test]
fn a_double_tap_on_a_structure_keeps_to_its_kind_beside_a_unit() {
    // A Skyhook hovers by the first Foundry's center, beyond the reticle's
    // reach of the footprint's far corner.
    let mut game = scene(
        &[
            (0, UnitKind::Skyhook, 30, 20),
            (0, UnitKind::Skyhook, 26, 24),
        ],
        &[
            (0, BuildingKind::Foundry, 30, 20),
            (0, BuildingKind::Foundry, 36, 20),
        ],
    );
    let mut input = pad_input(&mut game);
    rest_on(&mut game, &mut input, vec2(31.9, 21.9));
    press(&mut game, &mut input, PadButton::South);
    input.now += 0.2;
    press(&mut game, &mut input, PadButton::South);
    assert!(game.presentation.selection.units.is_empty());
    assert_eq!(
        game.presentation.selection.buildings,
        vec![building_at(&game, 30, 20), building_at(&game, 36, 20)]
    );
}
