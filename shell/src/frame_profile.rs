//! Opt-in native-frame timing for the live GPU shell.

use crate::camera::Camera;
use crate::frame_script::{FrameScriptRun, Step};
use aftermine_protocol::{
    FrameProfileView, FrameProfileWindowView, FrameScript, PassCostView, PassTimingView,
    SlowFrameView, TimingSummary,
};
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
    result
}

/// [`measure`] for drawing, which returns nothing.
pub(crate) fn pass(which: FramePass, draw: impl FnOnce()) {
    measure(which, draw);
}

#[derive(Debug, Clone)]
struct FrameSample {
    mode: String,
    tick_start: u64,
    tick_end: u64,
    started_at: Instant,
    work_ms: f64,
    submit_ms: f64,
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
}

/// A script underway or finished. A finished one stays until its samples are
/// reset, so the report can say it completed.
struct ScriptState {
    /// `None` once the script has finished.
    run: Option<FrameScriptRun>,
    measuring: bool,
    /// Unmeasured frames ran since the last sample.
    gap: bool,
    /// The script wants the match playing during measured frames.
    plays: bool,
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
    pub(crate) fn begin_script(&mut self, script: &FrameScript) -> Result<(), String> {
        if !self.enabled {
            return Err(
                "native frame profiling is disabled; launch the shell with --profile-frames"
                    .to_string(),
            );
        }
        self.samples.clear();
        self.window = None;
        self.script = Some(ScriptState {
            run: Some(FrameScriptRun::new(script)),
            measuring: false,
            gap: false,
            plays: script.plays,
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
        let was_playing = script.plays && script.measuring;
        let step = run.step(camera);
        script.measuring = step == Step::Measure;
        script.gap |= step == Step::Settle;
        if step == Step::Done {
            script.run = None;
        }
        let playing = script.plays && script.measuring;
        (playing != was_playing).then_some(!playing)
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
        let after_gap = match &mut self.script {
            Some(script) if !script.measuring => return,
            Some(script) => std::mem::take(&mut script.gap),
            None => false,
        };
        let now = Instant::now();
        let frame_started = now
            .checked_sub(Duration::from_secs_f64(
                (observation.work_ms + observation.submit_ms).max(0.0) / 1000.0,
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
            renderer: "gpu".to_string(),
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
            script_complete: self.script.as_ref().map(|script| script.run.is_none()),
        };
        if reset {
            self.samples.clear();
            self.window = None;
            self.script = None;
        }
        view
    }
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
