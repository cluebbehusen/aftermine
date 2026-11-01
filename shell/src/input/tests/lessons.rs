//! Basic Training played the way each card words it, through the real
//! input funnel and a headless HUD: with mouse and keys, and with a finger
//! alone. A lesson whose copy names a gesture, card, or key the shell
//! does not answer fails here.

use super::*;
use crate::panel::CardAction;
use crate::render::hud::{self, HudEnv};
use crate::tutorial::{Lesson, Tutorial};
use aftermine_sim::BuildingKind;

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
fn own(game: &Game, kind: UnitKind) -> Vec<(aftermine_sim::UnitId, Vec2)> {
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
                && matches!(u.order, aftermine_sim::Order::Harvest { .. })
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

/// Open ground beside the Harvesters, where the raid is headed, and
/// beside the Turrets, where the Warden is headed.
const BY_HARVESTERS: Vec2 = Vec2::new(7.5, 25.5);
const BY_TURRETS: Vec2 = Vec2::new(10.5, 20.5);
/// Open ground on the high ground, beside the Sentinels holding it.
const ON_THE_HEIGHTS: Vec2 = Vec2::new(50.5, 9.5);
/// Open ground west of the enemy outpost.
const BY_THE_OUTPOST: Vec2 = Vec2::new(50.5, 19.5);

/// Selects every own machine that can fight. Selecting has its own
/// lesson; this one is about what the army does once chosen.
fn select_army(game: &mut Game) {
    let human = game.presentation.human;
    game.presentation.selection.units = game
        .state
        .units()
        .iter()
        .filter(|u| u.player == human && u.kind.stats().can_fight())
        .map(|u| u.id)
        .collect();
    game.presentation.selection.buildings.clear();
}

/// Centers the camera on `world`, as the camera keys or minimap would.
fn look_at(game: &mut Game, input: &InputState, world: Vec2) {
    game.presentation.camera.center = world;
    game.presentation.camera.pan(Vec2::ZERO);
    relayout(game, input);
}

/// The middle of the standing enemy machines of `kind`.
fn enemy(game: &Game, kind: UnitKind) -> Option<Vec2> {
    let squad: Vec<Vec2> = game
        .state
        .units()
        .iter()
        .filter(|u| u.player != game.presentation.human && u.kind == kind)
        .map(|u| vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>()))
        .collect();
    (!squad.is_empty()).then(|| squad.iter().copied().sum::<Vec2>() / squad.len() as f32)
}

/// Sends the army after the enemy machines of `kind` until the card
/// moves on to `next`, ordering again whenever the army has gone idle
/// with some of them still standing.
fn defeat(
    game: &mut Game,
    input: &mut InputState,
    kind: UnitKind,
    next: &str,
    hunt_to: &dyn Fn(&mut Game, &mut InputState, Vec2),
) {
    for _ in 0..8 {
        let Some(squad) = enemy(game, kind) else {
            break;
        };
        hunt_to(game, input, squad);
        for _ in 0..20 * 30 {
            if objective(game) == Some(next) {
                return;
            }
            game.do_tick();
            relayout(game, input);
            Tutorial::advance_in(game);
        }
    }
    run_to(game, input, Some(next));
}

fn staged(game: &Game, stance: fn(&Command) -> bool) -> bool {
    game.pending.iter().any(|c| stance(&c.command))
}

fn hunt(command: &Command) -> bool {
    matches!(command, Command::Hunt { .. })
}

fn run(command: &Command) -> bool {
    matches!(command, Command::Run { .. })
}

/// The enemy outpost's Fabricator, while it stands.
fn outpost(game: &Game) -> Option<Vec2> {
    game.state
        .buildings()
        .iter()
        .find(|b| b.player != game.presentation.human && b.kind == BuildingKind::Fabricator)
        .map(|b| {
            let c = b.center();
            vec2(c.x.to_num::<f32>(), c.y.to_num::<f32>())
        })
}

