//! Named native frame workloads: where a frame's time goes on each screen, map
//! size, view and camera motion.

use crate::auto::{ShellGuard, build_shell_executable_for, isolate_home, write_home_config};
use crate::client::Client;
use crate::profile::{ProfileWorkspace, connect_within, expect_ok, expect_performance};
use aftermine_kit::runner;
use aftermine_protocol::{
    CameraMotion, FrameProbes, FrameProfileView, FrameScript, Request, StateFilter, StateView,
    TimingSummary,
};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The logical size of an 11-inch iPad, the game's main 120 Hz target.
const WINDOW: [u32; 2] = [1210, 834];
const NEAR_ZOOM: f64 = 32.0;
const FAR_ZOOM: f64 = 8.0;
const SETTLE_FRAMES: u32 = 120;
const HOLD_FRAMES: u32 = 240;
const PAN_FRAMES: u32 = 480;
/// A held pan's speed at [`NEAR_ZOOM`] with default settings.
const PAN_TILES_PER_SECOND: f64 = 28.0;
const ZOOM_REPEATS: u32 = 3;
const REST_FRAMES: u32 = 60;
/// A shell rebuilds a replay's match before it starts serving.
const LOAD_TIMEOUT: Duration = Duration::from_secs(600);
const TRIAL_TIMEOUT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_millis(500);
/// No display refreshes faster than this. Frames coming quicker mean nothing is
/// pacing them, which on a Mac means its displays are asleep.
const FASTEST_DISPLAY_MS: f64 = 4.0;
/// Enough spinning threads to keep a fast core cluster awake, and few enough to
/// leave the shell a core of its own.
const CLOCK_HOLDERS: usize = 2;
/// One frame's budget on a 120 Hz display.
const BUDGET_NAME: &str = "over 8.33 ms";

/// Inputs for one profiling run.
pub struct GraphicsOptions<'a> {
    /// Keep only workloads whose name contains this.
    pub only: Option<&'a str>,
    /// Repeats per workload; tables report the median.
    pub trials: usize,
    /// Debug-server port used by the temporary shells.
    pub port: u16,
    /// Build the development profile instead of release.
    pub dev: bool,
    /// Wait for the GPU after each frame and time the wait.
    pub gpu: bool,
    /// Passes to leave undrawn.
    pub skip: &'a [String],
    /// Leave clock speeds to the OS instead of holding them up.
    pub natural_clocks: bool,
}

/// Threads that spin for as long as this lives. On a quiet Mac the OS runs a
/// light frame several times slower than it can, and how much slower wanders
/// from run to run, which hides real differences. Busy cores keep frames near
/// full speed, so runs compare.
struct HeldClocks {
    stop: Arc<AtomicBool>,
    holders: Vec<std::thread::JoinHandle<()>>,
}

impl HeldClocks {
    fn start() -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let holders = (0..CLOCK_HOLDERS)
            .map(|_| {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    let mut spins = 0_u64;
                    while !stop.load(Ordering::Relaxed) {
                        spins = std::hint::black_box(spins.wrapping_add(1));
                    }
                })
            })
            .collect();
        Self { stop, holders }
    }
}

impl Drop for HeldClocks {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for holder in self.holders.drain(..) {
            holder.join().ok();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Size {
    Small,
    Large,
}

impl Size {
    const fn name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Large => "large",
        }
    }

