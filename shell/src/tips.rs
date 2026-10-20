//! Match tips: one short card at a time that points the player at
//! something the match needs, such as idle Harvesters or an attack. Each
//! rule reads only the human seat's own forces, what its fog lets it
//! know, and the attack alerts the HUD already raises; none issues a
//! command, so replays, saves and the simulation never see a tip. Which
//! tips have shown lives on the session, not in its save, so a resumed
//! match can show one again.

use crate::game::{AlertKind, Game};
use crate::platform::Hands;
use chassis::grid::TilePos;
use oxide_sim::stats::Domain;
use oxide_sim::{Building, BuildingKind, Order, PlayerId, State, Target, Tick, UnitId, UnitKind};

const SECOND: Tick = oxide_sim::TICKS_PER_SECOND as Tick;
/// No tip shows this early: the opening belongs to the player.
const OPENING: Tick = 60 * SECOND;
/// The least time from one tip appearing to the next.
const GAP: Tick = 120 * SECOND;
/// How long a tip stays up.
const SHOWN: Tick = 15 * SECOND;
/// How long a leaving tip takes to fade out.
const FADE: Tick = SECOND;
/// How long a Harvester stands idle before it is worth a tip.
const IDLE_WAIT: Tick = 30 * SECOND;
/// How long scrap sits unspent before it is worth a tip.
const UNSPENT_WAIT: Tick = 45 * SECOND;
/// The bank that counts as scrap to spend.
const UNSPENT_BANK: u32 = 400;
/// How long without an enemy machine in sight before cleanup tips show.
const QUIET: Tick = 30 * SECOND;
/// When an unclaimed Extractor frame near the Foundry is worth a tip.
const FRAMES_AFTER: Tick = 120 * SECOND;
/// When a player who has never seen an enemy building is told to scout.
const SCOUT_AFTER: Tick = 300 * SECOND;
/// How far from a Foundry's center a frame counts as near, in tiles.
const FRAME_REACH: i32 = 12;
/// How far from an own building's footprint a wreck counts as near, in
/// tiles.
const WRECK_REACH: i32 = 10;
/// How far from a Foundry's footprint a scrap pile counts as near, in
/// tiles. Every shipped map puts its home deposit this close.
const SCRAP_REACH: i32 = 12;

/// One thing a tip can point out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tip {
    /// The HUD has raised an attack alert.
    UnderAttack,
    /// An enemy aircraft is in sight and nothing of the player's can hit
    /// it.
    EnemyAircraft,
    /// A Harvester has stood idle for a while, and a scrap pile lies
    /// safe and in sight near a Foundry.
    IdleHarvesters,
    /// The bank has sat full with nothing training or being built.
    UnspentScrap,
    /// A building is badly damaged and nobody is welding it.
    WeldDamage,
    /// A wreck lies in sight near the base and nobody is collecting
    /// wrecks.
    Wrecks,
    /// A derelict Extractor the player has seen near their Foundry is
    /// still unclaimed.
    ExtractorFrame,
    /// Minutes in, the player has never seen an enemy building.
    FindEnemy,
}

/// A tip's text for each pairing of pointer and keyboard, as lessons
/// word it: keys only once one is in use, and a finger long-presses.
struct Copy {
    mouse_keys: &'static str,
    mouse: &'static str,
    touch_keys: &'static str,
    touch: &'static str,
}

impl Tip {
    /// Every tip, most urgent first: when several are due, the first
    /// shows.
    pub(crate) const ALL: [Tip; 8] = [
        Tip::UnderAttack,
        Tip::EnemyAircraft,
        Tip::IdleHarvesters,
        Tip::UnspentScrap,
        Tip::WeldDamage,
        Tip::Wrecks,
        Tip::ExtractorFrame,
        Tip::FindEnemy,
    ];

