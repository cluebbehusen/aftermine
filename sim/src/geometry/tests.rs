use super::{footprint_distance, foundry_supports, work_tile};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;

#[test]
fn a_worker_on_a_work_position_s_edge_takes_a_mirrored_tile() {
    let (width, height) = (24, 16);
    let turn = |point: Vec2Fx| Vec2Fx::new(Fx::from_num(width), Fx::from_num(height)) - point;
    let at = |x: &str, y: &str| Vec2Fx::new(Fx::lit(x), Fx::lit(y));
    // A four-by-four footprint whose centre sits on tile seams, with each
    // worker standing on the edge its work position lies on.
    let centre = at("8", "6");
    for (entry, from) in [
        (at("8", "3.6"), at("8", "1")),
        (at("8", "8.4"), at("8", "11")),
        (at("5.6", "6"), at("3", "6")),
        (at("10.4", "6"), at("13", "6")),
        (at("10.4", "4"), at("10.4", "2")),
        (at("10", "6"), at("10", "2")),
    ] {
        let tile = work_tile(entry, from, centre);
        assert_eq!(
            work_tile(turn(entry), turn(from), turn(centre)),
            TilePos::new(width - 1 - tile.x, height - 1 - tile.y),
            "{entry:?} from {from:?}"
        );
    }
}

#[test]
fn footprints_touching_are_one_apart_and_support_ends_past_eight() {
    let at = TilePos::new;
    assert_eq!(footprint_distance(at(0, 0), (2, 2), at(2, 0), (2, 2)), 1);
    assert_eq!(footprint_distance(at(0, 0), (2, 2), at(1, 1), (2, 2)), 0);
    assert_eq!(footprint_distance(at(0, 0), (1, 1), at(4, 8), (2, 2)), 8);
    assert!(foundry_supports(at(0, 0), at(9, 0)));
    assert!(!foundry_supports(at(0, 0), at(10, 0)));
    assert!(foundry_supports(at(9, 9), at(0, 0)));
}
