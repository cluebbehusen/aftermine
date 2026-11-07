//! Turning intent into sim commands: the right-click context order,
//! contextual digits, control groups, and factory training. The sim
//! re-validates everything — this module only shapes intent, and its
//! fog checks exist so a click cannot *probe* what the player can't see.

use super::{InputState, ground_tile, tile_center, unit_pick_radius};
use crate::game::{Game, PingKind};
use crate::numeric;
use aftermine_sim::{Command, Rally, Target};
use chassis::grid::TilePos;
use macroquad::prelude::{Vec2, vec2};

pub(super) fn selected_producers(game: &Game) -> Vec<aftermine_sim::BuildingId> {
    crate::building_actions::SelectedBuildings::inspect(&game.view()).producers()
}

pub(super) fn rally_selected_producers(game: &mut Game, rally: Rally) {
    let producers = selected_producers(game);
    if producers.is_empty() {
        return;
    }
    rally_producers(game, producers, rally);
}

/// Points `producers` at `rally` and pings where it lands.
pub(super) fn rally_producers(
    game: &mut Game,
    producers: Vec<aftermine_sim::BuildingId>,
    rally: Rally,
) {
    for building in producers {
        game.issue(Command::SetRally {
            building,
            rally: Some(rally),
        });
    }
    let at = match rally {
        Rally::Tile(tile) => tile_center(tile),
        Rally::Unit(unit) => match game.state.unit(unit) {
            Some(u) => vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>()),
            None => return,
        },
    };
    game.presentation.ping(at, PingKind::Rally);
}

/// How far around its point an order reaches for what it names, in world
/// units. A click names only what lies under it: a unit by its own pick
/// radius, a structure by its footprint. A controller's reticle reaches as
/// far as it snaps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OrderReach {
    /// The least distance from a unit's center that still names it.
    units: f32,
    /// How far from a structure's footprint still names it.
    structures: f32,
}

impl OrderReach {
    /// A click's: exactly what lies under the pointer.
    pub(crate) const EXACT: Self = Self {
        units: 0.0,
        structures: 0.0,
    };

    /// A reach of `units` around a unit's center and `structures` around
    /// a footprint, in world units.
    pub(super) const fn new(units: f32, structures: f32) -> Self {
        Self { units, structures }
    }

    /// How far from its center a unit of `kind` is named.
    pub(super) fn unit(self, kind: aftermine_sim::UnitKind) -> f32 {
        unit_pick_radius(kind).max(self.units)
    }

    /// The building `pred` accepts that `world` names: the one under it,
    /// else the nearest whose footprint lies within reach.
    pub(super) fn building(
        self,
        game: &Game,
        world: Vec2,
        pred: impl Fn(&aftermine_sim::Building) -> bool,
    ) -> Option<&aftermine_sim::Building> {
        let under = game
            .state
            .buildings_at(numeric::tile_at(world))
            .find(|building| pred(building));
        if under.is_some() || self.structures <= 0.0 {
            return under;
        }
        game.state
            .buildings()
            .iter()
            .filter(|building| pred(building))
            .filter_map(|building| {
                let (min, max) = super::select::footprint(building);
                let distance = world.clamp(min, max).distance(world);
                (distance <= self.structures).then_some((distance, building))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)))
            .map(|(_, building)| building)
    }
}

/// The rally a world click names: the own unit within `reach`, never a
/// drone, or else the ground.
pub(super) fn rally_at(game: &Game, world: Vec2, reach: OrderReach) -> Rally {
    game.state
        .units()
        .iter()
        .filter(|u| u.player == game.presentation.human && u.host.is_none())
        .map(|u| {
            let at = vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>());
            (at.distance(world), u.id, u.kind)
        })
        .filter(|(distance, _, kind)| *distance <= reach.unit(*kind))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map_or_else(
            || Rally::Tile(ground_tile(&game.state, world)),
            |(_, unit, _)| Rally::Unit(unit),
        )
}