    /// The card's heading.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Tip::UnderAttack => "Under attack",
            Tip::EnemyAircraft => "Enemy aircraft",
            Tip::IdleHarvesters => "Idle Harvesters",
            Tip::UnspentScrap => "Unspent scrap",
            Tip::WeldDamage => "Damaged buildings",
            Tip::Wrecks => "Wrecks near your base",
            Tip::ExtractorFrame => "Unclaimed Extractor",
            Tip::FindEnemy => "Find the enemy",
        }
    }

    /// What the card says to do, for the hands in use.
    pub(crate) fn body(self, hands: Hands) -> &'static str {
        let copy = self.copy();
        match (hands.touch(), hands.keys) {
            (false, true) => copy.mouse_keys,
            (false, false) => copy.mouse,
            (true, true) => copy.touch_keys,
            (true, false) => copy.touch,
        }
    }

    fn copy(self) -> Copy {
        match self {
            Tip::UnderAttack => Copy {
                mouse_keys: "You are under attack. Click the alert badge or press {alert} to see where.",
                mouse: "You are under attack. Click the alert badge to see where.",
                touch_keys: "You are under attack. Tap the alert badge or press {alert} to see where.",
                touch: "You are under attack. Tap the alert badge to see where.",
            },
            Tip::EnemyAircraft => Copy {
                mouse_keys: "Enemy aircraft are in sight, and you have nothing that can shoot them. Select a Harvester, click Build or press {build}, and choose Flak Turret.",
                mouse: "Enemy aircraft are in sight, and you have nothing that can shoot them. Select a Harvester, click Build, and choose Flak Turret.",
                touch_keys: "Enemy aircraft are in sight, and you have nothing that can shoot them. Select a Harvester, tap Build or press {build}, and choose Flak Turret.",
                touch: "Enemy aircraft are in sight, and you have nothing that can shoot them. Select a Harvester, tap Build, and choose Flak Turret.",
            },
            Tip::IdleHarvesters => Copy {
                mouse_keys: "Some of your Harvesters are idle. Click the idle badge or press {idle} to select one, then right-click a scrap pile.",
                mouse: "Some of your Harvesters are idle. Click the idle badge to select one, then right-click a scrap pile.",
                touch_keys: "Some of your Harvesters are idle. Tap the idle badge or press {idle} to select one, then long-press a scrap pile.",
                touch: "Some of your Harvesters are idle. Tap the idle badge to select one, then long-press a scrap pile.",
            },
            Tip::UnspentScrap => Copy {
                mouse_keys: "You have scrap to spend. Select your Foundry to train machines, or select a Harvester and click Build or press {build}.",
                mouse: "You have scrap to spend. Select your Foundry to train machines, or select a Harvester and click Build.",
                touch_keys: "You have scrap to spend. Select your Foundry to train machines, or select a Harvester and tap Build or press {build}.",
                touch: "You have scrap to spend. Select your Foundry to train machines, or select a Harvester and tap Build.",
            },
            Tip::WeldDamage => Copy {
                mouse_keys: "Some of your buildings are damaged. Select a Harvester and right-click a damaged building to weld it.",
                mouse: "Some of your buildings are damaged. Select a Harvester and right-click a damaged building to weld it.",
                touch_keys: "Some of your buildings are damaged. Select a Harvester and long-press a damaged building to weld it.",
                touch: "Some of your buildings are damaged. Select a Harvester and long-press a damaged building to weld it.",
            },
            Tip::Wrecks => Copy {
                mouse_keys: "There are wrecks near your base. Select a Harvester and right-click a wreck to collect its scrap.",
                mouse: "There are wrecks near your base. Select a Harvester and right-click a wreck to collect its scrap.",
                touch_keys: "There are wrecks near your base. Select a Harvester and long-press a wreck to collect its scrap.",
                touch: "There are wrecks near your base. Select a Harvester and long-press a wreck to collect its scrap.",
            },
            Tip::ExtractorFrame => Copy {
                mouse_keys: "A derelict Extractor near your base is still unclaimed. To restore it, select a Harvester, click Build or press {build}, choose Extractor, and place it on the frame.",
                mouse: "A derelict Extractor near your base is still unclaimed. To restore it, select a Harvester, click Build, choose Extractor, and place it on the frame.",
                touch_keys: "A derelict Extractor near your base is still unclaimed. To restore it, select a Harvester, tap Build or press {build}, choose Extractor, and place it on the frame.",
                touch: "A derelict Extractor near your base is still unclaimed. To restore it, select a Harvester, tap Build, choose Extractor, and place it on the frame.",
            },
            Tip::FindEnemy => Copy {
                mouse_keys: "You haven't found the enemy base yet. Select your Surveyor and right-click far across the map to scout.",
                mouse: "You haven't found the enemy base yet. Select your Surveyor and right-click far across the map to scout.",
                touch_keys: "You haven't found the enemy base yet. Select your Surveyor and long-press far across the map to scout.",
                touch: "You haven't found the enemy base yet. Select your Surveyor and long-press far across the map to scout.",
            },
        }
    }

    /// Whether the tip has something to point at now.
    fn due(self, seat: &Seat<'_>, watch: &Watch, now: Tick) -> bool {
        match self {
            Tip::UnderAttack => seat
                .alerts
                .iter()
                .any(|(_, _, kind)| *kind == AlertKind::Attack),
            Tip::EnemyAircraft => seat.enemy_aircraft_in_sight() && !seat.can_hit_air(),
            Tip::IdleHarvesters => {
                watch.idle.iter().any(|(_, since)| now - since >= IDLE_WAIT)
                    && seat.safe_scrap_near_foundry()
            }
            Tip::UnspentScrap => watch
                .unspent
                .is_some_and(|since| now - since >= UNSPENT_WAIT),
            Tip::WeldDamage => {
                watch.quiet(now) && seat.workers().next().is_some() && seat.unwelded_damage()
            }
            Tip::Wrecks => {
                watch.quiet(now)
                    && seat.workers().next().is_some()
                    && !seat.collecting_wrecks()
                    && seat.wreck_near_base()
            }
            Tip::ExtractorFrame => {
                now >= FRAMES_AFTER
                    && seat.workers().next().is_some()
                    && seat.open_frame_near_foundry()
            }
            Tip::FindEnemy => {
                now >= SCOUT_AFTER
                    && !watch.found_base
                    && seat.own_units().any(|u| u.kind == UnitKind::Surveyor)
            }
        }
    }
}

