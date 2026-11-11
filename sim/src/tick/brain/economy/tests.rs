use super::{BuildingId, DropOffs};
use crate::state::PROGRESS_ENVELOPE;

#[test]
fn the_live_meter_saturates_below_the_progress_envelope() {
    // The step math's overflow guard: whatever a torch has lived
    // through, the meter it bills from stays under the ceiling the
    // ramp products are proven to fit (state.rs pins the products).
    assert_eq!(super::metered(0), 0);
    assert_eq!(super::metered(PROGRESS_ENVELOPE - 1), PROGRESS_ENVELOPE - 1);
    assert_eq!(super::metered(PROGRESS_ENVELOPE), PROGRESS_ENVELOPE - 1);
    assert_eq!(super::metered(u32::MAX), PROGRESS_ENVELOPE - 1);
}

#[test]
fn captured_drop_offs_list_what_a_scan_of_every_structure_finds() {
    use crate::PlayerId;
    use crate::stats::BuildingKind::{Foundry, Turret};
    use chassis::grid::TilePos;
    let mut state = crate::Scenario::skirmish()
        .build()
        .expect("embedded skirmish builds");
    let (west, east) = (PlayerId(0), PlayerId(1));
    state.place_site(west, Foundry, TilePos::new(10, 3));
    state.place_building(east, Foundry, TilePos::new(14, 3));
    state.place_building(west, Turret, TilePos::new(18, 3));
    let fallen = state.place_building(west, Foundry, TilePos::new(10, 8));
    state.building_mut(fallen).expect("just placed").hp = 0;
    state.place_provisional_site(west, Foundry, TilePos::new(14, 8));
    state.place_building(west, Foundry, TilePos::new(18, 8));
    for player in [west, east] {
        let scan = |built: bool| -> Vec<BuildingId> {
            state
                .buildings()
                .iter()
                .filter(|b| {
                    b.player == player && b.hp > 0 && b.kind.is_drop_off() && (b.built() || !built)
                })
                .map(|b| b.id)
                .collect()
        };
        let listed = |drop_offs: DropOffs| -> Vec<BuildingId> {
            drop_offs.of(&state, player).map(|b| b.id).collect()
        };
        assert_eq!(listed(DropOffs::built(&state)), scan(true));
        assert_eq!(listed(DropOffs::standing(&state)), scan(false));
    }
    assert!(
        DropOffs::standing(&state).of(&state, west).count()
            > DropOffs::built(&state).of(&state, west).count(),
        "unfinished drop-offs stand but take no deliveries"
    );
}
