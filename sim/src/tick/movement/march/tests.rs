use super::*;

fn walker(slot: usize, left: i32, top: &str) -> Walker {
    Walker {
        slot,
        left: Fx::from_num(left),
        top: top.parse().expect("a decimal speed"),
    }
}

fn caps(walkers: &[Walker]) -> Vec<(usize, Fx)> {
    pace(walkers).collect()
}

const SENTINEL: &str = "0.11";
const BOMBARD: &str = "0.06";

#[test]
fn members_side_by_side_keep_the_slowest_pace() {
    let walkers = [walker(0, 20, SENTINEL), walker(1, 20, BOMBARD)];
    assert_eq!(caps(&walkers), [(0, Fx::lit("0.06"))]);
}

#[test]
fn a_member_ahead_keeps_the_pace_rather_than_waiting() {
    let walkers = [walker(0, 4, SENTINEL), walker(1, 20, BOMBARD)];
    assert_eq!(caps(&walkers), [(0, Fx::lit("0.06"))]);
}

#[test]
fn a_member_behind_hurries_to_arrive_with_the_last() {
    let walkers = [walker(0, 28, SENTINEL), walker(1, 20, BOMBARD)];
    let [(slot, cap)] = caps(&walkers)[..] else {
        panic!("only the Sentinel is capped");
    };
    assert_eq!(slot, 0);
    assert_eq!(cap, Fx::from_num(28) * Fx::lit("0.06") / Fx::from_num(20));
    assert!(cap > Fx::lit("0.06") && cap < Fx::lit("0.11"));
}

#[test]
fn a_member_too_far_behind_to_catch_up_drives_flat_out() {
    let walkers = [walker(0, 60, SENTINEL), walker(1, 20, BOMBARD)];
    assert_eq!(caps(&walkers), [(0, Fx::lit("0.11"))]);
}

#[test]
fn a_lone_or_even_march_is_left_alone() {
    assert!(caps(&[walker(0, 20, SENTINEL)]).is_empty());
    assert!(caps(&[walker(0, 20, SENTINEL), walker(1, 9, SENTINEL)]).is_empty());
    assert!(caps(&[]).is_empty());
}

#[test]
fn a_march_with_nowhere_left_to_go_keeps_the_pace() {
    let walkers = [walker(0, 0, SENTINEL), walker(1, 0, BOMBARD)];
    assert_eq!(caps(&walkers), [(0, Fx::lit("0.06"))]);
}

#[test]
fn route_length_counts_straight_and_diagonal_steps() {
    use chassis::grid::TilePos;
    let path = PathFollow {
        final_point: None,
        goal: TilePos::new(3, 1),
        waypoints: vec![
            TilePos::new(1, 0),
            TilePos::new(2, 1),
            TilePos::new(3, 1),
            TilePos::new(3, 1),
            TilePos::new(6, 5),
        ],
        next: 0,
        clearing: false,
    };
    let mut state = crate::Scenario::skirmish()
        .build()
        .expect("skirmish builds");
    let unit = state.units.first_mut().expect("skirmish has units");
    unit.pos = TilePos::new(0, 0).center();
    let expected = Fx::ONE + DIAGONAL + Fx::ONE + Fx::ZERO + Fx::from_num(5);
    assert_eq!(remaining(unit, &path), expected);
    let past = PathFollow { next: 5, ..path };
    assert_eq!(remaining(unit, &past), Fx::ZERO);
}
