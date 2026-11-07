use super::super::tests::pad_scene::{FRAME, down, left_stick, pad_input, scene, units_of, up};
use super::super::{ArmedMode, InputState, apply_events, track_hands, update_held};
use super::current;
use crate::action::BindingMap;
use crate::game::Game;
use aftermine_protocol::{PadButton, RawEvent};
use aftermine_sim::{BuildingKind, Command, UnitKind};

/// Publishes the panel as the HUD's layout pass does before input.
fn publish(game: &Game, input: &InputState) {
    *game.presentation.panel_model.borrow_mut() =
        crate::panel::build_for_input(&game.view(), &BindingMap::classic(), input);
}

/// Feeds events as the Playing frame does, the HUD laid out between them.
fn feed(game: &mut Game, input: &mut InputState, events: &[RawEvent]) {
    for event in events {
        track_hands(input, event, false);
    }
    publish(game, input);
    apply_events(game, input, &BindingMap::classic(), events, publish);
}

/// One frame of held input after a fresh layout.
fn frame(game: &mut Game, input: &mut InputState) {
    publish(game, input);
    update_held(game, input, FRAME);
    input.now += f64::from(FRAME);
}

fn tap(game: &mut Game, input: &mut InputState, button: PadButton) {
    feed(game, input, &[down(button), up(button)]);
}

/// Pushes the stick toward `(x, y)` for a frame, then lets it go.
fn point(game: &mut Game, input: &mut InputState, (x, y): (f32, f32)) {
    feed(game, input, &[left_stick(x, y)]);
    frame(game, input);
    feed(game, input, &[left_stick(0.0, 0.0)]);
    frame(game, input);
}

/// Points the stick at the open ring's slice named `title`.
fn aim(game: &mut Game, input: &mut InputState, title: &str) {
    let names = titles(game, input);
    let index = names
        .iter()
        .position(|name| name == title)
        .unwrap_or_else(|| panic!("no {title} slice in {names:?}"));
    let toward = crate::radial::slice_direction(index, names.len());
    point(game, input, (toward.x, toward.y));
    assert_eq!(pointed(game, input).as_deref(), Some(title));
}

/// Points at `title` and picks it.
fn pick(game: &mut Game, input: &mut InputState, title: &str) {
    aim(game, input, title);
    tap(game, input, PadButton::South);
    frame(game, input);
}

/// The open ring's slice titles, in order clockwise from the top.
fn titles(game: &Game, input: &InputState) -> Vec<String> {
    current(&game.view(), input)
        .expect("a ring is open")
        .slices
        .iter()
        .map(|slice| slice.card.title.clone())
        .collect()
}

fn heading(game: &Game, input: &InputState) -> Option<String> {
    current(&game.view(), input).map(|ring| ring.heading)
}

/// The title of the slice the stick points at.
fn pointed(game: &Game, input: &InputState) -> Option<String> {
    let ring = current(&game.view(), input)?;
    let index = input.ring.as_ref()?.pointed(&ring)?;
    Some(ring.slices[index].card.title.clone())
}

/// The queued count the open ring shows on `title`.
fn count(game: &Game, input: &InputState, title: &str) -> usize {
    current(&game.view(), input)
        .and_then(|ring| {
            ring.slices
                .into_iter()
                .find(|slice| slice.card.title == title)
        })
        .map_or(0, |slice| slice.count)
}

fn names(titles: &[&str]) -> Vec<String> {
    titles.iter().map(|title| (*title).to_string()).collect()
}

/// A scene with `units` of seat 0 selected and the ring opened on them.
fn opened_on(units: &[(u8, UnitKind, i32, i32)]) -> (Game, InputState) {
    let mut game = scene(units, &[]);
    let mut input = pad_input(&mut game);
    let mut selected: Vec<_> = units
        .iter()
        .flat_map(|(_, kind, ..)| units_of(&game, 0, *kind))
        .collect();
    selected.sort_unstable();
    selected.dedup();
    game.presentation.selection.units = selected;
    frame(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::North);
    frame(&mut game, &mut input);
    (game, input)
}

fn sentinels() -> (Game, InputState) {
    opened_on(&[
        (0, UnitKind::Sentinel, 30, 20),
        (0, UnitKind::Sentinel, 31, 20),
    ])
}

fn harvester() -> (Game, InputState) {
    opened_on(&[(0, UnitKind::Harvester, 30, 20)])
}

