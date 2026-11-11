use super::*;
use crate::game::{Game, SoundKind};
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

fn stingers(game: &Game) -> Vec<SoundKind> {
    game.presentation
        .sounds_pending
        .iter()
        .map(|(kind, _)| *kind)
        .filter(|kind| matches!(kind, SoundKind::Victory | SoundKind::Defeat))
        .collect()
}

#[test]
fn a_seat_that_surrendered_shares_its_teams_win_in_stinger_and_score() {
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
    assert_eq!(stingers(&game), [SoundKind::Victory]);
    assert_eq!(match_scene(&game.view(), false), Scene::Victory);
    assert_eq!(
        Verdict::of(&game.state, PlayerId(1)),
        Some(Verdict::Surrendered)
    );
}

#[test]
fn a_draw_plays_no_defeat() {
    let mut game = Game::new(Scenario::skirmish()).expect("game");
    surrender(&mut game, &[0, 1]);
    assert_eq!(
        Verdict::of(&game.state, game.presentation.human),
        Some(Verdict::Draw)
    );
    assert_eq!(stingers(&game), []);
    assert_eq!(match_scene(&game.view(), false), Scene::Result);
}

#[test]
fn a_seat_on_the_losing_side_hears_defeat() {
    let mut game = Game::new(Scenario::skirmish()).expect("game");
    surrender(&mut game, &[0]);
    assert_eq!(
        Verdict::of(&game.state, PlayerId(0)),
        Some(Verdict::Surrendered)
    );
    assert_eq!(stingers(&game), [SoundKind::Defeat]);
    assert_eq!(match_scene(&game.view(), false), Scene::Defeat);
    assert_eq!(
        Verdict::of(&game.state, PlayerId(1)),
        Some(Verdict::Victory)
    );
}
