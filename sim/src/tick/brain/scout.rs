//! The Scout order: see a tile with the owner's knowledge alone.
//!
//! A scout goes to its tile while the way and the tile are safe from every
//! threat its team knows of. Otherwise it walks to the safe tile nearest the
//! tile that still sees it. When its own ground turns dangerous it backs away,
//! and when no safe view exists it waits. Each tick spent that way is a tick of
//! caution; past [`SCOUT_HOLD_TICKS`] of them the scout gives the leg up.

use super::super::movement::half_turn_reversed;
use super::super::reach::Reach;
use super::super::spatial::UnitIndex;
use super::economy::{first_flagged_waypoint, keep_flagged_route, known_ground_passable};
use crate::event::{Event, StallReason};
use crate::ids::{PlayerId, UnitId};
use crate::state::{PathFollow, State};
use crate::stats::{
    DANGER_HOLD_REPORT_PERIOD, HARVEST_DANGER_RETRY_TICKS, SCOUT_HOLD_TICKS, SCOUT_RETREAT_RADIUS,
    SCOUT_ROUTE_ATTEMPTS,
};
use crate::vision::GroundSalvageDanger;
use chassis::grid::TilePos;

/// Runs one tick of a Scout leg toward `target`.
pub(super) fn scout(
    state: &mut State,
    index: &UnitIndex,
    reach: &mut Reach,
    danger: &GroundSalvageDanger,
    id: UnitId,
    target: TilePos,
    events: &mut Vec<Event>,
) {
    let unit = state.unit(id).expect("caller checked");
    let (tile, player, heading) = (unit.tile(), unit.player, unit.heading);
    let vision = unit.kind.stats().vision;
    let endangered = danger.scout_contains(tile);
    // A crowd of scouts sent to one tile cannot all stand on it; one that
    // sees it and touches the crowd already there has arrived with them.
    // Danger outranks arrival: a scout that reaches its tile as a threat
    // closes in backs off rather than idling under fire.
    if !endangered
        && (tile == target
            || (sees(tile, target, vision) && reach.crowd_touches(state, index, id, target)))
    {
        finish(state, id, None, events);
        return;
    }
    let eligible = |at: TilePos| {
        known_ground_passable(state, danger, player, at) && !danger.scout_contains(at)
    };
    let step = |at: TilePos| {
        known_ground_passable(state, danger, player, at) && danger.scout_route_safe_from(tile, at)
    };
    let path = unit.path.as_ref();
    let flagged = path.is_some_and(|path| first_flagged_waypoint(path, step).is_some());
    // A retreat runs to safe ground wherever it is; otherwise the walk must
    // still end somewhere that sees the target.
    let keep = path.is_some_and(|path| {
        eligible(path.goal)
            && (endangered || sees(path.goal, target, vision))
            && keep_flagged_route(state, id, path, step)
    });
    let cautious = endangered || flagged;
    if keep {
        if cautious {
            caution(state, id, events);
        }
        return;
    }
    let had_path = path.is_some();
    let at_vantage = !endangered && sees(tile, target, vision) && eligible(tile);
    let progress = unit.progress;
    let due = had_path
        || at_vantage
        || progress == 0
        || u64::from(progress).is_multiple_of(HARVEST_DANGER_RETRY_TICKS);
    if due {
        let planned = plan(
            state,
            danger,
            Origin {
                player,
                tile,
                heading,
                endangered,
            },
            target,
            vision,
        );
        match planned {
            Some(Route::Here) => {
                finish(state, id, Some(StallReason::SafeVantage), events);
                return;
            }
            Some(Route::Path(path)) => {
                state.unit_mut(id).expect("caller checked").path = Some(path);
                if cautious {
                    caution(state, id, events);
                }
                return;
            }
            None => state.unit_mut(id).expect("caller checked").path = None,
        }
    }
    hold(state, id, had_path, events);
}

/// Where a scout plans from.
#[derive(Clone, Copy)]
struct Origin {
    player: PlayerId,
    tile: TilePos,
    heading: u8,
    /// Whether known danger covers `tile`.
    endangered: bool,
}

/// Where a plan sends the scout.
enum Route {
    /// It already stands on the best place it can safely reach.
    Here,
    /// A safe walk there.
    Path(PathFollow),
}

