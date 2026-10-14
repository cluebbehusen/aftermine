//! What a seat may know about magazines and warheads, and the battery
//! assignment both interception and a team's forecast use.
//!
//! The observation and the protocol view read these projections, so the
//! bot's and the player's knowledge of strikes agree by construction.

use crate::ids::{BuildingId, PlayerId, WarheadId};
use crate::state::{Building, State, Warhead};
use chassis::Tick;
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;
use serde::{Deserialize, Serialize};

/// A magazine building's rounds as one seat may know them: loaded rounds
/// and a silo's deck are physical and show wherever the building is in
/// sight; orders, assembly and the held strike stay with its team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownMagazine {
    /// The Spire or Lattice.
    pub building: BuildingId,
    /// Rounds ready.
    pub loaded: u8,
    /// A silo's deck facing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<u8>,
    /// The team's own detail; `None` for a hostile building.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<MagazineDetail>,
}

/// A team's own view of one of its magazines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MagazineDetail {
    /// Paid rounds not yet assembled.
    pub ordered: u8,
    /// Ticks of work on the round being assembled.
    pub progress: u32,
    /// Whether it orders rounds on its own.
    pub autofill: bool,
    /// A silo's ordered strike, not yet launched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<TilePos>,
}

/// A warhead in flight as one seat may know it. The team's own and allied
/// warheads are known in full; a hostile one appears while the team holds
/// it in sight or radar, its line once the team has tracked it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownWarhead {
    /// The warhead.
    pub id: WarheadId,
    /// The launching seat, when known: always for the team's own, for a
    /// hostile warhead only while in true sight.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<PlayerId>,
    /// Whether it is hostile to the viewer.
    pub hostile: bool,
    /// Where it was on the last tick.
    pub position: Vec2Fx,
    /// Its line, once known: launch point, impact point and arrival tick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<WarheadLine>,
    /// The team's hold on a hostile warhead: ticks held so far.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<u8>,
    /// The viewer team's battery expected to stop it, if any would on the
    /// rounds loaded now. A forecast, not a promise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interceptor: Option<BuildingId>,
}

/// A warhead's whole flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarheadLine {
    /// Where it launched.
    pub launch: Vec2Fx,
    /// Where it lands.
    pub impact: Vec2Fx,
    /// The tick it lands on.
    pub arrival: Tick,
}

impl State {
    /// Every magazine `viewer` may know of, in building-id order.
    pub fn known_magazines(&self, viewer: PlayerId) -> Vec<KnownMagazine> {
        self.buildings
            .iter()
            .filter_map(|b| {
                let magazine = b.magazine?;
                let own_team = !self.hostile(viewer, b.player);
                let in_sight = || {
                    self.building_apparent(viewer, b)
                        && b.tiles().any(|tile| self.can_see(viewer, tile))
                };
                if !own_team && !in_sight() {
                    return None;
                }
                Some(KnownMagazine {
                    building: b.id,
                    loaded: magazine.loaded,
                    heading: b.launcher.map(|launcher| launcher.heading),
                    detail: own_team.then(|| MagazineDetail {
                        ordered: magazine.ordered,
                        progress: magazine.progress,
                        autofill: magazine.autofill,
                        target: b.launcher.and_then(|launcher| launcher.target),
                    }),
                })
            })
            .collect()
    }

    /// Every magazine as an omniscient QA view shows it to `viewer`:
    /// hostile ones as if in sight.
    pub fn omniscient_magazines(&self, viewer: PlayerId) -> Vec<KnownMagazine> {
        self.buildings
            .iter()
            .filter_map(|b| {
                let magazine = b.magazine?;
                Some(KnownMagazine {
                    building: b.id,
                    loaded: magazine.loaded,
                    heading: b.launcher.map(|launcher| launcher.heading),
                    detail: (!self.hostile(viewer, b.player)).then(|| MagazineDetail {
                        ordered: magazine.ordered,
                        progress: magazine.progress,
                        autofill: magazine.autofill,
                        target: b.launcher.and_then(|launcher| launcher.target),
                    }),
                })
            })
            .collect()
    }

