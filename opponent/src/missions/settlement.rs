//! Settlements: lifting a builder, with an escort while known enemies stand
//! near, to a resource site on ground no own worker stands on. A settlement
//! only delivers. Once the builder has landed, the site is an ordinary build
//! the saving target pays for and the landed worker raises, and its escort
//! hands over to the site's local guard.

use super::air::Legs;
use super::{MISSION_CAP, Mission, Missions, Phase, Task, carrier, fill, force, hunt, mine};
use super::{air, lift, reserve, strength};
use crate::decision::{Ledger, Site, site};
use crate::frame::{doubled, footprint_centre};
use crate::investments::{self, Investment, Step};
use crate::map::MapModel;
use crate::memory::Memory;
use crate::placement::Danger;
use crate::profile::ResolvedProfile;
use crate::workers;
use aftermine_sim::observation::ObservationData;
use aftermine_sim::stats::Domain;
use aftermine_sim::{BuildingKind, Command, UnitId, UnitKind};
use chassis::grid::TilePos;
use serde::{Deserialize, Serialize};

/// The building a settlement delivers a builder for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Target {
    /// An expansion Foundry on `site`'s footprint at `anchor`.
    Foundry { site: u16, anchor: TilePos },
    /// An Extractor on the frame at `anchor`.
    Extractor { anchor: TilePos },
}

impl Target {
    pub(super) fn kind(self) -> BuildingKind {
        match self {
            Self::Foundry { .. } => BuildingKind::Foundry,
            Self::Extractor { .. } => BuildingKind::Extractor,
        }
    }

    pub(super) fn anchor(self) -> TilePos {
        match self {
            Self::Foundry { anchor, .. } | Self::Extractor { anchor } => anchor,
        }
    }

