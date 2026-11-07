//! Picking and selection: click, box, double-click-kind, and the idle
//! harvester cycle. Any owner's visible units are selectable (allies for
//! reading, enemies for inspection), but a selection holds one owner:
//! picks and merges of a different owner replace it, since a mixed
//! own+ally selection would refuse every command under own-gating. Fog
//! and ownership rules for orders live in `orders`.

use crate::game::Game;
use crate::numeric;
use aftermine_sim::{BuildingId, PlayerId, UnitId};
use macroquad::prelude::{Vec2, vec2};

/// Own harvesters with nothing to do, in id order — the cycle key and
/// the HUD badge both read this.
pub fn idle_harvesters(game: &crate::game::Scene<'_>) -> Vec<UnitId> {
    game.state
        .units()
        .iter()
        .filter(|u| {
            u.player == game.presentation.human
                && u.kind.stats().harvest.is_some()
                && u.order == aftermine_sim::Order::Idle
        })
        .map(|u| u.id)
        .collect()
}

/// Whether a unit belongs to its owner's army: it fights, or it shields
/// the army from fire. Workers, the unarmed scouts and the unarmed
/// transport stay out, and a drone flies for its host alone.
pub(super) fn in_army(unit: &aftermine_sim::Unit) -> bool {
    let stats = unit.kind.stats();
    unit.host.is_none() && (stats.can_fight() || stats.point_defense.is_some())
}

/// The viewer's army units in id order: all of them, or with `in_view`
/// only those inside the camera's view, the same bounds a double-click
/// sweeps.
pub(crate) fn army_units(game: &crate::game::Scene<'_>, in_view: bool) -> Vec<UnitId> {
    let (lo, hi) = game.presentation.camera.world_rect();
    let mut ids: Vec<UnitId> = game
        .state
        .units()
        .iter()
        .filter(|u| u.player == game.presentation.human && in_army(u))
        .filter(|u| {
            let p = vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>());
            !in_view || (p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y)
        })
        .map(|u| u.id)
        .collect();
    ids.sort_unstable();
    ids
}

/// Replaces the selection with the viewer's army, or with the part of it
/// in view. With no such unit the selection stays and the press is
/// refused.
pub(super) fn select_army(game: &mut Game, input: &mut super::InputState, in_view: bool) {
    let ids = army_units(&game.view(), in_view);
    if ids.is_empty() {
        game.presentation
            .sounds_pending
            .push((crate::game::SoundKind::Denied, None));
        return;
    }
    input.close_construction();
    let selection = &mut game.presentation.selection;
    selection.units = ids;
    selection.buildings.clear();
    selection.pile = None;
}

/// Selects the next idle harvester after the current selection (id
/// order, wrapping) and centers the camera on it. Stateless: the
/// selection itself is the cursor.
pub(super) fn cycle_idle_worker(game: &mut Game, input: &mut super::InputState) {
    let idle = idle_harvesters(&game.view());
    let Some(&first) = idle.first() else {
        game.presentation
            .sounds_pending
            .push((crate::game::SoundKind::Denied, None));
        return;
    };
    let next = match game.presentation.selection.units.as_slice() {
        [current] => idle
            .iter()
            .copied()
            .find(|id| id > current)
            .unwrap_or(first),
        _ => first,
    };
    game.presentation.selection.units = vec![next];
    game.presentation.selection.buildings.clear();
    let unit = game.state.unit(next).expect("listed above");
    let at = vec2(unit.pos.x.to_num::<f32>(), unit.pos.y.to_num::<f32>());
    super::reticle::look_at(game, input, at);
}

/// How far around a pointer a press picks, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Reach {
    /// From a unit's center; never less than the unit's own radius.
    units: f32,
    /// From a structure's footprint; with none, the press must land on
    /// the footprint.
    structures: Option<f32>,
}

impl Reach {
    pub(super) const MOUSE: Self = Self {
        units: 10.0,
        structures: None,
    };
    /// Wider, for a fingertip.
    pub(super) const TOUCH: Self = Self {
        units: 22.0,
        structures: None,
    };

    /// As far around a unit as around a structure's footprint.
    pub(super) const fn around(px: f32) -> Self {
        Self {
            units: px,
            structures: Some(px),
        }
    }

    /// How far around a unit's center it reaches, in logical pixels at UI
    /// scale `ui`.
    pub(super) fn unit_px(self, ui: f32) -> f32 {
        self.units * ui
    }

