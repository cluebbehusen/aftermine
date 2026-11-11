//! Opt-in native-frame timing for the live GPU shell.

use crate::camera::Camera;
use crate::frame_script::{FrameScriptRun, Step};
use aftermine_protocol::{
    FrameProbes, FrameProfileView, FrameProfileWindowView, FrameScript, PassCostView,
    PassDrawCallsView, PassTimingView, SlowFrameView, TimingSummary,
};
use macroquad::prelude::{Texture2D, set_default_camera};
use std::cell::Cell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const MAX_SAMPLES: usize = 65_536;

/// The report's name for frame code outside any named pass.
const OTHER_PASS: &str = "other";

/// One frame's budget on a 120 Hz display.
const BUDGET_120_HZ_MS: f64 = 1000.0 / 120.0;

/// A named slice of a frame's work, in draw order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FramePass {
    Sim,
    TerrainBake,
    Terrain,
    Salvage,
    GroundEffects,
    Structures,
    Units,
    Markers,
    Effects,
    Fog,
    WorldMarks,
    Hud,
    Minimap,
    Menu,
}

const PASSES: usize = 14;

impl FramePass {
    const ALL: [Self; PASSES] = [
        Self::Sim,
        Self::TerrainBake,
        Self::Terrain,
        Self::Salvage,
        Self::GroundEffects,
        Self::Structures,
        Self::Units,
        Self::Markers,
        Self::Effects,
        Self::Fog,
        Self::WorldMarks,
        Self::Hud,
        Self::Minimap,
        Self::Menu,
    ];

    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|pass| pass.name() == name)
    }

    /// The passes a frame can't do without: skipping the sim would stop time
    /// and the viewer's camera, and skipping the bake would redraw terrain live.
    const fn skippable(self) -> bool {
        !matches!(self, Self::Sim | Self::TerrainBake)
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Sim => "sim",
            Self::TerrainBake => "terrain_bake",
            Self::Terrain => "terrain",
            Self::Salvage => "salvage",
            Self::GroundEffects => "ground_effects",
            Self::Structures => "structures",
            Self::Units => "units",
            Self::Markers => "markers",
            Self::Effects => "effects",
            Self::Fog => "fog",
            Self::WorldMarks => "world_marks",
            Self::Hud => "hud",
            Self::Minimap => "minimap",
            Self::Menu => "menu",
        }
    }
}

