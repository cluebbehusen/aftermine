//! The input funnel.
//!
//! macroquad's polled state becomes [`RawEvent`]s, injected events from the
//! debug socket are appended to the same list, and [`apply_events`] is the
//! only code that turns events into camera motion, selection, or sim
//! commands. Input that bypasses this module escapes every injected-event
//! test.

use crate::action::{Action, ActionEvent, ActionResolver, BindingMap};
use crate::game::{Game, PingKind};
use crate::numeric;
use crate::numeric::Fit;
use chassis::grid::TilePos;
use macroquad::prelude::{self as mq, Vec2, vec2};
use oxide_protocol::{Key, MouseButton, RawEvent};
use oxide_sim::{Command, UnitId};

/// Logical pixels of mouse travel under which a press+release counts as a
/// click (scaled with the user-facing UI at use).
const CLICK_SLOP: f32 = 6.0;

fn click_slop(ui: f32) -> f32 {
    CLICK_SLOP * ui
}

/// Shared with the drag-rectangle renderer, so what draws as a drag is
/// exactly what selects as one.
pub fn drag_threshold(ui: f32) -> f32 {
    click_slop(ui)
}

/// How much feedback a live selection drag should show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragFeedback {
    /// The pointer has not moved from the press.
    Still,
    /// The pointer moved, but release would still count as a click.
    Outline,
    /// Release would box-select, so preview the units inside too.
    Selection,
}

/// Classifies visual drag feedback without changing click-vs-drag semantics.
pub(crate) fn drag_feedback(origin: Vec2, at: Vec2, ui: f32) -> DragFeedback {
    if origin == at {
        DragFeedback::Still
    } else if origin.distance(at) <= drag_threshold(ui) {
        DragFeedback::Outline
    } else {
        DragFeedback::Selection
    }
}

/// World-unit pick radius around a unit's center.
const PICK_RADIUS: f32 = 0.6;

fn unit_pick_radius(kind: oxide_sim::UnitKind) -> f32 {
    PICK_RADIUS
        .max(crate::render::unit_visual_radius(kind))
        .max(kind.stats().radius.to_num::<f32>())
}

/// Camera pan speed in screen pixels per second (converted by zoom).
pub(crate) const PAN_PX_PER_SEC: f32 = 900.0;

/// The one persistent world-targeting mode currently armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArmedMode {
    /// Place this structure.
    Build(oxide_sim::BuildingKind),
    /// Set a producer's rally point.
    Rally,
    /// Pick the selected Spires' strike tile.
    Strike,
    /// Strip an own structure.
    Salvage,
    /// Weld a damaged own ground unit.
    Weld,
    /// Escort an own unit.
    Accompany,
    /// Move without engaging.
    Run,
    /// Fight and pursue along a route.
    Hunt,
    /// Scout around known enemy reach.
    Scout,
    /// Collect this many patrol waypoints.
    Patrol(usize),
}

impl ArmedMode {
    /// Compact persistent label naming the armed mode.
    pub(crate) fn label(self) -> String {
        match self {
            Self::Build(kind) => crate::typography::entity_name(kind.name()),
            Self::Rally => "Set rally".to_string(),
            Self::Strike => "Launch".to_string(),
            Self::Salvage => "Salvage".to_string(),
            Self::Weld => "Weld".to_string(),
            Self::Accompany => "Accompany".to_string(),
            Self::Run => "Run".to_string(),
            Self::Hunt => "Hunt".to_string(),
            Self::Scout => "Scout".to_string(),
            Self::Patrol(0) => "Patrol".to_string(),
            Self::Patrol(count) => format!("Patrol \u{b7} {count}"),
        }
    }
}

/// Where the placement preview sits: the touch ghost when one is down,
/// otherwise under the mouse, but only while the mouse is the pointer in
/// use (a stale mouse point on a touch device would draw a stray ghost).
pub(crate) fn placement_preview_anchor(
    game: &crate::game::Scene<'_>,
    input: &InputState,
) -> Option<(oxide_sim::BuildingKind, TilePos)> {
    let kind = input.placing_kind()?;
    if let Some(anchor) = input.ghost_anchor() {
        return Some((kind, anchor));
    }
    (input.last_pointer == Pointer::Mouse).then(|| {
        let world = game.presentation.camera.to_world(input.mouse);
        let hovered = numeric::tile_at(world);
        (kind, placement_anchor(game, kind, hovered))
    })
}

/// Which pointer produced a press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pointer {
    Mouse,
    Touch,
}

impl Pointer {
    /// The pointer an event shows in use, if it shows one. A wheel
    /// carries no position, so it leaves the pointer as it was: before
    /// any motion, hover and edge pan would read a stale mouse point.
    fn of(event: &RawEvent) -> Option<Self> {
        match event {
            RawEvent::MouseDown { .. } | RawEvent::MouseMove { .. } => Some(Self::Mouse),
            RawEvent::TouchDown { .. } => Some(Self::Touch),
            _ => None,
        }
    }
}

/// Notes the hands behind one event, whatever the screen: the pointer in
/// use and whether a hardware key has been pressed. A key pressed while
/// a text field has focus doesn't count, since iOS's on-screen keyboard
/// sends Enter, Space, and Backspace as key events.
pub fn track_hands(input: &mut InputState, event: &RawEvent, text_entry: bool) {
    if let Some(pointer) = Pointer::of(event) {
        input.last_pointer = pointer;
    }
    if !text_entry && matches!(event, RawEvent::KeyDown { .. }) {
        input.keys_seen = true;
    }
}

/// Where a touch placement's ghost sits until a tap on it confirms.
/// Touch has no hover, so the ghost stands in for the mouse's preview:
/// dropped where the finger lands, dragged into place, and built only
/// where it is drawn.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacementGhost {
    pub(crate) anchor: TilePos,
    /// While a finger drags it: the anchor and the world point at grab.
    grab: Option<(TilePos, Vec2)>,
    /// The straight row its handle has pulled out.
    row: Option<GhostRow>,
}

/// A touch row: the step between its cells, how many cells from the ghost
/// on it covers, and its plan, which lasts until the check mark builds it
/// or the cross folds it back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GhostRow {
    pub(crate) step: (i32, i32),
    count: usize,
    plan: PlacingStroke,
    judged: RowJudged,
}

/// What a touch row's plan was judged against. A change in the staged
/// batch, the selection, or QUEUE judges it again at once; the world's own
/// drift waits for [`ROW_RECHECK_TICKS`].
#[derive(Debug, Clone, PartialEq)]
struct RowJudged {
    tick: oxide_sim::Tick,
    pending: usize,
    queue: bool,
    units: Vec<UnitId>,
}

impl RowJudged {
    fn now(game: &crate::game::Scene<'_>, queue: bool) -> Self {
        Self {
            tick: game.state.current_tick(),
            pending: game.pending.len(),
            queue,
            units: game.presentation.selection.units.clone(),
        }
    }

    /// Whether a plan judged against `self` must be judged again by `now`;
    /// `strict` also counts any tick at all.
    fn stale(&self, now: &Self, strict: bool) -> bool {
        let wait = if strict { 1 } else { ROW_RECHECK_TICKS };
        self.pending != now.pending
            || self.queue != now.queue
            || self.units != now.units
            || now.tick >= self.tick + wait
    }
}

/// How long a held row's plan stands before the world's drift judges it
/// again: judging a row replays every stamp before each one, so a held
/// row is rechecked twice a second rather than every tick. The check mark
/// rechecks it whole before building.
const ROW_RECHECK_TICKS: oxide_sim::Tick = (oxide_sim::TICKS_PER_SECOND / 2) as oxide_sim::Tick;

impl PlacementGhost {
    fn at(anchor: TilePos) -> Self {
        Self {
            anchor,
            grab: None,
            row: None,
        }
    }
}

/// An armed strike: the loaded Spires it launches and, on touch, the aim
/// that stands in for the mouse's hover ring.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Strike {
    pub(crate) spires: Vec<oxide_sim::BuildingId>,
    aim: Option<StrikeAim>,
}

/// Where a touch strike will land until its check mark launches it. Like
/// the placement ghost, a tap drops or moves it, a finger drags it, and
/// only the check mark fires: a warhead is too costly to spend on a stray
/// tap.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StrikeAim {
    tile: TilePos,
    /// While a finger drags it: the tile and the world point at grab.
    grab: Option<(TilePos, Vec2)>,
}

impl Strike {
    pub(crate) fn new(spires: Vec<oxide_sim::BuildingId>) -> Self {
        Self { spires, aim: None }
    }

    /// The tile the touch aim names, once one is down.
    pub(crate) fn aim(&self) -> Option<TilePos> {
        self.aim.map(|aim| aim.tile)
    }

    fn aim_at(&mut self, tile: TilePos) {
        self.aim = Some(StrikeAim { tile, grab: None });
    }
}

/// A command verb armed for the next world click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClickVerb {
    /// The next left-click on an own built building sends the selected
    /// harvesters to strip it.
    Salvage,
    /// The next left-click on a damaged own ground unit sends the
    /// selected harvesters to weld it.
    Weld,
    /// The next left-click on an own unit sends the rest of the selection
    /// to escort it.
    Accompany,
    /// The next ground click sends the selection walking obliviously —
    /// no engaging, no auto-acquire en route.
    Run,
    /// The next ground click sends the selection on a fighting march
    /// that chases enemies along its route.
    Hunt,
    /// The next ground click sends the selected scouts to see it from
    /// safe ground.
    Scout,
}

/// Cross-frame input state (cursor, held keys, drag origin).
#[expect(
    clippy::struct_excessive_bools,
    reason = "a minimap drag, the palette, the queue toggle and a menu request are independent"
)]
pub struct InputState {
    /// Last known cursor position, window pixels.
    pub mouse: Vec2,
    /// Where the current left-drag started, if any.
    pub drag_origin: Option<Vec2>,
    /// Whether a left-drag is steering the camera via the minimap —
    /// clicks there never start a selection box, they drive the view.
    pub(crate) minimap_drag: bool,
    /// Middle-drag pan anchor: the world follows the hand.
    pub(crate) mmb_anchor: Option<Vec2>,
    /// Control groups 1..=5 (assigned with Ctrl+N, recalled with N).
    groups: [Vec<UnitId>; crate::action::CONTROL_GROUPS],
    /// Previous click, for double-click detection.
    last_click: Option<(f64, Vec2)>,
    /// Previous group recall, for double-tap camera centering.
    last_recall: Option<(usize, f64)>,
    /// Camera bookmarks (Ctrl+F5..F8 set, F5..F8 recall). Session
    /// state: a new match's coordinates mean different ground.
    pub(crate) bookmarks: [Option<Vec2>; 4],
    /// Waypoints collected while arming a patrol (`R`), if any.
    pub(crate) patrol_route: Option<Vec<TilePos>>,
    /// The armed building placement, if any.
    pub(crate) placing: Option<Placement>,
    /// The pointer used last: the mouse's hover only means something
    /// while the mouse is the one in use.
    pub(crate) last_pointer: Pointer,
    /// Whether a hardware key has been pressed, so copy may name keys.
    pub(crate) keys_seen: bool,
    /// The verb the next world click issues, if one is armed.
    pub(crate) click_verb: Option<ClickVerb>,
    /// Producers whose rally point the next world/minimap click sets.
    pub(crate) rallying: Vec<oxide_sim::BuildingId>,
    /// The armed strike, while Spires wait for their target.
    pub(crate) striking: Option<Strike>,
    /// Whether the build palette is open (`B`; digits pick a structure).
    pub(crate) build_menu: bool,
    pub(crate) build_category: Option<u8>,
    /// This frame's chrome scale (dpi x user), injected by the frame
    /// loop so hit math never queries the window.
    pub(crate) ui: f32,
    /// This frame's wall clock, injected likewise (double-click and
    /// double-tap timing).
    pub(crate) now: f64,
    /// Camera feel from settings, injected per frame.
    pub(crate) camera_prefs: crate::config::CameraPrefs,
    /// Touch timing windows, injected from config each frame.
    pub(crate) touch_prefs: crate::config::TouchPrefs,
    /// Live fingers, in touch-down order (deterministic; two at most
    /// matter). Each carries its origin, latest point, down time, and
    /// whether it moved past the slop or already fired a long-press.
    pub(crate) touches: Vec<(u64, TouchPoint)>,
    /// Wall-clock stamp of the last completed tap, for double-taps.
    pub(crate) last_tap: Option<(f64, macroquad::prelude::Vec2)>,
    /// The QUEUE toggle: touch's stand-in for a held Shift.
    pub(crate) queue_toggle: QueueToggle,
    /// Whether the mouse is dragging the touch ghost's row handle.
    pub(crate) handle_drag: bool,
    /// The live two-finger gesture, if two fingers are down.
    pub(crate) pair: Option<Pair>,
    /// The menu button was pressed this frame. Input cannot switch
    /// screens itself, so the frame loop takes this one-shot request.
    pub(crate) menu_requested: bool,
    /// The active binding profile.
    /// Chord state: modifier truth and held actions.
    pub(crate) resolver: ActionResolver,
}

/// An armed building placement and everything that lasts only while it is
/// armed, so no stroke or ghost outlives its mode.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placement {
    pub(crate) kind: oxide_sim::BuildingKind,
    /// The live drag-to-place stroke, while the button stays down.
    stroke: Option<PlacingStroke>,
    /// The touch placement's ghost, once a tap has dropped one.
    ghost: Option<PlacementGhost>,
    /// The last site this placement staged, once one has. Letting go of
    /// queue then ends it: the next plain build would replace the chain just
    /// queued.
    chain: Option<Chain>,
}

/// Where a placement chain stands: the last site staged and the step a
/// queued touch chain offers its next ghost at.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Chain {
    last: TilePos,
    step: (i32, i32),
}

/// The QUEUE toggle, which works like the iPad's Shift key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum QueueToggle {
    Off,
    /// On for the action at hand, since the wall-clock `at`.
    Latched {
        at: f64,
    },
    /// On until tapped off.
    Locked,
}

/// What a latched QUEUE's action could have changed during an input step.
pub(crate) struct LatchProbe {
    armed: bool,
    staged: usize,
    units: Vec<UnitId>,
    buildings: Vec<oxide_sim::BuildingId>,
}

/// A live drag-to-place stroke: the row it plans, staged only when the
/// button comes up. Each stamp was judged after the staged batch and every
/// stamp before it, so the plan never outruns the bank or the queue.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PlacingStroke {
    /// Whether the opening stamp queues behind the builder's program; the
    /// rest always do.
    queue: bool,
    /// The planned stamps in order. Whether each defers its claim is
    /// derived from the player's sight whenever it is needed, never kept,
    /// so a stamp whose ground falls into fog mid-drag is judged as fog.
    stamps: Vec<TilePos>,
}

impl PlacingStroke {
    /// Whether stamp `index` queues.
    fn queues(&self, index: usize) -> bool {
        index > 0 || self.queue
    }

