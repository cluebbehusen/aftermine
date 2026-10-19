//! Presentation shared by live play and replay playback. World state is borrowed.
use super::{
    Effect, PingKind, SoundKind, Toast, angle_delta, fx, projectiles, rotor_hull_turn_rate,
    world_vec,
};
use super::{EffectKind, Selection};
use crate::camera::Camera;
use crate::numeric;
use macroquad::prelude::{Vec2, vec2};
use oxide_sim::{Building, Event, PlayerCommand, PlayerId, Scenario, State, Target, UnitId};
use std::collections::HashMap;

/// What an alert warns of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertKind {
    /// Own forces under fire, or a hostile warhead inbound.
    Attack,
    /// A hostile experimental of this kind is assembling or has stood up.
    Assembly(oxide_sim::UnitKind),
}

pub struct Presentation {
    /// The seat local input controls.
    pub human: PlayerId,
    /// Presentation camera.
    pub camera: Camera,
    /// Current selection.
    pub selection: Selection,
    /// Debug overlay on?
    pub overlay: bool,
    /// Positions at the previous tick, for render interpolation.
    pub prev_pos: HashMap<u32, Vec2>,
    /// Compass headings at the previous tick, for angular interpolation.
    pub prev_heading: HashMap<u32, u8>,
    /// Mounted guns' world bearings at the previous tick, by weapon slot.
    pub(crate) prev_mounts: HashMap<u32, [Option<u8>; oxide_sim::stats::MAX_WEAPONS]>,
    /// Sprite rotation per unit (radians; 0 = up).
    pub facing: HashMap<u32, f32>,
    pub(super) hull_heading: HashMap<u32, (f32, f32)>,
    /// Combat aim overrides: unit id -> (angle, fx-clock stamp). A shot
    /// turns the shooter toward its victim and holds briefly; movement
    /// facing resumes when the hold expires. Presentation only.
    pub aim_units: HashMap<u32, (f32, f32)>,
    pub(crate) aim_unit_targets: HashMap<u32, Target>,
    /// Same for buildings (turret mounts track their last victim).
    pub aim_buildings: HashMap<u32, (f32, f32)>,
    /// The live victims defense mounts follow between reports. The last
    /// legal angle remains in `aim_buildings` after a target disappears.
    pub(crate) aim_building_targets: HashMap<u32, Target>,
    /// The Lattice pods that fired: (battery id, pod) -> (angle, fx-clock
    /// stamp). A pod holds its shot's bearing briefly, then swings back
    /// north to reload. Presentation only.
    pub(crate) pod_shots: HashMap<(u32, u8), (f32, f32)>,
    /// Hostile warheads already announced, so each warns once.
    pub(crate) warned_warheads: std::collections::HashSet<oxide_sim::WarheadId>,
    /// Hostile assembly cradles already announced, by owner and anchor, so
    /// each warns once.
    pub(crate) warned_cradles: std::collections::BTreeSet<(u8, i32, i32)>,
    /// Interceptors in flight ahead of their forecast stops.
    pub(crate) prelaunches: Vec<super::fx::Prelaunch>,
    /// Action-driven authored sprite state. This remembers only transient
    /// output events; clearing it never changes simulation truth.
    pub(crate) animations: crate::presentation_animation::AnimationController,
    pub(crate) track_motion: HashMap<u32, crate::track_motion::TrackMotion>,
    pub(crate) maw_motion: HashMap<u32, crate::maw_motion::MawMotion>,
    /// Each drone host's docks, by host.
    pub(crate) dock_motion: HashMap<u32, crate::dock_motion::DockMotion>,
    /// Drones still rising from their docks: host, dock and launch tick.
    pub(crate) drone_launches: HashMap<u32, (UnitId, usize, u64)>,
    /// Each Roc's engine heat last tick and this tick, 0 cold to 1 hot.
    engine_heat: HashMap<u32, [f32; 2]>,
    /// Eased collision slides, kept only while a body is drawn off its
    /// simulation pose.
    pub(crate) slide_motion: HashMap<u32, crate::slide_motion::SlideMotion>,
    pub(crate) projectile_releases: projectiles::ProjectileReleases,
    pub(super) fx_previous: fx::PreviousEffects,
    /// Live effects.
    pub fx: Vec<Effect>,
    /// Retains projectile identity across the impact tick.
    pub(crate) audio_timeline: crate::audio_timeline::AudioTimeline,
    /// Clips queued by this frame's ticks; the main loop drains and plays.
    pub sounds_pending: Vec<(SoundKind, Option<Vec2>)>,
    /// Transient HUD messages, newest last.
    pub toasts: Vec<Toast>,
    /// The network's word on players a LAN match lost track of, or on the
    /// host's pause, pinned while it holds.
    pub net_notice: Option<String>,
    /// Scorch decals where buildings died: (world pos, seconds old).
    /// Scorch decals: where, age in seconds, and width in tiles.
    pub scorches: Vec<(Vec2, f32, f32)>,
    /// Live alerts: world position, seconds of age, and what they warn
    /// of. Pulsed on the minimap, jumpable, aged out by `update_fx`.
    pub alerts: Vec<(Vec2, f32, AlertKind)>,
    /// The latest alert position, which the jump key targets.
    pub last_alert: Option<Vec2>,
    /// Per-region alert rate limiter: 8-tile cell to last raise time in
    /// fx-seconds.
    pub(super) alert_gate: HashMap<(i32, i32), f32>,
    /// Presentation clock: seconds of fx time since session start.
    pub(super) fx_clock: f32,
    /// When each remembered tile (ghost anchors, scrap, wrecks) was
    /// last actually seen, on the fx clock; drives the staleness fade. A
    /// `RefCell` because drawing borrows presentation.
    pub last_seen: std::cell::RefCell<HashMap<(i32, i32), f32>>,
    /// The minimap's cached terrain-and-fog texture layer. Presentation
    /// only, lazily created by the first minimap draw (headless sessions
    /// never touch the GPU). A `RefCell` because drawing borrows presentation.
    pub minimap_layer: std::cell::RefCell<Option<crate::render::MinimapLayer>>,
    /// Slabs, blockers, pits and terraces, laid out from the map by the
    /// first draw. A Presentation never outlives its map.
    ground: std::cell::OnceCell<crate::render::GroundLayout>,
    pub boundary_fog: crate::boundary_fog::BoundaryFog,
    /// The chrome geometry of the latest HUD layout pass. Hit-testing
    /// reads this same model so drawn and clickable regions cannot
    /// disagree. A `Cell` because drawing borrows presentation.
    pub layout: std::cell::Cell<crate::layout::LayoutModel>,
    /// The command panel of the latest HUD layout pass, published with
    /// `layout` and read by input and the tooltip pass.
    pub panel_model: std::cell::RefCell<Option<crate::panel::Panel>>,
    /// The selection's programs through the staged commands, shared by the
    /// orders dock and the waypoint chain. A `RefCell` because drawing
    /// borrows presentation.
    pub(crate) projection: std::cell::RefCell<super::projection::Projection>,
    /// The latest placement verdict, shared by the ghost, the mode ribbon,
    /// and the click.
    pub(crate) placement_verdict: crate::input::VerdictCache,
    /// Whether the surrender overlay (banner + concede stats + the
    /// Esc-to-menu exit) is up. Presentation only; opening the pause
    /// menu dismisses it so spectating the ally stays unobstructed.
    pub conceded_banner: bool,
    /// How the picture relates to the local seat. `overlay` is the
    /// developer's F1 view (grid, ids, camera internals) and is fog-free
    /// whatever the stance.
    pub(crate) stance: Stance,
    /// Where between the last executed tick and the next the picture is
    /// drawn, set by the clock that paces it.
    tick_fraction: f32,
}

