//! Basic Training played the way each card words it, through the real
//! input funnel and a headless HUD: with mouse and keys, and with a finger
//! alone. A lesson whose copy names a gesture, card, or key the shell
//! does not answer fails here.

use super::*;
use crate::panel::CardAction;
use crate::render::hud::{self, HudEnv};
use crate::tutorial::{Lesson, Tutorial};
use oxide_sim::BuildingKind;

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

/// Events through the funnel, the HUD laid out again between them as a
/// frame does.
fn send(game: &mut Game, input: &mut InputState, events: &[RawEvent]) {
    crate::input::apply_events(game, input, &classic(), events, relayout);
    relayout(game, input);
    Tutorial::advance_in(game);
}

fn begin(lesson: Lesson) -> (Game, InputState) {
    let game = Tutorial::begin_at(lesson, vec2(1280.0, 800.0)).expect("lesson scenes build");
    let input = InputState::new();
    relayout(&game, &input);
    (game, input)
}

/// The objective the card shows.
fn objective(game: &Game) -> Option<&'static str> {
    game.tutorial
        .as_ref()
        .expect("a lesson runs")
        .current()
        .map(|step| step.title)
}

/// The paragraphs the card shows for `hands`, keys named.
fn copy(game: &Game, pointer: Pointer, keys: bool) -> String {
    let step = game
        .tutorial
        .as_ref()
        .and_then(Tutorial::current)
        .expect("a step is up");
    step.body(crate::platform::Hands { pointer, keys })
        .iter()
        .map(|text| crate::render::tutorial_card::expand(text, &classic()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Plays on until `done`, keeping the lesson's books as a frame does.
fn run_until(game: &mut Game, input: &InputState, what: &str, done: impl Fn(&Game) -> bool) {
    for _ in 0..20 * 120 {
        if done(game) {
            return;
        }
        game.do_tick();
        relayout(game, input);
        Tutorial::advance_in(game);
    }
    panic!("{what} never happened");
}

fn run_to(game: &mut Game, input: &InputState, title: Option<&str>) {
    run_until(game, input, title.unwrap_or("the finish"), |game| {
        objective(game) == title
    });
}

/// The human's machines of `kind`, with where each stands.
fn own(game: &Game, kind: UnitKind) -> Vec<(oxide_sim::UnitId, Vec2)> {
    game.state
        .units()
        .iter()
        .filter(|u| u.player == game.presentation.human && u.kind == kind)
        .map(|u| (u.id, vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>())))
        .collect()
}

fn on_screen(game: &Game, world: Vec2) -> Vec2 {
    game.presentation.camera.to_screen(world)
}

fn foundry(game: &Game) -> Vec2 {
    let c = game.home_foundry().expect("the lesson's Foundry").center();
    vec2(c.x.to_num::<f32>(), c.y.to_num::<f32>())
}

fn fabricator(game: &Game) -> Vec2 {
    let fabricator = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player != game.presentation.human)
        .expect("the enemy Fabricator stands")
        .center();
    vec2(fabricator.x.to_num::<f32>(), fabricator.y.to_num::<f32>())
}

/// Where the laid-out panel card answering `action` sits.
fn card(game: &Game, action: CardAction) -> Vec2 {
    let layout = game.presentation.layout.get();
    layout.cards[..layout.card_count]
        .iter()
        .find(|(_, at)| *at == action)
        .map_or_else(
            || {
                panic!(
                    "no {action:?} card among {:?}",
                    &layout.cards[..layout.card_count]
                )
            },
            |(rect, _)| rect.center(),
        )
}

fn click_at(game: &mut Game, input: &mut InputState, p: Vec2) {
    send(game, input, &click(p.x, p.y));
}

fn right_click_at(game: &mut Game, input: &mut InputState, world: Vec2) {
    let p = on_screen(game, world);
    send(game, input, &[right_down(p)]);
}

fn press(game: &mut Game, input: &mut InputState, key: Key) {
    send(game, input, &[key_down(key), key_up(key)]);
}

fn tap_at(game: &mut Game, input: &mut InputState, p: Vec2) {
    input.now += 1.0;
    send(game, input, &[touch_down(1, p)]);
    input.now += 0.05;
    send(game, input, &[touch_up(1, p)]);
}

fn long_press_at(game: &mut Game, input: &mut InputState, world: Vec2) {
    long_press_world(game, input, world);
    relayout(game, input);
    Tutorial::advance_in(game);
}

/// The scrap pile beside the Foundry.
const HOME_PILE: Vec2 = Vec2::new(13.5, 5.5);
/// The frame beside the Foundry and the one the Surveyor found.
const NEAR_FRAME: Vec2 = Vec2::new(13.5, 13.5);
const FAR_FRAME: Vec2 = Vec2::new(31.5, 9.5);

fn hauling(game: &Game) -> usize {
    game.state
        .units()
        .iter()
        .filter(|u| {
            u.player == game.presentation.human
                && matches!(u.order, oxide_sim::Order::Harvest { .. })
        })
        .count()
}

fn extractors(game: &Game) -> usize {
    game.state
        .buildings()
        .iter()
        .filter(|b| b.player == game.presentation.human && b.kind == BuildingKind::Extractor)
        .count()
}

#[test]
fn the_command_lesson_plays_as_its_mouse_and_keys_card_says() {
    let (mut game, mut input) = begin(Lesson::Command);

    assert_eq!(objective(&game), Some("Move the camera"));
    assert!(copy(&game, Pointer::Mouse, true).contains("W A S D"));
    send(&mut game, &mut input, &[key_down(Key::D)]);
    update_held(&mut game, &input, 0.5);
    send(&mut game, &mut input, &[key_up(Key::D)]);

    assert_eq!(objective(&game), Some("Select a Sentinel"));
    let squad = own(&game, UnitKind::Sentinel);
    let first = on_screen(&game, squad[0].1);
    click_at(&mut game, &mut input, first);

    assert_eq!(objective(&game), Some("Select the squad"));
    assert!(copy(&game, Pointer::Mouse, true).contains("Drag a box"));
    let corner = |game: &Game, f: fn(f32, f32) -> f32, pad: f32| {
        let (x, y) = squad
            .iter()
            .fold((squad[0].1.x, squad[0].1.y), |(x, y), (_, at)| {
                (f(x, at.x), f(y, at.y))
            });
        on_screen(game, vec2(x + pad, y + pad))
    };
    let (a, b) = (corner(&game, f32::min, -1.0), corner(&game, f32::max, 1.0));
    send(
        &mut game,
        &mut input,
        &[left_down(a), mouse_move(b), left_up(b)],
    );

    assert_eq!(objective(&game), Some("Find the enemy"));
    right_click_at(&mut game, &mut input, vec2(39.5, 12.5));
    assert!(
        game.pending
            .iter()
            .any(|c| matches!(c.command, Command::Advance { .. })),
        "a right-click on ground moves the squad"
    );
    run_to(&mut game, &input, Some("Destroy the Fabricator"));

    // The squad walked east; the camera follows it there with the keys.
    while on_screen(&game, fabricator(&game)).x > 1100.0 {
        send(&mut game, &mut input, &[key_down(Key::D)]);
        update_held(&mut game, &input, 0.1);
        send(&mut game, &mut input, &[key_up(Key::D)]);
    }
    let target = fabricator(&game);
    right_click_at(&mut game, &mut input, target);
    assert!(
        game.pending
            .iter()
            .any(|c| matches!(c.command, Command::Attack { .. })),
        "a right-click on the Fabricator attacks it"
    );
    run_to(&mut game, &input, None);
}

#[test]
fn the_command_lesson_plays_as_its_touch_card_says() {
    let (mut game, mut input) = begin(Lesson::Command);

    assert_eq!(objective(&game), Some("Move the camera"));
    assert!(copy(&game, Pointer::Touch, false).contains("Drag one finger"));
    let from = vec2(900.0, 400.0);
    send(&mut game, &mut input, &[touch_down(1, from)]);
    send(
        &mut game,
        &mut input,
        &[touch_move(1, from - vec2(240.0, 0.0))],
    );
    send(
        &mut game,
        &mut input,
        &[touch_up(1, from - vec2(240.0, 0.0))],
    );

    assert_eq!(objective(&game), Some("Select a Sentinel"));
    let squad = own(&game, UnitKind::Sentinel);
    let first = on_screen(&game, squad[0].1);
    tap_at(&mut game, &mut input, first);

    assert_eq!(objective(&game), Some("Select the squad"));
    assert!(copy(&game, Pointer::Touch, false).contains("Hold two fingers"));
    let a = on_screen(&game, squad[0].1 - vec2(1.5, 1.5));
    let b = on_screen(&game, squad[3].1 + vec2(1.5, 1.5));
    input.now += 1.0;
    send(&mut game, &mut input, &[touch_down(1, a), touch_down(2, b)]);
    input.now += (touch::BOX_REST_MS + 10.0) / 1000.0;
    update_touch(&mut game, &mut input);
    send(&mut game, &mut input, &[touch_up(2, b)]);
    send(&mut game, &mut input, &[touch_up(1, a)]);

    assert_eq!(objective(&game), Some("Find the enemy"));
    assert!(copy(&game, Pointer::Touch, false).contains("Long-press open ground"));
    long_press_at(&mut game, &mut input, vec2(39.5, 12.5));
    run_to(&mut game, &input, Some("Destroy the Fabricator"));

    while on_screen(&game, fabricator(&game)).x > 1100.0 {
        input.now += 1.0;
        send(&mut game, &mut input, &[touch_down(3, from)]);
        send(
            &mut game,
            &mut input,
            &[touch_move(3, from - vec2(120.0, 0.0))],
        );
        send(
            &mut game,
            &mut input,
            &[touch_up(3, from - vec2(120.0, 0.0))],
        );
    }
    let target = fabricator(&game);
    long_press_at(&mut game, &mut input, target);
    run_to(&mut game, &input, None);
}

#[test]
fn the_scrap_lesson_plays_as_its_mouse_and_keys_card_says() {
    let (mut game, mut input) = begin(Lesson::Scrap);

    assert_eq!(objective(&game), Some("Collect scrap"));
    let hauler = on_screen(&game, own(&game, UnitKind::Harvester)[0].1);
    click_at(&mut game, &mut input, hauler);
    right_click_at(&mut game, &mut input, HOME_PILE);
    run_to(&mut game, &input, Some("Train a Harvester"));

    assert!(copy(&game, Pointer::Mouse, true).contains("press Q"));
    let home = on_screen(&game, foundry(&game));
    click_at(&mut game, &mut input, home);
    press(&mut game, &mut input, Key::Q);
    run_to(&mut game, &input, Some("Put every Harvester to work"));

    assert!(copy(&game, Pointer::Mouse, true).contains("press N"));
    while hauling(&game) < 3 {
        press(&mut game, &mut input, Key::N);
        assert_eq!(game.presentation.selection.units.len(), 1, "N grabs one");
        right_click_at(&mut game, &mut input, HOME_PILE);
        game.do_tick();
    }
    run_to(&mut game, &input, Some("Restore an Extractor"));

    assert!(copy(&game, Pointer::Mouse, true).contains("press B"));
    let builder = on_screen(&game, own(&game, UnitKind::Harvester)[0].1);
    click_at(&mut game, &mut input, builder);
    press(&mut game, &mut input, Key::B);
    let extractor = card(&game, CardAction::ArmBuild(BuildingKind::Extractor));
    click_at(&mut game, &mut input, extractor);
    let frame = on_screen(&game, NEAR_FRAME);
    click_at(&mut game, &mut input, frame);
    assert_eq!(
        extractors(&game) + game.pending.len(),
        1,
        "the frame took the site"
    );
    run_to(&mut game, &input, Some("Restore the far frame"));

    run_until(
        &mut game,
        &input,
        "the bank covering a second Extractor",
        |game| game.state.player(game.presentation.human).scrap >= 100,
    );
    let builder = on_screen(&game, own(&game, UnitKind::Harvester)[0].1);
    click_at(&mut game, &mut input, builder);
    // Construction stays open after a placement.
    if !input.construction_open() {
        let build = card(
            &game,
            CardAction::Dispatch(crate::action::Action::ToggleBuildPalette),
        );
        click_at(&mut game, &mut input, build);
    }
    let extractor = card(&game, CardAction::ArmBuild(BuildingKind::Extractor));
    click_at(&mut game, &mut input, extractor);
    let frame = on_screen(&game, FAR_FRAME);
    click_at(&mut game, &mut input, frame);
    run_to(&mut game, &input, None);
}

#[test]
fn the_scrap_lesson_plays_as_its_touch_card_says() {
    let (mut game, mut input) = begin(Lesson::Scrap);

    assert_eq!(objective(&game), Some("Collect scrap"));
    let hauler = on_screen(&game, own(&game, UnitKind::Harvester)[0].1);
    tap_at(&mut game, &mut input, hauler);
    long_press_at(&mut game, &mut input, HOME_PILE);
    run_to(&mut game, &input, Some("Train a Harvester"));

    assert!(copy(&game, Pointer::Touch, false).contains("tap Harvester in the bottom panel"));
    let home = on_screen(&game, foundry(&game));
    tap_at(&mut game, &mut input, home);
    let harvester = card(
        &game,
        CardAction::Dispatch(crate::action::Action::TrainSlot(0)),
    );
    tap_at(&mut game, &mut input, harvester);
    run_to(&mut game, &input, Some("Put every Harvester to work"));

    assert!(copy(&game, Pointer::Touch, false).contains("Tap the badge"));
    while hauling(&game) < 3 {
        let badge = game.presentation.layout.get().idle_badge;
        assert!(
            badge.w > 0.0,
            "the idle badge shows while a Harvester idles"
        );
        tap_at(&mut game, &mut input, badge.center());
        assert_eq!(
            game.presentation.selection.units.len(),
            1,
            "the badge grabs one"
        );
        long_press_at(&mut game, &mut input, HOME_PILE);
        game.do_tick();
        relayout(&game, &input);
    }
    run_to(&mut game, &input, Some("Restore an Extractor"));

    for (frame, next) in [
        (NEAR_FRAME, Some("Restore the far frame")),
        (FAR_FRAME, None),
    ] {
        run_until(
            &mut game,
            &input,
            "the bank covering an Extractor",
            |game| game.state.player(game.presentation.human).scrap >= 100,
        );
        let builder = on_screen(&game, own(&game, UnitKind::Harvester)[0].1);
        tap_at(&mut game, &mut input, builder);
        // Construction stays open after a placement.
        if !input.construction_open() {
            let build = card(
                &game,
                CardAction::Dispatch(crate::action::Action::ToggleBuildPalette),
            );
            tap_at(&mut game, &mut input, build);
        }
        let extractor = card(&game, CardAction::ArmBuild(BuildingKind::Extractor));
        tap_at(&mut game, &mut input, extractor);
        let ground = on_screen(&game, frame);
        tap_at(&mut game, &mut input, ground);
        assert!(game.pending.is_empty(), "moving the ghost builds nothing");
        tap_check(&mut game, &mut input);
        assert!(
            game.pending.iter().any(|c| matches!(
                c.command,
                Command::Build {
                    kind: BuildingKind::Extractor,
                    ..
                }
            )),
            "the check mark places the Extractor: {:?}",
            game.pending
        );
        run_to(&mut game, &input, next);
    }
}

/// Where the Build lesson's Fabricator goes: the middle of open ground
/// south of the Foundry. The row of Barricades runs west to east north of
/// it, clear of the card and the panel.
const FABRICATOR_SPOT: Vec2 = Vec2::new(6.0, 14.0);
const BARRICADES: [Vec2; 3] = [
    Vec2::new(9.5, 8.5),
    Vec2::new(10.5, 8.5),
    Vec2::new(11.5, 8.5),
];
const RALLY_SPOT: Vec2 = Vec2::new(13.5, 9.5);

fn own_building(game: &Game, kind: BuildingKind) -> Vec2 {
    let c = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player == game.presentation.human && b.kind == kind)
        .expect("the building stands")
        .center();
    vec2(c.x.to_num::<f32>(), c.y.to_num::<f32>())
}

