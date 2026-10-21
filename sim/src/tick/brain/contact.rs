//! Bounded final approaches shared by building work and contact weapons.
use super::locomotion::approach_rect;
use super::*;
use crate::geometry::rect_approach_key_from;
use crate::stats::Domain;
use crate::tick::crowding;
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::{TilePos, as_index, cell_count};

pub(super) fn approach(state: &mut State, id: UnitId, building: BuildingId) -> bool {
    approach_building(state, id, building)
}

fn approach_building(state: &mut State, id: UnitId, building: BuildingId) -> bool {
    let b = state.building(building).expect("live target");
    let (anchor, size) = (b.anchor, b.kind.size());
    let unit = state.unit(id).expect("contact unit");
    let (from, pos, player) = (unit.tile(), unit.pos, unit.player);
    if pos.dist_sq(crate::geometry::footprint_contact(pos, anchor, size))
        > const { Fx::lit("6.25") }
    {
        return approach_rect(state, id, anchor, size);
    }
    let pads = crew_pad_entries(state, unit, b);
    // A held position stays only while it is still free and admissible:
    // ground claimed beside it since it was chosen can leave the motor
    // unable to land there. A crew holds only a pad.
    if unit.path.as_ref().is_some_and(|path| {
        path.final_point.is_some_and(|point| {
            !crowding::claimed(state, id, point, true)
                && admissible(state, unit, b, path.goal, point)
                && pads.as_ref().is_none_or(|pads| {
                    pads.iter().any(|&entry| {
                        crate::geometry::work_tile(entry, pos, b.center()) == path.goal
                    })
                })
        })
    }) {
        return true;
    }
    let frame = crate::tick::rect_approach_origin(state, player, from, anchor, size);
    let choose = |mut candidates: Vec<crowding::Position>| {
        candidates.sort_by_key(|candidate| {
            (
                pos.dist_sq(candidate.point),
                rect_approach_key_from(from, frame, anchor, size, candidate.goal),
                (candidate.point.x - pos.x) * (frame.center().y - pos.y)
                    - (candidate.point.y - pos.y) * (frame.center().x - pos.x),
            )
        });
        crowding::choose(
            state,
            id,
            candidates,
            anchor.center()
                + Vec2Fx::new(Fx::from_num(size.0 - 1), Fx::from_num(size.1 - 1)) / Fx::from_num(2),
            |tile| state.passable(tile),
            |goal| route(state, unit, from.center(), goal),
        )
    };
    let path = choose(ordinary_stances(state, unit, b, pads));
    let reachable = path.is_some();
    state.unit_mut(id).expect("contact unit").path = path;
    reachable
}

/// The positions from which `unit` may meet `b`: its crew pads, or work
/// positions around the footprint.
fn ordinary_stances(
    state: &State,
    unit: &crate::Unit,
    b: &crate::Building,
    pads: Option<Vec<Vec2Fx>>,
) -> Vec<crowding::Position> {
    let surface = state.contact_surface(b);
    pads.unwrap_or_else(|| {
        crate::geometry::work_positions(
            b.anchor,
            b.kind.size(),
            clearance(unit),
            unit.kind.stats().radius * crate::stats::SAME_OWNER_COMPRESSION,
        )
    })
    .into_iter()
    .filter_map(|entry| {
        let point = surface.stance(entry, clearance(unit));
        let goal = crate::geometry::work_tile(entry, unit.pos, b.center());
        (state.passable(goal) && admissible(state, unit, b, goal, point))
            .then_some(crowding::Position { goal, point })
    })
    .collect()
}

/// The tiles a contact unit could set out for to meet `b`, before any
/// route or crowd is weighed.
pub(super) fn stance_goals(state: &State, unit: &crate::Unit, b: &crate::Building) -> Vec<TilePos> {
    let pads = crew_pad_entries(state, unit, b);
    ordinary_stances(state, unit, b, pads)
        .into_iter()
        .map(|position| position.goal)
        .collect()
}

/// Where a crew member raising an assembly cradle stands to work: off the
/// footprint at each of its crew pads.
fn crew_pad_entries(
    state: &State,
    unit: &crate::Unit,
    site: &crate::Building,
) -> Option<Vec<Vec2Fx>> {
    let pads = state.crew_pads_for(unit, site)?;
    let gap = clearance(unit);
    Some(
        pads.iter()
            .map(|pad| {
                let x = Fx::from_num(pad.x) + Fx::lit("0.5");
                let y = if pad.y < site.anchor.y {
                    Fx::from_num(site.anchor.y) - gap
                } else {
                    Fx::from_num(pad.y) + gap
                };
                Vec2Fx::new(x, y)
            })
            .collect(),
    )
}

