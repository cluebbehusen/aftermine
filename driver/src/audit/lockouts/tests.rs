use super::*;
use oxide_sim::Scenario;
use oxide_sim::scenario::ScenarioMode;

/// A two-seat field drawn tile by tile: open floor at level 1 unless
/// painted otherwise.
struct Sketch {
    rows: Vec<Vec<char>>,
    levels: Vec<Vec<char>>,
}

impl Sketch {
    fn new(width: usize, height: usize) -> Self {
        Self {
            rows: vec![vec!['.'; width]; height],
            levels: vec![vec!['1'; width]; height],
        }
    }

    /// Paints the inclusive rectangle `(x0, y0)..=(x1, y1)` with glyph `c`
    /// at level `level`.
    fn paint(&mut self, (x0, y0): (usize, usize), (x1, y1): (usize, usize), c: char, level: char) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.rows[y][x] = c;
                self.levels[y][x] = level;
            }
        }
    }

    fn put(&mut self, (x, y): (usize, usize), c: char) {
        self.rows[y][x] = c;
    }

    fn build(self) -> State {
        let lines = |grid: Vec<Vec<char>>| grid.into_iter().map(String::from_iter).collect();
        Scenario {
            levels: lines(self.levels),
            mode: ScenarioMode::Match,
            name: "lockouts".into(),
            map: lines(self.rows),
            players: Scenario::skirmish().players,
            units: Vec::new(),
            buildings: Vec::new(),
            meta: None,
        }
        .build()
        .unwrap()
    }
}

/// An 80x50 field with Foundries mirrored on the middle row and a pit
/// `gap` tiles wide around an island holding `frames` (offsets from its
/// corner). The island sits `shift` tiles right of centre.
fn sea(island: (usize, usize), frames: &[(usize, usize)], gap: usize, shift: usize) -> Sketch {
    let mut sketch = Sketch::new(80, 50);
    let corner = (40 + shift - island.0 / 2, 25 - island.1 / 2);
    sketch.paint(
        (corner.0 - gap, corner.1 - gap),
        (corner.0 + island.0 - 1 + gap, corner.1 + island.1 - 1 + gap),
        '~',
        '1',
    );
    sketch.paint(
        corner,
        (corner.0 + island.0 - 1, corner.1 + island.1 - 1),
        '.',
        '1',
    );
    for &(dx, dy) in frames {
        sketch.put((corner.0 + dx, corner.1 + dy), 'E');
    }
    sketch.put((4, 24), '1');
    sketch.put((74, 24), '2');
    sketch
}

/// Four frames packed into a 4x4 island.
const PACKED: [(usize, usize); 4] = [(0, 0), (2, 0), (0, 2), (2, 2)];

#[test]
fn an_air_only_island_out_of_artillery_reach_is_locked() {
    let prizes = prizes(&sea((4, 4), &PACKED, 15, 0).build());
    assert_eq!(prizes.len(), 1, "{prizes:?}");
    let prize = &prizes[0];
    assert!(prize.island);
    assert_eq!(prize.frames.len(), 4);
    assert!(prize.first.by_air);
    assert_eq!(prize.shelling, 0);
    assert!(prize.contested());
    assert!(prize.locked());
}

#[test]
fn ground_within_artillery_reach_unlocks_an_island() {
    let prizes = prizes(&sea((4, 4), &PACKED, 3, 0).build());
    assert_eq!(prizes.len(), 1, "{prizes:?}");
    assert!(prizes[0].shelling >= SHELLING_FLOOR, "{prizes:?}");
    assert!(!prizes[0].locked());
}

