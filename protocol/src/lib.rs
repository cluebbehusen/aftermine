#![doc = include_str!("../README.md")]

pub mod framing;
pub mod input;
pub mod session;
pub mod view;

use aftermine_sim::{Command, Event, PlayerId};
use serde::{Deserialize, Serialize};

pub use input::{Key, MouseButton, RawEvent};
pub use session::{DebugSession, check_speed, dispatch_shared};
pub use view::{
    BuildingView, CameraView, FogView, GhostView, PlayerView, RememberedTileView, StateFilter,
    StateView, StatusView, UiView, UnitView,
};

/// Default TCP port for `--debug-server`.
pub const DEFAULT_PORT: u16 = 4123;

/// Largest fast-forward request the shell will execute in one operation.
pub const MAX_ADVANCE_TICKS: u64 = 1_000_000;

/// Largest presentation-preserving step the shell will execute at once.
pub const MAX_PRESENT_TICKS: u64 = 120;

/// Ticks per second both sides budget for a synchronous advance, set
/// conservatively so a slow machine still finishes inside the deadline. The
/// client derives its read timeout and the server its reply deadline from
/// this one figure, so the two agree about a legal wait.
pub const ADVANCE_TICKS_PER_BUDGET_SECOND: u64 = 1_000;

/// Longest request line a server accepts, newline excluded. A longer line
/// is answered with an error naming the limit and the connection is closed,
/// so framing bounds its own allocation.
pub const MAX_FRAME_BYTES: usize = 1 << 20;

/// Longest response line a client should accept, newline excluded. Replies
/// scale with the world (a deep `query_state`, a long presented-event
/// drain), so this is far larger than [`MAX_FRAME_BYTES`].
pub const MAX_RESPONSE_BYTES: usize = 64 << 20;

/// Everything a client can ask of a running shell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "method",
    content = "params",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Request {
    /// Who are you, what tick is it, are we paused.
    Status,
    /// A structured snapshot of sim state, filterable by section.
    QueryState {
        /// Which sections to include.
        #[serde(default)]
        filter: StateFilter,
    },
    /// The world as one seat knows it: its own economy and command
    /// eligibility, visibility mask, entities filtered to current sight,
    /// ghost memories, remembered salvage, and radar contacts. The fog-honest
    /// counterpart to the omniscient [`Request::QueryState`].
    QueryFogView {
        /// The seat whose knowledge to report.
        player: PlayerId,
    },
    /// Camera position, zoom, and visible world rectangle.
    QueryCamera,
    /// Shell mode and active menu state.
    QueryUi,
    /// Timing summary from the real window's opt-in frame profiler. A
    /// windowless session refuses this instead of reporting CPU-renderer
    /// timings as though they described the native shell.
    QueryPerformance {
        /// Clear samples after taking the snapshot.
        #[serde(default)]
        reset: bool,
    },
    /// Arm an exact native Playing-screen profile window. The current visible
    /// tick must equal `from_tick`; the shell records every active Playing
    /// frame while the live match spans the window and auto-pauses on `to_tick`.
    BeginPerformanceWindow {
        /// Exact live tick at which measurement begins.
        from_tick: u64,
        /// Exact live tick at which measurement ends and the shell pauses.
        to_tick: u64,
    },
    /// Run a scripted window of native frames on whatever the window shows:
    /// Home, the live match, or the viewer. Samples collected so far are
    /// cleared. Poll [`Request::QueryPerformance`] for its completion.
    RunFrameScript {
        /// What to run.
        script: FrameScript,
    },
    /// The canonical state fingerprint at the current tick.
    StateHash,
    /// Run sim ticks now (bots included), regardless of pause state, then
    /// report the resulting tick and hash. Requests above
    /// [`MAX_ADVANCE_TICKS`] are capped; the reply's `ticks` field reports
    /// what actually ran.
    AdvanceTicks {
        /// How many ticks to run.
        ticks: u64,
    },
    /// Run a small number of ticks without suppressing presentation, then
    /// return every sim event produced. This is the deterministic way for
    /// an agent to observe command rejection, shots, deaths, and transient
    /// shell feedback. Requests above [`MAX_PRESENT_TICKS`] are capped.
    PresentTicks {
        /// How many ticks to run.
        ticks: u64,
    },
    /// Stop the wall clock driving the sim. Rendering continues.
    Pause,
    /// Resume wall-clock ticking.
    Resume,
    /// Scale wall-clock time (2.0 = double speed). Sim ticks are unchanged
    /// in size; they just fire more or less often.
    SetSpeed {
        /// Multiplier applied to real time.
        multiplier: f64,
    },
    /// Issue a game command as `player`, stamped for the next tick through
    /// the same path mouse clicks use.
    SendCommand {
        /// Acting player.
        player: PlayerId,
        /// The command.
        command: Command,
    },
    /// Push a synthetic input event into the shell's funnel,
    /// indistinguishable from hardware input.
    InjectEvent {
        /// The event.
        event: RawEvent,
    },
    /// Write the current frame to a PNG and return its path.
    Screenshot {
        /// Target path; defaults to `screenshots/tick-N.png`.
        #[serde(default)]
        path: Option<String>,
    },
    /// Toggle the debug overlay (grid, ids, paths, hp).
    ToggleOverlay,
    /// Replace the current match with a scenario file.
    LoadScenario {
        /// Path to a scenario JSON.
        path: String,
    },
    /// Resume a session from a replay file: rebuild its scenario, re-run
    /// every recorded tick, and keep recording from there.
    LoadReplay {
        /// Path to a replay JSON.
        path: String,
    },
    /// Write the session so far as a replay JSON.
    SaveReplay {
        /// Target path.
        path: String,
    },
}