/// How the picture relates to the local seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stance {
    /// The seat plays under its team's fog.
    Seat,
    /// The seat is out of an undecided match with no human teammate left
    /// to tip off. `Game` re-derives this with every tick: fog lifts, but
    /// the chrome stays the seat's.
    Out,
    /// Fog-free viewing with a spectator's chrome, for the playback viewer
    /// and the final map.
    Spectator,
}

/// Salvage the viewer knows lies on a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Salvage {
    /// A scrap node, with the scrap left in it and in any wreck on it.
    Scrap(u32),
    /// A wreck, with the scrap left in it.
    Wreck(u32),
}

impl Salvage {
    /// The scrap left, whatever the source.
    pub(crate) fn amount(self) -> u32 {
        match self {
            Self::Scrap(amount) | Self::Wreck(amount) => amount,
        }
    }
}

/// What `human` knows of the salvage on `tile`: live amounts on visible
/// ground, remembered amounts under fog, and nothing where they have
/// never looked, so no readout can leak what fog took back.
pub(crate) fn known_salvage(
    state: &State,
    human: PlayerId,
    all_seeing: bool,
    tile: chassis::grid::TilePos,
) -> Option<Salvage> {
    let vision = state.vision(human);
    let (scrap, wreck) = if all_seeing || vision.visible(tile) {
        (state.map().scrap_at(tile), state.map().wreck_at(tile))
    } else if vision.explored(tile) {
        (vision.remembered_scrap(tile), vision.remembered_wreck(tile))
    } else {
        return None;
    };
    classify_salvage(scrap, wreck)
}