/// Whether `kind` cuts with a drum across the front of a hull broader than
/// a small body, as the Maw does: it cuts a building only with the drum
/// pressed flat against one of its sides.
pub(in crate::tick) fn square_cutter(kind: crate::UnitKind) -> bool {
    kind.stats().contact_reach.is_some()
        && crate::stats::hull_excess(kind.stats().radius) > Fx::ZERO
}

/// Half the width of a wide cutter's drum, end plate to end plate, as the
/// Maw's art draws it.
const DRUM_HALF_WIDTH: Fx = Fx::lit("0.95");

/// How far ahead of the drum's edge a side counts as under it: the contact
/// reach, and a little slack for a hull that settles just short.
const DRUM_SLACK: Fx = Fx::lit("0.03");

/// The cosine of the largest angle between a drum and a side it presses
/// flat against: about eight degrees.
const FLAT: Fx = Fx::lit("0.99");

/// How near a hostile building's outline must come to a wide cutter's drum
/// to jam its guns.
const JAM_REACH: Fx = Fx::lit("0.4");

/// The edge of a wide cutter's drum, its hull at `pos` facing `forward`:
/// it runs across the hull's front, square to its heading, where the hull's
/// collision edge meets it.
pub(in crate::tick) fn drum_edge(
    kind: crate::UnitKind,
    pos: Vec2Fx,
    forward: Vec2Fx,
) -> (Vec2Fx, Vec2Fx) {
    let middle = pos + forward * kind.stats().radius;
    let across = Vec2Fx::new(-forward.y, forward.x) * DRUM_HALF_WIDTH;
    (middle - across, middle + across)
}

/// Where `unit`'s drum presses flat against a side of `b`, with that
/// side's outward normal. `None` for a unit that is not a wide cutter, or a
/// drum askew to every side it reaches.
pub(in crate::tick) fn drum_contact(
    state: &State,
    unit: &crate::Unit,
    b: &crate::Building,
) -> Option<(Vec2Fx, Vec2Fx)> {
    if !square_cutter(unit.kind) {
        return None;
    }
    let forward = chassis::compass::dir(unit.heading);
    let reach = unit.kind.stats().contact_reach? + DRUM_SLACK;
    state.contact_surface(b).flat_contact(
        drum_edge(unit.kind, unit.pos, forward),
        forward,
        reach,
        FLAT,
    )
}

/// Whether `unit`'s drum comes near enough `b`'s outline, at any angle, to
/// jam its guns.
pub(in crate::tick) fn jams(state: &State, unit: &crate::Unit, b: &crate::Building) -> bool {
    let span = drum_span(unit.kind);
    square_cutter(unit.kind) && unit.pos.dist_sq(b.closest_point_to(unit.pos)) <= span * span && {
        let (from, to) = drum_edge(unit.kind, unit.pos, chassis::compass::dir(unit.heading));
        state.contact_surface(b).meets(from, to, JAM_REACH)
    }
}

/// Whether a wide cutter of `player`'s cuts `b` when its drum presses on
/// it: any standing hostile building but a buried charge, which lies under
/// the drum, not in its path.
pub(in crate::tick) fn cuts(state: &State, player: crate::PlayerId, b: &crate::Building) -> bool {
    b.hp > 0 && !b.provisional() && !b.kind.is_stealthy() && state.hostile(player, b.player)
}

/// A bound on how far from a wide cutter's position its drum reaches a
/// footprint.
pub(in crate::tick) fn drum_span(kind: crate::UnitKind) -> Fx {
    kind.stats().radius + DRUM_HALF_WIDTH + Fx::ONE
}

/// How near a hostile building in a wide cutter's way must lie for it to
/// turn to cut that building first. One farther along is left until the
/// route, which may yet change, comes nearer.
const PLOUGH_REACH: Fx = Fx::lit("20");

/// A route for `unit` from `from` to `goal` over open ground. A wide
/// cutter on the attack, hunting or attacking, that finds none plans
/// through the hostile buildings its owner sees, which its drum cuts out of
/// its way.
pub(in crate::tick) fn route(
    state: &State,
    unit: &crate::Unit,
    from: Vec2Fx,
    goal: TilePos,
) -> Option<Vec<TilePos>> {
    crate::tick::route_for_position(state, unit.kind, from, goal).or_else(|| {
        (square_cutter(unit.kind)
            && matches!(unit.order, Order::Hunt { .. } | Order::Attack { .. }))
        .then(|| plough_route(state, unit, TilePos::containing(from), goal))
        .flatten()
    })
}

