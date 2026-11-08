use super::super::tests::pad_scene::{FRAME, down, left_stick, pad_input, scene, units_of, up};
use super::super::{ArmedMode, Group, InputState, apply_events, track_hands, update_held};
use super::current;
use crate::action::BindingMap;
use crate::game::Game;
use crate::panel::CardIcon;
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
    assert!(
        input.ring.is_some(),
        "a mouse that only moves leaves it open"
    );
    feed(
        &mut game,
        &mut input,
        &[RawEvent::MouseDown {
            button: aftermine_protocol::MouseButton::Left,
            x: 5.0,
            y: 5.0,
        }],
    );
    assert!(input.ring.is_none(), "a click closes it");
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

#[test]
fn a_flick_and_a_press_in_one_frame_pick_where_the_flick_points() {
    let (mut game, mut input) = sentinels();
    aim(&mut game, &mut input, "Patrol");
    // The frame records its stick move before its buttons: A must still
    // pick Hunt, where the stick now points, not the Patrol it left.
    let hunt = crate::radial::slice_direction(1, 6);
    feed(
        &mut game,
        &mut input,
        &[
            left_stick(hunt.x, hunt.y),
            down(PadButton::South),
            up(PadButton::South),
        ],
    );
    assert!(input.ring.is_none());
    assert_eq!(input.armed_mode(), Some(ArmedMode::Hunt));
}

#[test]
fn grouped_magazines_count_staged_rounds_and_x_cancels_one() {
    let mut game = scene(
        &[],
        &[
            (0, BuildingKind::Lattice, 26, 18),
            (0, BuildingKind::Lattice, 32, 18),
        ],
    );
    let mut input = pad_input(&mut game);
    game.presentation.selection.buildings = game
        .state
        .buildings()
        .iter()
        .filter(|b| b.kind == BuildingKind::Lattice)
        .map(|b| b.id)
        .collect();
    frame(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::North);
    frame(&mut game, &mut input);
    // Two selected magazines have no dock to count from, and no tick has
    // run: the count still climbs with every order staged.
    pick(&mut game, &mut input, "Order");
    tap(&mut game, &mut input, PadButton::South);
    frame(&mut game, &mut input);
    assert!(
        input.ring.is_some(),
        "ordering leaves the ring open for more"
    );
    assert_eq!(count(&game, &input, "Order"), 2);
    tap(&mut game, &mut input, PadButton::West);
    frame(&mut game, &mut input);
    assert_eq!(count(&game, &input, "Order"), 1);
}

#[test]
fn y_waits_while_a_held_a_lays_a_row() {
    let (mut game, mut input) = harvester();
    pick(&mut game, &mut input, "Build");
    pick(&mut game, &mut input, "Defense");
    pick(&mut game, &mut input, "Barricade");
    assert_eq!(
        input.armed_mode(),
        Some(ArmedMode::Build(BuildingKind::Barricade))
    );
    feed(&mut game, &mut input, &[down(PadButton::South)]);
    assert!(input.stroke_by().is_some(), "a held A lays a row");
    tap(&mut game, &mut input, PadButton::North);
    assert!(input.ring.is_none());
    assert!(denied(&game));
}

/// Opens the group ring with D-pad right, as a frame lays it out.
fn open_groups(game: &mut Game, input: &mut InputState) {
    frame(game, input);
    tap(game, input, PadButton::DpadRight);
    frame(game, input);
}

/// Group `number`'s slice in the open ring.
fn slot(game: &Game, input: &InputState, number: usize) -> crate::radial::Slice {
    current(&game.view(), input)
        .expect("a ring is open")
        .slices
        .into_iter()
        .nth(number - 1)
        .expect("the ring holds the slot")
}

/// Two Sentinels and a Scuttler of seat 0, and a Sentinel of seat 1.
fn squads() -> Game {
    scene(
        &[
            (0, UnitKind::Sentinel, 30, 20),
            (0, UnitKind::Sentinel, 31, 20),
            (0, UnitKind::Scuttler, 26, 24),
            (1, UnitKind::Sentinel, 40, 30),
        ],
        &[],
    )
}

