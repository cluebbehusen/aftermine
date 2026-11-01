//! Spire salvos and Fill: each strike pick fires the banner's salvo size and
//! the strike stays armed until none is ready; Fill orders a round for every
//! empty slot from a Shift-click, a right-click, or a long press.

use super::*;
use crate::render::hud::{self, HudEnv};
use macroquad::math::Rect;

/// Selected Spires, the first `loaded` of them holding a warhead.
fn spires(count: usize, loaded: usize, scrap: u32) -> (Game, Vec<aftermine_sim::BuildingId>) {
    let mut game = crate::building_actions::tests::fixture(
        aftermine_sim::BuildingKind::Spire,
        &vec![0; count],
        scrap,
    );
    let ids = game.presentation.selection.buildings.clone();
    let mut json = serde_json::to_value(&*game.state).unwrap();
    for b in json["buildings"].as_array_mut().unwrap() {
        if ids[..loaded]
            .iter()
            .any(|id| b["id"] == serde_json::json!(id))
        {
            b["magazine"]["loaded"] = 1.into();
        }
    }
    *game.state = serde_json::from_value(json).unwrap();
    (game, ids)
}

/// Lays the HUD out headlessly in the 1280x800 test window.
fn relayout(game: &Game, input: &InputState) {
    hud::refresh(
        &game.view(),
        input,
        &classic(),
        HudEnv {
            viewport: vec2(1280.0, 800.0),
            ui: 1.0,
        },
        None,
        &|_, text: &str, size: f32| text.chars().count() as f32 * size * 0.5,
    );
}

fn last_toast(game: &Game) -> Option<&str> {
    game.presentation
        .toasts
        .last()
        .map(|toast| toast.text.as_str())
}

fn click_world(game: &mut Game, input: &mut InputState, world: Vec2) {
    let p = game.presentation.camera.to_screen(world);
    apply_events(game, input, &click(p.x, p.y));
}

fn orders(game: &Game) -> Vec<aftermine_sim::BuildingId> {
    game.pending
        .iter()
        .filter_map(|pc| match pc.command {
            Command::OrderRound { building } => Some(building),
            _ => None,
        })
        .collect()
}

#[test]
fn each_pick_fires_the_salvo_and_aiming_stays_on_until_none_is_ready() {
    let (mut game, ids) = spires(3, 3, 0);
    let mut input = InputState::new();
    input.salvo = 2;
    dispatch_action(&mut game, &mut input, Action::Launch);
    assert_eq!(salvo_readout(&game.view(), &input), Some((2, 3)));
    click_world(&mut game, &mut input, vec2(20.5, 6.5));
    assert_eq!(launches(&game).len(), 2);
    assert_eq!(last_toast(&game), None, "the aim bar counts; no toast");
    assert_eq!(
        input.armed_mode(),
        Some(ArmedMode::Strike),
        "aiming stays on"
    );
    assert_eq!(
        salvo_readout(&game.view(), &input),
        Some((1, 1)),
        "the banner never offers more than are ready"
    );
    click_world(&mut game, &mut input, vec2(30.5, 8.5));
    let mut fired: Vec<_> = launches(&game)
        .into_iter()
        .map(|(spire, _)| spire)
        .collect();
    assert_eq!(launches(&game)[2].1, TilePos::new(30, 8));
    fired.sort_unstable();
    assert_eq!(fired, ids, "each Spire fires once");
    assert!(input.striking.is_none(), "the last ready warhead disarms");
    assert_eq!(input.salvo, 2, "the size is kept for the next strike");
}

#[test]
fn a_shift_click_fires_every_ready_warhead() {
    let (mut game, _) = spires(4, 3, 0);
    let mut input = InputState::new();
    dispatch_action(&mut game, &mut input, Action::Launch);
    apply_events(&mut game, &mut input, &[key_down(Key::Shift)]);
    assert_eq!(salvo_readout(&game.view(), &input), Some((3, 3)));
    click_world(&mut game, &mut input, vec2(20.5, 6.5));
    apply_events(&mut game, &mut input, &[key_up(Key::Shift)]);
    assert_eq!(launches(&game).len(), 3);
    assert!(input.striking.is_none());
    assert_eq!(input.salvo, 1, "Shift leaves the size alone");
}

#[test]
fn a_spire_loaded_while_aiming_joins_the_strike() {
    let (mut game, ids) = spires(2, 1, 0);
    let mut input = InputState::new();
    dispatch_action(&mut game, &mut input, Action::Launch);
    assert_eq!(salvo_readout(&game.view(), &input), Some((1, 1)));
    let mut json = serde_json::to_value(&*game.state).unwrap();
    for b in json["buildings"].as_array_mut().unwrap() {
        if b["id"] == serde_json::json!(ids[1]) {
            b["magazine"]["loaded"] = 1.into();
        }
    }
    *game.state = serde_json::from_value(json).unwrap();
    assert_eq!(salvo_readout(&game.view(), &input), Some((1, 2)));
}

