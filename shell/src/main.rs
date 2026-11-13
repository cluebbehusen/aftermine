#![doc = include_str!("../README.md")]
// A release build on Windows opens no console window behind the game;
// debug builds keep one for development output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The workspace's disallowed types guard simulation determinism. Shell state
// (interpolation caches, input sets) is presentation and may hash.
#![allow(clippy::disallowed_types)]

mod action;
mod app;
mod assets;
mod audio_mix;
mod autosave;
mod bot_label;
mod boundary_fog;
mod building_actions;
mod button;
mod camera;
mod clock_label;
mod cloud_mirror;
mod config;
mod debug_server;
mod description;
mod diagnostic_report;
mod dock_motion;
mod entity_lod;
mod entropy;
mod frame_profile;
mod frame_script;
mod frame_time;
mod game;
mod hint_line;
mod hints;
mod home_backdrop;
mod idle_weld;
mod input;
mod layout;
mod look;
mod loop_audio;
mod maw_audio;
mod maw_motion;
mod menu;
mod menu_stick;
mod mixer;
mod nav;
mod netplay;
mod numeric;
mod pacing;
mod pad;
mod panel;
mod paths;
mod performance;
mod platform;
mod presentation_animation;
mod press;
mod production;
mod radial;
mod rejoin_ticket;
mod render;
mod rocket_audio;
mod saved_game;
mod saves;
mod screens;
mod seat_style;
mod seating;
mod slide_motion;
mod soundtrack;
mod sprite_contact;
mod strategic_markers;
mod strike_audio;
mod tabs;
mod text_field;
mod theme;
mod tips;
mod track_motion;
mod transports;
mod tutorial;
mod typography;
mod unit_bays;
mod viewer_touch;
mod wording;

use crate::numeric::Fit;
use clap::Parser;
use macroquad::prelude::*;

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent command-line switches"
)]
#[derive(Parser)]
#[command(name = "aftermine-shell", version, about = "Aftermine, playable")]
struct Args {
    /// Scenario JSON path (skips the menu).
    #[arg(long, conflicts_with = "replay")]
    scenario: Option<String>,

    /// Resume a session from a replay JSON (skips the menu).
    #[arg(long)]
    replay: Option<String>,

    /// Open a replay in the read-only playback viewer (pause, speed,
    /// seek — no recorder, no commands).
    #[arg(long, conflicts_with_all = ["scenario", "replay", "automation"])]
    watch: Option<String>,

    /// Open match setup on the --scenario file with its LAN lobby listening
    /// on this address (e.g. 0.0.0.0:4200). The host takes the first seat the
    /// scenario leaves to a human.
    #[arg(
        long,
        requires = "scenario",
        conflicts_with_all = ["replay", "watch", "join", "automation", "paused", "speed"]
    )]
    host: Option<String>,

    /// Join the LAN match hosted at this address (e.g. 192.168.1.20:4200).
    #[arg(
        long,
        conflicts_with_all = ["scenario", "replay", "watch", "automation", "paused", "speed"]
    )]
    join: Option<String>,

    /// Serve the debug protocol on --port (skips the menu unless automated).
    #[arg(long)]
    debug_server: bool,

    /// Deterministic UI-driving mode: start at the main menu and accept only
    /// injected input, never hardware input.
    #[arg(
        long,
        requires = "debug_server",
        conflicts_with_all = ["scenario", "replay"]
    )]
    automation: bool,

    /// Debug server port.
    #[arg(long, default_value_t = aftermine_protocol::DEFAULT_PORT)]
    port: u16,

    /// Seconds a silent debug connection may sit before it is closed.
    /// Generous by default — a paused driven-mode agent legitimately
    /// parks between commands; raise it for parked-overnight sessions.
    #[arg(long, requires = "debug_server", default_value_t = 30 * 60,
          value_parser = clap::value_parser!(u64).range(1..))]
    debug_idle_timeout: u64,

    /// Start with the sim clock stopped: time advances only via the debug
    /// socket (driven mode). Rendering still runs.
    #[arg(long)]
    paused: bool,

    /// Wall-clock speed multiplier.
    #[arg(long, default_value_t = 1.0, value_parser = parse_speed)]
    speed: f64,

    /// Window size as `WIDTHxHEIGHT` (e.g. 800x600).
    #[arg(long, value_parser = parse_window)]
    window: Option<(u32, u32)>,

    /// Render at logical (non-retina) pixel density, exercising the 1x
    /// layout path on high-DPI displays.
    #[arg(long)]
    no_high_dpi: bool,

    /// Collect bounded native GPU-shell frame timings for
    /// `query_performance`. Off by default so ordinary play pays no timing or
    /// sample-retention cost.
    #[arg(long, requires = "debug_server")]
    profile_frames: bool,
}

