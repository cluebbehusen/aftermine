//! Taps on HUD chrome while another finger rests on the battlefield, as a
//! hand holding an iPad or panning with a thumb does: the tapping finger
//! presses what it landed on, and the resting finger's lift neither selects
//! nor fires a long-press.

use super::*;
use crate::layout::GroupSlot;
use macroquad::math::Rect;

/// Where `world` sits on screen, checked clear of the HUD the test laid
/// out.
fn open_screen_point(game: &Game, world: Vec2) -> Vec2 {
    let p = game.presentation.camera.to_screen(world);
    assert!(!click_on_hud(game, p), "premise: {p} is battlefield");
    p
}

/// A press and lift of finger `id` at `p` while another finger stays down.
fn tap_beside(game: &mut Game, input: &mut InputState, id: u64, p: Vec2) {
    input.now += 0.1;
    apply_events(game, input, &[touch_down(id, p)]);
    input.now += 0.05;
    update_touch(game, input);
    apply_events(game, input, &[touch_up(id, p)]);
}

/// Waits out a resting finger's long-press window, a frame at a time.
fn outlast_the_long_press(game: &mut Game, input: &mut InputState) {
    let end = input.now + f64::from(input.touch_prefs.long_press_ms) / 1000.0 + 0.1;
    while input.now < end {
        input.now += 0.05;
        update_touch(game, input);
        assert_eq!(
            long_press_progress(input),
            None,
            "the resting finger charges"
        );
    }
}

fn trains(game: &Game, foundry: aftermine_sim::BuildingId) -> usize {
    game.pending
        .iter()
        .filter(|pc| matches!(pc.command, Command::Train { building, .. } if building == foundry))
        .count()
}

/// The own Foundry selected with its HUD laid out, and the rect of a card
/// that trains from it.
fn foundry_train_card() -> (Game, aftermine_sim::BuildingId, Rect) {
    let mut game = headless_game();
    let foundry = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player == game.presentation.human)
        .expect("own Foundry")
        .id;
    game.presentation.selection.buildings = vec![foundry];
    relayout::relayout(&game, &InputState::new());
    let layout = game.presentation.layout.get();
    let card = layout.cards[..layout.card_count]
        .iter()
        .find(|(_, action)| {
            matches!(
                action,
                crate::panel::CardAction::Dispatch(Action::TrainSlot(_))
            )
        })
        .expect("premise: the Foundry offers a train card")
        .0;
    (game, foundry, card)
}

/// Lays the HUD out and returns the rect of group 1's slot, which recalls.
fn group_one_slot(game: &Game, input: &InputState) -> Rect {
    relayout::relayout(game, input);
    game.presentation
        .layout
        .get()
        .group_slots
        .into_iter()
        .flatten()
        .find(|(_, slot)| *slot == GroupSlot::Recall(1))
        .expect("premise: the column offers group 1")
        .0
}

/// Lifts finger `id` at `p`, or has the platform take it away.
fn leave(game: &mut Game, input: &mut InputState, id: u64, p: Vec2, cancelled: bool) {
    let event = if cancelled {
        touch_cancel(id)
    } else {
        touch_up(id, p)
    };
    apply_events(game, input, &[event]);
}

#[test]
fn a_second_finger_trains_from_a_card_while_the_first_rests_or_pans() {
    for pans in [false, true] {
        let (mut game, foundry, card) = foundry_train_card();
        let mut input = InputState::new();
        // On an own unit, where a tap or a long-press would select it.
        let (_, unit) = own_fighter(&game);
        let mut rest = open_screen_point(&game, unit);

        input.now = 5.0;
        apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
        if pans {
            rest += vec2(-80.0, 0.0);
            apply_events(&mut game, &mut input, &[touch_move(1, rest)]);
        }
        tap_beside(&mut game, &mut input, 2, card.center());
        assert_eq!(trains(&game, foundry), 1, "pans {pans}: the card trains");

        if pans {
            let camera = game.presentation.camera.center;
            rest += vec2(-80.0, 0.0);
            apply_events(&mut game, &mut input, &[touch_move(1, rest)]);
            assert_ne!(
                game.presentation.camera.center, camera,
                "the pan carries on"
            );
        }
        outlast_the_long_press(&mut game, &mut input);
        apply_events(&mut game, &mut input, &[touch_up(1, rest)]);
        assert_eq!(
            game.presentation.selection.buildings,
            vec![foundry],
            "pans {pans}: the resting finger selects nothing"
        );
        assert!(game.presentation.selection.units.is_empty());
        assert_eq!(game.pending.len(), 1, "pans {pans}: {:?}", game.pending);
    }
}

