//! The Roster's words: every unit and structure in tech order, each with
//! its description, figures and notes. The shell's Roster screen sets these
//! pages, and `aftermine-driver rules` prints them, so a player and an agent
//! read the same rules. Every figure comes from the stats table.

mod description;
mod figures;
mod names;

pub use description::{building_description, tiles, unit_description};
pub use figures::{
    building_economy_lines, building_stat_line, building_weapon_lines, cradle_crew_line,
    point_defense_line, tick_time_label, unit_stat_line, weapon_lines,
};
pub use names::{entity_name, plural_name, series};

use aftermine_sim::stats::{BuildingKind, Domain, UnitKind};
use names::article;
use std::fmt::Write as _;

/// One page of the Roster.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// A unit.
    Unit(UnitKind),
    /// A structure.
    Building(BuildingKind),
}

impl Entry {
    /// The page's title: the kind's name as players read it.
    pub fn name(self) -> String {
        match self {
            Self::Unit(kind) => entity_name(kind.name()),
            Self::Building(kind) => entity_name(kind.name()),
        }
    }

    /// The line under the title: a unit's domain and price, or a
    /// structure's price.
    pub fn header(self) -> String {
        match self {
            Self::Unit(kind) => {
                let stats = kind.stats();
                let domain = match stats.domain {
                    Domain::Ground => "ground",
                    Domain::Air => "air",
                };
                format!("{domain} | {} scrap", stats.cost)
            }
            Self::Building(kind) => format!("{} scrap", kind.base_stats().construction.cost),
        }
    }
}

/// The Roster in reading order: the factories as the player unlocks
/// them, each followed by what it trains, then every structure. The
/// section order is the tech order, so a new player reads the game the
/// way a match unfolds.
pub fn sections() -> Vec<(&'static str, Vec<Entry>)> {
    let trained_at = |factory: BuildingKind| -> Vec<Entry> {
        factory
            .base_stats()
            .produces
            .iter()
            .map(|kind| Entry::Unit(*kind))
            .collect()
    };
    vec![
        ("FOUNDRY", trained_at(BuildingKind::Foundry)),
        ("FABRICATOR", trained_at(BuildingKind::Fabricator)),
        ("AIRWORKS", trained_at(BuildingKind::Airworks)),
        ("CRUCIBLE", trained_at(BuildingKind::Crucible)),
        (
            "CRADLES",
            BuildingKind::ALL
                .into_iter()
                .filter_map(BuildingKind::assembles)
                .map(Entry::Unit)
                .collect(),
        ),
        (
            "DRONE BAYS",
            UnitKind::ALL
                .into_iter()
                .filter_map(|host| host.stats().bay)
                .map(|bay| Entry::Unit(bay.drone))
                .collect(),
        ),
        (
            "STRUCTURES",
            [
                BuildingKind::Foundry,
                BuildingKind::Fabricator,
                BuildingKind::Airworks,
                BuildingKind::Crucible,
                BuildingKind::Turret,
                BuildingKind::FlakTurret,
                BuildingKind::Bastion,
                BuildingKind::Barricade,
                BuildingKind::ScuttleCharge,
                BuildingKind::Extractor,
                BuildingKind::Reclaimer,
                BuildingKind::Array,
                BuildingKind::RepairBay,
                BuildingKind::Spire,
                BuildingKind::Lattice,
                BuildingKind::MawCradle,
                BuildingKind::RocCradle,
            ]
            .into_iter()
            .map(Entry::Building)
            .collect(),
        ),
    ]
}

/// How strongly a piece of a page reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    /// The description.
    Body,
    /// The headline figures.
    Primary,
    /// Weapons and notes.
    Secondary,
}

/// One piece of a page before it is set.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Piece {
    /// Prose that wraps to the page's width.
    Paragraph(String, Tone),
    /// A line of figures that stands alone.
    Line(String, Tone),
}

/// What a page says: description, figures, weapons, and what else the kind
/// does, the same lines the training tooltip shows.
pub fn page(entry: Entry) -> Vec<Piece> {
    let mut pieces = Vec::new();
    match entry {
        Entry::Unit(kind) => {
            pieces.push(Piece::Paragraph(unit_description(kind), Tone::Body));
            pieces.push(Piece::Line(String::new(), Tone::Body));
            pieces.push(Piece::Line(unit_stat_line(kind), Tone::Primary));
            for line in weapon_lines(kind)
                .into_iter()
                .chain(point_defense_line(kind))
            {
                pieces.push(Piece::Line(line, Tone::Secondary));
            }
            for line in unit_notes(kind) {
                pieces.push(Piece::Paragraph(line, Tone::Secondary));
            }
        }
        Entry::Building(kind) => {
            pieces.push(Piece::Paragraph(building_description(kind), Tone::Body));
            pieces.push(Piece::Line(String::new(), Tone::Body));
            pieces.push(Piece::Line(building_stat_line(kind), Tone::Primary));
            for line in building_weapon_lines(kind, 0) {
                pieces.push(Piece::Line(line, Tone::Secondary));
            }
            for line in building_notes(kind) {
                pieces.push(Piece::Paragraph(line, Tone::Secondary));
            }
        }
    }
    pieces
}