    const fn map(self) -> &'static str {
        match self {
            Self::Small => "scenarios/smelter-yard.json",
            Self::Large => "scenarios/grand-viaduct.json",
        }
    }

    /// Late enough for real armies, early enough that the match is undecided.
    const fn ticks(self) -> u64 {
        match self {
            Self::Small => 6_480,
            Self::Large => 18_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    /// Seat zero's fog, through the live match.
    Fogged,
    /// No fog, through the replay viewer.
    Open,
}

impl View {
    const fn name(self) -> &'static str {
        match self {
            Self::Fogged => "fogged",
            Self::Open => "open",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Home,
    Match(Size, View),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shot {
    /// The screen's own camera, untouched.
    Own,
    Near,
    Far,
    Pan,
    ZoomOut,
    ZoomIn,
}

impl Shot {
    const MATCH: [Self; 5] = [
        Self::Near,
        Self::Far,
        Self::Pan,
        Self::ZoomOut,
        Self::ZoomIn,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::Near => "near",
            Self::Far => "far",
            Self::Pan => "pan",
            Self::ZoomOut => "zoom-out",
            Self::ZoomIn => "zoom-in",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Workload {
    name: String,
    source: Source,
    shot: Shot,
    /// Whether the match plays while frames are measured; every other workload
    /// holds it paused. A match that has played can't repeat, so each trial of
    /// this workload gets a fresh shell.
    running: bool,
}

/// Every workload, with those sharing a source next to each other so a run
/// launches each shell once.
fn workloads() -> Vec<Workload> {
    let mut all = vec![Workload {
        name: "home".to_string(),
        source: Source::Home,
        shot: Shot::Own,
        running: false,
    }];
    for size in [Size::Small, Size::Large] {
        for view in [View::Fogged, View::Open] {
            let source = Source::Match(size, view);
            for shot in Shot::MATCH {
                all.push(Workload {
                    name: format!("{}-{}-{}", size.name(), view.name(), shot.name()),
                    source,
                    shot,
                    running: false,
                });
            }
            if (size, view) == (Size::Large, View::Fogged) {
                all.push(Workload {
                    name: format!("{}-{}-running", size.name(), view.name()),
                    source,
                    shot: Shot::Near,
                    running: true,
                });
            }
        }
    }
    all
}

fn select(only: Option<&str>) -> Result<Vec<Workload>> {
    let selected: Vec<Workload> = workloads()
        .into_iter()
        .filter(|workload| only.is_none_or(|only| workload.name.contains(only)))
        .collect();
    ensure!(
        !selected.is_empty(),
        "no workload name contains {:?}",
        only.unwrap_or_default()
    );
    Ok(selected)
}

fn script(workload: &Workload, center: [f64; 2]) -> FrameScript {
    let motion = match workload.shot {
        Shot::Own => CameraMotion::None {
            frames: HOLD_FRAMES,
        },
        Shot::Near => CameraMotion::Hold {
            center,
            zoom: NEAR_ZOOM,
            frames: HOLD_FRAMES,
        },
        Shot::Far => CameraMotion::Hold {
            center,
            zoom: FAR_ZOOM,
            frames: HOLD_FRAMES,
        },
        Shot::Pan => CameraMotion::Pan {
            center,
            zoom: NEAR_ZOOM,
            tiles_per_second: PAN_TILES_PER_SECOND,
            frames: PAN_FRAMES,
        },
        Shot::ZoomOut => zoom(center, NEAR_ZOOM, FAR_ZOOM),
        Shot::ZoomIn => zoom(center, FAR_ZOOM, NEAR_ZOOM),
    };
    FrameScript {
        settle_frames: SETTLE_FRAMES,
        plays: workload.running,
        motion,
    }
}

const fn zoom(center: [f64; 2], from: f64, to: f64) -> CameraMotion {
    CameraMotion::Zoom {
        center,
        from,
        to,
        repeats: ZOOM_REPEATS,
        rest_frames: REST_FRAMES,
    }
}

/// Where seat zero's forces stand on average, where a player's camera would be.
fn seat_focus(state: &StateView) -> Option<[f64; 2]> {
    let points: Vec<[f64; 2]> = state
        .units
        .iter()
        .filter(|unit| unit.player == 0)
        .map(|unit| unit.pos)
        .chain(
            state
                .buildings
                .iter()
                .filter(|building| building.player == 0)
                .map(|building| building.anchor.map(f64::from)),
        )
        .collect();
    if points.is_empty() {
        return None;
    }
    let count = points.len() as f64;
    let axis = |index: usize| points.iter().map(|point| point[index]).sum::<f64>() / count;
    Some([axis(0), axis(1)])
}

/// One workload's results across its trials.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkloadResult {
    /// Workload name.
    pub name: String,
    /// Units in the whole match, or zero on Home.
    pub units: usize,
    /// Structures in the whole match, or zero on Home.
    pub structures: usize,
    /// Camera center and zoom when the last trial ended, for checking that a
    /// script drew the same frames as an earlier run.
    pub final_camera: [f64; 3],
    /// Each trial's full frame profile.
    pub trials: Vec<FrameProfileView>,
}

/// A whole run. Written with `--out` and read back by `--compare`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphicsReport {
    /// Source revision of the driver that ran it.
    pub revision: String,
    /// Logical window size.
    pub window: [u32; 2],
    /// Whether frames waited for the GPU, which changes their pacing.
    pub gpu: bool,
    /// Passes left undrawn.
    pub skipped: Vec<String>,
    /// Whether clock speeds were left to the OS. Those numbers run higher and
    /// vary more.
    pub natural_clocks: bool,
    /// Results in run order.
    pub workloads: Vec<WorkloadResult>,
}

/// Runs the selected workloads, each source in its own temporary shell.
pub fn run(options: &GraphicsOptions<'_>) -> Result<GraphicsReport> {
    ensure!(options.trials > 0, "--trials must be at least 1");
    let selected = select(options.only)?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let executable = build_shell_executable_for(!options.dev)?;
    let _clocks = (!options.natural_clocks).then(HeldClocks::start);
    let mut results = Vec::new();
    let mut start = 0;
    while start < selected.len() {
        let source = selected[start].source;
        let group = selected[start..]
            .iter()
            .take_while(|workload| workload.source == source)
            .count();
        results.extend(run_source(
            &root,
            &executable,
            source,
            &selected[start..start + group],
            options,
        )?);
        start += group;
    }
    Ok(GraphicsReport {
        revision: crate::build_identity().revision.clone(),
        window: WINDOW,
        gpu: options.gpu,
        skipped: options.skip.to_vec(),
        natural_clocks: options.natural_clocks,
        workloads: results,
    })
}

fn run_source(
    root: &Path,
    executable: &Path,
    source: Source,
    group: &[Workload],
    options: &GraphicsOptions<'_>,
) -> Result<Vec<WorkloadResult>> {
    let mut shell = None;
    let mut results = Vec::new();
    for workload in group {
        eprintln!("profiling {}", workload.name);
        let mut trials = Vec::new();
        for trial in 0..options.trials {
            if workload.running {
                shell = None;
            }
            let live = match &mut shell {
                Some(live) => live,
                None => shell.insert(ProfiledShell::launch(root, executable, source, options)?),
            };
            let frame_script = script(workload, live.center);
            let probes = probes_for_trial(trial, options);
            trials.push(run_trial(&mut live.client, &frame_script, &probes)?);
        }
        let live = shell.as_mut().context("no trial ran")?;
        let camera = live.client.camera()?;
        results.push(WorkloadResult {
            name: workload.name.clone(),
            units: live.units,
            structures: live.structures,
            final_camera: [camera.center[0], camera.center[1], camera.zoom],
            trials,
        });
        if workload.running {
            shell = None;
        }
    }
    Ok(results)
}

/// Draw calls come from one plain trial per workload: a capture leaves its
/// frames undrawable for timing and allocates a texture per draw call.
fn probes_for_trial(trial: usize, options: &GraphicsOptions<'_>) -> FrameProbes {
    FrameProbes {
        draw_calls: trial == 0 && !options.gpu && options.skip.is_empty(),
        gpu_wait: options.gpu,
        skip_passes: options.skip.to_vec(),
    }
}

/// A Mac with its displays asleep stops pacing and presenting frames, and the
/// numbers change with it, so hold them awake for as long as `pid` runs.
fn keep_display_awake(pid: u32) {
    if cfg!(target_os = "macos") {
        drop(
            std::process::Command::new("caffeinate")
                .args(["-d", "-u", "-w", &pid.to_string()])
                .spawn(),
        );
    }
}

/// A temporary shell showing one source, ready for scripts.
struct ProfiledShell {
    client: Client,
    units: usize,
    structures: usize,
    /// Where scripts frame the camera.
    center: [f64; 2],
    // Stops the shell before its folder goes away, so this order matters.
    _guard: ShellGuard,
    _workspace: ProfileWorkspace,
}

impl ProfiledShell {
    fn launch(
        root: &Path,
        executable: &Path,
        source: Source,
        options: &GraphicsOptions<'_>,
    ) -> Result<Self> {
        let workspace = ProfileWorkspace::scratch(root, options.port)?;
        write_home_config(&workspace.directory, include_str!("quiet_config.json"))?;
        let mut command = std::process::Command::new(executable);
        command
            .args([
                "--debug-server",
                "--profile-frames",
                "--window",
                &format!("{}x{}", WINDOW[0], WINDOW[1]),
                "--port",
                &options.port.to_string(),
            ])
            .env("AFTERMINE_RESOURCE_ROOT", root)
            .current_dir(&workspace.directory);
        match source {
            Source::Home => {
                command.arg("--automation");
            }
            Source::Match(size, view) => {
                let replay = input_replay(root, size)?;
                let flag = match view {
                    View::Fogged => "--replay",
                    View::Open => "--watch",
                };
                command.args([flag, &replay.to_string_lossy(), "--paused"]);
            }
        }
        isolate_home(&mut command, &workspace.directory);
        let child = command.spawn().context("spawning profiled shell")?;
        keep_display_awake(child.id());
        let guard = ShellGuard::new(child);
        let mut client = connect_within(options.port, LOAD_TIMEOUT)?;

        let mut units = 0;
        let mut structures = 0;
        let mut focus = None;
        if let Source::Match(size, view) = source {
            if view == View::Open {
                let advanced = client.advance(size.ticks())?;
                ensure!(
                    advanced.tick == size.ticks(),
                    "viewer stopped at tick {} on the way to {}",
                    advanced.tick,
                    size.ticks()
                );
            }
            let status = client.status()?;
            ensure!(
                status.tick == size.ticks() && status.result.is_none(),
                "expected an undecided match at tick {}, found tick {} with result {:?}",
                size.ticks(),
                status.tick,
                status.result
            );
            let state = client.state(StateFilter {
                players: false,
                units: true,
                buildings: true,
                map: false,
            })?;
            units = state.units.len();
            structures = state.buildings.len();
            focus = seat_focus(&state);
        }
        let opening = client.camera()?;
        Ok(Self {
            client,
            units,
            structures,
            center: focus.unwrap_or(opening.center),
            _guard: guard,
            _workspace: workspace,
        })
    }
}

/// The replay behind a match workload, generated the first time it is needed
/// and kept under the gitignored `replays/` so comparisons share one input.
fn input_replay(root: &Path, size: Size) -> Result<PathBuf> {
    let path = root
        .join("replays/graphics")
        .join(format!("{}-{}.json", size.name(), size.ticks()));
    if aftermine_kit::load_replay(&path).is_ok_and(|replay| {
        replay.validate(Some(aftermine_sim::SIM_VERSION)).is_ok()
            && replay.meta.ticks == Some(size.ticks())
    }) {
        return Ok(path);
    }
    eprintln!("generating {}", path.display());
    let map = root.join(size.map());
    let mut scenario = runner::load_scenario(&map.to_string_lossy())?;
    aftermine_kit::bench::all_bots(&mut scenario);
    let build = crate::build_identity();
    let outcome = runner::run_scenario(&scenario, size.ticks(), true, Some(&build))?;
    ensure!(
        outcome.state.result().is_none(),
        "{} was decided before tick {}; choose an earlier one",
        size.map(),
        size.ticks()
    );
    let replay = outcome.replay.context("the run recorded no replay")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("creating the replay folder")?;
    }
    replay.save(&path).context("saving the generated replay")?;
    Ok(path)
}

fn run_trial(
    client: &mut Client,
    script: &FrameScript,
    probes: &FrameProbes,
) -> Result<FrameProfileView> {
    start_script(client, script, probes)?;
    let deadline = Instant::now() + TRIAL_TIMEOUT;
    let profile = loop {
        std::thread::sleep(POLL);
        let profile = expect_performance(
            client.call(Request::QueryPerformance { reset: false })?,
            "read frame profile",
        )?;
        if profile.script_complete == Some(true) {
            break profile;
        }
        ensure!(Instant::now() < deadline, "the frame script never finished");
    };
    check_trial(&profile, probes)?;
    Ok(profile)
}

/// Refuses a trial that came back without what it was asked to measure.
fn check_trial(profile: &FrameProfileView, probes: &FrameProbes) -> Result<()> {
    ensure!(profile.frames > 0, "the frame script measured no frames");
    ensure!(
        !probes.draw_calls || !profile.draw_calls.is_empty(),
        "the frame script ended before its draw call capture"
    );
    Ok(())
}

/// The viewer refuses scripts while its background fill would compete with
/// the measured frames.
fn start_script(client: &mut Client, script: &FrameScript, probes: &FrameProbes) -> Result<()> {
    let deadline = Instant::now() + LOAD_TIMEOUT;
    loop {
        let request = Request::RunFrameScript {
            script: script.clone(),
            probes: probes.clone(),
        };
        match client.call(request) {
            Ok(reply) => return expect_ok(&reply, "run frame script"),
            Err(error) if error.to_string().contains("still filling") => {
                if Instant::now() >= deadline {
                    bail!("the viewer never finished filling its record");
                }
                std::thread::sleep(POLL);
            }
            Err(error) => return Err(error),
        }
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied().unwrap_or(0.0)
}

/// A summary whose every field is the median across `summaries`.
fn median_timing(summaries: &[&TimingSummary]) -> TimingSummary {
    let field = |read: fn(&TimingSummary) -> f64| {
        median(summaries.iter().map(|timing| read(timing)).collect())
    };
    TimingSummary {
        mean_ms: field(|timing| timing.mean_ms),
        p50_ms: field(|timing| timing.p50_ms),
        p95_ms: field(|timing| timing.p95_ms),
        p99_ms: field(|timing| timing.p99_ms),
        max_ms: field(|timing| timing.max_ms),
    }
}

/// One pass of a workload, reduced across trials.
#[derive(Debug, Clone, PartialEq)]
struct PassDigest {
    name: String,
    timing: TimingSummary,
    /// What the pass took in the workload's slowest frame.
    worst_frame_ms: f64,
    /// Draw calls and indices in the captured frame, when one was captured.
    draws: Option<(usize, usize)>,
}

/// A workload reduced across trials.
#[derive(Debug, Clone, PartialEq)]
struct Digest {
    frames: f64,
    over_budget: f64,
    cpu: TimingSummary,
    work: TimingSummary,
    submit: TimingSummary,
    interval: TimingSummary,
    gpu_wait: Option<TimingSummary>,
    /// Costliest first.
    passes: Vec<PassDigest>,
}

fn digest(result: &WorkloadResult) -> Digest {
    let trials = &result.trials;
    let slowest = trials
        .iter()
        .filter_map(|trial| trial.slowest.as_ref())
        .max_by(|a, b| (a.work_ms + a.submit_ms).total_cmp(&(b.work_ms + b.submit_ms)));
    let names: Vec<String> = trials.first().map_or_else(Vec::new, |trial| {
        trial.passes.iter().map(|pass| pass.name.clone()).collect()
    });
    let mut passes: Vec<PassDigest> = names
        .into_iter()
        .map(|name| {
            let timings: Vec<&TimingSummary> = trials
                .iter()
                .flat_map(|trial| trial.passes.iter())
                .filter(|pass| pass.name == name)
                .map(|pass| &pass.timing)
                .collect();
            let timing = median_timing(&timings);
            let worst_frame_ms = slowest
                .and_then(|frame| frame.passes.iter().find(|pass| pass.name == name))
                .map_or(0.0, |pass| pass.ms);
            let draws = trials
                .iter()
                .flat_map(|trial| trial.draw_calls.iter())
                .find(|row| row.name == name)
                .map(|row| (row.draw_calls, row.indices));
            PassDigest {
                name,
                timing,
                worst_frame_ms,
                draws,
            }
        })
        .collect();
    passes.sort_by(|a, b| b.timing.mean_ms.total_cmp(&a.timing.mean_ms));
    let overall = |read: fn(&FrameProfileView) -> &TimingSummary| {
        median_timing(&trials.iter().map(read).collect::<Vec<_>>())
    };
    Digest {
        frames: median(trials.iter().map(|trial| trial.frames as f64).collect()),
        over_budget: median(
            trials
                .iter()
                .map(|trial| trial.cpu_over_8_3_ms as f64)
                .collect(),
        ),
        cpu: overall(|trial| &trial.cpu),
        work: overall(|trial| &trial.work),
        submit: overall(|trial| &trial.submit),
        interval: overall(|trial| &trial.interval),
        gpu_wait: {
            let waits: Vec<&TimingSummary> = trials
                .iter()
                .filter_map(|trial| trial.gpu_wait.as_ref())
                .collect();
            (!waits.is_empty()).then(|| median_timing(&waits))
        },
        passes,
    }
}

/// `mean` against the same row of an earlier run, when it has one.
fn with_change(mean: f64, earlier: Option<f64>) -> String {
    match earlier {
        Some(earlier) => format!("{mean:6.2} ({:+.2})", mean - earlier),
        None => format!("{mean:6.2}"),
    }
}

fn timing_row(out: &mut String, label: &str, timing: &TimingSummary, earlier: Option<f64>) {
    writeln!(
        out,
        "  {label:<15} {} {:6.2} {:6.2} {:6.2}",
        with_change(timing.mean_ms, earlier),
        timing.p95_ms,
        timing.p99_ms,
        timing.max_ms
    )
    .expect("writing to a string");
}

/// Whether two runs measured frames under the same conditions.
const fn comparable(a: &GraphicsReport, b: &GraphicsReport) -> bool {
    a.natural_clocks == b.natural_clocks && a.gpu == b.gpu
}

/// A pass that never reaches this adds only noise to the table.
const SMALLEST_PASS_SHOWN_MS: f64 = 0.01;

/// The tables a run prints: one line per workload, then each in detail. With
/// `baseline`, means also show their change from that run's same workload.
pub fn render(report: &GraphicsReport, baseline: Option<&GraphicsReport>) -> String {
    let mut out = String::new();
    writeln!(
        out,
        "revision {}, window {}x{}, milliseconds, {} clocks{}{}\n",
        report.revision,
        report.window[0],
        report.window[1],
        if report.natural_clocks {
            "natural"
        } else {
            "held"
        },
        if report.gpu {
            ", waiting for the GPU each frame"
        } else {
            ""
        },
        if report.skipped.is_empty() {
            String::new()
        } else {
            format!(", without {}", report.skipped.join(", "))
        },
    )
    .expect("writing to a string");
    if report
        .workloads
        .iter()
        .any(|result| digest(result).interval.p50_ms < FASTEST_DISPLAY_MS)
    {
        writeln!(
            out,
            "WARNING: frames ran faster than any display refreshes, so nothing paced them.\n\
             The displays are probably asleep. These numbers don't compare with paced runs.\n"
        )
        .expect("writing to a string");
    }
    if baseline.is_some_and(|baseline| !comparable(report, baseline)) {
        writeln!(
            out,
            "WARNING: the compared run used different clocks or GPU waiting, so its changes \
             mean little.\n"
        )
        .expect("writing to a string");
    }
    let earlier_digest = |name: &str| {
        baseline
            .and_then(|baseline| baseline.workloads.iter().find(|result| result.name == name))
            .map(digest)
    };

    writeln!(
        out,
        "{:<26} {:>14} {:>6} {:>6} {:>13} {:>9}  costliest pass",
        "workload", "cpu mean", "p95", "max", BUDGET_NAME, "interval"
    )
    .expect("writing to a string");
    for result in &report.workloads {
        let current = digest(result);
        let earlier = earlier_digest(&result.name);
        let top = current
            .passes
            .first()
            .map_or("none", |pass| pass.name.as_str());
        writeln!(
            out,
            "{:<26} {:>14} {:6.2} {:6.2} {:>13} {:9.2}  {top}",
            result.name,
            with_change(
                current.cpu.mean_ms,
                earlier.map(|earlier| earlier.cpu.mean_ms)
            ),
            current.cpu.p95_ms,
            current.cpu.max_ms,
            current.over_budget,
            current.interval.p50_ms
        )
        .expect("writing to a string");
    }

    for result in &report.workloads {
        let current = digest(result);
        let earlier = earlier_digest(&result.name);
        writeln!(
            out,
            "\n{}: {} units, {} structures, {} frames, {} {BUDGET_NAME}",
            result.name, result.units, result.structures, current.frames, current.over_budget
        )
        .expect("writing to a string");
        writeln!(
            out,
            "  {:<15} {:>6} {:>6} {:>6} {:>6}",
            "", "mean", "p95", "p99", "max"
        )
        .expect("writing to a string");
        timing_row(
            &mut out,
            "cpu",
            &current.cpu,
            earlier.as_ref().map(|earlier| earlier.cpu.mean_ms),
        );
        timing_row(
            &mut out,
            "frame code",
            &current.work,
            earlier.as_ref().map(|earlier| earlier.work.mean_ms),
        );
        timing_row(
            &mut out,
            "submit",
            &current.submit,
            earlier.as_ref().map(|earlier| earlier.submit.mean_ms),
        );
        if let Some(gpu_wait) = &current.gpu_wait {
            let was = earlier
                .as_ref()
                .and_then(|earlier| earlier.gpu_wait.as_ref())
                .map(|was| was.mean_ms);
            timing_row(&mut out, "gpu wait", gpu_wait, was);
        }
        writeln!(
            out,
            "  {:<15} {:>6} {:>6} {:>6} {:>6}  worst frame  draws  indices",
            "pass", "mean", "p95", "p99", "max"
        )
        .expect("writing to a string");
        for pass in &current.passes {
            let draws_nothing = pass.draws.is_none_or(|(draws, _)| draws == 0);
            if pass.timing.max_ms < SMALLEST_PASS_SHOWN_MS && draws_nothing {
                continue;
            }
            let was = earlier
                .as_ref()
                .and_then(|earlier| earlier.passes.iter().find(|row| row.name == pass.name))
                .map(|row| row.timing.mean_ms);
            let (draws, indices) = pass.draws.map_or_else(
                || ("-".to_string(), "-".to_string()),
                |(draws, indices)| (draws.to_string(), indices.to_string()),
            );
            writeln!(
                out,
                "  {:<15} {} {:6.2} {:6.2} {:6.2}  {:>11.2}  {draws:>5}  {indices:>7}",
                pass.name,
                with_change(pass.timing.mean_ms, was),
                pass.timing.p95_ms,
                pass.timing.p99_ms,
                pass.timing.max_ms,
                pass.worst_frame_ms
            )
            .expect("writing to a string");
        }
    }
    out
}

#[cfg(test)]
mod tests;