thread_local! {
    static PASS_TIMING: Cell<bool> = const { Cell::new(false) };
    static PASS_SPENT: Cell<[f64; PASSES]> = const { Cell::new([0.0; PASSES]) };
    /// One bit per [`FramePass`] to leave undrawn.
    static PASS_SKIP: Cell<u32> = const { Cell::new(0) };
    /// Set for the frame macroquad is capturing draw calls from.
    static DRAW_CAPTURE: Cell<bool> = const { Cell::new(false) };
    /// Captured draw calls already charged to a pass.
    static DRAW_SEEN: Cell<usize> = const { Cell::new(0) };
    static DRAW_COUNTS: Cell<[DrawCount; PASSES]> = const {
        Cell::new([DrawCount { draw_calls: 0, indices: 0 }; PASSES])
    };
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DrawCount {
    draw_calls: usize,
    indices: usize,
}

/// Measured frames before the one whose draw calls get captured. Early enough
/// that it falls inside a script's first zoom glide.
const FRAMES_BEFORE_CAPTURE: u32 = 30;

/// Captured draw calls that no pass has claimed yet.
fn unclaimed_draw_calls() -> DrawCount {
    let captured = macroquad::telemetry::drawcalls();
    let seen = DRAW_SEEN.replace(captured.len());
    let fresh = captured.get(seen..).unwrap_or_default();
    DrawCount {
        draw_calls: fresh.len(),
        indices: fresh.iter().map(|call| call.indices_count).sum(),
    }
}

/// Runs `work`, charging its time to `which` when profiling is on. Passes must
/// not nest, or the inner one counts twice. Off never reads the clock.
pub(crate) fn measure<R>(which: FramePass, work: impl FnOnce() -> R) -> R {
    if !PASS_TIMING.get() {
        return work();
    }
    let started = Instant::now();
    let result = work();
    let mut spent = PASS_SPENT.get();
    spent[which as usize] += started.elapsed().as_secs_f64() * 1000.0;
    PASS_SPENT.set(spent);
    if DRAW_CAPTURE.get() {
        // Submitting now is what ties each captured draw call to this pass.
        set_default_camera();
        let fresh = unclaimed_draw_calls();
        let mut counts = DRAW_COUNTS.get();
        counts[which as usize].draw_calls += fresh.draw_calls;
        counts[which as usize].indices += fresh.indices;
        DRAW_COUNTS.set(counts);
    }
    result
}

/// [`measure`] for drawing that a profiling run may leave out.
pub(crate) fn pass(which: FramePass, draw: impl FnOnce()) {
    if PASS_SKIP.get() & (1 << which as u32) == 0 {
        measure(which, draw);
    }
}

/// Copies one pixel of the frame and reads it back. On a tile-based GPU that
/// can't finish until the whole frame has rendered. A readback from a separate
/// render target doesn't work: it returns without waiting for the frame.
struct GpuFence(Texture2D);

impl GpuFence {
    fn new() -> Self {
        Self(Texture2D::from_rgba8(1, 1, &[0; 4]))
    }

    /// Milliseconds spent waiting for the GPU.
    fn wait(&self) -> f64 {
        let started = Instant::now();
        self.0.grab_screen();
        drop(self.0.get_texture_data());
        started.elapsed().as_secs_f64() * 1000.0
    }
}

#[derive(Debug, Clone)]
struct FrameSample {
    mode: String,
    tick_start: u64,
    tick_end: u64,
    started_at: Instant,
    work_ms: f64,
    submit_ms: f64,
    gpu_wait_ms: Option<f64>,
    passes: [f64; PASSES],
    units: usize,
    buildings: usize,
    /// Unmeasured frames ran since the previous sample, so the wall time
    /// between them isn't a frame interval.
    after_gap: bool,
}

impl FrameSample {
    fn cpu_ms(&self) -> f64 {
        self.work_ms + self.submit_ms
    }

    /// Frame code outside any named pass: input, UI logic and bookkeeping.
    fn other_ms(&self) -> f64 {
        (self.work_ms - self.passes.iter().sum::<f64>()).max(0.0)
    }
}

/// Input captured at the native frame boundary.
#[derive(Clone, Copy)]
pub(crate) struct FrameObservation<'a> {
    pub(crate) mode: &'a str,
    pub(crate) active_playing: bool,
    pub(crate) tick_start: u64,
    pub(crate) tick_end: u64,
    pub(crate) work_ms: f64,
    pub(crate) submit_ms: f64,
    pub(crate) units: usize,
    pub(crate) buildings: usize,
}

/// Bounded collector enabled only by `--profile-frames`.
pub(crate) struct FrameProfiler {
    enabled: bool,
    samples: VecDeque<FrameSample>,
    window: Option<ProfileWindow>,
    script: Option<ScriptState>,
    /// This frame's wait for the GPU, between measuring it and recording it.
    pending_gpu_wait: Option<f64>,
    /// The captured frame's draw calls.
    draw_calls: Vec<PassDrawCallsView>,
}

/// Where a script stands on capturing a frame's draw calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawCapture {
    /// Not asked for, or already finished.
    Off,
    /// Counting down measured frames.
    Waiting { left: u32 },
    /// Macroquad starts capturing when this frame ends.
    Requested,
    /// This frame is being captured.
    Capturing,
    /// The frame after, which inherits the capture's GPU backlog.
    Recovering,
}

/// A script underway or finished. A finished one stays until its samples are
/// reset, so the report can say it completed.
struct ScriptState {
    /// `None` once the script has finished.
    run: Option<FrameScriptRun>,
    measuring: bool,
    /// Unmeasured frames ran since the last sample.
    gap: bool,
    capture: DrawCapture,
    /// Present when the script times waiting for the GPU.
    fence: Option<GpuFence>,
    clock: ScriptClock,
}

/// What a script is doing with the live match's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptClock {
    /// The script leaves it alone.
    Untouched,
    /// Paused until measuring starts.
    Held,
    /// Playing while frames are measured.
    Playing,
}