/// [`sea`] with a closed peak ring one tile out from the island, inside the
/// pit.
fn ringed() -> State {
    let mut sketch = sea((4, 4), &PACKED, 3, 0);
    let corner = (38, 23);
    sketch.paint(
        (corner.0 - 2, corner.1 - 2),
        (corner.0 + 5, corner.1 + 5),
        '^',
        '1',
    );
    sketch.paint(
        (corner.0 - 1, corner.1 - 1),
        (corner.0 + 4, corner.1 + 4),
        '~',
        '1',
    );
    sketch.paint(corner, (corner.0 + 3, corner.1 + 3), '.', '1');
    for (dx, dy) in PACKED {
        sketch.put((corner.0 + dx, corner.1 + dy), 'E');
    }
    sketch.build()
}

/// An island in a pit against the north edge, with ground in artillery
/// reach west, east and south. With `walls`, a peak cup open to the north
/// stands between the island and that ground; aircraft enter over its rim.
fn cupped(walls: bool) -> State {
    let mut sketch = Sketch::new(80, 40);
    sketch.paint((30, 0), (49, 15), '~', '1');
    sketch.paint((38, 3), (41, 6), '.', '1');
    if walls {
        sketch.paint((35, 2), (35, 9), '^', '1');
        sketch.paint((44, 2), (44, 9), '^', '1');
        sketch.paint((35, 9), (44, 9), '^', '1');
    }
    for (dx, dy) in PACKED {
        sketch.put((38 + dx, 3 + dy), 'E');
    }
    sketch.put((4, 30), '1');
    sketch.put((74, 30), '2');
    sketch.build()
}

#[test]
fn a_peak_in_the_line_blocks_shelling() {
    let open = prizes(&cupped(false));
    assert_eq!(open.len(), 1, "{open:?}");
    assert!(open[0].shelling >= SHELLING_FLOOR, "{open:?}");
    let walled = prizes(&cupped(true));
    assert_eq!(walled.len(), 1, "{walled:?}");
    assert_eq!(walled[0].shelling, 0, "{walled:?}");
    assert!(walled[0].locked(), "{walled:?}");
}

#[test]
fn frames_no_seat_can_reach_are_no_prize() {
    // A closed peak ring keeps aircraft out too.
    assert_eq!(prizes(&ringed()), Vec::new());
}

#[test]
fn a_prize_one_seat_reaches_well_first_is_not_contested() {
    let prizes = prizes(&sea((4, 4), &PACKED, 15, 12).build());
    assert_eq!(prizes.len(), 1, "{prizes:?}");
    let prize = &prizes[0];
    assert_eq!(prize.first.seat, 1);
    assert!(!prize.contested(), "{prize:?}");
    assert!(!prize.locked());
}

#[test]
fn frames_spread_across_an_island_count_as_one_prize() {
    // No Foundry site supports two of these frames.
    let corners = [(0, 0), (20, 0), (0, 20), (20, 20)];
    let prizes = prizes(&sea((22, 22), &corners, 3, 0).build());
    assert_eq!(prizes.len(), 1, "{prizes:?}");
    assert!(prizes[0].island);
    assert_eq!(prizes[0].frames.len(), 4);
}

/// A 61x40 field with mirrored Foundries below a cliff-ringed bench
/// holding four frames, reached by one ramp three tiles wide. The bench
/// stands above the floor, or below it when `sunken`.
fn bench_sketch(sunken: bool) -> Sketch {
    let (inner, outer) = if sunken { ('1', '2') } else { ('2', '1') };
    let mut sketch = Sketch::new(61, 40);
    sketch.paint((0, 0), (60, 39), '.', outer);
    // The cliff takes the lower level it bounds.
    sketch.paint((21, 2), (39, 16), '=', '1');
    sketch.paint((22, 3), (38, 15), '.', inner);
    sketch.paint((29, 15), (31, 17), '/', '1');
    for anchor in [(24, 5), (35, 5), (24, 11), (35, 11)] {
        sketch.put(anchor, 'E');
    }
    sketch.put((4, 34), '1');
    sketch.put((55, 34), '2');
    sketch
}

fn bench(sunken: bool) -> State {
    bench_sketch(sunken).build()
}

