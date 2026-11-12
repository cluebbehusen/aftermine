//! The queue strip through the real funnel: pad events as the frame loop
//! feeds them, the HUD laid out between them, against a staged sandbox.

use super::super::tests::pad_scene::{FRAME, down, left_stick, pad_input, up};
use super::super::{InputState, apply_events, track_hands, update_held};
use super::*;
use crate::render::hud::{self, HudEnv};
use crate::tests::support::{building_at, denied, scene};
use aftermine_protocol::RawEvent;
use aftermine_sim::{BuildingKind, Command, UnitKind};
use macroquad::prelude::vec2;

/// Lays the HUD out headlessly in the 1280x800 test window and publishes
/// it, as each frame does before input.
fn relayout(game: &Game, input: &InputState) {
    hud::refresh(
        &game.view(),
        input,
        &BindingMap::classic(),
        HudEnv {
            viewport: vec2(1280.0, 800.0),
            ui: 1.0,
        },
        None,
        &|_, text: &str, size: f32| text.chars().count() as f32 * size * 0.5,
    );
}

/// Feeds events as the Playing frame does, the HUD laid out between them.
fn feed(game: &mut Game, input: &mut InputState, events: &[RawEvent]) {
    for event in events {
        track_hands(input, event, false);
    }
    relayout(game, input);
    apply_events(game, input, &BindingMap::classic(), events, relayout);
}

/// One frame of held input after a fresh layout.
fn frame(game: &mut Game, input: &mut InputState) {
    relayout(game, input);
    update_held(game, input, FRAME);
    input.now += f64::from(FRAME);
}

fn tap(game: &mut Game, input: &mut InputState, button: PadButton) {
    feed(game, input, &[down(button), up(button)]);
    frame(game, input);
}

/// A Foundry, selected, training three Harvesters.
fn foundry() -> (Game, InputState, BuildingId) {
    let mut game = scene(&[], &[(0, BuildingKind::Foundry, 30, 18)]);
    let mut input = pad_input(&mut game);
    let foundry = building_at(&game, 30, 18);
    for _ in 0..3 {
        game.issue(Command::Train {
            building: foundry,
            kind: UnitKind::Harvester,
        });
    }
    game.do_tick();
    game.presentation.selection.set_buildings(vec![foundry]);
    frame(&mut game, &mut input);
    (game, input, foundry)
}

/// The dock entries as the HUD published them.
fn published(game: &Game) -> Vec<Entry> {
    entries(&game.presentation.layout.get())
}

fn lit(input: &InputState) -> CardHit {
    input.dock.as_ref().expect("the strip is open").lit()
}

fn staged(game: &Game) -> Vec<Command> {
    game.pending.iter().map(|pc| pc.command.clone()).collect()
}

/// Walks the open strip up to Stop, which tops the dock.
fn to_stop(game: &mut Game, input: &mut InputState) {
    for _ in 0..published(game).len() {
        if lit(input).row == CardRow::Stop {
            return;
        }
        tap(game, input, PadButton::DpadUp);
    }
    assert_eq!(lit(input).row, CardRow::Stop, "Stop tops the dock");
}

#[test]
fn the_left_stick_click_lights_the_chip_nearest_the_band_and_a_cancels_it() {
    let (mut game, mut input, foundry) = foundry();
    let dock: Vec<Entry> = published(&game);
    assert_eq!(dock.len(), 4, "three queued Harvesters and Stop");
    tap(&mut game, &mut input, PadButton::LeftStick);
    let bottom = dock.last().expect("a chip").hit;
    assert_eq!(lit(&input), bottom, "the chip nearest the command band");
    assert_eq!(bottom.row, CardRow::Queue);
    tap(&mut game, &mut input, PadButton::South);
    let CardAction::CancelQueue(building, index) = bottom.action else {
        panic!("the chip cancels a queued unit: {:?}", bottom.action);
    };
    assert_eq!(building, foundry);
    assert_eq!(
        staged(&game),
        [Command::CancelTrain { building, index }],
        "A cancels the lit chip as a click does"
    );
    assert_eq!(
        game.presentation.selection.buildings(),
        vec![foundry],
        "and its release never selects at the reticle"
    );
}