    /// The same reach in world units at UI scale `ui` and camera `zoom`,
    /// for an order given at the pointer.
    pub(super) fn in_world(self, ui: f32, zoom: f32) -> super::orders::OrderReach {
        let world = |px: f32| px * ui / zoom;
        super::orders::OrderReach::new(world(self.units), world(self.structures.unwrap_or(0.0)))
    }
}

/// World-space pick radius around a unit: never below the unit's own
/// radius, and wider when zoomed out so it keeps a screen-space floor.
fn pick_radius(game: &Game, ui: f32, kind: aftermine_sim::UnitKind, reach: Reach) -> f32 {
    (reach.units * ui / game.presentation.camera.zoom).max(super::unit_pick_radius(kind))
}

/// What a press would select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Picked {
    Unit(UnitId, aftermine_sim::PlayerId),
    Building(aftermine_sim::BuildingId, aftermine_sim::PlayerId),
}

/// The entity a press at `screen` picks: the nearest selectable unit in
/// reach (own units outrank foreign ones, so a scrum never steals the
/// press from the machine you command), else a selectable building
/// under it.
pub(super) fn pick(game: &Game, screen: Vec2, ui: f32, reach: Reach) -> Option<Picked> {
    let world = game.presentation.camera.to_world(screen);
    let tile = numeric::tile_at(world);
    // Over known salvage only a unit's body counts: the slop that widens
    // a fingertip or a zoomed-out cursor must not hand the harvester
    // working a pile the tap aimed at the pile itself.
    let salvage_here = game.view().known_salvage(tile).is_some();
    let unit = game
        .state
        .units()
        .iter()
        .filter(|u| selectable(game, u))
        .filter_map(|u| {
            let p = vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>());
            let distance = p.distance(world);
            let radius = if salvage_here {
                super::unit_pick_radius(u.kind)
            } else {
                pick_radius(game, ui, u.kind, reach)
            };
            (distance <= radius).then_some((
                u.player != game.presentation.human,
                distance,
                u.id,
                u.player,
            ))
        })
        .min_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).expect("finite"));
    if let Some((_, _, id, owner)) = unit {
        return Some(Picked::Unit(id, owner));
    }
    // Only the human's own buildings skip the sight check: built ally
    // buildings are always inside shared team sight anyway, but ally
    // sites are blind until built, and selecting one through fog would
    // leak its live kind and hp through the panel.
    let under = game
        .state
        .buildings_at(tile)
        .find(|b| selectable_building(game, b));
    if let Some(b) = under {
        return Some(Picked::Building(b.id, b.player));
    }
    // The reticle also snaps onto a structure whose footprint lies in reach,
    // but not from over known salvage, where the pile is what it aims at.
    if salvage_here {
        return None;
    }
    let reach = reach.structures? * ui / game.presentation.camera.zoom;
    game.state
        .buildings()
        .iter()
        .filter(|b| selectable_building(game, b))
        .filter_map(|b| {
            let (w, h) = b.kind.size();
            let min = vec2(b.anchor.x as f32, b.anchor.y as f32);
            let max = min + vec2(w as f32, h as f32);
            let distance = world.clamp(min, max).distance(world);
            (distance <= reach).then_some((b.player != game.presentation.human, distance, b))
        })
        .min_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).expect("finite"))
        .map(|(_, _, b)| Picked::Building(b.id, b.player))
}

/// Whether the HUD covers `screen` for a controller's press: its chrome,
/// and the minimap too, whose steering belongs to HUD focus rather than to
/// the battlefield hidden under it.
pub(super) fn under_hud_for_pad(game: &Game, screen: Vec2) -> bool {
    click_on_hud(game, screen) || crate::render::minimap_world_at(&game.view(), screen).is_some()
}

/// Whether HUD chrome swallows a click at `screen`, per the published
/// HUD layout.
pub(super) fn click_on_hud(game: &Game, screen: Vec2) -> bool {
    game.presentation.layout.get().chrome_owns(screen)
}

/// Whether the human may see this unit at all: own and allies always
/// (team sight), enemies only on currently visible ground. Selection
/// must never reach through fog. A drone flies for its host alone and is
/// never selected.
fn selectable(game: &Game, unit: &aftermine_sim::Unit) -> bool {
    sees_unit(&game.view(), unit)
}

fn sees_unit(game: &crate::game::Scene<'_>, unit: &aftermine_sim::Unit) -> bool {
    unit.host.is_none()
        && (!game.state.hostile(game.presentation.human, unit.player)
            || game.presentation.all_seeing()
            || game.my_vision().sees_unit(unit))
}

/// Whether the human may select this building without learning through
/// fog — or through stealth: an undetected buried charge must not be
/// clickable on ground the player merely sees.
fn selectable_building(game: &Game, building: &aftermine_sim::Building) -> bool {
    sees_building(&game.view(), building)
}