/// A wide cutter's route over open ground and through the buildings it
/// ploughs. Like any route it keeps clear of buildings its owner does not
/// see.
fn plough_route(
    state: &State,
    unit: &crate::Unit,
    from: TilePos,
    goal: TilePos,
) -> Option<Vec<TilePos>> {
    let (width, height) = (state.map.width(), state.map.height());
    let cell = |tile: TilePos| {
        ((0..width).contains(&tile.x) && (0..height).contains(&tile.y))
            .then(|| as_index(tile.y) * as_index(width) + as_index(tile.x))
    };
    let mut ploughed = vec![false; cell_count(width, height)];
    for b in state
        .buildings()
        .iter()
        .filter(|b| ploughs(state, unit.player, b))
    {
        for i in b.tiles().filter_map(cell) {
            ploughed[i] = true;
        }
    }
    let open = |tile: TilePos| {
        state.passable(tile)
            || (state.map.terrain_passable(tile) && cell(tile).is_some_and(|i| ploughed[i]))
    };
    let radius = unit.kind.stats().radius;
    chassis::path::astar(
        width,
        height,
        from,
        goal,
        |tile| crate::geometry::hull_open(tile, radius, open),
        crate::stats::PATH_EXPANSION_CAP,
    )
}

/// Whether a wide cutter of `player`'s ploughs through `b`: a building it
/// cuts that its owner sees.
pub(in crate::tick) fn ploughs(
    state: &State,
    player: crate::PlayerId,
    b: &crate::Building,
) -> bool {
    cuts(state, player, b)
        && b.tiles().any(|tile| state.can_see(player, tile))
        && state.building_apparent(player, b)
}

/// The building a wide cutter turns to cut first on its way to `goal`: of
/// the hostile buildings within [`PLOUGH_REACH`] that its route runs
/// through, those its hull would cover at the first step blocked by any,
/// the nearest.
pub(in crate::tick) fn plough_blocker(
    state: &State,
    unit: &crate::Unit,
    goal: TilePos,
) -> Option<BuildingId> {
    let blockers: Vec<&crate::Building> = state
        .buildings()
        .iter()
        .filter(|b| {
            unit.pos.dist_sq(b.closest_point_to(unit.pos)) <= PLOUGH_REACH * PLOUGH_REACH
                && ploughs(state, unit.player, b)
        })
        .collect();
    if blockers.is_empty() {
        return None;
    }
    let radius = unit.kind.stats().radius;
    plough_route(state, unit, unit.tile(), goal)?
        .into_iter()
        .find_map(|step| {
            let hull: Vec<TilePos> = crate::geometry::body_tiles(step.center(), radius).collect();
            blockers
                .iter()
                .filter(|b| b.tiles().any(|tile| hull.contains(&tile)))
                .min_by_key(|b| nearest(unit, b, b.closest_point_to(unit.pos)))
                .map(|b| b.id)
        })
}

/// Ranks `b`, met at `point`, among buildings a wide cutter might cut:
/// nearest first, then by side relative to its heading, so mirrored
/// cutters choose mirrored buildings.
pub(in crate::tick) fn nearest(
    unit: &crate::Unit,
    b: &crate::Building,
    point: Vec2Fx,
) -> (Fx, Fx, Fx, BuildingId) {
    let forward = chassis::compass::dir(unit.heading);
    let side = |at: Vec2Fx| forward.x * (at.y - unit.pos.y) - forward.y * (at.x - unit.pos.x);
    (unit.pos.dist_sq(point), side(point), side(b.center()), b.id)
}

pub(in crate::tick) fn clearance(unit: &crate::Unit) -> Fx {
    unit.kind.stats().contact_reach.map_or_else(
        || unit.kind.stats().radius + crate::stats::WORK_FOOTPRINT_GAP,
        |reach| unit.kind.stats().radius + reach - const { Fx::lit("0.05") },
    )
}

pub(in crate::tick) fn endpoint(
    state: &State,
    id: UnitId,
    building: BuildingId,
    entry: Vec2Fx,
) -> Option<Vec2Fx> {
    let unit = state.unit(id)?;
    let b = state.building(building)?;
    let point = state.contact_surface(b).stance(entry, clearance(unit));
    admissible(
        state,
        unit,
        b,
        crate::geometry::work_tile(entry, unit.pos, b.center()),
        point,
    )
    .then_some(point)
}

