//! Building selections: the kind strip trims a mixed selection, the
//! select-all button gathers every one of a kind, units included, and
//! control groups save and recall buildings as they do units.

use super::*;
use crate::panel::CardAction;
use aftermine_sim::{BuildingKind, Command};
use macroquad::math::Rect;

/// The fixture's own buildings all selected: `turrets` Turrets, a
/// Fabricator, a Crucible, and the starting Foundry.
fn base(turrets: usize) -> Game {
    let mut game =
        crate::building_actions::tests::fixture(BuildingKind::Turret, &vec![0; turrets], 0);
    game.presentation.selection.buildings = own_buildings(&game, None);
    game
}

fn own_buildings(game: &Game, kind: Option<BuildingKind>) -> Vec<aftermine_sim::BuildingId> {
    game.state
        .buildings()
        .iter()
        .filter(|b| b.player == game.presentation.human && kind.is_none_or(|kind| b.kind == kind))
        .map(|b| b.id)
        .collect()
}

/// Every own building but those of `kind`.
fn others(game: &Game, kind: BuildingKind) -> Vec<aftermine_sim::BuildingId> {
    own_buildings(game, None)
        .into_iter()
        .filter(|id| game.state.building(*id).is_some_and(|b| b.kind != kind))
        .collect()
}

/// Publishes the selection's strip chips in one row, as drawing does,
/// and returns their rects.
fn publish_strip(game: &mut Game, input: &InputState) -> Vec<(Rect, CardAction)> {
    let panel = crate::panel::build_for_input(&game.view(), &classic(), input).expect("a panel");
    let mut layout = bare_layout(680.0, 1200.0);
    let chips: Vec<_> = panel
        .roster
        .iter()
        .enumerate()
        .map(|(slot, card)| {
            (
                Rect::new(20.0 + 68.0 * slot as f32, 600.0, 64.0, 64.0),
                card.action,
            )
        })
        .collect();
    for (slot, chip) in chips.iter().enumerate() {
        layout.roster_slots[slot] = *chip;
    }
    layout.roster_count = chips.len();
    game.presentation.layout.set(layout);
    *game.presentation.panel_model.borrow_mut() = Some(panel);
    chips
}

fn chip(chips: &[(Rect, CardAction)], action: CardAction) -> Vec2 {
    chips
        .iter()
        .find(|(_, a)| *a == action)
        .map(|(rect, _)| rect.center())
        .expect("the chip is offered")
}

#[test]
fn a_kind_chip_keeps_its_kind_and_shift_or_ctrl_drops_it() {
    let mut game = base(2);
    let mut input = InputState::new();
    let turrets = CardAction::FilterBuilding(BuildingKind::Turret);
    let at = chip(&publish_strip(&mut game, &input), turrets);
    apply_events(&mut game, &mut input, &click(at.x, at.y));
    assert_eq!(
        game.presentation.selection.buildings,
        own_buildings(&game, Some(BuildingKind::Turret))
    );

    for modifier in [Key::Shift, Key::Ctrl] {
        game.presentation.selection.buildings = own_buildings(&game, None);
        let at = chip(&publish_strip(&mut game, &input), turrets);
        apply_events(&mut game, &mut input, &[key_down(modifier)]);
        apply_events(&mut game, &mut input, &click(at.x, at.y));
        apply_events(&mut game, &mut input, &[key_up(modifier)]);
        assert_eq!(
            game.presentation.selection.buildings,
            others(&game, BuildingKind::Turret),
            "{modifier:?} drops the Turrets"
        );
    }
    assert!(game.pending.is_empty(), "trimming issues no command");
}

#[test]
fn a_tap_with_queue_lit_drops_a_kind() {
    let mut game = base(2);
    let mut input = InputState::new();
    let at = chip(
        &publish_strip(&mut game, &input),
        CardAction::FilterBuilding(BuildingKind::Crucible),
    );
    input.toggle_queue();
    tap(&mut game, &mut input, at);
    assert_eq!(
        game.presentation.selection.buildings,
        others(&game, BuildingKind::Crucible)
    );
}

/// Publishes the selection's select-all button where drawing might put
/// it and returns its centre.
fn publish_select_all(game: &mut Game, input: &InputState) -> Vec2 {
    let panel = crate::panel::build_for_input(&game.view(), &classic(), input).expect("a panel");
    let all = panel.select_all.as_ref().expect("the button is offered");
    let rect = Rect::new(400.0, 620.0, 52.0, 26.0);
    let mut layout = bare_layout(600.0, 1200.0);
    layout.select_all = (rect, all.card.action);
    game.presentation.layout.set(layout);
    *game.presentation.panel_model.borrow_mut() = Some(panel);
    rect.center()
}

#[test]
fn the_all_button_selects_every_one_you_own_by_click_or_tap() {
    let mut game = base(3);
    let turrets = own_buildings(&game, Some(BuildingKind::Turret));
    for touch in [false, true] {
        let mut input = InputState::new();
        game.presentation.selection.buildings = vec![turrets[2]];
        let at = publish_select_all(&mut game, &input);
        if touch {
            tap(&mut game, &mut input, at);
        } else {
            apply_events(&mut game, &mut input, &click(at.x, at.y));
        }
        assert_eq!(
            game.presentation.selection.buildings, turrets,
            "touch {touch}"
        );
        let panel = crate::panel::build_for_input(&game.view(), &classic(), &input).unwrap();
        assert!(panel.select_all.is_none(), "nothing more to gather");
    }
    assert!(game.pending.is_empty(), "gathering issues no command");
}

