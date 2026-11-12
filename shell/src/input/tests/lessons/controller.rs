//! Basic Training played the way each card words it to a controller alone:
//! the reticle, A, X, the rings and the D-pad, through the same funnel.

use super::*;
use crate::input::HudHands;
use aftermine_protocol::{PadButton, PadFamily};

const PAD: u64 = 1;
/// Seconds per simulated frame.
const FRAME: f32 = 1.0 / 60.0;

/// The paragraphs the card shows while an Xbox controller is in use.
fn pad_copy(game: &Game) -> String {
    let step = game
        .tutorial
        .as_ref()
        .and_then(Tutorial::current)
        .expect("a step is up");
    let hands = HudHands::Pad {
        names: Some(PadFamily::Xbox),
    }
    .copy_hands(crate::platform::Hands::BUILD);
    step.body(hands)
        .iter()
        .map(|text| crate::render::tutorial_card::expand(text, &classic()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Pad events as the frame loop feeds them: every screen notes the hands,
/// then the match applies them.
fn feed(game: &mut Game, input: &mut InputState, events: &[RawEvent]) {
    for event in events {
        crate::input::track_hands(input, event, false);
    }
    send(game, input, events);
    input.settle_hud();
}

/// One frame of held input, laid out again as a frame does.
fn frame(game: &mut Game, input: &mut InputState) {
    update_held(game, input, FRAME);
    input.now += f64::from(FRAME);
    relayout(game, input);
    Tutorial::advance_in(game);
}

/// The lesson with an Xbox controller in use.
fn pad_begin(lesson: Lesson) -> (Game, InputState) {
    let (mut game, mut input) = begin(lesson);
    feed(
        &mut game,
        &mut input,
        &[RawEvent::PadConnected {
            pad: PAD,
            family: PadFamily::Xbox,
        }],
    );
    input.last_pointer = Pointer::Pad;
    frame(&mut game, &mut input);
    (game, input)
}

fn stick(x: f32, y: f32) -> RawEvent {
    RawEvent::PadStick {
        pad: PAD,
        stick: aftermine_protocol::Stick::Left,
        x,
        y,
    }
}

fn hold(game: &mut Game, input: &mut InputState, button: PadButton) {
    feed(game, input, &[RawEvent::PadDown { pad: PAD, button }]);
}

fn let_go(game: &mut Game, input: &mut InputState, button: PadButton) {
    feed(game, input, &[RawEvent::PadUp { pad: PAD, button }]);
}

/// A press and release, well apart from the last, so it is no double tap.
fn tap(game: &mut Game, input: &mut InputState, button: PadButton) {
    input.now += 1.0;
    hold(game, input, button);
    let_go(game, input, button);
}

/// Rests the reticle on `world`, as moving it there with the stick does.
fn rest(game: &mut Game, input: &mut InputState, world: Vec2) {
    crate::input::reticle::look_at(game, input, world);
    relayout(game, input);
}

/// Rests the reticle on `world` and presses `button` there.
fn tap_at(game: &mut Game, input: &mut InputState, world: Vec2, button: PadButton) {
    rest(game, input, world);
    tap(game, input, button);
}

/// Points the stick at the open ring's slice named `title`.
fn aim(game: &mut Game, input: &mut InputState, title: &str) {
    let ring = crate::input::pad_ring::current(&game.view(), input).expect("a ring is open");
    let titles: Vec<&str> = ring.slices.iter().map(|s| s.card.title.as_str()).collect();
    let index = titles
        .iter()
        .position(|name| *name == title)
        .unwrap_or_else(|| panic!("no {title} slice in {titles:?}"));
    let toward = crate::radial::slice_direction(index, titles.len());
    feed(game, input, &[stick(toward.x, toward.y)]);
    frame(game, input);
    feed(game, input, &[stick(0.0, 0.0)]);
    frame(game, input);
}

/// Points the stick at the open ring's slice named `title` and picks it
/// with A.
fn pick(game: &mut Game, input: &mut InputState, title: &str) {
    aim(game, input, title);
    tap(game, input, PadButton::South);
}

/// Opens the selection's command ring and walks `path` through it.
fn command(game: &mut Game, input: &mut InputState, path: &[&str]) {
    tap(game, input, PadButton::North);
    for title in path {
        pick(game, input, title);
    }
}

/// Selects a Harvester, picks `kind` through Build, and places it at
/// `spot` with A.
fn place(game: &mut Game, input: &mut InputState, path: &[&str], kind: BuildingKind, spot: Vec2) {
    let cost = kind.base_stats().construction.cost;
    run_until(game, input, "the bank covering the building", |game| {
        game.state.player(game.presentation.human).scrap >= cost
    });
    let builder = own(game, UnitKind::Harvester)[0].1;
    tap_at(game, input, builder, PadButton::South);
    command(game, input, path);
    tap_at(game, input, spot, PadButton::South);
    assert!(placed(game, kind) >= 1, "A places the {kind:?}");
}

/// Pushes the stick toward `(x, y)` a frame at a time until `there`.
fn push_until(
    game: &mut Game,
    input: &mut InputState,
    (x, y): (f32, f32),
    there: impl Fn(Vec2) -> bool,
) {
    feed(game, input, &[stick(x, y)]);
    for _ in 0..600 {
        if there(input.reticle.ground(&game.presentation.camera)) {
            break;
        }
        frame(game, input);
    }
    feed(game, input, &[stick(0.0, 0.0)]);
    frame(game, input);
}

#[test]
fn the_command_lesson_plays_as_its_controller_card_says() {
    let (mut game, mut input) = pad_begin(Lesson::Command);

    assert_eq!(objective(&game), Some("Move the camera"));
    assert!(pad_copy(&game).contains("Push the left stick to move the camera"));
    let start = game.presentation.camera.center;
    push_until(&mut game, &mut input, (1.0, 0.0), |at| at.x > start.x + 5.0);

    assert_eq!(objective(&game), Some("Select a Sentinel"));
    assert!(pad_copy(&game).contains("press A to select it"));
    let squad = own(&game, UnitKind::Sentinel);
    tap_at(&mut game, &mut input, squad[0].1, PadButton::South);

    assert_eq!(objective(&game), Some("Select the squad"));
    assert!(pad_copy(&game).contains("Hold A and move the reticle across all four Sentinels"));
    // Across the top pair, down, and back across the bottom pair.
    let (near, far) = squad
        .iter()
        .fold((squad[0].1, squad[0].1), |(near, far), (_, at)| {
            (near.min(*at), far.max(*at))
        });
    rest(&mut game, &mut input, near);
    hold(&mut game, &mut input, PadButton::South);
    push_until(&mut game, &mut input, (1.0, 0.0), |at| at.x >= far.x);
    push_until(&mut game, &mut input, (0.0, 1.0), |at| at.y >= far.y);
    push_until(&mut game, &mut input, (-1.0, 0.0), |at| at.x <= near.x);
    let_go(&mut game, &mut input, PadButton::South);
    assert_eq!(
        game.presentation.selection.units().len(),
        4,
        "the sweep selects the squad"
    );

    assert_eq!(objective(&game), Some("Find the enemy"));
    assert!(pad_copy(&game).contains("press X to move the squad there"));
    tap_at(&mut game, &mut input, vec2(39.5, 12.5), PadButton::West);
    assert!(
        game.pending
            .iter()
            .any(|c| matches!(c.command, Command::Advance { .. })),
        "X on open ground moves the squad"
    );
    run_to(&mut game, &input, Some("Destroy the Fabricator"));

    assert!(pad_copy(&game).contains("Press X on the Fabricator"));
    let target = fabricator(&game);
    tap_at(&mut game, &mut input, target, PadButton::West);
    assert!(
        game.pending
            .iter()
            .any(|c| matches!(c.command, Command::Attack { .. })),
        "X on the Fabricator attacks it"
    );
    run_to(&mut game, &input, None);
}

#[test]
fn the_scrap_lesson_plays_as_its_controller_card_says() {
    let (mut game, mut input) = pad_begin(Lesson::Scrap);

    assert_eq!(objective(&game), Some("Collect scrap"));
    assert!(pad_copy(&game).contains("press X on a scrap pile"));
    let hauler = own(&game, UnitKind::Harvester)[0].1;
    tap_at(&mut game, &mut input, hauler, PadButton::South);
    tap_at(&mut game, &mut input, HOME_PILE, PadButton::West);
    run_to(&mut game, &input, Some("Train a Harvester"));

    assert!(pad_copy(&game).contains("Point the left stick at Harvester and press A"));
    let home = foundry(&game);
    tap_at(&mut game, &mut input, home, PadButton::South);
    command(&mut game, &mut input, &["Harvester"]);
    assert!(input.ring.is_some(), "training leaves the ring open");
    assert!(pad_copy(&game).contains("Press B to close it"));
    tap(&mut game, &mut input, PadButton::East);
    assert!(input.ring.is_none(), "B closes it");
    run_to(&mut game, &input, Some("Put every Harvester to work"));

    assert!(pad_copy(&game).contains("Press D-pad down to select an idle Harvester"));
    while hauling(&game) < 3 {
        tap(&mut game, &mut input, PadButton::DpadDown);
        assert_eq!(
            game.presentation.selection.units().len(),
            1,
            "D-pad down grabs one"
        );
        tap_at(&mut game, &mut input, HOME_PILE, PadButton::West);
        game.do_tick();
        relayout(&game, &input);
    }
    run_to(&mut game, &input, Some("Restore an Extractor"));

    assert!(pad_copy(&game).contains("pick Build, then Economy, then Extractor"));
    let path = ["Build", "Economy", "Extractor"];
    place(
        &mut game,
        &mut input,
        &path,
        BuildingKind::Extractor,
        NEAR_FRAME,
    );
    run_to(&mut game, &input, Some("Restore the far frame"));

    place(
        &mut game,
        &mut input,
        &path,
        BuildingKind::Extractor,
        FAR_FRAME,
    );
    run_to(&mut game, &input, None);
}

#[test]
fn the_build_lesson_plays_as_its_controller_card_says() {
    let (mut game, mut input) = pad_begin(Lesson::Build);

    assert_eq!(objective(&game), Some("Build a Fabricator"));
    assert!(pad_copy(&game).contains("Pick Fabricator in Production"));
    place(
        &mut game,
        &mut input,
        &["Build", "Production", "Fabricator"],
        BuildingKind::Fabricator,
        FABRICATOR_SPOT,
    );
    run_to(&mut game, &input, Some("Build three Barricades"));

    assert!(pad_copy(&game).contains("hold A and move the reticle to lay a row of three"));
    let builder = own(&game, UnitKind::Harvester)[0].1;
    tap_at(&mut game, &mut input, builder, PadButton::South);
    command(&mut game, &mut input, &["Build", "Defense", "Barricade"]);
    rest(&mut game, &mut input, BARRICADES[0]);
    hold(&mut game, &mut input, PadButton::South);
    push_until(&mut game, &mut input, (1.0, 0.0), |at| {
        at.x >= BARRICADES[2].x
    });
    let_go(&mut game, &mut input, PadButton::South);
    assert!(
        placed(&game, BuildingKind::Barricade) >= 3,
        "a held A lays a row"
    );
    run_to(&mut game, &input, Some("Set a rally point"));

    assert!(pad_copy(&game).contains("press X on the ground where they should gather"));
    let fabricator = own_building(&game, BuildingKind::Fabricator);
    tap_at(&mut game, &mut input, fabricator, PadButton::South);
    tap_at(&mut game, &mut input, RALLY_SPOT, PadButton::West);
    assert!(
        game.pending
            .iter()
            .any(|c| matches!(c.command, Command::SetRally { rally: Some(_), .. })),
        "X on ground with the Fabricator selected rallies it"
    );
    run_to(&mut game, &input, Some("Train a Lancer"));

    assert!(pad_copy(&game).contains("pick Lancer, then press B to close the ring"));
    command(&mut game, &mut input, &["Lancer"]);
    tap(&mut game, &mut input, PadButton::East);
    assert!(input.ring.is_none(), "B closes the ring");
    run_to(&mut game, &input, Some("Make a control group"));

    assert!(pad_copy(&game).contains("Point the left stick at Group 1 and press X"));
    let lancer = own(&game, UnitKind::Lancer)[0];
    tap_at(&mut game, &mut input, lancer.1, PadButton::South);
    tap(&mut game, &mut input, PadButton::DpadRight);
    aim(&mut game, &mut input, "Group 1");
    tap(&mut game, &mut input, PadButton::West);
    assert_eq!(
        objective(&game),
        None,
        "saving the group finishes the lesson"
    );

    assert!(input.ring.is_none(), "saving a group closes the ring");
    game.presentation.selection.clear();
    tap(&mut game, &mut input, PadButton::DpadRight);
    pick(&mut game, &mut input, "Group 1");
    assert_eq!(
        game.presentation.selection.units(),
        [lancer.0],
        "picking Group 1 with A recalls it, as the card says"
    );
}

#[test]
fn the_build_lesson_s_lt_chain_ends_with_b_as_its_controller_card_says() {
    let (mut game, mut input) = pad_begin(Lesson::Build);
    place(
        &mut game,
        &mut input,
        &["Build", "Production", "Fabricator"],
        BuildingKind::Fabricator,
        FABRICATOR_SPOT,
    );
    run_to(&mut game, &input, Some("Build three Barricades"));

    assert!(
        pad_copy(&game).contains("hold LT and press A on three spots, and press B to stop placing")
    );
    let builder = own(&game, UnitKind::Harvester)[0].1;
    tap_at(&mut game, &mut input, builder, PadButton::South);
    command(&mut game, &mut input, &["Build", "Defense", "Barricade"]);
    hold(&mut game, &mut input, PadButton::LeftTrigger);
    for spot in BARRICADES {
        tap_at(&mut game, &mut input, spot, PadButton::South);
    }
    let_go(&mut game, &mut input, PadButton::LeftTrigger);
    assert_eq!(
        placed(&game, BuildingKind::Barricade),
        3,
        "a held LT chains them"
    );
    assert!(input.placing().is_some(), "letting go of LT keeps placing");
    tap(&mut game, &mut input, PadButton::East);
    assert!(input.placing().is_none(), "B stops placing");
    assert!(input.ring.is_none(), "without opening the build ring");
    assert!(!input.construction_open(), "and closes construction");
    run_to(&mut game, &input, Some("Set a rally point"));

    let fabricator = own_building(&game, BuildingKind::Fabricator);
    tap_at(&mut game, &mut input, fabricator, PadButton::South);
    assert_eq!(
        game.presentation.selection.buildings().len(),
        1,
        "A selects the Fabricator"
    );
    tap_at(&mut game, &mut input, RALLY_SPOT, PadButton::West);
    assert!(
        game.pending
            .iter()
            .any(|c| matches!(c.command, Command::SetRally { rally: Some(_), .. })),
        "X on ground with the Fabricator selected rallies it"
    );
}

#[test]
fn the_fight_lesson_plays_as_its_controller_card_says() {
    let (mut game, mut input) = pad_begin(Lesson::Fight);
    let order_to = |game: &mut Game, input: &mut InputState, verb: &str, world: Vec2| {
        tap(game, input, PadButton::LeftShoulder);
        rest(game, input, world);
        command(game, input, &[verb]);
        tap(game, input, PadButton::South);
    };
    let hunt_to = |game: &mut Game, input: &mut InputState, world: Vec2| {
        order_to(game, input, "Hunt", world);
        assert!(staged(game, hunt), "Hunt and A send the army hunting");
    };

    assert_eq!(objective(&game), Some("Stop the raid"));
    assert!(pad_copy(&game).contains("Press LB to select your army, then press Y and pick Hunt"));
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

    assert!(pad_copy(&game).contains("press Y and pick Run"));
    order_to(&mut game, &mut input, "Run", BY_TURRETS);
    assert!(staged(&game, run), "Run and A run the army home");
    run_to(&mut game, &input, Some("Destroy the outpost"));

    assert!(pad_copy(&game).contains("press X on each structure"));
    hunt_to(&mut game, &mut input, BY_THE_OUTPOST);
    run_until(&mut game, &input, "the outpost Turret's fall", |game| {
        !game
            .state
            .buildings()
            .iter()
            .any(|b| b.player != game.presentation.human && b.kind == BuildingKind::Turret)
    });
    if let Some(fabricator) = outpost(&game) {
        tap(&mut game, &mut input, PadButton::LeftShoulder);
        tap_at(&mut game, &mut input, fabricator, PadButton::West);
        assert!(
            game.pending
                .iter()
                .any(|c| matches!(c.command, Command::Attack { .. })),
            "X on the Fabricator attacks it"
        );
    }
    run_to(&mut game, &input, None);
}

#[test]
fn the_see_and_sustain_lesson_plays_as_its_controller_card_says() {
    let (mut game, mut input) = pad_begin(Lesson::Sustain);
    let select = |game: &mut Game, input: &mut InputState, world: Vec2| {
        tap_at(game, input, world, PadButton::South);
        assert_eq!(
            game.presentation.selection.units().len(),
            1,
            "A picks one machine"
        );
    };

    assert_eq!(objective(&game), Some("Scout the east"));
    assert!(pad_copy(&game).contains("move the reticle far to the east, and press X"));
    let surveyor = own(&game, UnitKind::Surveyor)[0].1;
    select(&mut game, &mut input, surveyor);
    tap_at(&mut game, &mut input, FAR_EAST, PadButton::West);
    assert!(
        staged(&game, |c| matches!(c, Command::Scout { .. })),
        "a lone Surveyor scouts where it is sent"
    );
    run_to(&mut game, &input, Some("Build an Array"));

    assert!(pad_copy(&game).contains("pick Build, then Utility, then Array"));
    place(
        &mut game,
        &mut input,
        &["Build", "Utility", "Array"],
        BuildingKind::Array,
        ARRAY_SPOT,
    );
    run_to(&mut game, &input, Some("Build a Flak Turret"));

    assert!(pad_copy(&game).contains("pick Build, then Defense, then Flak Turret"));
    place(
        &mut game,
        &mut input,
        &["Build", "Defense", "Flak Turret"],
        BuildingKind::FlakTurret,
        FLAK_SPOT,
    );
    run_to(&mut game, &input, Some("Shoot down the Buzzards"));

    assert!(pad_copy(&game).contains("Press D-pad down to select an idle Harvester"));
    while hauling(&game) < 3 {
        tap(&mut game, &mut input, PadButton::DpadDown);
        assert_eq!(game.presentation.selection.units().len(), 1);
        tap_at(&mut game, &mut input, NORTH_PILE, PadButton::West);
        game.do_tick();
        relayout(&game, &input);
    }
    run_to(&mut game, &input, Some("Repair the damage"));

    assert!(pad_copy(&game).contains("press X on a damaged structure"));
    let welder = own(&game, UnitKind::Harvester)[0].1;
    select(&mut game, &mut input, welder);
    let patient = damaged(&game);
    tap_at(&mut game, &mut input, patient, PadButton::West);
    assert!(
        staged(&game, |c| matches!(
            c,
            Command::Repair { .. } | Command::RepairUnit { .. }
        )),
        "a Harvester sent onto damage welds it"
    );
    run_to(&mut game, &input, Some("Build a Repair Bay"));

    assert!(pad_copy(&game).contains("pick Build, then Utility, then Repair Bay"));
    place(
        &mut game,
        &mut input,
        &["Build", "Utility", "Repair Bay"],
        BuildingKind::RepairBay,
        REPAIR_BAY_SPOT,
    );
    run_to(&mut game, &input, Some("Collect the wrecks"));

    assert!(pad_copy(&game).contains("press X on a wreck"));
    let collector = own(&game, UnitKind::Harvester)[0].1;
    select(&mut game, &mut input, collector);
    let wreck = wreck(&game);
    tap_at(&mut game, &mut input, wreck, PadButton::West);
    assert!(
        staged(&game, |c| matches!(c, Command::Harvest { .. })),
        "a Harvester sent onto a wreck collects it"
    );
    run_to(&mut game, &input, None);
}