/// Whether a contact position can be driven to straight from its goal
/// tile's center, the way the motor finishes every final approach.
fn admissible(
    state: &State,
    unit: &crate::Unit,
    building: &crate::Building,
    goal: TilePos,
    point: Vec2Fx,
) -> bool {
    if point.dist_sq(goal.center()) > const { Fx::lit("2.25") } {
        return false;
    }
    let outlines = cut_outlines(state, unit, point);
    state
        .ground_terrain()
        .with_contact(Some(state.contact_surface(building)))
        .with_outlines(&outlines)
        .contact_clear(goal.center(), point, collision_radius(unit))
}

/// The outlines of the hostile buildings near `at` that a wide cutter
/// would cut. Pressing on a building, it meets these at their drawn shape,
/// as it meets its target, where their whole tiles would hold its hull off
/// a building set in a wall. Every other body meets them at whole tiles.
pub(in crate::tick) fn cut_outlines(
    state: &State,
    unit: &crate::Unit,
    at: Vec2Fx,
) -> Vec<crate::building_contact::Surface> {
    if !square_cutter(unit.kind) {
        return Vec::new();
    }
    // A stance lies within 1.5 tiles of the goal tile its admissibility
    // sweep starts from, so this reaches every building the hull meets on
    // the way.
    let span = collision_radius(unit) + const { Fx::lit("2") };
    state
        .buildings()
        .iter()
        .filter(|b| {
            cuts(state, unit.player, b) && at.dist_sq(b.closest_point_to(at)) <= span * span
        })
        .map(|b| state.contact_surface(b))
        .collect()
}

pub(in crate::tick) fn collision_radius(unit: &crate::Unit) -> Fx {
    unit.kind
        .stats()
        .radius
        .min(clearance(unit) - const { Fx::lit("0.002") })
}

pub(in crate::tick) fn surface_for(
    state: &State,
    unit: &crate::Unit,
) -> Option<crate::building_contact::Surface> {
    if unit.domain() != Domain::Ground {
        return None;
    }
    let target = match unit.order {
        Order::Attack { target, .. } if unit.kind.stats().contact_reach.is_some() => state
            .attack_view(unit.player, target)
            .and_then(|view| match view.entity {
                Some(Target::Building(id)) => Some(id),
                _ => None,
            }),
        Order::Build { site } => Some(site),
        Order::Repair { building }
        | Order::Salvage { building }
        | Order::Tend {
            weld: Some(Target::Building(building)),
            ..
        } => Some(building),
        Order::ReturnCargo { foundry, .. } => Some(foundry),
        Order::Harvest { .. } if unit.carrying() > 0 => {
            unit.unloading().map(|u| u.foundry).or_else(|| {
                state
                    .buildings()
                    .iter()
                    .find(|b| {
                        b.hp > 0
                            && b.player == unit.player
                            && b.kind.is_drop_off()
                            && unit.path.as_ref().is_some_and(|p| {
                                crate::tick::tile_adjacent_to_rect(p.goal, b.anchor, b.kind.size())
                            })
                    })
                    .map(|b| b.id)
            })
        }
        _ => None,
    };
    let b = target
        .and_then(|id| state.building(id))
        .filter(|b| {
            // Within a tile and a half of the footprint, from a wide hull's edge.
            let near =
                const { Fx::lit("1.5") } + crate::stats::hull_excess(unit.kind.stats().radius);
            b.hp > 0 && unit.pos.dist_sq(b.closest_point_to(unit.pos)) <= near * near
        })
        .or_else(|| {
            (!state.passable(unit.tile()))
                .then(|| {
                    state
                        .buildings_at(unit.tile())
                        .find(|b| !b.kind.is_stealthy() && !b.provisional())
                })
                .flatten()
        })?;
    Some(state.contact_surface(b))
}

/// A worker that already reaches the surface stops where it stands instead
/// of finishing the walk to its chosen position, so a position it cannot
/// land on never keeps it from working.
pub(super) fn approach_worker(state: &mut State, id: UnitId, building: BuildingId) -> bool {
    let unit = state.unit(id).expect("contact unit");
    if state.in_building_work_reach(unit, building) {
        state.unit_mut(id).expect("contact unit").path = None;
        return true;
    }
    approach_building(state, id, building)
}