#[test]
fn dpad_right_opens_the_five_groups_with_nothing_selected() {
    let mut game = squads();
    let mut input = pad_input(&mut game);
    open_groups(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("GROUPS"));
    assert_eq!(
        titles(&game, &input),
        names(&["Group 1", "Group 2", "Group 3", "Group 4", "Group 5"])
    );
    for number in 1..=5 {
        let empty = slot(&game, &input, number);
        assert!(!empty.card.enabled, "an empty group draws faint");
        assert_eq!(empty.count, 0);
        assert_eq!(empty.card.desc, names(&["Empty."]));
    }
    tap(&mut game, &mut input, PadButton::DpadRight);
    assert!(input.ring.is_none(), "its own button closes it");
}

#[test]
fn x_saves_the_selection_to_the_pointed_group_and_closes_the_ring() {
    let mut game = squads();
    let mut input = pad_input(&mut game);
    let sentinels = units_of(&game, 0, UnitKind::Sentinel);
    let scuttler = units_of(&game, 0, UnitKind::Scuttler);
    game.presentation.selection.units = [sentinels.clone(), scuttler].concat();
    open_groups(&mut game, &mut input);
    aim(&mut game, &mut input, "Group 2");
    tap(&mut game, &mut input, PadButton::West);
    assert!(input.ring.is_none());
    assert!(input.groups[1].same_members(&Group::Units(game.presentation.selection.units.clone())));
    open_groups(&mut game, &mut input);
    // The slice wears the kind most members share, how many it holds,
    // and its members by kind, the most numerous first.
    let two = slot(&game, &input, 2);
    assert!(two.card.enabled);
    assert_eq!(two.count, 3);
    assert_eq!(two.card.icon, CardIcon::Unit(UnitKind::Sentinel));
    assert_eq!(two.card.desc, names(&["2 Sentinels, 1 Scuttler."]));
    assert_eq!(slot(&game, &input, 1).count, 0);
}

#[test]
fn a_recalls_the_pointed_group_and_an_empty_one_refuses() {
    let mut game = squads();
    let mut input = pad_input(&mut game);
    let sentinels = units_of(&game, 0, UnitKind::Sentinel);
    let scuttler = units_of(&game, 0, UnitKind::Scuttler);
    input.groups[0] = Group::Units(sentinels.clone());
    game.presentation.selection.units.clone_from(&scuttler);
    open_groups(&mut game, &mut input);
    aim(&mut game, &mut input, "Group 3");
    tap(&mut game, &mut input, PadButton::South);
    assert!(denied(&game));
    assert!(
        input.ring.is_some(),
        "a refused recall leaves the ring open"
    );
    assert_eq!(game.presentation.selection.units, scuttler);
    pick(&mut game, &mut input, "Group 1");
    assert!(input.ring.is_none());
    assert_eq!(game.presentation.selection.units, sentinels);
}

#[test]
fn x_keeps_only_own_units_and_with_nothing_selected_empties_the_group() {
    let mut game = squads();
    let mut input = pad_input(&mut game);
    let own = units_of(&game, 0, UnitKind::Sentinel);
    let enemy = units_of(&game, 1, UnitKind::Sentinel);
    game.presentation.selection.units = [own.clone(), enemy].concat();
    open_groups(&mut game, &mut input);
    aim(&mut game, &mut input, "Group 1");
    tap(&mut game, &mut input, PadButton::West);
    assert_eq!(input.groups[0], Group::Units(own));
    assert!(toasted(&game, "Only your own units join a control group"));
    // As Ctrl and a number does, X with nothing selected clears the group.
    game.presentation.selection.units.clear();
    open_groups(&mut game, &mut input);
    aim(&mut game, &mut input, "Group 1");
    tap(&mut game, &mut input, PadButton::West);
    assert!(input.groups[0].is_empty());
}

