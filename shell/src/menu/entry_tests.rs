use super::{browser_blurb, split_format, team_count, team_sizes};
use aftermine_sim::scenario::{MapSize, MapWealth, ScenarioMeta};

#[test]
fn a_seat_without_a_team_is_its_own_team() {
    assert_eq!(team_count([None, None, None, None]), 4);
    assert_eq!(team_count([Some(0), Some(1), Some(0), Some(1)]), 2);
    assert_eq!(team_count([Some(0), None, Some(0)]), 2);
    assert_eq!(team_sizes([Some(0), None, Some(0)]), [1, 2]);
    assert_eq!(team_sizes([Some(1), Some(1), Some(1), Some(0)]), [1, 3]);
}

#[test]
fn a_played_format_names_each_side_as_it_was() {
    for (teams, format) in [
        (&[][..], "Solo"),
        (&[1], "Solo"),
        (&[1, 1], "1v1"),
        (&[2, 2], "2v2"),
        (&[1, 7], "1v7"),
        (&[3, 5], "3v5"),
        (&[2, 2, 2], "2v2v2"),
        (&[1, 1, 1, 1], "4-way FFA"),
        (&[3], "3 seats"),
    ] {
        assert_eq!(split_format(teams), format, "{teams:?}");
    }
}

#[test]
fn a_blurb_shows_only_the_badges_a_map_carries() {
    let meta = ScenarioMeta {
        hook: "Two lanes meet at a pit.".into(),
        size: Some(MapSize::Small),
        mode: "1v1".into(),
        wealth: Some(MapWealth::Standard),
        ..ScenarioMeta::default()
    };
    assert_eq!(
        browser_blurb(&meta),
        "Two lanes meet at a pit.  [small - 1v1 - standard]"
    );
    let timed = ScenarioMeta {
        duration: "5-8 min".into(),
        wealth: None,
        ..meta.clone()
    };
    assert_eq!(
        browser_blurb(&timed),
        "Two lanes meet at a pit.  [small | 5-8 min - 1v1]"
    );
    let bare = ScenarioMeta {
        size: None,
        ..timed
    };
    assert_eq!(
        browser_blurb(&bare),
        "Two lanes meet at a pit.  [5-8 min - 1v1]"
    );
}
