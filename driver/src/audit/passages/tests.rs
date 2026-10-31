use super::*;
use aftermine_sim::Scenario;
use aftermine_sim::scenario::ScenarioMode;

/// A walled two-seat field whose rows are given; `1` and `2` anchor the
/// Foundries.
fn field(rows: &[&str]) -> State {
    let width = rows[0].len();
    let mut map = vec!["#".repeat(width + 2)];
    map.extend(rows.iter().map(|r| format!("#{r}#")));
    map.push("#".repeat(width + 2));
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "passages".into(),
        map,
        players: Scenario::skirmish().players,
        units: Vec::new(),
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .unwrap()
}

/// Two open rooms joined through a wall by a gap `gap` tiles tall.
fn rooms(gap: usize) -> State {
    let mut rows = Vec::new();
    for y in 0..9 {
        let door = (4 - gap / 2..4 - gap / 2 + gap).contains(&y);
        let wall = if door { '.' } else { '#' };
        let mut row = String::new();
        row.push_str(match y {
            3 => "..1.......",
            _ => "..........",
        });
        row.push(wall);
        row.push_str(match y {
            3 => ".......2..",
            _ => "..........",
        });
        rows.push(row);
    }
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    field(&refs)
}

#[test]
fn a_gap_narrower_than_three_on_the_route_is_reported() {
    for gap in [1, 2] {
        let passages = narrow_passages(&rooms(gap));
        assert_eq!(passages.len(), 1, "gap {gap}: {passages:?}");
        // The door sits in the wall column, x = 11 with the border.
        assert_eq!(passages[0].min.0, 11, "gap {gap}");
    }
    assert!(narrow_passages(&rooms(3)).is_empty());
}

#[test]
fn a_notch_nothing_needs_to_enter_is_not_reported() {
    // A one-tile slot cut into the north wall, off every route.
    let state = field(&[
        "....#.#.....",
        "............",
        "..1.......2.",
        "............",
        "............",
    ]);
    assert!(narrow_passages(&state).is_empty());
}

/// Two nine-tall rooms stacked corner to corner, overlapping by
/// `overlap` columns.
fn staircase(overlap: usize) -> State {
    let mut rows = Vec::new();
    for y in 0..18 {
        let row: String = (0..18)
            .map(|x| {
                let open = if y < 9 { x < 10 } else { x >= 10 - overlap };
                match (x, y) {
                    (3, 3) => '1',
                    (13, 12) => '2',
                    _ if open => '.',
                    _ => '#',
                }
            })
            .collect();
        rows.push(row);
    }
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    field(&refs)
}

#[test]
fn a_two_wide_staircase_between_open_rooms_is_narrow() {
    // Every tile of the two-column overlap is covered by some open
    // square, but no square slides from one room into the other.
    let passages = narrow_passages(&staircase(2));
    assert_eq!(passages.len(), 1, "{passages:?}");
    assert!(narrow_passages(&staircase(3)).is_empty());
}

