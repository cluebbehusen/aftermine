use super::*;
use crate::recording::WorldOrigin;
use crate::runner;
use oxide_sim::Command;
use oxide_sim::scenario::BotConfig;

const SAVED: u64 = 2_100;

fn build() -> BuildIdentity {
    BuildIdentity::default()
}

/// A bot duel played to [`SAVED`]: its scenario, world, and record.
fn played() -> (Scenario, State, GameReplay) {
    let mut scenario = Scenario::skirmish();
    crate::bench::all_bots(&mut scenario);
    let run = runner::run_scenario(&scenario, SAVED, true, Some(&build())).unwrap();
    let mut recorder = run.replay.unwrap();
    recorder.meta.ticks = None;
    (scenario, run.state, recorder)
}

/// The record a save without history starts at `saved`.
fn from_resume(scenario: &Scenario, saved: &State) -> GameReplay {
    GameReplay::with_origin(
        SIM_VERSION,
        build().label(),
        scenario.clone(),
        WorldOrigin::capture(scenario, saved).unwrap(),
    )
    .unwrap()
}

fn settled(pending: PendingHistory, recorder: &mut GameReplay, keyframes: &mut KeyframeRecorder) {
    pending.finish();
    assert!(pending.settle(recorder, keyframes).is_none());
}

#[test]
fn a_resumed_match_keeps_its_whole_record_and_rebuilds_its_keyframes() {
    let (scenario, saved, recorder) = played();
    assert!(!recorder.commands.is_empty(), "the bots must play");
    let history = SessionHistory::capture(&recorder, SAVED).unwrap();
    let mut resumed = history.recording(&scenario, &saved, &build()).unwrap();
    assert_eq!(resumed.start_tick(), 0);
    assert_eq!(
        serde_json::to_value(&resumed.commands).unwrap(),
        serde_json::to_value(&recorder.commands).unwrap()
    );

    let pending = PendingHistory::start(&resumed, &saved, from_resume(&scenario, &saved));
    resumed.record(
        SAVED,
        PlayerCommand {
            player: oxide_sim::PlayerId(0),
            command: Command::Surrender,
        },
    );
    let mut keyframes = KeyframeRecorder::default();
    settled(pending, &mut resumed, &mut keyframes);
    assert_eq!(resumed.start_tick(), 0, "a reproduced history stays");
    assert_eq!(resumed.commands.len(), recorder.commands.len() + 1);
    let rebuilt = keyframes.encoded();
    assert_eq!(rebuilt.ticks().collect::<Vec<_>>(), [1_024, 2_048]);
    let origin = scenario.build().unwrap();
    let mut straight = scenario.build().unwrap();
    let mut feed = crate::ReplayPlayback::new(&recorder);
    for (index, tick) in rebuilt.ticks().enumerate() {
        while straight.current_tick() < tick {
            feed.step(&mut straight);
        }
        assert_eq!(
            rebuilt.decode(index, &origin).unwrap().hash(),
            straight.hash()
        );
    }
}

#[test]
fn a_history_that_does_not_reproduce_the_save_gives_way_to_the_resume() {
    let (scenario, saved, mut recorder) = played();
    recorder.commands.retain(|timed| timed.tick < 600);
    let mut resumed = SessionHistory::capture(&recorder, SAVED)
        .unwrap()
        .recording(&scenario, &saved, &build())
        .expect("its shape is sound");
    let pending = PendingHistory::start(&resumed, &saved, from_resume(&scenario, &saved));
    let surrender = PlayerCommand {
        player: oxide_sim::PlayerId(0),
        command: Command::Surrender,
    };
    resumed.record(SAVED + 5, surrender);
    let mut finished = resumed.clone();
    finished.meta.kind = Some("match".into());
    let mut keyframes = KeyframeRecorder::default();
    pending
        .clone()
        .wait(&mut finished, &mut keyframes.pending());
    assert_eq!(
        finished.start_tick(),
        SAVED,
        "a save waits for the same check"
    );
    assert_eq!(
        finished.meta.kind.as_deref(),
        Some("match"),
        "and keeps the record's metadata"
    );
    settled(pending, &mut resumed, &mut keyframes);
    assert_eq!(
        resumed.start_tick(),
        SAVED,
        "the record restarts at the resume"
    );
    assert_eq!(resumed.commands.len(), 1, "keeping what was recorded since");
    assert_eq!(resumed.commands[0].tick, SAVED + 5);
    assert!(keyframes.encoded().is_empty());
}

#[test]
fn a_saved_history_must_fit_its_session() {
    let (scenario, saved, recorder) = played();
    let accepts = |change: &dyn Fn(&mut GameReplay, &mut Scenario)| {
        let mut recorder = recorder.clone();
        let mut scenario = scenario.clone();
        change(&mut recorder, &mut scenario);
        SessionHistory::capture(&recorder, SAVED)
            .unwrap()
            .recording(&scenario, &saved, &build())
    };
    accepts(&|_, _| {}).unwrap();
    let taken = accepts(&|recorder, scenario| {
        recorder.setup.players[0].bot = false;
        recorder.setup.players[0].bot_config = None;
        scenario.players[0].bot_config = Some(BotConfig::default());
    })
    .expect("a seat a bot took over later");
    assert!(!taken.setup.players[0].bot, "the record keeps how it began");
    assert!(accepts(&|recorder, _| recorder.setup.players.truncate(1)).is_err());
    assert!(accepts(&|recorder, _| recorder.setup.players[1].team = Some(7)).is_err());
    assert!(accepts(&|recorder, _| recorder.commands.last_mut().unwrap().tick = SAVED).is_err());
    assert!(accepts(&|recorder, _| recorder.commands[0].tick = 1_000_000).is_err());
    assert!(
        accepts(&|recorder, _| recorder.commands[0].command.player = oxide_sim::PlayerId(9))
            .is_err()
    );
    assert!(
        SessionHistory::capture(&from_resume(&scenario, &saved), SAVED).is_none(),
        "a record from a world has no history"
    );
    assert!(
        SessionHistory::capture(&recorder, crate::MAX_REPLAY_TICKS + 1).is_none(),
        "a match past the replay tick limit saves no history it could not load"
    );
}

#[test]
fn a_finished_match_waits_for_its_check_after_the_match_is_gone() {
    let (scenario, saved, recorder) = played();
    let mut finished = SessionHistory::capture(&recorder, SAVED)
        .unwrap()
        .recording(&scenario, &saved, &build())
        .unwrap();
    let pending = PendingHistory::start(&finished, &saved, from_resume(&scenario, &saved));
    let saving = pending.clone();
    drop(pending);
    let mut keyframes = KeyframeRecorder::default().pending();
    saving.wait(&mut finished, &mut keyframes);
    assert_eq!(finished.start_tick(), 0, "a reproduced history stays");
    assert_eq!(
        keyframes.wait().ticks().collect::<Vec<_>>(),
        [1_024, 2_048],
        "beside the keyframes it rebuilt"
    );
}