#[test]
fn a_group_of_structures_wears_its_lead_and_a_lost_member_drops_out() {
    let mut game = scene(
        &[(0, UnitKind::Sentinel, 30, 20)],
        &[
            (0, BuildingKind::Foundry, 30, 18),
            (0, BuildingKind::Turret, 24, 24),
        ],
    );
    let mut input = pad_input(&mut game);
    let structures: Vec<_> = game.state.buildings().iter().map(|b| b.id).collect();
    input.groups[0] = Group::Buildings(structures);
    // An id that names nothing any more, as a destroyed unit's does.
    let sentinel = units_of(&game, 0, UnitKind::Sentinel)[0];
    input.groups[1] = Group::Units(vec![sentinel, aftermine_sim::UnitId(9_999)]);
    open_groups(&mut game, &mut input);
    let one = slot(&game, &input, 1);
    assert_eq!(one.count, 2);
    assert!(matches!(
        one.card.icon,
        CardIcon::Building(BuildingKind::Foundry, _)
    ));
    assert_eq!(one.card.desc, names(&["1 Foundry, 1 Turret."]));
    let two = slot(&game, &input, 2);
    assert_eq!(two.count, 1);
    assert_eq!(two.card.desc, names(&["1 Sentinel."]));
}

#[test]
fn each_rings_button_opens_it_in_the_others_place_and_closes_its_own() {
    let (mut game, mut input) = sentinels();
    assert_eq!(heading(&game, &input).as_deref(), Some("COMMANDS"));
    tap(&mut game, &mut input, PadButton::DpadRight);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("GROUPS"));
    tap(&mut game, &mut input, PadButton::North);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("COMMANDS"));
    tap(&mut game, &mut input, PadButton::North);
    assert!(input.ring.is_none());
    open_groups(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::East);
    assert!(input.ring.is_none(), "B closes the group ring");
    // With no command ring to offer, Y over the group ring keeps it.
    game.presentation.selection.units.clear();
    open_groups(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::North);
    frame(&mut game, &mut input);
    assert!(denied(&game));
    assert_eq!(heading(&game, &input).as_deref(), Some("GROUPS"));
}

#[test]
fn leaving_the_build_rings_for_the_groups_closes_their_palette() {
    let (mut game, mut input) = harvester();
    pick(&mut game, &mut input, "Build");
    assert!(input.build_menu);
    tap(&mut game, &mut input, PadButton::DpadRight);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("GROUPS"));
    assert!(!input.build_menu);
    // An armed placement stays through the swap, as through Y.
    tap(&mut game, &mut input, PadButton::North);
    frame(&mut game, &mut input);
    pick(&mut game, &mut input, "Build");
    pick(&mut game, &mut input, "Defense");
    pick(&mut game, &mut input, "Turret");
    tap(&mut game, &mut input, PadButton::North);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("DEFENSE"));
    tap(&mut game, &mut input, PadButton::DpadRight);
    frame(&mut game, &mut input);
    assert_eq!(heading(&game, &input).as_deref(), Some("GROUPS"));
    assert_eq!(
        input.armed_mode(),
        Some(ArmedMode::Build(BuildingKind::Turret))
    );
}

#[test]
fn a_spectator_has_no_group_ring() {
    let mut game = squads();
    let mut input = pad_input(&mut game);
    game.presentation.stance = crate::game::Stance::Spectator;
    open_groups(&mut game, &mut input);
    assert!(input.ring.is_none());
    assert!(denied(&game));
}

/// One Lattice, selected, with its ring open.
fn lattice() -> (Game, InputState) {
    let mut game = scene(&[], &[(0, BuildingKind::Lattice, 30, 18)]);
    let mut input = pad_input(&mut game);
    game.presentation.selection.buildings = game.state.buildings().iter().map(|b| b.id).collect();
    frame(&mut game, &mut input);
    tap(&mut game, &mut input, PadButton::North);
    frame(&mut game, &mut input);
    (game, input)
}