/// What the rules remember between reads.
#[derive(Debug, Default)]
struct Watch {
    /// Own Harvesters standing idle, each with the tick it was first seen
    /// so.
    idle: Vec<(UnitId, Tick)>,
    /// Since when the bank has sat unspent.
    unspent: Option<Tick>,
    /// When an enemy machine was last in sight.
    enemy_seen: Option<Tick>,
    /// Whether an enemy building has ever been in sight.
    found_base: bool,
}

impl Watch {
    fn update(&mut self, seat: &Seat<'_>, now: Tick) {
        let was = std::mem::take(&mut self.idle);
        self.idle = seat
            .workers()
            .filter(|u| u.order == Order::Idle)
            .map(|u| {
                let since = was.iter().find(|(id, _)| *id == u.id);
                (u.id, since.map_or(now, |(_, since)| *since))
            })
            .collect();
        self.unspent = (seat.bank() >= UNSPENT_BANK && seat.has_foundry() && !seat.spending())
            .then(|| self.unspent.unwrap_or(now));
        if seat.enemy_machine_in_sight() {
            self.enemy_seen = Some(now);
        }
        self.found_base = self.found_base || seat.enemy_building_seen();
    }

    /// Whether no enemy machine has been in sight for a while.
    fn quiet(&self, now: Tick) -> bool {
        self.enemy_seen.is_none_or(|seen| now - seen >= QUIET)
    }
}

/// The tip on screen and the tick it leaves by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Showing {
    tip: Tip,
    until: Tick,
}

/// The session's tips: what shows now, what has shown, and what the
/// rules are watching.
#[derive(Debug, Default)]
pub(crate) struct Tips {
    showing: Option<Showing>,
    /// Tips already shown this session; each shows once.
    shown: Vec<Tip>,
    /// When the latest tip appeared.
    last: Option<Tick>,
    /// The tick the rules last read, so a frame without a new tick reads
    /// nothing.
    read: Option<Tick>,
    watch: Watch,
}

impl Tips {
    /// Reads the rules against `game`'s latest tick. Tips stay away when
    /// the setting is off, during a lesson, once the match is decided,
    /// and from a seat that can no longer act. `reduced` drops the fade:
    /// a tip whose reason has passed leaves at once.
    pub(crate) fn advance_in(game: &mut Game, on: bool, reduced: bool) {
        let human = game.presentation.human;
        let active = on
            && game.tutorial.is_none()
            && game.state.result().is_none()
            && game.state.accepts_commands(human)
            && !game.presentation.spectating();
        let seat = Seat {
            state: &game.state,
            human,
            alerts: &game.presentation.alerts,
        };
        game.tips.observe(&seat, active, reduced);
    }