fn placed(game: &Game, kind: BuildingKind) -> usize {
    game.pending
        .iter()
        .filter(|c| matches!(c.command, Command::Build { kind: k, .. } if k == kind))
        .count()
}

#[test]
fn the_build_lesson_plays_as_its_mouse_and_keys_card_says() {
    let (mut game, mut input) = begin(Lesson::Build);

    assert_eq!(objective(&game), Some("Build a Fabricator"));
    assert!(copy(&game, Pointer::Mouse, true).contains("press B"));
    let builder = on_screen(&game, own(&game, UnitKind::Harvester)[0].1);
    click_at(&mut game, &mut input, builder);
    press(&mut game, &mut input, Key::B);
    let fabricator = card(&game, CardAction::ArmBuild(BuildingKind::Fabricator));
    click_at(&mut game, &mut input, fabricator);
    let spot = on_screen(&game, FABRICATOR_SPOT);
    click_at(&mut game, &mut input, spot);
    assert_eq!(
        placed(&game, BuildingKind::Fabricator),
        1,
        "the click places it"
    );
    run_to(&mut game, &input, Some("Build three Barricades"));

    assert!(copy(&game, Pointer::Mouse, true).contains("hold Shift"));
    let builder = on_screen(&game, own(&game, UnitKind::Harvester)[0].1);
    click_at(&mut game, &mut input, builder);
    if !input.construction_open() {
        press(&mut game, &mut input, Key::B);
    }
    let barricade = card(&game, CardAction::ArmBuild(BuildingKind::Barricade));
    click_at(&mut game, &mut input, barricade);
    send(&mut game, &mut input, &[key_down(Key::Shift)]);
    for spot in BARRICADES {
        let p = on_screen(&game, spot);
        click_at(&mut game, &mut input, p);
    }
    send(&mut game, &mut input, &[key_up(Key::Shift)]);
    assert_eq!(
        placed(&game, BuildingKind::Barricade),
        3,
        "Shift chains them"
    );
    run_to(&mut game, &input, Some("Set a rally point"));

    assert!(copy(&game, Pointer::Mouse, true).contains("press Y"));
    let fabricator = on_screen(&game, own_building(&game, BuildingKind::Fabricator));
    click_at(&mut game, &mut input, fabricator);
    right_click_at(&mut game, &mut input, RALLY_SPOT);
    assert!(
        game.pending
            .iter()
            .any(|c| matches!(c.command, Command::SetRally { rally: Some(_), .. })),
        "a right-click on ground with the Fabricator selected rallies it"
    );
    run_to(&mut game, &input, Some("Train a Lancer"));

    assert!(copy(&game, Pointer::Mouse, true).contains("press Q"));
    press(&mut game, &mut input, Key::Q);
    run_to(&mut game, &input, Some("Make a control group"));

    assert!(copy(&game, Pointer::Mouse, true).contains("press Ctrl+1"));
    let lancer = on_screen(&game, own(&game, UnitKind::Lancer)[0].1);
    click_at(&mut game, &mut input, lancer);
    send(
        &mut game,
        &mut input,
        &[
            key_down(Key::Ctrl),
            key_down(Key::Num1),
            key_up(Key::Num1),
            key_up(Key::Ctrl),
        ],
    );
    assert_eq!(
        objective(&game),
        None,
        "saving the group finishes the lesson"
    );
    game.presentation.selection.units.clear();
    press(&mut game, &mut input, Key::Num1);
    assert_eq!(
        game.presentation.selection.units,
        [own(&game, UnitKind::Lancer)[0].0],
        "1 recalls the group, as the card says"
    );
}

