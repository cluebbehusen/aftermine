use crate::game::Game;
use aftermine_sim::{BuildingId, Command, Scenario, UnitKind};

fn train(game: &mut Game) {
    game.issue(Command::Train {
        building: BuildingId(0),
        kind: UnitKind::Harvester,
    });
}

#[test]
fn hud_reads_share_one_projection_until_the_staged_batch_changes() {
    let mut game = Game::new(Scenario::skirmish()).unwrap();
    let human = game.presentation.human;
    let bank = game.state.player(human).scrap;
    let cost = UnitKind::Harvester.stats().cost;

    train(&mut game);
    let (kept, scrap) = game
        .view()
        .with_staged(|view| (view.units().as_ptr(), view.scrap(human)));
    assert_eq!(scrap, Some(bank - cost));
    let again = game.view().with_staged(|view| view.units().as_ptr());
    assert_eq!(again, kept, "a second read reuses the projection");

    train(&mut game);
    let (fresh, scrap) = game
        .view()
        .with_staged(|view| (view.units().as_ptr(), view.scrap(human)));
    assert_ne!(fresh, kept, "another staged command projects again");
    assert_eq!(scrap, Some(bank - 2 * cost));

    game.do_tick();
    let (drained, scrap) = game
        .view()
        .with_staged(|view| (view.units().as_ptr(), view.scrap(human)));
    assert_eq!(
        drained,
        game.state.units().as_ptr(),
        "nothing staged, no copy"
    );
    assert_eq!(scrap, Some(game.state.player(human).scrap));
}
