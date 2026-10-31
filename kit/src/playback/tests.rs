use super::*;
use crate::keyframes::{KEYFRAME_EVERY, KeyframeRecorder};
use crate::runner;
use aftermine_sim::Scenario;

fn recorded_match() -> GameReplay {
    let mut scenario = Scenario::skirmish();
    crate::bench::all_bots(&mut scenario);
    runner::run_scenario(
        &scenario,
        900,
        true,
        Some(&crate::build::BuildIdentity::default()),
    )
    .unwrap()
    .replay
    .unwrap()
}

/// The 900-tick match held open to `ticks`, with the keyframes a live
/// recorder keeps along the way.
fn with_keyframes(mut replay: GameReplay, ticks: u64) -> Recording {
    replay.meta.ticks = Some(ticks);
    let mut straight = Playback::load(replay.clone()).unwrap();
    let mut recorder = KeyframeRecorder::default();
    while !straight.at_end() {
        straight.advance(1);
        recorder.observe(&straight.state);
    }
    Recording {
        replay,
        keyframes: recorder.pending().wait(),
    }
}

fn straight_hash(replay: &GameReplay, tick: u64) -> u64 {
    let mut straight = Playback::load(replay.clone()).unwrap();
    straight.seek(tick);
    straight.state.hash()
}

#[test]
fn a_budgeted_seek_lands_bit_identical_to_a_straight_one() {
    // The slices must be invisible: however many frames a seek is
    // spread across, the state it lands on is the state one big
    // seek produces.
    let replay = recorded_match();
    let mut straight = Playback::load(replay.clone()).unwrap();
    straight.seek(333);
    let expected = straight.state.hash();

    let mut sliced = Playback::load(replay).unwrap();
    let mut slices = 0;
    while !sliced.seek_step(333, 50) {
        slices += 1;
        assert!(slices < 100, "the budget must make progress");
    }
    assert_eq!(sliced.position(), 333);
    assert_eq!(sliced.state.hash(), expected, "slices changed the state");
    assert!(slices >= 5, "a 50-tick budget takes several frames");
}

#[test]
fn a_seeked_position_matches_the_straight_run() {
    let replay = recorded_match();
    let mut straight = Playback::load(replay.clone()).unwrap();
    straight.advance(700);
    let truth = straight.state.hash();

    let mut seeker = Playback::load(replay).unwrap();
    seeker.advance(900);
    assert!(seeker.at_end());
    seeker.seek(700); // backward, from the origin
    assert_eq!(seeker.position(), 700);
    assert_eq!(
        seeker.state.hash(),
        truth,
        "a seek is a re-simulation, not an approximation"
    );
    seeker.seek(123);
    seeker.seek(700); // and forward again
    assert_eq!(seeker.state.hash(), truth);
}

#[test]
fn a_seek_restores_the_stored_keyframe_before_its_target() {
    let recording = with_keyframes(recorded_match(), 5_000);
    assert_eq!(
        recording.keyframes.ticks().collect::<Vec<_>>(),
        [1_024, 2_048, 3_072, 4_096]
    );
    let replay = recording.replay.clone();
    let mut playback = Playback::open(recording).unwrap();
    for target in [4_100, 0, 2_048, 3_000, 1_024, 5_000, 4_096, u64::MAX] {
        let clamped = target.min(5_000);
        let restored = (clamped / KEYFRAME_EVERY) * KEYFRAME_EVERY;
        let before = playback.position();
        let reached = playback.seek_step(target, 0);
        if clamped < before || restored > before {
            assert_eq!(
                playback.position(),
                restored,
                "a zero budget only restores, landing on the keyframe"
            );
        }
        assert_eq!(reached, playback.position() == clamped);
        playback.seek(target);
        assert_eq!(playback.position(), clamped);
        assert_eq!(playback.state.hash(), straight_hash(&replay, clamped));
        assert_eq!(
            playback.next_cmd,
            replay.commands.partition_point(|c| c.tick < clamped),
            "a restore never replays a command twice"
        );
    }
}

#[test]
fn the_fill_keeps_worlds_a_seek_restores_nearby() {
    let recording = with_keyframes(recorded_match(), 5_000);
    let replay = recording.replay.clone();
    let mut playback = Playback::open(recording).unwrap();
    playback.start_fill();
    playback.finish_fill();
    let spacing = fill_spacing(5_000);
    assert_eq!(spacing, FILL_EVERY);
    for target in [4_999, 130, 2_500, 1_024, 700] {
        playback.seek_step(target, 0);
        assert_eq!(playback.position(), target / spacing * spacing);
        playback.seek(target);
        assert_eq!(playback.state.hash(), straight_hash(&replay, target));
    }
    assert!(playback.stored_trusted(), "honest keyframes stay in use");
}

