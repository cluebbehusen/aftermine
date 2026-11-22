use super::*;

fn stalled(player: u8, unit: u32) -> Event {
    Event::OrderStalled {
        unit: UnitId(unit),
        player: PlayerId(player),
        pos: Vec2Fx::default(),
        reason: StallReason::NoRoute,
    }
}

fn rejected(player: u8, reason: RejectReason) -> Event {
    Event::CommandRejected {
        player: PlayerId(player),
        reason,
    }
}

fn assembled(player: u8, unit: u32) -> Event {
    Event::UnitAssembled {
        cradle: aftermine_sim::BuildingId(9),
        unit: UnitId(unit),
        kind: UnitKind::Maw,
        player: PlayerId(player),
        pos: Vec2Fx::default(),
    }
}

#[test]
fn a_buffer_keeps_the_seats_own_failures_and_other_seats_rollouts_in_order() {
    let mut events = OwnEvents::default();
    events.record(
        PlayerId(1),
        &[
            rejected(0, RejectReason::NotEnoughScrap),
            stalled(1, 7),
            Event::ScrapDeposited {
                unit: aftermine_sim::UnitId(0),
                foundry: aftermine_sim::BuildingId(0),
                player: PlayerId(1),
                amount: 5,
            },
            stalled(0, 3),
            assembled(1, 4),
            rejected(1, RejectReason::QueueFull),
            assembled(0, 5),
        ],
    );
    events.record(PlayerId(1), &[rejected(1, RejectReason::BadSite)]);
    assert_eq!(
        events.take(),
        [
            OwnEvent::OrderStalled {
                unit: UnitId(7),
                pos: Vec2Fx::default(),
                reason: StallReason::NoRoute,
            },
            OwnEvent::CommandRejected {
                reason: RejectReason::QueueFull,
            },
            OwnEvent::MachineAssembled {
                unit: UnitId(5),
                kind: UnitKind::Maw,
                player: PlayerId(0),
                pos: Vec2Fx::default(),
            },
            OwnEvent::CommandRejected {
                reason: RejectReason::BadSite,
            },
        ]
    );
    assert_eq!(events, OwnEvents::default());
}

#[test]
fn repeats_of_one_failure_keep_its_first_place() {
    let mut events = OwnEvents::default();
    let mut tick: Vec<Event> = (0..40).map(|_| stalled(0, 7)).collect();
    tick.insert(1, stalled(0, 8));
    tick.extend([
        rejected(0, RejectReason::BadSite),
        rejected(0, RejectReason::QueueFull),
        rejected(0, RejectReason::BadSite),
    ]);
    events.record(PlayerId(0), &tick);
    let wide: Vec<Event> = (100..200).map(|unit| stalled(0, unit)).collect();
    events.record(PlayerId(0), &wide);
    let kept = events.take();
    assert_eq!(kept.len(), 4 + wide.len(), "{kept:?}");
    let stalled_unit = |event: &OwnEvent| match event {
        OwnEvent::OrderStalled { unit, .. } => Some(unit.0),
        OwnEvent::CommandRejected { .. }
        | OwnEvent::Hit { .. }
        | OwnEvent::MachineAssembled { .. } => None,
    };
    assert_eq!(stalled_unit(&kept[0]), Some(7));
    assert_eq!(stalled_unit(&kept[1]), Some(8));
    assert_eq!(
        kept[2..4],
        [
            OwnEvent::CommandRejected {
                reason: RejectReason::BadSite
            },
            OwnEvent::CommandRejected {
                reason: RejectReason::QueueFull
            },
        ]
    );
}

#[test]
fn a_full_buffer_drops_its_oldest_events_and_refuses_to_load_over_its_cap() {
    let mut events = OwnEvents::default();
    let tick: Vec<Event> = (0..u32::try_from(CAP).unwrap() + 6)
        .map(|unit| stalled(0, unit))
        .collect();
    events.record(PlayerId(0), &tick);
    let json = serde_json::to_value(&events).unwrap();
    let kept: Vec<u32> = events
        .take()
        .into_iter()
        .map(|event| match event {
            OwnEvent::OrderStalled { unit, .. } => unit.0,
            OwnEvent::CommandRejected { .. }
            | OwnEvent::Hit { .. }
            | OwnEvent::MachineAssembled { .. } => unreachable!(),
        })
        .collect();
    assert_eq!(
        kept,
        (6..u32::try_from(CAP).unwrap() + 6).collect::<Vec<_>>()
    );

    assert_eq!(json.as_array().unwrap().len(), CAP);
    assert_eq!(
        (&json[0]["event"], &json[0]["unit"], &json[0]["reason"]),
        (
            &serde_json::json!("order_stalled"),
            &serde_json::json!(6),
            &serde_json::json!("no_route")
        )
    );
    let restored: OwnEvents = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&restored).unwrap(), json);
    let mut over = json;
    over.as_array_mut()
        .unwrap()
        .push(serde_json::json!({"event": "command_rejected", "reason": "bad_site"}));
    assert!(serde_json::from_value::<OwnEvents>(over).is_err());
}

#[test]
fn the_seat_s_own_hits_are_kept_once_per_place_and_bounded() {
    let hit = |player: u8, x: i32| Event::DamageTaken {
        player: PlayerId(player),
        pos: Vec2Fx::new(chassis::fx::Fx::from_num(x), chassis::fx::Fx::from_num(3)),
    };
    let mut events = OwnEvents::default();
    // A structure hit by every volley is one place; another seat's hit is
    // not the seat's to know.
    events.record(PlayerId(0), &[hit(0, 7), hit(0, 7), hit(1, 9), hit(0, 8)]);
    let held = events.clone().take();
    assert_eq!(held.len(), 2, "{held:?}");
    // Kept through a checkpoint.
    let json = serde_json::to_value(&events).unwrap();
    let restored: OwnEvents = serde_json::from_value(json).unwrap();
    assert_eq!(restored, events);
    // A battle among moving units fills only the hits' share: a stall still
    // gets in.
    let many: Vec<Event> = (100..100 + i32::try_from(HITS).unwrap() * 2)
        .map(|x| hit(0, x))
        .collect();
    events.record(PlayerId(0), &many);
    events.record(PlayerId(0), &[stalled(0, 5)]);
    let kept = events.take();
    let hits = kept
        .iter()
        .filter(|event| matches!(event, OwnEvent::Hit { .. }))
        .count();
    assert_eq!(hits, HITS);
    assert!(
        kept.iter()
            .any(|event| matches!(event, OwnEvent::OrderStalled { .. })),
        "{kept:?}"
    );
}