/// A tile's salvage from its node and wreck amounts. A wreck can lie on
/// a live node (a flyer downed over it); it is recoverable once the node
/// is stripped, so it counts toward the node's total.
fn classify_salvage(scrap: u32, wreck: u32) -> Option<Salvage> {
    match (scrap, wreck) {
        (0, 0) => None,
        (0, wreck) => Some(Salvage::Wreck(wreck)),
        (scrap, wreck) => Some(Salvage::Scrap(scrap.saturating_add(wreck))),
    }
}

/// One immutable world paired with its presentation and pending human commands.
#[derive(Clone, Copy)]
pub(crate) struct Scene<'a> {
    pub state: &'a State,
    pub scenario: &'a Scenario,
    pub pending: &'a [PlayerCommand],
    pub presentation: &'a Presentation,
    /// The clock pacing this world: paused, speed, and render alpha.
    pub clock: &'a super::Clock,
    pub seat_styles: crate::seat_style::SeatStyles,
}
impl<'a> Scene<'a> {
    pub fn new(
        state: &'a State,
        scenario: &'a Scenario,
        pending: &'a [PlayerCommand],
        presentation: &'a Presentation,
        clock: &'a super::Clock,
    ) -> Self {
        Self {
            state,
            scenario,
            pending,
            presentation,
            clock,
            seat_styles: crate::seat_style::SeatStyles::new(
                state,
                presentation.human,
                crate::render::colorblind(),
            ),
        }
    }

    /// Whether the current unit selection is the human's to command.
    /// Empty selections read as own (nothing to gate); a foreign
    /// selection is read-only everywhere a verb would act.
    pub fn selection_commandable(&self) -> bool {
        self.presentation
            .selection
            .units
            .first()
            .and_then(|id| self.state.unit(*id))
            .is_none_or(|u| u.player == self.presentation.human)
    }
    /// The human's first Foundry (hotkey target, camera home).
    pub fn home_foundry(&self) -> Option<&'a Building> {
        self.state.buildings().iter().find(|b| {
            b.player == self.presentation.human
                && !b.provisional()
                && b.kind == oxide_sim::BuildingKind::Foundry
        })
    }
    /// The map's ground layouts, built by the first call.
    pub(crate) fn ground(&self) -> &'a crate::render::GroundLayout {
        self.presentation
            .ground
            .get_or_init(|| crate::render::GroundLayout::build(self.state.map(), self.scenario))
    }
    /// The local player's fog view (what rendering and targeting honor).
    pub fn my_vision(&self) -> &oxide_sim::Vision {
        self.state.vision(self.presentation.human)
    }
    /// Whether the viewer sees `unit` now, as the renderer decides.
    pub(crate) fn sees_unit(&self, unit: &oxide_sim::Unit) -> bool {
        sees(
            self.state,
            self.presentation.human,
            self.presentation.all_seeing(),
            unit,
        )
    }
    /// What the local player knows of the salvage on `tile`.
    pub(crate) fn known_salvage(&self, tile: chassis::grid::TilePos) -> Option<Salvage> {
        known_salvage(
            self.state,
            self.presentation.human,
            self.presentation.all_seeing(),
            tile,
        )
    }
    pub(crate) fn draw_hull_heading(&self, id: UnitId, alpha: f32) -> f32 {
        self.presentation.draw_hull_heading(self.state, id, alpha)
    }

    /// The decorated selection's programs as the staged commands will leave
    /// them. Callers drop the borrow before asking again.
    pub(crate) fn projection(&self) -> std::cell::Ref<'a, super::projection::Projection> {
        let units = crate::render::entities::decor_units(self);
        let cache = &self.presentation.projection;
        if !cache
            .borrow()
            .is_current(self.state.current_tick(), self.pending.len(), &units)
        {
            cache.replace(super::projection::Projection::capture(
                self.state,
                self.pending,
                units,
            ));
        }
        cache.borrow()
    }
}
impl Presentation {
    pub(crate) fn new(state: &State, human: PlayerId, viewport: Vec2) -> Self {
        let focus = state
            .buildings()
            .iter()
            .find(|b| b.player == human)
            .map_or_else(
                || {
                    vec2(
                        state.map().width() as f32 * 0.5,
                        state.map().height() as f32 * 0.5,
                    )
                },
                |b| world_vec(b.center()),
            );
        let camera = Camera::new(focus, state.map().width(), state.map().height(), viewport);
        let boundary_fog = crate::boundary_fog::BoundaryFog::new(state, human);
        Self {
            human,
            camera,
            selection: Selection::default(),
            overlay: false,
            prev_pos: HashMap::new(),
            prev_heading: HashMap::new(),
            prev_mounts: HashMap::new(),
            facing: HashMap::new(),
            hull_heading: HashMap::new(),
            aim_units: HashMap::new(),
            aim_unit_targets: HashMap::new(),
            aim_buildings: HashMap::new(),
            aim_building_targets: HashMap::new(),
            pod_shots: HashMap::new(),
            warned_warheads: std::collections::HashSet::new(),
            warned_cradles: std::collections::BTreeSet::new(),
            prelaunches: Vec::new(),
            animations: crate::presentation_animation::AnimationController::default(),
            track_motion: HashMap::new(),
            maw_motion: HashMap::new(),
            dock_motion: HashMap::new(),
            drone_launches: HashMap::new(),
            engine_heat: HashMap::new(),
            slide_motion: HashMap::new(),
            projectile_releases: projectiles::ProjectileReleases::default(),
            fx_previous: fx::PreviousEffects::default(),
            fx: Vec::new(),
            audio_timeline: crate::audio_timeline::AudioTimeline::default(),
            sounds_pending: Vec::new(),
            last_seen: std::cell::RefCell::new(HashMap::new()),
            minimap_layer: std::cell::RefCell::new(None),
            ground: std::cell::OnceCell::new(),
            boundary_fog,
            toasts: Vec::new(),
            net_notice: None,
            scorches: Vec::new(),
            alerts: Vec::new(),
            last_alert: None,
            alert_gate: HashMap::new(),
            fx_clock: 0.0,
            layout: std::cell::Cell::new(crate::layout::LayoutModel::default()),
            panel_model: std::cell::RefCell::new(None),
            projection: std::cell::RefCell::default(),
            placement_verdict: crate::input::VerdictCache::default(),
            conceded_banner: false,
            stance: Stance::Seat,
            tick_fraction: 0.0,
        }
    }
    /// Whether rendering should ignore fog: the debug overlay or any
    /// stance but a seat in play. Chrome decides separately.
    pub fn all_seeing(&self) -> bool {
        self.overlay || self.stance != Stance::Seat
    }