#[test]
fn a_raised_bench_behind_one_ramp_is_locked() {
    let prizes = prizes(&bench(false));
    assert_eq!(prizes.len(), 1, "{prizes:?}");
    let prize = &prizes[0];
    assert!(!prize.island);
    assert_eq!(prize.frames.len(), 4);
    assert_eq!(prize.doors, 3);
    assert!(!prize.first.by_air);
    assert!(prize.contested());
    assert!(prize.locked(), "{prize:?}");
}

#[test]
fn a_sunken_basin_shelled_from_its_rim_is_not_locked() {
    let prizes = prizes(&bench(true));
    assert_eq!(prizes.len(), 1, "{prizes:?}");
    assert_eq!(prizes[0].doors, 3);
    assert!(prizes[0].shelling >= SHELLING_FLOOR, "{prizes:?}");
    assert!(!prizes[0].locked());
}

#[test]
fn scrap_never_walls_off_a_prize() {
    // Rich scrap where the pit was: miners clear it, so the island is
    // reachable on foot and the ground around it can shell it.
    let mut sketch = sea((4, 4), &PACKED, 3, 0);
    sketch.paint((35, 20), (44, 29), 'S', '1');
    sketch.paint((38, 23), (41, 26), '.', '1');
    for (dx, dy) in PACKED {
        sketch.put((38 + dx, 23 + dy), 'E');
    }
    let prizes = prizes(&sketch.build());
    assert!(prizes.iter().all(|prize| !prize.island), "{prizes:?}");
    assert!(prizes.iter().all(|prize| !prize.locked()), "{prizes:?}");
}

#[test]
fn a_map_without_foundries_has_no_prizes() {
    let mut sketch = sea((4, 4), &PACKED, 15, 0);
    sketch.put((4, 24), '.');
    sketch.put((74, 24), '.');
    let lines = |grid: Vec<Vec<char>>| grid.into_iter().map(String::from_iter).collect();
    let state = Scenario {
        levels: lines(sketch.levels),
        mode: ScenarioMode::Sandbox,
        name: "lockouts".into(),
        map: lines(sketch.rows),
        players: Scenario::skirmish().players,
        units: Vec::new(),
        buildings: Vec::new(),
        meta: None,
    }
    .build()
    .unwrap();
    assert_eq!(prizes(&state), Vec::new());
}

#[test]
fn a_cluster_behind_a_door_counts_even_beside_an_outside_frame() {
    // A fifth frame on the floor just below the ramp joins the bench
    // frames in one site's support, but a holder can leave it outside.
    let mut sketch = bench_sketch(false);
    sketch.put((29, 19), 'E');
    let prizes = prizes(&sketch.build());
    assert!(
        prizes
            .iter()
            .any(|prize| prize.frames.len() == 4 && prize.locked()),
        "{prizes:?}"
    );
}

#[test]
fn scrap_never_blocks_a_foundry_site() {
    // Scrap covers every site between two lone frames on the floor, yet a
    // miner clears it, so the pair still seeds a pocket.
    let mut sketch = Sketch::new(61, 40);
    sketch.paint((23, 0), (37, 39), 'S', '1');
    sketch.put((21, 12), 'E');
    sketch.put((38, 12), 'E');
    sketch.put((4, 34), '1');
    sketch.put((55, 34), '2');
    let prizes = prizes(&sketch.build());
    assert!(
        prizes
            .iter()
            .any(|prize| prize.frames == vec![(21, 12), (38, 12)]),
        "{prizes:?}"
    );
}

#[test]
fn aircraft_arrive_by_their_route_around_peaks() {
    // A peak wall in front of the west Foundry, open only at the south
    // edge, sends its aircraft the long way round.
    let mut sketch = sea((4, 4), &PACKED, 15, 0);
    sketch.paint((10, 0), (10, 47), '^', '1');
    let prizes = prizes(&sketch.build());
    assert_eq!(prizes.len(), 1, "{prizes:?}");
    assert_eq!(prizes[0].first.seat, 1);
    assert!(!prizes[0].contested(), "{prizes:?}");
}