#[test]
fn the_fight_lesson_plays_as_its_mouse_and_keys_card_says() {
    let (mut game, mut input) = begin(Lesson::Fight);
    let hunt_to = |game: &mut Game, input: &mut InputState, world: Vec2| {
        select_army(game);
        look_at(game, input, world);
        press(game, input, Key::F);
        let p = on_screen(game, world);
        click_at(game, input, p);
        assert!(staged(game, hunt), "F and a click send the army hunting");
    };

    assert_eq!(objective(&game), Some("Stop the raid"));
    assert!(copy(&game, Pointer::Mouse, true).contains("press F"));
    hunt_to(&mut game, &mut input, BY_HARVESTERS);
    run_to(&mut game, &input, Some("Get around the rocks"));

    defeat(
        &mut game,
        &mut input,
        UnitKind::Lancer,
        "Take the ramp",
        &hunt_to,
    );

    hunt_to(&mut game, &mut input, ON_THE_HEIGHTS);
    run_to(&mut game, &input, Some("Fall back"));

    assert!(copy(&game, Pointer::Mouse, true).contains("press G"));
    select_army(&mut game);
    look_at(&mut game, &input, BY_TURRETS);
    press(&mut game, &mut input, Key::G);
    let p = on_screen(&game, BY_TURRETS);
    click_at(&mut game, &mut input, p);
    assert!(staged(&game, run), "G and a click run the army home");
    run_to(&mut game, &input, Some("Destroy the outpost"));

    hunt_to(&mut game, &mut input, BY_THE_OUTPOST);
    run_until(&mut game, &input, "the outpost Turret's fall", |game| {
        !game
            .state
            .buildings()
            .iter()
            .any(|b| b.player != game.presentation.human && b.kind == BuildingKind::Turret)
    });
    if let Some(fabricator) = outpost(&game) {
        select_army(&mut game);
        look_at(&mut game, &input, fabricator);
        right_click_at(&mut game, &mut input, fabricator);
        assert!(
            game.pending
                .iter()
                .any(|c| matches!(c.command, Command::Attack { .. })),
            "a right-click on the Fabricator attacks it"
        );
    }
    run_to(&mut game, &input, None);
}

#[test]
fn the_fight_lesson_plays_as_its_touch_card_says() {
    let (mut game, mut input) = begin(Lesson::Fight);
    let order_to = |game: &mut Game, input: &mut InputState, action, world: Vec2| {
        select_army(game);
        look_at(game, input, world);
        let verb = card(game, CardAction::Dispatch(action));
        tap_at(game, input, verb);
        let p = on_screen(game, world);
        tap_at(game, input, p);
    };
    let hunt_to = |game: &mut Game, input: &mut InputState, world: Vec2| {
        order_to(game, input, crate::action::Action::Hunt, world);
        assert!(staged(game, hunt), "Hunt and a tap send the army hunting");
    };

    assert_eq!(objective(&game), Some("Stop the raid"));
    assert!(copy(&game, Pointer::Touch, false).contains("tap Hunt"));
    hunt_to(&mut game, &mut input, BY_HARVESTERS);
    run_to(&mut game, &input, Some("Get around the rocks"));

    defeat(
        &mut game,
        &mut input,
        UnitKind::Lancer,
        "Take the ramp",
        &hunt_to,
    );

    hunt_to(&mut game, &mut input, ON_THE_HEIGHTS);
    run_to(&mut game, &input, Some("Fall back"));

    assert!(copy(&game, Pointer::Touch, false).contains("tap Run"));
    order_to(
        &mut game,
        &mut input,
        crate::action::Action::Run,
        BY_TURRETS,
    );
    assert!(staged(&game, run), "Run and a tap run the army home");
    run_to(&mut game, &input, Some("Destroy the outpost"));

    hunt_to(&mut game, &mut input, BY_THE_OUTPOST);
    run_until(&mut game, &input, "the outpost Turret's fall", |game| {
        !game
            .state
            .buildings()
            .iter()
            .any(|b| b.player != game.presentation.human && b.kind == BuildingKind::Turret)
    });
    if let Some(fabricator) = outpost(&game) {
        select_army(&mut game);
        look_at(&mut game, &input, fabricator);
        long_press_at(&mut game, &mut input, fabricator);
    }
    run_to(&mut game, &input, None);
}

