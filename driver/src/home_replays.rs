//! Records the matches that play behind Home.

use aftermine_kit::build::BuildIdentity;
use aftermine_kit::keyframes::{KEYFRAME_EVERY, Keyframes};
use aftermine_kit::playback::Playback;
use aftermine_kit::{fights, runner, save_recording};
use anyhow::{Context, Result};
use std::path::Path;

/// One map of each team size and free-for-all shape Home shows.
pub const MAPS: [&str; 6] = [
    "highwall",
    "crosswinds",
    "trident-plateau",
    "derelict-belt",
    "crossroads",
    "eight-pits",
];

/// Ticks a scene plays from its checkpoint before Home starts it over: three
/// minutes.
const CLIP: u64 = 3_600;
/// The latest a scene's checkpoint can sit.
const LATEST: u64 = 16 * KEYFRAME_EVERY;

/// Records every scene into `out`. A fixed build label keeps a re-recording
/// byte-identical until the simulation or a bot changes.
pub fn record(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out)?;
    let build = BuildIdentity::default();
    for name in MAPS {
        let mut scenario = runner::load_scenario(&format!("scenarios/{name}.json"))?;
        aftermine_kit::bench::all_bots(&mut scenario);
        let replay = runner::run_scenario(&scenario, LATEST + CLIP, true, Some(&build))?
            .replay
            .context("a recorded run keeps its replay")?;
        let mut playback = Playback::load(replay)?;
        let mut best = None;
        let mut best_opposed = 0;
        let mut tick = KEYFRAME_EVERY;
        while tick <= LATEST {
            playback.seek(tick);
            if playback.state.result().is_some() {
                break;
            }
            let opposed = fights::fiercest(&playback.state).map_or(0, |fight| fight.opposed);
            if opposed > best_opposed {
                best_opposed = opposed;
                best = Some(tick);
            }
            tick += KEYFRAME_EVERY;
        }
        let checkpoint = best.with_context(|| format!("{name} never reaches a fight"))?;
        let replay = runner::run_scenario(&scenario, checkpoint + CLIP, true, Some(&build))?
            .replay
            .context("a recorded run keeps its replay")?;
        let mut playback = Playback::load(replay.clone())?;
        playback.seek(checkpoint);
        let keyframes = Keyframes::single(&playback.state)?;
        let path = out.join(format!("{name}.amreplay"));
        save_recording(&path, &replay, &keyframes)?;
        eprintln!(
            "{name}: starts at tick {checkpoint} with {best_opposed} opposed units, {} KB",
            std::fs::metadata(&path)?.len() / 1024
        );
    }
    Ok(())
}
