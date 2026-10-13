use super::team_count;

#[test]
fn a_seat_without_a_team_is_its_own_team() {
    assert_eq!(team_count([None, None, None, None]), 4);
    assert_eq!(team_count([Some(0), Some(1), Some(0), Some(1)]), 2);
    assert_eq!(team_count([Some(0), None, Some(0)]), 2);
}
