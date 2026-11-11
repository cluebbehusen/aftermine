//! Order programs as the staged commands will leave them.
//!
//! The orders dock and the waypoint chain show the selection's programs with
//! every staged command applied, so a chip clicked while paused, or while a
//! networked order is still on its way, names an order in the program that
//! command will actually edit.

use aftermine_sim::{
    Building, BuildingId, CommandPhase, CommandPhaseView, Order, PlayerCommand, PlayerId, State,
    Tick, Unit, UnitId,
};

/// The world as the staged commands leave it, shared by every read of it
/// until a tick or another staged command: the staged batch only grows, or
/// drains along with a tick, so its length tells its versions apart.
pub(crate) struct StagedPhase {
    tick: Tick,
    pending: usize,
    phase: CommandPhase,
}

impl StagedPhase {
    pub(super) fn capture(state: &State, pending: &[PlayerCommand]) -> Self {
        Self {
            tick: state.current_tick(),
            pending: pending.len(),
            phase: state.command_phase(pending),
        }
    }

    pub(super) fn is_current(&self, tick: Tick, pending: usize) -> bool {
        self.tick == tick && self.pending == pending
    }

    pub(super) fn view(&self) -> CommandPhaseView<'_> {
        self.phase.view()
    }
}

/// One unit's order program, active order first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Program {
    /// The active order, then the queue.
    pub(crate) orders: Vec<Order>,
    /// Whether finished orders rotate to the back (a patrol).
    pub(crate) looping: bool,
}

impl Program {
    fn of(unit: &Unit) -> Self {
        Self {
            orders: std::iter::once(&unit.order)
                .chain(&unit.queue)
                .copied()
                .collect(),
            looping: unit.looping,
        }
    }

    /// Whether the unit is idle with nothing queued.
    pub(crate) fn is_idle(&self) -> bool {
        matches!(self.orders[..], [Order::Idle])
    }
}

/// What a projection was captured from.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Captured {
    tick: Tick,
    pending: usize,
    units: Vec<UnitId>,
}

/// The programs of the selected units after the staged commands, with the
/// sites their Build orders name. It covers the whole selection, not just
/// the units that draw decor, because selection-wide facts such as a patrol
/// read it. It is captured once per tick, staged-command count and
/// selection: commands are only ever appended to the staged batch or drained
/// from it along with a tick.
#[derive(Debug, Default)]
pub(crate) struct Projection {
    captured: Option<Captured>,
    programs: Vec<(UnitId, Program)>,
    sites: Vec<Building>,
    /// The selection's Build and Found orders, with their owners, whose site
    /// a worker outside the selection also holds.
    shared: Vec<(PlayerId, Order)>,
}

impl Projection {
    pub(super) fn is_current(&self, tick: Tick, pending: usize, units: &[UnitId]) -> bool {
        self.captured.as_ref().is_some_and(|captured| {
            captured.tick == tick && captured.pending == pending && captured.units == units
        })
    }

    /// Projects `units` through the `pending` commands, whose command phase
    /// `staged` reads. Only a staged batch needs it; otherwise the programs
    /// are the world's own.
    pub(super) fn capture(
        state: &State,
        pending: usize,
        staged: Option<CommandPhaseView<'_>>,
        units: Vec<UnitId>,
    ) -> Self {
        let (programs, sites, shared) = match staged {
            None => {
                let programs = programs(&units, |id| state.unit(id));
                let shared = shared_sites(
                    &programs,
                    |id| state.unit(id),
                    state.buildings(),
                    |site| state.site_crew(site),
                );
                (programs, Vec::new(), shared)
            }
            Some(view) => {
                let programs = programs(&units, |id| view.unit(id));
                let sites = view
                    .buildings()
                    .iter()
                    .filter(|site| {
                        programs.iter().any(|(_, program)| {
                            program.orders.contains(&Order::Build { site: site.id })
                        })
                    })
                    .cloned()
                    .collect();
                let shared = shared_sites(
                    &programs,
                    |id| view.unit(id),
                    view.buildings(),
                    |site| view.site_crew(site),
                );
                (programs, sites, shared)
            }
        };
        Self {
            captured: Some(Captured {
                tick: state.current_tick(),
                pending,
                units,
            }),
            programs,
            sites,
            shared,
        }
    }

    /// A selected unit's projected program.
    pub(crate) fn program(&self, id: UnitId) -> Option<&Program> {
        self.programs
            .binary_search_by_key(&id, |(unit, _)| *unit)
            .ok()
            .map(|index| &self.programs[index].1)
    }

    /// A building as the staged commands leave it, so a site a staged Build
    /// claims is already there.
    pub(crate) fn building<'a>(&'a self, state: &'a State, id: BuildingId) -> Option<&'a Building> {
        self.sites
            .iter()
            .find(|site| site.id == id)
            .or_else(|| state.building(id))
    }

    /// Whether a worker outside the selection still holds the site that a
    /// selected unit of `player` names with its Build or Found `order`.
    pub(crate) fn site_shared(&self, player: PlayerId, order: Order) -> bool {
        self.shared.contains(&(player, order))
    }
}

/// The Build and Found orders in `programs`, with their owners, whose site a
/// worker outside `programs` also holds.
fn shared_sites<'w, C: Iterator<Item = UnitId>>(
    programs: &[(UnitId, Program)],
    unit_of: impl Fn(UnitId) -> Option<&'w Unit>,
    buildings: &'w [Building],
    crew: impl Fn(BuildingId) -> C,
) -> Vec<(PlayerId, Order)> {
    let selected = |id: UnitId| {
        programs
            .binary_search_by_key(&id, |(unit, _)| *unit)
            .is_ok()
    };
    let mut seen = Vec::new();
    let mut shared = Vec::new();
    for (unit, program) in programs {
        let Some(player) = unit_of(*unit).map(|unit| unit.player) else {
            continue;
        };
        for &order in &program.orders {
            if !matches!(order, Order::Build { .. } | Order::Found { .. })
                || seen.contains(&(player, order))
            {
                continue;
            }
            seen.push((player, order));
            let site = buildings.iter().find(|site| match order {
                Order::Build { site: id } => site.id == id,
                Order::Found { kind, anchor } => {
                    site.player == player
                        && site.kind == kind
                        && site.anchor == anchor
                        && site.under_construction()
                }
                _ => false,
            });
            if site.is_some_and(|site| crew(site.id).any(|id| !selected(id))) {
                shared.push((player, order));
            }
        }
    }
    shared
}

/// The programs of `ids` in id order, so a selection-wide read looks each
/// up without scanning.
fn programs<'u>(
    ids: &[UnitId],
    unit: impl Fn(UnitId) -> Option<&'u Unit>,
) -> Vec<(UnitId, Program)> {
    let mut programs: Vec<_> = ids
        .iter()
        .filter_map(|&id| Some((id, Program::of(unit(id)?))))
        .collect();
    programs.sort_unstable_by_key(|(id, _)| *id);
    programs.dedup_by_key(|(id, _)| *id);
    programs
}

#[cfg(test)]
mod tests;
