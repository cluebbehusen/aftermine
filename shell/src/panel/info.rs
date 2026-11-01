//! Selection facts, independent of their placement in the HUD.

use super::*;
use crate::game::Scene;
use aftermine_sim::stats::{
    BuildingKind, CHARGE_ARRAY_DETECT_RADIUS, CHARGE_BASE_ARRAY_DETECT_RADIUS, CHARGE_BLAST_RADIUS,
    CHARGE_DAMAGE, CHARGE_SCOUT_DETECT_RADIUS, CHARGE_TRIGGER_RADIUS, CRUCIBLE_SMELT_RADIUS,
    Domain, FOUNDRY_DRIP_START_TICK, REPAIR_BAY_PERIOD, REPAIR_BAY_RADIUS, REPAIR_BAY_STEP,
    WeaponStats,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatIcon {
    Capability(CapabilityIcon),
    Verb(VerbIcon),
}

#[derive(Debug, Clone)]
pub(crate) struct StatRow {
    pub label: String,
    pub value: String,
    pub icon: Option<StatIcon>,
    pub section: bool,
}

/// A single selection's health and the ownership color its bar wears.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Health {
    pub hp: u32,
    pub max_hp: u32,
    pub color: macroquad::prelude::Color,
}

impl Health {
    fn of(game: &Scene<'_>, owner: aftermine_sim::PlayerId, hp: u32, max_hp: u32) -> Self {
        Self {
            hp,
            max_hp,
            color: crate::render::seat_identity_color(game, owner),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct SelectionInfo {
    pub health: Option<Health>,
    pub status: Vec<String>,
    pub rows: Vec<StatRow>,
    pub upgrade: Option<super::upgrade::UpgradeComparison>,
}

impl SelectionInfo {
    /// A cradle's assembly progress, its crew for its owner, and the machine
    /// it becomes.
    fn cradle_rows(&mut self, game: &Scene<'_>, b: &aftermine_sim::Building) {
        if let (Some(progress), Some(construction)) =
            (b.construction_progress(), b.stats().construction)
        {
            self.row(
                "Assembly",
                format!(
                    "{}%",
                    u64::from(progress) * 100 / u64::from(construction.build_ticks.max(1))
                ),
                Some(StatIcon::Verb(VerbIcon::Build)),
            );
        }
        if b.player == game.presentation.human {
            self.row(
                "Crew",
                format!(
                    "{}/{}",
                    game.state.cradle_crew(b.id).len(),
                    aftermine_sim::stats::ASSEMBLY_CREW_CAP
                ),
                Some(StatIcon::Verb(VerbIcon::Build)),
            );
        }
        if let Some(machine) = b.kind.assembles() {
            self.row("Becomes", entity_name(machine.name()), None);
        }
    }

    /// What a kiln machine's kiln is doing.
    fn kiln_row(&mut self, game: &Scene<'_>, u: &aftermine_sim::Unit) {
        let stats = u.kind.stats();
        if let aftermine_sim::stats::Mend::Kiln(_) = stats.mend {
            self.row(
                "Kiln",
                if game.state.kiln_fuel(u.id).is_some() {
                    "Eating wreck"
                } else if u.hp < stats.max_hp {
                    "No wreck in reach"
                } else {
                    "Cold"
                }
                .into(),
                Some(StatIcon::Verb(VerbIcon::Repair)),
            );
        }
    }

    fn repair_bay_rows(&mut self) {
        self.row(
            "Repair radius",
            format!("{:.1} tiles", REPAIR_BAY_RADIUS.to_num::<f32>()),
            Some(StatIcon::Capability(CapabilityIcon::Repair)),
        );
        self.row(
            "Patients",
            aftermine_sim::stats::REPAIR_BAY_PATIENTS.to_string(),
            None,
        );
        self.row(
            "Repair rate",
            format!(
                "{:.0}% hp/s",
                aftermine_sim::stats::REPAIR_BAY_FAST_PERMILLE_PER_SECOND as f32 / 10.0
            ),
            None,
        );
        self.row(
            "Under fire",
            format!(
                "{:.1} hp/s",
                REPAIR_BAY_STEP as f32 * aftermine_sim::TICKS_PER_SECOND as f32
                    / REPAIR_BAY_PERIOD as f32
            ),
            None,
        );
    }

    /// Rounds as the viewer may know them, then what they do: a Spire's
    /// warhead, a Lattice's ring and the hostile warheads falling into it.
    /// Ordered rounds and their assembly live in the queue dock.
    fn magazine_rows(&mut self, game: &Scene<'_>, b: &aftermine_sim::Building) {
        let Some(stats) = b.kind.magazine() else {
            return;
        };
        let known = game
            .state
            .known_magazines(game.presentation.human)
            .into_iter()
            .find(|known| known.building == b.id);
        if let Some(known) = known {
            self.row(
                if b.kind == BuildingKind::Spire {
                    "Warheads"
                } else {
                    "Interceptors"
                },
                format!("{}/{}", known.loaded, stats.capacity),
                Some(StatIcon::Capability(CapabilityIcon::Intercept)),
            );
            if let Some(target) = known.detail.and_then(|detail| detail.target) {
                self.status
                    .push(format!("Turning to strike {}, {}", target.x, target.y));
            }
        }
        if let Some(battery) = b.kind.battery() {
            self.row(
                "Ring",
                format!("{:.0} tiles", battery.ring.to_num::<f32>()),
                Some(StatIcon::Capability(CapabilityIcon::Intercept)),
            );
            let warheads = game.state.known_warheads(game.presentation.human);
            self.status
                .extend(incoming_warheads(&warheads, b.center(), battery.ring));
        } else {
            let warhead = aftermine_sim::stats::WARHEAD;
            self.row(
                "Warhead",
                format!("{} damage", warhead.peak_damage),
                Some(StatIcon::Capability(CapabilityIcon::Weapon)),
            );
            self.row(
                "Blast",
                format!(
                    "{:.1}-{:.1} tiles",
                    warhead.full_radius.to_num::<f32>(),
                    warhead.outer_radius.to_num::<f32>()
                ),
                None,
            );
        }
    }

    fn row(&mut self, label: &str, value: String, icon: Option<StatIcon>) {
        self.rows.push(StatRow {
            label: label.into(),
            value,
            icon,
            section: false,
        });
    }

    /// Charges, reach and recharge for a point defender. Lit emitters show
    /// the count to anyone in sight, like wounds.
    fn point_defense(&mut self, unit: &aftermine_sim::Unit) {
        let Some(defense) = unit.kind.stats().point_defense else {
            return;
        };
        let held = unit.point_defense.map_or(0, |held| held.charges);
        self.row("Charges", format!("{held}/{}", defense.charges), None);
        self.row(
            "Intercept reach",
            format!("{:.1} tiles", defense.radius.to_num::<f32>()),
            Some(StatIcon::Capability(CapabilityIcon::Intercept)),
        );
        self.row(
            "Recharge",
            tick_time_label(u32::from(defense.recharge_ticks)),
            None,
        );
    }

    /// A weapon's rows: what it deals, to structures too where that
    /// differs, and how far it reaches, which for a `contact` weapon is the
    /// hulls touching.
    pub(super) fn weapons(&mut self, weapons: &[WeaponStats], contact: bool) {
        for weapon in weapons {
            let label = match (
                weapon.targets.covers(Domain::Ground),
                weapon.targets.covers(Domain::Air),
            ) {
                (true, true) => "Ground + air",
                (false, true) => "Air",
                _ => "Ground",
            };
            self.row(
                label,
                format!("{} dmg/hit", weapon.damage),
                Some(StatIcon::Capability(weapon_capability_icon(weapon))),
            );
            self.rows.last_mut().unwrap().section = true;
            if let Some(structure) = weapon.building_damage {
                self.row("Structures", format!("{structure} dmg/hit"), None);
            }
            let range = if contact {
                "Contact".to_string()
            } else if weapon.minimum_range > chassis::fx::Fx::ZERO {
                format!(
                    "{:.1}-{:.1} tiles",
                    weapon.minimum_range.to_num::<f32>(),
                    weapon.range.to_num::<f32>()
                )
            } else {
                format!("{:.1} tiles", weapon.range.to_num::<f32>())
            };
            self.row("Range", range, None);
            self.row("Reload", tick_time_label(weapon.cooldown_ticks), None);
            if let Some(radius) = weapon.splash {
                self.row(
                    "Splash",
                    format!("{:.1} tiles", radius.to_num::<f32>()),
                    None,
                );
            }
        }
    }

    fn ownership(&mut self, game: &Scene<'_>, owner: aftermine_sim::PlayerId) {
        if owner != game.presentation.human {
            let relation = if game.state.hostile(game.presentation.human, owner) {
                "Hostile"
            } else {
                "Ally"
            };
            self.status
                .push(format!("{relation}: {}", game.state.player(owner).name));
            if let Some(controller) = bot_controller_label(game, owner) {
                self.status.push(controller);
            }
        }
    }
}

/// A machine's radar ring and, for one that unfolds it only at rest, how far
/// it has.
fn radar_facts(info: &mut SelectionInfo, unit: &aftermine_sim::Unit) {
    let Some(radar) = unit.kind.stats().radar else {
        return;
    };
    let parked = if radar.deploy_ticks > 0 {
        if unit.radar().is_some() {
            info.status.push("Radar up".into());
        } else if unit.radar_deploy > 0 {
            info.status.push("Unfolding radar".into());
        }
        " (parked)"
    } else {
        ""
    };
    info.row(
        "Radar",
        format!("{} tiles{parked}", radar.radius),
        Some(StatIcon::Capability(CapabilityIcon::Radar)),
    );
}

/// A carrier's sling and a host's drone bay: what each holds to its owner
/// and team, and only how much it holds to anyone else.
fn hold_rows(info: &mut SelectionInfo, game: &Scene<'_>, u: &aftermine_sim::Unit) {
    let stats = u.kind.stats();
    let hostile = game.state.hostile(game.presentation.human, u.player);
    if stats.transport_capacity > 0 {
        let value = if hostile {
            format!("{} points", stats.transport_capacity)
        } else {
            let held: u8 = u.cargo.iter().map(|r| r.kind.stats().transport_size).sum();
            format!("{held}/{} points", stats.transport_capacity)
        };
        info.row("Cargo", value, Some(StatIcon::Verb(VerbIcon::Build)));
    }
    if let Some(bay) = stats.bay {
        let value = if hostile {
            format!("{} drones", bay.capacity)
        } else {
            crate::unit_bays::readout(game, u.id).unwrap_or_default()
        };
        info.row("Drones", value, Some(StatIcon::Verb(VerbIcon::Build)));
    }
}

pub(crate) fn selection_info(game: &Scene<'_>, panel: &Panel) -> SelectionInfo {
    use StatIcon::{Capability as Cap, Verb};
    let mut info = SelectionInfo::default();
    if let Some(tile) = game.presentation.selection.pile {
        if let Some(salvage) = game.known_salvage(tile) {
            info.row(
                "Scrap left",
                salvage.amount().to_string(),
                Some(Verb(VerbIcon::Harvest)),
            );
            // Own harvesters only: another player's orders are intent
            // the viewer has no license to read.
            let harvesters = game
                .state
                .units()
                .iter()
                .filter(|u| {
                    u.player == game.presentation.human
                        && matches!(u.order, aftermine_sim::Order::Harvest { node, .. } if node == tile)
                })
                .count();
            info.row("Harvesters", harvesters.to_string(), None);
        }
        return info;
    }
    if game.presentation.selection.buildings.len() == 1 {
        let Some(b) = game
            .state
            .building(game.presentation.selection.buildings[0])
        else {
            return info;
        };
        let stats = b.stats();
        if b.player == game.presentation.human && b.built() {
            info.upgrade = super::upgrade::comparison(b.kind, b.tier);
        }
        info.health = Some(Health::of(game, b.player, b.hp, stats.max_hp));
        info.ownership(game, b.player);
        if !b.built() {
            info.status.push(
                if b.tier > 0 {
                    "Upgrading"
                } else {
                    "Under construction"
                }
                .into(),
            );
        }
        info.row(
            "Sight",
            format!("{} tiles", stats.vision),
            Some(Cap(CapabilityIcon::Vision)),
        );
        if b.player == game.presentation.human {
            if let Some(income) = game.state.extractor_income(b.id) {
                info.row(
                    "Income",
                    format!("{} scrap/min", income.scrap_per_minute()),
                    Some(Verb(VerbIcon::Harvest)),
                );
                info.row(
                    "Support",
                    if income.is_supported() {
                        "Foundry".to_string()
                    } else {
                        "Remote".to_string()
                    },
                    Some(Cap(CapabilityIcon::EconomySupport)),
                );
            } else if matches!(b.kind, BuildingKind::Foundry | BuildingKind::Reclaimer) {
                let income = building_income(game, b);
                if b.built() && b.kind == BuildingKind::Foundry && income == 0 {
                    let remaining = FOUNDRY_DRIP_START_TICK
                        .saturating_sub(game.state.current_tick())
                        .div_ceil(u64::from(aftermine_sim::TICKS_PER_SECOND));
                    info.row(
                        "Income starts",
                        format!("{remaining}s"),
                        Some(Verb(VerbIcon::Harvest)),
                    );
                } else {
                    info.row(
                        "Income",
                        format!("{income} scrap/min"),
                        Some(Verb(VerbIcon::Harvest)),
                    );
                }
            }
        }
        if let Some(radar) = b.stats().radar {
            info.row(
                "Radar",
                format!("{radar} tiles"),
                Some(Cap(CapabilityIcon::Radar)),
            );
        }
        match b.kind {
            BuildingKind::Array => {
                let radius = if b.tier == 0 {
                    CHARGE_BASE_ARRAY_DETECT_RADIUS
                } else {
                    CHARGE_ARRAY_DETECT_RADIUS
                };
                info.row(
                    "Mine detection",
                    format!("{radius} tiles"),
                    Some(Cap(CapabilityIcon::Radar)),
                );
            }
            BuildingKind::RepairBay => info.repair_bay_rows(),
            BuildingKind::Spire | BuildingKind::Lattice => info.magazine_rows(game, b),
            BuildingKind::MawCradle | BuildingKind::RocCradle => info.cradle_rows(game, b),
            BuildingKind::Crucible => info.row(
                "Smelt reach",
                format!("{:.1} tiles", CRUCIBLE_SMELT_RADIUS.to_num::<f32>()),
                Some(Verb(VerbIcon::Harvest)),
            ),
            BuildingKind::ScuttleCharge => {
                info.row(
                    "Ground blast",
                    format!("{CHARGE_DAMAGE} damage"),
                    Some(Cap(CapabilityIcon::Weapon)),
                );
                info.row(
                    "Blast radius",
                    format!("{:.1} tiles", CHARGE_BLAST_RADIUS.to_num::<f32>()),
                    None,
                );
                info.row(
                    "Trigger",
                    format!("{:.1} tiles", CHARGE_TRIGGER_RADIUS.to_num::<f32>()),
                    None,
                );
            }
            BuildingKind::Foundry
            | BuildingKind::Turret
            | BuildingKind::Fabricator
            | BuildingKind::FlakTurret
            | BuildingKind::Bastion
            | BuildingKind::Reclaimer
            | BuildingKind::Extractor
            | BuildingKind::Airworks
            | BuildingKind::Barricade => {}
        }
        info.weapons(stats.weapons, false);
    } else if game.presentation.selection.units.len() == 1 {
        let Some(u) = game.state.unit(game.presentation.selection.units[0]) else {
            return info;
        };
        let stats = u.kind.stats();
        info.health = Some(Health::of(game, u.player, u.hp, stats.max_hp));
        info.ownership(game, u.player);
        if u.landed() {
            info.status.push("Landed".into());
        }
        info.row(
            "Speed",
            format!(
                "{} tiles/s",
                unit_speed_label(u.kind).trim_end_matches(" tiles/sec")
            ),
            Some(Verb(VerbIcon::Move)),
        );
        info.row(
            "Sight",
            format!("{} tiles", stats.vision),
            Some(Cap(CapabilityIcon::Vision)),
        );
        if crate::look::scout(u.kind) {
            info.row(
                "Mine detection",
                format!("{CHARGE_SCOUT_DETECT_RADIUS} tiles"),
                Some(Cap(CapabilityIcon::Radar)),
            );
        }
        radar_facts(&mut info, u);
        hold_rows(&mut info, game, u);
        if let Some(harvest) = stats.harvest {
            info.row(
                "Scrap load",
                if u.player == game.presentation.human {
                    format!("{}/{}", u.carrying(), harvest.capacity)
                } else {
                    format!("{} capacity", harvest.capacity)
                },
                Some(Verb(VerbIcon::Harvest)),
            );
            info.row(
                "Gather",
                format!(
                    "{:.1} scrap/s",
                    aftermine_sim::TICKS_PER_SECOND as f32 / harvest.ticks_per_scrap as f32
                ),
                None,
            );
        }
        info.point_defense(u);
        info.kiln_row(game, u);
        if let Some(demolition) = stats.demolition {
            info.row(
                "Structure hit",
                format!("{} damage", demolition.structure_damage),
                Some(Cap(CapabilityIcon::Weapon)),
            );
            info.row(
                "Ground blast",
                format!("{} damage", demolition.splash_damage),
                None,
            );
            info.row(
                "Blast radius",
                format!("{:.1} tiles", demolition.blast_radius.to_num::<f32>()),
                None,
            );
        }
        info.weapons(stats.weapons, stats.contact_reach.is_some());
        match stats.target_priority {
            aftermine_sim::stats::TargetPriority::Heaviest => {
                info.row("Targets", "Heaviest first".into(), None);
            }
            aftermine_sim::stats::TargetPriority::Buildings => {
                info.row("Targets", "Structures first".into(), None);
            }
            aftermine_sim::stats::TargetPriority::Danger => {}
        }
    } else {
        if !panel.summary.is_empty() {
            info.status.push(panel.summary.clone());
        }
        if !game.presentation.selection.buildings.is_empty() {
            info.status
                .extend(crate::building_actions::SelectedBuildings::inspect(game).ammunition());
        }
        if let Some(u) = subject_unit(game).and_then(|id| game.state.unit(id)) {
            info.ownership(game, u.player);
        } else if let Some(b) = game
            .presentation
            .selection
            .buildings
            .first()
            .and_then(|id| game.state.building(*id))
        {
            info.ownership(game, b.player);
        }
    }
    info
}

/// One line on the hostile warheads a battery's ring should know about:
/// those landing inside it, how many no battery is forecast to stop, and
/// contacts still being tracked, whose line and interceptor are unknown.
fn incoming_warheads(
    warheads: &[aftermine_sim::strike::KnownWarhead],
    centre: chassis::fx::Vec2Fx,
    ring: chassis::fx::Fx,
) -> Option<String> {
    let (mut incoming, mut unanswered, mut tracking) = (0, 0, 0);
    for warhead in warheads.iter().filter(|warhead| warhead.hostile) {
        match warhead.line {
            None => tracking += 1,
            Some(line) if line.impact.dist_sq(centre) > ring * ring => {}
            Some(_) => {
                incoming += 1;
                unanswered += usize::from(warhead.interceptor.is_none());
            }
        }
    }
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let mut line = if incoming > 0 {
        format!("{incoming} incoming warhead{}", plural(incoming))
    } else if tracking > 0 {
        return Some(format!("Tracking {tracking} warhead{}", plural(tracking)));
    } else {
        return None;
    };
    if unanswered > 0 {
        let _ = write!(line, ", {unanswered} unanswered");
    }
    if tracking > 0 {
        let _ = write!(line, ", tracking {tracking} more");
    }
    Some(line)
}

#[cfg(test)]
mod tests;