    fn investment(self) -> Investment {
        match self {
            Self::Foundry { site, .. } => Investment::Expansion(site),
            Self::Extractor { anchor } => Investment::Extractor(anchor),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum SettlePhase {
    /// The builder and ground escorts board carriers at the source.
    Load,
    /// Carriers and air escorts fly to the landing.
    Fly,
    /// The builder has landed and gone back to ordinary work; escorts hold
    /// the landing until the building is placed.
    Found,
    /// A failed attempt sets down whatever is still aboard at home.
    Recover,
}

impl SettlePhase {
    pub(super) fn phase(self) -> Phase {
        match self {
            Self::Load => Phase::Load,
            Self::Fly => Phase::Fly,
            Self::Found => Phase::Found,
            Self::Recover => Phase::Recover,
        }
    }

    /// Ticks the phase should end within. Boarding and flight give up at
    /// theirs; a flight home only reports it. A held landing has none: it
    /// ends once the building is placed or no known enemy is near.
    pub(super) fn timeout(self) -> Option<u64> {
        match self {
            Self::Load => Some(lift::LOAD_TICKS),
            Self::Fly | Self::Recover => Some(lift::FLY_TICKS),
            Self::Found => None,
        }
    }
}

/// What a decision's settlements read.
pub(crate) struct Settlement<'a> {
    observation: &'a ObservationData,
    map: &'a MapModel,
    profile: &'a ResolvedProfile,
    memory: &'a Memory,
    danger: &'a Danger<'a>,
    scratch: &'a super::Scratch,
}

impl<'a> Settlement<'a> {
    pub(crate) fn new(
        observation: &'a ObservationData,
        map: &'a MapModel,
        profile: &'a ResolvedProfile,
        memory: &'a Memory,
        danger: &'a Danger<'a>,
        scratch: &'a super::Scratch,
    ) -> Self {
        Self {
            observation,
            map,
            profile,
            memory,
            danger,
            scratch,
        }
    }

    /// The building `investment` needs a builder lifted for: the one a
    /// settlement already holds, else where the ordinary build would place
    /// it, while no own worker stands on that ground. A worker already there
    /// builds it the ordinary way, including one a settlement landed or
    /// stranded.
    fn target(&self, missions: &Missions, investment: Investment) -> Option<Target> {
        if let Some(target) = missions.settling(investment) {
            return Some(target);
        }
        let target = match investment {
            Investment::Expansion(index) => match self.site(investment, BuildingKind::Foundry) {
                Site::At(anchor, _) | Site::Unseen(anchor) => Target::Foundry {
                    site: index,
                    anchor,
                },
                Site::Nowhere => return None,
            },
            Investment::Extractor(anchor) => Target::Extractor { anchor },
            _ => return None,
        };
        (!self.manned(self.map.component(target.anchor()))).then_some(target)
    }

    /// Whether an own worker stands on `ground`.
    fn manned(&self, ground: Option<u32>) -> bool {
        self.observation
            .my_units
            .iter()
            .any(|u| workers::worker(u.kind) && self.map.component(u.tile) == ground)
    }

    /// Where a building of `kind` toward `investment` would stand, as far as
    /// the seat knows, ignoring this decision's plans.
    pub(crate) fn site(&self, investment: Investment, kind: BuildingKind) -> Site {
        site(
            self.observation,
            self.map,
            self.memory,
            self.danger,
            investment,
            kind,
            &[],
        )
    }

    /// The built Foundry a settlement for `target` starts from: one on ground
    /// a worker stands on first, then one on the target's ground, else the
    /// nearest.
    fn source(&self, target: Target) -> Option<TilePos> {
        let centre = footprint_centre(target.kind(), target.anchor());
        self.observation
            .my_buildings
            .iter()
            .filter(|b| b.kind == BuildingKind::Foundry && b.built)
            .min_by_key(|b| {
                (
                    !self.manned(self.map.component(b.anchor)),
                    self.map.component(b.anchor) != self.map.component(target.anchor()),
                    self.scratch
                        .frame
                        .rank(centre, footprint_centre(b.kind, b.anchor)),
                    b.id,
                )
            })
            .map(|b| b.anchor)
    }

    fn by_air(&self, source: TilePos, target: Target) -> bool {
        self.map.component(source) != self.map.component(target.anchor())
    }

    /// The next purchase toward `investment`. A site no worker can reach
    /// first needs a worker at a Foundry, then, to lift one, an Airworks and
    /// enough carriers for the builder and its ground escort; while a
    /// settlement flies, the building itself stays next so the saving keeps
    /// its scrap.
    pub(crate) fn next(&self, missions: &Missions, investment: Investment) -> Option<(Step, u32)> {
        let basic = investments::step(self.observation, self.map, investment)?;
        let Some(target) = self.target(missions, investment) else {
            return Some(basic);
        };
        if basic.0 != Step::Build(target.kind()) || missions.settling(investment).is_some() {
            return Some(basic);
        }
        let Some(source) = self.source(target) else {
            return Some(basic);
        };
        if !self.manned(self.map.component(source)) {
            let queued = self.observation.my_queues.iter().flatten();
            return (!queued.copied().any(workers::worker)).then_some((
                Step::Train(UnitKind::Harvester),
                UnitKind::Harvester.stats().cost,
            ));
        }
        if !self.by_air(source, target) {
            return Some(basic);
        }
        if !self
            .observation
            .my_buildings
            .iter()
            .any(|b| b.kind == BuildingKind::Airworks && b.built)
        {
            return investments::step(
                self.observation,
                self.map,
                Investment::Tech(BuildingKind::Airworks),
            );
        }
        let builder = self
            .observation
            .my_units
            .iter()
            .filter(|u| {
                workers::worker(u.kind) && self.map.component(u.tile) == self.map.component(source)
            })
            .map(|u| usize::from(u.kind.stats().transport_size))
            .max()
            .expect("the source's ground is manned");
        let escort: usize = self
            .escorts(missions, target, source)
            .into_iter()
            .filter_map(|id| mine(self.observation, id))
            .filter(|u| u.kind.stats().domain == Domain::Ground)
            .map(|u| usize::from(u.kind.stats().transport_size))
            .sum();
        let free = missions.available(self.observation, None);
        let room = |kinds: &mut dyn Iterator<Item = UnitKind>| -> usize {
            kinds
                .map(|kind| usize::from(kind.stats().transport_capacity))
                .sum()
        };
        let have = room(
            &mut self
                .observation
                .my_units
                .iter()
                .filter(|u| carrier(u.kind) && u.cargo == 0 && free.binary_search(&u.id).is_ok())
                .map(|u| u.kind),
        );
        let coming = room(&mut self.observation.my_queues.iter().flatten().copied());
        if have + coming < builder + escort {
            return Some((
                Step::Train(UnitKind::Skyhook),
                UnitKind::Skyhook.stats().cost,
            ));
        }
        Some(basic)
    }

    fn delivery(&self) -> lift::Delivery<'_> {
        lift::Delivery::new(self.observation, self.map, self.scratch)
    }

    /// Free armed units that would meet `target`'s escort need beyond what
    /// its settlement already holds: aircraft first when the escort must fly,
    /// then the nearest to `source`, never what home keeps in reserve.
    fn escorts(&self, missions: &Missions, target: Target, source: TilePos) -> Vec<UnitId> {
        let need = self.need(target);
        if need == [0, 0] {
            return Vec::new();
        }
        let stuck = self.memory.stuck();
        let units: Vec<_> = missions
            .available(self.observation, None)
            .into_iter()
            .filter_map(|id| mine(self.observation, id))
            .filter(|u| !carrier(u.kind) && !workers::worker(u.kind) && u.kind.stats().can_fight())
            .filter(|u| {
                u.kind.stats().domain == Domain::Air
                    || self.map.component(u.tile) == self.map.component(source)
            })
            .filter(|u| !stuck.contains(&u.id))
            .collect();
        let mut units = missions
            .spare(self.observation, self.map, self.scratch.reserve)
            .outermost(self.map, self.scratch.frame, units);
        let by_air = self.by_air(source, target);
        units.sort_by_key(|u| {
            (
                by_air && u.kind.stats().domain != Domain::Air,
                self.scratch.frame.rank(doubled(source), doubled(u.tile)),
                u.id,
            )
        });
        let mut have = strength(self.observation, missions.members(target));
        fill(units, need, &mut have, usize::MAX)
    }

    /// What the selected `investment`'s escort still lacks against ground
    /// and air, counting queued units that could join it, and whether it
    /// must fly. Raises production needs; promises the settlement nothing.
    pub(crate) fn capabilities(
        &self,
        missions: &Missions,
        investment: Investment,
    ) -> Option<([u64; 2], bool)> {
        let target = self.target(missions, investment)?;
        let source = self.source(target)?;
        let by_air = self.by_air(source, target);
        let escorts = self
            .escorts(missions, target, source)
            .into_iter()
            .filter(|id| {
                !by_air
                    || mine(self.observation, *id)
                        .is_some_and(|u| u.kind.stats().domain == Domain::Air)
            });
        let mut have = strength(self.observation, escorts.chain(missions.members(target)));
        for (producer, queue) in self
            .observation
            .my_buildings
            .iter()
            .zip(&self.observation.my_queues)
        {
            let joins = |kind: &&UnitKind| {
                kind.stats().domain == Domain::Air
                    || (!by_air
                        && self.map.component(producer.anchor) == self.map.component(source))
            };
            for kind in queue.iter().filter(joins) {
                let added = force(*kind, kind.stats().max_hp);
                have = [have[0] + added[0], have[1] + added[1]];
            }
        }
        let need = self.need(target);
        Some((
            [
                need[0].saturating_sub(have[0]),
                need[1].saturating_sub(have[1]),
            ],
            by_air,
        ))
    }

    /// The escort a settlement brings: what the site's local guard will
    /// need once it stands, so the escort can stay on as that guard.
    fn need(&self, target: Target) -> [u64; 2] {
        reserve::need(
            self.observation,
            self.map,
            self.memory,
            self.profile.stance,
            target.kind(),
            target.anchor(),
        )
    }
}

impl Missions {
    /// Workers settlements hold, which ordinary worker orders leave alone.
    pub(crate) fn held_workers(&self, observation: &ObservationData) -> Vec<UnitId> {
        self.list
            .iter()
            .filter(|m| matches!(m.task, Task::Settle { .. }))
            .flat_map(|m| &m.units)
            .copied()
            .filter(|id| mine(observation, *id).is_some_and(|u| workers::worker(u.kind)))
            .collect()
    }