/// A Foundry selected and its ring open.
fn foundry() -> (Game, InputState) {
    let mut game = scene(&[], &[(0, BuildingKind::Foundry, 30, 18)]);
    let mut input = pad_input(&mut game);
    game.presentation.selection.buildings = game
        .state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Foundry)
        .map(|b| b.id)
        .collect();
    frame(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::North);
    frame(&mut game, &mut input);
    (game, input)
}

fn denied(game: &Game) -> bool {
    game.presentation
        .sounds_pending
        .iter()
        .any(|(sound, _)| *sound == crate::game::SoundKind::Denied)
}

fn toasted(game: &Game, text: &str) -> bool {
    game.presentation
        .toasts
        .iter()
        .any(|toast| toast.text == text)
}

#[test]
fn y_opens_the_ring_and_b_closes_it_keeping_the_selection() {
    let (mut game, mut input) = sentinels();
    let selected = game.presentation.selection.units.clone();
    // Idle units keep Stop's place, grayed, so it never moves.
    assert_eq!(
        titles(&game, &input),
        names(&["Run", "Hunt", "Patrol", "Accompany", "Retire", "Stop"])
    );
    assert_eq!(heading(&game, &input).as_deref(), Some("COMMANDS"));
    tap(&mut game, &mut input, PadButton::East);
    assert!(input.ring.is_none());
    assert_eq!(game.presentation.selection.units, selected);
}

#[test]
fn y_with_nothing_to_offer_refuses() {
    let mut game = scene(&[(0, UnitKind::Sentinel, 30, 20)], &[]);
    let mut input = pad_input(&mut game);
    tap(&mut game, &mut input, PadButton::North);
    assert!(input.ring.is_none());
    assert!(denied(&game));
}

#[test]
fn the_stick_points_and_a_arms_what_the_card_arms() {
    let (mut game, mut input) = sentinels();
    // Up-right of six slices is the second: Hunt. Letting go keeps it.
    point(&mut game, &mut input, (0.7, -0.7));
    assert_eq!(pointed(&game, &input).as_deref(), Some("Hunt"));
    tap(&mut game, &mut input, PadButton::South);
    assert!(input.ring.is_none());
    assert_eq!(input.armed_mode(), Some(ArmedMode::Hunt));
}

#[test]
fn a_with_nothing_pointed_refuses_and_keeps_the_ring() {
    let (mut game, mut input) = sentinels();
    tap(&mut game, &mut input, PadButton::South);
    assert!(input.ring.is_some());
    assert!(denied(&game));
}

#[test]
fn a_grayed_slice_explains_itself_and_stages_nothing() {
    let (mut game, mut input) = sentinels();
    pick(&mut game, &mut input, "Stop");
    assert!(toasted(&game, "Nothing to stop"));
    assert!(game.pending.is_empty());
    assert!(input.ring.is_some(), "a refused pick leaves the ring open");
}

#[test]
fn stop_stages_the_cards_own_order_once_the_units_are_busy() {
    let (mut game, mut input) = sentinels();
    tap(&mut game, &mut input, PadButton::East);
    let units = game.presentation.selection.units.clone();
    game.issue(Command::Advance {
        units: units.clone(),
        goal: chassis::grid::TilePos::new(40, 30),
        queue: false,
    });
    game.advance_ticks(1);
    frame(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::North);
    pick(&mut game, &mut input, "Stop");
    assert!(input.ring.is_none());
    assert!(game.pending.iter().any(|command| command.command
        == Command::Stop {
            units: units.clone()
        }));
}

#[test]
fn the_camera_holds_while_the_stick_points_and_until_it_is_let_go() {
    let (mut game, mut input) = sentinels();
    let center = game.presentation.camera.center;
    feed(&mut game, &mut input, &[left_stick(1.0, 0.0)]);
    for _ in 0..10 {
        frame(&mut game, &mut input);
    }
    assert_eq!(game.presentation.camera.center, center);
    // Picking with the stick still pushed leaves it latched.
    tap(&mut game, &mut input, PadButton::South);
    assert!(input.ring.is_none());
    frame(&mut game, &mut input);
    assert_eq!(game.presentation.camera.center, center);
    feed(&mut game, &mut input, &[left_stick(0.0, 0.0)]);
    frame(&mut game, &mut input);
    feed(&mut game, &mut input, &[left_stick(1.0, 0.0)]);
    frame(&mut game, &mut input);
    assert_ne!(game.presentation.camera.center, center);
}

