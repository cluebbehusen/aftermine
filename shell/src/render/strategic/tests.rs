use super::*;

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