#[test]
fn the_all_button_gathers_units_but_not_the_ones_riding_a_transport() {
    let scenario = aftermine_sim::Scenario::from_json(
        r#"{
        "name":"Select all riders",
        "players":[{"name":"F","scrap":0,"bot":false},
                   {"name":"C","scrap":0,"bot":true}],
        "map":["....................", "..1..............2..", "....................",
               "....................", "....................", "...................."],
        "units":[
          {"player":0,"kind":"sentinel","x":6,"y":4},
          {"player":0,"kind":"sentinel","x":7,"y":4},
          {"player":0,"kind":"sentinel","x":8,"y":4},
          {"player":0,"kind":"skyhook","x":10,"y":4},
          {"player":1,"kind":"sentinel","x":16,"y":4}]
    }"#,
    )
    .expect("inline rider scenario parses");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("rider game builds");
    let human = game.presentation.human;
    let own = |game: &Game, kind| -> Vec<aftermine_sim::UnitId> {
        game.state
            .units()
            .iter()
            .filter(|u| u.player == human && u.kind == kind)
            .map(|u| u.id)
            .collect()
    };
    let sentinels = own(&game, UnitKind::Sentinel);
    let skyhook = own(&game, UnitKind::Skyhook)[0];
    game.issue(Command::Load {
        units: vec![sentinels[2]],
        transport: skyhook,
        queue: false,
    });
    for _ in 0..180 {
        game.do_tick();
    }
    assert!(
        game.state.unit(sentinels[2]).is_none()
            && game.state.unit(skyhook).unwrap().cargo.len() == 1,
        "premise: one Sentinel rides as cargo"
    );
    let mut input = InputState::new();
    game.presentation.selection.units = vec![sentinels[1]];
    let at = publish_select_all(&mut game, &input);
    let title = game
        .presentation
        .panel_model
        .borrow()
        .as_ref()
        .unwrap()
        .select_all
        .as_ref()
        .unwrap()
        .card
        .title
        .clone();
    assert_eq!(
        title, "Select all 2 Sentinels",
        "the rider and the enemy's do not count"
    );
    apply_events(&mut game, &mut input, &click(at.x, at.y));
    assert_eq!(game.presentation.selection.units, sentinels[..2]);
    assert!(game.pending.is_empty(), "gathering issues no command");
}

#[test]
fn a_group_saves_and_recalls_buildings_and_a_second_press_centers_them() {
    let mut game = base(2);
    let mut input = InputState::new();
    let turrets = own_buildings(&game, Some(BuildingKind::Turret));
    game.presentation.selection.buildings.clone_from(&turrets);
    assert_eq!(input.group_on_offer(&game.view()), Some(1));
    apply_events(
        &mut game,
        &mut input,
        &[
            key_down(Key::Ctrl),
            key_down(Key::Num1),
            key_up(Key::Num1),
            key_up(Key::Ctrl),
        ],
    );
    assert_eq!(input.groups[0], Group::Buildings(turrets.clone()));
    assert_eq!(input.selected_group(&game.view()), Some(1));
    assert_eq!(input.group_counts(&game.view()), [2, 0, 0, 0, 0]);
    assert_eq!(
        input.group_leads(&game.view())[0],
        Some(crate::panel::CardIcon::Building(BuildingKind::Turret, 0))
    );

    game.presentation.selection.buildings = own_buildings(&game, Some(BuildingKind::Crucible));
    let far = vec2(30.0, 2.0);
    game.presentation.camera.center = far;
    input.now = 10.0;
    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Num1), key_up(Key::Num1)],
    );
    assert_eq!(game.presentation.selection.buildings, turrets);
    assert!(game.presentation.selection.units.is_empty());
    assert_eq!(
        game.presentation.camera.center, far,
        "one recall leaves the camera"
    );
    input.now = 10.2;
    apply_events(
        &mut game,
        &mut input,
        &[key_down(Key::Num1), key_up(Key::Num1)],
    );
    assert_ne!(
        game.presentation.camera.center, far,
        "a quick second recall centers"
    );
    assert!(game.pending.is_empty());
}

#[test]
fn a_group_holds_units_or_buildings_and_drops_what_is_not_yours() {
    let mut game = base(1);
    let mut input = InputState::new();
    let enemy = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player != game.presentation.human)
        .expect("the target's Foundry")
        .id;
    game.presentation.selection.buildings = vec![enemy];
    dispatch_action(&mut game, &mut input, Action::AssignGroup(2));
    assert!(input.groups[1].is_empty());
    assert_eq!(
        game.presentation.toasts.last().map(|t| t.text.as_str()),
        Some("Only your own buildings join a control group")
    );

    // A building group replaced by units holds only the units.
    let turret = own_buildings(&game, Some(BuildingKind::Turret));
    game.presentation.selection.buildings.clone_from(&turret);
    dispatch_action(&mut game, &mut input, Action::AssignGroup(3));
    assert_eq!(input.groups[2], Group::Buildings(turret));
    game.presentation.selection.buildings.clear();
    dispatch_action(&mut game, &mut input, Action::AssignGroup(3));
    assert!(
        input.groups[2].is_empty(),
        "an empty selection clears the group"
    );
}