#[test]
fn the_release_of_a_press_the_ring_took_never_reaches_the_match() {
    let (mut game, mut input) = sentinels();
    tap(&mut game, &mut input, PadButton::East);
    feed(&mut game, &mut input, &[down(PadButton::North)]);
    assert!(input.ring.is_some());
    feed(&mut game, &mut input, &[up(PadButton::North)]);
    assert!(input.ring_held.is_empty());
    assert!(
        input.ring.is_some(),
        "the ring stays open with nothing held"
    );
    // The A that picks closes the ring, and its release goes nowhere.
    aim(&mut game, &mut input, "Patrol");
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    assert!(input.ring.is_none());
    feed(&mut game, &mut input, &[up(PadButton::South)]);
    assert!(input.ring_held.is_empty());
    assert!(input.pad_press.is_none());
}

#[test]
fn other_buttons_wait_while_the_left_trigger_still_queues() {
    let mut game = scene(
        &[
            (0, UnitKind::Harvester, 30, 20),
            (0, UnitKind::Sentinel, 34, 20),
        ],
        &[],
    );
    let mut input = pad_input(&mut game);
    game.presentation.selection.units = units_of(&game, 0, UnitKind::Harvester);
    frame(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::North);
    let selected = game.presentation.selection.units.clone();
    for button in [
        PadButton::LeftShoulder,
        PadButton::RightShoulder,
        PadButton::RightTrigger,
        PadButton::DpadUp,
    ] {
        tap(&mut game, &mut input, button);
    }
    assert!(input.ring.is_some());
    assert_eq!(game.presentation.selection.units, selected);
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    assert!(input.queue_held());
    feed(&mut game, &mut input, &[up(PadButton::LeftTrigger)]);
    assert!(!input.queue_held());
}

#[test]
fn another_hand_or_a_lost_controller_closes_the_ring() {
    let (mut game, mut input) = sentinels();
    feed(
        &mut game,
        &mut input,
        &[RawEvent::MouseMove { x: 5.0, y: 5.0 }],
    );
    assert!(input.ring.is_none());
    tap(&mut game, &mut input, PadButton::North);
    feed(
        &mut game,
        &mut input,
        &[RawEvent::KeyDown {
            key: aftermine_protocol::Key::H,
        }],
    );
    assert!(input.ring.is_none());
    tap(&mut game, &mut input, PadButton::North);
    assert!(input.ring.is_some());
    feed(
        &mut game,
        &mut input,
        &[RawEvent::PadDisconnected { pad: 1 }],
    );
    assert!(input.ring.is_none());
}

#[test]
fn a_ring_left_with_nothing_closes() {
    let (mut game, mut input) = sentinels();
    game.presentation.selection.units.clear();
    frame(&mut game, &mut input);
    assert!(input.ring.is_none());
}

#[test]
fn a_harvester_builds_through_the_build_rings() {
    let (mut game, mut input) = harvester();
    assert_eq!(
        titles(&game, &input),
        names(&[
            "Patrol",
            "Accompany",
            "Salvage",
            "Repair",
            "Retire",
            "Build",
            "Return cargo",
            "Stop"
        ])
    );
    pick(&mut game, &mut input, "Build");
    assert!(input.build_menu, "the panel shows the palette beneath");
    assert_eq!(heading(&game, &input).as_deref(), Some("BUILD"));
    assert_eq!(
        titles(&game, &input),
        names(&[
            "Economy",
            "Production",
            "Defense",
            "Utility",
            "Strategic",
            "Experimental"
        ])
    );
    pick(&mut game, &mut input, "Defense");
    assert_eq!(input.build_category, Some(2));
    assert_eq!(heading(&game, &input).as_deref(), Some("DEFENSE"));
    assert_eq!(
        titles(&game, &input),
        names(&["Turret", "Flak Turret", "Bastion", "Barricade"])
    );
    // B steps back as the build keys do.
    tap(&mut game, &mut input, PadButton::East);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("BUILD"));
    assert_eq!(input.build_category, None);
    assert!(input.build_menu);
    pick(&mut game, &mut input, "Defense");
    pick(&mut game, &mut input, "Turret");
    assert!(input.ring.is_none());
    assert_eq!(
        input.armed_mode(),
        Some(ArmedMode::Build(BuildingKind::Turret))
    );
    // Y over the armed placement reopens on its category, and closing
    // again keeps the placement.
    tap(&mut game, &mut input, PadButton::North);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("DEFENSE"));
    tap(&mut game, &mut input, PadButton::North);
    assert_eq!(
        input.armed_mode(),
        Some(ArmedMode::Build(BuildingKind::Turret))
    );
}

