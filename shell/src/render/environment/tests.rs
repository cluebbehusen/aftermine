use super::*;
use chassis::grid::as_index;

#[test]
fn solid_authored_walls_are_absorbed_but_open_lanes_stay_playable() {
    let closed = ["#####", "#...#", "#...#", "#####"].map(str::to_string);
    assert_eq!(
        BoundaryInsets::from_rows(&closed),
        BoundaryInsets {
            top: true,
            right: true,
            bottom: true,
            left: true,
        }
    );
    let open = ["##.##", "#...#", "....#", "##.##"].map(str::to_string);
    assert_eq!(
        BoundaryInsets::from_rows(&open),
        BoundaryInsets {
            top: false,
            right: true,
            bottom: false,
            left: false,
        }
    );
}

#[test]
fn floor_wave_carries_across_the_terrace_benches() {
    let phases: Vec<_> = (0..6)
        .map(|tile_x| WAVE_LIFTS[as_index(tile_x) % 6])
        .collect();
    assert_eq!(phases, WAVE_LIFTS);
    assert!(phases.iter().any(|lift| *lift < 0));
    assert!(phases.iter().any(|lift| *lift > 0));
}