#[test]
fn a_second_finger_recalls_a_group_while_the_first_rests_on_open_ground() {
    let mut game = headless_game();
    let mut input = InputState::new();
    let (fighter, at) = own_fighter(&game);
    input.groups[0] = Group::Units(vec![fighter]);
    let slot = group_one_slot(&game, &input);
    // Open ground, where a tap would deselect and a long-press would order
    // the recalled group there.
    let rest = open_screen_point(&game, at + vec2(4.0, 2.0));

    input.now = 5.0;
    apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
    tap_beside(&mut game, &mut input, 2, slot.center());
    assert_eq!(
        game.presentation.selection.units,
        vec![fighter],
        "the slot recalls"
    );

    outlast_the_long_press(&mut game, &mut input);
    apply_events(&mut game, &mut input, &[touch_up(1, rest)]);
    assert!(game.pending.is_empty(), "no long-press: {:?}", game.pending);
    assert_eq!(
        game.presentation.selection.units,
        vec![fighter],
        "the resting finger's lift keeps the group"
    );
}

#[test]
fn a_second_finger_held_on_a_slot_saves_the_selection_there() {
    let mut game = headless_game();
    let mut input = InputState::new();
    let (fighter, at) = own_fighter(&game);
    let harvester = game
        .state
        .units()
        .iter()
        .find(|u| u.player == game.presentation.human && u.kind == UnitKind::Harvester)
        .expect("own Harvester")
        .id;
    input.groups[0] = Group::Units(vec![harvester]);
    game.presentation.selection.units = vec![fighter];
    let slot = group_one_slot(&game, &input);
    let rest = open_screen_point(&game, at + vec2(4.0, 2.0));

    input.now = 5.0;
    apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
    input.now = 5.1;
    apply_events(&mut game, &mut input, &[touch_down(2, slot.center())]);
    input.now = 5.1 + (TOUCH_REST_MS + 50.0) / 1000.0;
    update_touch(&mut game, &mut input);
    let (ring, progress) = group_press_progress(&input).expect("the slot charges");
    assert_eq!(ring, slot.center());
    assert!(progress > 0.0 && progress < 1.0);
    outlast_the_long_press(&mut game, &mut input);
    assert_eq!(
        input.groups[0],
        Group::Units(vec![fighter]),
        "the hold saves the selection to group 1"
    );
    apply_events(&mut game, &mut input, &[touch_up(2, slot.center())]);
    assert_eq!(input.last_recall, None, "and its lift recalls nothing");
    apply_events(&mut game, &mut input, &[touch_up(1, rest)]);
    assert_eq!(game.presentation.selection.units, vec![fighter]);
    assert!(game.pending.is_empty(), "{:?}", game.pending);
}

#[test]
fn a_second_finger_held_on_the_weld_card_switches_auto_weld() {
    let mut game = headless_game();
    let mut input = InputState::new();
    let (_, at) = own_fighter(&game);
    let worker = game
        .state
        .units()
        .iter()
        .find(|u| u.player == game.presentation.human && u.worker.is_some())
        .expect("own worker")
        .id;
    game.presentation.selection.units = vec![worker];
    relayout::relayout(&game, &input);
    let index = game
        .presentation
        .panel_model
        .borrow()
        .as_ref()
        .and_then(|panel| {
            panel
                .cards
                .iter()
                .position(|card| card.title == "Repair" && card.second_act().is_some())
        })
        .expect("premise: a worker's panel offers Weld with an automatic mode");
    let weld = game.presentation.layout.get().cards[index].0;
    let rest = open_screen_point(&game, at + vec2(4.0, 2.0));

    input.now = 5.0;
    apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
    input.now = 5.1;
    apply_events(&mut game, &mut input, &[touch_down(2, weld.center())]);
    input.now = 5.1 + (TOUCH_REST_MS + 50.0) / 1000.0;
    update_touch(&mut game, &mut input);
    let (ring, progress) = card_press_progress(&input).expect("the card charges");
    assert_eq!(ring, weld.center());
    assert!(progress > 0.0 && progress < 1.0);
    outlast_the_long_press(&mut game, &mut input);
    assert!(
        matches!(
            game.pending.as_slice(),
            [PlayerCommand {
                command: Command::SetIdleWeld { units, enabled: false },
                ..
            }] if *units == [worker]
        ),
        "the hold switches auto-weld off: {:?}",
        game.pending
    );
    apply_events(&mut game, &mut input, &[touch_up(2, weld.center())]);
    assert_eq!(input.armed_mode(), None, "and its lift arms no weld");
    apply_events(&mut game, &mut input, &[touch_up(1, rest)]);
    assert_eq!(game.pending.len(), 1, "{:?}", game.pending);
}