#[test]
fn the_banner_steppers_size_the_salvo_and_the_rest_of_it_stands_down() {
    let (mut game, _) = spires(3, 3, 0);
    let mut input = InputState::new();
    dispatch_action(&mut game, &mut input, Action::Launch);
    relayout(&game, &input);
    let layout = game.presentation.layout.get();
    let (less, more, ribbon) = (layout.salvo_less, layout.salvo_more, layout.mode_ribbon);
    for stepper in [less, more] {
        assert!(
            stepper.w > 0.0 && ribbon.contains(stepper.center()),
            "{stepper:?} in {ribbon:?}"
        );
    }
    assert!(less.right() <= more.x, "minus sits left of plus");
    let press = |game: &mut Game, input: &mut InputState, rect: Rect| {
        apply_events(game, input, &click(rect.center().x, rect.center().y));
        relayout(game, input);
    };
    press(&mut game, &mut input, less);
    assert_eq!(input.salvo, 1, "the size never drops below one");
    assert!(
        input.striking.is_some(),
        "a stepper never cancels the strike"
    );
    for _ in 0..4 {
        press(&mut game, &mut input, more);
    }
    assert_eq!(input.salvo, 3, "nor grows past the ready");
    assert_eq!(
        game.presentation.layout.get().salvo_more,
        more,
        "the steppers hold still"
    );
    press(&mut game, &mut input, less);
    assert_eq!(salvo_readout(&game.view(), &input), Some((2, 3)));
    assert!(launches(&game).is_empty());
    press(
        &mut game,
        &mut input,
        Rect::new(ribbon.x + 2.0, ribbon.y + 2.0, 4.0, 4.0),
    );
    assert!(input.striking.is_none(), "the rest of the ribbon cancels");
}

#[test]
fn a_touch_check_mark_fires_the_salvo_and_keeps_its_aim() {
    let (mut game, _) = spires(3, 3, 0);
    let mut input = InputState::new();
    input.last_pointer = Pointer::Touch;
    input.salvo = 2;
    dispatch_action(&mut game, &mut input, Action::Launch);
    tap_world(&mut game, &mut input, vec2(20.5, 6.5));
    let controls = lay_out_ghost_controls(&game, &input);
    tap(&mut game, &mut input, controls.confirm.center());
    assert_eq!(launches(&game).len(), 2);
    assert_eq!(
        input.striking.as_ref().and_then(Strike::aim),
        Some(TilePos::new(20, 6)),
        "the aim waits for the next salvo"
    );
    tap(&mut game, &mut input, controls.confirm.center());
    assert_eq!(launches(&game).len(), 3);
    assert!(input.striking.is_none());
}

/// Publishes the selection's command cards in one row, as drawing does,
/// and returns the Order card's rect.
fn publish_order(game: &mut Game, input: &InputState) -> Rect {
    let panel = crate::panel::build_for_input(&game.view(), &classic(), input).expect("a panel");
    let mut layout = bare_layout(680.0, 1200.0);
    let mut order = None;
    for (slot, card) in panel.cards.iter().enumerate() {
        let rect = Rect::new(240.0 + 90.0 * slot as f32, 700.0, 84.0, 72.0);
        layout.cards[slot] = (rect, card.action);
        if card.action == crate::panel::CardAction::Dispatch(Action::OrderRound) {
            order = Some(rect);
        }
    }
    layout.card_count = panel.cards.len();
    game.presentation.layout.set(layout);
    *game.presentation.panel_model.borrow_mut() = Some(panel);
    order.expect("a Spire's panel offers Order")
}

#[test]
fn shift_turns_order_into_fill_and_a_shift_click_fills_every_empty_spire() {
    let cost = aftermine_sim::BuildingKind::Spire
        .magazine()
        .unwrap()
        .round_cost;
    let (mut game, ids) = spires(4, 1, 10 * cost);
    let mut input = InputState::new();
    let at = publish_order(&mut game, &input).center();
    apply_events(&mut game, &mut input, &[key_down(Key::Shift)]);
    let panel = crate::panel::build_for_input(&game.view(), &classic(), &input).unwrap();
    let fill = panel
        .cards
        .iter()
        .find(|card| card.action == crate::panel::CardAction::Dispatch(Action::OrderRound))
        .unwrap();
    assert_eq!((fill.title.as_str(), fill.cost), ("Fill 3", Some(3 * cost)));
    apply_events(&mut game, &mut input, &click(at.x, at.y));
    apply_events(&mut game, &mut input, &[key_up(Key::Shift)]);
    assert_eq!(orders(&game), ids[1..].to_vec());
    assert_eq!(last_toast(&game), None, "a full order needs no toast");
    // Released, the card is a single Order again.
    let panel = crate::panel::build_for_input(&game.view(), &classic(), &input).unwrap();
    assert!(panel.cards.iter().any(|card| card.title == "Order"));
}

#[test]
fn a_right_click_or_a_long_press_on_order_fills_and_a_press_orders_one() {
    let cost = aftermine_sim::BuildingKind::Spire
        .magazine()
        .unwrap()
        .round_cost;
    let (mut game, ids) = spires(3, 0, 10 * cost);
    let mut input = InputState::new();
    let at = publish_order(&mut game, &input).center();
    apply_events(&mut game, &mut input, &[right_down(at)]);
    assert_eq!(orders(&game), ids);

    let (mut game, ids) = spires(3, 0, 10 * cost);
    let mut input = InputState::new();
    let at = publish_order(&mut game, &input).center();
    input.now = 5.0;
    apply_events(&mut game, &mut input, &[touch_down(1, at)]);
    input.now = 5.25;
    assert!(card_press_progress(&input).is_some(), "the card charges");
    input.now = 6.0;
    update_touch(&mut game, &mut input);
    assert_eq!(orders(&game), ids);
    apply_events(&mut game, &mut input, &[touch_up(1, at)]);
    assert_eq!(
        orders(&game).len(),
        3,
        "the lift after the hold orders nothing more"
    );

    let (mut game, ids) = spires(3, 0, 10 * cost);
    let mut input = InputState::new();
    let at = publish_order(&mut game, &input).center();
    apply_events(&mut game, &mut input, &click(at.x, at.y));
    assert_eq!(orders(&game), vec![ids[0]]);
}
