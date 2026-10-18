use super::*;
use oxide_sim::scenario::{PlayerSpec, ScenarioMode, UnitSpec};

/// A plateau at level 2 over a floor at level 1, joined by a 3x3 ramp.
const ROWS: [&str; 11] = [
    "##############",
    "#............#",
    "#............#",
    "#............#",
    "#....///.....#",
    "#====///=====#",
    "#....///.....#",
    "#............#",
    "#.1........2.#",
    "#............#",
    "##############",
];
const LEVELS: [&str; 11] = [
    "22222222222222",
    "22222222222222",
    "22222222222222",
    "22222222222222",
    "22222111222222",
    "22222111222222",
    "11111111111111",
    "11111111111111",
    "11111111111111",
    "11111111111111",
    "11111111111111",
];

fn scenario(rows: &[String], levels: &[String]) -> Scenario {
    Scenario {
        mode: ScenarioMode::Sandbox,
        name: "levels".into(),
        map: rows.to_vec(),
        levels: levels.to_vec(),
        players: (0..2)
            .map(|seat| PlayerSpec {
                name: format!("seat {seat}"),
                team: None,
                scrap: 0,
                bot: false,
                bot_config: None,
            })
            .collect(),
        units: Vec::new(),
        buildings: Vec::new(),
        meta: None,
    }
}

fn owned(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|row| (*row).to_string()).collect()
}

/// `rows` with the cell at `(x, y)` set to `c`.
fn with(mut rows: Vec<String>, x: usize, y: usize, c: char) -> Vec<String> {
    rows[y].replace_range(x..=x, &c.to_string());
    rows
}

fn faults_of(rows: &[String], levels: &[String]) -> Vec<String> {
    level_faults(&scenario(rows, levels)).expect("the map parses")
}

#[test]
fn a_well_made_tiered_map_passes() {
    assert_eq!(
        faults_of(&owned(&ROWS), &owned(&LEVELS)),
        Vec::<String>::new()
    );
}

#[test]
fn a_flat_map_passes_unless_it_carries_a_single_level_layer() {
    let flat: Vec<String> = ROWS
        .iter()
        .map(|row| row.replace(['=', '/'], "."))
        .collect();
    assert!(faults_of(&flat, &[]).is_empty());
    let uniform = vec!["1".repeat(14); 11];
    let faults = faults_of(&flat, &uniform);
    assert_eq!(faults.len(), 1, "{faults:?}");
    assert!(faults[0].contains("leave it out"));
}

#[test]
fn a_narrow_ramp_is_refused() {
    let mut rows = owned(&ROWS);
    let mut levels = owned(&LEVELS);
    for y in 4..=6 {
        rows = with(rows, 7, y, if y == 5 { '=' } else { '.' });
        levels = with(levels, 7, y, if y == 6 { '1' } else { '2' });
    }
    let faults = faults_of(&rows, &levels);
    assert!(
        faults.iter().any(|f| f.contains("2 tiles across")),
        "{faults:?}"
    );
}

#[test]
fn a_ramp_must_be_a_filled_rectangle() {
    let rows = with(owned(&ROWS), 7, 4, '.');
    let levels = with(owned(&LEVELS), 7, 4, '2');
    let faults = faults_of(&rows, &levels);
    assert!(
        faults.iter().any(|f| f.contains("filled rectangle")),
        "{faults:?}"
    );
}

#[test]
fn a_ramp_must_climb_through_a_cliff() {
    // A ramp laid on open floor joins nothing: the floor meets both ends.
    let mut rows = owned(&ROWS);
    let mut levels = owned(&LEVELS);
    for x in 9..=11 {
        rows = with(rows, x, 7, '/');
        levels = with(levels, x, 7, '0');
    }
    let faults = faults_of(&rows, &levels);
    assert!(
        faults.iter().any(|f| f.contains("does not run through")),
        "{faults:?}"
    );
}

#[test]
fn a_ramp_keeps_clear_of_peaks() {
    let rows = with(owned(&ROWS), 4, 3, '^');
    let faults = faults_of(&rows, &owned(&LEVELS));
    assert!(
        faults.iter().any(|f| f.contains("touches a peak")),
        "{faults:?}"
    );
}

#[test]
fn a_peak_stands_on_one_level() {
    let mut rows = owned(&ROWS);
    for y in 3..=6 {
        rows = with(rows, 11, y, '^');
    }
    let faults = faults_of(&rows, &owned(&LEVELS));
    assert!(
        faults
            .iter()
            .any(|f| f.contains("Peak") && f.contains("[1, 2]")),
        "{faults:?}"
    );
}

#[test]
fn a_cliff_that_parts_nothing_is_refused() {
    let rows = with(owned(&ROWS), 10, 8, '=');
    let levels = with(owned(&LEVELS), 10, 8, '1');
    let faults = faults_of(&rows, &levels);
    assert!(
        faults.iter().any(|f| f.contains("parts no levels")),
        "{faults:?}"
    );
}

#[test]
fn starts_stay_off_ramps() {
    let mut scenario = scenario(&owned(&ROWS), &owned(&LEVELS));
    scenario.units = vec![UnitSpec {
        player: 0,
        kind: oxide_sim::UnitKind::Sentinel,
        x: 6,
        y: 5,
    }];
    let faults = level_faults(&scenario).unwrap();
    assert!(faults.iter().any(|f| f.contains("unit #0")), "{faults:?}");
    let rows = with(with(owned(&ROWS), 2, 8, '.'), 4, 6, '1');
    let faults = faults_of(&rows, &owned(&LEVELS));
    assert!(
        faults
            .iter()
            .any(|f| f.contains("Foundry") && f.contains("ramp")),
        "{faults:?}"
    );
}
