use super::*;
use crate::layout::ArmyReach;
use aftermine_sim::UnitId;

/// Own units of every role on the left, one more Sentinel far to the
/// right, and an enemy Sentinel among the own army.
fn army_game() -> Game {
    let mut map = vec![".".repeat(64); 24];
    map[2].replace_range(2..3, "1");
    map[20].replace_range(58..59, "2");
    let own = [
        ("harvester", 6),
        ("excavator", 8),
        ("skyhook", 10),
        ("sentinel", 12),
        ("mantle", 14),
        ("kestrel", 16),
        ("surveyor", 18),
        ("sapper", 20),
        ("roc", 22),
    ];
    let mut units: Vec<_> = own
        .iter()
        .map(|(kind, x)| serde_json::json!({"player": 0, "kind": kind, "x": x, "y": 10}))
        .collect();
    units.push(serde_json::json!({"player": 0, "kind": "sentinel", "x": 56, "y": 10}));
    units.push(serde_json::json!({"player": 1, "kind": "sentinel", "x": 13, "y": 12}));
    let scenario = aftermine_sim::Scenario::from_json(
        &serde_json::json!({
            "name": "Army selection", "players": [
                {"name": "You", "scrap": 1000, "bot": false},
                {"name": "Opponent", "scrap": 0, "bot": true}
            ],
            "map": map,
            "units": units,
            "buildings": []
        })
        .to_string(),
    )
    .unwrap();
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).unwrap();
    // The view spans x 4 to 24 and leaves the far Sentinel out.
    game.presentation.camera.zoom = 64.0;
    game.presentation.camera.center = vec2(14.0, 10.0);
    game
}

/// The viewer's units of `kinds`, in id order.
fn own(game: &Game, kinds: &[UnitKind]) -> Vec<UnitId> {
    let mut ids: Vec<_> = game
        .state
        .units()
        .iter()
        .filter(|u| u.player == game.presentation.human && kinds.contains(&u.kind))
        .map(|u| u.id)
        .collect();
    ids.sort_unstable();
    ids
}

const ARMY: [UnitKind; 4] = [
    UnitKind::Sentinel,
    UnitKind::Mantle,
    UnitKind::Sapper,
    UnitKind::Roc,
];

fn ctrl_chord(key: Key) -> [RawEvent; 4] {
    [
        key_down(Key::Ctrl),
        key_down(key),
        key_up(key),
        key_up(Key::Ctrl),
    ]
}

fn denied(game: &Game) -> bool {
    game.presentation
        .sounds_pending
        .contains(&(crate::game::SoundKind::Denied, None))
}

#[test]
fn the_army_is_every_unit_that_fights_or_shields_but_no_drone() {
    let game = army_game();
    let all = crate::input::army_units(&game.view(), false);
    assert_eq!(all, own(&game, &ARMY));
    let sentinel = game
        .state
        .units()
        .iter()
        .find(|u| u.kind == UnitKind::Sentinel)
        .unwrap();
    let mut drone = sentinel.clone();
    drone.kind = UnitKind::Drone;
    drone.host = Some(sentinel.id);
    assert!(
        !super::super::select::in_army(&drone),
        "a drone flies for its host"
    );
}

#[test]
fn ctrl_a_selects_the_army_and_ctrl_s_the_part_in_view() {
    let mut game = army_game();
    let mut input = InputState::new();
    let harvester = own(&game, &[UnitKind::Harvester]);
    game.presentation.selection.units.clone_from(&harvester);
    let center = game.presentation.camera.center;
    apply_events(&mut game, &mut input, &ctrl_chord(Key::A));
    assert_eq!(game.presentation.selection.units, own(&game, &ARMY));
    assert!(game.presentation.selection.buildings.is_empty());
    assert_eq!(
        game.presentation.camera.center, center,
        "Ctrl+A is not a held pan"
    );

    apply_events(&mut game, &mut input, &ctrl_chord(Key::S));
    let far = game
        .state
        .units()
        .iter()
        .find(|u| u.pos.x.to_num::<f32>() > 50.0)
        .unwrap()
        .id;
    let mut in_view = own(&game, &ARMY);
    in_view.retain(|id| *id != far);
    assert_eq!(game.presentation.selection.units, in_view);
    assert!(game.pending.is_empty(), "selection issues no command");
}