    /// Whether `anchor` would overlap a planned stamp of `kind`.
    fn overlaps(&self, kind: oxide_sim::BuildingKind, anchor: TilePos) -> bool {
        let (w, h) = kind.size();
        self.stamps
            .iter()
            .any(|a| (a.x - anchor.x).abs() < w && (a.y - anchor.y).abs() < h)
    }
}

/// Whether a build at `anchor` must defer its claim: some footprint
/// tile is not currently visible to the human. Decides both the amber
/// ghost and the `defer` flag the armed click emits.
pub(crate) fn build_defer_needed(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    anchor: TilePos,
) -> bool {
    let (w, h) = kind.size();
    (0..h).any(|dy| {
        (0..w).any(|dx| {
            !game
                .state
                .vision(game.presentation.human)
                .visible(anchor.offset(dx, dy))
        })
    })
}

/// Unit programs a non-queued build will replace. Queued builds preserve
/// every claim because they append behind the current program.
fn replaced_build_units<'a>(game: &'a crate::game::Scene<'_>, queue: bool) -> &'a [UnitId] {
    if queue {
        &[]
    } else {
        &game.presentation.selection.units
    }
}

pub(crate) fn available_construction_scrap(
    game: &crate::game::Scene<'_>,
    input: &InputState,
) -> u32 {
    let replaced = replaced_build_units(game, input.queue_held());
    if game.pending.is_empty() {
        return game
            .state
            .player(game.presentation.human)
            .scrap
            .saturating_add(
                game.state
                    .construction_refund(game.presentation.human, replaced),
            );
    }
    game.state.inspect_command_phase(game.pending, |view| {
        view.scrap(game.presentation.human)
            .unwrap_or(0)
            .saturating_add(view.construction_refund(game.presentation.human, replaced))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PendingBuildFunds {
    scrap: u32,
    refund: u32,
}

impl PendingBuildFunds {
    fn available(self) -> u32 {
        self.scrap.saturating_add(self.refund)
    }
}

struct PendingBuildProjection {
    refusal: Option<oxide_sim::PlaceRefusal>,
    funds: PendingBuildFunds,
    queue_has_room: bool,
    /// The human's own blocking buildings once the batch lands, staged and
    /// planned sites included: footprints the player always knows.
    own_footprints: Vec<(TilePos, (i32, i32))>,
}

/// Projects the exact command phase once for every placement surface:
/// ground, bank, and the founder's program all read the same accepted batch,
/// the staged commands perhaps followed by a stroke's planned stamps. Every
/// accepted site is already paid. Replacement can spend the full refund from
/// unstarted sites whose last worker commitment it removes.
fn pending_build_projection_for(
    game: &crate::game::Scene<'_>,
    batch: &[oxide_sim::PlayerCommand],
    kind: oxide_sim::BuildingKind,
    anchor: TilePos,
    queue: bool,
    defer: bool,
) -> PendingBuildProjection {
    let replaced = replaced_build_units(game, queue);
    game.state.inspect_command_phase(batch, |state| {
        let refusal = if state.accepts_commands(game.presentation.human) {
            state.place_intent_refusal_replacing(game.presentation.human, kind, anchor, replaced)
        } else {
            Some(oxide_sim::PlaceRefusal::NotConstructible)
        };
        let refund = state.construction_refund(game.presentation.human, replaced);
        let crew: Vec<_> = state
            .units()
            .iter()
            .filter(|unit| {
                unit.player == game.presentation.human
                    && unit.kind.stats().harvest.is_some()
                    && game.presentation.selection.units.contains(&unit.id)
            })
            .collect();
        let can_assign = |unit: &&oxide_sim::Unit| {
            !queue
                || matches!(unit.order, oxide_sim::Order::Idle)
                || unit.queue.len() < oxide_sim::stats::ORDER_QUEUE_CAP
        };
        let queue_has_room = if defer {
            crew.iter().any(can_assign)
        } else {
            crew.first().is_some_and(can_assign)
        };
        let own_footprints = state
            .buildings()
            .iter()
            .filter(|b| b.player == game.presentation.human && !b.provisional())
            .map(|b| (b.anchor, b.kind.size()))
            .collect();
        PendingBuildProjection {
            refusal,
            own_footprints,
            funds: PendingBuildFunds {
                scrap: state
                    .scrap(game.presentation.human)
                    .expect("the live human seat remains in the projection"),
                refund,
            },
            queue_has_room,
        }
    })
}

/// What placing at an anchor would do, judged after the staged batch: the
/// ghost tints from it, the mode ribbon says it, and a click acts on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlacementVerdict {
    /// The crew founds the site now, on visible ground.
    Builds,
    /// Part of the footprint is remembered ground: the crew walks out and
    /// founds on arrival.
    BuildsOnArrival,
    /// The ground or the rules refuse the site.
    Refused(oxide_sim::PlaceRefusal),
    /// The bank is this much scrap short of the site's cost.
    Short(u32),
    /// No selected builder has room for the order.
    QueueFull,
    /// The ground the player has seen leaves the founder no way to the
    /// site's doorstep.
    Unreachable,
}

impl PlacementVerdict {
    /// Whether a click here stages the build.
    pub(crate) fn builds(self) -> bool {
        matches!(self, Self::Builds | Self::BuildsOnArrival)
    }
}

/// The candidate a cached verdict was judged for. Nothing else it reads
/// can change without one of these changing: the world moves only with
/// the tick, and the staged batch only grows until a tick drains it.
#[derive(Debug, PartialEq, Eq)]
struct VerdictKey {
    tick: oxide_sim::Tick,
    pending: usize,
    units: Vec<UnitId>,
    kind: oxide_sim::BuildingKind,
    anchor: TilePos,
    queue: bool,
    /// The stroke judged behind: whether it opens queued, and its stamps.
    plan: Option<(bool, Vec<TilePos>)>,
}

/// The latest placement verdict. The ghost, every HUD layout pass, and the
/// click ask several times a frame, and a staged batch makes each answer
/// cost a command-phase projection.
#[derive(Debug, Default)]
pub(crate) struct VerdictCache(std::cell::RefCell<Option<(VerdictKey, PlacementVerdict)>>);

impl VerdictCache {
    pub(crate) fn clear(&self) {
        self.0.take();
    }
}

/// What placing `kind` at `anchor` would do for the selected builders.
pub(crate) fn placement_verdict(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    anchor: TilePos,
    queue: bool,
) -> PlacementVerdict {
    judge(game, kind, anchor, queue, None)
}

/// What one more stamp at `anchor` would do behind `stroke`'s plan. It
/// always queues.
fn stroke_verdict(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    stroke: &PlacingStroke,
    anchor: TilePos,
) -> PlacementVerdict {
    judge(game, kind, anchor, true, Some(stroke))
}

/// The staged batch followed by `stroke`'s stamps, as the sim would take
/// them if the button came up now.
fn planned_batch(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    stroke: &PlacingStroke,
) -> Vec<oxide_sim::PlayerCommand> {
    let units = &game.presentation.selection.units;
    let stamps =
        stroke
            .stamps
            .iter()
            .enumerate()
            .map(|(index, &anchor)| oxide_sim::PlayerCommand {
                player: game.presentation.human,
                command: Command::Build {
                    units: units.clone(),
                    kind,
                    anchor,
                    queue: stroke.queues(index),
                    defer: build_defer_needed(game, kind, anchor),
                },
            });
    game.pending.iter().cloned().chain(stamps).collect()
}

fn judge(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    anchor: TilePos,
    queue: bool,
    stroke: Option<&PlacingStroke>,
) -> PlacementVerdict {
    let key = VerdictKey {
        tick: game.state.current_tick(),
        pending: game.pending.len(),
        units: game.presentation.selection.units.clone(),
        kind,
        anchor,
        queue,
        plan: stroke.map(|stroke| (stroke.queue, stroke.stamps.clone())),
    };
    let cache = &game.presentation.placement_verdict.0;
    if let Some((judged, verdict)) = cache.borrow().as_ref()
        && *judged == key
    {
        return *verdict;
    }
    let defer = build_defer_needed(game, kind, anchor);
    let projection = match stroke {
        Some(stroke) => {
            let batch = planned_batch(game, kind, stroke);
            pending_build_projection_for(game, &batch, kind, anchor, queue, defer)
        }
        None => pending_build_projection_for(game, game.pending, kind, anchor, queue, defer),
    };
    let cost = kind.base_stats().construction.map_or(0, |c| c.cost);
    let available = projection.funds.available();
    // The order a click checks them in: the ground, the bank, the queue.
    let verdict = if let Some(refusal) = projection.refusal {
        PlacementVerdict::Refused(refusal)
    } else if available < cost {
        PlacementVerdict::Short(cost - available)
    } else if !projection.queue_has_room {
        PlacementVerdict::QueueFull
    } else if defer {
        PlacementVerdict::BuildsOnArrival
    } else if !founder_may_reach(game, kind, anchor, &projection.own_footprints) {
        PlacementVerdict::Unreachable
    } else {
        PlacementVerdict::Builds
    };
    cache.replace(Some((key, verdict)));
    verdict
}

/// Whether the founder may reach a doorstep of `kind` at `anchor`, as far
/// as the player can tell. Only ground the player can see blocked, has
/// explored as impassable terrain, or holds under `own` footprints (the
/// player's buildings once the staged and planned batch lands) counts:
/// unexplored and fogged ground is taken as open, so this never calls
/// reachable ground unreachable and never reads what fog hides. The sim's
/// own path check still decides; this catches the walled-off pockets,
/// plateaus, and rows that seal their own doorsteps the player can see.
/// Moves run eight ways without cutting corners, as the sim's search does.
fn founder_may_reach(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    anchor: TilePos,
    own: &[(TilePos, (i32, i32))],
) -> bool {
    let human = game.presentation.human;
    let Some(founder) = game
        .presentation
        .selection
        .units
        .iter()
        .filter_map(|id| game.state.unit(*id))
        .find(|unit| unit.player == human && unit.kind.stats().harvest.is_some())
    else {
        return true;
    };
    let (w, h) = kind.size();
    let inside =
        |t: TilePos| t.x >= anchor.x && t.x < anchor.x + w && t.y >= anchor.y && t.y < anchor.y + h;
    let doorstep = |t: TilePos| {
        !inside(t)
            && t.x >= anchor.x - 1
            && t.x <= anchor.x + w
            && t.y >= anchor.y - 1
            && t.y <= anchor.y + h
    };
    let from = founder.tile();
    if inside(from) {
        return true;
    }
    let map = game.state.map();
    let vision = game.state.vision(human);
    let (width, height) = (map.width(), map.height());
    let index = |t: TilePos| (t.y * width + t.x).fit::<usize>();
    let mut own_blocks = vec![false; (width * height).fit::<usize>()];
    for &(corner, (w, h)) in own {
        for dy in 0..h {
            for dx in 0..w {
                let t = corner.offset(dx, dy);
                if map.tile(t).is_some() {
                    own_blocks[index(t)] = true;
                }
            }
        }
    }
    // The founder may walk off its own tile even if a site now covers it,
    // as the sim's search may, but no covered tile is ever a doorstep.
    let open = |t: TilePos| {
        if inside(t) {
            return false;
        }
        let Some(tile) = map.tile(t) else {
            return false;
        };
        if own_blocks[index(t)] {
            false
        } else if vision.visible(t) {
            game.state.passable(t)
        } else if vision.explored(t) {
            !tile.terrain.blocks_ground()
        } else {
            true
        }
    };
    let mut seen = vec![false; (width * height).fit::<usize>()];
    let mut frontier = std::collections::VecDeque::from([from]);
    if map.tile(from).is_some() {
        seen[index(from)] = true;
    }
    while let Some(at) = frontier.pop_front() {
        if doorstep(at) && open(at) {
            return true;
        }
        for (dx, dy) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let next = at.offset(dx, dy);
            // As `chassis::path::astar` moves: a diagonal step never cuts
            // a corner, so both cardinal companions must be open.
            let corner_clear =
                dx == 0 || dy == 0 || (open(at.offset(dx, 0)) && open(at.offset(0, dy)));
            if map.tile(next).is_some() && !seen[index(next)] && corner_clear && open(next) {
                seen[index(next)] = true;
                frontier.push_back(next);
            }
        }
    }
    false
}

/// The number a queued build takes in the dock's subject's program, if
/// the subject will take it: a subject with a full queue is skipped while
/// another selected builder may still accept the order.
pub(crate) fn queued_position(game: &crate::game::Scene<'_>) -> Option<usize> {
    placement_backlog(game)
        .filter(|backlog| *backlog <= oxide_sim::stats::ORDER_QUEUE_CAP)
        .map(|backlog| backlog + 1)
}

/// Whether the mouse is the pointer placing and it rests on chrome or the
/// minimap, where a click does something other than place.
pub(crate) fn cursor_on_chrome(game: &crate::game::Scene<'_>, input: &InputState) -> bool {
    input.ghost_anchor().is_none()
        && input.last_pointer == Pointer::Mouse
        && (game.presentation.layout.get().chrome_owns(input.mouse)
            || crate::render::minimap_world_at(game, input.mouse).is_some())
}

/// How many orders the placement's builder already has, counted as its
/// dock chips are. The builder is the dock's subject when that can build,
/// else the lowest-id selected own worker: every selected worker takes the
/// order, and this one stands for them as the dock does.
pub(crate) fn placement_backlog(game: &crate::game::Scene<'_>) -> Option<usize> {
    let builds = |id: &UnitId| {
        game.state.unit(*id).is_some_and(|unit| {
            unit.player == game.presentation.human && unit.kind.stats().harvest.is_some()
        })
    };
    let builder = crate::panel::subject_unit(game)
        .filter(builds)
        .or_else(|| {
            game.presentation
                .selection
                .units
                .iter()
                .copied()
                .find(builds)
        })?;
    let projection = game.projection();
    let program = projection.program(builder)?;
    Some(
        program
            .orders
            .iter()
            .filter(|order| match order {
                oxide_sim::Order::Idle => false,
                // A queued order replaces a trip from an idle post rather
                // than waiting behind it.
                oxide_sim::Order::Tend { .. } => program.looping,
                _ => true,
            })
            .count(),
    )
}

/// Where a staged build lands in its builder's program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lands {
    /// Queued at this 1-based program position, its dock chip's number.
    Queues(usize),
    /// Replacing this many orders.
    Replaces(usize),
}

/// The placement preview's spot and what a click there, or a release of
/// the stroke reaching it, would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PreviewSpot {
    pub(crate) kind: oxide_sim::BuildingKind,
    pub(crate) anchor: TilePos,
    pub(crate) verdict: PlacementVerdict,
    /// Where it lands, when it builds and its builder is known.
    pub(crate) lands: Option<Lands>,
}