/// What a unit does besides fight, as page lines.
fn unit_notes(kind: UnitKind) -> Vec<String> {
    let stats = kind.stats();
    let mut notes = Vec::new();
    if let Some(harvest) = stats.harvest {
        notes.push(format!(
            "Carries {} scrap per trip and collects {:.0} scrap per second.",
            harvest.capacity,
            aftermine_sim::TICKS_PER_SECOND as f32 / harvest.ticks_per_scrap as f32
        ));
        notes.push("Repairs your structures and ground units.".to_string());
    }
    if stats.transport_capacity > 0 {
        notes.push(format!(
            "Carries up to {} transport slots of ground units.",
            stats.transport_capacity
        ));
    }
    if stats.transport_size > 1 {
        notes.push(format!("Takes {} transport slots.", stats.transport_size));
    }
    if stats.turn_rate > 0 {
        notes.push("Lands when idle.".to_string());
    }
    if let Some(radar) = stats.radar {
        notes.push(if radar.deploy_ticks > 0 {
            format!(
                "Its radar reaches {} tiles and works only while it stands still.",
                radar.radius
            )
        } else {
            format!("Its radar reaches {} tiles.", radar.radius)
        });
    }
    if stats.structural {
        notes.push("Sappers and other Maws treat it as a structure.".to_string());
    }
    if matches!(stats.mend, aftermine_sim::stats::Mend::Kiln(_)) {
        notes.push(
            "Repairs itself by eating nearby wrecks. Workers and Repair Bays can't repair it."
                .to_string(),
        );
    }
    if !stats.requires.is_empty() {
        notes.push(format!("Needs {}.", needs(stats.requires)));
    }
    if let Some(cradle) = BuildingKind::ALL
        .into_iter()
        .find(|b| b.assembles() == Some(kind))
    {
        notes.push(format!("Built in a {}.", entity_name(cradle.name())));
    }
    notes
}

