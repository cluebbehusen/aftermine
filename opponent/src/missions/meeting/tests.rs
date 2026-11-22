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

#[test]
fn a_second_gun_beside_one_is_its_escort() {
    // A defense going at a gun meets every other enemy that would join the
    // fight there, another gun included: only the gun itself is left out.
    let seen = |guns: &[(i32, i32)]| {
        let mut scenario = Scenario::skirmish();
        scenario.units.push(aftermine_sim::scenario::UnitSpec {
            player: 0,
            kind: UnitKind::Sentinel,
            x: 7,
            y: 4,
        });
        for (x, y) in guns {
            scenario.units.push(aftermine_sim::scenario::UnitSpec {
                player: 1,
                kind: UnitKind::Bombard,
                x: *x,
                y: *y,
            });
        }
        let state = scenario.build().unwrap();
        ObservationData::fog_honest(&state, PlayerId(0))
    };
    let memory = Memory::default();
    let gun = TilePos::new(10, 4);
    let pair = seen(&[(10, 4), (11, 5)]);
    assert_eq!(pair.enemy_units.len(), 2, "premise: both guns in sight");
    assert!(escort(&pair, &memory, gun) > 0);
    let alone = seen(&[(10, 4)]);
    assert_eq!(alone.enemy_units.len(), 1, "premise: the gun in sight");
    assert_eq!(escort(&alone, &memory, gun), 0);
}