struct ProfileWindow {
    from_tick: u64,
    to_tick: u64,
    next_tick: u64,
    started_at: Option<Instant>,
    elapsed_ms: f64,
    complete: bool,
    truncated: bool,
    start_barrier: bool,
}

impl FrameProfiler {
    pub(crate) fn new(enabled: bool) -> Self {
        PASS_TIMING.set(enabled);
        PASS_SPENT.take();
        Self {
            enabled,
            samples: VecDeque::new(),
            window: None,
            script: None,
            pending_gpu_wait: None,
            draw_calls: Vec::new(),
        }
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) fn arm(&mut self, from_tick: u64, to_tick: u64) -> Result<(), String> {
        if !self.enabled {
            return Err(
                "native frame profiling is disabled; launch the shell with --profile-frames"
                    .to_string(),
            );
        }
        if to_tick <= from_tick {
            return Err("profile window end must be greater than its start".to_string());
        }
        self.samples.clear();
        self.script = None;
        PASS_SKIP.set(0);
        self.window = Some(ProfileWindow {
            from_tick,
            to_tick,
            next_tick: from_tick,
            started_at: None,
            elapsed_ms: 0.0,
            complete: false,
            truncated: false,
            start_barrier: true,
        });
        Ok(())
    }

    /// Starts a frame script, replacing any samples, window or earlier script.
    pub(crate) fn begin_script(
        &mut self,
        script: &FrameScript,
        probes: &FrameProbes,
    ) -> Result<(), String> {
        if !self.enabled {
            return Err(
                "native frame profiling is disabled; launch the shell with --profile-frames"
                    .to_string(),
            );
        }
        let skip = skip_mask(&probes.skip_passes)?;
        self.samples.clear();
        self.window = None;
        self.draw_calls.clear();
        PASS_SKIP.set(skip);
        self.script = Some(ScriptState {
            run: Some(FrameScriptRun::new(script)),
            measuring: false,
            gap: false,
            capture: if probes.draw_calls {
                DrawCapture::Waiting {
                    left: FRAMES_BEFORE_CAPTURE,
                }
            } else {
                DrawCapture::Off
            },
            fence: probes.gpu_wait.then(GpuFence::new),
            clock: if script.plays {
                ScriptClock::Held
            } else {
                ScriptClock::Untouched
            },
        });
        Ok(())
    }

    /// Whether frames currently advance by the script's fixed step.
    pub(crate) fn scripted(&self) -> bool {
        self.script
            .as_ref()
            .is_some_and(|script| script.run.is_some())
    }

    /// Moves a running script on to the frame about to be drawn. Returns the
    /// live match's new paused state when the script changes it.
    pub(crate) fn step_script(&mut self, camera: &mut Camera) -> Option<bool> {
        let script = self.script.as_mut()?;
        let run = script.run.as_mut()?;
        let step = run.step(camera);
        script.measuring = step == Step::Measure;
        if script.measuring {
            script.capture = match script.capture {
                DrawCapture::Off => DrawCapture::Off,
                DrawCapture::Waiting { left: 0 } => {
                    macroquad::telemetry::capture_frame();
                    DrawCapture::Requested
                }
                DrawCapture::Waiting { left } => DrawCapture::Waiting { left: left - 1 },
                DrawCapture::Requested => {
                    DRAW_CAPTURE.set(true);
                    DRAW_SEEN.take();
                    DRAW_COUNTS.take();
                    script.measuring = false;
                    DrawCapture::Capturing
                }
                DrawCapture::Capturing => {
                    script.measuring = false;
                    DrawCapture::Recovering
                }
                DrawCapture::Recovering => DrawCapture::Off,
            };
        }
        script.gap |= !script.measuring;
        if step == Step::Done {
            script.run = None;
            PASS_SKIP.set(0);
        }
        // Keyed to the step, not to `measuring`: a captured frame still plays,
        // so a trial with a capture ticks like one without.
        match (script.clock, step == Step::Measure) {
            (ScriptClock::Held, true) => {
                script.clock = ScriptClock::Playing;
                Some(false)
            }
            (ScriptClock::Playing, false) => {
                script.clock = ScriptClock::Held;
                Some(true)
            }
            _ => None,
        }
    }