#[test]
fn the_build_lesson_plays_as_its_touch_card_says() {
    let (mut game, mut input) = begin(Lesson::Build);

    assert_eq!(objective(&game), Some("Build a Fabricator"));
    assert!(copy(&game, Pointer::Touch, false).contains("tap the check mark"));
    let builder = on_screen(&game, own(&game, UnitKind::Harvester)[0].1);
    tap_at(&mut game, &mut input, builder);
    let build = card(
        &game,
        CardAction::Dispatch(crate::action::Action::ToggleBuildPalette),
    );
    tap_at(&mut game, &mut input, build);
    let fabricator = card(&game, CardAction::ArmBuild(BuildingKind::Fabricator));
    tap_at(&mut game, &mut input, fabricator);
    let spot = on_screen(&game, FABRICATOR_SPOT);
    tap_at(&mut game, &mut input, spot);
    tap_check(&mut game, &mut input);
    assert_eq!(
        placed(&game, BuildingKind::Fabricator),
        1,
        "the check mark places it"
    );
    run_to(&mut game, &input, Some("Build three Barricades"));

    assert!(copy(&game, Pointer::Touch, false).contains("Drag the handle"));
    let builder = on_screen(&game, own(&game, UnitKind::Harvester)[0].1);
    tap_at(&mut game, &mut input, builder);
    if !input.construction_open() {
        let build = card(
            &game,
            CardAction::Dispatch(crate::action::Action::ToggleBuildPalette),
        );
        tap_at(&mut game, &mut input, build);
    }
    let barricade = card(&game, CardAction::ArmBuild(BuildingKind::Barricade));
    tap_at(&mut game, &mut input, barricade);
    let first = on_screen(&game, BARRICADES[0]);
    tap_at(&mut game, &mut input, first);
    // The row ends on the tile under the last Barricade.
    super::touch_placement::pull_handle_to(
        &mut game,
        &mut input,
        chassis::grid::TilePos::new(11, 8),
    );
    tap_check(&mut game, &mut input);
    assert_eq!(
        placed(&game, BuildingKind::Barricade),
        3,
        "the handle lays a row"
    );
    run_to(&mut game, &input, Some("Set a rally point"));

    assert!(copy(&game, Pointer::Touch, false).contains("long-press the ground"));
    let fabricator = on_screen(&game, own_building(&game, BuildingKind::Fabricator));
    tap_at(&mut game, &mut input, fabricator);
    long_press_at(&mut game, &mut input, RALLY_SPOT);
    run_to(&mut game, &input, Some("Train a Lancer"));

    let lancer = card(
        &game,
        CardAction::Dispatch(crate::action::Action::TrainSlot(0)),
    );
    tap_at(&mut game, &mut input, lancer);
    run_to(&mut game, &input, Some("Make a control group"));

    assert!(copy(&game, Pointer::Touch, false).contains("tap the plus"));
    let lancer = on_screen(&game, own(&game, UnitKind::Lancer)[0].1);
    tap_at(&mut game, &mut input, lancer);
    let plus = game
        .presentation
        .layout
        .get()
        .group_slots
        .iter()
        .flatten()
        .find(|(_, slot)| matches!(slot, crate::layout::GroupSlot::Assign(_)))
        .map(|(rect, _)| rect.center())
        .expect("the bar offers to save the selection");
    tap_at(&mut game, &mut input, plus);
    assert_eq!(
        objective(&game),
        None,
        "saving the group finishes the lesson"
    );
}