    fn observe(&mut self, seat: &Seat<'_>, active: bool, reduced: bool) {
        if !active {
            self.showing = None;
            return;
        }
        let now = seat.state.current_tick();
        if self.read == Some(now) {
            return;
        }
        self.read = Some(now);
        self.watch.update(seat, now);
        if let Some(showing) = &mut self.showing
            && !showing.tip.due(seat, &self.watch, now)
        {
            showing.until = showing.until.min(if reduced { now } else { now + FADE });
        }
        if self.showing.is_some_and(|showing| now >= showing.until) {
            self.showing = None;
        }
        if self.showing.is_some() || now < OPENING || self.last.is_some_and(|last| now < last + GAP)
        {
            return;
        }
        if let Some(tip) = Tip::ALL
            .into_iter()
            .find(|tip| !self.shown.contains(tip) && tip.due(seat, &self.watch, now))
        {
            self.showing = Some(Showing {
                tip,
                until: now + SHOWN,
            });
            self.shown.push(tip);
            self.last = Some(now);
        }
    }

    /// The tip on screen, if any.
    pub(crate) fn showing(&self) -> Option<Tip> {
        self.showing.map(|showing| showing.tip)
    }

    /// The card's opacity at tick `now` plus `fraction` of the next: full
    /// until the last second of its stay, then fading out.
    pub(crate) fn alpha(&self, now: Tick, fraction: f32, reduced: bool) -> f32 {
        match self.showing {
            None => 0.0,
            Some(_) if reduced => 1.0,
            Some(showing) => {
                let left = showing.until.saturating_sub(now) as f32 - fraction;
                (left / FADE as f32).clamp(0.0, 1.0)
            }
        }
    }

    /// Closes the tip on screen. It still counts as shown.
    pub(crate) fn dismiss(&mut self) {
        self.showing = None;
    }
}

/// The human seat as its player knows it: its own forces, what its
/// sight shows now, and the HUD's alerts.
struct Seat<'a> {
    state: &'a State,
    human: PlayerId,
    alerts: &'a [(macroquad::prelude::Vec2, f32, AlertKind)],
}

impl<'a> Seat<'a> {
    fn bank(&self) -> u32 {
        self.state.player(self.human).scrap
    }