    /// Waits for the GPU to finish a measured frame when the script asked to.
    /// Call after the frame's draws have been submitted.
    pub(crate) fn wait_for_gpu(&mut self) {
        let Some(script) = &mut self.script else {
            return;
        };
        if script.measuring
            && let Some(fence) = &script.fence
        {
            self.pending_gpu_wait = Some(fence.wait());
        }
    }

    /// Consumes the one unmeasured live frame after Resume. Keeping the match
    /// from advancing on that frame prevents request dispatch and reply work
    /// from contaminating the first sample.
    pub(crate) fn take_start_barrier(&mut self) -> bool {
        let Some(window) = &mut self.window else {
            return false;
        };
        std::mem::take(&mut window.start_barrier)
    }

    pub(crate) fn stop_tick(&self) -> Option<u64> {
        self.window
            .as_ref()
            .filter(|window| !window.complete)
            .map(|window| window.to_tick)
    }

    pub(crate) fn record(&mut self, observation: FrameObservation<'_>) {
        if !self.enabled {
            return;
        }
        let passes = PASS_SPENT.take();
        if DRAW_CAPTURE.replace(false) {
            self.draw_calls = captured_draw_calls();
        }
        let gpu_wait_ms = self.pending_gpu_wait.take();
        let after_gap = match &mut self.script {
            Some(script) if !script.measuring => return,
            Some(script) => std::mem::take(&mut script.gap),
            None => false,
        };
        let now = Instant::now();
        let frame_started = now
            .checked_sub(Duration::from_secs_f64(
                (observation.work_ms + observation.submit_ms + gpu_wait_ms.unwrap_or(0.0)).max(0.0)
                    / 1000.0,
            ))
            .unwrap_or(now);
        if let Some(window) = &self.window
            && (window.complete
                || window.start_barrier
                || observation.mode != "playing"
                || !observation.active_playing
                || observation.tick_start != window.next_tick
                || observation.tick_end > window.to_tick)
        {
            return;
        }
        if self.samples.len() == MAX_SAMPLES {
            self.samples.pop_front();
            if let Some(window) = &mut self.window {
                window.truncated = true;
            }
        }
        self.samples.push_back(FrameSample {
            mode: observation.mode.to_string(),
            tick_start: observation.tick_start,
            tick_end: observation.tick_end,
            started_at: frame_started,
            work_ms: observation.work_ms,
            submit_ms: observation.submit_ms,
            gpu_wait_ms,
            passes,
            units: observation.units,
            buildings: observation.buildings,
            after_gap,
        });
        if let Some(window) = &mut self.window {
            window.started_at.get_or_insert(frame_started);
            window.next_tick = observation.tick_end;
            if window.next_tick == window.to_tick {
                window.complete = true;
                window.elapsed_ms = now
                    .duration_since(window.started_at.expect("set above"))
                    .as_secs_f64()
                    * 1000.0;
            }
        }
    }

