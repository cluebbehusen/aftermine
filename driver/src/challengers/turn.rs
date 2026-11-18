//! One decision of a challenger: what its seat knows right now, the scrap it
//! may still spend, and the commands it has issued so far, with the shared
//! skills every plan uses: running an economy, placing structures, training,
//! and sending groups to fight.

use super::map::MapFacts;
use aftermine_sim::observation::{BuildingObs, ObservationData, UnitObs};
use aftermine_sim::stats::Domain;
use aftermine_sim::{
    AttackTarget, BuildingId, BuildingKind, Command, PlayerId, Rally, RememberedBuilding, State,
    UnitId, UnitKind,
};
use chassis::grid::TilePos;
use std::collections::{BTreeMap, BTreeSet};

/// The most commands one decision issues: about what a quick player does in
/// half a second.
pub(super) const COMMANDS_PER_DECISION: usize = 6;

/// How close, in tiles, a visible enemy must be to a group for it to fight.
pub(super) const ENGAGE_REACH: i32 = 8;

/// What a unit was last told, so a plan can repeat itself every decision
/// without reissuing an order the unit is already carrying out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Intent {
    Hunt(TilePos),
    Run(TilePos),
    Attack(AttackTarget),
    Harvest(TilePos),
    Scout(TilePos),
    Accompany(UnitId),
    Build,
}

/// The orders a challenger has given its units, by unit.
#[derive(Debug, Clone, Default)]
pub(super) struct Orders {
    last: BTreeMap<UnitId, (Intent, u64)>,
    rallies: BTreeMap<BuildingId, TilePos>,
}

impl Orders {
    /// Forgets units and buildings the seat no longer has.
    pub(super) fn prune(&mut self, obs: &ObservationData) {
        self.last
            .retain(|id, _| obs.my_units.iter().any(|unit| unit.id == *id));
        self.rallies
            .retain(|id, _| obs.my_buildings.iter().any(|b| b.id == *id));
    }

    /// The order last given to `unit`.
    pub(super) fn intent(&self, unit: UnitId) -> Option<Intent> {
        self.last.get(&unit).map(|(intent, _)| *intent)
    }

    /// The tick `unit` was last given an order.
    pub(super) fn ordered_at(&self, unit: UnitId) -> Option<u64> {
        self.last.get(&unit).map(|(_, at)| *at)
    }
}

/// How an attempt to place a structure went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Build {
    /// The site is down.
    Placed,
    /// A site and a builder are ready, but the bank is short of this price.
    Short(u32),
    /// No site or builder.
    NoSite,
}

/// What a group should prefer to shoot when nothing shoots back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Prey {
    /// Workers first, then structures.
    Workers,
    /// Armed structures first, then workers, then other structures.
    Defenses,
    /// Extractors first, then workers, then other structures.
    Extractors,
    /// Foundries first, then armed structures, then the rest.
    Foundry,
}

/// One decision in progress.
pub(super) struct Turn<'a> {
    state: &'a State,
    pub(super) obs: &'a ObservationData,
    pub(super) map: &'a MapFacts,
    pub(super) me: PlayerId,
    pub(super) tick: u64,
    bank: u32,
    reserved: u32,
    commands: Vec<Command>,
    ordered: BTreeSet<UnitId>,
    kept: BTreeSet<UnitId>,
    orders: &'a mut Orders,
}

impl<'a> Turn<'a> {
    pub(super) fn new(
        state: &'a State,
        obs: &'a ObservationData,
        map: &'a MapFacts,
        orders: &'a mut Orders,
    ) -> Self {
        orders.prune(obs);
        Self {
            state,
            obs,
            map,
            me: obs.me,
            tick: obs.tick,
            bank: obs.scrap,
            reserved: 0,
            commands: Vec::new(),
            ordered: BTreeSet::new(),
            kept: BTreeSet::new(),
            orders,
        }
    }

    pub(super) fn finish(self) -> Vec<Command> {
        self.commands
    }

    /// Whether this decision may issue another command.
    pub(super) fn room(&self) -> bool {
        self.commands.len() < COMMANDS_PER_DECISION
    }

    fn push(&mut self, command: Command) -> bool {
        if !self.room() {
            return false;
        }
        self.commands.push(command);
        true
    }

    /// Scrap left to spend after what this decision has bought or set aside.
    pub(super) fn spendable(&self) -> u32 {
        self.bank.saturating_sub(self.reserved)
    }