/// The spot the placement preview judges: under the cursor or at the
/// touch ghost, behind a live stroke's plan when one is down.
pub(crate) fn preview_spot(
    game: &crate::game::Scene<'_>,
    input: &InputState,
) -> Option<PreviewSpot> {
    use PlacementVerdict::{Builds, BuildsOnArrival, Refused};
    let (kind, anchor) = placement_preview_anchor(game, input)?;
    let backlog = placement_backlog(game);
    let replaces = || backlog.filter(|orders| *orders > 0).map(Lands::Replaces);
    let (verdict, lands) = if let Some(stroke) = input.active_plan() {
        // A stroke that opened in place of the program starts it afresh. A
        // position past a full queue would skip the dock's subject.
        let base = if stroke.queue { backlog } else { Some(0) };
        let queued = |index: usize| {
            base.map(|base| base + index + 1)
                .filter(|position| *position <= oxide_sim::stats::ORDER_QUEUE_CAP + 1)
                .map(Lands::Queues)
        };
        if let Some(index) = stroke.stamps.iter().position(|a| *a == anchor) {
            let verdict = if build_defer_needed(game, kind, anchor) {
                BuildsOnArrival
            } else {
                Builds
            };
            let lands = if stroke.queues(index) {
                queued(index)
            } else {
                replaces()
            };
            (verdict, lands)
        } else if stroke.overlaps(kind, anchor) {
            (Refused(oxide_sim::PlaceRefusal::Building), None)
        } else {
            (
                stroke_verdict(game, kind, stroke, anchor),
                queued(stroke.stamps.len()),
            )
        }
    } else {
        let queue = input.queue_held();
        let lands = if queue {
            queued_position(game).map(Lands::Queues)
        } else {
            replaces()
        };
        (placement_verdict(game, kind, anchor, queue), lands)
    };
    Some(PreviewSpot {
        kind,
        anchor,
        verdict,
        lands: lands.filter(|_| verdict.builds()),
    })
}

/// Why the ground or the rules refuse a site, as a lowercase fragment.
pub(crate) fn refusal_reason(refusal: oxide_sim::PlaceRefusal) -> &'static str {
    use oxide_sim::PlaceRefusal;
    match refusal {
        PlaceRefusal::Fog => "you haven't scouted that ground",
        PlaceRefusal::Terrain => "impassable ground",
        PlaceRefusal::Building => "something already stands there",
        PlaceRefusal::Unit => "an enemy machine is holding that ground",
        PlaceRefusal::NotConstructible => "that can't be built",
        PlaceRefusal::Prerequisite => "needs its tech building",
        PlaceRefusal::FrameRequired => "an extractor rebuilds only on a derelict frame",
        PlaceRefusal::FrameBlocked => "that ground belongs to a derelict frame",
    }
}

/// The toast a refused placement click raises.
fn refusal_toast(refusal: oxide_sim::PlaceRefusal) -> String {
    use oxide_sim::PlaceRefusal;
    let reason = refusal_reason(refusal);
    match refusal {
        PlaceRefusal::Fog
        | PlaceRefusal::Terrain
        | PlaceRefusal::Building
        | PlaceRefusal::Unit
        | PlaceRefusal::FrameBlocked => format!("Can't build there: {reason}"),
        PlaceRefusal::Prerequisite => format!("Can't build that yet: {reason}"),
        PlaceRefusal::NotConstructible | PlaceRefusal::FrameRequired => reason.to_string(),
    }
}

/// The fog-honest anchor shared by the placement ghost and every input path.
/// A discovered Extractor frame is one 2x2 target, not four unrelated tiles.
pub(crate) fn placement_anchor(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    clicked: TilePos,
) -> TilePos {
    game.state
        .canonical_build_anchor(game.presentation.human, kind, clicked)
}

fn placement_ping(kind: oxide_sim::BuildingKind, anchor: TilePos) -> Vec2 {
    crate::game::world_vec(oxide_sim::geometry::footprint_center(anchor, kind.size()))
}

/// The map tile a ground order names. The camera's edge slack lets the
/// cursor rest past the map, so the tile clamps back onto it.
pub(crate) fn ground_tile(state: &oxide_sim::State, world: Vec2) -> TilePos {
    let map = state.map();
    TilePos::new(
        numeric::to_i32(world.x.floor()).clamp(0, map.width() - 1),
        numeric::to_i32(world.y.floor()).clamp(0, map.height() - 1),
    )
}

/// Where a ground order's acknowledgment draws: the centre of the
/// ordered tile, which is also where its waypoint marker sits.
pub(crate) fn tile_center(tile: TilePos) -> Vec2 {
    vec2(tile.x as f32 + 0.5, tile.y as f32 + 0.5)
}

impl InputState {
    pub(crate) fn construction_open(&self) -> bool {
        self.build_menu || self.placing.is_some()
    }

    /// Fresh input state.
    pub fn new() -> Self {
        Self {
            mouse: vec2(0.0, 0.0),
            drag_origin: None,
            minimap_drag: false,
            mmb_anchor: None,
            groups: Default::default(),
            last_click: None,
            last_recall: None,
            patrol_route: None,
            placing: None,
            last_pointer: crate::platform::Hands::BUILD.pointer,
            keys_seen: crate::platform::Hands::BUILD.keys,
            click_verb: None,
            rallying: Vec::new(),
            striking: None,
            build_menu: false,
            build_category: None,
            ui: 1.0,
            now: 0.0,
            camera_prefs: crate::config::CameraPrefs::default(),
            touch_prefs: crate::config::TouchPrefs::default(),
            touches: Vec::new(),
            last_tap: None,
            queue_toggle: QueueToggle::Off,
            handle_drag: false,
            pair: None,
            menu_requested: false,
            bookmarks: [None; 4],
            resolver: ActionResolver::default(),
        }
    }

    /// Feeds a key edge through the binding map.
    fn key_edge(
        &mut self,
        bindings: &BindingMap,
        key: Key,
        down: bool,
        context: crate::action::Context,
    ) -> Option<ActionEvent> {
        self.resolver.key_edge_in(bindings, key, down, context)
    }

    pub(crate) fn context(&self, game: &Game) -> crate::action::Context {
        use crate::action::Context;
        if self.construction_open()
            && game.presentation.selection.units.iter().any(|id| {
                game.state.unit(*id).is_some_and(|u| {
                    u.player == game.presentation.human && u.kind.stats().harvest.is_some()
                })
            })
        {
            return self
                .build_category
                .map_or(Context::Construction, Context::BuildCategory);
        }
        if game.presentation.selection.units.iter().any(|id| {
            game.state
                .unit(*id)
                .is_some_and(|unit| unit.kind.stats().harvest.is_some())
        }) {
            Context::Workers
        } else if !game.presentation.selection.units.is_empty() {
            Context::Units
        } else if !orders::selected_producers(game).is_empty() {
            Context::Production
        } else if !game.presentation.selection.buildings.is_empty() {
            Context::Buildings
        } else {
            Context::Empty
        }
    }

    pub(crate) fn close_construction(&mut self) {
        self.build_menu = false;
        self.build_category = None;
        self.disarm_click_verbs();
        self.patrol_route = None;
    }

    /// Disarms every left-click verb, so arming placement, salvage, repair,
    /// run, hunt, rally, or patrol stands the others down. `armed_click`
    /// resolves modes in a fixed priority order, so two live at once would
    /// make the next click do something other than what the toast promised.
    pub(crate) fn disarm_click_verbs(&mut self) {
        self.stop_placing();
        self.patrol_route = None;
        self.click_verb = None;
        self.rallying.clear();
        self.striking = None;
    }

    /// Arms placement of `kind`, with no stroke or ghost yet.
    pub(crate) fn arm_placement(&mut self, kind: oxide_sim::BuildingKind) {
        self.placing = Some(Placement {
            kind,
            stroke: None,
            ghost: None,
            chain: None,
        });
    }

    /// Drops placement: the armed kind, its stroke, and its ghost.
    pub(crate) fn stop_placing(&mut self) {
        self.placing = None;
    }

    /// The building kind armed for placement, if any.
    pub(crate) fn placing_kind(&self) -> Option<oxide_sim::BuildingKind> {
        self.placing.as_ref().map(|placement| placement.kind)
    }

    /// The plan placement is previewing: a live mouse stroke, else the
    /// touch ghost's row.
    fn active_plan(&self) -> Option<&PlacingStroke> {
        let placement = self.placing.as_ref()?;
        placement
            .stroke
            .as_ref()
            .or_else(|| placement.ghost.as_ref()?.row.as_ref().map(|row| &row.plan))
    }

    /// The previewed plan's stamps.
    pub(crate) fn planned_stamps(&self) -> &[TilePos] {
        self.active_plan()
            .map_or(&[], |plan| plan.stamps.as_slice())
    }

    /// Throws a live stroke's plan, or a touch ghost's row, away, keeping
    /// placement armed unless queue was let go during the stroke after a
    /// chain had staged. Returns whether there was one.
    pub(crate) fn drop_stroke(&mut self) -> bool {
        let dropped = self.placing.as_mut().is_some_and(|placement| {
            placement.stroke.take().is_some()
                || placement
                    .ghost
                    .as_mut()
                    .is_some_and(|ghost| ghost.row.take().is_some())
        });
        if dropped && !self.queue_held() {
            self.queue_released();
        }
        dropped
    }

    /// The touch ghost's anchor, while placement is armed.
    pub(crate) fn ghost_anchor(&self) -> Option<TilePos> {
        self.placing
            .as_ref()?
            .ghost
            .as_ref()
            .map(|ghost| ghost.anchor)
    }

    /// Queue was let go (Shift up, or QUEUE off). A placement that already
    /// staged a build ends, as releasing Shift ends a chain in other RTS
    /// games: the next plain build would replace everything just queued. A
    /// live stroke is left to its release, which decides the same way.
    fn queue_released(&mut self) {
        if self
            .placing
            .as_ref()
            .is_some_and(|placement| placement.chain.is_some() && placement.stroke.is_none())
        {
            self.stop_placing();
        }
    }

    /// Whether `verb` is the armed click verb.
    pub(crate) fn armed(&self, verb: ClickVerb) -> bool {
        self.click_verb == Some(verb)
    }

    /// Current persistent mode, in the same priority order the world
    /// click resolver uses.
    pub(crate) fn armed_mode(&self) -> Option<ArmedMode> {
        self.striking
            .is_some()
            .then_some(ArmedMode::Strike)
            .or_else(|| (!self.rallying.is_empty()).then_some(ArmedMode::Rally))
            .or_else(|| self.placing_kind().map(ArmedMode::Build))
            .or_else(|| {
                self.click_verb.map(|verb| match verb {
                    ClickVerb::Salvage => ArmedMode::Salvage,
                    ClickVerb::Weld => ArmedMode::Weld,
                    ClickVerb::Accompany => ArmedMode::Accompany,
                    ClickVerb::Run => ArmedMode::Run,
                    ClickVerb::Hunt => ArmedMode::Hunt,
                    ClickVerb::Scout => ArmedMode::Scout,
                })
            })
            .or_else(|| {
                self.patrol_route
                    .as_ref()
                    .map(|route| ArmedMode::Patrol(route.len()))
            })
    }

    /// Cancels whichever persistent targeting mode is armed.
    pub(crate) fn cancel_armed_mode(&mut self) -> bool {
        let armed = self.armed_mode().is_some();
        self.disarm_click_verbs();
        self.patrol_route = None;
        armed
    }

    /// Drops everything that assumes continuity (held keys, open drags,
    /// armed modes, live touches), keeping only the cursor position. Called
    /// on every mode transition: a menu eats the matching release events,
    /// and stale held state would otherwise pan the camera forever or fire
    /// a phantom box-select after resuming.
    pub fn reset_transient(&mut self) {
        self.resolver.clear();
        self.drag_origin = None;
        self.minimap_drag = false;
        self.mmb_anchor = None;
        self.patrol_route = None;
        self.stop_placing();
        self.click_verb = None;
        self.rallying.clear();
        self.striking = None;
        self.build_menu = false;
        self.build_category = None;
        self.touches.clear();
        self.last_tap = None;
        self.queue_toggle = QueueToggle::Off;
        self.handle_drag = false;
        self.pair = None;
        self.menu_requested = false;
    }

    /// Where a lone finger has rested on chrome long enough to preview
    /// the card under it.
    pub(crate) fn touch_preview(&self) -> Option<Vec2> {
        let [(_, finger)] = self.touches.as_slice() else {
            return None;
        };
        let rested = (self.now - finger.down_at) * 1000.0 >= TOUCH_REST_MS;
        (finger.born == TouchBorn::Chrome && !finger.moved && !finger.spent && rested)
            .then_some(finger.at)
    }

    /// Whether a press should queue its order, add to the selection, or
    /// keep an armed mode for another target: a held Shift on a
    /// keyboard, the sticky QUEUE toggle on touch.
    pub(crate) fn queue_held(&self) -> bool {
        self.queue_toggle != QueueToggle::Off || self.resolver.shift_held()
    }

    /// Each control group's live own-unit count. Dead ids linger in a
    /// group until its next recall, so the strip counts for itself.
    pub(crate) fn group_counts(
        &self,
        game: &crate::game::Scene<'_>,
    ) -> [usize; crate::action::CONTROL_GROUPS] {
        std::array::from_fn(|slot| {
            self.groups[slot]
                .iter()
                .filter(|id| {
                    game.state
                        .unit(**id)
                        .is_some_and(|u| u.player == game.presentation.human)
                })
                .count()
        })
    }

    /// The group the selection already is, exactly.
    pub(crate) fn selected_group(&self, game: &crate::game::Scene<'_>) -> Option<u8> {
        let human = game.presentation.human;
        let mut own: Vec<UnitId> = game.presentation.selection.units.clone();
        own.sort_unstable();
        if own.is_empty()
            || own
                .iter()
                .any(|id| game.state.unit(*id).is_none_or(|u| u.player != human))
        {
            return None;
        }
        (0..self.groups.len())
            .find(|slot| {
                let mut ids: Vec<UnitId> = self.groups[*slot]
                    .iter()
                    .copied()
                    .filter(|id| game.state.unit(*id).is_some_and(|u| u.player == human))
                    .collect();
                ids.sort_unstable();
                ids == own
            })
            .map(|slot| slot.fit::<u8>() + 1)
    }

    /// The group the strip's "+" would save the selection to: the lowest
    /// empty one, while the selection holds own units that no live group
    /// already names exactly.
    pub(crate) fn group_on_offer(&self, game: &crate::game::Scene<'_>) -> Option<u8> {
        let human = game.presentation.human;
        let mut own: Vec<UnitId> = game
            .presentation
            .selection
            .units
            .iter()
            .copied()
            .filter(|id| game.state.unit(*id).is_some_and(|u| u.player == human))
            .collect();
        if own.is_empty() {
            return None;
        }
        own.sort_unstable();
        let live = |slot: usize| -> Vec<UnitId> {
            let mut ids: Vec<UnitId> = self.groups[slot]
                .iter()
                .copied()
                .filter(|id| game.state.unit(*id).is_some_and(|u| u.player == human))
                .collect();
            ids.sort_unstable();
            ids
        };
        if (0..self.groups.len()).any(|slot| live(slot) == own) {
            return None;
        }
        (0..self.groups.len())
            .find(|slot| live(*slot).is_empty())
            .map(|slot| slot.fit::<u8>() + 1)
    }