    /// Whether the picture carries a spectator's chrome instead of the
    /// seat's.
    pub fn spectating(&self) -> bool {
        self.stance == Stance::Spectator
    }

    /// The effect clock that aim holds and recoil age against.
    pub fn fx_time(&self) -> f32 {
        self.fx_clock
    }

    /// How far the presentation clock sits between the last executed
    /// tick and the next, 0..1, frozen while paused. Drives anything that
    /// must move on sim time rather than wall time.
    pub fn tick_fraction(&self) -> f32 {
        self.tick_fraction
    }

    /// Sets where between ticks the picture is drawn. Only the clock that
    /// paces this session's world calls it; drawing never moves time.
    pub(crate) fn set_tick_fraction(&mut self, fraction: f32) {
        self.tick_fraction = fraction.clamp(0.0, 1.0);
    }

    /// Drops an order-acknowledgment ping at a world point.
    pub fn ping(&mut self, at: Vec2, kind: PingKind) {
        self.ping_order(at, kind, false);
    }

    /// Drops an order-acknowledgment ping; a `queued` order's ping says
    /// it joined the program rather than replacing it.
    pub fn ping_order(&mut self, at: Vec2, kind: PingKind, queued: bool) {
        // Accepted orders get an audible acknowledgment too; the mixer
        // rate-limits bursts.
        self.sounds_pending.push((SoundKind::Ack, None));
        self.fx.push(Effect {
            kind: EffectKind::Ping { at, kind, queued },
            age: 0.0,
        });
    }

    /// Raises a transient HUD message (capped; oldest fall off).
    pub fn toast(&mut self, text: impl Into<String>) {
        let text = crate::typography::sentence_case(&text.into());
        self.toasts.retain(|toast| toast.text != text);
        self.toasts.push(Toast { text, age: 0.0 });
        if self.toasts.len() > 3 {
            self.toasts.remove(0);
        }
    }

    /// Raises an under-attack alert, rate-limited per 8-tile region so a
    /// running battle alerts once, not once per hit.
    pub(super) fn raise_alert(&mut self, world: Vec2) {
        let cell = (
            numeric::to_i32(world.x / 8.0),
            numeric::to_i32(world.y / 8.0),
        );
        let now = self.fx_clock;
        if self
            .alert_gate
            .get(&cell)
            .is_some_and(|&last| now - last < 6.0)
        {
            return;
        }
        self.alert_gate.insert(cell, now);
        self.alerts.push((world, 0.0, AlertKind::Attack));
        self.last_alert = Some(world);
        self.sounds_pending.push((SoundKind::Alert, None));
    }

    /// Marks an incoming hostile warhead on the minimap and plays its
    /// warning. It bypasses the under-attack gate: each warhead warns once,
    /// even into a region already under attack.
    pub(super) fn raise_warhead_alert(&mut self, world: Vec2) {
        self.alerts.push((world, 0.0, AlertKind::Attack));
        self.last_alert = Some(world);
        self.sounds_pending.push((SoundKind::IncomingWarning, None));
    }