    /// Every warhead in flight as an omniscient QA view shows it to
    /// `viewer`: hostile ones identified and on their line.
    pub fn omniscient_warheads(&self, viewer: PlayerId) -> Vec<KnownWarhead> {
        let last = self.tick.saturating_sub(1);
        self.warheads
            .iter()
            .map(|w| KnownWarhead {
                id: w.id,
                player: Some(w.player),
                hostile: self.hostile(viewer, w.player),
                position: w.position(last),
                line: Some(WarheadLine {
                    launch: w.launch,
                    impact: w.impact,
                    arrival: w.arrival,
                }),
                exposure: None,
                interceptor: None,
            })
            .collect()
    }

    /// Every warhead in flight `viewer` may know of, in id order.
    pub fn known_warheads(&self, viewer: PlayerId) -> Vec<KnownWarhead> {
        let last = self.tick.saturating_sub(1);
        let forecast = assign(self, self.tick, Scope::Team(viewer));
        let vision = self.vision(viewer);
        self.warheads
            .iter()
            .enumerate()
            .filter_map(|(index, w)| {
                let position = w.position(last);
                let line = WarheadLine {
                    launch: w.launch,
                    impact: w.impact,
                    arrival: w.arrival,
                };
                let interceptor = forecast
                    .iter()
                    .find(|engagement| engagement.warhead == index)
                    .map(|engagement| self.buildings[engagement.battery].id);
                if !self.hostile(viewer, w.player) {
                    return Some(KnownWarhead {
                        id: w.id,
                        player: Some(w.player),
                        hostile: false,
                        position,
                        line: Some(line),
                        exposure: None,
                        interceptor,
                    });
                }
                let track = vision.warhead(w.id)?;
                if !track.in_view && !track.tracked() {
                    return None;
                }
                Some(KnownWarhead {
                    id: w.id,
                    player: self
                        .can_see(viewer, TilePos::containing(position))
                        .then_some(w.player),
                    hostile: true,
                    position,
                    line: track.tracked().then_some(line),
                    exposure: Some(track.exposure),
                    interceptor,
                })
            })
            .collect()
    }
}

/// A stop the batteries will make on the rounds loaded now, as the
/// simulation resolves it unless the world changes first: a battery falls,
/// a warhead launched later reaches a ring sooner, or the match ends.
/// Omniscient, for presentation that draws it only where its viewer sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterceptionForecast {
    /// The warhead to be stopped.
    pub warhead: WarheadId,
    /// The battery that stops it.
    pub battery: BuildingId,
    /// The pod that fires: the battery's loaded count after the shot.
    pub pod: u8,
    /// The tick the stop resolves on.
    pub tick: Tick,
    /// Where the warhead stands then.
    pub at: Vec2Fx,
}

impl State {
    /// Every stop the batteries will make on the rounds loaded now, for the
    /// warheads their teams have tracked, in assignment order.
    pub fn interception_forecast(&self) -> Vec<InterceptionForecast> {
        assign(self, self.tick, Scope::Every)
            .into_iter()
            .map(|engagement| InterceptionForecast {
                warhead: self.warheads[engagement.warhead].id,
                battery: self.buildings[engagement.battery].id,
                pod: engagement.pod,
                tick: engagement.tick,
                at: engagement.at,
            })
            .collect()
    }
}

/// Which batteries an assignment matches, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scope {
    /// The stops interception makes this tick.
    Now,
    /// The viewer team's batteries, as each pair will stand on its ring
    /// entry or now, whichever is later.
    Team(PlayerId),
    /// Every battery, likewise.
    Every,
}

/// One battery spending one interceptor on one warhead.
pub(crate) struct Engagement {
    /// Index into the state's warheads.
    pub(crate) warhead: usize,
    /// Index into the state's buildings.
    pub(crate) battery: usize,
    /// Where the warhead is stopped.
    pub(crate) at: Vec2Fx,
    /// The tick it is stopped on.
    pub(crate) tick: Tick,
    /// The pod that fires: the loaded count left after the shot.
    pub(crate) pod: u8,
}