    /// Sets `amount` aside for something worth more than what follows.
    pub(super) fn reserve(&mut self, amount: u32) {
        self.reserved = self.reserved.saturating_add(amount);
    }

    // ---- What the seat knows --------------------------------------------

    /// Own units of `kind`.
    pub(super) fn units(&self, kind: UnitKind) -> impl Iterator<Item = &'a UnitObs> + use<'a> {
        self.obs.my_units.iter().filter(move |u| u.kind == kind)
    }

    /// Own units of `kind` plus those queued at own producers.
    pub(super) fn count(&self, kind: UnitKind) -> u32 {
        let queued = self
            .obs
            .my_queues
            .iter()
            .flatten()
            .filter(|queued| **queued == kind)
            .count();
        u32::try_from(self.units(kind).count() + queued).unwrap_or(u32::MAX)
    }

    /// Own structures of `kind`, sites included.
    pub(super) fn structures(
        &self,
        kind: BuildingKind,
    ) -> impl Iterator<Item = &'a BuildingObs> + use<'a> {
        self.obs.my_buildings.iter().filter(move |b| b.kind == kind)
    }

    /// Own finished structures of `kind`.
    pub(super) fn finished(
        &self,
        kind: BuildingKind,
    ) -> impl Iterator<Item = &'a BuildingObs> + use<'a> {
        self.structures(kind).filter(|b| b.built)
    }

    /// Whether the seat has a finished structure of `kind`.
    pub(super) fn has(&self, kind: BuildingKind) -> bool {
        self.finished(kind).next().is_some()
    }

