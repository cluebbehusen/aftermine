use super::*;

/// Stale points on one row east of a home at the origin, `xs` tiles out, each
/// its own goal.
fn row(xs: &[i32]) -> (Vec<Point>, Vec<Option<TilePos>>) {
    let points: Vec<Point> = xs
        .iter()
        .map(|x| Point {
            anchor: TilePos::new(*x, 0),
            value: 1,
        })
        .collect();
    let goals = points.iter().map(|point| Some(point.anchor)).collect();
    (points, goals)
}

#[test]
fn a_tour_takes_the_points_it_reaches_in_time_and_no_more() {
    let surveyor = UnitKind::Surveyor;
    let reach = (Fx::from_num(TRAVEL_TICKS) * surveyor.stats().speed).to_num::<i32>();
    let (points, goals) = row(&[reach / 3, 2 * reach / 3, 4 * reach / 3]);
    let scouted = vec![0; points.len()];
    let board = Board {
        now: STALE_TICKS,
        frame: HomeFrame::at(TilePos::new(0, 0), 4 * reach, 10),
        points: &points,
        scouted: &scouted,
    };
    let home = doubled(TilePos::new(0, 0));
    let tour = |spent: u64, held: &[u16]| {
        let mut taken = held.to_vec();
        let tourer = Tourer {
            from: home,
            spent,
            kind: surveyor,
        };
        board.tour(tourer, &goals, &mut taken);
        taken
    };
    assert_eq!(tour(0, &[]), [0, 1], "the third lies beyond the window");
    assert_eq!(tour(0, &[0]), [0, 1], "a held point is passed over");
    assert_eq!(
        tour(TRAVEL_TICKS - 1, &[]),
        [] as [u16; 0],
        "a scout with its window spent takes nothing more"
    );
}
