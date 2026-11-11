use super::*;
use crate::{Scenario, UnitKind};

impl Vision {
    /// Drops one tile from current sight until the next vision refresh.
    pub(crate) fn conceal(&mut self, pos: TilePos) {
        if let Some(visible) = self.visible.get_mut(pos) {
            *visible = false;
        }
    }
}

#[test]
fn eyes_sharing_a_tile_see_what_the_widest_sees() {
    let (narrow, wide) = (UnitKind::Bombard, UnitKind::Kestrel);
    assert!(narrow.stats().vision < wide.stats().vision);
    let visible = |kinds: &[UnitKind]| {
        let mut state = Scenario::skirmish().build().expect("skirmish builds");
        state.units.clear();
        let tile = TilePos::new(state.map.width() / 2, state.map.height() / 2);
        for &kind in kinds {
            state.spawn_unit(PlayerId(0), kind, tile.center());
        }
        state.refresh_vision();
        state.vision(PlayerId(0)).visible.clone()
    };
    let widest = visible(&[wide]);
    assert_eq!(visible(&[narrow, wide]), widest);
    assert_eq!(visible(&[wide, narrow]), widest);
    assert_ne!(visible(&[narrow]), widest);
}

#[test]
fn memories_sharing_a_corner_and_an_owner_sort_by_kind() {
    // Seat 0 remembers one of seat 1's charges at the corner, then sees
    // the Fabricator seat 1 stands from that corner, all but the corner
    // tile itself: the two memories differ only in kind.
    let scenario = serde_json::json!({
        "name": "shared-corner", "mode": "sandbox",
        "map": vec!["................"; 9],
        "players": [
            {"name": "A", "scrap": 0, "bot": false},
            {"name": "B", "scrap": 0, "bot": false}
        ],
        "units": [],
        "buildings": [{"player": 1, "kind": "fabricator", "x": 3, "y": 3}]
    });
    let mut state = Scenario::from_json(&scenario.to_string())
        .expect("scenario parses")
        .build()
        .expect("scenario builds");
    let corner = TilePos::new(3, 3);
    let radius = UnitKind::Sentinel.stats().vision;
    state.spawn_unit(
        PlayerId(0),
        UnitKind::Sentinel,
        TilePos::new(corner.x + 1 + radius, corner.y + 1).center(),
    );
    let charge = BuildingKind::ScuttleCharge;
    state.vision[0].ghosts.push(GhostBuilding {
        kind: charge,
        owner: PlayerId(1),
        anchor: corner,
        hp: charge.base_stats().max_hp,
        built: true,
        tier: 0,
    });
    state.refresh_vision();
    let vision = state.vision(PlayerId(0));
    assert!(
        !vision.visible(corner) && vision.visible(corner.offset(1, 1)),
        "premise: the Fabricator is in sight but its corner is not"
    );
    let kinds: Vec<_> = vision.ghosts().iter().map(|ghost| ghost.kind).collect();
    assert_eq!(kinds, [BuildingKind::Fabricator, charge]);
}
