use super::*;
use crate::state::Leash;
use chassis::grid::TilePos;

#[test]
fn an_append_a_full_queue_refuses_leaves_the_unit_untouched() {
    let mut state = crate::Scenario::skirmish().build().unwrap();
    let tick = state.tick;
    let unit = &mut state.units[0];
    let busy = Order::Harvest {
        node: TilePos::new(5, 5),
        anchor: TilePos::new(5, 5),
        withdrawing: false,
    };
    unit.order = busy;
    unit.queue = std::iter::repeat_n(busy, ORDER_QUEUE_CAP).collect();
    unit.leash = Some(Leash {
        anchor: unit.tile(),
        patience: 1,
        cooldown: 2,
    });
    unit.settled = 5;
    unit.worker_mut().danger_retry_at = Some(tick + 3);
    let (id, player) = (unit.id, unit.player);
    let before = state.units[0].clone();
    let mut events = Vec::new();
    apply(
        &mut state,
        &[PlayerCommand {
            player,
            command: crate::Command::Run {
                units: vec![id],
                goal: TilePos::new(8, 8),
                queue: true,
            },
        }],
        &mut events,
    );
    assert!(events.iter().any(|event| matches!(
        event,
        Event::CommandRejected {
            reason: RejectReason::QueueFull,
            ..
        }
    )));
    assert_eq!(state.units[0], before);
}

/// Every verb that hands each unit of a selection its own order refuses a
/// selection whose every eligible unit has a full queue, and changes nothing.
#[test]
fn each_unit_verb_refuses_a_selection_whose_queues_are_full() {
    use crate::ids::AttackTarget;
    use crate::scenario::{BuildingSpec, UnitSpec};
    use crate::stats::{BuildingKind, UnitKind};
    let mut scenario = crate::Scenario::skirmish();
    let mut rows = vec![vec!['.'; 30]; 20];
    rows[1][1] = '1';
    rows[17][27] = '2';
    rows[8][6] = 's';
    scenario.map = rows
        .into_iter()
        .map(|row| row.into_iter().collect())
        .collect();
    let unit = |player, kind, x, y| UnitSpec { player, kind, x, y };
    scenario.units = vec![
        unit(0, UnitKind::Harvester, 8, 8),
        unit(0, UnitKind::Sentinel, 9, 8),
        unit(0, UnitKind::Surveyor, 10, 8),
        unit(0, UnitKind::Skyhook, 11, 8),
        unit(1, UnitKind::Sentinel, 12, 9),
    ];
    scenario.buildings = vec![BuildingSpec {
        player: 0,
        kind: BuildingKind::Crucible,
        x: 4,
        y: 11,
    }];
    let mut state = scenario.build().unwrap();
    let ids: Vec<UnitId> = state.units.iter().map(|unit| unit.id).collect();
    let [worker, sentinel, surveyor, skyhook, enemy] = ids[..] else {
        unreachable!("five staged units");
    };
    let crucible = state
        .buildings
        .iter()
        .find(|b| b.kind == BuildingKind::Crucible)
        .unwrap()
        .id;
    state.building_mut(crucible).unwrap().hp -= 10;
    state.unit_mut(sentinel).unwrap().hp -= 10;
    let busy = Order::Scout {
        tile: TilePos::new(2, 2),
    };
    for unit in &mut state.units {
        unit.order = busy;
        unit.queue = std::iter::repeat_n(busy, ORDER_QUEUE_CAP).collect();
    }
    let node = TilePos::new(6, 8);
    let cases = [
        Command::Scout {
            units: vec![surveyor],
            goal: TilePos::new(15, 15),
            queue: true,
        },
        Command::Attack {
            units: vec![sentinel, worker],
            target: AttackTarget::from(crate::Target::Unit(enemy)),
            queue: true,
        },
        Command::Harvest {
            units: vec![worker],
            node,
            queue: true,
        },
        Command::Repair {
            units: vec![worker],
            building: crucible,
            queue: true,
        },
        Command::Salvage {
            units: vec![worker],
            building: crucible,
            queue: true,
        },
        Command::RepairUnit {
            units: vec![worker],
            target: sentinel,
            queue: true,
        },
        Command::Accompany {
            units: vec![sentinel],
            target: worker,
            queue: true,
        },
        Command::Retire {
            units: vec![sentinel],
            queue: true,
        },
        Command::Load {
            units: vec![sentinel],
            transport: skyhook,
            queue: true,
        },
    ];
    for command in cases {
        let mut tried = state.clone();
        let mut events = Vec::new();
        apply(
            &mut tried,
            &[PlayerCommand {
                player: crate::ids::PlayerId(0),
                command: command.clone(),
            }],
            &mut events,
        );
        assert_eq!(
            events,
            vec![Event::CommandRejected {
                player: crate::ids::PlayerId(0),
                reason: RejectReason::QueueFull,
            }],
            "{command:?}"
        );
        assert_eq!(tried.hash(), state.hash(), "{command:?}");
    }
}
