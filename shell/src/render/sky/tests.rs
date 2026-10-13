use super::*;

#[test]
fn a_warhead_track_runs_from_launch_to_impact_over_its_flight() {
    let line = oxide_sim::strike::WarheadLine {
        launch: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("2"), chassis::fx::Fx::lit("2")),
        impact: chassis::fx::Vec2Fx::new(chassis::fx::Fx::lit("22"), chassis::fx::Fx::lit("2")),
        arrival: 1_000,
    };
    let ticks = oxide_sim::stats::WARHEAD.flight_ticks(line.launch, line.impact) as f32;
    let start = 1_000.0 - ticks;
    assert_eq!(warhead_track(line, start - 5.0), (Vec2::new(2.0, 2.0), 0.0));
    let (middle, half) = warhead_track(line, start + ticks / 2.0);
    assert!((middle - Vec2::new(12.0, 2.0)).length() < 1e-3);
    assert!((half - 0.5).abs() < 1e-6);
    assert_eq!(warhead_track(line, 5_000.0), (Vec2::new(22.0, 2.0), 1.0));
}
