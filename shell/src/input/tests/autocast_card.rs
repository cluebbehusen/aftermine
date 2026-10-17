//! The Weld card's autocast: a right-click or a long press switches the
//! selected workers' idle welding, and neither arms a weld; a plain press
//! still does.

use super::*;
use macroquad::math::Rect;

/// Publishes the selection's command cards in one row, as drawing does,
/// and returns the Weld card's rect.
fn publish_weld(game: &mut Game, input: &InputState) -> Rect {
    let panel = crate::panel::build_for_input(&game.view(), &classic(), input).expect("a panel");
    let mut layout = bare_layout(680.0, 1200.0);
    let mut weld = None;
    for (slot, card) in panel.cards.iter().enumerate() {
        let rect = Rect::new(240.0 + 90.0 * slot as f32, 700.0, 84.0, 72.0);
        layout.cards[slot] = (rect, card.action);
        if card.title == "Weld" {
            weld = Some(rect);
        }
    }
    layout.card_count = panel.cards.len();
    game.presentation.layout.set(layout);
    *game.presentation.panel_model.borrow_mut() = Some(panel);
    weld.expect("a worker's panel offers Weld")
}

fn selected_worker(game: &mut Game) -> oxide_sim::UnitId {
    let worker = game
        .state
        .units()
        .iter()
        .find(|u| u.player == game.presentation.human && u.worker.is_some())
        .expect("the skirmish starts with a crew")
        .id;
    game.presentation.selection.units = vec![worker];
    worker
}

fn switched_off(game: &Game, worker: oxide_sim::UnitId) -> bool {
    game.pending.iter().any(|pending| {
        matches!(
            &pending.command,
            Command::SetIdleWeld { units, enabled: false } if *units == [worker]
        )
    })
}

#[test]
fn a_right_click_on_the_weld_card_switches_auto_weld_without_arming_a_weld() {
    let mut game = headless_game();
    let mut input = InputState::new();
    let worker = selected_worker(&mut game);
    let weld = publish_weld(&mut game, &input);
    apply_events(&mut game, &mut input, &[right_down(weld.center())]);
    assert!(switched_off(&game, worker), "{:?}", game.pending);
    assert_eq!(input.armed_mode(), None, "no weld is armed");
    assert_eq!(
        game.pending.len(),
        1,
        "and nothing is ordered to that point"
    );
}

#[test]
fn a_long_press_on_the_weld_card_switches_auto_weld_and_its_lift_arms_nothing() {
    let mut game = headless_game();
    let mut input = InputState::new();
    let worker = selected_worker(&mut game);
    let at = publish_weld(&mut game, &input).center();
    input.now = 5.0;
    apply_events(&mut game, &mut input, &[touch_down(1, at)]);
    input.now = 5.25;
    let (_, progress) = autocast_press_progress(&input).expect("the card charges");
    assert!(progress > 0.0 && progress < 1.0);
    assert_eq!(
        long_press_progress(&input),
        None,
        "no world ring over chrome"
    );
    input.now = 6.0;
    update_touch(&mut game, &mut input);
    assert!(switched_off(&game, worker), "{:?}", game.pending);
    apply_events(&mut game, &mut input, &[touch_up(1, at)]);
    assert_eq!(
        input.armed_mode(),
        None,
        "the lift after the hold arms no weld"
    );
    assert_eq!(game.pending.len(), 1);
}

#[test]
fn a_press_on_the_weld_card_still_arms_a_weld() {
    let mut game = headless_game();
    let mut input = InputState::new();
    selected_worker(&mut game);
    let at = publish_weld(&mut game, &input).center();
    apply_events(&mut game, &mut input, &[left_down(at), left_up(at)]);
    assert_eq!(input.armed_mode(), Some(ArmedMode::Weld));
    assert!(game.pending.is_empty());
}