/// What a structure does besides stand, as page lines: what it adds,
/// its requirements, what it trains, and its upgrade rungs.
fn building_notes(kind: BuildingKind) -> Vec<String> {
    let base = kind.base_stats();
    let seconds = |ticks: u32| ticks as f32 / aftermine_sim::TICKS_PER_SECOND as f32;
    let mut notes = building_economy_lines(kind);
    match kind {
        BuildingKind::Crucible => {
            notes.push(format!(
                "Melts wrecks within {} tiles into scrap.",
                tiles(aftermine_sim::stats::CRUCIBLE_SMELT_RADIUS)
            ));
            notes.push("Units you Retire are taken apart here for a refund.".to_string());
        }
        BuildingKind::ScuttleCharge => {
            notes.push(format!(
                "Deals {} damage to enemy ground units within {} tiles.",
                aftermine_sim::stats::CHARGE_DAMAGE,
                tiles(aftermine_sim::stats::CHARGE_BLAST_RADIUS)
            ));
            notes.push("Enemies can see it while it is being built.".to_string());
            notes.push(format!("{} reveal it.", series(&charge_finders())));
        }
        BuildingKind::Array => notes.push(format!(
            "Its radar reaches {} tiles. It reveals Scuttle Charges within {} tiles where you also have sight.",
            base.radar.unwrap_or(0),
            aftermine_sim::stats::CHARGE_BASE_ARRAY_DETECT_RADIUS
        )),
        BuildingKind::RepairBay => notes.push(format!(
            "Repairs up to {} units at a time within {} tiles, faster once they are out of combat.",
            aftermine_sim::stats::REPAIR_BAY_PATIENTS,
            tiles(aftermine_sim::stats::REPAIR_BAY_RADIUS)
        )),
        _ => {}
    }
    if let Some(magazine) = kind.magazine() {
        let round = if kind == BuildingKind::Spire {
            "warhead"
        } else {
            "interceptor"
        };
        notes.push(format!(
            "Holds {} {round}{}. Each costs {} scrap and takes {:.0} s to build.",
            magazine.capacity,
            if magazine.capacity == 1 { "" } else { "s" },
            magazine.round_cost,
            seconds(magazine.round_ticks),
        ));
    }
    if kind == BuildingKind::Spire {
        let warhead = aftermine_sim::stats::WARHEAD;
        let resisted = BuildingKind::Foundry.warhead_resistance_permille();
        notes.push(format!(
            "A warhead deals up to {} damage within {} tiles. Foundry armor blocks {} of it.",
            warhead.peak_damage,
            tiles(warhead.outer_radius),
            if resisted == 500 {
                "half".to_string()
            } else {
                format!("{}%", resisted / 10)
            },
        ));
    }
    if kind.battery().is_some() {
        notes.push("It only stops warheads that friendly sight or radar has tracked.".to_string());
    }
    notes.extend(cradle_crew_line(kind));
    let c = base.construction;
    if !c.requires.is_empty() {
        // You start with a Foundry, so its requirement is for another one.
        notes.push(if kind == BuildingKind::Foundry {
            format!("Building another Foundry needs {}.", needs(c.requires))
        } else {
            format!("Needs {}.", needs(c.requires))
        });
    }
    if !base.produces.is_empty() {
        let names: Vec<String> = base
            .produces
            .iter()
            .map(|u| entity_name(u.name()))
            .collect();
        notes.push(format!("Trains {}.", names.join(", ")));
    }
    for (tier, stats) in kind.tiers().iter().enumerate().skip(1) {
        let c = stats.construction;
        let tier = u8::try_from(tier).unwrap_or(u8::MAX);
        let name = entity_name(kind.tier_name(tier));
        let mut needed: Vec<String> = c.requires.iter().map(|p| article(p.name())).collect();
        if kind.upgrade_needs_foundry_support(tier - 1) {
            needed.push(format!(
                "a Foundry within {} tiles",
                aftermine_sim::stats::EXTRACTOR_SUPPORT_RADIUS
            ));
        }
        let mut line = format!(
            "Upgrades to {name} for {} scrap in {:.0} s.",
            c.cost,
            seconds(c.build_ticks)
        );
        if !needed.is_empty() {
            let _ = write!(line, " Needs {}.", series(&needed));
        }
        notes.push(line);
        notes.push(format!(
            "{name}: {}",
            upgrade_figures(kind, tier).join(" | ")
        ));
    }
    notes
}

/// What a structure's upgrade rung has, after its hp: its weapons, its
/// income, and any sight or reach it gains over the rung below.
fn upgrade_figures(kind: BuildingKind, tier: u8) -> Vec<String> {
    use aftermine_sim::{ExtractorIncome, ExtractorSupport};
    let stats = kind.tier_stats(tier);
    let below = kind.tier_stats(tier - 1);
    let mut figures = vec![format!("{} hp", stats.max_hp)];
    if stats.vision != below.vision {
        figures.push(format!("sight {}", stats.vision));
    }
    if stats.radar != below.radar
        && let Some(radar) = stats.radar
    {
        figures.push(format!("radar {radar}"));
    }
    figures.extend(building_weapon_lines(kind, tier));
    match kind {
        BuildingKind::Extractor => {
            let rate = |support| ExtractorIncome::new(tier, support).scrap_per_minute();
            figures.push(format!(
                "{} scrap/min, or {} with your Foundry nearby",
                rate(ExtractorSupport::Remote),
                rate(ExtractorSupport::Supported)
            ));
        }
        BuildingKind::Reclaimer => figures.push(format!(
            "{} scrap/min",
            60 * u64::from(aftermine_sim::TICKS_PER_SECOND) / aftermine_sim::stats::REFINERY_PERIOD
        )),
        BuildingKind::Array => figures.push(format!(
            "reveals Scuttle Charges within {} tiles where you also have sight",
            aftermine_sim::stats::CHARGE_ARRAY_DETECT_RADIUS
        )),
        _ => {}
    }
    figures
}

/// The units and structures that reveal hidden Scuttle Charges, plural,
/// in Roster order.
fn charge_finders() -> Vec<String> {
    sections()
        .into_iter()
        .flat_map(|(_, entries)| entries)
        .filter_map(|entry| match entry {
            Entry::Unit(kind) if kind.stats().charge_detection => Some(kind.name()),
            Entry::Building(BuildingKind::Array) => Some(BuildingKind::Array.name()),
            _ => None,
        })
        .map(|name| plural_name(&entity_name(name)))
        .collect()
}

/// "a Crucible, a Deep Bore, and an Airworks".
fn needs(requires: &[aftermine_sim::stats::Prerequisite]) -> String {
    let names: Vec<String> = requires.iter().map(|p| article(p.name())).collect();
    series(&names)
}

#[cfg(test)]
mod tests;