#[test]
fn with_no_army_in_view_the_press_is_refused_and_the_selection_stays() {
    let mut game = army_game();
    let mut input = InputState::new();
    game.presentation.camera.center = vec2(40.0, 4.0);
    let harvester = own(&game, &[UnitKind::Harvester]);
    game.presentation.selection.units.clone_from(&harvester);
    apply_events(&mut game, &mut input, &ctrl_chord(Key::S));
    assert_eq!(game.presentation.selection.units, harvester);
    assert!(denied(&game));
}

#[test]
fn selecting_the_army_closes_the_build_palette() {
    let mut game = army_game();
    let mut input = InputState::new();
    let harvester = own(&game, &[UnitKind::Harvester]);
    game.presentation.selection.units.clone_from(&harvester);
    dispatch_action(&mut game, &mut input, Action::ToggleBuildPalette);
    assert!(input.build_menu, "premise: the palette is open");
    dispatch_action(&mut game, &mut input, Action::SelectArmy);
    assert!(!input.build_menu);
}

/// A layout with the strip's two buttons where the HUD would publish them.
fn publish_strip(game: &Game) -> crate::layout::ArmyStrip {
    let minimap = crate::render::minimap_rect_scaled(64, 24, vec2(1280.0, 800.0), 1.0);
    let strip = crate::layout::army_strip(1.0, false, minimap).unwrap();
    let mut layout = bare_layout(f32::INFINITY, 0.0);
    layout.minimap = minimap;
    layout.army_strip = strip.plate;
    layout.army_buttons = [
        Some((strip.all, ArmyReach::All)),
        Some((strip.in_view, ArmyReach::InView)),
    ];
    game.presentation.layout.set(layout);
    strip
}

#[test]
fn the_strip_buttons_answer_clicks_and_never_reach_the_world() {
    let mut game = army_game();
    let mut input = InputState::new();
    let strip = publish_strip(&game);
    let p = strip.all.center();
    apply_events(&mut game, &mut input, &click(p.x, p.y));
    assert_eq!(game.presentation.selection.units, own(&game, &ARMY));
    let p = strip.in_view.center();
    apply_events(&mut game, &mut input, &click(p.x, p.y));
    assert_eq!(
        game.presentation.selection.units.len(),
        ARMY.len(),
        "the far Sentinel is out of view"
    );
    // The plate's margin is chrome: a near miss selects nothing under it.
    let margin = vec2(strip.plate.x + 2.0, strip.plate.center().y);
    apply_events(&mut game, &mut input, &click(margin.x, margin.y));
    assert_eq!(game.presentation.selection.units.len(), ARMY.len());
    assert!(game.pending.is_empty());
}

#[test]
fn a_tap_presses_only_the_button_its_finger_landed_on() {
    let mut game = army_game();
    let mut input = InputState::new();
    let strip = publish_strip(&game);
    tap(&mut game, &mut input, strip.in_view.center());
    assert_eq!(game.presentation.selection.units.len(), ARMY.len());

    // A still finger that lands on the wide button's edge and lifts across
    // the gap on the square presses neither.
    game.presentation.selection.units.clear();
    let y = strip.all.center().y;
    input.now += 1.0;
    apply_events(
        &mut game,
        &mut input,
        &[touch_down(1, vec2(strip.all.x + strip.all.w - 1.0, y))],
    );
    input.now += 0.05;
    apply_events(
        &mut game,
        &mut input,
        &[touch_up(1, vec2(strip.in_view.x + 1.0, y))],
    );
    assert!(game.presentation.selection.units.is_empty());
}