    /// A tap on QUEUE, as on the iPad's Shift key: off latches it for the
    /// action at hand, a second tap inside the double-tap window locks it,
    /// and any later tap turns it off.
    pub(crate) fn toggle_queue(&mut self) {
        self.queue_toggle = match self.queue_toggle {
            QueueToggle::Off => QueueToggle::Latched { at: self.now },
            QueueToggle::Latched { at }
                if (self.now - at) * 1000.0 < f64::from(self.touch_prefs.double_tap_ms) =>
            {
                QueueToggle::Locked
            }
            QueueToggle::Latched { .. } | QueueToggle::Locked => QueueToggle::Off,
        };
    }

    /// What an input step may change that finishes a latched QUEUE's action,
    /// taken only while QUEUE is latched, so ordinary input copies nothing.
    pub(crate) fn latch_probe(&self, game: &Game) -> Option<LatchProbe> {
        matches!(self.queue_toggle, QueueToggle::Latched { .. }).then(|| LatchProbe {
            armed: self.armed_mode().is_some(),
            staged: game.pending.len(),
            units: game.presentation.selection.units.clone(),
            buildings: game.presentation.selection.buildings.clone(),
        })
    }

    /// A latched QUEUE covers one action. While a mode is armed that is the
    /// whole mode, a placement chain or a patrol route, so it lets go when
    /// the mode ends; otherwise it is the next order or selection change.
    pub(crate) fn settle_latch(&mut self, game: &Game, before: Option<&LatchProbe>) {
        let Some(before) = before else {
            return;
        };
        if !matches!(self.queue_toggle, QueueToggle::Latched { .. }) {
            return;
        }
        let armed = self.armed_mode().is_some();
        let selection = &game.presentation.selection;
        let acted = game.pending.len() != before.staged
            || selection.units != before.units
            || selection.buildings != before.buildings;
        if !armed && (before.armed || acted) {
            self.queue_toggle = QueueToggle::Off;
        }
    }

    /// Consumes this frame's menu-button press, if any.
    pub(crate) fn take_menu_request(&mut self) -> bool {
        std::mem::take(&mut self.menu_requested)
    }

    /// Everything `reset_transient` drops, plus state that assumes the
    /// *match* continues: control groups, double-click memory, recall
    /// timing. Called whenever the `Game` is replaced (restart, new map,
    /// replay load) — unit ids restart from zero there, and a stale group
    /// would resolve to unrelated units in the new world.
    pub fn reset_session(&mut self) {
        self.reset_transient();
        self.groups = Default::default();
        self.bookmarks = [None; 4];
        self.last_click = None;
        self.last_recall = None;
    }
}

const KEY_MAP: [(Key, mq::KeyCode); 44] = [
    (Key::Tab, mq::KeyCode::Tab),
    (Key::Up, mq::KeyCode::Up),
    (Key::Down, mq::KeyCode::Down),
    (Key::Left, mq::KeyCode::Left),
    (Key::Right, mq::KeyCode::Right),
    (Key::H, mq::KeyCode::H),
    (Key::S, mq::KeyCode::S),
    (Key::P, mq::KeyCode::P),
    (Key::R, mq::KeyCode::R),
    (Key::B, mq::KeyCode::B),
    (Key::N, mq::KeyCode::N),
    (Key::X, mq::KeyCode::X),
    (Key::Escape, mq::KeyCode::Escape),
    (Key::Space, mq::KeyCode::Space),
    (Key::F1, mq::KeyCode::F1),
    (Key::Enter, mq::KeyCode::Enter),
    (Key::PageUp, mq::KeyCode::PageUp),
    (Key::PageDown, mq::KeyCode::PageDown),
    (Key::Home, mq::KeyCode::Home),
    (Key::End, mq::KeyCode::End),
    (Key::F5, mq::KeyCode::F5),
    (Key::F6, mq::KeyCode::F6),
    (Key::F7, mq::KeyCode::F7),
    (Key::F8, mq::KeyCode::F8),
    (Key::A, mq::KeyCode::A),
    (Key::C, mq::KeyCode::C),
    (Key::D, mq::KeyCode::D),
    (Key::E, mq::KeyCode::E),
    (Key::F, mq::KeyCode::F),
    (Key::G, mq::KeyCode::G),
    (Key::I, mq::KeyCode::I),
    (Key::J, mq::KeyCode::J),
    (Key::K, mq::KeyCode::K),
    (Key::L, mq::KeyCode::L),
    (Key::M, mq::KeyCode::M),
    (Key::O, mq::KeyCode::O),
    (Key::Q, mq::KeyCode::Q),
    (Key::T, mq::KeyCode::T),
    (Key::U, mq::KeyCode::U),
    (Key::V, mq::KeyCode::V),
    (Key::W, mq::KeyCode::W),
    (Key::Y, mq::KeyCode::Y),
    (Key::Z, mq::KeyCode::Z),
    (Key::Backspace, mq::KeyCode::Backspace),
];

/// Translates one hardware touch phase into the raw event the debug
/// harness also injects, so a real fingertip and an injected one walk
/// identical code. `Stationary` maps to nothing: the long-press timer
/// rides the frame loop, not the event stream.
fn touch_event(phase: mq::TouchPhase, id: u64, x: f32, y: f32) -> Option<RawEvent> {
    match phase {
        mq::TouchPhase::Started => Some(RawEvent::TouchDown { id, x, y }),
        mq::TouchPhase::Moved => Some(RawEvent::TouchMove { id, x, y }),
        mq::TouchPhase::Ended => Some(RawEvent::TouchUp { id, x, y }),
        mq::TouchPhase::Cancelled => Some(RawEvent::TouchCancel { id }),
        mq::TouchPhase::Stationary => None,
    }
}

/// The platform's ordered pointer stream, translated event by event
/// into [`RawEvent`]s that keep their own coordinates.
///
/// macroquad's polled surface keeps only the frame's last cursor position
/// and a "was pressed this frame" flag, so a press made while the pointer
/// moves would land wherever the pointer ended the frame, and a drag box
/// would anchor ahead of the click.
///
/// Raw events arrive in backing-store pixels; the dpi factor is
/// injected (never queried) so the whole adapter runs headless.
pub(crate) struct PointerStream {
    dpi: f32,
    /// Whether a text field owns input.
    text_entry: bool,
    /// Reads the system clipboard for a paste.
    clipboard: fn() -> Option<String>,
    /// Translated events, in arrival order.
    pub(crate) events: Vec<RawEvent>,
}

impl PointerStream {
    pub(crate) fn new(dpi: f32, text_entry: bool) -> Self {
        Self {
            dpi: if dpi > 0.0 { dpi } else { 1.0 },
            text_entry,
            clipboard: macroquad::miniquad::window::clipboard_get,
            events: Vec::new(),
        }
    }

    /// Backing-store pixels to the logical space every other coordinate
    /// in the shell speaks (what `screen_width()` reports).
    fn logical(&self, x: f32, y: f32) -> (f32, f32) {
        (x / self.dpi, y / self.dpi)
    }
}

fn mouse_button(button: mq::MouseButton) -> Option<MouseButton> {
    match button {
        mq::MouseButton::Left => Some(MouseButton::Left),
        mq::MouseButton::Right => Some(MouseButton::Right),
        mq::MouseButton::Middle => Some(MouseButton::Middle),
        mq::MouseButton::Unknown => None,
    }
}

static WINDOW_ACTIVITY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Last native minimize/restore event consumed by the input adapter.
/// None means no such event has been observed yet.
pub(crate) fn reported_minimized() -> Option<bool> {
    match WINDOW_ACTIVITY.load(std::sync::atomic::Ordering::Relaxed) {
        1 => Some(false),
        2 => Some(true),
        _ => None,
    }
}

impl macroquad::miniquad::EventHandler for PointerStream {
    fn window_minimized_event(&mut self) {
        WINDOW_ACTIVITY.store(2, std::sync::atomic::Ordering::Relaxed);
        oxide_kit::diagnostics::minimized(true);
    }
    fn window_restored_event(&mut self) {
        WINDOW_ACTIVITY.store(1, std::sync::atomic::Ordering::Relaxed);
        oxide_kit::diagnostics::minimized(false);
    }
    // The collector never runs a frame; it only replays input.
    fn update(&mut self) {}
    fn draw(&mut self) {}

    fn mouse_motion_event(&mut self, x: f32, y: f32) {
        let (x, y) = self.logical(x, y);
        self.events.push(RawEvent::MouseMove { x, y });
    }

    fn mouse_button_down_event(&mut self, button: mq::MouseButton, x: f32, y: f32) {
        let (x, y) = self.logical(x, y);
        if let Some(button) = mouse_button(button) {
            self.events.push(RawEvent::MouseDown { button, x, y });
        }
    }

    fn mouse_button_up_event(&mut self, button: mq::MouseButton, x: f32, y: f32) {
        let (x, y) = self.logical(x, y);
        if let Some(button) = mouse_button(button) {
            self.events.push(RawEvent::MouseUp { button, x, y });
        }
    }

    /// A fingertip must arrive once, as a touch: the trait's default
    /// `touch_event` also emulates mouse clicks. The stream keeps each
    /// phase in order, where the polled `touches()` snapshot keeps only a
    /// finger's last phase per frame and would lose a tap that lands and
    /// lifts inside one frame.
    fn touch_event(&mut self, phase: macroquad::miniquad::TouchPhase, id: u64, x: f32, y: f32) {
        let (x, y) = self.logical(x, y);
        if let Some(event) = touch_event(phase.into(), id, x, y) {
            self.events.push(event);
        }
    }

    /// The polled keyboard surface exposes the initial edge but drops OS
    /// repeat. Preserve repeated Backspace presses only while a text field
    /// owns input; every gameplay binding keeps edge-only semantics. A
    /// paste chord there types the clipboard as ordinary Text events.
    fn key_down_event(
        &mut self,
        keycode: macroquad::miniquad::KeyCode,
        keymods: macroquad::miniquad::KeyMods,
        repeat: bool,
    ) {
        if !self.text_entry {
            return;
        }
        if repeat && keycode == macroquad::miniquad::KeyCode::Backspace {
            self.events.push(RawEvent::KeyDown {
                key: Key::Backspace,
            });
        } else if !repeat
            && keycode == macroquad::miniquad::KeyCode::V
            && chorded(keymods)
            && let Some(text) = (self.clipboard)()
        {
            self.events.extend(
                text.chars()
                    .filter(|ch| typable(*ch))
                    .map(|ch| RawEvent::Text { ch }),
            );
        }
    }

    /// Typed characters, layout- and shift-resolved by the OS (a
    /// Key-to-character table would get non-US layouts wrong). Only
    /// printable ASCII passes, filtered here so nothing downstream needs
    /// its own filter: the menu font is Latin-1 and UI strings stay ASCII.
    /// A shortcut chord types nothing: macOS reports Cmd+V as a `v`.
    fn char_event(
        &mut self,
        character: char,
        keymods: macroquad::miniquad::KeyMods,
        _repeat: bool,
    ) {
        if typable(character) && !chorded(keymods) {
            self.events.push(RawEvent::Text { ch: character });
        }
    }
}

fn typable(ch: char) -> bool {
    ('\u{20}'..='\u{7e}').contains(&ch)
}

/// Whether Cmd, or Ctrl without Alt, is held. `AltGr` reaches some platforms
/// as Ctrl+Alt and still types.
fn chorded(mods: macroquad::miniquad::KeyMods) -> bool {
    mods.logo || (mods.ctrl && !mods.alt)
}

// A fingertip must arrive once, as a touch: macroquad otherwise mirrors
// every touch into synthetic mouse events and the same finger would both
// pan the camera and drag a box.
static TOUCH_SETUP: std::sync::Once = std::sync::Once::new();
// The subscriber is registered on demand so an automation shell, which
// never polls hardware, never accumulates a queue it won't drain.
// Hardware shells must arm before the prologue instead: macroquad's
// register_input_subscriber starts an empty queue, so events dispatched
// earlier (clicks or typing while assets and the autosave scan load in
// frame 1) are lost.
static POINTER_SUB: std::sync::OnceLock<usize> = std::sync::OnceLock::new();

/// Subscribes to the hardware input stream now, so events arriving
/// before the first [`poll_events`] queue instead of vanishing. Call
/// at the top of the frame future, before any prologue work; never
/// call from an automation shell. `poll_events` still self-arms as a
/// fallback and keeps its first-poll cursor seed either way.
pub fn arm_hardware() {
    TOUCH_SETUP.call_once(|| mq::simulate_mouse_with_touch(false));
    POINTER_SUB.get_or_init(mq::utils::register_input_subscriber);
}

/// Converts this frame's hardware input into events. Purely a poll-to-event
/// adapter; interpretation happens in [`apply_events`].
pub fn poll_events(text_entry: bool) -> Vec<RawEvent> {
    static FIRST_POLL: std::sync::Once = std::sync::Once::new();
    TOUCH_SETUP.call_once(|| mq::simulate_mouse_with_touch(false));
    let mut events = Vec::new();
    // Pointer and touch events in true arrival order, each with its own
    // position.
    let sub = *POINTER_SUB.get_or_init(mq::utils::register_input_subscriber);
    FIRST_POLL.call_once(|| {
        // However early the subscriber was armed, a stationary cursor
        // never queues a baseline: seed the stream with the position
        // the pointer already holds, or edge pan and wheel zoom anchor
        // at (0, 0) until the first real motion. mouse_position() is
        // already logical (macroquad divides its dpi out). A touch-only
        // device has no cursor until a trackpad moves one, and a seed
        // would read as the mouse in use.
        if !crate::platform::TOUCH_ONLY {
            let (x, y) = mq::mouse_position();
            events.push(RawEvent::MouseMove { x, y });
        }
    });
    let mut stream = PointerStream::new(macroquad::miniquad::window::dpi_scale(), text_entry);
    mq::utils::repeat_all_miniquad_input(&mut stream, sub);
    events.append(&mut stream.events);
    // macroquad's mouse_leave_event marks held buttons released in the
    // polled state without queuing any subscriber event, so a button let
    // go outside the window would leave drags, pans, and placement strokes
    // latched. Synthesize the missing MouseUp at the last known logical
    // position; a rare duplicate MouseUp is harmless because every release
    // handler tolerates an idle repeat.
    for (mq_btn, btn) in [
        (mq::MouseButton::Left, MouseButton::Left),
        (mq::MouseButton::Middle, MouseButton::Middle),
        (mq::MouseButton::Right, MouseButton::Right),
    ] {
        if mq::is_mouse_button_released(mq_btn)
            && !events
                .iter()
                .any(|e| matches!(e, RawEvent::MouseUp { button, .. } if *button == btn))
        {
            let (x, y) = mq::mouse_position();
            events.push(RawEvent::MouseUp { button: btn, x, y });
        }
    }
    let wheel = mq::mouse_wheel().1;
    if wheel != 0.0 {
        events.push(RawEvent::Wheel {
            delta: normalize_wheel(wheel, NATIVE_WHEEL_UNITS),
        });
    }
    // Modifier edges land before ordinary key edges: a chord pressed
    // whole within one frame (Ctrl and F5 together) must resolve as
    // Ctrl+F5, not as F5 followed by a late Ctrl.
    for (key, a, b) in [
        (Key::Shift, mq::KeyCode::LeftShift, mq::KeyCode::RightShift),
        (
            Key::Ctrl,
            mq::KeyCode::LeftControl,
            mq::KeyCode::RightControl,
        ),
    ] {
        modifier_edges(key, KeyPoll::of(a), KeyPoll::of(b), &mut events);
    }
    for (key, code) in KEY_MAP {
        if mq::is_key_pressed(code) {
            events.push(RawEvent::KeyDown { key });
        }
        if mq::is_key_released(code) {
            events.push(RawEvent::KeyUp { key });
        }
    }
    for (key, code) in [
        (Key::Num1, mq::KeyCode::Key1),
        (Key::Num2, mq::KeyCode::Key2),
        (Key::Num3, mq::KeyCode::Key3),
        (Key::Num4, mq::KeyCode::Key4),
        (Key::Num5, mq::KeyCode::Key5),
        (Key::Num6, mq::KeyCode::Key6),
        (Key::Num7, mq::KeyCode::Key7),
        (Key::Num8, mq::KeyCode::Key8),
        (Key::Num9, mq::KeyCode::Key9),
    ] {
        if mq::is_key_pressed(code) {
            events.push(RawEvent::KeyDown { key });
        }
        if mq::is_key_released(code) {
            events.push(RawEvent::KeyUp { key });
        }
    }
    events
}