#[test]
fn a_chrome_finger_whose_partner_leaves_first_still_presses_as_it_lifts() {
    for cancelled in [false, true] {
        let (mut game, foundry, card) = foundry_train_card();
        let mut input = InputState::new();
        let (fighter, at) = own_fighter(&game);
        let rest = open_screen_point(&game, at);
        input.now = 5.0;
        apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
        input.now = 5.1;
        apply_events(&mut game, &mut input, &[touch_down(2, card.center())]);
        input.now = 5.15;
        leave(&mut game, &mut input, 1, rest, cancelled);
        assert_eq!(
            game.presentation.selection.buildings,
            vec![foundry],
            "cancelled {cancelled}: the battlefield finger selects nothing"
        );
        input.now = 5.2;
        apply_events(&mut game, &mut input, &[touch_up(2, card.center())]);
        assert_eq!(
            trains(&game, foundry),
            1,
            "cancelled {cancelled}: the card trains"
        );

        input.groups[0] = Group::Units(vec![fighter]);
        let slot = group_one_slot(&game, &input);
        input.now = 10.0;
        apply_events(&mut game, &mut input, &[touch_down(3, rest)]);
        input.now = 10.1;
        apply_events(&mut game, &mut input, &[touch_down(4, slot.center())]);
        input.now = 10.15;
        leave(&mut game, &mut input, 3, rest, cancelled);
        input.now = 10.2;
        apply_events(&mut game, &mut input, &[touch_up(4, slot.center())]);
        assert_eq!(
            game.presentation.selection.units,
            vec![fighter],
            "cancelled {cancelled}: the slot recalls"
        );
    }
}

#[test]
fn a_second_finger_cancels_from_the_ribbon_cross_and_builds_from_the_check() {
    let mut game = headless_game();
    let mut input = InputState::new();
    let (fighter, at) = own_fighter(&game);
    game.presentation.selection.units = vec![fighter];
    dispatch_action(&mut game, &mut input, Action::Hunt);
    assert!(input.armed(ClickVerb::Hunt), "premise: Hunt is armed");
    relayout::relayout(&game, &input);
    let cross = game.presentation.layout.get().ribbon_cancel;
    assert!(cross.w > 0.0, "premise: the ribbon shows its cross");
    let rest = open_screen_point(&game, at + vec2(4.0, 2.0));
    input.now = 5.0;
    apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
    tap_beside(&mut game, &mut input, 2, cross.center());
    assert_eq!(input.armed_mode(), None, "the cross stands Hunt down");
    apply_events(&mut game, &mut input, &[touch_up(1, rest)]);
    assert!(game.pending.is_empty(), "{:?}", game.pending);
    assert_eq!(game.presentation.selection.units, vec![fighter]);

    let kind = aftermine_sim::BuildingKind::Turret;
    let (mut game, mut input, _) = armed_placement(kind);
    let foundry = game.state.buildings()[0].anchor;
    tap_world(
        &mut game,
        &mut input,
        vec2(foundry.x as f32 + 3.5, foundry.y as f32 + 3.5),
    );
    let ghost = input.ghost_anchor().expect("premise: a ghost is down");
    let check = lay_out_ghost_controls(&game, &input).confirm;
    let rest = open_screen_point(&game, vec2(foundry.x as f32 - 3.5, foundry.y as f32 + 8.5));
    apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
    tap_beside(&mut game, &mut input, 2, check.center());
    assert_eq!(staged_anchors(&game), vec![ghost], "the check builds");
    apply_events(&mut game, &mut input, &[touch_up(1, rest)]);
    assert_eq!(staged_anchors(&game), vec![ghost], "and the rest adds none");
}

/// Publishes a panel band whose top edge sits just under `p`, so a finger
/// can land on the band and lift at `p` without leaving the slop, and
/// returns where it lands.
fn band_just_under(game: &Game, p: Vec2) -> Vec2 {
    game.presentation.layout.set(bare_layout(p.y + 6.0, 1280.0));
    let landing = p + vec2(0.0, 8.0);
    assert!(click_on_hud(game, landing) && !click_on_hud(game, p));
    landing
}