    /// The target of the settlement toward `investment`, if one runs.
    fn settling(&self, investment: Investment) -> Option<Target> {
        self.list.iter().find_map(|m| match m.task {
            Task::Settle { target, .. } if target.investment() == investment => Some(target),
            _ => None,
        })
    }

    /// Units the settlement toward `target` holds.
    fn members(&self, target: Target) -> impl Iterator<Item = UnitId> + '_ {
        self.list
            .iter()
            .filter(move |m| matches!(m.task, Task::Settle { target: held, .. } if held == target))
            .flat_map(|m| m.units.iter().copied())
    }

    /// Advances every settlement, returning the targets of those that failed
    /// to deliver.
    pub(crate) fn advance_settlements(
        &mut self,
        context: &Settlement<'_>,
        ledger: &mut Ledger,
    ) -> Vec<(BuildingKind, TilePos)> {
        let mut failed = Vec::new();
        for id in self.ids(|task| matches!(task, Task::Settle { .. })) {
            if let Some(index) = self.index_of(id)
                && let Some(target) = self.advance_settlement(index, context, ledger)
            {
                failed.push((target.kind(), target.anchor()));
            }
        }
        failed
    }

    /// Starts a settlement toward the `selected` saving target once its
    /// building is next and affordable: the builder boards carriers at the
    /// source, ground escorts with it, and air escorts fly out to meet it.
    pub(crate) fn settle(
        &mut self,
        context: &Settlement<'_>,
        selected: Option<Investment>,
        ledger: &mut Ledger,
    ) {
        let Some(investment) = selected else {
            return;
        };
        let Some(target) = context.target(self, investment) else {
            return;
        };
        if self.settling(investment).is_some()
            || self.list.len() >= MISSION_CAP
            || !context.next(self, investment).is_some_and(|(step, price)| {
                step == Step::Build(target.kind()) && ledger.available() >= price
            })
        {
            return;
        }
        let Some(source) = context
            .source(target)
            .filter(|s| context.by_air(*s, target))
        else {
            return;
        };
        let Some(builder) = workers::builder(
            context.observation,
            context.map,
            context.scratch.frame,
            source,
            doubled(source),
            ledger,
        ) else {
            return;
        };
        let Some(landing) = context
            .delivery()
            .landing(target.anchor(), target.kind().size(), 2)
        else {
            return;
        };
        // Only escorts that already exist are taken: a site whose need the
        // free army cannot meet waits for production instead of a promise.
        let escorts = context.escorts(self, target, source);
        let need = context.need(target);
        let have = strength(context.observation, escorts.iter().copied());
        if have[0] < need[0] || have[1] < need[1] {
            return;
        }
        let (ground, air): (Vec<_>, Vec<_>) = escorts.into_iter().partition(|id| {
            mine(context.observation, *id).is_some_and(|u| u.kind.stats().domain == Domain::Ground)
        });
        let mut payload = vec![builder];
        payload.extend(ground);
        let mut units = Vec::new();
        self.load_settlers(context, source, &payload, &mut units, ledger);
        if !units.contains(&builder) {
            return;
        }
        if !air.is_empty() && ledger.order(hunt(air.clone(), source)) {
            units.extend(air);
        }
        units.sort_unstable();
        units.dedup();
        self.list.push(Mission {
            id: self.next,
            since: context.observation.tick,
            units,
            goal: landing,
            task: Task::Settle {
                target,
                phase: SettlePhase::Load,
            },
        });
        self.next += 1;
    }

