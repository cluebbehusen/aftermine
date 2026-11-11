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
