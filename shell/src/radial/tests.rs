use super::*;

#[test]
fn a_push_points_at_the_slice_whose_share_holds_it() {
    // Eight slices: straight up is the first, right the third.
    assert_eq!(slice_toward(vec2(0.0, -1.0), 8), Some(0));
    assert_eq!(slice_toward(vec2(1.0, 0.0), 8), Some(2));
    assert_eq!(slice_toward(vec2(-0.7, -0.7), 8), Some(7));
    // Six slices share the circle in sixths, so up-right is the second
    // and left is the fifth.
    assert_eq!(slice_toward(vec2(0.7, -0.7), 6), Some(1));
    assert_eq!(slice_toward(vec2(-1.0, -0.3), 6), Some(5));
    // Two slices split it top and bottom.
    assert_eq!(slice_toward(vec2(0.2, 0.9), 2), Some(1));
    assert_eq!(slice_toward(vec2(0.9, -0.2), 2), Some(0));
    // Under half travel points at nothing, and so does an empty ring.
    assert_eq!(slice_toward(vec2(0.3, 0.0), 8), None);
    assert_eq!(slice_toward(vec2(1.0, 0.0), 0), None);
}