fn sees_building(game: &crate::game::Scene<'_>, building: &aftermine_sim::Building) -> bool {
    building.player == game.presentation.human
        || game.presentation.all_seeing()
        || (building.tiles().any(|tile| game.my_vision().visible(tile))
            && game
                .state
                .building_apparent(game.presentation.human, building))
}

impl Picked {
    /// Whether the human may still see what was picked: a pick made before
    /// a tick may have walked into fog or died since.
    pub(crate) fn seen(self, game: &crate::game::Scene<'_>) -> bool {
        match self {
            Self::Unit(id, _) => game.state.unit(id).is_some_and(|u| sees_unit(game, u)),
            Self::Building(id, _) => game
                .state
                .building(id)
                .is_some_and(|b| sees_building(game, b)),
        }
    }
}

pub(super) fn click_select(game: &mut Game, screen: Vec2, additive: bool, ui: f32, reach: Reach) {
    let picked = pick(game, screen, ui, reach);
    select_picked(game, picked, screen, additive);
}

/// Selects what a press `picked`, as a click does: a unit or building
/// replaces the selection, or with `additive` toggles within one owner; a
/// miss clears it, or inspects known salvage under `screen`, and an
/// additive miss leaves it alone.
pub(super) fn select_picked(game: &mut Game, picked: Option<Picked>, screen: Vec2, additive: bool) {
    if !additive {
        game.presentation.selection.buildings.clear();
    }
    if picked.is_some() {
        game.presentation.selection.pile = None;
    }
    if let Some(Picked::Unit(id, owner)) = picked {
        game.presentation.selection.buildings.clear();
        let current_owner = game
            .presentation
            .selection
            .units
            .first()
            .and_then(|u| game.state.unit(*u))
            .map(|u| u.player);
        if additive && current_owner == Some(owner) {
            // Shift-click toggles membership within one allegiance.
            if let Some(index) = game
                .presentation
                .selection
                .units
                .iter()
                .position(|u| *u == id)
            {
                game.presentation.selection.units.remove(index);
            } else {
                game.presentation.selection.units.push(id);
            }
        } else {
            // A different owner replaces the selection, keeping it to one
            // allegiance.
            game.presentation.selection.units = vec![id];
        }
        return;
    }
    // …then a building under the cursor (any owner whose ground shows).
    if let Some(Picked::Building(id, owner)) = picked {
        game.presentation.selection.units.clear();
        let current_owner = game
            .presentation
            .selection
            .buildings
            .first()
            .and_then(|id| game.state.building(*id))
            .map(|selected| selected.player);
        if additive && current_owner == Some(owner) {
            if let Some(index) = game
                .presentation
                .selection
                .buildings
                .iter()
                .position(|selected| *selected == id)
            {
                game.presentation.selection.buildings.remove(index);
            } else {
                game.presentation.selection.buildings.push(id);
                game.presentation.selection.buildings.sort_unstable();
            }
        } else {
            game.presentation.selection.buildings = vec![id];
        }
        return;
    }
    if additive {
        return; // shift-miss leaves the selection alone
    }
    // …otherwise inspect the salvage under it, or clear.
    game.presentation.selection.units.clear();
    game.presentation.selection.buildings.clear();
    let world = game.presentation.camera.to_world(screen);
    let tile = numeric::tile_at(world);
    game.presentation.selection.pile = game.view().known_salvage(tile).is_some().then_some(tile);
}

pub(super) fn box_select(game: &mut Game, a_screen: Vec2, b_screen: Vec2, additive: bool) {
    let a = game.presentation.camera.to_world(a_screen);
    let b = game.presentation.camera.to_world(b_screen);
    let (lo, hi) = (a.min(b), a.max(b));
    let inside = |p: Vec2| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    let units: Vec<UnitId> = game
        .state
        .units()
        .iter()
        .filter(|u| inside(vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>())))
        .map(|u| u.id)
        .collect();
    // Building centers are the pick points, matching units and avoiding
    // edge-only grabs of a large footprint.
    let buildings: Vec<BuildingId> = game
        .state
        .buildings()
        .iter()
        .filter(|building| {
            let center = building.center();
            inside(vec2(center.x.to_num::<f32>(), center.y.to_num::<f32>()))
        })
        .map(|building| building.id)
        .collect();
    select_swept(game, &units, &buildings, additive);
}

/// What a sweep of the battlefield selects, out of everything it covered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Kept {
    /// One owner's units, in id order.
    Units(PlayerId, Vec<UnitId>),
    /// One owner's buildings, in id order.
    Buildings(PlayerId, Vec<BuildingId>),
}

