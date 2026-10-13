use super::*;

#[test]
fn a_warhead_climbs_steeply_cruises_high_and_dives_steeply() {
    let (start, climb) = altitude(0.0);
    let (apex, level) = altitude(0.5);
    let (end, dive) = altitude(1.0);
    assert!(
        start < 0.15 && end < 0.15,
        "it starts and ends on the ground"
    );
    assert!(
        (apex - 1.0).abs() < 1e-6 && level.abs() < 1e-3,
        "level at the top"
    );
    assert!(climb > 3.0 && dive < -3.0, "steep at both ends");
    // Most of the flight is spent high.
    assert!(altitude(0.2).0 > 0.7 && altitude(0.8).0 > 0.7);
    for p in [0.1, 0.25, 0.4] {
        assert!((altitude(p).0 - altitude(1.0 - p).0).abs() < 1e-5);
    }
}

#[test]
fn a_flight_runs_from_launch_to_arrival_and_holds_at_the_ends() {
    let line = oxide_sim::strike::WarheadLine {
        launch: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("2"), chassis::fx::Fx::lit("2")),
        impact: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("22"), chassis::fx::Fx::lit("2")),
        arrival: 1_000,
    };
    let flight = Flight::of(line);
    let ticks = oxide_sim::stats::WARHEAD.flight_ticks(line.launch, line.impact) as f32;
    assert_eq!(flight.arrival - flight.launched_at, ticks);
    assert_eq!(flight.ground(0.0), vec2(2.0, 2.0));
    let middle = flight.ground(flight.launched_at + ticks / 2.0);
    assert!((middle - vec2(12.0, 2.0)).length() < 1e-3);
    assert_eq!(flight.ground(5_000.0), vec2(22.0, 2.0));
    assert!(flight.apex() >= 2.0 && flight.apex() <= 9.0);
}

#[test]
fn deck_rotation_puts_the_art_nose_on_the_compass_heading() {
    assert!(
        deck_rotation(192).rem_euclid(std::f32::consts::TAU) < 1e-6,
        "north"
    );
    assert!(
        (deck_rotation(0) - std::f32::consts::FRAC_PI_2).abs() < 1e-6,
        "east"
    );
}