    pub(crate) fn snapshot(&mut self, reset: bool) -> FrameProfileView {
        let frames = self.samples.len();
        let tick_start = self.samples.front().map_or(0, |sample| sample.tick_start);
        let tick_end = self.samples.back().map_or(0, |sample| sample.tick_end);
        let ticks_presented = self.samples.iter().fold(0u64, |total, sample| {
            total.saturating_add(sample.tick_end.saturating_sub(sample.tick_start))
        });
        let series = |value: &dyn Fn(&FrameSample) -> f64| -> TimingSummary {
            summarize(self.samples.iter().map(value).collect())
        };
        let interval_values: Vec<f64> = self
            .samples
            .iter()
            .zip(self.samples.iter().skip(1))
            .filter(|(_, current)| !current.after_gap)
            .map(|(previous, current)| {
                current
                    .started_at
                    .saturating_duration_since(previous.started_at)
                    .as_secs_f64()
                    * 1000.0
            })
            .collect();
        let slowest = self
            .samples
            .iter()
            .max_by(|a, b| a.cpu_ms().total_cmp(&b.cpu_ms()))
            .map(|sample| SlowFrameView {
                mode: sample.mode.clone(),
                tick_start: sample.tick_start,
                tick_end: sample.tick_end,
                work_ms: sample.work_ms,
                submit_ms: sample.submit_ms,
                passes: FramePass::ALL
                    .iter()
                    .map(|&pass| PassCostView {
                        name: pass.name().to_string(),
                        ms: sample.passes[pass as usize],
                    })
                    .chain([PassCostView {
                        name: OTHER_PASS.to_string(),
                        ms: sample.other_ms(),
                    }])
                    .collect(),
                units: sample.units,
                buildings: sample.buildings,
            });
        let view = FrameProfileView {
            frames,
            tick_start,
            tick_end,
            ticks_presented,
            work: series(&|sample| sample.work_ms),
            submit: series(&|sample| sample.submit_ms),
            cpu: series(&FrameSample::cpu_ms),
            passes: FramePass::ALL
                .iter()
                .map(|&pass| PassTimingView {
                    name: pass.name().to_string(),
                    timing: series(&|sample| sample.passes[pass as usize]),
                })
                .chain([PassTimingView {
                    name: OTHER_PASS.to_string(),
                    timing: series(&FrameSample::other_ms),
                }])
                .collect(),
            interval: summarize(interval_values),
            cpu_over_8_3_ms: self
                .samples
                .iter()
                .filter(|sample| sample.cpu_ms() > BUDGET_120_HZ_MS)
                .count(),
            work_over_16_7_ms: self
                .samples
                .iter()
                .filter(|sample| sample.work_ms > 1000.0 / 60.0)
                .count(),
            work_over_33_3_ms: self
                .samples
                .iter()
                .filter(|sample| sample.work_ms > 2000.0 / 60.0)
                .count(),
            slowest,
            window: self.window.as_ref().map(|window| FrameProfileWindowView {
                from_tick: window.from_tick,
                to_tick: window.to_tick,
                complete: window.complete,
                elapsed_ms: window.elapsed_ms,
                truncated: window.truncated,
            }),
            gpu_wait: self
                .samples
                .iter()
                .any(|sample| sample.gpu_wait_ms.is_some())
                .then(|| {
                    summarize(
                        self.samples
                            .iter()
                            .filter_map(|sample| sample.gpu_wait_ms)
                            .collect(),
                    )
                }),
            draw_calls: self.draw_calls.clone(),
            script_complete: self.script.as_ref().map(|script| script.run.is_none()),
        };
        if reset {
            self.samples.clear();
            self.window = None;
            self.script = None;
            self.draw_calls.clear();
            PASS_SKIP.set(0);
        }
        view
    }
}

fn skip_mask(names: &[String]) -> Result<u32, String> {
    names.iter().try_fold(0, |mask, name| {
        let pass = FramePass::named(name).ok_or_else(|| format!("no pass is named {name:?}"))?;
        if pass.skippable() {
            Ok(mask | 1 << pass as u32)
        } else {
            Err(format!("the {name} pass can't be skipped"))
        }
    })
}

/// The finished capture's counts: each pass's, then whatever no pass claimed.
fn captured_draw_calls() -> Vec<PassDrawCallsView> {
    let view = |name: &str, count: DrawCount| PassDrawCallsView {
        name: name.to_string(),
        draw_calls: count.draw_calls,
        indices: count.indices,
    };
    let counts = DRAW_COUNTS.take();
    let other = unclaimed_draw_calls();
    FramePass::ALL
        .iter()
        .map(|&pass| view(pass.name(), counts[pass as usize]))
        .chain([view(OTHER_PASS, other)])
        .collect()
}

fn summarize(mut values: Vec<f64>) -> TimingSummary {
    if values.is_empty() {
        return TimingSummary {
            mean_ms: 0.0,
            p50_ms: 0.0,
            p95_ms: 0.0,
            p99_ms: 0.0,
            max_ms: 0.0,
        };
    }
    values.sort_by(f64::total_cmp);
    let mean_ms = values.iter().sum::<f64>() / values.len() as f64;
    let percentile = |percent: usize| {
        let index = (values.len() * percent).div_ceil(100).saturating_sub(1);
        values[index]
    };
    TimingSummary {
        mean_ms,
        p50_ms: percentile(50),
        p95_ms: percentile(95),
        p99_ms: percentile(99),
        max_ms: *values.last().expect("nonempty values"),
    }
}

#[cfg(test)]
mod tests;
