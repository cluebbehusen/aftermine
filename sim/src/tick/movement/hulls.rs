//! Structural hulls in the way of their own side.
//!
//! A machine built like a structure, the Maw, gives no ground to a smaller
//! body (see `push_shares`), and routes ignore units, so a route can run
//! straight under one that stands still and jam there. A small friendly body
//! routes round such a hull when there is room close beside it. Where there
//! is none, a hull parked with no orders drives out of the way itself.

use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;

use crate::state::{Order, PathFollow, State, Unit};
use crate::stats::{COLLISION_MAX_STEP, Domain, SMALL_BODY_RADIUS, hull_excess};

/// The tiles under a hull that a small body cannot stand on without
/// touching it: every tile whose centre lies within the hull's radius plus
/// a small body's, along each axis.
#[derive(Clone, Copy)]
struct Footprint {
    low: TilePos,
    high: TilePos,
}

impl Footprint {
    fn at(pos: Vec2Fx, radius: Fx) -> Self {
        let reach = radius + SMALL_BODY_RADIUS;
        let half = const { Fx::lit("0.5") };
        let span = |c: Fx| {
            (
                (c - reach - half).floor().to_num::<i32>() + 1,
                (c + reach - half).ceil().to_num::<i32>() - 1,
            )
        };
        let ((x0, x1), (y0, y1)) = (span(pos.x), span(pos.y));
        Self {
            low: TilePos::new(x0, y0),
            high: TilePos::new(x1, y1),
        }
    }

    fn of(hull: &Unit) -> Self {
        Self::at(hull.pos, hull.kind.stats().radius)
    }

    fn tiles(self) -> impl Iterator<Item = TilePos> {
        (self.low.y..=self.high.y)
            .flat_map(move |y| (self.low.x..=self.high.x).map(move |x| TilePos::new(x, y)))
    }

    fn covers(self, tile: TilePos) -> bool {
        (self.low.x..=self.high.x).contains(&tile.x) && (self.low.y..=self.high.y).contains(&tile.y)
    }

    /// The footprint's extent in tiles along its wider axis.
    fn width(self) -> i32 {
        (self.high.x - self.low.x).max(self.high.y - self.low.y) + 1
    }

    fn grown(self, by: i32) -> Self {
        Self {
            low: self.low.offset(-by, -by),
            high: self.high.offset(by, by),
        }
    }

    fn area(self) -> u32 {
        let side = |low: i32, high: i32| u32::try_from(high - low + 1).unwrap_or(0);
        side(self.low.x, self.high.x) * side(self.low.y, self.high.y)
    }
}

/// Whether `hull` is a structural body standing still that `mover` must go
/// round. Only a friendly hull counts: steering round an enemy before
/// contact would leak where it is.
fn stands_in_the_way(state: &State, mover: &Unit, hull: &Unit) -> bool {
    hull.id != mover.id
        && hull.hp > 0
        && hull.domain() == Domain::Ground
        && hull.kind.stats().structural
        && hull.path.is_none()
        && hull.drive_speed() == Fx::ZERO
        && !state.hostile(mover.player, hull.player)
}

/// `waypoints`, a route for `mover` from `from`, with each stretch that runs
/// under a friendly structural hull standing still replaced by a way round
/// it, when one exists within the hull's own width of it. A stretch with no
/// such way stays as it was. Only a small ground body reroutes; a hull as
/// wide shoves the standing one instead.
pub(in crate::tick) fn round_standing_hulls(
    state: &State,
    mover: &Unit,
    from: Vec2Fx,
    mut waypoints: Vec<TilePos>,
) -> Vec<TilePos> {
    if mover.domain() != Domain::Ground || hull_excess(mover.kind.stats().radius) > Fx::ZERO {
        return waypoints;
    }
    let hulls: Vec<Footprint> = state
        .units
        .iter()
        .filter(|hull| stands_in_the_way(state, mover, hull))
        .map(Footprint::of)
        .collect();
    if hulls.is_empty() {
        return waypoints;
    }
    let mut scan = 0;
    while let Some(first) = waypoints
        .get(scan..)
        .and_then(|rest| rest.iter().position(|&t| hulls.iter().any(|h| h.covers(t))))
        .map(|offset| scan + offset)
    {
        let hull = *hulls
            .iter()
            .find(|h| h.covers(waypoints[first]))
            .expect("a hull covers the waypoint just found");
        let last = waypoints
            .iter()
            .rposition(|&t| hull.covers(t))
            .expect("the first covered waypoint is covered");
        // A route ending under the hull keeps its way in.
        let Some(&exit) = waypoints.get(last + 1) else {
            break;
        };
        let entry = match first.checked_sub(1) {
            Some(before) => waypoints[before],
            None => TilePos::containing(from),
        };
        let window = hull.grown(hull.width());
        let detour = chassis::path::astar(
            state.map.width(),
            state.map.height(),
            entry,
            exit,
            |t| window.covers(t) && !hulls.iter().any(|h| h.covers(t)) && state.passable(t),
            window.area(),
        );
        match detour {
            Some(detour) => {
                let length = detour.len();
                waypoints.splice(first..=last + 1, detour);
                scan = first + length;
            }
            None => scan = last + 1,
        }
    }
    waypoints
}

