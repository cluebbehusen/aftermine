use super::*;

#[test]
fn looping_rows_wrap_either_way() {
    assert_eq!(cycle(0.0, 7), 0);
    assert_eq!(cycle(6.99, 7), 6);
    assert_eq!(cycle(7.0, 7), 0);
    assert_eq!(cycle(-0.5, 7), 6);
    assert_eq!(cycle(-14.0, 7), 0);
}

#[test]
fn belt_scrap_stays_between_the_belt_rails() {
    for variant in 0..64 {
        let place = scatter(variant);
        assert!((0.0..=1.0).contains(&place), "{variant}: {place}");
    }
}

#[test]
fn the_crane_rides_its_runways_and_starts_at_the_nose() {
    // The runways run unit-canvas columns 33 to 223 along an east-facing
    // hull; the bridge's end trucks reach 12 columns either side of its
    // column 128, and the nose is east.
    let (bridge, along) = crane_offsets(0.0);
    assert_eq!(along, 0.0);
    assert_eq!(bridge, BRIDGE_TRAVEL - BRIDGE_CENTRE);
    for step in 0..=2_000 {
        let (bridge, along) = crane_offsets(step as f32 / 2_000.0);
        let column = 128.0 + bridge;
        assert!(
            column - 12.0 >= 33.0 && column + 12.0 <= 223.0,
            "{step}: {column}"
        );
        assert!(along.abs() <= TROLLEY_TRAVEL, "{step}: {along}");
    }
}

#[test]
fn the_crane_moves_only_as_assembly_advances() {
    assert_eq!(crane_offsets(0.37), crane_offsets(0.37));
    assert_ne!(crane_offsets(0.37), crane_offsets(0.38));
}

#[test]
fn each_lamp_sits_on_the_runway_beside_its_pad() {
    let size = aftermine_sim::BuildingKind::MawCradle.size();
    let pads = aftermine_sim::BuildingKind::MawCradle.crew_pads().unwrap();
    for pad in pads {
        let lamp = lamp_centre(pad, size);
        let tile = CRADLE_CANVAS / size.0 as f32;
        assert!((0.0..CRADLE_CANVAS).contains(&lamp.x), "{pad:?}");
        assert!((0.0..CRADLE_CANVAS).contains(&lamp.y), "{pad:?}");
        let column = pad.0 as f32 * tile;
        assert!((column..column + tile).contains(&lamp.x), "{pad:?}");
        assert_eq!(lamp.y < CRADLE_CANVAS * 0.5, pad.1 < 0, "{pad:?}");
    }
}

#[test]
fn the_treads_roll_with_the_shared_belts_mechanics() {
    // The cleats' frames step one canvas pixel toward the tail; a tile of
    // forward travel runs the upper run a tile of canvas toward the nose.
    let frames = 7;
    let pixels_per_tile = CANVAS / super::super::unit_draw_scale(aftermine_sim::UnitKind::Maw);
    assert_eq!(
        pixels_per_tile, 64.0,
        "a tile of canvas at the roster's scale"
    );
    for step in 0..=128_u8 {
        let travel = f32::from(step) / pixels_per_tile;
        let toward_nose = usize::from(step) % frames;
        assert_eq!(
            tread_phase(travel, frames),
            (frames - toward_nose) % frames,
            "{step} px of travel"
        );
        assert_eq!(
            tread_phase(-travel, frames),
            toward_nose,
            "reversing runs it back"
        );
    }
    // Each side rolls its own travel at the bogies' gauge, as the shared
    // odometer turns a pivot into counter-rolling belts.
    assert_eq!(
        crate::render::tread_gauge(aftermine_sim::UnitKind::Maw),
        Some(GAUGE)
    );
}