/// One physical key as this frame's poll saw it.
#[derive(Debug, Clone, Copy, Default)]
struct KeyPoll {
    pressed: bool,
    released: bool,
    down: bool,
}

impl KeyPoll {
    fn of(code: mq::KeyCode) -> Self {
        Self {
            pressed: mq::is_key_pressed(code),
            released: mq::is_key_released(code),
            down: mq::is_key_down(code),
        }
    }

    /// Whether the key was held as the frame began: not if it was pressed
    /// this frame, otherwise if it is still down or just came up.
    fn held_before(self) -> bool {
        !self.pressed && (self.down || self.released)
    }
}

/// The edges of a logical modifier shared by two physical keys. It goes
/// down with the first key of the pair and up only once neither is held,
/// so releasing one of a held pair keeps the modifier.
fn modifier_edges(key: Key, a: KeyPoll, b: KeyPoll, events: &mut Vec<RawEvent>) {
    if (a.pressed || b.pressed) && !a.held_before() && !b.held_before() {
        events.push(RawEvent::KeyDown { key });
    }
    if (a.released || b.released) && !a.down && !b.down {
        events.push(RawEvent::KeyUp { key });
    }
}

mod dispatch;
mod orders;
mod select;
#[cfg(test)]
mod tests;
mod touch;

use dispatch::dispatch_action;
use orders::{context_order, rally_at, rally_producers, rally_selected_producers};
pub use select::idle_harvesters;
use select::{
    box_select, click_on_hud, click_select, cycle_idle_worker, select_all_of_kind_on_screen,
};
pub use touch::update_touch;
pub(crate) use touch::{
    Pair, TOUCH_REST_MS, TouchBorn, TouchPoint, group_press_progress, long_press_progress,
    touch_box,
};

/// The cursor shape the current intent deserves: crosshair while
/// placing a building or plotting a patrol, a pointer over clickable
/// chrome, the arrow otherwise. Pure — the loop applies it.
pub fn desired_cursor(game: &Game, input: &InputState) -> macroquad::miniquad::CursorIcon {
    use macroquad::miniquad::CursorIcon;
    let layout = game.presentation.layout.get();
    let row = [
        layout.queue_toggle,
        layout.mode_ribbon,
        layout.ghost_confirm,
        layout.ghost_cancel,
        layout.ghost_handle,
    ];
    if row.iter().any(|r| r.w > 0.0 && r.contains(input.mouse)) {
        return CursorIcon::Pointer;
    }
    if input.placing.is_some()
        || input.patrol_route.is_some()
        || input.click_verb.is_some()
        || !input.rallying.is_empty()
        || input.striking.is_some()
    {
        return CursorIcon::Crosshair;
    }
    let layout = game.presentation.layout.get();
    let p = input.mouse;
    if layout.minimap.contains(p)
        || layout.chrome_owns(p)
        || (layout.idle_badge.w > 0.0 && layout.idle_badge.contains(p))
    {
        return CursorIcon::Pointer;
    }
    CursorIcon::Default
}

/// Applies a frame's events — hardware and injected alike — to the game,
/// laying the HUD out again through `relayout` before each press or
/// release that follows another event, and once after the batch. An
/// earlier event can open a panel or move chrome, and a press must
/// hit-test what the next frame draws.
pub fn apply_events(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    events: &[RawEvent],
    mut relayout: impl FnMut(&Game, &InputState),
) {
    for (index, event) in events.iter().enumerate() {
        let press = matches!(
            event,
            RawEvent::MouseDown { .. }
                | RawEvent::MouseUp { .. }
                | RawEvent::TouchDown { .. }
                | RawEvent::TouchUp { .. }
        );
        if press && index > 0 {
            relayout(game, input);
        }
        let queued = input.queue_held();
        let probe = input.latch_probe(game);
        apply_event(game, input, bindings, event);
        input.settle_latch(game, probe.as_ref());
        if queued && !input.queue_held() {
            input.queue_released();
        }
    }
    let selection = &mut game.presentation.selection;
    if !selection.units.is_empty() || !selection.buildings.is_empty() {
        selection.pile = None;
    }
    // A QUEUE tap or a staged command moves what a held row would build.
    refresh_row(&game.view(), input, false);
    if !events.is_empty() {
        relayout(game, input);
    }
}

fn apply_event(game: &mut Game, input: &mut InputState, bindings: &BindingMap, event: &RawEvent) {
    if let Some(pointer) = Pointer::of(event) {
        input.last_pointer = pointer;
    }
    match *event {
        RawEvent::MouseMove { x, y } => {
            let from = std::mem::replace(&mut input.mouse, vec2(x, y));
            // A held minimap press keeps steering: clamp the cursor
            // into the minimap so sliding off its edge doesn't stall
            // the pan mid-gesture.
            if input.minimap_drag
                && let Some(world) = crate::render::minimap_world_clamped(&game.view(), vec2(x, y))
            {
                game.presentation.camera.center = world;
                game.presentation.camera.pan(Vec2::ZERO);
            }
            // Middle-drag: the world follows the hand, so the pan
            // moves against the cursor delta, scaled out of screen
            // space by the zoom.
            if let Some(anchor) = input.mmb_anchor {
                let delta = vec2(x, y) - anchor;
                game.presentation
                    .camera
                    .pan(-delta / game.presentation.camera.zoom);
                input.mmb_anchor = Some(vec2(x, y));
            }
            // Drag-to-place: while the button stays down in
            // placement mode, every new non-overlapping anchor the plan
            // can still afford and fit joins it, queued behind the
            // stamps before it: a wall in one stroke. Nothing stages
            // until the release, so a careless stroke costs nothing.
            // Refused cells skip silently (the ghost tells that story).
            extend_stroke(game, input, from, vec2(x, y));
            if input.handle_drag {
                pull_row(&game.view(), input, vec2(x, y));
            }
        }
        RawEvent::Wheel { delta } => crate::camera::controls::wheel_zoom(
            &mut game.presentation.camera,
            input.mouse,
            delta,
            input.camera_prefs,
        ),
        RawEvent::MouseDown {
            button: MouseButton::Left,
            x,
            y,
        } => {
            input.mouse = vec2(x, y);
            // A trackpad drags the touch ghost's row handle as a finger does,
            // where the cross and check mark don't claim the press first.
            let layout = game.presentation.layout.get();
            let handle = layout.ghost_handle;
            if handle.w > 0.0
                && handle.contains(vec2(x, y))
                && ghost_control_at(&layout, vec2(x, y)).is_none()
            {
                input.handle_drag = true;
                return;
            }
            if ribbon_row_press(game, input, vec2(x, y), Pointer::Mouse)
                || armed_click(game, input, bindings, vec2(x, y), Pointer::Mouse)
            {
                return;
            }
            // Panel cards are buttons: each carries the exact action
            // its click performs — the same action its hotkey routes.
            let layout = game.presentation.layout.get();
            if let Some(hit) = crate::layout::card_under(&layout, vec2(x, y), None) {
                press_card(game, input, bindings, hit);
                return;
            }
            // The idle badge cycles workers on click.
            let layout = game.presentation.layout.get();
            let badge = layout.idle_badge;
            if badge.w > 0.0 && badge.contains(vec2(x, y)) {
                cycle_idle_worker(game);
                return;
            }
            let alert = layout.alert_badge;
            if alert.w > 0.0 && alert.contains(vec2(x, y)) {
                dispatch_action(game, input, bindings, Action::JumpToLastAlert);
                return;
            }
            if let Some(slot) = crate::layout::group_slot_under(&layout, vec2(x, y), None) {
                let assign = input.resolver.ctrl_held();
                press_group_slot(game, input, bindings, slot, assign);
                return;
            }
            if layout.menu_button.w > 0.0 && layout.menu_button.contains(vec2(x, y)) {
                input.menu_requested = true;
                return;
            }
            if layout.pause_status.w > 0.0 && layout.pause_status.contains(vec2(x, y)) {
                dispatch_action(game, input, bindings, Action::TogglePause);
                return;
            }
            // The minimap owns clicks landing on it: jump the camera,
            // never start a drag-select there. HUD chrome swallows
            // clicks outright.
            if let Some(world) = crate::render::minimap_world_at(&game.view(), vec2(x, y)) {
                game.presentation.camera.center = world;
                game.presentation.camera.pan(Vec2::ZERO); // re-clamp
                input.minimap_drag = true;
            } else if !click_on_hud(game, vec2(x, y)) {
                input.drag_origin = Some(vec2(x, y));
            }
        }
        RawEvent::MouseUp {
            button: MouseButton::Left,
            x,
            y,
        } => {
            let from = std::mem::replace(&mut input.mouse, vec2(x, y));
            input.minimap_drag = false;
            if std::mem::take(&mut input.handle_drag) {
                return;
            }
            // The placement stroke ends at release, taking the ground
            // up to the release point; Shift decides whether the mode
            // stays armed, as letting go of it mid-stroke would have.
            extend_stroke(game, input, from, vec2(x, y));
            let stroke = input
                .placing
                .as_mut()
                .and_then(|placement| placement.stroke.take());
            if let (Some(kind), Some(stroke)) = (input.placing_kind(), stroke) {
                commit_stroke(game, input, kind, &stroke, None);
                if !input.queue_held() {
                    input.queue_released();
                }
            }
            if let Some(origin) = input.drag_origin.take() {
                let release = vec2(x, y);
                let additive = input.queue_held();
                if origin.distance(release) <= click_slop(input.ui) {
                    let now = input.now;
                    let double = !additive
                        && input.last_click.take().is_some_and(|(t, p)| {
                            now - t < 0.35 && p.distance(release) <= 12.0 * input.ui
                        });
                    if double {
                        select_all_of_kind_on_screen(game, release, input.ui, Pointer::Mouse);
                    } else {
                        click_select(game, release, additive, input.ui, Pointer::Mouse);
                    }
                    input.last_click = Some((now, release));
                } else {
                    box_select(game, origin, release, additive);
                }
            }
        }
        RawEvent::MouseDown {
            button: MouseButton::Right,
            x,
            y,
        } => {
            input.mouse = vec2(x, y);
            // A right-click mid-stroke throws the planned row away and
            // keeps placing; nothing was spent.
            if input.drop_stroke() {
                return;
            }
            // A contextual order is a new intent, so it also exits
            // any one-shot mode left armed by a prior build/salvage/
            // weld/run/hunt gesture. In particular, a move
            // away from a deferred Found order must not leave a
            // placement ghost stuck to the cursor. Patrol is the
            // exception: its right-clicks are collecting the route.
            if input.patrol_route.is_none() {
                input.close_construction();
            }
            // A right-click on the minimap orders to that world tile
            // (ground semantics — entities can't be picked at that
            // scale); anywhere else, full context ordering. HUD chrome
            // swallows the click.
            let queue = input.queue_held();
            if let Some(world) = crate::render::minimap_world_at(&game.view(), vec2(x, y)) {
                let tile = ground_tile(&game.state, world);
                if input.patrol_route.is_some() {
                    add_patrol_waypoint(game, input, bindings, world);
                } else {
                    let units = game.presentation.selection.units.clone();
                    // The same commandability gate the world path
                    // applies: an inspected ally or enemy takes no
                    // orders from the minimap either.
                    if !units.is_empty() && game.selection_commandable() {
                        game.issue(Command::Advance {
                            units,
                            goal: tile,
                            queue,
                        });
                        game.presentation
                            .ping_order(tile_center(tile), PingKind::Move, queue);
                    } else if units.is_empty() {
                        rally_selected_producers(game, oxide_sim::Rally::Tile(tile));
                    }
                }
            } else if !click_on_hud(game, vec2(x, y)) {
                let world = game.presentation.camera.to_world(vec2(x, y));
                if input.patrol_route.is_some() {
                    add_patrol_waypoint(game, input, bindings, world);
                } else {
                    context_order(game, vec2(x, y), queue);
                }
            }
        }
        RawEvent::MouseUp {
            button: MouseButton::Right,
            ..
        } => {}
        RawEvent::MouseDown {
            button: MouseButton::Middle,
            x,
            y,
        } => {
            input.mmb_anchor = Some(vec2(x, y));
        }
        RawEvent::MouseUp {
            button: MouseButton::Middle,
            ..
        } => {
            input.mmb_anchor = None;
        }
        RawEvent::KeyDown { key } => {
            if let Some(ActionEvent::Pressed(action)) =
                input.key_edge(bindings, key, true, input.context(game))
            {
                dispatch_action(game, input, bindings, action);
            }
        }
        RawEvent::KeyUp { key } => {
            let _ = input.key_edge(bindings, key, false, input.context(game));
        }
        RawEvent::TouchDown { id, x, y } => touch::down(game, input, id, vec2(x, y)),
        RawEvent::TouchMove { id, x, y } => touch::moved(game, input, id, vec2(x, y)),
        RawEvent::Text { .. } => {
            // Typed characters exist for menu text fields (the
            // save-name flow); gameplay deliberately has no text
            // consumer — letters reach the world as semantic keys.
        }
        RawEvent::TouchUp { id, x, y } => touch::up(game, input, bindings, id, vec2(x, y)),
        RawEvent::TouchCancel { id } => touch::cancel(input, id),
    }
    if input.construction_open()
        && !matches!(
            input.context(game),
            crate::action::Context::Construction | crate::action::Context::BuildCategory(_)
        )
    {
        input.close_construction();
    }
}