    /// Marks a hostile assembly cradle the moment its first work makes it
    /// public. It bypasses the under-attack gate: each cradle warns once.
    pub(super) fn raise_cradle_alert(&mut self, world: Vec2, kind: oxide_sim::UnitKind) {
        self.alerts.push((world, 0.0, AlertKind::Assembly(kind)));
        self.last_alert = Some(world);
        self.sounds_pending.push((SoundKind::Alert, None));
    }

    /// Interpolated draw position for a unit, trailing a collision slide by
    /// its eased lag.
    pub fn draw_pos(&self, id: UnitId, current: chassis::fx::Vec2Fx, alpha: f32) -> Vec2 {
        let now = world_vec(current);
        let lag = self
            .slide_motion
            .get(&id.0)
            .map_or(Vec2::ZERO, |slide| slide.lag(alpha));
        match self.prev_pos.get(&id.0) {
            Some(prev) => prev.lerp(now, alpha) - lag,
            None => now - lag,
        }
    }

    /// Drawn hull lean toward a collision slide, added to the body's rotation.
    pub(crate) fn slide_yaw(&self, id: UnitId, alpha: f32, reduced_motion: bool) -> f32 {
        if reduced_motion {
            return 0.0;
        }
        self.slide_motion
            .get(&id.0)
            .map_or(0.0, |slide| slide.yaw(alpha))
    }

    pub fn draw_heading(&self, id: UnitId, current: u8, alpha: f32) -> f32 {
        let previous = self.prev_heading.get(&id.0).copied().unwrap_or(current);
        let delta = current.wrapping_sub(previous).cast_signed();
        (f32::from(previous) + f32::from(delta) * alpha.clamp(0.0, 1.0)) * std::f32::consts::TAU
            / 256.0
            + std::f32::consts::FRAC_PI_2
    }

    /// The drawn world bearing of the mounted gun in `slot`, turning the
    /// short way from last tick's: a gun traverses far less than a half
    /// turn a tick, so it never appears to sweep its blind quarter.
    pub(crate) fn draw_mount_heading(
        &self,
        unit: &oxide_sim::state::Unit,
        slot: usize,
        alpha: f32,
    ) -> Option<f32> {
        let current = unit.mount_bearing(slot)?;
        let previous = self
            .prev_mounts
            .get(&unit.id.0)
            .and_then(|bearings| bearings[slot])
            .unwrap_or(current);
        let delta = current.wrapping_sub(previous).cast_signed();
        Some(
            (f32::from(previous) + f32::from(delta) * alpha.clamp(0.0, 1.0))
                * std::f32::consts::TAU
                / 256.0
                + std::f32::consts::FRAC_PI_2,
        )
    }

    pub(crate) fn chassis_turning(&self, unit: &oxide_sim::state::Unit) -> bool {
        if unit.kind.ground_turn_rate() == 0 {
            return false;
        }
        if unit.kind.has_ground_turret() {
            self.hull_heading
                .get(&unit.id.0)
                .is_some_and(|(previous, current)| angle_delta(*previous, *current).abs() > 1e-6)
        } else {
            self.prev_heading
                .get(&unit.id.0)
                .is_some_and(|previous| *previous != unit.heading)
        }
    }

    /// Drops queued transient presentation so a bulk jump (a seek) does
    /// not replay it as a burst of noise.
    pub fn drop_presentation(&mut self, state: &State) {
        self.fx.clear();
        self.projection.take();
        self.placement_verdict.clear();
        self.restore_pending_crashes(state);
        self.sounds_pending.clear();
        self.toasts.clear();
        // Aim holds and recoil stamps are per-timeline: after a seek,
        // an id that exists at the destination must not face or flash
        // for a shot fired on the timeline we just left.
        self.aim_units.clear();
        self.aim_unit_targets.clear();
        self.hull_heading.clear();
        self.aim_buildings.clear();
        self.aim_building_targets.clear();
        self.pod_shots.clear();
        self.warned_warheads.clear();
        self.warned_cradles.clear();
        self.prelaunches.clear();
        self.animations.reset_transients();
        self.animations.reset_workers(state);
        self.animations.observe_radars(state);
        self.animations.observe_braces(state);
        self.audio_timeline.clear();
        self.track_motion.clear();
        self.maw_motion.clear();
        self.dock_motion.clear();
        self.drone_launches.clear();
        self.engine_heat.clear();
        self.slide_motion.clear();
    }

    /// A Maw cuts while its own hits are this recent: two strokes of its
    /// cutter.
    const MAW_CUT_TICKS: u64 = 8;