#[test]
fn keyframes_that_disagree_with_the_log_are_dropped_once_the_fill_checks_them() {
    // Keyframes from another history of the same match: valid worlds at
    // the right ticks, but not the worlds these commands produce.
    let honest = recorded_match();
    let mut other = honest.clone();
    other.commands.clear();
    let forged = with_keyframes(other, 5_000).keyframes;
    let mut replay = honest;
    replay.meta.ticks = Some(5_000);
    let truth = straight_hash(&replay, 2_048);
    let mut playback = Playback::open(Recording {
        replay,
        keyframes: forged,
    })
    .unwrap();
    playback.seek(2_048);
    assert_ne!(
        playback.state.hash(),
        truth,
        "unchecked, a stored keyframe is believed"
    );
    playback.start_fill();
    playback.finish_fill();
    assert!(!playback.stored_trusted());
    playback.seek(0);
    playback.seek(2_048);
    assert_eq!(playback.state.hash(), truth, "the log is authoritative");
}

#[test]
fn a_keyframe_that_fails_to_decode_falls_back_to_the_log() {
    let mut recording = with_keyframes(recorded_match(), 3_000);
    let mut frames = recording.keyframes.frames().to_vec();
    let mut bytes = frames[1].bytes.to_vec();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    frames[1].bytes = bytes.into();
    recording.keyframes = Keyframes::new(frames);
    let replay = recording.replay.clone();
    let mut playback = Playback::open(recording).unwrap();
    playback.seek(2_100);
    assert_eq!(playback.state.hash(), straight_hash(&replay, 2_100));
    assert!(
        playback.stored.is_empty(),
        "the damaged record's keyframes are forgotten"
    );
}

#[test]
fn keyframes_outside_the_record_are_refused() {
    let recording = with_keyframes(recorded_match(), 3_000);
    let mut shorter = recording.replay.clone();
    shorter.meta.ticks = Some(2_000);
    assert!(
        Playback::open(Recording {
            replay: shorter,
            keyframes: recording.keyframes,
        })
        .is_err()
    );
}

#[test]
fn fill_memory_is_bounded_at_every_length() {
    for span in [0, 900, 40_000, 500_000, 2_000_000] {
        let spacing = fill_spacing(span);
        assert!(spacing >= FILL_EVERY);
        assert!(spacing.is_power_of_two());
        assert!(KEYFRAME_EVERY.is_multiple_of(FILL_EVERY));
        assert!(
            span / spacing <= MAX_FILLED,
            "{span} ticks at spacing {spacing} keeps too many worlds"
        );
    }
}

#[test]
fn an_absurd_claimed_length_is_refused_interactively() {
    let mut replay = recorded_match();
    replay.meta.ticks = Some(1_000_000_000);
    assert!(
        Playback::load(replay).is_err(),
        "a billion-tick claim must not hang the first End press"
    );
}

#[test]
fn end_home_end_replays_the_same_bytes() {
    let replay = recorded_match();
    let mut pb = Playback::load(replay).unwrap();
    pb.advance(900);
    let full = pb.state.hash();
    pb.seek(0);
    assert_eq!(pb.position(), 0);
    pb.seek(900);
    assert_eq!(
        pb.state.hash(),
        full,
        "the round trip lands on the same bytes"
    );
}

#[test]
fn playback_never_outruns_the_record() {
    let replay = recorded_match();
    let mut pb = Playback::load(replay).unwrap();
    pb.advance(5_000);
    assert_eq!(pb.position(), 900, "the record ends where it ends");
    pb.seek(2_000);
    assert_eq!(pb.position(), 900);
}

#[test]
fn missing_duration_uses_the_tick_after_the_last_command() {
    let mut inferred = recorded_match();
    inferred.meta.ticks = None;
    let total = inferred
        .commands
        .last()
        .expect("the bot fixture issues commands")
        .tick
        + 1;

    let mut explicit = inferred.clone();
    explicit.meta.ticks = Some(total);
    let mut inferred_playback = Playback::load(inferred).unwrap();
    let mut explicit_playback = Playback::load(explicit).unwrap();
    assert_eq!(inferred_playback.total(), total);

    inferred_playback.seek(u64::MAX);
    explicit_playback.seek(total);
    assert_eq!(inferred_playback.position(), total);
    assert_eq!(
        inferred_playback.state.hash(),
        explicit_playback.state.hash(),
        "legacy records without duration reproduce the same inferred span"
    );
}