#[test]
fn closing_the_build_rings_closes_the_palette_they_opened() {
    let (mut game, mut input) = harvester();
    pick(&mut game, &mut input, "Build");
    tap(&mut game, &mut input, PadButton::East);
    frame(&mut game, &mut input);
    assert!(!input.build_menu);
    assert_eq!(heading(&game, &input).as_deref(), Some("COMMANDS"));
    pick(&mut game, &mut input, "Build");
    pick(&mut game, &mut input, "Economy");
    tap(&mut game, &mut input, PadButton::North);
    assert!(input.ring.is_none());
    assert!(!input.build_menu);
    assert_eq!(input.build_category, None);
}

#[test]
fn holding_b_closes_every_ring() {
    let (mut game, mut input) = harvester();
    pick(&mut game, &mut input, "Build");
    pick(&mut game, &mut input, "Utility");
    feed(&mut game, &mut input, &[down(PadButton::East)]);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("BUILD"));
    // Let go early, and B was only one step back.
    feed(&mut game, &mut input, &[up(PadButton::East)]);
    for _ in 0..40 {
        frame(&mut game, &mut input);
    }
    assert_eq!(heading(&game, &input).as_deref(), Some("BUILD"));
    feed(&mut game, &mut input, &[down(PadButton::East)]);
    for _ in 0..40 {
        frame(&mut game, &mut input);
    }
    assert!(input.ring.is_none());
    assert!(!input.build_menu);
    feed(&mut game, &mut input, &[up(PadButton::East)]);
    assert!(input.ring_held.is_empty());
}

#[test]
fn a_structure_ring_trains_counts_and_cancels_one() {
    let (mut game, mut input) = foundry();
    assert_eq!(
        titles(&game, &input),
        names(&[
            "Set rally",
            "Clear rally",
            "Harvester",
            "Sentinel",
            "Scuttler",
            "Excavator",
            "Surveyor"
        ])
    );
    pick(&mut game, &mut input, "Harvester");
    tap(&mut game, &mut input, PadButton::South);
    frame(&mut game, &mut input);
    assert!(input.ring.is_some(), "training leaves the ring open");
    assert_eq!(count(&game, &input, "Harvester"), 2);
    tap(&mut game, &mut input, PadButton::West);
    frame(&mut game, &mut input);
    assert_eq!(count(&game, &input, "Harvester"), 1);
    // A slice with nothing queued has nothing for X to cancel.
    aim(&mut game, &mut input, "Sentinel");
    tap(&mut game, &mut input, PadButton::West);
    assert!(denied(&game));
    assert_eq!(count(&game, &input, "Harvester"), 1);
}

#[test]
fn a_mixed_selection_keeps_its_harvesters_cards_behind_one_slice() {
    let (mut game, mut input) = opened_on(&[
        (0, UnitKind::Sentinel, 30, 20),
        (0, UnitKind::Harvester, 26, 20),
    ]);
    assert_eq!(
        titles(&game, &input),
        names(&[
            "Run",
            "Hunt",
            "Patrol",
            "Accompany",
            "Harvesters",
            "Retire",
            "Stop"
        ])
    );
    pick(&mut game, &mut input, "Harvesters");
    assert_eq!(heading(&game, &input).as_deref(), Some("HARVESTERS"));
    assert_eq!(
        titles(&game, &input),
        names(&["Salvage", "Repair", "Build", "Return cargo"])
    );
    // When the Harvesters leave the selection, their ring falls back to
    // the first.
    game.presentation.selection.units = units_of(&game, 0, UnitKind::Sentinel);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("COMMANDS"));
}

#[test]
fn a_ring_over_four_kinds_with_their_own_cards_gives_the_rest_to_more() {
    let (mut game, mut input) = opened_on(&[
        (0, UnitKind::Sentinel, 30, 20),
        (0, UnitKind::Surveyor, 28, 20),
        (0, UnitKind::Harvester, 26, 20),
        (0, UnitKind::Skyhook, 30, 24),
        (0, UnitKind::Roc, 34, 26),
    ]);
    assert_eq!(
        titles(&game, &input),
        names(&[
            "Scout",
            "Run",
            "Hunt",
            "Patrol",
            "Accompany",
            "Harvesters",
            "More",
            "Stop"
        ])
    );
    pick(&mut game, &mut input, "More");
    assert_eq!(
        titles(&game, &input),
        names(&["Retire", "Drones", "Unload here"])
    );
    pick(&mut game, &mut input, "Drones");
    assert_eq!(heading(&game, &input).as_deref(), Some("DRONES"));
    tap(&mut game, &mut input, PadButton::East);
    tap(&mut game, &mut input, PadButton::East);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("COMMANDS"));
}