/// A press on the ribbon row: QUEUE toggles, and the ribbon cancels its
/// mode but keeps the selection. It runs before every other target, for
/// mouse and touch alike.
pub(super) fn ribbon_row_press(
    game: &mut Game,
    input: &mut InputState,
    p: Vec2,
    pointer: Pointer,
) -> bool {
    let layout = game.presentation.layout.get();
    let ui = input.ui;
    let hits = |rect: macroquad::math::Rect| {
        rect.w > 0.0
            && match pointer {
                Pointer::Mouse => rect.contains(p),
                Pointer::Touch => crate::layout::touch_pad(rect, ui).contains(p),
            }
    };
    // The ghost's controls draw over everything else, so they answer first.
    if hits(layout.ghost_confirm) {
        confirm_ghost(game, input);
    } else if hits(layout.ghost_cancel) {
        cancel_ghost(input);
    } else if hits(layout.queue_toggle) {
        input.toggle_queue();
    } else if hits(layout.mode_ribbon) {
        if input.cancel_armed_mode() {
            game.presentation
                .sounds_pending
                .push((crate::game::SoundKind::Click, None));
        }
    } else {
        return false;
    }
    true
}

/// A press on a control-group slot: a saved group recalls (twice
/// quickly centers the camera on it) and the "+" saves the selection;
/// with `assign` (Ctrl, or a fingertip's long-press) any slot saves the
/// selection, which clears the group when nothing is selected.
pub(super) fn press_group_slot(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    slot: crate::layout::GroupSlot,
    assign: bool,
) {
    use crate::layout::GroupSlot;
    match (slot, assign) {
        (GroupSlot::Recall(n), false) => dispatch_action(game, input, bindings, Action::Slot(n)),
        (GroupSlot::Assign(n), _) | (GroupSlot::Recall(n) | GroupSlot::Empty(n), true) => {
            dispatch_action(game, input, bindings, Action::AssignGroup(n));
        }
        (GroupSlot::Empty(_), false) => {}
    }
}

/// One armed world click or tap at screen point `p`. Returns whether
/// an armed mode consumed the event (whatever the outcome: issued,
/// denied, or a minimap camera jump). Mouse and touch route here
/// identically: a fingertip that armed a Build card completes the build
/// with its next tap.
fn armed_click(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    p: Vec2,
    pointer: Pointer,
) -> bool {
    if click_on_hud(game, p) && crate::render::minimap_world_at(&game.view(), p).is_none() {
        return false;
    }
    if let Some(strike) = &mut input.striking {
        // Any tile on the map may be struck, seen or not; the minimap
        // names one as well as the world.
        let minimap = crate::render::minimap_world_at(&game.view(), p);
        let world = minimap
            .or_else(|| (!click_on_hud(game, p)).then(|| game.presentation.camera.to_world(p)));
        if let Some(world) = world {
            let target = ground_tile(&game.state, world);
            if pointer == Pointer::Touch || strike.aim.is_some() {
                // A tap only aims; the check mark launches. A minimap aim
                // brings the camera along so its ring and controls show.
                strike.aim_at(target);
                if minimap.is_some() {
                    game.presentation.camera.center = tile_center(target);
                    game.presentation.camera.pan(Vec2::ZERO); // re-clamp
                }
            } else {
                launch_strike(game, input, target);
            }
        }
        return true;
    }
    if !input.rallying.is_empty() {
        // The minimap names ground only; the world names the own unit under
        // the click, which new units then escort, or else the ground.
        let rally = if let Some(world) = crate::render::minimap_world_at(&game.view(), p) {
            Some(oxide_sim::Rally::Tile(ground_tile(&game.state, world)))
        } else {
            (!click_on_hud(game, p)).then(|| rally_at(game, game.presentation.camera.to_world(p)))
        };
        if let Some(rally) = rally {
            let producers = crate::building_actions::SelectedBuildings::inspect_ids(
                &game.view(),
                &input.rallying,
            )
            .producers();
            rally_producers(game, producers, rally);
            input.rallying.clear();
            game.presentation.toast("Rally point set");
        }
        return true;
    }
    if let Some(kind) = input.placing_kind() {
        // The minimap keeps its meaning while placing: jump the
        // camera, never misread the click as world ground (that would
        // spend scrap on a bogus tile).
        if let Some(world) = crate::render::minimap_world_at(&game.view(), p) {
            game.presentation.camera.center = world;
            game.presentation.camera.pan(Vec2::ZERO); // re-clamp
        } else if !click_on_hud(game, p) {
            let world = game.presentation.camera.to_world(p);
            if pointer == Pointer::Touch || input.ghost_anchor().is_some() {
                // A tap never builds on its own: it drops the ghost (or
                // moves it here), and only the check mark builds. While a
                // ghost is down a click does the same, so a trackpad that
                // takes over from a finger builds where the ghost and the
                // ribbon say, never at an unpreviewed cursor.
                let anchor = ghost_anchor_under(&game.view(), kind, world);
                if let Some(placement) = &mut input.placing {
                    // A moved ghost leaves its row behind.
                    placement.ghost = Some(PlacementGhost::at(anchor));
                }
                return true;
            }
            let clicked = numeric::tile_at(world);
            let anchor = placement_anchor(&game.view(), kind, clicked);
            // The press plans the stroke's first stamp; dragging plans
            // more of the same kind, queued, and the release stages
            // them. Whether the MODE survives the release is still
            // Shift's call, decided at MouseUp.
            let queue = input.queue_held();
            let verdict = placement_verdict(&game.view(), kind, anchor, queue);
            if let Some(refusal) = refusal_text(kind, verdict) {
                refuse(game, refusal);
            } else if let Some(placement) = &mut input.placing {
                placement.stroke = Some(PlacingStroke {
                    queue,
                    stamps: vec![anchor],
                });
            }
        }
        return true;
    }
    armed_verb_click(game, input, bindings, p)
}

/// The anchor that centers `kind`'s footprint on a finger at `world`,
/// so the building lands under the finger rather than down and right of
/// it. An Extractor snaps to the frame under the finger instead.
fn ghost_anchor_under(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    world: Vec2,
) -> TilePos {
    if kind == oxide_sim::BuildingKind::Extractor {
        let tile = numeric::tile_at(world);
        return placement_anchor(game, kind, tile);
    }
    let (w, h) = kind.size();
    TilePos::new(
        numeric::to_i32((world.x - w as f32 * 0.5).round()),
        numeric::to_i32((world.y - h as f32 * 0.5).round()),
    )
}

/// The ghost's footprint on screen.
pub(crate) fn ghost_footprint(
    game: &crate::game::Scene<'_>,
    input: &InputState,
) -> Option<macroquad::math::Rect> {
    if let Some(strike) = &input.striking {
        let blast = oxide_sim::stats::WARHEAD.outer_radius.to_num::<f32>();
        return Some(strike_disc_on_screen(game, strike.aim()?, blast));
    }
    let (kind, anchor) = (input.placing_kind()?, input.ghost_anchor()?);
    Some(footprint_on_screen(game, kind, anchor))
}

/// The screen square around a disc of `radius` tiles centred on `tile`.
fn strike_disc_on_screen(
    game: &crate::game::Scene<'_>,
    tile: TilePos,
    radius: f32,
) -> macroquad::math::Rect {
    let camera = &game.presentation.camera;
    let center = camera.to_screen(tile_center(tile));
    let r = radius * camera.zoom;
    macroquad::math::Rect::new(center.x - r, center.y - r, r * 2.0, r * 2.0)
}

fn footprint_on_screen(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    anchor: TilePos,
) -> macroquad::math::Rect {
    let (w, h) = kind.size();
    let zoom = game.presentation.camera.zoom;
    let corner = game
        .presentation
        .camera
        .to_screen(vec2(anchor.x as f32, anchor.y as f32));
    macroquad::math::Rect::new(corner.x, corner.y, w as f32 * zoom, h as f32 * zoom)
}

/// The ghost's footprint on screen, padded to a fingertip.
pub(crate) fn ghost_touch_rect(
    game: &crate::game::Scene<'_>,
    input: &InputState,
) -> Option<macroquad::math::Rect> {
    // A strike's aim is grabbed by its full-damage core, so a tap in its
    // outer ring still moves it there.
    let grip = match input.striking.as_ref() {
        Some(strike) => strike_disc_on_screen(
            game,
            strike.aim()?,
            oxide_sim::stats::WARHEAD.full_radius.to_num::<f32>(),
        ),
        None => ghost_footprint(game, input)?,
    };
    Some(crate::layout::touch_pad(grip, input.ui))
}

/// Where the touch ghost's row handle sits on screen: past the far edge of
/// the row it has pulled out, or the ghost's east edge before it has one.
/// An Extractor snaps to frames and lays no rows.
pub(crate) fn row_handle_at(game: &crate::game::Scene<'_>, input: &InputState) -> Option<Vec2> {
    let placement = input.placing.as_ref()?;
    let ghost = placement.ghost.as_ref()?;
    if placement.kind == oxide_sim::BuildingKind::Extractor {
        return None;
    }
    let row = ghost.row.as_ref();
    // The handle rides the row's far cell, whether or not that cell builds.
    let end = row.map_or(ghost.anchor, |row| {
        row_cell(ghost.anchor, row.step, row.count - 1)
    });
    let rect = footprint_on_screen(game, placement.kind, end);
    let (dx, dy) = row.map_or((1, 0), |row| (row.step.0.signum(), row.step.1.signum()));
    let edge = match (dx, dy) {
        (-1, _) => vec2(rect.x, rect.center().y),
        (_, -1) => vec2(rect.center().x, rect.y),
        (_, 1) => vec2(rect.center().x, rect.y + rect.h),
        _ => vec2(rect.x + rect.w, rect.center().y),
    };
    // Just outside the edge, so the ghost keeps its own body to drag.
    let reach = crate::theme::MIN_TOUCH_TARGET * input.ui * 0.5;
    Some(edge + vec2(dx as f32, dy as f32) * reach)
}

/// Whether the touch ghost's row runs downward, where its controls would
/// otherwise hang.
pub(crate) fn row_runs_down(input: &InputState) -> bool {
    input
        .placing
        .as_ref()
        .and_then(|placement| placement.ghost.as_ref()?.row.as_ref())
        .is_some_and(|row| row.step.1 > 0)
}

/// The longest row a handle pulls out: one order queue's worth bounds
/// the cells judged per drag.
const ROW_CAP: usize = oxide_sim::stats::ORDER_QUEUE_CAP;

/// Cell `k` of a row from `origin`.
fn row_cell(origin: TilePos, step: (i32, i32), k: usize) -> TilePos {
    let k = k.fit::<i32>();
    TilePos::new(origin.x + step.0 * k, origin.y + step.1 * k)
}

/// Plans cells `cells` of a row from `origin` into `plan`, each judged
/// behind the stamps before it; refused cells are skipped as a mouse
/// stroke skips them.
fn plan_row_cells(
    game: &crate::game::Scene<'_>,
    kind: oxide_sim::BuildingKind,
    origin: TilePos,
    step: (i32, i32),
    cells: std::ops::Range<usize>,
    plan: &mut PlacingStroke,
) {
    for k in cells {
        let anchor = row_cell(origin, step, k);
        let verdict = if plan.stamps.is_empty() {
            placement_verdict(game, kind, anchor, plan.queue)
        } else {
            stroke_verdict(game, kind, plan, anchor)
        };
        if verdict.builds() {
            plan.stamps.push(anchor);
        }
    }
}

/// A finger or cursor dragging the row handle: the row runs from the ghost
/// along whichever axis the pointer leads on, one footprint per step,
/// growing a cell for each footprint the pointer travels past where the
/// handle sits, so the handle's own offset adds nothing at any zoom.
pub(crate) fn pull_row(game: &crate::game::Scene<'_>, input: &mut InputState, p: Vec2) {
    let queue = input.queue_held();
    let reach = crate::theme::MIN_TOUCH_TARGET * input.ui * 0.5 / game.presentation.camera.zoom;
    let Some(Placement {
        kind,
        ghost: Some(ghost),
        ..
    }) = input.placing.as_mut()
    else {
        return;
    };
    let kind = *kind;
    let (w, h) = kind.size();
    let origin = ghost.anchor;
    let center = vec2(
        origin.x as f32 + w as f32 * 0.5,
        origin.y as f32 + h as f32 * 0.5,
    );
    let lead = game.presentation.camera.to_world(p) - center;
    let (step, along, size) = if lead.x.abs() >= lead.y.abs() {
        (
            (numeric::to_i32(lead.x.signum()) * w, 0),
            lead.x.abs(),
            w as f32,
        )
    } else {
        (
            (0, numeric::to_i32(lead.y.signum()) * h),
            lead.y.abs(),
            h as f32,
        )
    };
    let travelled = ((along - size * 0.5 - reach) / size).round().max(0.0);
    let count = (numeric::to_i32(travelled).fit::<usize>() + 1).min(ROW_CAP);
    if count < 2 {
        ghost.row = None;
        return;
    }
    let now = RowJudged::now(game, queue);
    match &mut ghost.row {
        Some(row) if row.step == step && !row.judged.stale(&now, false) => {
            if count < row.count {
                let kept: Vec<_> = (0..count).map(|k| row_cell(origin, step, k)).collect();
                row.plan.stamps.retain(|anchor| kept.contains(anchor));
            } else if count > row.count {
                plan_row_cells(game, kind, origin, step, row.count..count, &mut row.plan);
            }
            row.count = count;
        }
        _ => {
            let mut plan = PlacingStroke {
                queue,
                stamps: Vec::new(),
            };
            plan_row_cells(game, kind, origin, step, 0..count, &mut plan);
            ghost.row = Some(GhostRow {
                step,
                count,
                plan,
                judged: now,
            });
        }
    }
}

/// Judges a held row again once the batch, the selection, or QUEUE has
/// moved, or the world has run on (any tick when `strict`), so its check
/// mark never promises sites that would all fail, nor builds with a queue
/// mode the player has since changed.
pub(crate) fn refresh_row(game: &crate::game::Scene<'_>, input: &mut InputState, strict: bool) {
    let queue = input.queue_held();
    let Some(Placement {
        kind,
        ghost: Some(ghost),
        ..
    }) = input.placing.as_mut()
    else {
        return;
    };
    let origin = ghost.anchor;
    let Some(row) = &mut ghost.row else {
        return;
    };
    let now = RowJudged::now(game, queue);
    if !row.judged.stale(&now, strict) {
        return;
    }
    let mut plan = PlacingStroke {
        queue,
        stamps: Vec::new(),
    };
    plan_row_cells(game, *kind, origin, row.step, 0..row.count, &mut plan);
    row.plan = plan;
    row.judged = now;
}