/// Where See and sustain's buildings go: the Array and the Flak Turret
/// east of the Foundry, and the Repair Bay beside it.
const ARRAY_SPOT: Vec2 = Vec2::new(9.5, 10.5);
const FLAK_SPOT: Vec2 = Vec2::new(10.5, 12.5);
const REPAIR_BAY_SPOT: Vec2 = Vec2::new(7.0, 15.0);
/// Open ground far to the east, past the gap in the ridge.
const FAR_EAST: Vec2 = Vec2::new(50.5, 12.5);
/// The scrap pile north of the Foundry.
const NORTH_PILE: Vec2 = Vec2::new(2.5, 6.5);

/// The center of the first damaged own building, or else the first
/// wounded own ground machine.
fn damaged(game: &Game) -> Vec2 {
    let human = game.presentation.human;
    let building = game
        .state
        .buildings()
        .iter()
        .find(|b| b.player == human && b.built() && b.hp < b.stats().max_hp)
        .map(aftermine_sim::Building::center);
    let machine = game
        .state
        .units()
        .iter()
        .find(|u| {
            u.player == human
                && u.hp < u.kind.stats().max_hp
                && u.domain() == aftermine_sim::stats::Domain::Ground
        })
        .map(|u| u.pos);
    let at = building.or(machine).expect("the raid left damage");
    vec2(at.x.to_num::<f32>(), at.y.to_num::<f32>())
}

/// The middle of the first wreck tile.
fn wreck(game: &Game) -> Vec2 {
    let map = game.state.map();
    (0..map.height())
        .flat_map(|y| (0..map.width()).map(move |x| chassis::grid::TilePos::new(x, y)))
        .find(|tile| map.wreck_at(*tile) > 0)
        .map(|tile| vec2(tile.x as f32 + 0.5, tile.y as f32 + 0.5))
        .expect("the raid left a wreck")
}

/// Points the camera at `world` and picks the own machine standing there,
/// with a click or a tap.
fn select_at(game: &mut Game, input: &mut InputState, world: Vec2, touch: bool) {
    look_at(game, input, world);
    let p = on_screen(game, world);
    if touch {
        tap_at(game, input, p);
    } else {
        // Well apart from the last click, so it is no double-click.
        input.now += 1.0;
        click_at(game, input, p);
    }
    assert_eq!(
        game.presentation.selection.units.len(),
        1,
        "the {} picks one machine",
        if touch { "tap" } else { "click" }
    );
}

/// Selects a Harvester, opens construction, picks `kind`, and places it
/// at `spot` with a click, or with a drag of the outline and the check
/// mark.
fn place(game: &mut Game, input: &mut InputState, kind: BuildingKind, spot: Vec2, touch: bool) {
    let cost = kind.base_stats().construction.map_or(0, |c| c.cost);
    run_until(game, input, "the bank covering the building", |game| {
        game.state.player(game.presentation.human).scrap >= cost
    });
    let builder = own(game, UnitKind::Harvester)[0].1;
    select_at(game, input, builder, touch);
    if !input.construction_open() {
        if touch {
            let build = card(
                game,
                CardAction::Dispatch(crate::action::Action::ToggleBuildPalette),
            );
            tap_at(game, input, build);
        } else {
            press(game, input, Key::B);
        }
    }
    let armed = card(game, CardAction::ArmBuild(kind));
    look_at(game, input, spot);
    let p = on_screen(game, spot);
    if touch {
        tap_at(game, input, armed);
        tap_at(game, input, p);
        tap_check(game, input);
    } else {
        click_at(game, input, armed);
        click_at(game, input, p);
    }
    assert_eq!(placed(game, kind), 1, "the {kind:?} is placed");
}