    fn observe_maws(&mut self, state: &State) {
        let tick = state.current_tick();
        self.maw_motion
            .retain(|id, _| state.unit(UnitId(*id)).is_some());
        for unit in state
            .units()
            .iter()
            .filter(|unit| unit.kind == oxide_sim::UnitKind::Maw)
        {
            let cutting = self
                .animations
                .last_unit_attack(unit.id)
                .is_some_and(|hit| tick.saturating_sub(hit) <= Self::MAW_CUT_TICKS);
            let feeding = state.kiln_fuel(unit.id).is_some();
            self.maw_motion
                .entry(unit.id.0)
                .or_insert_with(|| crate::maw_motion::MawMotion::settled(tick, cutting, feeding))
                .observe(tick, cutting, feeding);
        }
    }

    /// Fills and empties each drone host's docks from this tick's launches
    /// and landings. A landing drone glides in from where it was drawn
    /// last tick.
    fn observe_docks(&mut self, state: &State, events: &[Event]) {
        let tick = state.current_tick();
        self.dock_motion.retain(|id, _| {
            state
                .unit(UnitId(*id))
                .is_some_and(|unit| unit.bay.is_some())
        });
        self.drone_launches.retain(|id, (_, _, at)| {
            state.unit(UnitId(*id)).is_some()
                && ((tick - *at) as f32) < crate::dock_motion::LAUNCH_TICKS
        });
        for unit in state.units() {
            let Some(bay) = unit.bay else {
                continue;
            };
            if !sees(state, self.human, self.all_seeing(), unit) {
                // Its bay comes back into view at rest, with no launch or
                // landing to give away what happened out of sight.
                self.dock_motion.insert(
                    unit.id.0,
                    crate::dock_motion::DockMotion::settled(bay.docked),
                );
                continue;
            }
            let mut launched = Vec::new();
            let mut landed = Vec::new();
            for event in events {
                match *event {
                    Event::DroneLaunched { host, drone, .. } if host == unit.id => {
                        launched.push(drone.0);
                    }
                    Event::DroneDocked { host, drone, .. } if host == unit.id => {
                        landed.push(drone.0);
                    }
                    _ => {}
                }
            }
            launched.sort_unstable();
            landed.sort_unstable();
            // A drone glides in only from where the viewer could see it.
            let landed: Vec<Vec2> = landed
                .iter()
                .filter_map(|drone| self.prev_pos.get(drone).copied())
                .filter(|from| {
                    self.all_seeing() || state.vision(self.human).visible(numeric::tile_at(*from))
                })
                .collect();
            let docks = self
                .dock_motion
                .entry(unit.id.0)
                .or_insert_with(|| crate::dock_motion::DockMotion::settled(bay.docked));
            for (drone, dock) in docks.observe(tick, bay.docked, &launched, &landed) {
                self.drone_launches.insert(drone, (unit.id, dock, tick));
            }
        }
    }

    /// A Roc's engines heat quickly while it moves and cool slowly once it
    /// stops. A Roc first seen moving starts cold.
    fn observe_engines(&mut self, state: &State) {
        // A Roc out of sight keeps no heat, so its glow never tells how it
        // moved while hidden.
        let (human, all_seeing) = (self.human, self.all_seeing());
        let sees = |unit: &oxide_sim::Unit| sees(state, human, all_seeing, unit);
        self.engine_heat
            .retain(|id, _| state.unit(UnitId(*id)).is_some_and(sees));
        for unit in state
            .units()
            .iter()
            .filter(|unit| unit.kind == oxide_sim::UnitKind::Roc && sees(unit))
        {
            let moving = self
                .prev_pos
                .get(&unit.id.0)
                .is_some_and(|previous| (*previous - world_vec(unit.pos)).length_squared() > 1e-6);
            let heat = self.engine_heat.entry(unit.id.0).or_insert([0.0; 2]);
            *heat = [heat[1], next_engine_heat(heat[1], moving)];
        }
    }

    /// A Roc's engine heat now, between last tick's and this tick's.
    pub(crate) fn engine_heat(&self, id: UnitId) -> f32 {
        self.engine_heat
            .get(&id.0)
            .map_or(0.0, |[previous, current]| {
                previous + (current - previous) * self.tick_fraction()
            })
    }