#[test]
fn the_dpad_walks_the_column_and_stops_at_its_ends() {
    let (mut game, mut input, _) = foundry();
    let dock: Vec<CardHit> = published(&game).iter().map(|entry| entry.hit).collect();
    tap(&mut game, &mut input, PadButton::LeftStick);
    for expected in dock.iter().rev().skip(1) {
        tap(&mut game, &mut input, PadButton::DpadUp);
        assert_eq!(lit(&input), *expected);
    }
    assert_eq!(lit(&input).row, CardRow::Stop, "Stop tops the dock");
    assert!(!denied(&game));
    tap(&mut game, &mut input, PadButton::DpadUp);
    assert!(denied(&game), "nothing lies above Stop");
    assert_eq!(lit(&input).row, CardRow::Stop);
    tap(&mut game, &mut input, PadButton::DpadDown);
    assert_eq!(lit(&input), dock[1], "down from Stop lights the top chip");
    tap(&mut game, &mut input, PadButton::DpadRight);
    assert_eq!(
        lit(&input),
        dock[1],
        "with no drone column nothing lies right"
    );
}

/// A dock as `queue_grid` wraps five chips into two columns, Stop above
/// them and a drone column of two beside them: 44 px chips, 4 px gaps.
fn wrapped() -> LayoutModel {
    let mut layout = LayoutModel::default();
    for index in 0..5u8 {
        let (row, column) = (f32::from(index / 2), f32::from(index % 2));
        layout.queue_slots[usize::from(index)] = (
            Rect::new(8.0 + column * 48.0, 600.0 + row * 48.0, 44.0, 44.0),
            CardAction::CancelQueue(BuildingId(1), index),
        );
    }
    layout.queue_count = 5;
    layout.queue_stop = (Rect::new(8.0, 530.0, 92.0, 44.0), CardAction::ClearQueues);
    for index in 0..2u8 {
        layout.drone_slots[usize::from(index)] = (
            Rect::new(120.0, 648.0 + f32::from(index) * 48.0, 44.0, 44.0),
            CardAction::CancelDrone(UnitId(9), index),
        );
    }
    layout.drone_count = 2;
    layout
}

#[test]
fn the_dpad_moves_as_the_chips_are_drawn_when_the_queue_wraps_into_columns() {
    let entries = entries(&wrapped());
    let go = |from: CardHit, nav: Nav| toward(&entries, from.rect, nav).map(|at| entries[at].hit);
    let find = |row: CardRow, index: usize| {
        entries
            .iter()
            .map(|entry| entry.hit)
            .find(|hit| (hit.row, hit.index) == (row, index))
            .expect("published")
    };
    let chip = |index| find(CardRow::Queue, index);
    let drone = |index| find(CardRow::Drones, index);
    let stop = find(CardRow::Stop, 0);

    assert_eq!(go(chip(0), Nav::Right), Some(chip(1)), "across a row");
    assert_eq!(go(chip(1), Nav::Left), Some(chip(0)));
    assert_eq!(go(chip(0), Nav::Down), Some(chip(2)), "down a column");
    assert_eq!(go(chip(3), Nav::Up), Some(chip(1)));
    assert_eq!(
        go(chip(3), Nav::Down),
        Some(chip(4)),
        "onto a short last row"
    );
    assert_eq!(go(chip(0), Nav::Up), Some(stop));
    assert_eq!(go(chip(1), Nav::Up), Some(stop));
    assert_eq!(go(stop, Nav::Down), Some(chip(0)));
    assert_eq!(go(chip(3), Nav::Right), Some(drone(0)), "out to the drones");
    assert_eq!(go(drone(1), Nav::Left), Some(chip(4)));
    assert_eq!(go(chip(0), Nav::Left), None, "and none past an edge");
    assert_eq!(go(chip(4), Nav::Down), None);
    assert_eq!(go(stop, Nav::Up), None);
}

#[test]
fn stop_clears_every_queue_of_the_selected_producers() {
    let (mut game, mut input, foundry) = foundry();
    tap(&mut game, &mut input, PadButton::LeftStick);
    to_stop(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::South);
    let cancelled = staged(&game)
        .iter()
        .filter(|command| matches!(command, Command::CancelTrain { building, .. } if *building == foundry))
        .count();
    assert_eq!(cancelled, 3, "Clear queues refunds all three");
}