/// Plays See and sustain the way its cards word it, with a mouse and keys
/// or with a finger.
fn play_sustain(touch: bool) {
    let (mut game, mut input) = begin(Lesson::Sustain);
    let (pointer, order) = if touch {
        (Pointer::Touch, "long-press")
    } else {
        (Pointer::Mouse, "right-click")
    };
    let order_at = |game: &mut Game, input: &mut InputState, world: Vec2| {
        look_at(game, input, world);
        if touch {
            long_press_at(game, input, world);
        } else {
            right_click_at(game, input, world);
        }
    };

    assert_eq!(objective(&game), Some("Scout the east"));
    assert!(
        copy(&game, pointer, !touch).contains(&format!("Surveyor and {order} far to the east"))
    );
    let surveyor = own(&game, UnitKind::Surveyor)[0].1;
    select_at(&mut game, &mut input, surveyor, touch);
    order_at(&mut game, &mut input, FAR_EAST);
    assert!(
        staged(&game, |c| matches!(c, Command::Scout { .. })),
        "a lone Surveyor scouts where it is sent"
    );
    run_to(&mut game, &input, Some("Build an Array"));

    assert!(copy(&game, pointer, !touch).contains("Array in the Utility column"));
    place(
        &mut game,
        &mut input,
        BuildingKind::Array,
        ARRAY_SPOT,
        touch,
    );
    run_to(&mut game, &input, Some("Build a Flak Turret"));

    assert!(copy(&game, pointer, !touch).contains("Flak Turret in the Defense column"));
    place(
        &mut game,
        &mut input,
        BuildingKind::FlakTurret,
        FLAK_SPOT,
        touch,
    );
    run_to(&mut game, &input, Some("Shoot down the Buzzards"));

    assert!(copy(&game, pointer, !touch).contains("select an idle Harvester"));
    while hauling(&game) < 3 {
        if touch {
            let badge = game.presentation.layout.get().idle_badge;
            tap_at(&mut game, &mut input, badge.center());
        } else {
            press(&mut game, &mut input, Key::N);
        }
        assert_eq!(game.presentation.selection.units.len(), 1);
        order_at(&mut game, &mut input, NORTH_PILE);
        game.do_tick();
        relayout(&game, &input);
    }
    run_to(&mut game, &input, Some("Repair the damage"));
    assert_eq!(
        own(&game, UnitKind::Harvester).len(),
        3,
        "Harvesters sent back to collecting stay out of the fight"
    );

    assert!(copy(&game, pointer, !touch).contains(&format!("{order} a damaged structure")));
    let welder = own(&game, UnitKind::Harvester)[0].1;
    select_at(&mut game, &mut input, welder, touch);
    let patient = damaged(&game);
    order_at(&mut game, &mut input, patient);
    assert!(
        staged(&game, |c| matches!(
            c,
            Command::Repair { .. } | Command::RepairUnit { .. }
        )),
        "a Harvester sent onto damage welds it"
    );
    run_to(&mut game, &input, Some("Build a Repair Bay"));

    assert!(copy(&game, pointer, !touch).contains("Repair Bay in the Utility column"));
    place(
        &mut game,
        &mut input,
        BuildingKind::RepairBay,
        REPAIR_BAY_SPOT,
        touch,
    );
    run_to(&mut game, &input, Some("Collect the wrecks"));

    assert!(copy(&game, pointer, !touch).contains(&format!("{order} a wreck")));
    let collector = own(&game, UnitKind::Harvester)[0].1;
    select_at(&mut game, &mut input, collector, touch);
    let wreck = wreck(&game);
    order_at(&mut game, &mut input, wreck);
    assert!(
        staged(&game, |c| matches!(c, Command::Harvest { .. })),
        "a Harvester sent onto a wreck collects it"
    );
    run_to(&mut game, &input, None);
}

#[test]
fn the_see_and_sustain_lesson_plays_as_its_mouse_and_keys_card_says() {
    play_sustain(false);
}

#[test]
fn the_see_and_sustain_lesson_plays_as_its_touch_card_says() {
    play_sustain(true);
}
