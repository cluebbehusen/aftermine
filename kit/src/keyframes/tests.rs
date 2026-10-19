use super::*;
use oxide_sim::Scenario;

/// A skirmish's worlds at each keyframe tick through `last`.
fn worlds(last: u64) -> Vec<State> {
    let mut state = Scenario::skirmish().build().unwrap();
    let mut worlds = Vec::new();
    while state.current_tick() < last {
        state.tick(&[]);
        if state.current_tick().is_multiple_of(KEYFRAME_EVERY) {
            worlds.push(state.clone());
        }
    }
    worlds
}

fn chain(worlds: &[State]) -> Keyframes {
    let mut encoder = ChainEncoder::default();
    Keyframes::new(worlds.iter().map(|w| encoder.encode(w).unwrap()).collect())
}

#[test]
fn chains_restart_so_a_restore_decodes_a_bounded_run() {
    let worlds = worlds(10 * KEYFRAME_EVERY);
    let keyframes = chain(&worlds);
    let encodings: Vec<Encoding> = keyframes.frames().iter().map(|f| f.encoding).collect();
    let mut expected = vec![Encoding::Delta; 10];
    expected[0] = Encoding::Full;
    expected[CHAIN] = Encoding::Full;
    assert_eq!(encodings, expected);
    assert!(
        keyframes.frames()[1].bytes.len() < keyframes.frames()[0].bytes.len(),
        "a delta carries only what changed"
    );
    keyframes.validate(0, 10 * KEYFRAME_EVERY).unwrap();
    let origin = Scenario::skirmish().build().unwrap();
    for (index, world) in worlds.iter().enumerate() {
        assert_eq!(
            keyframes.decode(index, &origin).unwrap().hash(),
            world.hash()
        );
    }
    assert_eq!(keyframes.at_or_before(KEYFRAME_EVERY - 1), None);
    assert_eq!(keyframes.at_or_before(3 * KEYFRAME_EVERY + 5), Some(2));
    assert_eq!(
        keyframes.hash_at(2 * KEYFRAME_EVERY),
        Some(worlds[1].hash())
    );
    assert_eq!(keyframes.hash_at(2 * KEYFRAME_EVERY + 1), None);
}

#[test]
fn an_index_off_the_record_or_beyond_its_chains_is_refused() {
    let good = chain(&worlds(3 * KEYFRAME_EVERY));
    let end = 3 * KEYFRAME_EVERY;
    good.validate(0, end).unwrap();
    let edit = |change: &dyn Fn(&mut Vec<Keyframe>)| {
        let mut frames = good.frames().to_vec();
        change(&mut frames);
        Keyframes::new(frames)
    };
    assert!(
        good.validate(KEYFRAME_EVERY, end).is_err(),
        "at or before the origin"
    );
    assert!(good.validate(0, end - 1).is_err(), "past the record's end");
    assert!(
        edit(&|f| f[1].tick += 1).validate(0, end).is_err(),
        "off the grid"
    );
    assert!(
        edit(&|f| f.swap(0, 1)).validate(0, end).is_err(),
        "out of order"
    );
    assert!(
        edit(&|f| f[0].encoding = Encoding::Delta)
            .validate(0, end)
            .is_err()
    );
    assert!(
        edit(&|f| f[2].decoded = MAX_DECODED + 1)
            .validate(0, end)
            .is_err()
    );
    let long = chain(&worlds(9 * KEYFRAME_EVERY));
    let mut frames = long.frames().to_vec();
    frames[CHAIN].encoding = Encoding::Delta;
    assert!(
        Keyframes::new(frames)
            .validate(0, 9 * KEYFRAME_EVERY)
            .is_err()
    );
}

#[test]
fn a_decoded_world_must_match_its_index_and_its_match() {
    let worlds = worlds(2 * KEYFRAME_EVERY);
    let origin = Scenario::skirmish().build().unwrap();
    let mut frames = chain(&worlds).frames().to_vec();
    frames[1].hash ^= 1;
    assert!(Keyframes::new(frames.clone()).decode(1, &origin).is_err());
    frames[1].hash ^= 1;
    frames[1].tick += KEYFRAME_EVERY;
    assert!(Keyframes::new(frames.clone()).decode(1, &origin).is_err());
    let mut other = Scenario::skirmish();
    other.mode = oxide_sim::scenario::ScenarioMode::Sandbox;
    for row in &mut other.map {
        *row = row.replace(['1', '2'], ".");
    }
    let other = other.build().unwrap();
    assert!(
        chain(&worlds).decode(0, &other).is_err(),
        "another match's rules"
    );
}

#[test]
fn the_recorder_keeps_each_grid_tick_once_and_hands_over_on_demand() {
    let worlds = worlds(3 * KEYFRAME_EVERY);
    let mut recorder = KeyframeRecorder::default();
    let mut state = Scenario::skirmish().build().unwrap();
    recorder.observe(&state);
    state.tick(&[]);
    recorder.observe(&state);
    assert_eq!(recorder.pending().wait().len(), 0, "only grid ticks");
    for world in &worlds {
        recorder.observe(world);
        recorder.observe(world);
    }
    let early = recorder.pending();
    recorder.observe(&worlds[0]);
    let kept = early.wait();
    assert_eq!(
        kept.ticks().collect::<Vec<_>>(),
        [KEYFRAME_EVERY, 2 * KEYFRAME_EVERY, 3 * KEYFRAME_EVERY]
    );
    let origin = Scenario::skirmish().build().unwrap();
    assert_eq!(kept.decode(2, &origin).unwrap().hash(), worlds[2].hash());
    let pending = recorder.pending();
    drop(recorder);
    assert_eq!(
        pending.wait().len(),
        3,
        "a finished match still hands them over"
    );
    assert!(u64::try_from(MAX_KEYFRAMES).unwrap() * KEYFRAME_EVERY >= crate::MAX_REPLAY_TICKS);
}

#[test]
fn rebuilt_keyframes_come_before_the_live_ones() {
    let worlds = worlds(4 * KEYFRAME_EVERY);
    let mut recorder = KeyframeRecorder::default();
    recorder.prepend(chain(&worlds[..3]));
    recorder.observe(&worlds[2]);
    recorder.observe(&worlds[3]);
    let kept = recorder.pending().wait();
    assert_eq!(
        kept.ticks().collect::<Vec<_>>(),
        [1, 2, 3, 4].map(|n| n * KEYFRAME_EVERY),
        "a live keyframe replaces a rebuilt one at its tick"
    );
    kept.validate(0, 4 * KEYFRAME_EVERY).unwrap();
    let origin = Scenario::skirmish().build().unwrap();
    for (index, world) in worlds.iter().enumerate() {
        assert_eq!(kept.decode(index, &origin).unwrap().hash(), world.hash());
    }
    assert_eq!(recorder.encoded().len(), 4);
}