/// The safe walk to the best place that sees `target`: the target itself
/// when it is safe, else the safe tile nearest it that still sees it. A
/// scout standing in danger first backs away to the nearest safe ground it
/// can reach without closing on any threat, and plans its view from there.
fn plan(
    state: &State,
    danger: &GroundSalvageDanger,
    origin: Origin,
    target: TilePos,
    vision: i32,
) -> Option<Route> {
    let Origin {
        player,
        tile: from,
        heading,
        endangered,
    } = origin;
    let eligible = |at: TilePos| {
        known_ground_passable(state, danger, player, at) && !danger.scout_contains(at)
    };
    if endangered {
        let reverse = half_turn_reversed(state, from, heading);
        let refuges = super::super::goals::ring_scan(from, SCOUT_RETREAT_RADIUS, reverse)
            .filter(|&at| eligible(at));
        return route_first(state, danger, player, from, refuges);
    }
    let frame = crate::tick::rect_approach_origin(state, player, from, target, (1, 1));
    let mut views: Vec<TilePos> = (-vision..=vision)
        .flat_map(|dy| (-vision..=vision).map(move |dx| target.offset(dx, dy)))
        .filter(|&at| sees(at, target, vision) && eligible(at))
        .collect();
    views.sort_by_key(|&at| {
        (
            distance_sq(at, target),
            crate::geometry::rect_approach_key_from(from, frame, target, (1, 1), at),
        )
    });
    // Standing on a view already, the scout settles there when the better
    // ones before it cannot be searched out.
    let standing = views.contains(&from);
    let found = route_first(state, danger, player, from, views);
    if standing {
        return found.or(Some(Route::Here));
    }
    found
}

/// The first of `candidates` a safe route reaches from `from`, in their
/// order. A failed search that explored all of `from`'s safe ground settles
/// every later candidate without another search; searches that hit the
/// expansion cap are allowed only [`SCOUT_ROUTE_ATTEMPTS`] times.
fn route_first(
    state: &State,
    danger: &GroundSalvageDanger,
    player: PlayerId,
    from: TilePos,
    candidates: impl IntoIterator<Item = TilePos>,
) -> Option<Route> {
    let step = |at: TilePos| {
        known_ground_passable(state, danger, player, at) && danger.scout_route_safe_from(from, at)
    };
    let mut capped = 0;
    let mut explored = false;
    for candidate in candidates {
        if candidate == from {
            return Some(Route::Here);
        }
        // Only the search that just failed is on record, and every search
        // here shares one rule, so its reach answers for every candidate.
        if explored
            && danger
                .last_route_reachability(&[candidate], false)
                .is_some_and(|reachable| !reachable[0])
        {
            continue;
        }
        if let Some(waypoints) = danger.find_route(from, candidate, step) {
            return Some(Route::Path(PathFollow {
                final_point: None,
                goal: candidate,
                waypoints,
                next: 0,
            }));
        }
        explored = danger.last_route_exhausted();
        if !explored {
            capped += 1;
            if capped >= SCOUT_ROUTE_ATTEMPTS {
                return None;
            }
        }
    }
    None
}

/// Stands with no safe way on, reporting the wait as it starts and then
/// periodically, and spends a tick of caution.
fn hold(state: &mut State, id: UnitId, began: bool, events: &mut Vec<Event>) {
    if caution(state, id, events) {
        return;
    }
    let unit = state.unit(id).expect("caller checked");
    if !unit.looping && (began || u64::from(unit.progress) % DANGER_HOLD_REPORT_PERIOD == 1) {
        events.push(Event::OrderStalled {
            unit: id,
            player: unit.player,
            pos: unit.pos,
            reason: StallReason::DangerHold,
        });
    }
}

/// Spends one tick of the leg's caution, giving the leg up once it runs
/// out. Returns whether it did.
fn caution(state: &mut State, id: UnitId, events: &mut Vec<Event>) -> bool {
    let unit = state.unit_mut(id).expect("caller checked");
    unit.progress = unit.progress.saturating_add(1);
    if unit.progress < SCOUT_HOLD_TICKS {
        return false;
    }
    finish(state, id, Some(StallReason::NoSafeVantage), events);
    true
}

/// Ends the leg, reporting `reason` unless the program loops: a patrol
/// lap passes silently.
fn finish(state: &mut State, id: UnitId, reason: Option<StallReason>, events: &mut Vec<Event>) {
    let unit = state.unit_mut(id).expect("caller checked");
    let (looping, player, pos) = (unit.looping, unit.player, unit.pos);
    unit.advance_queue();
    if let Some(reason) = reason
        && !looping
    {
        events.push(Event::OrderStalled {
            unit: id,
            player,
            pos,
            reason,
        });
    }
}

/// Whether a unit standing on `from` sees `target`: vision is a disc of
/// tile centers that rocks never block.
fn sees(from: TilePos, target: TilePos, vision: i32) -> bool {
    distance_sq(from, target) <= vision * vision
}

fn distance_sq(a: TilePos, b: TilePos) -> i32 {
    let (dx, dy) = (a.x - b.x, a.y - b.y);
    dx * dx + dy * dy
}