#[test]
fn b_or_another_click_closes_it_and_y_hands_over_to_the_ring() {
    let (mut game, mut input, _) = foundry();
    tap(&mut game, &mut input, PadButton::LeftStick);
    tap(&mut game, &mut input, PadButton::East);
    assert!(input.dock.is_none(), "B closes the strip");
    assert_eq!(
        game.presentation.selection.buildings().len(),
        1,
        "and keeps the selection"
    );
    tap(&mut game, &mut input, PadButton::LeftStick);
    tap(&mut game, &mut input, PadButton::LeftStick);
    assert!(input.dock.is_none(), "another click closes it");
    tap(&mut game, &mut input, PadButton::LeftStick);
    tap(&mut game, &mut input, PadButton::North);
    assert!(
        input.dock.is_none() && input.ring.is_some(),
        "Y opens the ring"
    );
    tap(&mut game, &mut input, PadButton::LeftStick);
    assert!(input.dock.is_none(), "the ring keeps the click while open");
}

#[test]
fn menu_view_and_the_left_trigger_reach_past_the_strip() {
    let (mut game, mut input, _) = foundry();
    tap(&mut game, &mut input, PadButton::LeftStick);
    tap(&mut game, &mut input, PadButton::Start);
    assert!(input.take_menu_request());
    tap(&mut game, &mut input, PadButton::Select);
    assert!(game.clock.paused);
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    assert!(input.queue_held());
    assert!(input.dock.is_some(), "none of them closes it");
}

#[test]
fn the_camera_holds_while_the_strip_is_open_and_until_the_stick_is_let_go() {
    let (mut game, mut input, _) = foundry();
    tap(&mut game, &mut input, PadButton::LeftStick);
    let held = game.presentation.camera.center;
    feed(&mut game, &mut input, &[left_stick(1.0, 0.0)]);
    for _ in 0..20 {
        frame(&mut game, &mut input);
    }
    assert_eq!(game.presentation.camera.center, held);
    tap(&mut game, &mut input, PadButton::East);
    for _ in 0..20 {
        frame(&mut game, &mut input);
    }
    assert_eq!(
        game.presentation.camera.center, held,
        "a stick still pushed as the strip closed stays still"
    );
    feed(&mut game, &mut input, &[left_stick(0.0, 0.0)]);
    frame(&mut game, &mut input);
    feed(&mut game, &mut input, &[left_stick(1.0, 0.0)]);
    frame(&mut game, &mut input);
    assert_ne!(game.presentation.camera.center, held, "until let go");
}

#[test]
fn it_closes_when_the_dock_empties_the_selection_changes_or_another_hand_takes_over() {
    let mut game = scene(&[], &[(0, BuildingKind::Foundry, 30, 18)]);
    let mut input = pad_input(&mut game);
    tap(&mut game, &mut input, PadButton::LeftStick);
    assert!(input.dock.is_none() && denied(&game), "nothing to press");

    let (mut game, mut input, _) = foundry();
    tap(&mut game, &mut input, PadButton::LeftStick);
    to_stop(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::South);
    game.do_tick();
    frame(&mut game, &mut input);
    assert!(input.dock.is_none(), "Stop emptied the dock");

    let (mut game, mut input, _) = foundry();
    tap(&mut game, &mut input, PadButton::LeftStick);
    game.presentation.selection.clear();
    frame(&mut game, &mut input);
    assert!(input.dock.is_none(), "the selection changed");

    let (mut game, mut input, _) = foundry();
    tap(&mut game, &mut input, PadButton::LeftStick);
    feed(
        &mut game,
        &mut input,
        &[RawEvent::MouseMove { x: 10.0, y: 10.0 }],
    );
    assert!(
        input.dock.is_some(),
        "a mouse that only moves leaves it open"
    );
    feed(
        &mut game,
        &mut input,
        &[RawEvent::MouseDown {
            button: aftermine_protocol::MouseButton::Left,
            x: 10.0,
            y: 10.0,
        }],
    );
    assert!(input.dock.is_none(), "a click took over");
}

#[test]
fn the_hud_hovers_the_lit_entry_and_display_chips_are_skipped() {
    let (mut game, mut input, _) = foundry();
    tap(&mut game, &mut input, PadButton::LeftStick);
    assert_eq!(
        input.chrome_hover_point(),
        Some(lit(&input).rect.center()),
        "the chip lights and shows its tooltip as on hover"
    );
    let mut layout = LayoutModel::default();
    layout.queue_slots[0] = (Rect::new(0.0, 100.0, 40.0, 40.0), CardAction::None);
    layout.queue_slots[1] = (
        Rect::new(0.0, 60.0, 40.0, 40.0),
        CardAction::CancelSite(BuildingId(7)),
    );
    layout.queue_count = 2;
    let shown: Vec<CardAction> = entries(&layout)
        .iter()
        .map(|entry| entry.hit.action)
        .collect();
    assert_eq!(shown, [CardAction::CancelSite(BuildingId(7))]);
}