/// Successful response payloads, tagged by `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    /// Generic acknowledgement.
    Ok,
    /// Answer to [`Request::Status`].
    Status(StatusView),
    /// Answer to [`Request::QueryState`].
    State(StateView),
    /// Answer to [`Request::QueryFogView`].
    Fog(FogView),
    /// Answer to [`Request::QueryCamera`].
    Camera(CameraView),
    /// Answer to [`Request::QueryUi`].
    Ui(UiView),
    /// Answer to [`Request::QueryPerformance`].
    Performance(FrameProfileView),
    /// Answer to [`Request::StateHash`].
    Hash(HashView),
    /// Answer to [`Request::AdvanceTicks`].
    Advanced(AdvancedView),
    /// Answer to [`Request::PresentTicks`].
    Presented(PresentedView),
    /// Answer to [`Request::Screenshot`].
    Screenshot(ScreenshotView),
    /// Answer to [`Request::ToggleOverlay`].
    Overlay(OverlayView),
    /// Answer to [`Request::SaveReplay`].
    Saved(SavedView),
}

/// Tick + fingerprint pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HashView {
    /// Current tick.
    pub tick: u64,
    /// State hash as `0x`-prefixed hex.
    pub hash: String,
}

/// Result of a fast-forward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvancedView {
    /// Ticks actually run.
    pub ticks: u64,
    /// Tick counter afterwards.
    pub tick: u64,
    /// State hash afterwards, as hex.
    pub hash: String,
}

/// Result of a presentation-preserving step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PresentedView {
    /// Ticks actually run.
    pub ticks: u64,
    /// Tick counter afterwards.
    pub tick: u64,
    /// State hash afterwards, as hex.
    pub hash: String,
    /// Events emitted across the interval, in tick and event order.
    pub events: Vec<Event>,
}

/// A frame-exact profiling workload. Each frame advances a fixed slice of
/// time, so the same frames get drawn on any machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameScript {
    /// Unmeasured frames after the camera is framed, so caches catch up. A zoom
    /// settles before every repeat.
    pub settle_frames: u32,
    /// Whether the live match plays during measured frames. It pauses again
    /// for settling and when the script ends, so every run ticks identically.
    #[serde(default)]
    pub plays: bool,
    /// What the camera does while frames are measured.
    pub motion: CameraMotion,
}

/// The camera's part in a [`FrameScript`]. Positions are world tiles and zooms
/// are logical pixels per tile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraMotion {
    /// Leave the camera alone, for a screen that doesn't use it.
    None {
        /// Frames to measure.
        frames: u32,
    },
    /// Hold one view.
    Hold {
        /// World point at the view's center.
        center: [f64; 2],
        /// Zoom to hold.
        zoom: f64,
        /// Frames to measure.
        frames: u32,
    },
    /// Pan steadily along the map's width, turning back at its edges.
    Pan {
        /// World point to start from.
        center: [f64; 2],
        /// Zoom to hold.
        zoom: f64,
        /// Pan speed.
        tiles_per_second: f64,
        /// Frames to measure.
        frames: u32,
    },
    /// Zoom from one level to another through the player's zoom glide,
    /// measuring each glide and the frames after it lands.
    Zoom {
        /// World point framed before each glide.
        center: [f64; 2],
        /// Zoom each glide starts from.
        from: f64,
        /// Zoom each glide lands on.
        to: f64,
        /// How many glides to run.
        repeats: u32,
        /// Frames measured after each landing, to cover re-baking terrain.
        rest_frames: u32,
    },
}

