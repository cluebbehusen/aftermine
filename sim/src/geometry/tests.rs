use super::{body_tiles, footprint_distance, foundry_supports, hull_open, work_tile};
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

#[test]
fn a_small_body_claims_exactly_its_centre_tile() {
    // Every radius up to the small-body limit, at positions across a tile
    // including its edges and corners, claims the containing tile alone.
    let radii = ["0.25", "0.3", "0.42", "0.5", "0.55"];
    for radius in radii {
        for qx in 0..=8 {
            for qy in 0..=8 {
                let pos = Vec2Fx::new(
                    Fx::from_num(5) + Fx::from_num(qx) / 8,
                    Fx::from_num(7) + Fx::from_num(qy) / 8,
                );
                let tiles: Vec<TilePos> = body_tiles(pos, Fx::lit(radius)).collect();
                assert_eq!(tiles, vec![TilePos::containing(pos)], "{radius} at {pos:?}");
            }
        }
    }
}

#[test]
fn a_wide_body_claims_the_tiles_its_hull_reaches_mirrored_under_a_half_turn() {
    let radius = Fx::lit("1.4");
    let at = |x: &str, y: &str| Vec2Fx::new(Fx::lit(x), Fx::lit(y));
    // Centred on a tile: the 3x3 block around it.
    let tiles: Vec<TilePos> = body_tiles(at("5.5", "7.5"), radius).collect();
    assert_eq!(tiles.len(), 9);
    assert!(tiles.contains(&TilePos::new(4, 6)) && tiles.contains(&TilePos::new(6, 8)));
    // On a tile corner: the 2x2 either side, symmetric about the corner.
    let corner: Vec<TilePos> = body_tiles(at("5", "7"), radius).collect();
    assert_eq!(
        corner,
        vec![
            TilePos::new(4, 6),
            TilePos::new(5, 6),
            TilePos::new(4, 7),
            TilePos::new(5, 7)
        ]
    );
    // A half turn of the map turns the claimed set with it.
    let (width, height) = (20, 16);
    for (x, y) in [("5.25", "7.5"), ("3.9", "8.1"), ("6", "6.5")] {
        let pos = at(x, y);
        let turned = Vec2Fx::new(Fx::from_num(width), Fx::from_num(height)) - pos;
        let mut mirrored: Vec<TilePos> = body_tiles(pos, radius)
            .map(|t| TilePos::new(width - 1 - t.x, height - 1 - t.y))
            .collect();
        mirrored.sort();
        let mut direct: Vec<TilePos> = body_tiles(turned, radius).collect();
        direct.sort();
        assert_eq!(mirrored, direct, "{x},{y}");
    }
}

#[test]
fn a_hull_fits_only_where_every_claimed_tile_is_open() {
    let rock = TilePos::new(6, 6);
    let open = |t: TilePos| t != rock;
    assert!(hull_open(TilePos::new(6, 7), Fx::lit("0.55"), open));
    assert!(!hull_open(TilePos::new(6, 7), Fx::lit("1.4"), open));
    assert!(hull_open(TilePos::new(6, 8), Fx::lit("1.4"), open));
}

#[test]
fn a_hull_backs_out_of_closed_ground_but_never_presses_further_in() {
    use super::hull_step_open;
    let rock = TilePos::new(7, 5);
    let open = |t: TilePos| t != rock;
    let radius = Fx::lit("1.4");
    let at = |x: &str, y: &str| Vec2Fx::new(Fx::lit(x), Fx::lit(y));
    // Centred beside the rock, the hull already covers it: stepping away
    // is allowed, stepping onto fresh closed ground is not.
    assert!(hull_step_open(
        at("6.5", "5.5"),
        at("6.4", "5.5"),
        radius,
        open
    ));
    assert!(!hull_step_open(
        at("5.5", "5.5"),
        at("6.5", "5.5"),
        radius,
        open
    ));
    // A small body is held only by the tile it steps onto.
    assert!(!hull_step_open(
        at("6.5", "5.5"),
        at("7.5", "5.5"),
        Fx::lit("0.3"),
        open
    ));
}

#[test]
fn a_hull_leg_is_open_only_if_every_tile_it_could_claim_is() {
    use super::hull_leg_open;
    let rock = TilePos::new(7, 7);
    let open = |t: TilePos| t != rock;
    let radius = Fx::lit("1.4");
    let at = |x: &str, y: &str| Vec2Fx::new(Fx::lit(x), Fx::lit(y));
    // Both ends clear the rock, but the corner of the block between them,
    // which a diagonal leg sweeps, does not.
    assert!(hull_open(TilePos::new(5, 6), radius, open));
    assert!(hull_open(TilePos::new(6, 5), radius, open));
    assert!(!hull_leg_open(
        at("5.5", "6.5"),
        at("6.5", "5.5"),
        radius,
        open
    ));
    assert!(hull_leg_open(
        at("5.5", "5.5"),
        at("5.5", "6.5"),
        radius,
        open
    ));
}

#[test]
fn the_widest_radius_and_excess_follow_the_roster() {
    use crate::stats::{MAX_HULL_EXCESS, MAX_UNIT_RADIUS, UnitKind, hull_excess};
    let widest = UnitKind::ALL
        .iter()
        .map(|k| k.stats().radius)
        .max()
        .expect("units exist");
    assert_eq!(MAX_UNIT_RADIUS, widest);
    assert_eq!(MAX_HULL_EXCESS, hull_excess(widest));
}