#[test]
fn a_finger_off_chrome_beside_another_never_reaches_the_world() {
    // Lifting on a unit, where a lone finger's tap would select it.
    let mut game = headless_game();
    let mut input = InputState::new();
    let (_, at) = own_fighter(&game);
    let rest = open_screen_point(&game, at + vec2(4.0, -2.0));
    let unit = open_screen_point(&game, at);
    let landing = band_just_under(&game, unit);
    input.now = 5.0;
    apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
    input.now += 0.1;
    apply_events(
        &mut game,
        &mut input,
        &[touch_down(2, landing), touch_up(2, unit)],
    );
    assert!(
        game.presentation.selection.units.is_empty(),
        "a chrome-born lift beside another selects nothing"
    );

    // Lifting on open ground while placing, where a lone finger's tap
    // would drop the ghost.
    let kind = aftermine_sim::BuildingKind::Turret;
    let (mut game, mut input, _) = armed_placement(kind);
    let foundry = game.state.buildings()[0].anchor;
    let ground = open_screen_point(&game, vec2(foundry.x as f32 + 3.5, foundry.y as f32 + 3.5));
    let rest = open_screen_point(&game, vec2(foundry.x as f32 - 3.5, foundry.y as f32 + 0.5));
    let landing = band_just_under(&game, ground);
    input.now = 5.0;
    apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
    input.now += 0.1;
    apply_events(
        &mut game,
        &mut input,
        &[touch_down(2, landing), touch_up(2, ground)],
    );
    assert_eq!(input.placing_kind(), Some(kind), "still placing");
    assert_eq!(input.ghost_anchor(), None, "and no ghost");
}

#[test]
fn minimap_ghost_and_handle_fingers_keep_their_jobs_beside_another() {
    // The minimap jumps the camera where the finger lands and steers it
    // as the finger drags.
    let mut game = headless_game();
    let mut input = InputState::new();
    let minimap = publish_minimap(&game);
    // Zoomed in, so the camera has somewhere to go.
    game.presentation.camera.zoom_at(vec2(640.0, 400.0), 4.0);
    game.presentation.camera.update(1.0);
    game.presentation.camera.center = vec2(4.0, 4.0);
    game.presentation.camera.pan(Vec2::ZERO);
    let rest = vec2(640.0, 400.0);
    assert!(!click_on_hud(&game, rest) && !minimap.contains(rest));
    input.now = 5.0;
    apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
    let before = game.presentation.camera.center;
    let first = vec2(minimap.x + 40.0, minimap.y + 40.0);
    apply_events(&mut game, &mut input, &[touch_down(2, first)]);
    let jumped = game.presentation.camera.center;
    assert_ne!(jumped, before, "the landing jumps the camera");
    let second = vec2(minimap.right() - 40.0, minimap.bottom() - 40.0);
    apply_events(&mut game, &mut input, &[touch_move(2, second)]);
    assert_ne!(
        game.presentation.camera.center, jumped,
        "the drag steers it"
    );
    apply_events(&mut game, &mut input, &[touch_up(2, second)]);
    apply_events(&mut game, &mut input, &[touch_up(1, rest)]);
    assert!(game.presentation.selection.units.is_empty());

    // The ghost and its handle only move: a lift beside another builds
    // nothing, whatever the ghost's controls lie under.
    let kind = aftermine_sim::BuildingKind::Barricade;
    let (mut game, mut input, _) = armed_placement(kind);
    let foundry = game.state.buildings()[0].anchor;
    tap_world(
        &mut game,
        &mut input,
        vec2(foundry.x as f32 + 3.5, foundry.y as f32 + 3.5),
    );
    let ghost = input.ghost_anchor().expect("premise: a ghost is down");
    let controls = lay_out_ghost_controls(&game, &input);
    assert!(
        controls.handle.w > 0.0,
        "premise: the ghost offers a handle"
    );
    let on_ghost = game
        .presentation
        .camera
        .to_screen(vec2(ghost.x as f32 + 0.5, ghost.y as f32 + 0.5));
    let rest = open_screen_point(&game, vec2(foundry.x as f32 - 3.5, foundry.y as f32 + 8.5));
    apply_events(&mut game, &mut input, &[touch_down(1, rest)]);
    for (id, p) in [(2, on_ghost), (3, controls.handle.center())] {
        tap_beside(&mut game, &mut input, id, p);
    }
    apply_events(&mut game, &mut input, &[touch_up(1, rest)]);
    assert!(staged_anchors(&game).is_empty(), "{:?}", game.pending);
    assert_eq!(input.ghost_anchor(), Some(ghost));
    assert_eq!(input.placing_kind(), Some(kind));
}