    /// Sprite rotation for this tick: a heading-first airframe faces where
    /// the simulation says it does, parked or flying, and everything else
    /// faces the way it last moved. Live ticks, playback, and seeks all
    /// agree through this one rule.
    pub(super) fn refresh_facing(&mut self, state: &State, movement: &[oxide_sim::GroundMotion]) {
        let tick = state.current_tick();
        self.observe_slides(state, movement);
        self.track_motion
            .retain(|id, _| state.unit(UnitId(*id)).is_some());
        for (unit, gauge) in state
            .units()
            .iter()
            .filter_map(|unit| Some((unit, crate::render::tread_gauge(unit.kind)?)))
        {
            let heading = f32::from(unit.heading) * std::f32::consts::TAU / 256.0
                + self
                    .slide_motion
                    .get(&unit.id.0)
                    .map_or(0.0, super::super::slide_motion::SlideMotion::current_yaw);
            self.track_motion
                .entry(unit.id.0)
                .or_insert_with(|| crate::track_motion::TrackMotion::new(tick, heading))
                .observe(
                    tick,
                    heading,
                    gauge,
                    // A shove along the hull rolls the belts like driving
                    // does; the part across the hull is a skid.
                    movement
                        .binary_search_by_key(&unit.id, |motion| motion.unit)
                        .ok()
                        .map_or(Vec2::ZERO, |index| {
                            world_vec(movement[index].propulsion + movement[index].correction)
                        }),
                );
        }

        self.observe_maws(state);

        self.aim_unit_targets
            .retain(|id, _| state.unit(UnitId(*id)).is_some());
        self.hull_heading
            .retain(|id, _| state.unit(UnitId(*id)).is_some());
        for unit in state.units() {
            if unit.kind.stats().turn_rate > 0
                || unit.kind.ground_turn_rate() > 0
                || unit.kind.stats().cruise_turn_rate > 0
            {
                let angle = f32::from(unit.heading) * std::f32::consts::TAU / 256.0;
                self.facing
                    .insert(unit.id.0, angle + std::f32::consts::FRAC_PI_2);
                if unit.kind.has_ground_turret() {
                    let target = angle + std::f32::consts::FRAC_PI_2;
                    let entry = self
                        .hull_heading
                        .entry(unit.id.0)
                        .or_insert((target, target));
                    *entry = (entry.1, target);
                }
                continue;
            }
            let now = world_vec(unit.pos);
            if let Some(prev) = self.prev_pos.get(&unit.id.0) {
                let delta = now - *prev;
                if delta.length_squared() > 1e-6 {
                    self.facing.insert(
                        unit.id.0,
                        delta.y.atan2(delta.x) + std::f32::consts::FRAC_PI_2,
                    );
                }
            }
            if let Some(turn) = rotor_hull_turn_rate(unit.kind) {
                let target = self.facing.get(&unit.id.0).copied().unwrap_or(0.0);
                let (previous, current) = self.hull_heading.entry(unit.id.0).or_insert((0.0, 0.0));
                *previous = *current;
                *current += angle_delta(*current, target).clamp(-turn, turn);
            }
        }
    }

    /// Whether a body's drawn hull may lean away from its simulation heading.
    /// A fixed weapon's hull is its aim and a body at work faces its work;
    /// neither may be drawn pointing anywhere else.
    fn slide_lean_allowed(&self, unit: &oxide_sim::state::Unit, propulsion: Vec2) -> bool {
        let aiming = !unit.kind.has_ground_turret()
            && (unit.braces() > 0
                || self
                    .aim_units
                    .get(&unit.id.0)
                    .is_some_and(|(_, at)| self.fx_clock - at < 1.2));
        !aiming && (propulsion != Vec2::ZERO || matches!(unit.order, oxide_sim::Order::Idle))
    }

    /// Eases this tick's collision slides. A body keeps an entry only while
    /// it is drawn off its simulation pose.
    fn observe_slides(&mut self, state: &State, movement: &[oxide_sim::GroundMotion]) {
        let tick = state.current_tick();
        let mut slides = std::mem::take(&mut self.slide_motion);
        slides.retain(|id, _| state.unit(UnitId(*id)).is_some());
        for motion in movement {
            // Newborns have no interpolation origin. Start their slide history
            // next tick so the first pose and the following tick meet.
            if !self.prev_pos.contains_key(&motion.unit.0) {
                continue;
            }
            let sliding = motion.correction != chassis::fx::Vec2Fx::ZERO;
            if !sliding && !slides.contains_key(&motion.unit.0) {
                continue;
            }
            let Some(unit) = state.unit(motion.unit) else {
                continue;
            };
            let propulsion = world_vec(motion.propulsion);
            slides
                .entry(unit.id.0)
                .or_insert_with(|| crate::slide_motion::SlideMotion::new(tick.wrapping_sub(1)))
                .observe(
                    tick,
                    f32::from(unit.heading) * std::f32::consts::TAU / 256.0,
                    propulsion,
                    world_vec(motion.correction),
                    unit.kind.stats().speed.to_num::<f32>(),
                    self.slide_lean_allowed(unit, propulsion),
                );
        }
        // A body at rest has no motion record; its lag still has to release.
        for (id, slide) in &mut slides {
            let unit = state.unit(UnitId(*id)).expect("retained live unit");
            slide.observe(
                tick,
                0.0,
                Vec2::ZERO,
                Vec2::ZERO,
                1.0,
                self.slide_lean_allowed(unit, Vec2::ZERO),
            );
        }
        slides.retain(|_, slide| !slide.settled());
        self.slide_motion = slides;
    }