    /// Loads `riders` at `source` into free carriers or those already among
    /// `members`, adding the carriers and riders sent to `members`.
    fn load_settlers(
        &self,
        context: &Settlement<'_>,
        source: TilePos,
        riders: &[UnitId],
        members: &mut Vec<UnitId>,
        ledger: &mut Ledger,
    ) {
        let free = self.available(context.observation, None);
        let mut transports: Vec<_> = context
            .observation
            .my_units
            .iter()
            .filter(|u| {
                carrier(u.kind) && (free.binary_search(&u.id).is_ok() || members.contains(&u.id))
            })
            .collect();
        transports.sort_by_key(|u| {
            (
                context.scratch.frame.rank(doubled(source), doubled(u.tile)),
                u.id,
            )
        });
        let riders: Vec<_> = riders
            .iter()
            .filter_map(|id| mine(context.observation, *id))
            .collect();
        let sent = context.delivery().load(
            source,
            &transports,
            &riders,
            super::UNIT_CAP.saturating_sub(members.len()),
            ledger,
        );
        members.extend(sent);
    }

    /// Moves the settlement at `index` on, returning its target if it failed
    /// to deliver.
    fn advance_settlement(
        &mut self,
        index: usize,
        context: &Settlement<'_>,
        ledger: &mut Ledger,
    ) -> Option<Target> {
        let observation = context.observation;
        let Task::Settle { target, phase } = self.list[index].task else {
            unreachable!("a settlement's task");
        };
        let goal = self.list[index].goal;
        let age = observation.tick - self.list[index].since;
        if matches!(phase, SettlePhase::Load | SettlePhase::Fly)
            && phase.timeout().is_some_and(|ticks| age >= ticks)
        {
            self.settle_phase(index, SettlePhase::Recover, observation.tick);
            self.recall(index, context, ledger);
            return Some(target);
        }
        let members = self.list[index].units.clone();
        let manifest = lift::Manifest::new(observation, &members);
        match phase {
            SettlePhase::Load => return self.board_settlers(index, context, &manifest, ledger),
            SettlePhase::Fly => {
                lift::land(&manifest.carriers, goal, ledger);
                let ground = context.map.component(target.anchor());
                if manifest.aboard.is_empty()
                    && !manifest.workers.is_empty()
                    && manifest
                        .workers
                        .iter()
                        .all(|u| context.map.component(u.tile) == ground)
                {
                    let landed: Vec<_> = manifest.workers.iter().map(|u| u.id).collect();
                    self.list[index].units.retain(|id| !landed.contains(id));
                    self.settle_phase(index, SettlePhase::Found, observation.tick);
                    self.found(index, context, ledger);
                }
            }
            SettlePhase::Found => self.found(index, context, ledger),
            SettlePhase::Recover => {
                if manifest.aboard.is_empty() {
                    self.return_carriers(index, context, ledger);
                    let units = self.list[index].units.clone();
                    if units.is_empty() || ledger.order(Command::Stop { units }) {
                        self.list.remove(index);
                    }
                } else if let Some(pad) = context.delivery().pad(goal) {
                    lift::land(&manifest.carriers, pad, ledger);
                }
            }
        }
        None
    }

