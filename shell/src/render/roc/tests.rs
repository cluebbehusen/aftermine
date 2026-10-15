use super::*;

#[test]
fn the_hatch_swings_open_for_a_rider_then_closes() {
    assert_eq!(hatch_opening(None), 0.0);
    let at = |progress| hatch_opening(Some(TransportActionState::Boarding { progress }));
    assert_eq!(at(0.0), 0.0);
    assert_eq!(at(0.4), 1.0, "open while the rider passes");
    assert_eq!(at(1.0), 0.0, "shut once it has");
    assert_eq!(
        hatch_opening(Some(TransportActionState::Unloading { progress: 0.4 })),
        1.0,
        "leaving opens it too"
    );
}

#[test]
fn hull_offsets_turn_with_the_hull() {
    let east = turned(vec2(0.0, -10.0), std::f32::consts::FRAC_PI_2, 2.0);
    assert!((east - vec2(20.0, 0.0)).length() < 1e-4, "{east}");
}