/// The touch ghost's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GhostControl {
    Confirm,
    Cancel,
}

/// The ghost button under `p`, if any.
pub(crate) fn ghost_control_at(
    layout: &crate::layout::LayoutModel,
    p: Vec2,
) -> Option<GhostControl> {
    let hit = |rect: macroquad::math::Rect| rect.w > 0.0 && rect.contains(p);
    if hit(layout.ghost_confirm) {
        Some(GhostControl::Confirm)
    } else if hit(layout.ghost_cancel) {
        Some(GhostControl::Cancel)
    } else {
        None
    }
}

/// Drops the touch ghost's row, keeping the ghost. Returns whether there
/// was one.
pub(super) fn drop_row(input: &mut InputState) -> bool {
    input
        .placing
        .as_mut()
        .and_then(|placement| placement.ghost.as_mut())
        .is_some_and(|ghost| ghost.row.take().is_some())
}

/// A finger landed on the ghost: it grabs it where it is.
pub(crate) fn grab_ghost(game: &crate::game::Scene<'_>, input: &mut InputState, p: Vec2) {
    let world = game.presentation.camera.to_world(p);
    if let Some(aim) = input
        .striking
        .as_mut()
        .and_then(|strike| strike.aim.as_mut())
    {
        aim.grab = Some((aim.tile, world));
        return;
    }
    if let Some(ghost) = input
        .placing
        .as_mut()
        .and_then(|placement| placement.ghost.as_mut())
    {
        ghost.grab = Some((ghost.anchor, world));
    }
}

/// A grabbing finger moved: the ghost follows by whole tiles, keeping
/// the offset where the finger took hold. A ghost smaller on screen than
/// a fingertip rides a fingertip above the finger instead, so the finger
/// never hides the spot or its tint; either way it builds where it is
/// drawn. Moving it leaves any row behind.
pub(crate) fn drag_ghost(game: &crate::game::Scene<'_>, input: &mut InputState, p: Vec2) {
    let lift = crate::theme::MIN_TOUCH_TARGET * input.ui;
    if let Some(aim) = input
        .striking
        .as_mut()
        .and_then(|strike| strike.aim.as_mut())
    {
        drag_aim(game, aim, p, lift);
        return;
    }
    let Some(Placement {
        kind,
        ghost: Some(ghost),
        ..
    }) = input.placing.as_mut()
    else {
        return;
    };
    let Some((anchor, grabbed)) = ghost.grab else {
        return;
    };
    let camera = &game.presentation.camera;
    let (w, h) = kind.size();
    let small = (w.max(h) as f32) * camera.zoom < lift;
    let moved = if small {
        let raised = camera.to_world(p - vec2(0.0, lift));
        ghost_anchor_under(game, *kind, raised)
    } else {
        let shift = camera.to_world(p) - grabbed;
        TilePos::new(
            anchor.x + numeric::to_i32(shift.x.round()),
            anchor.y + numeric::to_i32(shift.y.round()),
        )
    };
    let moved = placement_anchor(game, *kind, moved);
    if moved != ghost.anchor {
        ghost.anchor = moved;
        ghost.row = None;
    }
}

/// Launches every armed Spire at `target` and disarms the strike.
fn launch_strike(game: &mut Game, input: &mut InputState, target: TilePos) {
    let Some(strike) = input.striking.take() else {
        return;
    };
    for building in strike.spires {
        game.issue(Command::Launch { building, target });
    }
    game.presentation.toast("Strike ordered");
}

/// Drops a touch strike's aim at the middle of the view, so its ring and
/// controls show before the first tap.
pub(super) fn drop_aim_in_view(game: &crate::game::Scene<'_>, input: &mut InputState) {
    let tile = ground_tile(game.state, game.presentation.camera.center);
    if let Some(strike) = &mut input.striking {
        strike.aim_at(tile);
    }
}

/// A grabbing finger moved the strike aim, as [`drag_ghost`] moves a
/// footprint: by whole tiles from the grab, or a fingertip above the finger
/// while the full-damage core is smaller on screen than a fingertip.
fn drag_aim(game: &crate::game::Scene<'_>, aim: &mut StrikeAim, p: Vec2, lift: f32) {
    let Some((tile, grabbed)) = aim.grab else {
        return;
    };
    let camera = &game.presentation.camera;
    let core = oxide_sim::stats::WARHEAD.full_radius.to_num::<f32>() * 2.0;
    aim.tile = if core * camera.zoom < lift {
        ground_tile(game.state, camera.to_world(p - vec2(0.0, lift)))
    } else {
        let shift = camera.to_world(p) - grabbed;
        ground_tile(
            game.state,
            tile_center(tile) + vec2(shift.x.round(), shift.y.round()),
        )
    };
}

/// The ghost's check mark: build exactly where it is drawn, or the whole
/// row it has pulled out, judged afresh, since scrap, fog, and the
/// selection may have changed since it was dropped. A refusal keeps the
/// ghost; a success disarms, unless queueing keeps placement armed, when
/// the ghost moves on to offer the next spot.
pub(crate) fn confirm_ghost(game: &mut Game, input: &mut InputState) {
    if let Some(target) = input.striking.as_ref().and_then(Strike::aim) {
        launch_strike(game, input, target);
        return;
    }
    let Some(kind) = input.placing_kind() else {
        return;
    };
    // The row builds as the toggle stands now, judged against the world as
    // it stands now.
    refresh_row(&game.view(), input, true);
    let row = input
        .placing
        .as_mut()
        .and_then(|placement| placement.ghost.as_mut()?.row.take());
    let placed = if let Some(row) = row {
        commit_stroke(game, input, kind, &row.plan, Some(row.step))
    } else if let Some(anchor) = input.ghost_anchor() {
        place_at(game, input, kind, anchor)
    } else {
        false
    };
    if !placed {
        return;
    }
    if !input.queue_held() {
        input.stop_placing();
        return;
    }
    let view = game.view();
    if let Some(placement) = &mut input.placing
        && let Some(chain) = placement.chain
    {
        let next = TilePos::new(chain.last.x + chain.step.0, chain.last.y + chain.step.1);
        placement.ghost = Some(PlacementGhost::at(placement_anchor(&view, kind, next)));
    }
}

/// The ghost's cross: a strike stands down; a pulled-out row folds back to
/// the ghost; a lone ghost gives the building back to the palette.
pub(crate) fn cancel_ghost(input: &mut InputState) {
    if input.striking.take().is_some() {
        return;
    }
    if !drop_row(input) {
        input.stop_placing();
        input.build_menu = true;
    }
}

/// Drops a fresh ghost where a finger arms placement: at the middle of
/// the view, so the spot and its verdict show before the first tap.
fn drop_ghost_in_view(game: &crate::game::Scene<'_>, input: &mut InputState) {
    let Some(kind) = input.placing_kind() else {
        return;
    };
    let anchor = ghost_anchor_under(game, kind, game.presentation.camera.center);
    if let Some(placement) = &mut input.placing {
        placement.ghost = Some(PlacementGhost::at(anchor));
    }
}

/// Notes a staged site in the placement's chain. The step to the next
/// spot is `step`, else the offset from the last site, else one
/// footprint east.
fn record_placed(input: &mut InputState, anchor: TilePos, step: Option<(i32, i32)>) {
    let Some(placement) = &mut input.placing else {
        return;
    };
    let east = (placement.kind.size().0, 0);
    let step = step
        .or_else(|| {
            placement
                .chain
                .map(|chain| (anchor.x - chain.last.x, anchor.y - chain.last.y))
                .filter(|step| *step != (0, 0))
        })
        .unwrap_or(east);
    placement.chain = Some(Chain { last: anchor, step });
}

/// Stages `kind` at `anchor` for the selected builders, or says why
/// not. Refusals toast and keep placement armed. Returns whether it
/// placed.
fn place_at(
    game: &mut Game,
    input: &mut InputState,
    kind: oxide_sim::BuildingKind,
    anchor: TilePos,
) -> bool {
    // A held queue (Shift, or QUEUE on touch) both keeps placing and
    // queues the build behind the builder's current program.
    match stage_build(game, kind, anchor, input.queue_held()) {
        Ok(()) => {
            record_placed(input, anchor, None);
            true
        }
        Err(refusal) => {
            refuse(game, refusal);
            false
        }
    }
}

/// Stages a released stroke's stamps in order. Each is judged afresh
/// behind the ones staged before it, since a tick or another staged
/// command can change the ground or the bank while the button is down.
/// Returns whether any staged; skipped stamps toast.
fn commit_stroke(
    game: &mut Game,
    input: &mut InputState,
    kind: oxide_sim::BuildingKind,
    stroke: &PlacingStroke,
    step: Option<(i32, i32)>,
) -> bool {
    let mut refusals = Vec::new();
    let mut staged = Vec::new();
    for &anchor in &stroke.stamps {
        // The first stamp that stages opens the row, replacing the
        // program unless the stroke queued, even when an earlier one
        // fails its recheck; the rest queue behind it.
        match stage_build(game, kind, anchor, !staged.is_empty() || stroke.queue) {
            Ok(()) => staged.push(anchor),
            Err(refusal) => refusals.push(refusal),
        }
    }
    let total = stroke.stamps.len();
    let skipped = refusals.len();
    if let Some(first) = refusals.into_iter().next() {
        refuse(
            game,
            if total == 1 {
                first
            } else {
                format!("{skipped} of {total} sites skipped")
            },
        );
    }
    // A row hands on its own step; a stroke, the step between its last two
    // sites.
    if let Some(&last) = staged.last() {
        let step = step.or(match staged[..] {
            [.., before, last] => Some((last.x - before.x, last.y - before.y)),
            _ => None,
        });
        record_placed(input, last, step);
    }
    !staged.is_empty()
}
/// Plans the stamps a cursor dragged from `from` to `to` (screen points)
/// crosses while a stroke is live, sampled finely enough that a fast move
/// leaves no footprint out. Samples over chrome or the minimap plan nothing.
fn extend_stroke(game: &Game, input: &mut InputState, from: Vec2, to: Vec2) {
    let Some(Placement {
        kind,
        stroke: Some(stroke),
        ..
    }) = input.placing.as_mut()
    else {
        return;
    };
    let kind = *kind;
    let camera = &game.presentation.camera;
    let (a, b) = (camera.to_world(from), camera.to_world(to));
    // Half a tile a sample, bounded so a flung cursor cannot stall a frame.
    let samples = numeric::to_i32((a.distance(b) * 2.0).ceil()).clamp(1, 256);
    for step in 1..=samples {
        let t = step as f32 / samples as f32;
        let world = a.lerp(b, t);
        let screen = camera.to_screen(world);
        if click_on_hud(game, screen)
            || crate::render::minimap_world_at(&game.view(), screen).is_some()
        {
            continue;
        }
        let anchor = placement_anchor(&game.view(), kind, numeric::tile_at(world));
        if !stroke.overlaps(kind, anchor)
            && stroke_verdict(&game.view(), kind, stroke, anchor).builds()
        {
            stroke.stamps.push(anchor);
        }
    }
}

/// Stages `kind` at `anchor`, judged after the exact pending command phase
/// with future prices held for surviving deferred claims, or returns the
/// toast that says why the sim would refuse it. Explored-but-unseen ground
/// is judged by the remembered-state predicate the ghost tints from, so Fog
/// here means unscouted ground.
fn stage_build(
    game: &mut Game,
    kind: oxide_sim::BuildingKind,
    anchor: TilePos,
    queue: bool,
) -> Result<(), String> {
    let verdict = placement_verdict(&game.view(), kind, anchor, queue);
    if let Some(refusal) = refusal_text(kind, verdict) {
        return Err(refusal);
    }
    let units = game.presentation.selection.units.clone();
    game.issue(Command::Build {
        units,
        kind,
        anchor,
        queue,
        // Remembered ground defers the claim: the crew walks out and
        // founds on arrival, paying then. The amber ghost shows which
        // timing this stamp gets.
        defer: verdict == PlacementVerdict::BuildsOnArrival,
    });
    game.presentation
        .ping_order(placement_ping(kind, anchor), PingKind::Rally, queue);
    Ok(())
}

/// The toast a placement click raises for `verdict`, if it refuses.
fn refusal_text(kind: oxide_sim::BuildingKind, verdict: PlacementVerdict) -> Option<String> {
    match verdict {
        PlacementVerdict::Builds | PlacementVerdict::BuildsOnArrival => None,
        PlacementVerdict::Refused(refusal) => Some(refusal_toast(refusal)),
        PlacementVerdict::Short(_) => Some(format!(
            "Not enough scrap for a {}",
            crate::typography::entity_name(kind.name())
        )),
        PlacementVerdict::QueueFull => Some("That builder's order queue is full".to_string()),
        PlacementVerdict::Unreachable => {
            Some("Can't build there: your builders can't reach it".to_string())
        }
    }
}

/// Says a placement was refused: the toast and the denied sound.
fn refuse(game: &mut Game, refusal: String) {
    game.presentation.toast(refusal);
    game.presentation
        .sounds_pending
        .push((crate::game::SoundKind::Denied, None));
}