    /// Own armed units that are not workers, scouts or transports.
    pub(super) fn army(&self) -> Vec<&'a UnitObs> {
        self.obs
            .my_units
            .iter()
            .filter(|u| fighter(u.kind))
            .collect()
    }

    /// The own finished Foundry nearest `tile`.
    pub(super) fn nearest_foundry(&self, tile: TilePos) -> Option<&'a BuildingObs> {
        self.finished(BuildingKind::Foundry)
            .min_by_key(|b| (footprint_distance(b, tile), b.id))
    }

    /// The tile the seat defends: its first Foundry, or where it started.
    pub(super) fn home(&self) -> TilePos {
        self.nearest_foundry(self.map.home_centre())
            .map_or(self.map.home_centre(), |b| b.anchor.offset(1, 1))
    }

    /// Visible enemy units that fight, within `reach` of `tile`, nearest
    /// first.
    pub(super) fn threats_near(&self, tile: TilePos, reach: i32) -> Vec<&'a UnitObs> {
        let mut near: Vec<&UnitObs> = self
            .obs
            .enemy_units
            .iter()
            .filter(|u| fighter(u.kind) && u.tile.chebyshev(tile) <= reach)
            .collect();
        near.sort_by_key(|u| (u.tile.chebyshev(tile), u.id));
        near
    }

    /// Whether a visible armed enemy unit or structure stands within `reach`
    /// of `tile`.
    pub(super) fn danger_at(&self, tile: TilePos, reach: i32) -> bool {
        !self.threats_near(tile, reach).is_empty()
            || self
                .obs
                .enemy_buildings
                .iter()
                .any(|b| b.kind.fires() && b.hp > 0 && footprint_distance(b, tile) <= reach)
    }

    /// Known enemy structures of `kind`, live or remembered.
    pub(super) fn enemy_structures(
        &self,
        kind: BuildingKind,
    ) -> impl Iterator<Item = &'a BuildingObs> + use<'a> {
        self.obs
            .enemy_buildings
            .iter()
            .filter(move |b| b.kind == kind && b.hp > 0)
    }

    /// Keeps `unit` out of the shared worker pool for this decision: the
    /// plan has its own use for it.
    pub(super) fn keep(&mut self, unit: UnitId) {
        self.kept.insert(unit);
    }

    /// Whether `unit` already has an order from this decision.
    pub(super) fn busy(&self, unit: UnitId) -> bool {
        self.ordered.contains(&unit)
    }

    /// The order last given to `unit`, from any decision.
    pub(super) fn intent(&self, unit: UnitId) -> Option<Intent> {
        self.orders.intent(unit)
    }

    // ---- Orders -----------------------------------------------------------

    /// Gives `units` an order unless each is already carrying it out:
    /// told within `refresh` ticks and not idle since.
    fn order(&mut self, units: &[UnitId], intent: Intent, refresh: u64) -> bool {
        let fresh: Vec<UnitId> = units
            .iter()
            .copied()
            .filter(|id| !self.ordered.contains(id))
            .filter(|id| match self.orders.last.get(id) {
                Some((last, at)) if *last == intent => {
                    let idle = self.obs.my_units.iter().any(|u| u.id == *id && u.idle);
                    self.tick >= at + refresh || (idle && self.tick >= at + 40)
                }
                _ => true,
            })
            .collect();
        if fresh.is_empty() || !self.room() {
            return false;
        }
        let command = match intent {
            Intent::Hunt(goal) => Command::Hunt {
                units: fresh.clone(),
                goal,
                queue: false,
            },
            Intent::Run(goal) => Command::Run {
                units: fresh.clone(),
                goal,
                queue: false,
            },
            Intent::Attack(target) => Command::Attack {
                units: fresh.clone(),
                target,
                queue: false,
            },
            Intent::Harvest(node) => Command::Harvest {
                units: fresh.clone(),
                node,
                queue: false,
            },
            Intent::Scout(goal) => Command::Scout {
                units: fresh.clone(),
                goal,
                queue: false,
            },
            Intent::Accompany(target) => Command::Accompany {
                units: fresh.clone(),
                target,
                queue: false,
            },
            Intent::Build => return false,
        };
        self.push(command);
        for id in fresh {
            self.ordered.insert(id);
            self.orders.last.insert(id, (intent, self.tick));
        }
        true
    }

    pub(super) fn hunt(&mut self, units: &[UnitId], goal: TilePos) -> bool {
        self.order(units, Intent::Hunt(goal), 300)
    }

    pub(super) fn run(&mut self, units: &[UnitId], goal: TilePos) -> bool {
        self.order(units, Intent::Run(goal), 300)
    }

    pub(super) fn attack(&mut self, units: &[UnitId], target: AttackTarget) -> bool {
        self.order(units, Intent::Attack(target), 400)
    }

    pub(super) fn scout(&mut self, units: &[UnitId], goal: TilePos) -> bool {
        self.order(units, Intent::Scout(goal), 600)
    }

    pub(super) fn accompany(&mut self, units: &[UnitId], target: UnitId) -> bool {
        self.order(units, Intent::Accompany(target), 600)
    }

    /// Points `building`'s new units at `tile`, once.
    pub(super) fn rally(&mut self, building: BuildingId, tile: TilePos) -> bool {
        if self.orders.rallies.get(&building) == Some(&tile) || !self.room() {
            return false;
        }
        self.orders.rallies.insert(building, tile);
        self.push(Command::SetRally {
            building,
            rally: Some(Rally::Tile(tile)),
        })
    }

    // ---- Production ---------------------------------------------------------

    /// Queues `kind` at the own finished producer of it nearest `near`
    /// whose queue is shorter than `queue`, when the seat can pay and meets
    /// the prerequisites.
    pub(super) fn train(&mut self, kind: UnitKind, near: TilePos, queue: usize) -> bool {
        match self.producer(kind, near, queue) {
            Some(building) if self.spendable() >= kind.stats().cost && self.room() => {
                self.bank -= kind.stats().cost;
                self.push(Command::Train { building, kind })
            }
            _ => false,
        }
    }

    /// Trains `kind` like [`Turn::train`], or, when a producer is free but
    /// the bank is short, sets its price aside so nothing after takes it.
    pub(super) fn produce(&mut self, kind: UnitKind, near: TilePos, queue: usize) -> bool {
        if self.producer(kind, near, queue).is_none() {
            return false;
        }
        if self.train(kind, near, queue) {
            return true;
        }
        self.reserve(kind.stats().cost);
        false
    }

    /// Whether the seat has a finished producer of `kind` and meets its
    /// prerequisites.
    pub(super) fn can_train(&self, kind: UnitKind) -> bool {
        self.producer(kind, self.home(), usize::MAX).is_some()
    }

    /// The own finished producer of `kind` nearest `near` with fewer than
    /// `queue` queued, when the seat meets the prerequisites.
    fn producer(&self, kind: UnitKind, near: TilePos, queue: usize) -> Option<BuildingId> {
        if !self.state.requirements_met(self.me, kind.stats().requires) {
            return None;
        }
        self.obs
            .my_buildings
            .iter()
            .zip(&self.obs.my_queues)
            .filter(|(b, queued)| {
                b.built
                    && !b.provisional
                    && b.kind.tier_stats(b.tier).produces.contains(&kind)
                    && queued.len() < queue
            })
            .min_by_key(|(b, queued)| (footprint_distance(b, near), queued.len(), b.id))
            .map(|(b, _)| b.id)
    }

    /// Places `kind` on the free ground nearest `near`, within `radius`,
    /// with `builder` or else the free worker nearest it, when the seat can
    /// pay and meets the prerequisites.
    pub(super) fn build(
        &mut self,
        kind: BuildingKind,
        near: TilePos,
        radius: i32,
        builder: Option<UnitId>,
    ) -> Build {
        let cost = kind.base_stats().construction.cost;
        if !self.room() || !self.state.prerequisites_met(self.me, kind) {
            return Build::NoSite;
        }
        let Some(builder) = builder.or_else(|| self.free_worker(near)) else {
            return Build::NoSite;
        };
        let Some((anchor, defer)) = self.site(kind, near, radius, builder) else {
            return Build::NoSite;
        };
        if self.spendable() < cost {
            return Build::Short(cost);
        }
        self.place(kind, anchor, defer, builder, cost);
        Build::Placed
    }

    /// Builds `kind` near `near` unless the seat has one, site included;
    /// short of scrap, sets its price aside so nothing after takes it.
    pub(super) fn ensure(&mut self, kind: BuildingKind, near: TilePos, radius: i32) -> bool {
        if self.structures(kind).next().is_some() {
            return true;
        }
        if let Build::Short(cost) = self.build(kind, near, radius, None) {
            self.reserve(cost);
        }
        false
    }

    /// Builds another Foundry, up to `foundries`, beside the cluster of
    /// known free frames on the seat's half of the map that no own Foundry
    /// supports yet, so Extractors there pay in full and workers unload
    /// close by. One site at a time; short of scrap, it saves.
    pub(super) fn expand(&mut self, foundries: u32) -> bool {
        let kind = BuildingKind::Foundry;
        let own = u32::try_from(self.structures(kind).count()).unwrap_or(u32::MAX);
        if own >= foundries
            || self.structures(kind).any(|b| !b.built)
            || !self.state.prerequisites_met(self.me, kind)
        {
            return false;
        }
        let half = self.map.route_length() / 2 + i32::try_from(own).unwrap_or(0) * 2 - 4;
        let free: Vec<TilePos> = self
            .obs
            .known_frames
            .iter()
            .copied()
            .filter(|&frame| !self.state.extractor_frame_claim_known(self.me, frame))
            .filter(|&frame| self.map.steps_from_home(frame) <= half)
            .filter(|&frame| !self.danger_at(frame.offset(1, 1), 8))
            .filter(|&frame| {
                self.structures(kind)
                    .all(|f| footprint_distance(f, frame.offset(1, 1)) > 9)
            })
            .collect();
        let best = free
            .iter()
            .map(|&frame| {
                let cluster = free.iter().filter(|f| f.chebyshev(frame) <= 6).count();
                let cluster = i32::try_from(cluster).unwrap_or(0);
                (-cluster, self.map.steps_from_home(frame), frame)
            })
            .min_by_key(|&(cluster, steps, frame)| (cluster, steps, frame.y, frame.x));
        let Some((_, _, frame)) = best else {
            return false;
        };
        let toward_home = if self.map.home.x < frame.x { -3 } else { 3 };
        match self.build(kind, frame.offset(toward_home, 0), 5, None) {
            Build::Placed => true,
            Build::Short(cost) => {
                self.reserve(cost);
                false
            }
            Build::NoSite => false,
        }
    }

    /// Restores an Extractor on the known free frame nearest an own Foundry,
    /// within `reach` tiles of one and away from visible danger.
    pub(super) fn build_extractor(&mut self, reach: i32) -> Build {
        let kind = BuildingKind::Extractor;
        let cost = kind.base_stats().construction.cost;
        if !self.room() {
            return Build::NoSite;
        }
        let home = self.home();
        let mut frames: Vec<(i32, TilePos)> = self
            .obs
            .known_frames
            .iter()
            .copied()
            .filter(|&frame| !self.state.extractor_frame_claim_known(self.me, frame))
            .filter(|&frame| !self.danger_at(frame.offset(1, 1), 7))
            .map(|frame| {
                let steps = self
                    .obs
                    .my_buildings
                    .iter()
                    .filter(|b| b.kind == BuildingKind::Foundry && b.built)
                    .map(|b| b.anchor.offset(1, 1).chebyshev(frame.offset(1, 1)))
                    .min()
                    .unwrap_or_else(|| home.chebyshev(frame));
                (steps, frame)
            })
            .filter(|(steps, _)| *steps <= reach)
            .collect();
        frames.sort_unstable_by_key(|(steps, frame)| (*steps, frame.y, frame.x));
        for (_, frame) in frames {
            let Some(builder) = self.free_worker(frame) else {
                return Build::NoSite;
            };
            let visible = footprint(frame, kind.size()).all(|t| self.obs.visible(t));
            let refusal = if visible {
                self.state.place_refusal(self.me, kind, frame)
            } else {
                self.state
                    .place_intent_refusal(self.me, kind, frame, &[builder])
            };
            if refusal.is_none() {
                if self.spendable() < cost {
                    return Build::Short(cost);
                }
                self.place(kind, frame, !visible, builder, cost);
                return Build::Placed;
            }
        }
        Build::NoSite
    }

    /// Sends `builders` back to work on the own site of `kind` at `anchor`.
    pub(super) fn resume(
        &mut self,
        kind: BuildingKind,
        anchor: TilePos,
        builders: &[UnitId],
    ) -> bool {
        let builders: Vec<UnitId> = builders
            .iter()
            .copied()
            .filter(|id| !self.busy(*id))
            .collect();
        if builders.is_empty() || !self.room() {
            return false;
        }
        self.push(Command::Build {
            units: builders.clone(),
            kind,
            anchor,
            queue: false,
            defer: false,
        });
        for id in builders {
            self.ordered.insert(id);
            self.orders.last.insert(id, (Intent::Build, self.tick));
        }
        true
    }

    /// Gives every own site nobody is working on the free worker nearest
    /// it.
    pub(super) fn staff_sites(&mut self) {
        let idle_sites: Vec<(BuildingKind, TilePos)> = self
            .obs
            .my_buildings
            .iter()
            .filter(|b| !b.built && b.kind.assembles().is_none())
            .filter(|b| !self.obs.my_units.iter().any(|u| u.site == Some(b.id)))
            .map(|b| (b.kind, b.anchor))
            .collect();
        for (kind, anchor) in idle_sites {
            if let Some(worker) = self.free_worker(anchor) {
                self.resume(kind, anchor, &[worker]);
            }
        }
    }

    fn place(
        &mut self,
        kind: BuildingKind,
        anchor: TilePos,
        defer: bool,
        builder: UnitId,
        cost: u32,
    ) {
        self.bank -= cost;
        self.push(Command::Build {
            units: vec![builder],
            kind,
            anchor,
            queue: false,
            defer,
        });
        self.ordered.insert(builder);
        self.orders.last.insert(builder, (Intent::Build, self.tick));
    }

    /// The free ground for `kind` nearest `near` within `radius` that keeps
    /// a walkable ring around it, and whether its placement must wait for
    /// the builder to see it.
    fn site(
        &self,
        kind: BuildingKind,
        near: TilePos,
        radius: i32,
        builder: UnitId,
    ) -> Option<(TilePos, bool)> {
        let size = kind.size();
        for ring in 0..=radius {
            for dy in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs().max(dy.abs()) != ring {
                        continue;
                    }
                    let anchor = near.offset(dx, dy);
                    if !self.clear_ring(anchor, size) {
                        continue;
                    }
                    let tiles: Vec<TilePos> = footprint(anchor, size).collect();
                    if tiles.iter().all(|&t| self.obs.visible(t)) {
                        if self.state.place_refusal(self.me, kind, anchor).is_none() {
                            return Some((anchor, false));
                        }
                    } else if tiles.iter().all(|&t| self.obs.explored(t))
                        && self
                            .state
                            .place_intent_refusal(self.me, kind, anchor, &[builder])
                            .is_none()
                    {
                        return Some((anchor, true));
                    }
                }
            }
        }
        None
    }

    /// Whether the tiles around a footprint are open authored ground clear
    /// of own structures and frames, so a new structure walls nothing in.
    fn clear_ring(&self, anchor: TilePos, (w, h): (i32, i32)) -> bool {
        for y in anchor.y - 1..=anchor.y + h {
            for x in anchor.x - 1..=anchor.x + w {
                let tile = TilePos::new(x, y);
                let inside = x >= anchor.x && x < anchor.x + w && y >= anchor.y && y < anchor.y + h;
                if inside {
                    continue;
                }
                if !self.map.open(tile)
                    || self.map.frames.iter().any(|f| {
                        tile.x >= f.x && tile.x < f.x + 2 && tile.y >= f.y && tile.y < f.y + 2
                    })
                    || self
                        .obs
                        .my_buildings
                        .iter()
                        .any(|b| footprint_distance(b, tile) == 0)
                {
                    return false;
                }
            }
        }
        true
    }

    /// The own worker nearest `tile` that is free for new work: not
    /// building, not ordered this decision.
    pub(super) fn free_worker(&self, tile: TilePos) -> Option<UnitId> {
        self.units(UnitKind::Harvester)
            .chain(self.units(UnitKind::Excavator))
            .filter(|u| u.site.is_none() && !self.busy(u.id) && !self.kept.contains(&u.id))
            .filter(|u| self.intent(u.id) != Some(Intent::Build) || u.idle)
            .min_by_key(|u| (u.tile.chebyshev(tile), u.id))
            .map(|u| u.id)
    }

    // ---- Economy ------------------------------------------------------------

    /// Sends idle workers to the known scrap or wreck nearest an own
    /// Foundry, away from visible danger, spreading them over the nodes;
    /// when nothing is left near a Foundry, to the nearest salvage farther
    /// out.
    pub(super) fn harvest_idle(&mut self) {
        let idle: Vec<&UnitObs> = self
            .units(UnitKind::Harvester)
            .chain(self.units(UnitKind::Excavator))
            .filter(|u| {
                u.idle && u.site.is_none() && !self.busy(u.id) && !self.kept.contains(&u.id)
            })
            .collect();
        if idle.is_empty() {
            return;
        }
        let salvage: Vec<(i32, TilePos)> = self
            .obs
            .known_scrap
            .iter()
            .chain(
                self.obs
                    .known_wrecks
                    .iter()
                    .filter(|(_, amount)| *amount >= 5),
            )
            .filter(|(_, amount)| *amount > 0)
            .filter_map(|&(node, _)| {
                let foundry = self.nearest_foundry(node)?;
                let distance = footprint_distance(foundry, node);
                let wreck = !self.obs.known_scrap_at(node);
                let reach = if wreck { 10 } else { 20 };
                let ours = self.map.steps_from_home(node) <= self.map.route_length() / 2;
                (distance <= reach && (distance <= 12 || ours) && !self.danger_at(node, 7))
                    .then_some((distance, node))
            })
            .collect();
        let near = salvage.iter().any(|(distance, _)| *distance <= 12);
        let salvage: Vec<(i32, TilePos)> = salvage
            .into_iter()
            .filter(|(distance, _)| !near || *distance <= 12)
            .collect();
        let mut assigned: Vec<TilePos> = self
            .obs
            .my_units
            .iter()
            .filter_map(|u| u.harvesting)
            .collect();
        let mut plan: BTreeMap<TilePos, Vec<UnitId>> = BTreeMap::new();
        for worker in idle {
            let best = salvage
                .iter()
                .map(|&(distance, node)| {
                    let crowd = assigned.iter().filter(|a| **a == node).count();
                    let crowd = i32::try_from(crowd).unwrap_or(i32::MAX);
                    (distance + 3 * crowd + worker.tile.chebyshev(node) / 4, node)
                })
                .min_by_key(|(score, node)| (*score, node.y, node.x));
            if let Some((_, node)) = best {
                assigned.push(node);
                plan.entry(node).or_default().push(worker.id);
            }
        }
        for (node, workers) in plan {
            self.order(&workers, Intent::Harvest(node), 600);
        }
    }

    /// Keeps the seat at `workers` Harvesters, trained one at a time.
    /// Workers out of work hold training back.
    pub(super) fn train_workers(&mut self, workers: u32) -> bool {
        let idle = self
            .units(UnitKind::Harvester)
            .filter(|u| u.idle && u.site.is_none())
            .count();
        if self.count(UnitKind::Harvester) >= workers || idle >= 3 {
            return false;
        }
        let home = self.home();
        self.train(UnitKind::Harvester, home, 1)
    }

    /// Own Extractors, sites included.
    pub(super) fn extractors(&self) -> u32 {
        u32::try_from(self.structures(BuildingKind::Extractor).count()).unwrap_or(u32::MAX)
    }

    /// The groundwork of every economy: sites get builders and Extractors
    /// go up on known frames within `reach` up to `extractors`, one site at
    /// a time, ahead of anything bought after.
    pub(super) fn economy(&mut self, extractors: u32, reach: i32) {
        self.staff_sites();
        let site_open = self.structures(BuildingKind::Extractor).any(|b| !b.built);
        if self.extractors() < extractors
            && !site_open
            && let Build::Short(cost) = self.build_extractor(reach)
        {
            self.reserve(cost);
        }
    }

    /// Sends the seat's idle Surveyors through `sites` in turn with the
    /// Scout order, which keeps clear of known enemy reach: each goes on to
    /// the site after the one it last scouted.
    pub(super) fn scout_sites(&mut self, sites: &[TilePos], offset: usize) {
        if sites.is_empty() {
            return;
        }
        let scouts: Vec<UnitId> = self
            .units(UnitKind::Surveyor)
            .filter(|u| u.idle && !self.busy(u.id))
            .filter(|u| {
                self.orders
                    .ordered_at(u.id)
                    .is_none_or(|at| self.tick >= at + 200)
            })
            .map(|u| u.id)
            .collect();
        for (i, scout) in scouts.into_iter().enumerate() {
            let next = match self.intent(scout) {
                Some(Intent::Scout(last)) => sites
                    .iter()
                    .position(|site| *site == last)
                    .map_or(0, |at| at + 1),
                _ => offset + i,
            };
            self.scout(&[scout], sites[next % sites.len()]);
        }
    }

    // ---- Fighting -----------------------------------------------------------

    /// Sends `units` at the best visible prey near their centre, fighting
    /// armed enemies first; with nothing near, marches them on to
    /// `objective`. Returns whether anything near was worth a shot.
    pub(super) fn engage(&mut self, units: &[UnitId], objective: TilePos, prey: Prey) -> bool {
        let Some(centre) = self.centre(units) else {
            return false;
        };
        let (ground, air) = self.reach_of(units);
        let threat = self
            .threats_near(centre, ENGAGE_REACH)
            .into_iter()
            .find(|u| match u.body_domain() {
                Domain::Ground => ground,
                Domain::Air => air,
            });
        if let Some(threat) = threat {
            let tile = threat.tile;
            self.hunt(units, tile);
            return true;
        }
        if let Some(target) = self.prey_near(centre, ENGAGE_REACH, prey, ground) {
            self.attack(units, target);
            return true;
        }
        self.hunt(units, objective);
        false
    }

    /// The best target near `centre` that is not a fighting unit.
    pub(super) fn prey_near(
        &self,
        centre: TilePos,
        reach: i32,
        prey: Prey,
        ground: bool,
    ) -> Option<AttackTarget> {
        if !ground {
            return None;
        }
        let worker = self
            .obs
            .enemy_units
            .iter()
            .filter(|u| {
                matches!(
                    u.kind,
                    UnitKind::Harvester | UnitKind::Excavator | UnitKind::Surveyor
                ) && u.tile.chebyshev(centre) <= reach
            })
            .min_by_key(|u| (u.tile.chebyshev(centre), u.hp, u.id))
            .map(|u| AttackTarget::Unit(u.id));
        let building = |filter: &dyn Fn(&BuildingObs) -> bool| {
            self.obs
                .enemy_buildings
                .iter()
                .filter(|b| b.hp > 0 && !b.provisional && footprint_distance(b, centre) <= reach)
                .filter(|b| filter(b))
                .min_by_key(|b| (footprint_distance(b, centre), b.anchor.y, b.anchor.x))
                .map(building_target)
        };
        let armed = || building(&|b| b.kind.fires());
        let any = || building(&|b| b.kind != BuildingKind::ScuttleCharge);
        match prey {
            Prey::Workers => worker.or_else(armed).or_else(any),
            Prey::Defenses => armed().or(worker).or_else(any),
            Prey::Extractors => building(&|b| b.kind == BuildingKind::Extractor)
                .or(worker)
                .or_else(armed)
                .or_else(any),
            Prey::Foundry => building(&|b| b.kind == BuildingKind::Foundry)
                .or_else(armed)
                .or(worker)
                .or_else(any),
        }
    }

    /// Hunts, with `units`, the nearest visible enemy within `reach` of an
    /// own structure or worker that they can shoot. Returns whether there
    /// was one.
    pub(super) fn defend(&mut self, units: &[UnitId], reach: i32) -> bool {
        match self.intruder(units, reach) {
            Some(tile) => {
                self.hunt(units, tile);
                true
            }
            None => false,
        }
    }

    /// Where the nearest enemy `units` can shoot stands, within `reach` of
    /// an own structure or worker.
    pub(super) fn intruder(&self, units: &[UnitId], reach: i32) -> Option<TilePos> {
        if units.is_empty() {
            return None;
        }
        let (ground, air) = self.reach_of(units);
        let workers: Vec<TilePos> = self
            .obs
            .my_units
            .iter()
            .filter(|u| u.kind.stats().harvest.is_some())
            .map(|u| u.tile)
            .filter(|&tile| {
                self.nearest_foundry(tile)
                    .is_some_and(|f| footprint_distance(f, tile) <= 12)
            })
            .collect();
        self.obs
            .enemy_units
            .iter()
            .filter(|u| match u.body_domain() {
                Domain::Ground => ground,
                Domain::Air => air,
            })
            .filter_map(|u| {
                let distance = self
                    .obs
                    .my_buildings
                    .iter()
                    .map(|b| footprint_distance(b, u.tile))
                    .chain(workers.iter().map(|w| w.chebyshev(u.tile)))
                    .min()?;
                (distance <= reach).then_some((distance, u))
            })
            .min_by_key(|(distance, u)| (*distance, u.id))
            .map(|(_, u)| u.tile)
    }

    /// The mean tile of `units`.
    pub(super) fn centre(&self, units: &[UnitId]) -> Option<TilePos> {
        let tiles: Vec<TilePos> = self
            .obs
            .my_units
            .iter()
            .filter(|u| units.contains(&u.id))
            .map(|u| u.tile)
            .collect();
        let n = i32::try_from(tiles.len()).ok().filter(|n| *n > 0)?;
        let (x, y) = tiles.iter().fold((0, 0), |(x, y), t| (x + t.x, y + t.y));
        Some(TilePos::new(x / n, y / n))
    }

    /// Whether any of `units` can shoot ground, and whether any can shoot
    /// air.
    pub(super) fn reach_of(&self, units: &[UnitId]) -> (bool, bool) {
        let kinds = self
            .obs
            .my_units
            .iter()
            .filter(|u| units.contains(&u.id))
            .map(|u| u.kind.stats());
        kinds.fold((false, false), |(ground, air), stats| {
            (
                ground || stats.can_target(Domain::Ground) || stats.demolition.is_some(),
                air || stats.can_target(Domain::Air),
            )
        })
    }
}