/// A walled three-seat field: `1` and `2` share the west ground and `3`
/// stands alone east of a pit trench, with its Extractor frame behind a
/// one-tile door.
fn hybrid(west_gap: usize) -> State {
    let mut rows = Vec::new();
    for y in 0..12 {
        let west_wall = if (5..5 + west_gap).contains(&y) {
            '.'
        } else {
            '#'
        };
        let east_wall = if y == 5 { '.' } else { '#' };
        let row: String = (0..33)
            .map(|x| match (x, y) {
                (2, 5) => '1',
                (14, 5) => '2',
                (24, 5) => '3',
                (30, 5) => 'E',
                (9, _) => west_wall,
                (20, _) => '~',
                (27, _) => east_wall,
                _ => '.',
            })
            .collect();
        rows.push(row);
    }
    let width = rows[0].len();
    let mut map = vec!["#".repeat(width + 2)];
    map.extend(rows.iter().map(|r| format!("#{r}#")));
    map.push("#".repeat(width + 2));
    let mut players = Scenario::skirmish().players;
    let mut third = players[1].clone();
    third.name = "East".into();
    players.push(third);
    Scenario {
        levels: Vec::new(),
        mode: ScenarioMode::Match,
        name: "hybrid".into(),
        map,
        players,
        units: Vec::new(),
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .unwrap()
}

#[test]
fn the_rule_binds_ground_where_hostile_seats_meet_and_spares_lone_islands() {
    // The west door between two hostile seats counts; the east island's
    // door to its own frame never does, though traffic crosses it.
    let narrow = narrow_passages(&hybrid(1));
    assert_eq!(narrow.len(), 1, "{narrow:?}");
    assert_eq!(narrow[0].min.0, 10);
    assert!(narrow_passages(&hybrid(3)).is_empty());
}

#[test]
fn an_island_map_reports_no_passages() {
    // Each seat on its own island, its frame behind a one-tile door.
    let state = field(&[
        ".......#E.~.........",
        "..1....#..~.....2...",
        ".......#..~.........",
        ".......#..~.....#...",
        ".......#..~.....#...",
        ".......#..~.....#...",
        ".......#..~.....#.E.",
        "..........~.........",
    ]);
    // The west island's frame lies past a one-tile gap under its rock, so
    // the same layout on contested ground would report it.
    let grid = Grid::new(&state);
    assert!(narrow_tiles(&grid).iter().any(|&n| n));
    assert!(narrow_passages(&state).is_empty());
}

/// The traffic mask as it was first written: every pair of points' fields
/// at once, a tile on a shortest route when its two distances sum to the
/// pair's.
fn pairwise_traffic(grid: &Grid, points: &[Vec<TilePos>]) -> Vec<bool> {
    let fields: Vec<Vec<u32>> = points.iter().map(|p| grid.distances(p)).collect();
    let mut traffic = vec![false; grid.cells()];
    for i in 0..fields.len() {
        for j in (i + 1)..fields.len() {
            let total = points[j]
                .iter()
                .map(|&t| fields[i][grid.index(t)])
                .min()
                .unwrap_or(u32::MAX);
            if total == u32::MAX {
                continue;
            }
            for (cell, flag) in traffic.iter_mut().enumerate() {
                let (a, b) = (fields[i][cell], fields[j][cell]);
                if a != u32::MAX && b != u32::MAX && a + b == total {
                    *flag = true;
                }
            }
        }
    }
    traffic
}

#[test]
fn walking_back_marks_exactly_the_pairwise_shortest_routes() {
    let mut staged = vec![
        rooms(1),
        rooms(3),
        staircase(2),
        offset_blocks(1),
        diagonal_ends(3),
        hybrid(1),
    ];
    // Random rock fields with frames and the two Foundries in fixed spots.
    let mut rng = chassis::rng::Pcg32::new(41, 3);
    for _ in 0..12 {
        let rows: Vec<String> = (0..16)
            .map(|y| {
                (0..24)
                    .map(|x| match (x, y) {
                        (2, 2) => '1',
                        (20, 12) => '2',
                        (11, 7) | (4, 12) => 'E',
                        (1..=5, 1..=5) | (18..=23, 10..=15) => '.',
                        (10..=13, 6..=9) => '.',
                        (3..=6, 11..=14) => '.',
                        _ if rng.next_below(5) == 0 => '#',
                        _ => '.',
                    })
                    .collect()
            })
            .collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        staged.push(field(&refs));
    }
    for state in &staged {
        let grid = Grid::new(state);
        let points = points_of_interest(state, &grid);
        assert_eq!(traffic(&grid, &points), pairwise_traffic(&grid, &points));
    }
}

/// An open field with two rock blocks, one above the other, offset so
/// `gap` columns separate them where their corners meet on the straight
/// route. Open rows above and below leave a wide way around.
fn offset_blocks(gap: i32) -> State {
    let mut rows = Vec::new();
    for y in 0..21 {
        let row: String = (0..21)
            .map(|x| {
                let upper = (8..=9).contains(&x) && (3..=9).contains(&y);
                let lower = (10 + gap..=11 + gap).contains(&x) && (10..=16).contains(&y);
                match (x, y) {
                    (2, 9) => '1',
                    (17, 9) => '2',
                    _ if upper || lower => '#',
                    _ => '.',
                }
            })
            .collect();
        rows.push(row);
    }
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    field(&refs)
}

#[test]
fn a_diagonal_squeeze_counts_even_beside_a_wide_detour() {
    // A wide body can walk around both blocks, yet the straight route
    // still slips between their corners.
    for gap in [1, 2] {
        let passages = narrow_passages(&offset_blocks(gap));
        assert!(!passages.is_empty(), "gap {gap}");
    }
    assert!(narrow_passages(&offset_blocks(3)).is_empty());
}

/// An open field crossed by two rock walls whose ends meet on a diagonal:
/// the north wall ends at `(9, 9)` and the south wall starts `step` tiles
/// further along both axes. The only way between the Foundries runs past
/// the wall ends.
fn diagonal_ends(step: i32) -> State {
    let mut rows = Vec::new();
    for y in 0..24 {
        let row: String = (0..24)
            .map(|x| {
                let north = x == 9 && y <= 9;
                let south = x == 9 + step && y >= 9 + step;
                match (x, y) {
                    (3, 3) => '1',
                    (19, 19) => '2',
                    _ if north || south => '#',
                    _ => '.',
                }
            })
            .collect();
        rows.push(row);
    }
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    field(&refs)
}

#[test]
fn wall_ends_meeting_on_a_diagonal_must_leave_three_tiles() {
    for step in [2, 3] {
        assert!(
            !narrow_passages(&diagonal_ends(step)).is_empty(),
            "step {step}"
        );
    }
    assert!(narrow_passages(&diagonal_ends(4)).is_empty());
}

#[test]
fn obstacles_touching_at_a_corner_are_a_wall_not_a_gap() {
    // A scrap block touching a Foundry's corner: nothing can pass between,
    // so the tiles beside the contact are no squeeze.
    let state = field(&[
        "..............",
        "..............",
        "..............",
        "......1.......",
        "..............",
        "....ss........",
        "....ss........",
        "..............",
        "..............",
        "............2.",
        "..............",
    ]);
    assert!(narrow_passages(&state).is_empty());
}
