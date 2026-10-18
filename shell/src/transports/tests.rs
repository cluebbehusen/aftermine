use super::*;
use crate::action::BindingMap;
use crate::game::Game;
use crate::input::InputState;
use macroquad::prelude::vec2;
use oxide_protocol::{Key, RawEvent};
use oxide_sim::{PlayerCommand, Scenario, UnitKind};

/// A loaded Roc, a loaded Skyhook, an empty Skyhook, a Lancer and a
/// Harvester of the viewer's on open ground.
struct Fleet {
    game: Game,
    roc: UnitId,
    loaded: UnitId,
    empty: UnitId,
    lancer: UnitId,
    harvester: UnitId,
}

fn fleet() -> Fleet {
    let (width, height) = (26, 12);
    let mut map: Vec<String> = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    if x == 0 || y == 0 || x == width - 1 || y == height - 1 {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect();
    map[2].replace_range(2..3, "1");
    map[2].replace_range(23..24, "2");
    let unit = |player: u8, kind: &str, x: i32, y: i32| serde_json::json!({"player": player, "kind": kind, "x": x, "y": y});
    let scenario = Scenario::from_json(
        &serde_json::json!({
            "name": "group unload",
            "players": [
                {"name": "F", "scrap": 100, "bot": false},
                {"name": "C", "scrap": 100, "bot": true}
            ],
            "map": map,
            "units": [
                unit(0, "roc", 5, 5),
                unit(0, "skyhook", 12, 5),
                unit(0, "skyhook", 17, 5),
                unit(0, "sentinel", 5, 8),
                unit(0, "sentinel", 12, 7),
                unit(0, "lancer", 9, 9),
                unit(0, "harvester", 20, 9),
                unit(1, "skyhook", 23, 2)
            ]
        })
        .to_string(),
    )
    .expect("the fleet scenario parses");
    let mut game = Game::with_viewport(scenario, vec2(1280.0, 800.0)).expect("the fleet builds");
    let human = game.presentation.human;
    let own = |game: &Game, kind| -> Vec<UnitId> {
        game.state
            .units()
            .iter()
            .filter(|u| u.player == human && u.kind == kind)
            .map(|u| u.id)
            .collect()
    };
    let roc = own(&game, UnitKind::Roc)[0];
    let [loaded, empty] = own(&game, UnitKind::Skyhook)[..] else {
        panic!("the fleet fields two Skyhooks");
    };
    let sentinels = own(&game, UnitKind::Sentinel);
    let (lancer, harvester) = (
        own(&game, UnitKind::Lancer)[0],
        own(&game, UnitKind::Harvester)[0],
    );
    let load = |units: Vec<UnitId>, transport| PlayerCommand {
        player: human,
        command: Command::Load {
            units,
            transport,
            queue: false,
        },
    };
    game.state.tick(&[
        load(vec![sentinels[0]], roc),
        load(vec![sentinels[1]], loaded),
    ]);
    let aboard = |game: &Game, id| !game.state.unit(id).expect("alive").cargo.is_empty();
    for _ in 0..200 {
        if aboard(&game, roc) && aboard(&game, loaded) {
            break;
        }
        game.state.tick(&[]);
    }
    assert!(
        aboard(&game, roc) && aboard(&game, loaded),
        "premise: both riders board"
    );
    Fleet {
        game,
        roc,
        loaded,
        empty,
        lancer,
        harvester,
    }
}

fn here(game: &Game, id: UnitId) -> TilePos {
    game.state.unit(id).expect("alive").tile()
}

fn unload(transport: UnitId, at: TilePos, queue: bool) -> Command {
    Command::Unload {
        transport,
        at,
        queue,
    }
}

fn press(game: &mut Game, input: &mut InputState, events: &[RawEvent]) {
    crate::input::apply_events(game, input, &BindingMap::classic(), events, |_, _| {});
}

fn staged(game: &Game) -> Vec<&Command> {
    game.pending
        .iter()
        .map(|pending| &pending.command)
        .collect()
}

fn unload_card(game: &Game) -> crate::panel::Card {
    crate::panel::build_for_palette(&game.view(), &BindingMap::classic(), false)
        .expect("a panel")
        .cards
        .into_iter()
        .find(|card| card.action == crate::panel::CardAction::Unload)
        .expect("an Unload card")
}

#[test]
fn u_drops_every_loaded_sling_in_a_mixed_selection_where_it_hovers() {
    let Fleet {
        mut game,
        roc,
        loaded,
        empty,
        lancer,
        ..
    } = fleet();
    let mut input = InputState::new();
    game.presentation.selection.units = vec![roc, loaded, empty, lancer];
    let card = unload_card(&game);
    assert!(card.enabled);
    assert_eq!(card.hotkey, "U");
    press(&mut game, &mut input, &[RawEvent::KeyDown { key: Key::U }]);
    assert_eq!(
        staged(&game),
        [
            &unload(roc, here(&game, roc), false),
            &unload(loaded, here(&game, loaded), false),
        ],
        "the empty Skyhook keeps its orders"
    );
}

#[test]
fn shift_u_queues_each_drop_where_its_route_ends() {
    let Fleet {
        mut game,
        roc,
        loaded,
        lancer,
        ..
    } = fleet();
    let mut input = InputState::new();
    let goal = TilePos::new(9, 3);
    game.issue(Command::Run {
        units: vec![roc],
        goal,
        queue: false,
    });
    game.issue(Command::Accompany {
        units: vec![roc],
        target: lancer,
        queue: true,
    });
    game.presentation.selection.units = vec![roc, loaded];
    press(
        &mut game,
        &mut input,
        &[
            RawEvent::KeyDown { key: Key::Shift },
            RawEvent::KeyDown { key: Key::U },
        ],
    );
    assert_eq!(
        staged(&game)[2..],
        [
            &unload(roc, goal, true),
            &unload(loaded, here(&game, loaded), true),
        ],
        "the Roc's staged run ends at its goal, which an escort does not move; \
         the idle Skyhook's route ends where it hovers"
    );
}

#[test]
fn shift_u_drops_a_patrolling_sling_now_instead_of_joining_its_circuit() {
    let Fleet {
        mut game,
        roc,
        loaded,
        ..
    } = fleet();
    let mut input = InputState::new();
    game.issue(Command::Patrol {
        units: vec![roc],
        waypoints: vec![TilePos::new(9, 3), TilePos::new(5, 3)],
    });
    game.presentation.selection.units = vec![roc, loaded];
    press(
        &mut game,
        &mut input,
        &[
            RawEvent::KeyDown { key: Key::Shift },
            RawEvent::KeyDown { key: Key::U },
        ],
    );
    assert_eq!(
        staged(&game)[1..],
        [
            &unload(roc, here(&game, roc), false),
            &unload(loaded, here(&game, loaded), true),
        ]
    );
    let commands: Vec<_> = game.pending.drain(..).collect();
    game.state.tick(&commands);
    for _ in 0..200 {
        if game.state.unit(roc).expect("alive").cargo.is_empty() {
            break;
        }
        game.state.tick(&[]);
    }
    let roc = game.state.unit(roc).expect("alive");
    assert!(roc.cargo.is_empty(), "premise: the Roc drops its rider");
    assert!(
        !roc.looping && roc.queue.is_empty(),
        "no drop rotates back into a patrol"
    );
}

#[test]
fn empty_slings_offer_a_refused_card_and_order_nothing() {
    let Fleet {
        mut game, empty, ..
    } = fleet();
    game.presentation.selection.units = vec![empty];
    let card = unload_card(&game);
    assert!(!card.enabled);
    assert_eq!(card.why.as_deref(), Some("nothing is aboard"));
    let view = game.view();
    let transports = SelectedTransports::inspect(&view);
    assert!(!transports.is_empty() && !transports.any_loaded());
    assert_eq!(transports.unload(&view, false), []);
}

#[test]
fn a_worker_in_the_selection_keeps_the_card_but_not_its_key() {
    let Fleet {
        mut game,
        roc,
        harvester,
        ..
    } = fleet();
    let mut input = InputState::new();
    game.presentation.selection.units = vec![roc, harvester];
    let card = unload_card(&game);
    assert!(card.enabled);
    assert_eq!(card.hotkey, "", "U returns cargo for a worker selection");
    press(&mut game, &mut input, &[RawEvent::KeyDown { key: Key::U }]);
    assert_eq!(staged(&game), [] as [&Command; 0]);
}

#[test]
fn only_the_viewers_own_transports_unload() {
    let Fleet { mut game, roc, .. } = fleet();
    let foreign = game
        .state
        .units()
        .iter()
        .find(|u| u.player != game.presentation.human)
        .expect("the fleet fields a foreign Skyhook")
        .id;
    game.presentation.selection.units = vec![roc, foreign];
    assert_eq!(
        SelectedTransports::inspect(&game.view()).unload(&game.view(), false),
        [unload(roc, here(&game, roc), false)]
    );
}