/// The part of a sweep's cover a selection keeps, by one priority for the
/// box, the two-finger box and the controller's paint: own units, else own
/// buildings, else the units of one other owner, else that owner's
/// buildings. Commandable subjects win before foreign inspection, and
/// within one owner units outrank buildings, so a selection never mixes
/// mobile and static subjects. Covered entities the human can no longer
/// see, and the dead, drop out here.
pub(crate) fn kept(
    game: &crate::game::Scene<'_>,
    units: &[UnitId],
    buildings: &[BuildingId],
) -> Option<Kept> {
    let human = game.presentation.human;
    let units: Vec<&aftermine_sim::Unit> = units
        .iter()
        .filter_map(|id| game.state.unit(*id))
        .filter(|unit| sees_unit(game, unit))
        .collect();
    let buildings: Vec<&aftermine_sim::Building> = buildings
        .iter()
        .filter_map(|id| game.state.building(*id))
        .filter(|building| sees_building(game, building))
        .collect();
    let owned_units = |owner: PlayerId| {
        let mut ids: Vec<UnitId> = units
            .iter()
            .filter(|unit| unit.player == owner)
            .map(|unit| unit.id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        (!ids.is_empty()).then_some(Kept::Units(owner, ids))
    };
    let owned_buildings = |owner: PlayerId| {
        let mut ids: Vec<BuildingId> = buildings
            .iter()
            .filter(|building| building.player == owner)
            .map(|building| building.id)
            .collect();
        ids.sort_unstable();
        ids.dedup();
        (!ids.is_empty()).then_some(Kept::Buildings(owner, ids))
    };
    // With nothing commandable covered, inspect one visible foreign owner
    // at a time.
    let foreign_units = units
        .iter()
        .map(|unit| unit.player)
        .filter(|owner| *owner != human)
        .min();
    let foreign_buildings = buildings
        .iter()
        .map(|building| building.player)
        .filter(|owner| *owner != human)
        .min();
    owned_units(human)
        .or_else(|| owned_buildings(human))
        .or_else(|| foreign_units.and_then(owned_units))
        .or_else(|| foreign_buildings.and_then(owned_buildings))
}

/// Selects what [`kept`] keeps of a sweep's cover. Additive, it joins the
/// current selection when that is the same owner's of the same sort;
/// otherwise it replaces it, and a sweep that keeps nothing clears it.
pub(super) fn select_swept(
    game: &mut Game,
    units: &[UnitId],
    buildings: &[BuildingId],
    additive: bool,
) {
    let kept = kept(&game.view(), units, buildings);
    let state = &game.state;
    let selection = &mut game.presentation.selection;
    match kept {
        Some(Kept::Units(owner, mut ids)) => {
            if additive {
                ids.extend(
                    selection
                        .units
                        .iter()
                        .copied()
                        .filter(|id| state.unit(*id).is_some_and(|u| u.player == owner)),
                );
                ids.sort_unstable();
                ids.dedup();
            }
            selection.units = ids;
            selection.buildings.clear();
        }
        Some(Kept::Buildings(owner, mut ids)) => {
            if additive {
                ids.extend(
                    selection
                        .buildings
                        .iter()
                        .copied()
                        .filter(|id| state.building(*id).is_some_and(|b| b.player == owner)),
                );
                ids.sort_unstable();
                ids.dedup();
            }
            selection.units.clear();
            selection.buildings = ids;
        }
        None if additive => {}
        None => {
            selection.units.clear();
            selection.buildings.clear();
            selection.pile = None;
        }
    }
}

/// Double-click: visible entities of the picked kind and owner on screen.
pub(super) fn select_all_of_kind_on_screen(game: &mut Game, screen: Vec2, ui: f32, reach: Reach) {
    let world = game.presentation.camera.to_world(screen);
    // The sweep stays within the picked unit's owner: double-clicking an
    // ally harvester gathers that ally's harvesters on screen. Own units
    // outrank foreign at the pick, as with plain clicks.
    let picked = game
        .state
        .units()
        .iter()
        .filter(|u| selectable(game, u))
        .filter_map(|u| {
            let p = vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>());
            let distance = p.distance(world);
            (distance <= pick_radius(game, ui, u.kind, reach)).then_some((
                u.player != game.presentation.human,
                distance,
                u.kind,
                u.player,
            ))
        })
        .min_by(|a, b| (a.0, a.1).partial_cmp(&(b.0, b.1)).expect("finite"));
    let (lo, hi) = game.presentation.camera.world_rect();
    let on_screen = |p: Vec2| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    if let Some((_, _, kind, owner)) = picked {
        game.presentation.selection.buildings.clear();
        game.presentation.selection.units = game
            .state
            .units()
            .iter()
            .filter(|u| u.player == owner && u.kind == kind && selectable(game, u))
            .filter(|u| on_screen(vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>())))
            .map(|u| u.id)
            .collect();
        return;
    }

    // The building a press there picks: the one under it, or for the
    // reticle one within its reach.
    let Some(Picked::Building(id, _)) = pick(game, screen, ui, reach) else {
        return;
    };
    let Some(picked) = game.state.building(id) else {
        return;
    };
    let (kind, owner) = (picked.kind, picked.player);
    game.presentation.selection.units.clear();
    game.presentation.selection.buildings = game
        .state
        .buildings()
        .iter()
        .filter(|building| {
            building.player == owner && building.kind == kind && selectable_building(game, building)
        })
        .filter(|building| {
            let center = building.center();
            on_screen(vec2(center.x.to_num::<f32>(), center.y.to_num::<f32>()))
        })
        .map(|building| building.id)
        .collect();
}

