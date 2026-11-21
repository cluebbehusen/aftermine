use super::*;
use crate::events::OwnEvent;
use aftermine_sim::{PlayerId, Scenario, UnitId};
use chassis::fx::Vec2Fx;

#[test]
fn a_machine_reported_standing_up_out_of_sight_opposes_at_once() {
    let state = Scenario::skirmish().build().unwrap();
    let observation = ObservationData::fog_honest(&state, PlayerId(0));
    let place = TilePos::new(30, 10);
    let mut memory = Memory::default();
    memory.assembled(
        &observation,
        &[OwnEvent::MachineAssembled {
            unit: UnitId(9_000),
            kind: UnitKind::Maw,
            player: PlayerId(1),
            pos: Vec2Fx::new(Fx::from_num(place.x), Fx::from_num(place.y)),
        }],
        |player| player != PlayerId(0),
    );
    assert!(
        !observation
            .enemy_units
            .iter()
            .any(|enemy| enemy.id == UnitId(9_000)),
        "premise: the machine is out of sight"
    );
    assert!(opposed(&observation, &memory, place) > 0);
}
