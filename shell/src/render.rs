//! Drawing: map, entities, effects, HUD, debug overlay.
//!
//! Split by layer: submodules own the minimap, panel, chrome, and world
//! layers.
//!
//! Reads the sim, never writes it. Unit positions interpolate between the
//! previous and current tick so motion stays smooth between sim ticks.

use crate::assets::{
    ExcavatorPose as SpriteExcavatorPose, HarvesterPose as SpriteHarvesterPose, Sprites,
};
use crate::numeric;
use crate::numeric::Fit;
pub(crate) mod tracks;

/// The gauge a tracked hull's belts roll on, in world tiles: drawn belt
/// runs for most, the Maw's own bogies for the Maw.
pub(crate) fn tread_gauge(kind: oxide_sim::UnitKind) -> Option<f32> {
    if tracks::supported(kind) {
        Some(tracks::gauge(kind, unit_draw_scale(kind)))
    } else {
        (kind == oxide_sim::UnitKind::Maw).then_some(maw::GAUGE)
    }
}
static COLORBLIND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Colorblind palettes: swaps every ownership color (sprite accents, rings,
/// health bars, minimap marks) for colorblind-safe ones. Base sprite art is
/// unchanged.
pub fn set_colorblind(on: bool) {
    COLORBLIND.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn colorblind() -> bool {
    COLORBLIND.load(std::sync::atomic::Ordering::Relaxed)
}

static CONTROL_GROUPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether the HUD shows the control-group column.
pub fn set_control_groups(on: bool) {
    CONTROL_GROUPS.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn control_groups() -> bool {
    CONTROL_GROUPS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The viewer's own colorblind-aware seat color.
pub fn self_color() -> Color {
    crate::seat_style::self_color(colorblind())
}

/// The seat's ownership color as the viewer sees it: the self color, or its
/// stable ally- or enemy-family tint. Sprite accents, rings, health bars,
/// and minimap marks all wear it.
pub(crate) fn seat_identity_color(game: &Scene<'_>, owner: oxide_sim::PlayerId) -> Color {
    game.seat_styles.get(owner)
}

/// How faded a memory draws after `age` seconds unseen: 0 when fresh,
/// rising to a capped fade at ninety seconds. Memories fade but never
/// vanish.
pub fn staleness_fade(age: f32) -> f32 {
    (age / 90.0).clamp(0.0, 1.0) * 0.55
}

/// Draw opacity for a salvage tile: full while visible, then fading with
/// the time since it was last seen on the presentation clock.
pub(crate) fn resource_memory_opacity(game: &Scene<'_>, pos: chassis::grid::TilePos) -> f32 {
    let mut seen = game.presentation.last_seen.borrow_mut();
    let now = game.presentation.fx_time();
    if game.presentation.all_seeing() || game.my_vision().visible(pos) {
        seen.insert((pos.x, pos.y), now);
        1.0
    } else {
        let stamp = *seen.entry((pos.x, pos.y)).or_insert(now);
        1.0 - staleness_fade(now - stamp)
    }
}

mod boulders;
mod cell_field;
mod chrome;
mod destruction;
pub(crate) mod entities;
mod environment;
pub(crate) mod hud;
mod impacts;
mod levels;
pub(crate) mod maw;
mod minimap;
mod motion;
mod panel_draw;
mod panel_layout;
mod performance;
mod pits;
pub(crate) mod prim;
mod roc;
pub(crate) mod sky;
mod strategic;
mod stretches;
pub(crate) use sky::missile_seen_by;
pub(crate) use strategic::lattice_pod_pivot;
mod support_brackets;
mod terraces;
pub(crate) mod tutorial_card;
mod worker;
mod world;
use chrome::{
    draw_hud, draw_overlay, draw_overlay_info, draw_result_overlay, draw_salvage_tooltip,
};
use entities::{
    draw_blips, draw_breadcrumbs, draw_buildings, draw_drag_rect, draw_fx, draw_group_press_ring,
    draw_long_press_ring, draw_pending_founds, draw_pings, draw_placement_ghost, draw_rally_marker,
    draw_range_ground, draw_range_rings, draw_touch_box, draw_units,
};
pub use minimap::*;
use panel_draw::{draw_panel, draw_panel_tooltip};
use world::{draw_fog, draw_scorches, draw_tiles};

use crate::game::{EffectKind, Scene};
use crate::input::InputState;
use chassis::grid::TilePos;
use macroquad::prelude::*;
use oxide_sim::stats::SCRAP_NODE_AMOUNT;

pub(crate) use crate::theme::{
    SURFACE_CARD, TEXT_BODY, TEXT_DISABLED, TEXT_PRIMARY, TEXT_SECONDARY,
};

pub(crate) const OUTSIDE: Color = color_u8!(20, 20, 25, 255);
// World decoration (selection rings, rally poles, breadcrumbs) uses its
// own bone pair so raising the text contrast tiers in crate::theme never
// thickens world decoration.
const BONE: Color = color_u8!(232, 228, 216, 255);
const BONE_FAINT: Color = color_u8!(232, 228, 216, 90);
const SCRAP_COLOR: Color = crate::theme::TEXT_ACCENT;
const HP_BACK: Color = color_u8!(20, 20, 24, 220);
const DANGER: Color = crate::theme::TEXT_DANGER;
const PANEL: Color = crate::theme::SURFACE_PANEL;

fn capability_icon_color(icon: crate::panel::CapabilityIcon) -> Color {
    use crate::panel::CapabilityIcon;
    match icon {
        CapabilityIcon::Weapon => Color::new(0.85, 0.46, 0.36, 0.86),
        CapabilityIcon::AirWeapon => Color::new(0.90, 0.36, 0.50, 0.90),
        CapabilityIcon::DeadZone => Color::new(1.0, 0.68, 0.18, 0.92),
        CapabilityIcon::Vision => Color::new(0.63, 0.77, 0.94, 0.86),
        CapabilityIcon::Radar => Color::new(0.22, 0.76, 0.72, 0.90),
        CapabilityIcon::Repair => Color::new(0.38, 0.82, 0.45, 0.90),
        CapabilityIcon::EconomySupport => Color::new(0.95, 0.72, 0.24, 0.92),
        CapabilityIcon::Intercept => Color::new(0.64, 0.62, 1.0, 0.92),
    }
}

fn draw_capability_icon(
    center: Vec2,
    radius: f32,
    icon: crate::panel::CapabilityIcon,
    color: Color,
    plate: bool,
) {
    use crate::panel::CapabilityIcon;
    let radius = radius.max(2.0);
    let stroke = (radius * 0.18).clamp(1.0, 2.0);
    if plate {
        draw_circle(center.x, center.y, radius * 1.34, crate::theme::ICON_PLATE);
    }
    match icon {
        CapabilityIcon::Weapon | CapabilityIcon::AirWeapon | CapabilityIcon::DeadZone => {
            let reticle_radius = if icon == CapabilityIcon::AirWeapon {
                0.76
            } else {
                0.58
            };
            let tick_inner = if icon == CapabilityIcon::AirWeapon {
                0.84
            } else {
                0.38
            };
            draw_circle_lines(center.x, center.y, radius * reticle_radius, stroke, color);
            draw_line(
                center.x - radius,
                center.y,
                center.x - radius * tick_inner,
                center.y,
                stroke,
                color,
            );
            draw_line(
                center.x + radius * tick_inner,
                center.y,
                center.x + radius,
                center.y,
                stroke,
                color,
            );
            draw_line(
                center.x,
                center.y - radius,
                center.x,
                center.y - radius * tick_inner,
                stroke,
                color,
            );
            draw_line(
                center.x,
                center.y + radius * tick_inner,
                center.x,
                center.y + radius,
                stroke,
                color,
            );
            if icon == CapabilityIcon::AirWeapon {
                let aircraft = radius * 0.40;
                draw_line(
                    center.x,
                    center.y - aircraft,
                    center.x,
                    center.y + aircraft * 0.82,
                    stroke,
                    color,
                );
                draw_line(
                    center.x,
                    center.y - aircraft * 0.18,
                    center.x - aircraft,
                    center.y + aircraft * 0.34,
                    stroke,
                    color,
                );
                draw_line(
                    center.x,
                    center.y - aircraft * 0.18,
                    center.x + aircraft,
                    center.y + aircraft * 0.34,
                    stroke,
                    color,
                );
                draw_line(
                    center.x,
                    center.y + aircraft * 0.48,
                    center.x - aircraft * 0.48,
                    center.y + aircraft * 0.82,
                    stroke,
                    color,
                );
                draw_line(
                    center.x,
                    center.y + aircraft * 0.48,
                    center.x + aircraft * 0.48,
                    center.y + aircraft * 0.82,
                    stroke,
                    color,
                );
            }
            if icon == CapabilityIcon::DeadZone {
                draw_line(
                    center.x - radius * 0.78,
                    center.y + radius * 0.78,
                    center.x + radius * 0.78,
                    center.y - radius * 0.78,
                    stroke * 1.2,
                    color,
                );
            }
        }
        CapabilityIcon::Vision => {
            let left = vec2(center.x - radius, center.y);
            let right = vec2(center.x + radius, center.y);
            let upper = vec2(center.x, center.y - radius * 0.62);
            let lower = vec2(center.x, center.y + radius * 0.62);
            for (a, b) in [(left, upper), (upper, right), (right, lower), (lower, left)] {
                draw_line(a.x, a.y, b.x, b.y, stroke, color);
            }
            draw_circle(center.x, center.y, radius * 0.28, color);
        }
        CapabilityIcon::Radar => {
            let origin = center + vec2(-radius * 0.58, radius * 0.58);
            draw_circle(origin.x, origin.y, radius * 0.18, color);
            for ring in [0.62, 1.0] {
                let segments = 7;
                for segment in 0..segments {
                    let a = -std::f32::consts::FRAC_PI_2
                        + std::f32::consts::FRAC_PI_2 * segment as f32 / segments as f32;
                    let b = -std::f32::consts::FRAC_PI_2
                        + std::f32::consts::FRAC_PI_2 * (segment + 1) as f32 / segments as f32;
                    draw_line(
                        origin.x + a.cos() * radius * ring,
                        origin.y + a.sin() * radius * ring,
                        origin.x + b.cos() * radius * ring,
                        origin.y + b.sin() * radius * ring,
                        stroke,
                        color,
                    );
                }
            }
        }
        CapabilityIcon::Repair => {
            draw_line(
                center.x - radius,
                center.y,
                center.x + radius,
                center.y,
                stroke * 1.25,
                color,
            );
            draw_line(
                center.x,
                center.y - radius,
                center.x,
                center.y + radius,
                stroke * 1.25,
                color,
            );
        }
        CapabilityIcon::Intercept => {
            // A ring with a rising round inside it.
            draw_circle_lines(center.x, center.y, radius * 0.86, stroke, color);
            let tip = vec2(center.x, center.y - radius * 0.55);
            draw_line(
                tip.x,
                tip.y,
                center.x,
                center.y + radius * 0.5,
                stroke,
                color,
            );
            draw_line(
                tip.x,
                tip.y,
                tip.x - radius * 0.3,
                tip.y + radius * 0.32,
                stroke,
                color,
            );
            draw_line(
                tip.x,
                tip.y,
                tip.x + radius * 0.3,
                tip.y + radius * 0.32,
                stroke,
                color,
            );
        }
        CapabilityIcon::EconomySupport => {
            let left = center - vec2(radius * 0.48, 0.0);
            let right = center + vec2(radius * 0.48, 0.0);
            draw_circle_lines(left.x, left.y, radius * 0.42, stroke, color);
            draw_circle_lines(right.x, right.y, radius * 0.42, stroke, color);
            draw_line(
                left.x + radius * 0.22,
                left.y - radius * 0.22,
                right.x - radius * 0.22,
                right.y - radius * 0.22,
                stroke,
                color,
            );
            draw_line(
                left.x + radius * 0.22,
                left.y + radius * 0.22,
                right.x - radius * 0.22,
                right.y + radius * 0.22,
                stroke,
                color,
            );
        }
    }
}

/// The user's UI scale preference, stored as atomic f32 bits so the
/// settings screen can retune it live while draw and hit-test paths read
/// it lock-free.
#[cfg(not(test))]
static USER_SCALE: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(f32::to_bits(1.0));

// Parallel layout tests must not change each other's hit-test geometry.
#[cfg(test)]
thread_local! {
    static USER_SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// The smallest user scale: the Settings row's first step. Layouts are
/// checked only between this and [`MAX_USER_SCALE`].
pub const MIN_USER_SCALE: f32 = 0.75;

/// The largest user scale: the Settings row's last step.
pub const MAX_USER_SCALE: f32 = 1.5;

/// Installs the user scale factor, clamped to the range the Settings row
/// offers.
pub fn set_user_scale(factor: f32) {
    let factor = factor.clamp(MIN_USER_SCALE, MAX_USER_SCALE);
    #[cfg(not(test))]
    USER_SCALE.store(f32::to_bits(factor), std::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    USER_SCALE.set(factor);
}

static REDUCED_MOTION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Installs the accessibility damp: decorative animation (alert
/// pulses, ping rings, muzzle flashes) holds still when set.
pub fn set_reduced_motion(on: bool) {
    REDUCED_MOTION.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether decorative animation is damped.
pub fn reduced_motion() -> bool {
    REDUCED_MOTION.load(std::sync::atomic::Ordering::Relaxed)
}

/// UI scale factor: the user preference, capped by window size. Chrome
/// (text, bars, minimap) is authored in logical pixels, and
/// `screen_width()` and mouse coordinates are already logical because
/// macroquad's high-dpi backing store absorbs the display's pixel ratio,
/// so the DPI factor must not be applied here.
pub fn ui_scale() -> f32 {
    #[cfg(not(test))]
    let user = f32::from_bits(USER_SCALE.load(std::sync::atomic::Ordering::Relaxed));
    #[cfg(test)]
    let user = USER_SCALE.get();
    effective_ui_scale(user, viewport())
}

fn effective_ui_scale(user: f32, viewport: Vec2) -> f32 {
    // A narrow OR short window can't seat enlarged chrome. Width guards
    // horizontal packing; height keeps a 960x400 window from accepting
    // 150% cards that physically cannot fit even after the minimap yields.
    // The viewport is injected per frame, never queried from the window.
    let width_cap = (viewport.x / 640.0).max(1.0);
    let height_cap = (viewport.y / 400.0).max(1.0);
    user.min(width_cap).min(height_cap)
}

#[cfg(not(test))]
static VIEW_WIDTH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[cfg(not(test))]
static VIEW_HEIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// Under cfg(test) the storage is thread-local: libtest runs each test on
// its own thread, so a test that injects a small window cannot affect a
// concurrent layout test or leak into a later one, even by panicking.
// Every test thread starts at the 1280x800 default and needs no restore.
#[cfg(test)]
thread_local! {
    static VIEW: std::cell::Cell<(u32, u32)> = const { std::cell::Cell::new((0, 0)) };
}

/// Installs the window size; the frame loop calls this once per frame.
/// [`ui_scale`], menu layout, and session construction read it through
/// [`viewport`] instead of querying the window, so they run headless
/// (the default is 1280x800). HUD and world drawing query the window
/// directly.
#[cfg(not(test))]
pub fn set_viewport(w: f32, h: f32) {
    VIEW_WIDTH.store(w.to_bits(), std::sync::atomic::Ordering::Relaxed);
    VIEW_HEIGHT.store(h.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Test flavor of [`set_viewport`]: same contract, but the size is
/// this test thread's alone.
#[cfg(test)]
pub fn set_viewport(w: f32, h: f32) {
    VIEW.set((w.to_bits(), h.to_bits()));
}

/// The injected window size.
pub fn viewport() -> macroquad::prelude::Vec2 {
    macroquad::prelude::vec2(view_width(), view_height())
}

/// Breaks `text` into lines no wider than `max_w` under `measure`,
/// splitting only at spaces; a single word wider than the limit gets
/// its own line rather than being cut. Presentation-only text layout
/// for tooltips and the codex.
pub fn wrap_words(text: &str, measure: impl Fn(&str) -> f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_string()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && measure(&candidate) > max_w {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(not(test))]
fn view_bits() -> (u32, u32) {
    (
        VIEW_WIDTH.load(std::sync::atomic::Ordering::Relaxed),
        VIEW_HEIGHT.load(std::sync::atomic::Ordering::Relaxed),
    )
}

#[cfg(test)]
fn view_bits() -> (u32, u32) {
    VIEW.with(std::cell::Cell::get)
}

fn view_width() -> f32 {
    match view_bits().0 {
        0 => 1280.0,
        bits => f32::from_bits(bits),
    }
}

fn view_height() -> f32 {
    match view_bits().1 {
        0 => 800.0,
        bits => f32::from_bits(bits),
    }
}

/// Draws one frame.
pub fn draw(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    input: &InputState,
    bindings: &crate::action::BindingMap,
) {
    draw_with_performance(game, sprites, input, bindings, None);
}

pub(crate) fn draw_with_performance(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    input: &InputState,
    bindings: &crate::action::BindingMap,
    performance: Option<&crate::performance::PerformanceView>,
) {
    let hud = hud::refresh(
        game,
        input,
        bindings,
        hud::HudEnv::current(),
        performance,
        &hud::window_measure,
    );
    clear_background(OUTSIDE);
    environment::draw_backdrop(game);
    let alpha = game.clock.render_alpha();
    let ground = game.ground();
    let fractured = sprites.quarry_dressing(0).is_some();
    draw_tiles(game, sprites);
    world::draw_blockers(game, sprites, &ground.blockers);
    pits::draw_pits(game, &ground.pits, fractured);
    boulders::draw_boulders(game, sprites, &ground.slabs);
    crate::render::world::draw_extractor_frames(game, sprites);
    terraces::draw_terraces(game, &ground.terraces, fractured);
    draw_scorches(game, sprites);
    destruction::draw_ground_effects(game, sprites);
    draw_range_ground(game, input);
    strategic::draw_strike_ground(game, input);
    crate::strategic_markers::draw_resources(game);
    crate::strategic_markers::draw_extractor_frames(game);
    draw_buildings(game, sprites);
    entities::draw_selected_pile(game);
    crate::strategic_markers::draw_buildings(game);
    draw_units(game, sprites, alpha);
    crate::strategic_markers::draw_markers(game, alpha);
    draw_fx(game, sprites);
    // The debug overlay is deliberately omniscient; the spectator
    // stance (playback) and a seat out of the match skip the fog too but
    // never the debug chrome.
    if game.presentation.overlay {
        draw_overlay(game, alpha);
    } else if !game.presentation.all_seeing() {
        draw_fog(game);
    }
    // Own-order acknowledgments, rally flags, and radar blips sit above
    // the fog: they are the player's intent and intel, not world state.
    draw_pings(game);
    draw_range_rings(game, input);
    draw_blips(game);
    strategic::draw_warheads(game, sprites);
    entities::draw_public_buildings(game);
    entities::draw_repair_tethers(game);
    draw_rally_marker(game, alpha);
    strategic::draw_strike_preview(game, input);
    draw_breadcrumbs(game, input);
    // Deferred claims are the player's own intent, like breadcrumbs; a
    // spectator has none.
    if !game.presentation.spectating() {
        draw_pending_founds(game, sprites);
    }
    draw_placement_ghost(game, sprites, input);
    draw_drag_rect(game, input);
    draw_touch_box(game, input);
    draw_long_press_ring(input);
    draw_salvage_tooltip(game, input);
    draw_hud(game, sprites, input, bindings, &hud, performance);
    if game.presentation.overlay {
        draw_overlay_info(game);
    }
    draw_minimap(game);
    chrome::draw_ghost_controls(game, input);
    draw_group_press_ring(input);
    draw_result_overlay(game);
    draw_panel_tooltip(game, input);
}

const FOG_UNEXPLORED: Color = color_u8!(13, 13, 17, 255);
const FOG_EXPLORED: Color = color_u8!(13, 13, 17, 135);

/// Ground layouts that read the map alone, built on a match's first draw and
/// kept for it: terrain never changes once parsed, and the fog veil hides
/// whatever the player has not explored.
pub(crate) struct GroundLayout {
    blockers: world::Blockers,
    slabs: boulders::Slabs,
    pits: pits::PitField,
    terraces: terraces::Terraces,
    levels: Option<levels::Levels>,
}

impl GroundLayout {
    pub(crate) fn build(map: &oxide_sim::map::Map, scenario: &oxide_sim::Scenario) -> Self {
        let rows = &scenario.map;
        let blockers = world::Blockers::build(map, rows);
        // A tiered map draws the level its Foundries start on at flat
        // ground's brightness.
        let starts = scenario.parse_map_and_anchors().map_or_else(
            |_| Vec::new(),
            |(_, anchors)| anchors.into_iter().map(|(_, pos)| pos).collect(),
        );
        Self {
            slabs: boulders::Slabs::build(map, rows, &blockers),
            blockers,
            pits: pits::PitField::build(map),
            terraces: terraces::Terraces::build(map, rows),
            levels: levels::Levels::build(map, oxide_kit::render::floor_level(map, starts)),
        }
    }
}

fn visible_tiles(game: &crate::game::Scene<'_>) -> (TilePos, TilePos) {
    let (lo, hi) = game.presentation.camera.world_rect();
    let min = TilePos::new(
        numeric::to_i32(lo.x.floor()).max(0),
        numeric::to_i32(lo.y.floor()).max(0),
    );
    let max = TilePos::new(
        numeric::to_i32(hi.x.ceil()).min(game.state.map().width()),
        numeric::to_i32(hi.y.ceil()).min(game.state.map().height()),
    );
    (min, max)
}

/// Per-theme terrain grading: a subtle multiplier on ground-layer
/// sprites only. Units, chrome, the minimap, and the golden renderer
/// stay untinted; grading is atmosphere, never information.
pub fn theme_tint(theme: &str) -> Color {
    match theme {
        "rusted-yard" => Color::new(1.0, 0.95, 0.88, 1.0),
        "cold-circuitry" => Color::new(0.89, 0.95, 1.0, 1.0),
        "quarry-dust" => Color::new(1.0, 0.97, 0.90, 1.0),
        "basalt" => Color::new(0.92, 0.91, 1.0, 1.0),
        "slag" => Color::new(1.0, 0.92, 0.92, 1.0),
        "verdigris" => Color::new(0.90, 1.0, 0.94, 1.0),
        _ => WHITE,
    }
}

const GHOST_TINT: Color = color_u8!(150, 150, 165, 210);

fn unit_work_facing(
    from: chassis::fx::Vec2Fx,
    work: crate::presentation_animation::UnitWorkState,
) -> Option<f32> {
    use crate::presentation_animation::UnitWorkState;
    let target = match work {
        UnitWorkState::Harvesting { target, .. }
        | UnitWorkState::Constructing { target, .. }
        | UnitWorkState::Repairing { target, .. }
        | UnitWorkState::Salvaging { target, .. }
        | UnitWorkState::Unloading { target, .. } => target,
        UnitWorkState::Idle => return None,
    };
    let to_screen_space =
        |point: chassis::fx::Vec2Fx| vec2(point.x.to_num::<f32>(), point.y.to_num::<f32>());
    let direction = to_screen_space(target) - to_screen_space(from);
    (direction.length_squared() > 1e-6)
        .then(|| direction.y.atan2(direction.x) + std::f32::consts::FRAC_PI_2)
}

fn unit_selection_radius(kind: oxide_sim::UnitKind, zoom: f32, padding: f32) -> f32 {
    let collision_radius = kind.stats().radius.to_num::<f32>();
    let visual_radius = unit_visual_radius(kind);
    collision_radius.max(visual_radius) * zoom + padding
}

/// How far past the view a unit may stand and still reach into it: a
/// sprite wider than the usual margin reaches in from further out.
fn unit_cull_margin(kind: oxide_sim::UnitKind, margin: f32) -> f32 {
    margin.max(unit_visual_radius(kind) + 1.0)
}

/// A health bar's width in tiles: about the body, up to its whole width on
/// a hull wider than a tile.
fn hp_bar_tiles(kind: oxide_sim::UnitKind, draw_scale: f32) -> f32 {
    let widest = 1.4f32.max(2.0 * kind.stats().radius.to_num::<f32>());
    (draw_scale - 0.25).clamp(0.8, widest)
}

pub(crate) fn unit_visual_radius(kind: oxide_sim::UnitKind) -> f32 {
    unit_draw_scale(kind) * crate::look::unit(kind).extent * 0.5
}

pub(crate) fn unit_draw_scale(kind: oxide_sim::UnitKind) -> f32 {
    crate::look::unit(kind).scale
}

/// A flyer's shadow size, shadow offset and body lift at `zoom`.
pub(crate) fn air_presentation(kind: oxide_sim::UnitKind, zoom: f32) -> (Vec2, Vec2, f32) {
    let airframe = crate::look::unit(kind)
        .airframe
        .unwrap_or(crate::look::SMALL_AIRFRAME);
    (
        airframe.shadow * zoom,
        airframe.shadow_offset * zoom,
        airframe.lift * zoom,
    )
}

fn tracked_mount_angle(
    game: &crate::game::Scene<'_>,
    unit: &oxide_sim::Unit,
    alpha: f32,
) -> Option<f32> {
    let target = game
        .presentation
        .aim_unit_targets
        .get(&unit.id.0)
        .copied()
        .or_else(|| {
            if unit.kind.stats().demolition.is_some()
                && let oxide_sim::Order::Attack { target, .. } = unit.order
            {
                game.state
                    .attack_view(unit.player, target)
                    .and_then(|view| view.entity)
            } else {
                None
            }
        })?;
    let position = match target {
        oxide_sim::Target::Unit(id) => {
            let target = game.state.unit(id)?;
            if target.player != game.presentation.human
                && !game.presentation.all_seeing()
                && !game.my_vision().sees_unit(target)
            {
                return None;
            }
            game.presentation.draw_pos(target.id, target.pos, alpha)
        }
        oxide_sim::Target::Building(id) => {
            let target = game.state.building(id)?;
            if !game.presentation.all_seeing()
                && (!target.tiles().any(|tile| game.my_vision().visible(tile))
                    || !game
                        .state
                        .building_apparent(game.presentation.human, target))
            {
                return None;
            }
            let from = game.presentation.draw_pos(unit.id, unit.pos, alpha);
            let (width, height) = target.kind.size();
            from.clamp(
                vec2(target.anchor.x as f32, target.anchor.y as f32),
                vec2(
                    (target.anchor.x + width) as f32,
                    (target.anchor.y + height) as f32,
                ),
            )
        }
    };
    let direction = position - game.presentation.draw_pos(unit.id, unit.pos, alpha);
    (direction.length_squared() > 1e-6)
        .then(|| direction.y.atan2(direction.x) + std::f32::consts::FRAC_PI_2)
}

pub(crate) struct UnitBodyPose {
    pub center: Vec2,
    pub size: f32,
    pub source: Rect,
    accent: Rect,
    mechanism_layer: Option<Rect>,
    rotation: f32,
    pub body_rotation: f32,
    worker: bool,
    animation: crate::presentation_animation::UnitAnimationState,
}

pub(crate) fn unit_animation(
    presentation: &crate::game::Presentation,
    state: &oxide_sim::State,
    unit: &oxide_sim::Unit,
) -> crate::presentation_animation::UnitAnimationState {
    let current = vec2(unit.pos.x.to_num::<f32>(), unit.pos.y.to_num::<f32>());
    let moving = presentation
        .prev_pos
        .get(&unit.id.0)
        .is_some_and(|previous| (*previous - current).length_squared() > 1e-6);
    presentation.animations.unit_state(
        crate::presentation_animation::UnitAnimationFacts::capture(
            state,
            unit,
            moving || presentation.chassis_turning(unit),
        ),
        crate::presentation_animation::AnimationClock::from_state(
            state,
            presentation.tick_fraction(),
        ),
        crate::presentation_animation::AnimationOptions {
            reduced_motion: reduced_motion(),
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct UnitSpriteFrame {
    frame: motion::UnitFrame,
    hull_phase: u8,
    worker_phase: u8,
    cargo: u8,
    point_defense: Option<u8>,
}

impl UnitSpriteFrame {
    pub(crate) fn capture(
        kind: oxide_sim::UnitKind,
        animation: crate::presentation_animation::UnitAnimationState,
    ) -> Self {
        let mut frame = motion::unit_frame(kind, animation);
        if tracks::supported(kind) {
            match &mut frame {
                motion::UnitFrame::Moving(_) => frame = motion::UnitFrame::Idle,
                motion::UnitFrame::Harvester { pose, .. }
                    if matches!(pose, motion::HarvesterPose::Moving(_)) =>
                {
                    *pose = motion::HarvesterPose::Idle;
                }
                motion::UnitFrame::Excavator { pose, .. }
                    if matches!(pose, motion::ExcavatorPose::Moving(_)) =>
                {
                    *pose = motion::ExcavatorPose::Idle;
                }
                _ => {}
            }
        }
        let worker_phase = match animation.locomotion {
            crate::presentation_animation::LocomotionState::Moving { cycle }
                if !tracks::supported(kind) =>
            {
                motion::tread_phase(cycle)
            }
            _ => 0,
        };
        let hull_phase = match animation.propulsion {
            crate::presentation_animation::PropulsionState::LiftRotors { cycle } => {
                numeric::to_usize(cycle * 3.0).min(2)
            }
            crate::presentation_animation::PropulsionState::None => worker_phase,
        };
        Self {
            frame,
            hull_phase: hull_phase.fit::<u8>(),
            worker_phase: worker_phase.fit::<u8>(),
            point_defense: animation.point_defense.map(|held| {
                let steps = crate::assets::MANTLE_RECHARGE_STEPS;
                let charge = usize::from(held.charges.min(3)) * steps;
                let progress = if held.charges >= 3 {
                    0
                } else {
                    numeric::to_usize(held.recharge.clamp(0.0, 1.0) * steps as f32).min(steps - 1)
                };
                (charge + progress).fit::<u8>()
            }),
            cargo: animation
                .cargo
                .map_or(0, |cargo| numeric::to_u8((cargo.fill * 5.0).ceil())),
        }
    }
}

pub(crate) fn unit_body_sources(
    sprites: &Sprites,
    kind: oxide_sim::UnitKind,
    frame: UnitSpriteFrame,
) -> (Rect, Rect, Option<Rect>, bool) {
    let (source, accent, mechanism_layer) = match frame.frame {
        motion::UnitFrame::Idle => (sprites.unit(kind), sprites.unit_accent(kind), None),
        motion::UnitFrame::Moving(phase) => (
            sprites.unit_moving(kind, phase + 1),
            sprites.unit_moving_accent(kind, phase + 1),
            None,
        ),
        motion::UnitFrame::Action(action) => (
            sprites.unit_action(kind, action),
            sprites.unit_action_accent(kind, action),
            None,
        ),
        motion::UnitFrame::Harvester { cargo, pose } => {
            let pose = match pose {
                motion::HarvesterPose::Idle => SpriteHarvesterPose::Idle,
                motion::HarvesterPose::Moving(0) => SpriteHarvesterPose::Tread1,
                motion::HarvesterPose::Moving(_) => SpriteHarvesterPose::Tread2,
                motion::HarvesterPose::Scoop(0) => SpriteHarvesterPose::Scoop1,
                motion::HarvesterPose::Scoop(_) => SpriteHarvesterPose::Scoop2,
            };
            (
                sprites.harvester_frame(cargo, pose),
                sprites.harvester_frame_accent(cargo, pose),
                None,
            )
        }
        motion::UnitFrame::Excavator { cargo, pose } => {
            let pose = match pose {
                motion::ExcavatorPose::Idle => SpriteExcavatorPose::Idle,
                motion::ExcavatorPose::Moving(0) => SpriteExcavatorPose::Tread1,
                motion::ExcavatorPose::Moving(_) => SpriteExcavatorPose::Tread2,
                motion::ExcavatorPose::Working(0) => SpriteExcavatorPose::Work1,
                motion::ExcavatorPose::Working(1) => SpriteExcavatorPose::Work2,
                motion::ExcavatorPose::Working(2) => SpriteExcavatorPose::Work3,
                motion::ExcavatorPose::Working(_) => SpriteExcavatorPose::Work4,
            };
            (
                sprites.excavator_frame(pose),
                sprites.excavator_frame_accent(pose),
                Some(sprites.excavator_cargo(cargo)),
            )
        }
    };
    let (source, accent) = sprites.unit_rig(kind).map_or((source, accent), |rig| {
        rig.hull(if tracks::supported(kind) {
            0
        } else {
            usize::from(frame.hull_phase)
        })
    });
    let worker_body = sprites.worker_body(
        kind,
        usize::from(frame.cargo),
        usize::from(frame.worker_phase),
    );
    let (source, accent) = worker_body.unwrap_or((source, accent));
    let mechanism_layer = if kind == oxide_sim::UnitKind::Mantle {
        frame
            .point_defense
            .map(|frame| sprites.mantle_charge(usize::from(frame)))
    } else {
        mechanism_layer
    };
    (source, accent, mechanism_layer, worker_body.is_some())
}

pub(crate) fn unit_body_pose(
    game: &Scene<'_>,
    sprites: &Sprites,
    unit: &oxide_sim::Unit,
    alpha: f32,
) -> UnitBodyPose {
    let animation = unit_animation(game.presentation, game.state, unit);
    let frame = UnitSpriteFrame::capture(unit.kind, animation);
    let moving = game
        .presentation
        .prev_pos
        .get(&unit.id.0)
        .is_some_and(|previous| {
            (*previous - crate::game::world_vec(unit.pos)).length_squared() > 1e-6
        });
    let preparing = animation.weapons.iter().any(|cycle| {
        matches!(
            cycle,
            crate::presentation_animation::WeaponCycle::Preparing { .. }
        )
    });
    // Report/recovery owns the heading. A stationary heavy weapon
    // then keeps that aim throughout its physical reload; real
    // locomotion resumes movement facing instead of sliding sideways.
    let rig = sprites.unit_rig(unit.kind);
    let aim = game
        .presentation
        .aim_units
        .get(&unit.id.0)
        .copied()
        .map(|(angle, at)| {
            let angle = if rig.is_some() && animation.attack.is_none() {
                tracked_mount_angle(game, unit, alpha).unwrap_or(angle)
            } else {
                angle
            };
            (angle, at)
        });
    let work_facing = unit_work_facing(unit.pos, animation.work);
    let contact_facing = animation
        .demolition_preparation
        .and_then(|_| tracked_mount_angle(game, unit, alpha));
    let rotation = if unit.kind.stats().turn_rate > 0
        || unit.kind.ground_turn_rate() > 0
        || unit.kind.stats().cruise_turn_rate > 0
        || unit.kind.stats().turret_turn_rate > 0
    {
        game.presentation
            .draw_heading(unit.id, unit.weapon_heading(), alpha)
    } else if crate::game::rotor_hull_turn_rate(unit.kind).is_some() {
        game.draw_hull_heading(unit.id, alpha)
    } else if let Some(angle) = contact_facing {
        angle
    } else {
        match aim {
            Some((angle, at))
                if animation.attack.is_some()
                    || (rig.is_some() || !moving)
                        && (preparing || game.presentation.fx_time() - at < 1.2) =>
            {
                angle
            }
            _ => work_facing.unwrap_or_else(|| {
                game.presentation
                    .facing
                    .get(&unit.id.0)
                    .copied()
                    .unwrap_or(0.0)
            }),
        }
    };
    // A turreted rig leans only its hull; the mount keeps its true aim.
    let slide_yaw = game
        .presentation
        .slide_yaw(unit.id, alpha, reduced_motion());
    let rotation = if rig.is_some() {
        rotation
    } else {
        rotation + slide_yaw
    };
    let body_rotation = match rig {
        // Mounted guns ride a hull without a turret, so the heading drawn
        // above is the hull's own.
        Some(_) if unit.kind.stats().mounts.is_some() => rotation + slide_yaw,
        Some(_) => game.draw_hull_heading(unit.id, alpha) + slide_yaw,
        None => rotation,
    };
    let (source, accent, mechanism_layer, worker) = unit_body_sources(sprites, unit.kind, frame);
    let mut center = roc::drone_draw_pos(game, unit, alpha)
        .unwrap_or_else(|| game.presentation.draw_pos(unit.id, unit.pos, alpha));
    if unit.domain() == oxide_sim::stats::Domain::Air {
        center.y -= air_presentation(unit.kind, 1.0).2;
    }
    UnitBodyPose {
        center,
        size: unit_draw_scale(unit.kind),
        source,
        accent,
        mechanism_layer,
        rotation,
        body_rotation,
        worker,
        animation,
    }
}

fn draw_piledriver_anchors(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    unit: &oxide_sim::Unit,
    pose: (Vec2, f32, f32),
    zoom: f32,
) {
    let (body, body_rotation, dest) = pose;
    let deployment = game.presentation.animations.brace_deployment(
        unit,
        crate::presentation_animation::AnimationClock::from_state(
            game.state,
            game.presentation.tick_fraction(),
        ),
    );
    if let Some((anchor, accent)) = sprites.piledriver_anchors(deployment) {
        for (source, tint) in [
            (anchor, WHITE),
            (accent, seat_identity_color(game, unit.player)),
        ] {
            let factor = dest / 128.0;
            let center = body + vec2(-body_rotation.sin(), body_rotation.cos()) * (13.0 * factor);
            let size = vec2(source.w, source.h) * factor;
            sprites.draw_unit(
                center.x - size.x * 0.5,
                center.y - size.y * 0.5,
                tint,
                DrawTextureParams {
                    dest_size: Some(size),
                    source: Some(source),
                    rotation: body_rotation,
                    ..Default::default()
                },
                zoom,
            );
        }
    }
}

/// Where a mounted gun pivots on a hull drawn at `body`, turned by
/// `body_rotation`: the simulation's offsets, in tiles.
fn mount_pivot(
    body: Vec2,
    body_rotation: f32,
    mount: &oxide_sim::stats::MountStats,
    zoom: f32,
) -> Vec2 {
    let forward = vec2(body_rotation.sin(), -body_rotation.cos());
    let starboard = vec2(body_rotation.cos(), body_rotation.sin());
    body + forward * (mount.forward.to_num::<f32>() * zoom)
        + starboard * (mount.starboard.to_num::<f32>() * zoom)
}

/// Each mounted gun on its pivot, at its own interpolated bearing and in
/// its own recoil. The pivots ride the hull at the simulation's offsets,
/// and a gun's smaller canvas keeps the hull canvas's pixel density.
fn draw_mounted_guns(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    unit: &oxide_sim::Unit,
    rig: &crate::assets::UnitRig,
    (body, body_rotation, body_size): (Vec2, f32, Vec2),
    hull: Rect,
    alpha: f32,
) {
    let Some(guns) = unit.kind.stats().mounts else {
        return;
    };
    let zoom = game.presentation.camera.zoom;
    let clock = crate::presentation_animation::AnimationClock::from_state(
        game.state,
        game.presentation.tick_fraction(),
    );
    for (slot, mount) in guns.mounts.iter().enumerate() {
        let Some(rotation) = game.presentation.draw_mount_heading(unit, slot, alpha) else {
            continue;
        };
        let pivot = mount_pivot(body, body_rotation, mount, zoom);
        let action = game
            .presentation
            .animations
            .mount_attack(unit.id, unit.kind, slot, clock)
            .map(|attack| motion::unit_attack_frame(unit.kind, attack));
        let (gun, accent) = rig.mount(action);
        for (source, tint) in [
            (gun, WHITE),
            (accent, seat_identity_color(game, unit.player)),
        ] {
            let size = vec2(source.w, source.h) * (body_size.x / hull.w);
            sprites.draw_unit(
                pivot.x - size.x * 0.5,
                pivot.y - size.y * 0.5,
                tint,
                DrawTextureParams {
                    dest_size: Some(size),
                    source: Some(source),
                    rotation,
                    ..Default::default()
                },
                zoom,
            );
        }
    }
}

/// A unit's chassis and allegiance mask: the Maw from its runtime layers,
/// everything else from its frame with its drawn belts between the two.
fn draw_unit_body(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    unit: &oxide_sim::Unit,
    (body, body_rotation, body_size): (Vec2, f32, Vec2),
    (source, accent): (Rect, Rect),
    (draw_scale, alpha): (f32, f32),
) {
    let zoom = game.presentation.camera.zoom;
    let maw_rig = (unit.kind == oxide_sim::UnitKind::Maw)
        .then(|| sprites.maw_rigs())
        .flatten();
    if let Some(rigs) = maw_rig {
        let treads = game
            .presentation
            .track_motion
            .get(&unit.id.0)
            .map_or([0.0; 2], |motion| motion.distances(alpha));
        maw::draw(
            sprites,
            &rigs.maw,
            (body, body_rotation, body_size.x),
            seat_identity_color(game, unit.player),
            &maw::MawPose {
                treads,
                motion: game.presentation.maw_motion.get(&unit.id.0),
                alpha,
                still: reduced_motion(),
            },
            zoom,
        );
    } else {
        let params = DrawTextureParams {
            dest_size: Some(body_size),
            source: Some(source),
            rotation: body_rotation,
            ..Default::default()
        };
        sprites.draw_unit(
            body.x - body_size.x * 0.5,
            body.y - body_size.y * 0.5,
            WHITE,
            params.clone(),
            zoom,
        );
        if let Some(motion) = game.presentation.track_motion.get(&unit.id.0) {
            tracks::draw(
                unit.kind,
                body,
                body_size.x,
                body_rotation,
                if reduced_motion() {
                    [0.0; 2]
                } else {
                    motion.distances(alpha)
                },
                draw_scale,
            );
        }
        sprites.draw_unit(
            body.x - body_size.x * 0.5,
            body.y - body_size.y * 0.5,
            seat_identity_color(game, unit.player),
            DrawTextureParams {
                source: Some(accent),
                ..params
            },
            zoom,
        );
    }
}

/// An aircraft's ground shadow beneath `screen`, where it stands unlifted:
/// its rig's own shadows, or the shared airframe shadow.
fn draw_air_shadow(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    unit: &oxide_sim::Unit,
    rig: Option<&crate::assets::UnitRig>,
    (screen, body_rotation, dest): (Vec2, f32, f32),
    alpha: f32,
) {
    let zoom = game.presentation.camera.zoom;
    let (shadow_size, shadow_offset, body_lift) = air_presentation(unit.kind, zoom);
    if let Some(rig) = rig.filter(|rig| rig.shadow().is_some()) {
        roc::draw_shadows(
            game,
            sprites,
            unit,
            rig,
            (
                screen - vec2(0.0, body_lift),
                body_rotation,
                vec2(dest, dest),
            ),
            alpha,
        );
    } else {
        sprites.draw_unit(
            screen.x - shadow_size.x * 0.5 + shadow_offset.x,
            screen.y - shadow_size.y * 0.5 + shadow_offset.y,
            WHITE,
            DrawTextureParams {
                dest_size: Some(shadow_size),
                source: Some(sprites.air_shadow()),
                ..Default::default()
            },
            zoom,
        );
    }
}

/// A rig's single mount over its hull: a turret at its aim, a weaponless or
/// piledriving mechanism with the hull.
fn draw_rig_mount(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    unit: &oxide_sim::Unit,
    rig: &crate::assets::UnitRig,
    (body, body_size, rotation, body_rotation): (Vec2, Vec2, f32, f32),
    animation: crate::presentation_animation::UnitAnimationState,
) {
    let zoom = game.presentation.camera.zoom;
    let dest = body_size.x;
    let action = match motion::unit_mount_frame(unit.kind, animation) {
        motion::UnitFrame::Action(action) => Some(action),
        _ => None,
    };
    let (mount, accent) = rig.mount(action);
    for (source, tint) in [
        (mount, WHITE),
        (accent, seat_identity_color(game, unit.player)),
    ] {
        let mount_rotation = if unit.kind.stats().weapons.is_empty()
            || unit.kind == oxide_sim::UnitKind::Piledriver
        {
            body_rotation
        } else {
            rotation
        };
        let (center, size) = if unit.kind == oxide_sim::UnitKind::Piledriver {
            // The cropped mechanism retains its position on the 128-pixel chassis.
            let offset = vec2(4.0, -8.0) * (dest / 128.0);
            let (sin, cos) = mount_rotation.sin_cos();
            (
                body + vec2(
                    offset.x * cos - offset.y * sin,
                    offset.x * sin + offset.y * cos,
                ),
                vec2(source.w, source.h) * (dest / 128.0),
            )
        } else {
            (body, body_size)
        };
        sprites.draw_unit(
            center.x - size.x * 0.5,
            center.y - size.y * 0.5,
            tint,
            DrawTextureParams {
                dest_size: Some(size),
                source: Some(source),
                rotation: mount_rotation,
                ..Default::default()
            },
            zoom,
        );
    }
}

fn draw_unit_pass(
    game: &crate::game::Scene<'_>,
    sprites: &Sprites,
    alpha: f32,
    domain: oxide_sim::stats::Domain,
) {
    const CULL_MARGIN: f32 = 2.5;
    let zoom = game.presentation.camera.zoom;
    let airborne = domain == oxide_sim::stats::Domain::Air;
    // Frustum cull with a margin covering the sprite, its shadow, rings,
    // and bars.
    let (view_lo, view_hi) = game.presentation.camera.world_rect();
    for unit in game.state.units() {
        // The body's current layer, not its kind's: a parked airframe
        // draws among ground bodies with no shadow or lift.
        if unit.domain() != domain {
            continue;
        }
        if !crate::strategic_markers::visible(game, unit) {
            continue;
        }
        let pos = roc::drone_draw_pos(game, unit, alpha)
            .unwrap_or_else(|| game.presentation.draw_pos(unit.id, unit.pos, alpha));
        let margin = unit_cull_margin(unit.kind, CULL_MARGIN);
        if pos.x < view_lo.x - margin
            || pos.y < view_lo.y - margin
            || pos.x > view_hi.x + margin
            || pos.y > view_hi.y + margin
        {
            continue;
        }
        let mut screen = game.presentation.camera.to_screen(pos);
        if crate::strategic_markers::replaces_units(zoom) {
            continue;
        }
        let draw_scale = unit_draw_scale(unit.kind);
        let dest = zoom * draw_scale;
        let UnitBodyPose {
            source,
            accent,
            mechanism_layer,
            rotation,
            body_rotation,
            worker: worker_body,
            animation,
            ..
        } = unit_body_pose(game, sprites, unit, alpha);
        let rig = sprites.unit_rig(unit.kind);
        let roc_rig = (unit.kind == oxide_sim::UnitKind::Roc)
            .then(|| sprites.roc_rig())
            .flatten();
        if airborne {
            let body_lift = air_presentation(unit.kind, zoom).2;
            draw_air_shadow(
                game,
                sprites,
                unit,
                rig,
                (screen, body_rotation, dest),
                alpha,
            );
            // The body rides visibly above its shadow.
            screen.y -= body_lift;
        }
        let body = screen;
        if game.presentation.selection.units.contains(&unit.id) {
            let owner = seat_identity_color(game, unit.player);
            if unit.player == game.presentation.human {
                draw_circle_lines(
                    screen.x,
                    screen.y,
                    unit_selection_radius(unit.kind, zoom, 4.0),
                    2.0,
                    owner,
                );
            } else {
                // Inspected, not commanded: a thinner, wider ring, so a
                // selection the player cannot order never reads as one
                // they can.
                draw_circle_lines(
                    screen.x,
                    screen.y,
                    unit_selection_radius(unit.kind, zoom, 5.5),
                    1.5,
                    Color { a: 0.8, ..owner },
                );
            }
        }
        let body_size = vec2(dest, dest);
        if unit.kind == oxide_sim::UnitKind::Bombard
            && let Some(source) = sprites.bombard_spades(unit.braces())
        {
            sprites.draw_unit(
                body.x - body_size.x * 0.5,
                body.y - body_size.y * 0.5,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(body_size),
                    source: Some(source),
                    rotation,
                    ..Default::default()
                },
                zoom,
            );
        }
        if let (Some(rig), Some(roc)) = (rig, roc_rig) {
            roc::draw_under(
                game,
                sprites,
                unit,
                (rig, roc),
                (body, body_rotation, body_size),
                source,
                alpha,
            );
        }
        draw_unit_body(
            game,
            sprites,
            unit,
            (body, body_rotation, body_size),
            (source, accent),
            (draw_scale, alpha),
        );
        if worker_body {
            worker::draw(
                game,
                sprites,
                unit,
                animation,
                (body, body_rotation, body_size.x),
                alpha,
            );
        }
        if worker_body {
            worker::draw_welder(
                game,
                sprites,
                unit,
                animation,
                (body, body_rotation, body_size.x),
                alpha,
            );
        }
        if unit.kind == oxide_sim::UnitKind::Kestrel
            && let Some(cycle) = animation.scanner
            && let Some(source) = sprites.scout_radar()
        {
            let mount_y = 47.0;
            let offset = (mount_y - 64.0) / 128.0 * body_size.y;
            let center = body + vec2(-body_rotation.sin(), body_rotation.cos()) * offset;
            sprites.draw_unit(
                center.x - body_size.x * 0.5,
                center.y - body_size.y * 0.5,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(body_size),
                    source: Some(source),
                    rotation: body_rotation + cycle * std::f32::consts::TAU,
                    ..Default::default()
                },
                zoom,
            );
        }
        if unit.kind == oxide_sim::UnitKind::Piledriver {
            draw_piledriver_anchors(game, sprites, unit, (body, body_rotation, dest), zoom);
        }
        if let Some(rig) = rig
            && unit.kind.stats().mounts.is_some()
        {
            // The Roc's rig draws its guns under the hull.
            if roc_rig.is_none() {
                draw_mounted_guns(
                    game,
                    sprites,
                    unit,
                    rig,
                    (body, body_rotation, body_size),
                    source,
                    alpha,
                );
            }
        } else if let Some(rig) = rig {
            draw_rig_mount(
                game,
                sprites,
                unit,
                rig,
                (body, body_size, rotation, body_rotation),
                animation,
            );
        }
        if let Some(roc) = roc_rig {
            roc::draw_over(
                game,
                sprites,
                unit,
                roc,
                (body, body_rotation, body_size),
                animation,
                source,
            );
        }
        if let Some(mechanism_layer) = mechanism_layer {
            sprites.draw_unit(
                body.x - body_size.x * 0.5,
                body.y - body_size.y * 0.5,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(body_size),
                    source: Some(mechanism_layer),
                    rotation,
                    ..Default::default()
                },
                zoom,
            );
        }
        let max_hp = unit.kind.stats().max_hp;
        let w = zoom * hp_bar_tiles(unit.kind, draw_scale);
        if unit.hp < max_hp {
            hp_bar(
                screen.x - w * 0.5,
                screen.y - zoom * (draw_scale * 0.5 + 0.095),
                w,
                unit.hp,
                max_hp,
                seat_identity_color(game, unit.player),
            );
        }
        // The gold dismantling meter, like a building's teardown bar, and
        // like it shown only for the viewer's own machines.
        if unit.player == game.presentation.human && unit.retiring() && unit.progress > 0 {
            let fraction = unit.progress as f32 / oxide_sim::stats::RETIRE_TICKS as f32;
            let (x, y) = (
                screen.x - w * 0.5,
                screen.y + zoom * (draw_scale * 0.5 + 0.05),
            );
            draw_rectangle(x, y, w, 3.0, HP_BACK);
            draw_rectangle(x, y, w * fraction.min(1.0), 3.0, SCRAP_COLOR);
        }
    }
}

/// A little pennant marking a selected building's rally tile.
fn draw_rally_flag(game: &crate::game::Scene<'_>, rally: TilePos, zoom: f32) {
    let base = game
        .presentation
        .camera
        .to_screen(vec2(rally.x as f32 + 0.5, rally.y as f32 + 0.5));
    let pole_top = base - vec2(0.0, zoom * 0.7);
    draw_line(base.x, base.y, pole_top.x, pole_top.y, 2.0, BONE);
    draw_triangle(
        pole_top,
        pole_top + vec2(zoom * 0.45, zoom * 0.15),
        pole_top + vec2(0.0, zoom * 0.3),
        SCRAP_COLOR,
    );
    draw_circle(base.x, base.y, 3.0, BONE);
}

/// A health bar in its owner's color; the length alone shows the damage.
fn hp_bar(x: f32, y: f32, w: f32, hp: u32, max_hp: u32, owner: Color) {
    let fraction = hp as f32 / max_hp as f32;
    draw_rectangle(x, y, w, 3.0, HP_BACK);
    draw_rectangle(x, y, w * fraction, 3.0, owner);
}

/// How many selected units draw their rings and programs, so a large
/// selection does not stack dozens of overlapping circles.
const DECOR_CAP: usize = 12;

#[cfg(test)]
mod tests;