/// Movement pre-pass: a structural hull parked with no orders pulls aside
/// for its own side. A friendly ground body pressing on it, with a route
/// that runs under it and no way round close beside it, or that ends under
/// it, sends the hull to the nearest ground ahead along that route where it
/// leaves every such route clear. Sets `path` only, like
/// [`evict_claimed_ground`](super::evict_claimed_ground), which runs
/// first: the hull stops there with its orders unchanged. Runs in id order,
/// so a hull already sent aside no longer stands in a later one's check.
pub(in crate::tick) fn make_way(state: &mut State) {
    for slot in 0..state.units.len() {
        if let Some(path) = way_aside(state, slot) {
            state.units[slot].path = Some(path);
        }
    }
}

fn way_aside(state: &State, slot: usize) -> Option<PathFollow> {
    let hull = &state.units[slot];
    let radius = hull.kind.stats().radius;
    if hull.hp == 0
        || hull.domain() != Domain::Ground
        || !hull.kind.stats().structural
        || !matches!(hull.order, Order::Idle)
        || hull.path.is_some()
        || hull.drive_speed() != Fx::ZERO
    {
        return None;
    }
    let footprint = Footprint::of(hull);
    // The way to clear follows the first of the hull's own seat's bodies,
    // in seat order, and an ally's only when none of its own presses.
    let mut routes = Vec::new();
    let mut lead: Option<(bool, Vec2Fx, &[TilePos])> = None;
    for mover in &state.units {
        let Some(rest) = blocked_route(state, hull, footprint, mover) else {
            continue;
        };
        routes.extend_from_slice(rest);
        let own = mover.player == hull.player;
        if lead.is_none_or(|(lead_own, _, _)| own && !lead_own) {
            lead = Some((own, mover.pos, rest));
        }
    }
    let (_, presser, lead) = lead?;
    routes.sort_unstable();
    routes.dedup();
    let past = lead.iter().rposition(|&t| footprint.covers(t))? + 1;
    // Ahead is where the route leaves the hull or, for a route that ends
    // under it, onward the way the body presses.
    let (ahead, onward) = match lead.get(past) {
        Some(next) => (next.center() - hull.pos, &lead[past..]),
        None => (lead[past - 1].center() - presser, &lead[past - 1..]),
    };
    let from = hull.tile();
    let direction = if super::half_turn_reversed(state, from, hull.heading) {
        -1
    } else {
        1
    };
    // Room for the hull lies within its own width of the route.
    let reach = footprint.width();
    for &waypoint in onward {
        for r in 0..=reach {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) != r {
                        continue;
                    }
                    let goal = waypoint.offset(dx * direction, dy * direction);
                    let offset = goal.center() - hull.pos;
                    if goal == from
                        || offset.x * ahead.x + offset.y * ahead.y <= Fx::ZERO
                        || !crate::geometry::hull_open(goal, radius, |t| state.passable(t))
                    {
                        continue;
                    }
                    if Footprint::at(goal.center(), radius)
                        .tiles()
                        .any(|t| routes.binary_search(&t).is_ok())
                    {
                        continue;
                    }
                    if let Some(waypoints) = crate::tick::route_for(state, hull.kind, from, goal) {
                        return Some(PathFollow {
                            final_point: None,
                            goal,
                            waypoints,
                            next: 0,
                            clearing: false,
                        });
                    }
                }
            }
        }
    }
    None
}

/// The rest of `mover`'s route, when the friendly body presses on `hull`
/// with that route ending under it, or running under it with no way round
/// close beside it.
fn blocked_route<'a>(
    state: &State,
    hull: &Unit,
    footprint: Footprint,
    mover: &'a Unit,
) -> Option<&'a [TilePos]> {
    if mover.id == hull.id
        || mover.hp == 0
        || mover.domain() != Domain::Ground
        || state.hostile(hull.player, mover.player)
    {
        return None;
    }
    let touch = hull.kind.stats().radius + mover.kind.stats().radius + COLLISION_MAX_STEP;
    if mover.pos.dist_sq(hull.pos) > touch * touch {
        return None;
    }
    let path = mover.path.as_ref()?;
    let rest = path.waypoints.get(path.next as usize..)?;
    if !rest.iter().any(|&t| footprint.covers(t)) {
        return None;
    }
    round_standing_hulls(state, mover, mover.pos, rest.to_vec())
        .iter()
        .any(|&t| footprint.covers(t))
        .then_some(rest)
}