/// Whether `kind` is an armed unit that fights: not a worker, scout or
/// transport.
pub(super) fn fighter(kind: UnitKind) -> bool {
    let stats = kind.stats();
    stats.harvest.is_none() && (stats.can_fight() || stats.demolition.is_some())
}

/// The attack target naming a known enemy structure.
pub(super) fn building_target(b: &BuildingObs) -> AttackTarget {
    if b.seen {
        AttackTarget::Building(b.id)
    } else {
        AttackTarget::RememberedBuilding(RememberedBuilding {
            owner: b.player,
            building_kind: b.kind,
            anchor: b.anchor,
        })
    }
}

/// The tiles of a footprint.
pub(super) fn footprint(anchor: TilePos, (w, h): (i32, i32)) -> impl Iterator<Item = TilePos> {
    (0..h).flat_map(move |dy| (0..w).map(move |dx| anchor.offset(dx, dy)))
}

/// Chebyshev tiles from a structure's footprint to `tile`; zero inside it.
pub(super) fn footprint_distance(b: &BuildingObs, tile: TilePos) -> i32 {
    let (w, h) = b.kind.size();
    let dx = (b.anchor.x - tile.x)
        .max(tile.x - (b.anchor.x + w - 1))
        .max(0);
    let dy = (b.anchor.y - tile.y)
        .max(tile.y - (b.anchor.y + h - 1))
        .max(0);
    dx.max(dy)
}