/// The nearest hostile the player sees within `reach` of `world`, in a
/// domain `fires_at` takes: a unit, else a building.
fn visible_hostile_target_at(
    game: &Game,
    world: Vec2,
    reach: OrderReach,
    fires_at: &dyn Fn(aftermine_sim::stats::Domain) -> bool,
) -> Option<(Target, Vec2, aftermine_sim::stats::Domain)> {
    let unit = game
        .state
        .units()
        .iter()
        .filter(|unit| {
            game.state.hostile(game.presentation.human, unit.player)
                && game.my_vision().sees_unit(unit)
                && fires_at(unit.domain())
        })
        .filter_map(|unit| {
            let position = vec2(unit.pos.x.to_num::<f32>(), unit.pos.y.to_num::<f32>());
            let distance = position.distance(world);
            (distance <= reach.unit(unit.kind)).then_some((
                distance,
                unit.id,
                position,
                // Its layer right now: a parked airframe is a ground
                // target for the weapons that can reach ground.
                unit.domain(),
            ))
        })
        .min_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
        });
    if let Some((_, id, position, domain)) = unit {
        return Some((Target::Unit(id), position, domain));
    }
    if !fires_at(aftermine_sim::stats::Domain::Ground) {
        return None;
    }
    reach
        .building(game, world, |building| {
            game.state.hostile(game.presentation.human, building.player)
                && building
                    .tiles()
                    .any(|footprint| game.my_vision().visible(footprint))
                && game
                    .state
                    .building_apparent(game.presentation.human, building)
        })
        .map(|building| {
            let (min, max) = super::select::footprint(building);
            (
                Target::Building(building.id),
                world.clamp(min, max),
                aftermine_sim::stats::Domain::Ground,
            )
        })
}

fn known_hostile_target_at(
    game: &Game,
    world: Vec2,
    tile: TilePos,
    reach: OrderReach,
    fires_at: &dyn Fn(aftermine_sim::stats::Domain) -> bool,
) -> Option<(
    aftermine_sim::AttackTarget,
    Vec2,
    aftermine_sim::stats::Domain,
)> {
    if let Some((target, at, domain)) = visible_hostile_target_at(game, world, reach, fires_at) {
        return Some((target.into(), at, domain));
    }
    if let Some(track) =
        game.my_vision().tracks().iter().find(|track| {
            track.visible_unit.is_none() && track.tile == tile && fires_at(track.domain)
        })
    {
        return Some((
            aftermine_sim::AttackTarget::Contact(track.id),
            vec2(tile.x as f32 + 0.5, tile.y as f32 + 0.5),
            track.domain,
        ));
    }
    if !fires_at(aftermine_sim::stats::Domain::Ground) {
        return None;
    }
    if let Some(ghost) = game.my_vision().ghosts().iter().find(|ghost| {
        let (w, h) = ghost.kind.size();
        tile.x >= ghost.anchor.x
            && tile.y >= ghost.anchor.y
            && tile.x < ghost.anchor.x + w
            && tile.y < ghost.anchor.y + h
    }) {
        return Some((
            aftermine_sim::AttackTarget::RememberedBuilding(aftermine_sim::RememberedBuilding {
                owner: ghost.owner,
                building_kind: ghost.kind,
                anchor: ghost.anchor,
            }),
            world,
            aftermine_sim::stats::Domain::Ground,
        ));
    }
    if let Some(public) =
        aftermine_sim::observation::hostile_public_buildings(&game.state, game.presentation.human)
            .into_iter()
            .find(|public| public.covers(tile))
    {
        return Some((
            aftermine_sim::AttackTarget::RememberedBuilding(public.target()),
            world,
            aftermine_sim::stats::Domain::Ground,
        ));
    }
    None
}

/// Group recall works independently of the selected panel.
pub(super) fn digit_action(game: &mut Game, input: &mut InputState, slot: usize) {
    input.close_construction();

    if slot < 5 {
        group_action(game, input, slot);
    }
}

/// Recall (or with Ctrl, assign) a control group; a quick double-tap on
/// the same slot centers the camera on the group.
fn group_action(game: &mut Game, input: &mut InputState, slot: usize) {
    let alive = input.groups[slot].live(&game.view());
    input.groups[slot].clone_from(&alive);
    if alive.is_empty() {
        return;
    }
    alive.select(game);
    let now = input.now;
    if input
        .last_recall
        .is_some_and(|(s, t)| s == slot && now - t < 0.4)
        && let Some(center) = alive.center(&game.view())
    {
        super::reticle::look_at(game, input, center);
    }
    input.last_recall = Some((slot, now));
}