/// The armed left-click verbs after placement: salvage, weld, run, and
/// hunt.
fn armed_verb_click(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    p: Vec2,
) -> bool {
    if input.armed(ClickVerb::Salvage) {
        // As in placement: the minimap jumps the camera, a misclick keeps
        // the mode armed, and Shift chains teardowns behind the crew's
        // program.
        if let Some(world) = crate::render::minimap_world_at(&game.view(), p) {
            game.presentation.camera.center = world;
            game.presentation.camera.pan(Vec2::ZERO); // re-clamp
        } else if !click_on_hud(game, p) {
            let world = game.presentation.camera.to_world(p);
            let tile = numeric::tile_at(world);
            let target = game.state.buildings_at(tile).find(|b| {
                b.player == game.presentation.human
                    && b.built()
                    && b.kind != oxide_sim::BuildingKind::Foundry
            });
            let Some(building) = target.map(|b| b.id) else {
                game.presentation
                    .toast("Salvage wants an own built building (not a Foundry)");
                game.presentation
                    .sounds_pending
                    .push((crate::game::SoundKind::Denied, None));
                return true;
            };
            let units = game.presentation.selection.units.clone();
            game.issue(Command::Salvage {
                units,
                building,
                queue: input.queue_held(),
            });
            game.presentation
                .ping_order(world, PingKind::Harvest, input.queue_held());
            if !input.queue_held() {
                input.click_verb = None;
            }
        }
        return true;
    }
    if input.armed(ClickVerb::Weld) {
        // As in salvage: the minimap jumps the camera, a misclick keeps the
        // mode armed, and Shift chains welds behind the crew's program.
        if let Some(world) = crate::render::minimap_world_at(&game.view(), p) {
            game.presentation.camera.center = world;
            game.presentation.camera.pan(Vec2::ZERO); // re-clamp
        } else if !click_on_hud(game, p) {
            let world = game.presentation.camera.to_world(p);
            let patient = game
                .state
                .units()
                .iter()
                .filter(|u| {
                    u.player == game.presentation.human
                        && u.hp > 0
                        && u.hp < u.kind.stats().max_hp
                        && u.kind.stats().repairable()
                        && u.domain() == oxide_sim::stats::Domain::Ground
                })
                .map(|u| {
                    let at = vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>());
                    (at.distance(world), u.id, u.kind)
                })
                .filter(|(distance, _, kind)| *distance <= unit_pick_radius(*kind))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, target, _)) = patient else {
                game.presentation
                    .toast("Weld wants a damaged own ground unit");
                game.presentation
                    .sounds_pending
                    .push((crate::game::SoundKind::Denied, None));
                return true;
            };
            // A machine cannot weld itself: if the picked patient is the
            // only selected welder, the sim would reject after the ping, so
            // refuse here instead.
            let has_other_welder = game.presentation.selection.units.iter().any(|id| {
                *id != target
                    && game.state.unit(*id).is_some_and(|u| {
                        u.kind.stats().harvest.is_some() && u.player == game.presentation.human
                    })
            });
            if !has_other_welder {
                game.presentation.toast("Weld needs another welder in hand");
                game.presentation
                    .sounds_pending
                    .push((crate::game::SoundKind::Denied, None));
                return true;
            }
            let units = game.presentation.selection.units.clone();
            game.issue(Command::RepairUnit {
                units,
                target,
                queue: input.queue_held(),
            });
            game.presentation
                .ping_order(world, PingKind::Harvest, input.queue_held());
            if !input.queue_held() {
                input.click_verb = None;
            }
        }
        return true;
    }
    if input.armed(ClickVerb::Accompany) {
        // As in weld: the minimap jumps the camera, a misclick keeps the
        // mode armed, and Shift queues the escort behind each program. The
        // picked unit may be one of the selection; the rest escort it.
        if let Some(world) = crate::render::minimap_world_at(&game.view(), p) {
            game.presentation.camera.center = world;
            game.presentation.camera.pan(Vec2::ZERO); // re-clamp
        } else if !click_on_hud(game, p) {
            let world = game.presentation.camera.to_world(p);
            let picked = game
                .state
                .units()
                .iter()
                .filter(|u| u.player == game.presentation.human && u.hp > 0)
                .map(|u| {
                    let at = vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>());
                    (at.distance(world), u.id, u.kind)
                })
                .filter(|(distance, _, kind)| *distance <= unit_pick_radius(*kind))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, target, _)) = picked else {
                game.presentation.toast("Accompany wants an own unit");
                game.presentation
                    .sounds_pending
                    .push((crate::game::SoundKind::Denied, None));
                return true;
            };
            let has_escort = game.presentation.selection.units.iter().any(|id| {
                *id != target
                    && game
                        .state
                        .unit(*id)
                        .is_some_and(|u| u.player == game.presentation.human)
            });
            if !has_escort {
                game.presentation
                    .toast("Accompany needs another unit in hand");
                game.presentation
                    .sounds_pending
                    .push((crate::game::SoundKind::Denied, None));
                return true;
            }
            let units = game.presentation.selection.units.clone();
            game.issue(Command::Accompany {
                units,
                target,
                queue: input.queue_held(),
            });
            game.presentation
                .ping_order(world, PingKind::Move, input.queue_held());
            if !input.queue_held() {
                input.click_verb = None;
            }
        }
        return true;
    }
    if input.armed(ClickVerb::Run) {
        // The minimap jumps the camera, HUD chrome swallows the click, and
        // Shift chains legs and keeps the verb armed.
        if let Some(world) = crate::render::minimap_world_at(&game.view(), p) {
            game.presentation.camera.center = world;
            game.presentation.camera.pan(Vec2::ZERO); // re-clamp
        } else if !click_on_hud(game, p) {
            let world = game.presentation.camera.to_world(p);
            let goal = ground_tile(&game.state, world);
            let units = game.presentation.selection.units.clone();
            game.issue(Command::Run {
                units,
                goal,
                queue: input.queue_held(),
            });
            game.presentation
                .ping_order(tile_center(goal), PingKind::Move, input.queue_held());
            if !input.queue_held() {
                input.click_verb = None;
            }
        }
        return true;
    }
    if input.armed(ClickVerb::Hunt) {
        // Explicit fighting march: minimap jumps the camera, HUD
        // swallows, and Shift chains legs while keeping the verb armed.
        if let Some(world) = crate::render::minimap_world_at(&game.view(), p) {
            game.presentation.camera.center = world;
            game.presentation.camera.pan(Vec2::ZERO); // re-clamp
        } else if !click_on_hud(game, p) {
            let world = game.presentation.camera.to_world(p);
            let goal = ground_tile(&game.state, world);
            let units = game.presentation.selection.units.clone();
            game.issue(Command::Hunt {
                units,
                goal,
                queue: input.queue_held(),
            });
            game.presentation
                .ping_order(tile_center(goal), PingKind::Attack, input.queue_held());
            if !input.queue_held() {
                input.click_verb = None;
            }
        }
        return true;
    }
    if input.armed(ClickVerb::Scout) {
        // Like Run: the minimap jumps the camera, HUD chrome swallows the
        // click, and Shift chains legs and keeps the verb armed.
        if let Some(world) = crate::render::minimap_world_at(&game.view(), p) {
            game.presentation.camera.center = world;
            game.presentation.camera.pan(Vec2::ZERO); // re-clamp
        } else if !click_on_hud(game, p) {
            let world = game.presentation.camera.to_world(p);
            let goal = ground_tile(&game.state, world);
            let units = game.presentation.selection.units.clone();
            game.issue(Command::Scout {
                units,
                goal,
                queue: input.queue_held(),
            });
            game.presentation
                .ping_order(tile_center(goal), PingKind::Move, input.queue_held());
            if !input.queue_held() {
                input.click_verb = None;
            }
        }
        return true;
    }
    if input.patrol_route.is_some() {
        // Left-clicks and taps collect the route just as right-clicks
        // do; the minimap keeps its ground meaning.
        let world = crate::render::minimap_world_at(&game.view(), p)
            .or_else(|| (!click_on_hud(game, p)).then(|| game.presentation.camera.to_world(p)));
        if let Some(world) = world {
            add_patrol_waypoint(game, input, bindings, world);
        }
        return true;
    }
    false
}

/// Adds a waypoint at `world` to the patrol route being collected, or
/// says the route is full.
fn add_patrol_waypoint(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    world: Vec2,
) {
    let key = bindings.label(Action::Patrol);
    let Some(route) = &mut input.patrol_route else {
        return;
    };
    if route.len() >= oxide_sim::stats::ORDER_QUEUE_CAP {
        game.presentation
            .toast(patrol_full_toast(&key, crate::platform::hands()));
    } else {
        let waypoint = ground_tile(&game.state, world);
        route.push(waypoint);
        game.presentation
            .ping(tile_center(waypoint), PingKind::Rally);
    }
}

/// The toast that arms a patrol.
pub(crate) fn patrol_arm_toast(key: &str, hands: crate::platform::Hands) -> String {
    match (hands.touch(), hands.keys) {
        (false, true) => format!("Patrol: click waypoints, {key} to start"),
        (false, false) => "Patrol: click waypoints, then click Patrol again to start".to_string(),
        (true, true) => format!("Patrol: tap waypoints, then {key} or tap Patrol again to start"),
        (true, false) => "Patrol: tap waypoints, then tap Patrol again to start".to_string(),
    }
}

/// The toast when the route has no room for another waypoint.
fn patrol_full_toast(key: &str, hands: crate::platform::Hands) -> String {
    match (hands.touch(), hands.keys) {
        (false, true) => format!("Patrol is full: {key} starts it"),
        (false, false) => "Patrol is full: click Patrol to start it".to_string(),
        (true, true) => format!("Patrol is full: {key} or tap Patrol to start it"),
        (true, false) => "Patrol is full: tap Patrol to start it".to_string(),
    }
}

pub(crate) fn activate_action_card(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    action: Action,
) {
    let Some(panel) = crate::panel::build_for_input(&game.view(), bindings, input) else {
        return;
    };
    if let Some(card) = panel
        .cards
        .iter()
        .find(|card| card.action.semantic() == Some(action))
    {
        if card.enabled {
            activate_card(game, input, bindings, card.action);
        } else if let Some(why) = &card.why {
            game.presentation.toast(why.clone());
        }
    } else if let Action::TrainSlot(slot) = action {
        orders::train(game, slot as usize);
    }
}

/// One panel card pressed — by mouse or fingertip, the same act its
/// hotkey performs.
fn activate_card(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    action: crate::panel::CardAction,
) {
    match action {
        crate::panel::CardAction::Dispatch(Action::TrainSlot(slot)) => {
            orders::train(game, slot as usize);
        }
        crate::panel::CardAction::Dispatch(a) => dispatch_action(game, input, bindings, a),
        crate::panel::CardAction::ArmBuild(kind) => {
            input.build_menu = true;
            input.disarm_click_verbs();
            input.arm_placement(kind);
            input.build_category = Some(crate::action::building_category(kind));
            if input.last_pointer == Pointer::Touch {
                drop_ghost_in_view(&game.view(), input);
            }
        }
        crate::panel::CardAction::ArmRally => {
            let buildings = orders::selected_producers(game);
            if buildings.is_empty() {
                return;
            }
            input.disarm_click_verbs();
            input.rallying = buildings;
        }
        crate::panel::CardAction::CancelProduction(kind) => {
            crate::production::cancel_one(game, kind);
        }
        crate::panel::CardAction::CancelQueue(building, index) => {
            game.issue(Command::CancelTrain { building, index });
        }
        crate::panel::CardAction::CancelRound(building, index) => {
            game.issue(Command::CancelRound { building, index });
        }
        crate::panel::CardAction::CancelSite(building) => {
            game.issue(Command::Cancel { building });
        }
        crate::panel::CardAction::CancelFound(kind, anchor) => {
            game.issue(Command::CancelFound { kind, anchor });
        }
        crate::panel::CardAction::CancelOrder {
            unit,
            key,
            from_end,
        } => {
            let units = game
                .presentation
                .selection
                .units
                .iter()
                .copied()
                .filter(|&id| id != unit)
                .collect();
            game.issue(Command::CancelOrder {
                unit,
                key,
                from_end,
                units,
            });
        }
        crate::panel::CardAction::Upgrade => crate::building_actions::upgrade(game),
        crate::panel::CardAction::ScrapSites => crate::building_actions::scrap_sites(game),
        crate::panel::CardAction::UnloadHere(transport) => {
            if let Some(t) = game.state.unit(transport) {
                let at = t.tile();
                game.issue(Command::Unload {
                    transport,
                    at,
                    queue: false,
                });
            }
        }
        crate::panel::CardAction::ClearRally => {
            for building in orders::selected_producers(game) {
                game.issue(Command::SetRally {
                    building,
                    rally: None,
                });
            }
        }
        crate::panel::CardAction::FilterKind(kind) => {
            // The cut is shell-side only: selections are presentation,
            // no command leaves here. Ctrl, Shift, or a lit QUEUE drops
            // the kind instead of keeping it.
            let keep = !(input.resolver.ctrl_held() || input.queue_held());
            game.presentation.selection.units.retain(|id| {
                game.state
                    .unit(*id)
                    .is_some_and(|u| (u.kind == kind) == keep)
            });
        }
        crate::panel::CardAction::ClosePalette => input.close_construction(),
        crate::panel::CardAction::ClearQueues => crate::production::cancel_all(game),
        crate::panel::CardAction::None | crate::panel::CardAction::Refused => {}
    }
}

/// A pointer press on a drawn card: an enabled card acts, and a
/// disabled one explains itself the way its hotkey does.
fn press_card(
    game: &mut Game,
    input: &mut InputState,
    bindings: &BindingMap,
    hit: crate::layout::CardHit,
) {
    if hit.action != crate::panel::CardAction::Refused {
        activate_card(game, input, bindings, hit.action);
        return;
    }
    let why = game
        .presentation
        .panel_model
        .borrow()
        .as_ref()
        .and_then(|panel| panel.card(hit.row, hit.index))
        .and_then(|card| card.why.clone());
    if let Some(why) = why {
        game.presentation.toast(why);
    }
}

/// Continuous per-frame input (held-key and edge panning).
pub fn update_held(game: &mut Game, input: &InputState, dt: f32) {
    let mut dir = crate::camera::controls::held_pan(&input.resolver);
    if input.camera_prefs.edge_pan && input.last_pointer == Pointer::Mouse && dir == vec2(0.0, 0.0)
    {
        // Edge panning is opt-in because it fights windowed-mode mousing;
        // keyboard panning wins when both apply. A finger leaves the
        // mouse point wherever it last was, so only the mouse pans.
        const EDGE: f32 = 8.0;
        let viewport = game.presentation.camera.viewport();
        if input.mouse.x <= EDGE {
            dir.x -= 1.0;
        } else if input.mouse.x >= viewport.x - EDGE {
            dir.x += 1.0;
        }
        if input.mouse.y <= EDGE {
            dir.y -= 1.0;
        } else if input.mouse.y >= viewport.y - EDGE {
            dir.y += 1.0;
        }
    }
    crate::camera::controls::pan_toward(&mut game.presentation.camera, dir, input.camera_prefs, dt);
}

/// How the native layer scales a wheel reading.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WheelUnits {
    /// miniquad multiplies a macOS wheel's line count by 10 and passes a
    /// trackpad or Magic Mouse's point deltas through unscaled, so every
    /// reading under 10 comes from a precise device. Those points arrive
    /// as small whole numbers that would otherwise read as X11 detents.
    Mac,
    /// Windows' ±120 notches, X11's ±1 detents, and Wayland axis values.
    Generic,
}

const NATIVE_WHEEL_UNITS: WheelUnits = if cfg!(target_os = "macos") {
    WheelUnits::Mac
} else {
    WheelUnits::Generic
};

/// Points of macOS trackpad travel per zoom notch.
const TRACKPAD_POINTS_PER_NOTCH: f32 = 10.0;

/// Normalizes a raw wheel reading toward capped notch counts, so
/// trackpads (small continuous deltas) and discrete wheels (notches of
/// about 120) zoom at a comparable rate. A heuristic: small whole numbers
/// count as X11-style detents, fractional deltas as trackpads.
fn normalize_wheel(raw: f32, units: WheelUnits) -> f32 {
    let delta = if raw.abs() >= 40.0 {
        raw / 120.0
    } else if units == WheelUnits::Mac && raw.abs() < 10.0 {
        raw / TRACKPAD_POINTS_PER_NOTCH
    } else if raw.abs() <= 3.0 && raw.fract() == 0.0 {
        // X11-style discrete detents arrive as small whole numbers;
        // trackpads produce fractional deltas. Exact integers are notches.
        raw
    } else {
        raw / 10.0
    };
    delta.clamp(-3.0, 3.0)
}
