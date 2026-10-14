use super::*;
use oxide_sim::Scenario;
use oxide_sim::scenario::ScenarioMode;

/// A walled two-seat field whose rows are given; `1` and `2` anchor the
/// Foundries.
fn field(rows: &[&str]) -> State {
    let width = rows[0].len();
    let mut map = vec!["#".repeat(width + 2)];
    map.extend(rows.iter().map(|r| format!("#{r}#")));
    map.push("#".repeat(width + 2));
    Scenario {
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

#[test]
fn an_island_map_is_ground_severed() {
    assert!(!ground_severed(&rooms(1)));
    let state = field(&["..1..~..2..", ".....~.....", ".....~....."]);
    assert!(ground_severed(&state));
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