/// Right-click: order the selection by what's under the cursor — enemy →
/// attack, scrap → harvest, ground → advance. A controller's X gives the
/// same order with the reticle's `reach`. The sim re-validates everything;
/// this is only intent.
pub(super) fn context_order(game: &mut Game, screen: Vec2, queue: bool, reach: OrderReach) {
    let world = game.presentation.camera.to_world(screen);
    let tile = numeric::tile_at(world);
    if game.presentation.selection.units.is_empty() {
        // Selected defenses focus on the enemy one of them can fire at.
        let selected = crate::building_actions::SelectedBuildings::inspect(&game.view());
        let fires_at = |domain| !selected.defenses(Some(domain)).is_empty();
        if let Some((target, at, domain)) =
            known_hostile_target_at(game, world, tile, reach, &fires_at)
        {
            let defenses = selected.defenses(Some(domain));
            game.issue(Command::FocusFire {
                buildings: defenses,
                target,
            });
            game.presentation.ping(at, PingKind::Attack);
            return;
        }
        // Selected producers share one rally destination: the own unit
        // under the cursor, which new units then escort, or the ground.
        // Non-producers ignore the click.
        let rally = rally_at(game, world, reach);
        rally_selected_producers(game, rally);
        return;
    }
    if !game.selection_commandable() {
        game.presentation
            .toast("You can only command your own units");
        return;
    }
    let units = game.presentation.selection.units.clone();
    // Each verb crews by its own capability, mirroring the sim's crew
    // filters exactly: workers (harvest kit) carry build and harvest
    // labor, welders carry the torch. A coarser union here would stage
    // commands the sim rejects with an empty crew.
    let has_worker = units.iter().any(|id| {
        game.state
            .unit(*id)
            .is_some_and(|u| u.kind.stats().harvest.is_some())
    });
    let has_welder = units.iter().any(|id| {
        game.state
            .unit(*id)
            .is_some_and(|u| u.kind.stats().harvest.is_some())
    });

    // The harvest check reads the player's *memory*, not the live map —
    // probing fog with right-clicks must not reveal hidden scrap. Wreck
    // memory counts the same as node memory.
    let harvest = has_worker
        && (game.my_vision().remembered_scrap(tile) > 0
            || game.my_vision().remembered_wreck(tile) > 0);
    // Over a pile the order would harvest, only what lies under the point
    // counts: a reticle's reach must not hand the order to whatever stands
    // beside the pile.
    let reach = if harvest { OrderReach::EXACT } else { reach };

    // Own-footprint hits outrank enemy-radius hits: a raider attacking a
    // wall sits inside the pick radius of a click on that wall, and the
    // click means the building under the cursor. Own targets need no
    // visibility check, since ownership cannot probe fog and own
    // buildings always draw.
    let human = game.presentation.human;
    let own_building = reach.building(game, world, |b| b.player == human);
    if (has_worker || has_welder)
        && let Some(building) = own_building
    {
        if !building.built() {
            if building.tier > 0 {
                game.presentation.toast("Upgrade runs automatically");
                return;
            }
            if has_worker {
                // Resume the site: the sim commits every accepted
                // worker (builders stack). Send the building's own
                // anchor and kind — the cursor may be on any footprint
                // tile of a 2x2.
                game.issue(Command::Build {
                    units,
                    kind: building.kind,
                    anchor: building.anchor,
                    queue,
                    defer: false,
                });
                game.presentation
                    .ping_order(world, PingKind::Harvest, queue);
                return;
            }
            // Welders alone cannot lay construction: fall through.
        } else if building.kind.is_drop_off()
            && units.iter().any(|id| {
                game.state
                    .unit(*id)
                    .is_some_and(|unit| unit.kind.stats().harvest.is_some() && unit.carrying() > 0)
            })
        {
            let foundry = building.id;
            let repair = building.hp < building.stats().max_hp;
            let (loaded, others): (Vec<_>, Vec<_>) = units.into_iter().partition(|id| {
                game.state
                    .unit(*id)
                    .is_some_and(|unit| unit.kind.stats().harvest.is_some() && unit.carrying() > 0)
            });
            let welders: Vec<_> = others
                .into_iter()
                .filter(|id| {
                    game.state
                        .unit(*id)
                        .is_some_and(|unit| unit.kind.stats().harvest.is_some())
                })
                .collect();
            game.issue(Command::ReturnCargo {
                units: loaded,
                foundry: Some(foundry),
                repair,
            });
            if repair && !welders.is_empty() {
                game.issue(Command::Repair {
                    units: welders,
                    building: foundry,
                    queue,
                });
            }
            game.presentation.ping(world, PingKind::Harvest);
            return;
        } else if building.hp < building.stats().max_hp && has_welder {
            game.issue(Command::Repair {
                units,
                building: building.id,
                queue,
            });
            game.presentation
                .ping_order(world, PingKind::Harvest, queue);
            return;
        }
        // A healthy built own building — or a site without a worker, or
        // a wound without a torch — falls through to the ground order.
    }

    // Carriable ground machines right-clicked onto an own transport
    // climb aboard — before the hostile check reads the ground under
    // the hovering airframe.
    let carriable: Vec<aftermine_sim::UnitId> = units
        .iter()
        .copied()
        .filter(|id| {
            game.state
                .unit(*id)
                .is_some_and(|u| u.kind.stats().transport_size > 0)
        })
        .collect();
    if !carriable.is_empty() {
        let sling = game
            .state
            .units()
            .iter()
            .filter(|u| {
                u.player == game.presentation.human
                    && u.hp > 0
                    && u.kind.stats().transport_capacity > 0
                    && !game.presentation.selection.units.contains(&u.id)
            })
            .map(|u| {
                let p = vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>());
                (p.distance(world), u.id, u.kind)
            })
            .filter(|(distance, _, kind)| *distance <= reach.unit(*kind))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, transport, _)) = sling {
            game.issue(Command::Load {
                units: carriable,
                transport,
                queue,
            });
            game.presentation.ping_order(world, PingKind::Move, queue);
            return;
        }
    }
    // A selection of scouts scouts wherever it is sent, enemy or ground;
    // Run is the deliberate way to send one into danger.
    if units
        .iter()
        .all(|id| game.state.unit(*id).is_some_and(|u| u.kind.stats().scouts))
    {
        let goal = ground_tile(&game.state, world);
        game.issue(Command::Scout { units, goal, queue });
        game.presentation
            .ping_order(tile_center(goal), PingKind::Move, queue);
        return;
    }
    // Fog rules what right-click may target: unseen enemies aren't there
    // as far as the player is concerned (the sim enforces this too).
    if let Some((target, at, _)) = known_hostile_target_at(game, world, tile, reach, &|_| true) {
        game.issue(Command::Attack {
            units,
            target,
            queue,
        });
        game.presentation.ping_order(at, PingKind::Attack, queue);
        return;
    }
    // A wounded own ground unit under the cursor takes the weld, like the
    // damaged-building flow above, but only after the enemy checks (attack
    // intent stays reliable in a brawl) and never for a machine in the
    // current selection, so ordering a group that contains its own wounded
    // still reads as a move. The armed Weld verb reaches those.
    if has_welder {
        let patient = game
            .state
            .units()
            .iter()
            .filter(|u| {
                u.player == game.presentation.human
                    && u.hp > 0
                    && u.hp < u.kind.stats().max_hp
                    && u.kind.stats().repairable()
                    && u.domain() == aftermine_sim::stats::Domain::Ground
                    && !game.presentation.selection.units.contains(&u.id)
            })
            .map(|u| {
                let p = vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>());
                (p.distance(world), u.id, u.kind)
            })
            .filter(|(distance, _, kind)| *distance <= reach.unit(*kind))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, target, _)) = patient {
            game.issue(Command::RepairUnit {
                units,
                target,
                queue,
            });
            game.presentation
                .ping_order(world, PingKind::Harvest, queue);
            return;
        }
    }
    if harvest {
        game.issue(Command::Harvest {
            units,
            node: tile,
            queue,
        });
        game.presentation
            .ping_order(tile_center(tile), PingKind::Harvest, queue);
        return;
    }
    // Default ground movement keeps formation intent: weapons take
    // already-available shots, but machines never stop or chase.
    // Explicit hunt is the F verb.
    // Entity lookups above read the raw tile so a click in the edge
    // slack never binds to whatever sits on the edge; only the ground
    // goal clamps onto the map.
    let goal = ground_tile(&game.state, world);
    game.issue(Command::Advance { units, goal, queue });
    game.presentation
        .ping_order(tile_center(goal), PingKind::Move, queue);
}

/// Train the selected production slot through the shared pending-command view.
pub(super) fn train(game: &mut Game, slot: usize) {
    crate::production::train(game, slot);
}
