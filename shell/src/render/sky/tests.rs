use super::*;

#[test]
fn a_warhead_track_runs_from_launch_to_impact_over_its_flight() {
    let line = aftermine_sim::strike::WarheadLine {
        launch: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("2"), chassis::fx::Fx::lit("2")),
        impact: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("22"), chassis::fx::Fx::lit("2")),
        arrival: 1_000,
    };
    let ticks = aftermine_sim::stats::WARHEAD.flight_ticks(line.launch, line.impact) as f32;
    let start = 1_000.0 - ticks;
    assert_eq!(warhead_track(line, start - 5.0), (Vec2::new(2.0, 2.0), 0.0));
    let (middle, half) = warhead_track(line, start + ticks / 2.0);
    assert!((middle - Vec2::new(12.0, 2.0)).length() < 1e-3);
    assert!((half - 0.5).abs() < 1e-6);
    assert_eq!(warhead_track(line, 5_000.0), (Vec2::new(22.0, 2.0), 1.0));
}

#[test]
fn a_warhead_eases_off_its_rail_then_holds_the_simulations_pace() {
    let line = aftermine_sim::strike::WarheadLine {
        launch: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("2"), chassis::fx::Fx::lit("2")),
        impact: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("42"), chassis::fx::Fx::lit("2")),
        arrival: 1_000,
    };
    let flight = Flight::of(line);
    let span = flight.arrival - flight.launched_at;
    let even = |t: f32| t / span;
    // From rest: slower than the even pace at first.
    assert!(flight.progress(flight.launched_at + 2.0) < even(2.0) / 2.0);
    // Then exactly on it, so it stands where the simulation has it.
    for t in [RAIL_TICKS, RAIL_TICKS + 7.5, span / 2.0, span - 1.0] {
        assert!(
            (flight.progress(flight.launched_at + t) - even(t)).abs() < 1e-5,
            "{t}"
        );
    }
    // Low off the rail, cruising mid-flight, down on the target.
    assert_eq!(flight.altitude(flight.launched_at), 0.0);
    assert_eq!(flight.altitude(flight.launched_at + span / 2.0), CRUISE);
    assert_eq!(flight.altitude(flight.arrival), 0.0);
}

#[test]
fn an_interceptor_reaches_its_contact_on_time_and_flies_on_into_the_burst() {
    let path = InterceptorPath::to(Vec2::ZERO, Vec2::new(10.0, 0.0), 100.0, 122.0, 4.0);
    assert_eq!(path.at(100.0), Vec2::ZERO);
    assert!((path.at(122.0).x - (10.0 - INTERCEPTOR_NOSE - FUSE_GAP / 2.0)).abs() < 1e-5);
    let into = path.at(123.0).x - path.at(122.0).x;
    assert!(
        into > 0.0 && into <= 0.2 + 1e-5,
        "it drifts on into the burst"
    );
    assert_eq!(path.altitude_at(122.0), 4.0);
    assert_eq!(path.altitude_at(124.0), 4.0);
}