/// Every unit and building the human can see within `radius` logical
/// pixels of the ground the reticle crossed from `from` to `to`, where the
/// brush meets it on open battlefield: what HUD chrome or the minimap
/// covers there stays out. A unit is reached anywhere on its body, as a
/// press picks it; at the reticle's own reach this is exactly what a press
/// there could pick.
pub(super) fn swept_by_pad(
    game: &Game,
    from: Vec2,
    to: Vec2,
    radius: f32,
) -> (Vec<UnitId>, Vec<BuildingId>) {
    let camera = &game.presentation.camera;
    let reach = radius / camera.zoom;
    let uncovered = |world: Vec2| !under_hud_for_pad(game, camera.to_screen(world));
    let units = game
        .state
        .units()
        .iter()
        .filter(|u| selectable(game, u))
        .filter(|u| {
            let p = vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>());
            segment_distance(p, from, to) <= reach.max(super::unit_pick_radius(u.kind))
                && uncovered(p)
        })
        .map(|u| u.id)
        .collect();
    let buildings = game
        .state
        .buildings()
        .iter()
        .filter(|b| selectable_building(game, b))
        .filter(|b| {
            let (min, max) = footprint(b);
            let contact = rect_contact(min, max, from, to);
            segment_distance(contact, from, to) <= reach && uncovered(contact)
        })
        .map(|b| b.id)
        .collect();
    (units, buildings)
}

/// A building's footprint as world corners.
pub(super) fn footprint(building: &aftermine_sim::Building) -> (Vec2, Vec2) {
    let (w, h) = building.kind.size();
    let min = vec2(building.anchor.x as f32, building.anchor.y as f32);
    (min, min + vec2(w as f32, h as f32))
}

/// How far `p` lies from the segment `a`..`b`.
fn segment_distance(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let length = ab.length_squared();
    if length == 0.0 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / length).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The point of the rectangle `min`..`max` nearest the segment `a`..`b`:
/// where the segment enters it, if it does; else the nearest of its corners
/// and of the ends' nearest points on it, as two convex shapes apart come
/// closest at a corner of one.
fn rect_contact(min: Vec2, max: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    if let Some(enter) = entry(min, max, a, b) {
        return a + (b - a) * enter;
    }
    [min, vec2(max.x, min.y), max, vec2(min.x, max.y)]
        .into_iter()
        .chain([a, b].map(|end| end.clamp(min, max)))
        .min_by(|p, q| segment_distance(*p, a, b).total_cmp(&segment_distance(*q, a, b)))
        .expect("a rectangle has corners")
}

/// How far along the segment `a`..`b` it first touches the rectangle
/// `min`..`max`, as a fraction of its length, by clipping it against each
/// axis in turn; none where it misses.
fn entry(min: Vec2, max: Vec2, a: Vec2, b: Vec2) -> Option<f32> {
    let d = b - a;
    let (mut enter, mut leave) = (0.0_f32, 1.0_f32);
    for (start, step, lo, hi) in [(a.x, d.x, min.x, max.x), (a.y, d.y, min.y, max.y)] {
        if step == 0.0 {
            if start < lo || start > hi {
                return None;
            }
            continue;
        }
        let (t0, t1) = ((lo - start) / step, (hi - start) / step);
        enter = enter.max(t0.min(t1));
        leave = leave.min(t0.max(t1));
        if enter > leave {
            return None;
        }
    }
    Some(enter)
}