    fn vision(&self) -> &'a oxide_sim::Vision {
        self.state.vision(self.human)
    }

    fn own_units(&self) -> impl Iterator<Item = &'a oxide_sim::Unit> + use<'a> {
        let human = self.human;
        self.state
            .units()
            .iter()
            .filter(move |u| u.player == human && u.hp > 0)
    }

    fn workers(&self) -> impl Iterator<Item = &'a oxide_sim::Unit> + use<'a> {
        self.own_units()
            .filter(|u| u.kind.stats().harvest.is_some())
    }

    fn own_buildings(&self) -> impl Iterator<Item = &'a Building> + use<'a> {
        let human = self.human;
        self.state
            .buildings()
            .iter()
            .filter(move |b| b.player == human && b.hp > 0)
    }

    fn has_foundry(&self) -> bool {
        self.own_buildings()
            .any(|b| b.kind == BuildingKind::Foundry && b.built())
    }

    /// Whether scrap is already going somewhere: a machine in training, a
    /// building going up, or rounds on order.
    fn spending(&self) -> bool {
        self.own_buildings()
            .any(|b| !b.built() || !b.queue.is_empty() || b.magazine.is_some_and(|m| m.ordered > 0))
    }

    /// Enemy machines the player sees now.
    fn enemy_machines(&self) -> impl Iterator<Item = &'a oxide_sim::Unit> + use<'a> {
        let (state, human) = (self.state, self.human);
        state.units().iter().filter(move |u| {
            u.hp > 0 && state.hostile(human, u.player) && state.sees_unit(human, u)
        })
    }

    fn enemy_machine_in_sight(&self) -> bool {
        self.enemy_machines().next().is_some()
    }

    fn enemy_aircraft_in_sight(&self) -> bool {
        self.enemy_machines().any(|u| u.domain() == Domain::Air)
    }

    /// Whether the player has, builds or trains anything that shoots
    /// aircraft.
    fn can_hit_air(&self) -> bool {
        self.own_units()
            .any(|u| u.kind.stats().can_target(Domain::Air))
            || self.own_buildings().any(|b| {
                b.stats()
                    .weapons
                    .iter()
                    .any(|w| w.targets.covers(Domain::Air))
                    || b.queue
                        .iter()
                        .any(|kind| kind.stats().can_target(Domain::Air))
            })
    }

    /// Whether an enemy building is in sight now or remembered.
    fn enemy_building_seen(&self) -> bool {
        let (state, human) = (self.state, self.human);
        !self.vision().ghosts().is_empty()
            || state.buildings().iter().any(|b| {
                b.hp > 0
                    && state.hostile(human, b.player)
                    && state.building_apparent(human, b)
                    && b.tiles().any(|tile| state.can_see(human, tile))
            })
    }

    /// Whether a built own building is below 60% health with no Harvester
    /// welding it.
    fn unwelded_damage(&self) -> bool {
        self.own_buildings().any(|b| {
            let max = b.stats().max_hp;
            b.built()
                && !b.kind.is_stealthy()
                && b.hp * 10 < max * 6
                && !self.workers().any(|u| welds(u.order, b))
        })
    }

    /// Salvage the player believes lies at `tile`.
    fn known_wreck(&self, tile: TilePos) -> bool {
        if self.vision().visible(tile) {
            self.state.map().wreck_at(tile) > 0
        } else {
            self.vision().remembered_wreck(tile) > 0
        }
    }

    fn collecting_wrecks(&self) -> bool {
        self.workers().any(|u| {
            matches!(u.order, Order::Harvest { node, anchor, .. }
                if self.known_wreck(node) || self.known_wreck(anchor))
        })
    }

    /// Whether a wreck lies in sight near any own building.
    fn wreck_near_base(&self) -> bool {
        let (map, vision) = (self.state.map(), self.vision());
        self.own_buildings().any(|b| {
            around(b.anchor, b.kind.size(), WRECK_REACH)
                .any(|tile| vision.visible(tile) && map.wreck_at(tile) > 0)
        })
    }

    /// Whether a scrap pile lies in sight near a built own Foundry where
    /// Harvesters would judge it safe to collect.
    fn safe_scrap_near_foundry(&self) -> bool {
        let (map, vision) = (self.state.map(), self.vision());
        let piles: Vec<TilePos> = self
            .own_buildings()
            .filter(|b| b.kind == BuildingKind::Foundry && b.built())
            .flat_map(|b| around(b.anchor, b.kind.size(), SCRAP_REACH))
            .filter(|&tile| vision.visible(tile) && map.scrap_at(tile) > 0)
            .collect();
        // Capturing the danger snapshot walks the whole map, so it waits
        // for a pile to judge.
        if piles.is_empty() {
            return false;
        }
        let danger = self.state.salvage_danger(self.human);
        piles.into_iter().any(|tile| !danger(tile))
    }

    /// Whether a derelict Extractor the player has seen near a built own
    /// Foundry has no claim the player knows of.
    fn open_frame_near_foundry(&self) -> bool {
        let vision = self.vision();
        // Centers in half tiles keep the distance exact.
        let center = |anchor: TilePos, (w, h): (i32, i32)| (anchor.x * 2 + w, anchor.y * 2 + h);
        let foundries: Vec<(i32, i32)> = self
            .own_buildings()
            .filter(|b| b.kind == BuildingKind::Foundry && b.built())
            .map(|b| center(b.anchor, b.kind.size()))
            .collect();
        let frame_size = BuildingKind::Extractor.size();
        self.state.map().extractor_frames().iter().any(|&frame| {
            let (fx, fy) = center(frame, frame_size);
            let reach = FRAME_REACH * 2;
            footprint(frame, frame_size).any(|tile| vision.explored(tile))
                && foundries
                    .iter()
                    .any(|(x, y)| (fx - x).pow(2) + (fy - y).pow(2) <= reach * reach)
                && !self.state.extractor_frame_claim_known(self.human, frame)
        })
    }
}

/// Whether a worker on `order` is welding `building`.
fn welds(order: Order, building: &Building) -> bool {
    match order {
        Order::Repair { building: patient } => patient == building.id,
        Order::Tend {
            weld: Some(Target::Building(patient)),
            ..
        } => patient == building.id,
        Order::ReturnCargo {
            foundry,
            repair: true,
        } => foundry == building.id,
        _ => false,
    }
}

fn footprint(anchor: TilePos, (w, h): (i32, i32)) -> impl Iterator<Item = TilePos> {
    (0..h).flat_map(move |dy| (0..w).map(move |dx| anchor.offset(dx, dy)))
}

/// The tiles within `reach` of a footprint, the footprint included.
fn around(anchor: TilePos, (w, h): (i32, i32), reach: i32) -> impl Iterator<Item = TilePos> {
    footprint(
        anchor.offset(-reach, -reach),
        (w + 2 * reach, h + 2 * reach),
    )
}

#[cfg(test)]
mod tests;