#[test]
fn x_on_a_slice_with_nothing_to_cancel_does_what_a_right_click_does() {
    let (mut game, mut input) = harvester();
    let worker = units_of(&game, 0, UnitKind::Harvester);
    aim(&mut game, &mut input, "Repair");
    tap(&mut game, &mut input, PadButton::West);
    assert!(
        game.pending.iter().any(|command| matches!(
            &command.command,
            Command::SetIdleWeld { units, .. } if *units == worker
        )),
        "X switches auto-repair, as a right-click on the Repair card does"
    );
    // Order with nothing on order fills every empty slot, as its right-click
    // does; with rounds on order, X cancels one first.
    let (mut game, mut input) = lattice();
    aim(&mut game, &mut input, "Order");
    tap(&mut game, &mut input, PadButton::West);
    frame(&mut game, &mut input);
    assert_eq!(count(&game, &input, "Order"), 2);
    tap(&mut game, &mut input, PadButton::West);
    frame(&mut game, &mut input);
    assert_eq!(count(&game, &input, "Order"), 1);
}

#[test]
fn a_held_left_trigger_turns_order_into_fill_under_the_stick() {
    let (mut game, mut input) = lattice();
    aim(&mut game, &mut input, "Order");
    feed(&mut game, &mut input, &[down(PadButton::LeftTrigger)]);
    frame(&mut game, &mut input);
    assert_eq!(
        pointed(&game, &input).as_deref(),
        Some("Fill 2"),
        "the stick still points at the slice LT turned into Fill"
    );
    tap(&mut game, &mut input, PadButton::South);
    feed(&mut game, &mut input, &[up(PadButton::LeftTrigger)]);
    frame(&mut game, &mut input);
    assert_eq!(count(&game, &input, "Order"), 2, "one A filled both slots");
}

/// A sandbox holding one own `unit` or one built own `building`, with a
/// derelict Extractor frame at (40, 10) where an Extractor stands.
fn lone(unit: Option<UnitKind>, building: Option<BuildingKind>) -> Game {
    let mut map = vec![".".repeat(64); 40];
    map[10].replace_range(40..41, "E");
    let anchor = if building == Some(BuildingKind::Extractor) {
        (40, 10)
    } else {
        (30, 18)
    };
    let scenario = aftermine_sim::Scenario::from_json(
        &serde_json::json!({
            "name": "Lone kind",
            "mode": "sandbox",
            "players": [{"name": "You", "scrap": 1000, "bot": false}],
            "map": map,
            "units": unit
                .map(|kind| vec![serde_json::json!({"player": 0, "kind": kind, "x": 30, "y": 24})])
                .unwrap_or_default(),
            "buildings": building
                .map(|kind| vec![serde_json::json!({"player": 0, "kind": kind, "x": anchor.0, "y": anchor.1})])
                .unwrap_or_default(),
        })
        .to_string(),
    )
    .expect("the scene parses");
    Game::with_viewport(scenario, macroquad::prelude::vec2(1280.0, 800.0))
        .unwrap_or_else(|error| panic!("{unit:?} {building:?} stands alone: {error:#}"))
}

#[test]
fn a_ring_for_one_kind_never_needs_more() {
    use crate::radial::{Branch, MAX_SLICES, Pick};
    let scenes = UnitKind::ALL
        .iter()
        .map(|kind| (format!("{kind:?}"), lone(Some(*kind), None)))
        .chain(
            BuildingKind::ALL
                .iter()
                // A scenario cannot start with a cradle: one exists only
                // while an experimental assembles.
                .filter(|kind| !matches!(kind, BuildingKind::MawCradle | BuildingKind::RocCradle))
                .map(|kind| (format!("{kind:?}"), lone(None, Some(*kind)))),
        );
    for (name, mut game) in scenes {
        let mut input = pad_input(&mut game);
        let selection = &mut game.presentation.selection;
        selection.units = game.state.units().iter().map(|u| u.id).collect();
        selection.buildings = game.state.buildings().iter().map(|b| b.id).collect();
        frame(&mut game, &mut input);
        tap(&mut game, &mut input, PadButton::North);
        frame(&mut game, &mut input);
        let Some(ring) = current(&game.view(), &input) else {
            continue;
        };
        let titles: Vec<&str> = ring
            .slices
            .iter()
            .map(|slice| slice.card.title.as_str())
            .collect();
        assert!(
            ring.slices.len() <= MAX_SLICES
                && ring
                    .slices
                    .iter()
                    .all(|slice| slice.pick != Pick::Open(Branch::More)),
            "a lone {name} spills into More: {titles:?}"
        );
    }
}
