use super::*;
use crate::game::Game;
use crate::soundtrack::{Scene, match_scene};
use aftermine_sim::{Command, PlayerCommand, Scenario};

fn surrender(game: &mut Game, seats: &[u8]) {
    for &seat in seats {
        game.stage(PlayerCommand {
            player: PlayerId(seat),
            command: Command::Surrender,
        });
    }
    game.present_ticks(1);
}

#[test]
fn a_seat_that_surrendered_shares_its_teams_win_and_its_music() {
    let scenario = Scenario::load("../scenarios/bridge-town.json").expect("shipped map");
    let mut game = Game::new(scenario).expect("game");
    assert_eq!(game.presentation.human, PlayerId(0));
    surrender(&mut game, &[0]);
    assert_eq!(
        Verdict::of(&game.state, PlayerId(0)),
        None,
        "a teammate plays on"
    );
    surrender(&mut game, &[1, 3]);
    assert!(game.state.player(PlayerId(0)).resigned);
    assert!(game.state.winners().contains(&PlayerId(0)));
    assert_eq!(
        Verdict::of(&game.state, PlayerId(0)),
        Some(Verdict::Victory)
    );
    assert_eq!(match_scene(&game.view(), false), Scene::Victory);
    assert_eq!(
        Verdict::of(&game.state, PlayerId(1)),
        Some(Verdict::Surrendered)
    );
}

#[test]
fn a_draw_plays_neither_ending() {
    let mut game = Game::new(Scenario::skirmish()).expect("game");
    surrender(&mut game, &[0, 1]);
    assert_eq!(
        Verdict::of(&game.state, game.presentation.human),
        Some(Verdict::Draw)
    );
    assert_eq!(match_scene(&game.view(), false), Scene::Result);
}

#[test]
fn a_seat_on_the_losing_side_hears_the_defeat_music() {
    let mut game = Game::new(Scenario::skirmish()).expect("game");
    surrender(&mut game, &[0]);
    assert_eq!(
        Verdict::of(&game.state, PlayerId(0)),
        Some(Verdict::Surrendered)
    );
    assert_eq!(match_scene(&game.view(), false), Scene::Defeat);
    assert_eq!(
        Verdict::of(&game.state, PlayerId(1)),
        Some(Verdict::Victory)
    );
}
