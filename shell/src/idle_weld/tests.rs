use super::*;
use crate::game::Game;
use macroquad::prelude::vec2;
use oxide_sim::{PlayerCommand, PlayerId, Scenario};

fn game() -> Game {
    Game::with_viewport(Scenario::skirmish(), vec2(1280.0, 800.0)).expect("skirmish builds")
}

fn workers(game: &Game, player: PlayerId) -> Vec<UnitId> {
    game.state
        .units()
        .iter()
        .filter(|u| u.player == player && u.worker.is_some())
        .map(|u| u.id)
        .collect()
}

fn title(game: &Game) -> Option<String> {
    SelectedWorkers::inspect(&game.view())
        .card(&BindingMap::classic())
        .map(|card| card.title)
}

fn switch(game: &Game) -> Option<(Vec<UnitId>, bool)> {
    match SelectedWorkers::inspect(&game.view()).toggle()? {
        Command::SetIdleWeld { units, enabled } => Some((units, enabled)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_card_reads_on_only_while_every_selected_worker_welds_idle() {
    let mut game = game();
    let human = game.presentation.human;
    let own = workers(&game, human);
    assert!(own.len() >= 2, "premise: the skirmish starts with a crew");
    game.presentation.selection.units = own.clone();
    assert_eq!(title(&game).as_deref(), Some("Auto-weld: on"));
    assert_eq!(switch(&game), Some((own.clone(), false)));

    game.state.tick(&[PlayerCommand {
        player: human,
        command: Command::SetIdleWeld {
            units: vec![own[0]],
            enabled: false,
        },
    }]);
    assert_eq!(
        title(&game).as_deref(),
        Some("Auto-weld: off"),
        "one switched-off worker reads the mixed selection as off"
    );
    assert_eq!(
        switch(&game),
        Some((own, true)),
        "and the card turns all on"
    );
}

#[test]
fn a_staged_switch_shows_before_the_tick_runs() {
    let mut game = game();
    let own = workers(&game, game.presentation.human);
    game.presentation.selection.units = vec![own[0]];
    let command = SelectedWorkers::inspect(&game.view()).toggle().unwrap();
    game.issue(command);
    assert_eq!(title(&game).as_deref(), Some("Auto-weld: off"));
    assert_eq!(
        switch(&game),
        Some((vec![own[0]], true)),
        "pressing again undoes it"
    );
}

#[test]
fn only_the_viewers_own_workers_take_the_switch() {
    let mut game = game();
    let human = game.presentation.human;
    let own = workers(&game, human)[0];
    let foreign = workers(&game, PlayerId(1 - human.0))[0];
    game.presentation.selection.units = vec![own, foreign];
    assert_eq!(switch(&game), Some((vec![own], false)));
    game.presentation.selection.units = vec![foreign];
    assert_eq!(title(&game), None);
    assert_eq!(switch(&game), None);
}
