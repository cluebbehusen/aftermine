use super::*;
use aftermine_sim::Scenario;
use aftermine_sim::scenario::UnitSpec;

fn tracker(seats: u8) -> FightTracker {
    FightTracker {
        teams: (0..seats).collect(),
        history: VecDeque::new(),
        open: Vec::new(),
        seats: vec![SeatFights::default(); usize::from(seats)],
    }
}

fn armed(id: u32, seat: u8, x: i32, y: i32) -> Armed {
    Armed {
        id: UnitId(id),
        seat,
        team: seat,
        tile: TilePos::new(x, y),
        worth: 100,
    }
}

fn death(seat: u8, x: i32, y: i32, alone: bool) -> Death {
    Death {
        seat,
        team: seat,
        tile: TilePos::new(x, y),
        price: 100,
        alone,
    }
}

#[test]
fn deaths_close_in_time_and_place_make_one_fight_and_a_death_on_its_own_none() {
    let mut fights = tracker(2);
    fights.step(10, vec![death(0, 5, 5, false)], None);
    fights.step(20, vec![death(1, 7, 5, false)], None);
    fights.step(30, vec![death(0, 40, 40, true)], None);
    let seats = fights.finish();
    assert_eq!(seats[0].fights, 1);
    assert_eq!(seats[1].fights, 1);
    assert_eq!(
        (
            seats[0].losses,
            seats[0].alone,
            seats[0].lost,
            seats[0].killed
        ),
        (1, 0, 100, 100)
    );
}

#[test]
fn a_death_past_the_gap_starts_another_fight() {
    let mut fights = tracker(2);
    fights.step(10, vec![death(0, 5, 5, false)], None);
    fights.step(20, vec![death(1, 6, 5, false)], None);
    fights.step(20 + GAP_TICKS + 1, vec![], None);
    fights.step(20 + GAP_TICKS + 2, vec![death(0, 5, 5, true)], None);
    fights.step(20 + GAP_TICKS + 3, vec![death(1, 6, 5, true)], None);
    let seats = fights.finish();
    assert_eq!(
        (seats[0].fights, seats[0].losses, seats[0].alone),
        (2, 2, 1)
    );
}

#[test]
fn arrivals_are_late_after_their_side_s_first_and_a_meeting_is_judged_when_both_first_stand_near() {
    // West's one Sentinel stands against three of East's before the first
    // death; its second comes eleven seconds later.
    let mut fights = tracker(2);
    fights.step(
        10,
        vec![],
        Some(vec![
            armed(1, 0, 5, 5),
            armed(10, 1, 6, 5),
            armed(11, 1, 6, 6),
            armed(12, 1, 7, 5),
        ]),
    );
    fights.step(100, vec![death(0, 5, 5, true)], None);
    fights.step(
        220,
        vec![],
        Some(vec![armed(2, 0, 5, 6), armed(10, 1, 6, 5)]),
    );
    fights.step(230, vec![death(1, 6, 5, false)], None);
    let seats = fights.finish();
    assert_eq!((seats[0].arrivals, seats[0].late), (2, 1));
    assert_eq!((seats[0].met, seats[0].met_short), (1, 1));
    assert_eq!((seats[1].arrivals, seats[1].late), (3, 0));
    assert_eq!((seats[1].met, seats[1].met_short), (1, 0));
    assert_eq!((seats[0].losses, seats[0].alone), (1, 1));
}

#[test]
fn units_far_from_the_fight_neither_take_part_nor_arrive() {
    let mut fights = tracker(3);
    fights.step(
        10,
        vec![],
        Some(vec![
            armed(1, 0, 5, 5),
            armed(2, 1, 6, 5),
            armed(3, 2, 40, 40),
        ]),
    );
    fights.step(20, vec![death(0, 5, 5, false), death(1, 6, 5, false)], None);
    let seats = fights.finish();
    assert_eq!(seats[2], SeatFights::default());
    assert_eq!(seats[0].arrivals, 1);
}

#[test]
fn pooled_ratios_weigh_each_seat_leg_and_widen_with_their_spread() {
    assert_eq!(pooled_ratio([(0, 0)].into_iter()), None);
    assert_eq!(
        pooled_ratio([(5, 10), (5, 10)].into_iter()),
        Some((0.5, [0.5, 0.5]))
    );
    let (ratio, [low, high]) = pooled_ratio([(0, 10), (10, 10)].into_iter()).unwrap();
    assert!((ratio - 0.5).abs() < 1e-9);
    assert!(low < 0.0 && high > 1.0, "{low} {high}");
    let (ratio, [low, high]) = pooled_ratio([(1, 10), (3, 30), (2, 20)].into_iter()).unwrap();
    assert!((ratio - 0.1).abs() < 1e-9 && (low - 0.1).abs() < 1e-9 && (high - 0.1).abs() < 1e-9);
}

#[test]
fn a_simulated_skirmish_is_one_fight_whose_losses_each_side_counts_the_other_killing() {
    let mut scenario = Scenario::skirmish();
    for player in &mut scenario.players {
        player.bot = false;
        player.bot_config = None;
    }
    for y in 2..5 {
        scenario.units.extend([
            UnitSpec {
                player: 0,
                kind: UnitKind::Sentinel,
                x: 24,
                y,
            },
            UnitSpec {
                player: 1,
                kind: UnitKind::Sentinel,
                x: 26,
                y,
            },
        ]);
    }
    let mut state = scenario.build().unwrap();
    let mut fights = FightTracker::new(&state);
    let mut died = [0_u32; 2];
    for _ in 0..1_200 {
        let report = state.tick(&[]);
        for event in &report.events {
            if let Event::UnitDied { player, kind, .. } = event
                && *kind == UnitKind::Sentinel
            {
                died[usize::from(player.0)] += 1;
            }
        }
        fights.observe(&state, &report);
    }
    assert!(died[0] + died[1] >= 2, "premise: the Sentinels fight");
    let seats = fights.finish();
    let price = u64::from(UnitKind::Sentinel.stats().cost);
    for seat in 0..2 {
        assert_eq!(seats[seat].fights, 1, "{seats:?}");
        assert_eq!(seats[seat].losses, died[seat]);
        assert_eq!(seats[seat].lost, price * u64::from(died[seat]));
        assert_eq!(seats[seat].killed, seats[1 - seat].lost);
        assert_eq!((seats[seat].arrivals, seats[seat].met), (3, 1));
    }
}