/// Distribution summary for one frame-time measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimingSummary {
    /// Arithmetic mean.
    pub mean_ms: f64,
    /// Median.
    pub p50_ms: f64,
    /// 95th percentile.
    pub p95_ms: f64,
    /// 99th percentile.
    pub p99_ms: f64,
    /// Largest sample.
    pub max_ms: f64,
}

/// The slowest completed frame in a native-shell timing window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlowFrameView {
    /// Active screen while the frame ran.
    pub mode: String,
    /// Visible simulation tick at frame start.
    pub tick_start: u64,
    /// Visible simulation tick after the frame's update.
    pub tick_end: u64,
    /// CPU work from frame entry up to the final flush of queued draws.
    pub work_ms: f64,
    /// CPU time spent submitting the draws still queued at the end of the
    /// frame.
    pub submit_ms: f64,
    /// Where `work_ms` went, in the shell's pass order, then `other`.
    pub passes: Vec<PassCostView>,
    /// Units in the visible world after the update.
    pub units: usize,
    /// Buildings in the visible world after the update.
    pub buildings: usize,
}

/// One named slice of a single frame's work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PassCostView {
    /// Stable pass name.
    pub name: String,
    /// Milliseconds the pass took in this frame.
    pub ms: f64,
}

/// One named slice of frame work across a sample window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PassTimingView {
    /// Stable pass name.
    pub name: String,
    /// Per-frame distribution, counting frames where the pass took no time.
    pub timing: TimingSummary,
}

/// Bounded timing snapshot collected by a real GPU-backed shell window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameProfileView {
    /// Always `"gpu"`; the field prevents accidental comparison with the
    /// headless session's schematic CPU renderer.
    pub renderer: String,
    /// Completed frames represented by the summary.
    pub frames: usize,
    /// First visible tick in the sample window.
    pub tick_start: u64,
    /// Last visible tick in the sample window.
    pub tick_end: u64,
    /// Simulation ticks presented across the sampled frames.
    pub ticks_presented: u64,
    /// CPU work per frame, from frame entry up to the final flush of queued
    /// draws. Excludes `submit` and everything inside `next_frame`.
    pub work: TimingSummary,
    /// CPU time per frame submitting the draws still queued at frame end.
    /// Flushes forced earlier in a frame count toward the pass that forced
    /// them.
    pub submit: TimingSummary,
    /// `work` plus `submit`, the CPU's share of a frame's budget.
    pub cpu: TimingSummary,
    /// Where `work` went, in the shell's pass order, then `other`. Pass times
    /// are mostly queueing: a pass that changes camera or render target also
    /// pays to submit what earlier passes queued.
    pub passes: Vec<PassTimingView>,
    /// Wall intervals between consecutive sampled frame starts.
    pub interval: TimingSummary,
    /// Frames whose `cpu` time exceeded a 120 Hz budget.
    pub cpu_over_8_3_ms: usize,
    /// Frames whose CPU work exceeded a 60 Hz budget.
    pub work_over_16_7_ms: usize,
    /// Frames whose CPU work exceeded two 60 Hz budgets.
    pub work_over_33_3_ms: usize,
    /// Largest `cpu` frame, absent when no completed frame was sampled.
    pub slowest: Option<SlowFrameView>,
    /// Exact-window metadata when profiling was armed through
    /// [`Request::BeginPerformanceWindow`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<FrameProfileWindowView>,
    /// Whether the [`Request::RunFrameScript`] these samples belong to has
    /// finished; absent when no script ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_complete: Option<bool>,
}

/// State of one shell-side exact profile window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameProfileWindowView {
    /// Requested first tick.
    pub from_tick: u64,
    /// Requested exclusive end tick.
    pub to_tick: u64,
    /// Whether the shell reached `to_tick` and auto-paused.
    pub complete: bool,
    /// Wall time from the first sampled frame's start through the final
    /// sampled frame's completed CPU work.
    pub elapsed_ms: f64,
    /// Whether bounded retention evicted any sampled frame.
    pub truncated: bool,
}