    /// Records what the next tick's presentation needs to know about
    /// this one: every body's position for interpolation, and which
    /// airframes were parked for the death effect's fall-or-scatter
    /// choice.
    pub(crate) fn remember_previous_tick(&mut self, state: &State) {
        self.audio_timeline.remember_arrivals(state);
        self.fx_previous = fx::PreviousEffects::capture(self, state);
        self.prev_heading = state
            .units()
            .iter()
            .map(|unit| (unit.id.0, unit.weapon_heading()))
            .collect();
        self.prev_mounts = state
            .units()
            .iter()
            .filter(|unit| unit.mounts.is_some())
            .map(|unit| {
                (
                    unit.id.0,
                    std::array::from_fn(|slot| unit.mount_bearing(slot)),
                )
            })
            .collect();
        self.prev_pos = state
            .units()
            .iter()
            .map(|unit| (unit.id.0, world_vec(unit.pos)))
            .collect();
    }

    pub(crate) fn draw_hull_heading(&self, state: &State, id: UnitId, alpha: f32) -> f32 {
        self.hull_heading.get(&id.0).map_or_else(
            || {
                state.unit(id).map_or(0.0, |unit| {
                    if rotor_hull_turn_rate(unit.kind).is_some() {
                        return self.facing.get(&id.0).copied().unwrap_or(0.0);
                    }
                    f32::from(unit.heading) * std::f32::consts::TAU / 256.0
                        + std::f32::consts::FRAC_PI_2
                })
            },
            |(previous, current)| {
                previous + angle_delta(*previous, *current) * alpha.clamp(0.0, 1.0)
            },
        )
    }

    /// Updates shared presentation after one live or replay tick.
    pub(crate) fn observe_tick(
        &mut self,
        state: &oxide_sim::State,
        events: &[Event],
        movement: &[oxide_sim::GroundMotion],
    ) {
        self.animations.observe_events(state.current_tick(), events);
        self.animations.observe_workers(state);
        self.animations.observe_radars(state);
        self.queue_brace_sounds(state);
        self.animations.observe_braces(state);
        self.spawn_fx(state, events);
        self.observe_docks(state, events);
        self.observe_engines(state);
        // Effects retain the intercepted flight before its launch record is removed.
        self.projectile_releases.observe(state, events);
        self.refresh_facing(state, movement);

        self.facing
            .retain(|id, _| state.unit(UnitId(*id)).is_some());
        self.aim_units
            .retain(|id, _| state.unit(UnitId(*id)).is_some());
        self.aim_buildings
            .retain(|id, _| state.building(oxide_sim::BuildingId(*id)).is_some());
        self.aim_building_targets.retain(|id, target| {
            state.building(oxide_sim::BuildingId(*id)).is_some()
                && match target {
                    Target::Unit(id) => state.unit(*id).is_some(),
                    Target::Building(id) => state.building(*id).is_some(),
                }
        });
        self.animations.retain_live(state);
    }

    /// Resets presentation after a seek or replay rebuild and establishes that
    /// destination as both interpolation endpoints. Timeline-local facing,
    /// aim, reports, and effects cannot survive across the jump.
    pub(crate) fn reset_after_jump(&mut self, state: &State) {
        self.boundary_fog = crate::boundary_fog::BoundaryFog::new(state, self.human);
        self.projectile_releases = projectiles::ProjectileReleases::default();
        self.drop_presentation(state);
        self.remember_previous_tick(state);
        self.facing.clear();
        // Nothing has moved across a jump, but a heading-first airframe
        // still has a heading to show, parked or flying.
        self.refresh_facing(state, &[]);
    }
}

/// Whether `human` sees `unit` now, as the renderer decides.
fn sees(state: &State, human: PlayerId, all_seeing: bool, unit: &oxide_sim::Unit) -> bool {
    unit.player == human || all_seeing || state.vision(human).sees_unit(unit)
}

/// The engines' heat a tick on: easing toward full heat while moving, back
/// toward cold at a third of the rate once still.
pub(crate) fn next_engine_heat(heat: f32, moving: bool) -> f32 {
    if moving {
        heat + (1.0 - heat) * 0.12
    } else {
        let cooled = heat * (1.0 - 0.035);
        if cooled < 0.01 { 0.0 } else { cooled }
    }
}

#[cfg(test)]
mod tests;