/// Loaded batteries matched greedily to hostile warheads whose impact lies
/// in their ring, one interceptor per warhead: earliest stop first, then
/// earliest ring entry. A battery stops a warhead on its ring entry or the
/// tick its team locks on, whichever is later.
///
/// Interception ([`Scope::Now`]) takes only locked-on warheads already
/// inside the ring. A forecast also takes each tracked warhead still in
/// view, as the tick its lock would complete, and any warhead before its
/// ring entry.
///
/// A flight ends inside a convex disc, so a warhead that enters stays
/// inside until it lands. Every key after the entry tick measures the pair
/// itself, so a half-turned world resolves the half-turned way; list order
/// breaks only exact coincidences.
pub(crate) fn assign(state: &State, now: Tick, scope: Scope) -> Vec<Engagement> {
    let batteries: Vec<(usize, &Building, Vec2Fx, Fx)> = state
        .buildings
        .iter()
        .enumerate()
        .filter_map(|(index, b)| {
            let ring = b.kind.battery()?.ring;
            (b.built()
                && b.magazine.is_some_and(|m| m.loaded > 0)
                && match scope {
                    Scope::Team(viewer) => !state.hostile(viewer, b.player),
                    Scope::Now | Scope::Every => true,
                })
            .then(|| (index, b, b.center(), ring * ring))
        })
        .collect();
    let mut pairs = Vec::new();
    for (w, warhead) in state.warheads.iter().enumerate() {
        for (slot, &(_, b, centre, ring_sq)) in batteries.iter().enumerate() {
            let inside = |p: Vec2Fx| p.dist_sq(centre) <= ring_sq;
            if !state.hostile(warhead.player, b.player) || !inside(warhead.impact) {
                continue;
            }
            let Some(track) = state.vision(b.player).warhead(warhead.id) else {
                continue;
            };
            // The tick the battery's team locks on: now once locked; ahead,
            // only as a forecast of a tracked warhead still in view.
            let locked = if track.locked() {
                now
            } else if scope != Scope::Now && track.tracked() && track.in_view {
                now + Tick::from(track.lock_remaining())
            } else {
                continue;
            };
            let entry = entry_tick(warhead, inside);
            let fire = entry.max(locked);
            if (scope == Scope::Now && entry > now) || fire > warhead.arrival {
                continue;
            }
            let at = warhead.position(fire);
            let path = warhead.impact - warhead.launch;
            let offset = centre - at;
            let side = (path.x * offset.y - path.y * offset.x).signum();
            pairs.push((
                (
                    fire,
                    entry,
                    warhead.arrival,
                    at.dist_sq(centre),
                    warhead.impact.dist_sq(centre),
                    side,
                    w,
                    slot,
                ),
                at,
            ));
        }
    }
    pairs.sort_unstable_by_key(|(key, _)| *key);
    let mut rounds: Vec<u8> = batteries
        .iter()
        .map(|(_, b, _, _)| b.magazine.map_or(0, |m| m.loaded))
        .collect();
    let mut stopped = vec![false; state.warheads.len()];
    let mut engagements = Vec::new();
    for ((fire, _, _, _, _, _, w, slot), at) in pairs {
        if stopped[w] || rounds[slot] == 0 {
            continue;
        }
        stopped[w] = true;
        rounds[slot] -= 1;
        engagements.push(Engagement {
            warhead: w,
            battery: batteries[slot].0,
            at,
            tick: fire,
            pod: rounds[slot],
        });
    }
    engagements
}

/// The first tick of the flight standing inside the ring. The impact lies
/// inside, so inside-ness only ever turns on along the flight.
fn entry_tick(warhead: &Warhead, inside: impl Fn(Vec2Fx) -> bool) -> Tick {
    let (mut lo, mut hi) = (warhead.launched_at, warhead.arrival);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if inside(warhead.position(mid)) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    hi
}