/// Where a screenshot landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenshotView {
    /// PNG path (relative to the server's working directory unless absolute).
    pub path: String,
    /// Pixel width.
    pub width: u32,
    /// Pixel height.
    pub height: u32,
    /// Which renderer produced the pixels: `"gpu"` is the shell's real
    /// frame, `"cpu"` the headless session's schematic tiny-skia render.
    /// An agent judging visual polish must know which one it is reading.
    #[serde(default = "gpu_renderer")]
    pub renderer: String,
}

fn gpu_renderer() -> String {
    "gpu".to_string()
}

/// Overlay toggle result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayView {
    /// Whether the overlay is now on.
    pub enabled: bool,
}

/// Where a replay landed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedView {
    /// Replay path.
    pub path: String,
    /// Commands recorded so far.
    pub commands: usize,
}

/// A request with its correlation id.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RequestEnvelope {
    /// Client-chosen id, echoed in the response.
    pub id: u64,
    /// The request itself.
    #[serde(flatten)]
    pub request: Request,
}

#[derive(Default)]
enum WireParams {
    #[default]
    Missing,
    Present(serde_json::Value),
}

impl<'de> Deserialize<'de> for WireParams {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde_json::Value::deserialize(deserializer).map(Self::Present)
    }
}

impl<'de> Deserialize<'de> for RequestEnvelope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireEnvelope {
            id: u64,
            method: String,
            #[serde(default)]
            params: WireParams,
        }

        let raw = WireEnvelope::deserialize(deserializer)?;
        let mut request = serde_json::Map::from_iter([(
            "method".to_string(),
            serde_json::Value::String(raw.method),
        )]);
        if let WireParams::Present(params) = raw.params {
            request.insert("params".to_string(), params);
        }
        let request =
            serde_json::from_value(serde_json::Value::Object(request)).map_err(D::Error::custom)?;
        Ok(Self {
            id: raw.id,
            request,
        })
    }
}

/// A response with its correlation id. The outcome is an enum, so "both ok
/// and err" or "neither" are unrepresentable; on the wire it is
/// `{"id":…,"ok":{…}}` or `{"id":…,"err":"…"}`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseEnvelope {
    /// Echo of the request id (0 when the request was unparseable).
    pub id: u64,
    outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq)]
enum Outcome {
    Ok(Box<Reply>),
    Err(String),
}

impl ResponseEnvelope {
    /// A success response.
    pub fn ok(id: u64, reply: Reply) -> Self {
        Self {
            id,
            outcome: Outcome::Ok(Box::new(reply)),
        }
    }

    /// A failure response.
    pub fn err(id: u64, message: impl Into<String>) -> Self {
        Self {
            id,
            outcome: Outcome::Err(message.into()),
        }
    }

    /// Consumes the envelope into the reply or the error message.
    pub fn into_result(self) -> Result<Reply, String> {
        match self.outcome {
            Outcome::Ok(reply) => Ok(*reply),
            Outcome::Err(message) => Err(message),
        }
    }
}

impl Serialize for ResponseEnvelope {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let mut s = serializer.serialize_struct("ResponseEnvelope", 2)?;
        s.serialize_field("id", &self.id)?;
        match &self.outcome {
            Outcome::Ok(reply) => s.serialize_field("ok", reply)?,
            Outcome::Err(message) => s.serialize_field("err", message)?,
        }
        s.end()
    }
}

impl<'de> Deserialize<'de> for ResponseEnvelope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: u64,
            #[serde(default)]
            ok: Option<Reply>,
            #[serde(default)]
            err: Option<String>,
        }
        let raw = Raw::deserialize(deserializer)?;
        match (raw.ok, raw.err) {
            (Some(reply), None) => Ok(ResponseEnvelope::ok(raw.id, reply)),
            (None, Some(message)) => Ok(ResponseEnvelope::err(raw.id, message)),
            (Some(_), Some(_)) => Err(serde::de::Error::custom("response has both ok and err")),
            (None, None) => Err(serde::de::Error::custom("response has neither ok nor err")),
        }
    }
}

/// Formats a state hash the way the protocol expects it.
pub fn hash_hex(hash: u64) -> String {
    format!("{hash:#018x}")
}

#[cfg(test)]
mod tests;