/// Parses `WIDTHxHEIGHT` with sane floors — smaller than 640x400 and the
/// fixed chrome cannot physically fit.
fn parse_window(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| "expected WIDTHxHEIGHT, e.g. 800x600".to_string())?;
    let (w, h): (u32, u32) = (
        w.parse().map_err(|err| format!("{err}"))?,
        h.parse().map_err(|err| format!("{err}"))?,
    );
    if w < 640 || h < 400 {
        return Err("window must be at least 640x400".to_string());
    }
    if w > 16_384 || h > 16_384 {
        // The native config takes i32, so huge values must not reach it.
        return Err("window must be at most 16384x16384".to_string());
    }
    Ok((w, h))
}

/// Same envelope the debug socket enforces — the CLI shouldn't accept less
/// sane values than the protocol does.
fn parse_speed(s: &str) -> Result<f64, String> {
    let v: f64 = s.parse().map_err(|err| format!("{err}"))?;
    if v.is_finite() && (0.05..=64.0).contains(&v) {
        Ok(v)
    } else {
        Err("speed must be a finite value within 0.05..=64".to_string())
    }
}

fn window_conf() -> macroquad::conf::Conf {
    // The window is created before `main()` ever sees clap's output, so
    // the size/DPI flags are parsed here too — clap is idempotent and
    // errors surface identically on the second parse in `main()`.
    let args = cli_args();
    let (width, height) = args.window.unwrap_or(config::Config::load().window);
    batched(Conf {
        window_title: "Aftermine".to_string(),
        window_width: width.fit::<i32>(),
        window_height: height.fit::<i32>(),
        high_dpi: !args.no_high_dpi,
        // Miniquad replaces the macOS Dock icon with its 64px image. Leave
        // the Mac app's full-resolution icon, which Xcode compiles from the
        // asset catalog's AppIcon, in place when available; sharpening the
        // dev-run icon would take an unsafe AppKit call.
        icon: if cfg!(target_os = "macos")
            && paths::bundle_resources().is_some_and(|root| root.join("AppIcon.icns").is_file())
        {
            None
        } else {
            Some(macroquad::miniquad::conf::Icon {
                small: *include_bytes!("../../assets/icon/aftermine_16.rgba"),
                medium: *include_bytes!("../../assets/icon/aftermine_32.rgba"),
                big: *include_bytes!("../../assets/icon/aftermine_64.rgba"),
            })
        },
        // The top bar's buttons sit against the top edge, where iPadOS
        // would otherwise take a touch that drifts as the pull for
        // Notification Center or Control Center and cancel the tap.
        platform: macroquad::miniquad::conf::Platform {
            ios_defer_top_edge_gestures: true,
            ..Default::default()
        },
        ..Default::default()
    })
}

/// Vertices macroquad keeps room for in one draw call.
const DRAW_CALL_VERTICES: usize = 10_000;

/// `window` as macroquad's own conversion would take it, but with index room
/// for every quad the vertex room holds: macroquad's default holds a third of
/// that, so a batch of quads ended every 833.
fn batched(window: Conf) -> macroquad::conf::Conf {
    macroquad::conf::Conf {
        miniquad_conf: window,
        update_on: None,
        default_filter_mode: FilterMode::Linear,
        draw_call_vertex_capacity: DRAW_CALL_VERTICES,
        draw_call_index_capacity: DRAW_CALL_VERTICES / 4 * 6,
    }
}

/// The command line. Only a desktop build has one worth reading:
/// elsewhere a launcher's own arguments must not trip the parser into
/// exiting.
fn cli_args() -> Args {
    if platform::PLATFORM.desktop() {
        Args::parse()
    } else {
        Args::parse_from(["Aftermine"])
    }
}

#[macroquad::main(window_conf)]
async fn main() {
    if let Err(err) = Box::pin(app::run(cli_args())).await {
        let message = format!("{err:#}");
        aftermine_kit::diagnostics::fatal(&message);
        eprintln!("fatal: {message}");
        std::process::exit(1);
    }
}

fn build_identity() -> aftermine_kit::build::BuildIdentity {
    aftermine_kit::build::BuildIdentity::new(
        env!("CARGO_PKG_VERSION"),
        env!("AFTERMINE_BUILD_REVISION"),
        env!("AFTERMINE_BUILD_DIRTY"),
    )
}

#[cfg(test)]
mod tests;