    /// Holds the escort at the landing until the landed builder has placed
    /// the building somewhere on that ground, or no known enemy is near it,
    /// then lets every unit go: the site's local guard takes the escorts it
    /// needs in the same decision.
    fn found(&mut self, index: usize, context: &Settlement<'_>, ledger: &mut Ledger) {
        let Task::Settle { target, .. } = self.list[index].task else {
            unreachable!("a settlement's task");
        };
        self.return_carriers(index, context, ledger);
        let ground = context.map.component(target.anchor());
        let placed = context
            .observation
            .my_buildings
            .iter()
            .any(|b| b.kind == target.kind() && context.map.component(b.anchor) == ground);
        if placed || self.list[index].units.is_empty() || context.need(target) == [0, 0] {
            self.list.remove(index);
            return;
        }
        let idle: Vec<_> = self.list[index]
            .units
            .iter()
            .filter_map(|id| mine(context.observation, *id))
            .filter(|u| u.idle)
            .map(|u| u.id)
            .collect();
        if !idle.is_empty() {
            ledger.order(hunt(idle, self.list[index].goal));
        }
    }

    /// Boards the builder and ground escort, topping both up while they
    /// load, and sends the loaded carriers with the air escort once the
    /// escort meets the need. Returns the target if the builder never
    /// boarded.
    fn board_settlers(
        &mut self,
        index: usize,
        context: &Settlement<'_>,
        manifest: &lift::Manifest<'_>,
        ledger: &mut Ledger,
    ) -> Option<Target> {
        let observation = context.observation;
        let Task::Settle { target, .. } = self.list[index].task else {
            unreachable!("a settlement's task");
        };
        let goal = self.list[index].goal;
        let members = self.list[index].units.clone();
        let carrying = manifest.aboard.iter().any(|u| workers::worker(u.kind));
        let source = manifest
            .workers
            .first()
            .map(|u| u.tile)
            .or_else(|| {
                manifest
                    .carriers
                    .iter()
                    .find(|u| u.cargo > 0)
                    .map(|u| u.tile)
            })
            .or_else(|| context.source(target));
        if let Some(source) = source {
            let (mut payload, air): (Vec<_>, Vec<_>) = context
                .escorts(self, target, source)
                .into_iter()
                .partition(|id| {
                    mine(observation, *id).is_some_and(|u| u.kind.stats().domain == Domain::Ground)
                });
            if !carrying
                && manifest.workers.is_empty()
                && let Some(builder) = workers::builder(
                    observation,
                    context.map,
                    context.scratch.frame,
                    source,
                    doubled(source),
                    ledger,
                )
            {
                payload.push(builder);
            }
            let mut extra = members.clone();
            self.load_settlers(context, source, &payload, &mut extra, ledger);
            if !air.is_empty() && ledger.order(hunt(air.clone(), source)) {
                extra.extend(air);
            }
            for unit in extra {
                super::insert(&mut self.list[index].units, unit);
            }
            if self.list[index].units != members {
                return None;
            }
        }
        let riders = &manifest.riders;
        if carrying && !riders.is_empty() && riders.iter().all(|u| u.idle) {
            // A rider standing idle beside loaded carriers will not board:
            // it goes back to ordinary work and the rest fly without it.
            let grounded: Vec<_> = riders.iter().map(|u| u.id).collect();
            if ledger.order(Command::Stop {
                units: grounded.clone(),
            }) {
                self.list[index].units.retain(|id| !grounded.contains(id));
            }
            return None;
        }
        let need = context.need(target);
        let force = strength(
            observation,
            manifest
                .escorts
                .iter()
                .map(|u| u.id)
                .chain(manifest.aboard.iter().map(|u| u.id)),
        );
        if carrying && riders.is_empty() && force[0] >= need[0] && force[1] >= need[1] {
            let mut flying: Vec<_> = manifest
                .carriers
                .iter()
                .filter(|u| u.cargo > 0)
                .map(|u| u.id)
                .collect();
            flying.extend(
                manifest
                    .escorts
                    .iter()
                    .filter(|u| u.kind.stats().domain == Domain::Air)
                    .map(|u| u.id),
            );
            let via = manifest.carriers.first().and_then(|unit| {
                air::route(
                    observation,
                    context.scratch.frame,
                    &context.scratch.air,
                    unit.tile,
                    goal,
                )
            });
            // A gunship flies on without stopping to face what its guns
            // reach; they fire on the way regardless. When the decision's
            // orders cannot send it and the rest apart, all fly together
            // without engaging.
            let (gunships, others): (Vec<UnitId>, Vec<UnitId>) = flying.iter().partition(|id| {
                mine(observation, **id).is_some_and(|u| crate::gunship::gunship(u.kind))
            });
            let legs = if gunships.is_empty() {
                Legs::Hunt
            } else {
                Legs::Run
            };
            let sent = if gunships.is_empty() || others.is_empty() {
                air::fly(flying, via, goal, legs, ledger)
            } else if ledger.room() >= 2 * air::orders(via) {
                air::fly(gunships, via, goal, Legs::Run, ledger);
                air::fly(others, via, goal, Legs::Hunt, ledger);
                true
            } else {
                air::fly(flying, via, goal, Legs::Run, ledger)
            };
            if sent {
                self.settle_phase(index, SettlePhase::Fly, observation.tick);
            }
        } else if !carrying && riders.iter().all(|u| u.idle) {
            self.settle_phase(index, SettlePhase::Recover, observation.tick);
            return Some(target);
        }
        None
    }

