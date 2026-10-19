#![doc = include_str!("../README.md")]

pub mod bench;
pub mod bot_execution;
pub mod build;
pub mod checkpoint;
mod compression;
pub mod controller;
pub mod history;
pub mod keyframes;
pub mod perceptual;
pub mod playback;
pub mod recording;
pub mod render;
mod replay;
pub mod runner;
pub mod stats;
pub mod world_snapshot;

use oxide_sim::{PlayerCommand, Scenario};

/// Upper bound on the ticks a replay may span before loading or running it
/// is refused (about 28 game-hours). A syntactically valid file can claim an
/// absurd duration and freeze a UI for minutes. The headless driver may opt
/// out.
pub const MAX_REPLAY_TICKS: u64 = 2_000_000;

/// The concrete session replay type every Oxide surface records,
/// saves, and replays.
pub type GameReplay = chassis::replay::Replay<Scenario, PlayerCommand, recording::WorldOrigin>;

pub use replay::{
    Recording, ReplayPlayback, bounded_replay_duration, load_recording, load_replay,
    replay_duration, save_recording,
};

pub mod diagnostics;