    /// Turns the settlement's loaded carriers for a home pad, around known
    /// anti-air.
    fn recall(&self, index: usize, context: &Settlement<'_>, ledger: &mut Ledger) {
        let manifest = lift::Manifest::new(context.observation, &self.list[index].units);
        let loaded: Vec<_> = manifest.carriers.iter().filter(|u| u.cargo > 0).collect();
        let Some(first) = loaded.first() else {
            return;
        };
        let Some(pad) = context.delivery().pad(self.list[index].goal) else {
            return;
        };
        air::fly_around(
            context.observation,
            context.scratch.frame,
            &context.scratch.air,
            loaded.iter().map(|u| u.id).collect(),
            first.tile,
            pad,
            Legs::Run,
            ledger,
        );
    }

    fn settle_phase(&mut self, index: usize, phase: SettlePhase, now: u64) {
        let Task::Settle { target, .. } = self.list[index].task else {
            unreachable!("a settlement's task");
        };
        self.list[index].task = Task::Settle { target, phase };
        self.list[index].since = now;
    }

    /// Flies the settlement's empty carriers to a home pad and lets them go.
    fn return_carriers(&mut self, index: usize, context: &Settlement<'_>, ledger: &mut Ledger) {
        let manifest = lift::Manifest::new(context.observation, &self.list[index].units);
        let returned = context.delivery().return_empty(&manifest.carriers, ledger);
        self.list[index].units.retain(|id| !returned.contains(id));
    }
}
