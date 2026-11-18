//! What each unit and structure is, in one or two plain sentences, for
//! the Roster and the training and build tooltips.
//!
//! The typed words hold no figures. Each `{}` is filled from the stats
//! table, so a balance change cannot leave a description wrong.

use aftermine_sim::stats::{BuildingKind, UnitKind};
use chassis::fx::Fx;

/// A description before its figures are filled in.
struct Text {
    /// The typed words, with `{}` where each figure goes.
    words: &'static str,
    /// The figures, in order, already formatted.
    figures: Vec<String>,
}

impl Text {
    const fn plain(words: &'static str) -> Self {
        Self {
            words,
            figures: Vec::new(),
        }
    }

    fn with(words: &'static str, figures: impl IntoIterator<Item = String>) -> Self {
        Self {
            words,
            figures: figures.into_iter().collect(),
        }
    }

    fn fill(&self) -> String {
        let mut figures = self.figures.iter();
        let mut parts = self.words.split("{}");
        let mut out = parts.next().unwrap_or_default().to_string();
        for part in parts {
            out.push_str(figures.next().map_or("", String::as_str));
            out.push_str(part);
        }
        out
    }
}

/// A unit's description.
pub fn unit_description(kind: UnitKind) -> String {
    unit_text(kind).fill()
}

/// A structure's description.
pub fn building_description(kind: BuildingKind) -> String {
    building_text(kind).fill()
}

fn unit_text(kind: UnitKind) -> Text {
    let stats = kind.stats();
    let reveal = || [aftermine_sim::stats::CHARGE_SCOUT_DETECT_RADIUS.to_string()];
    match kind {
        UnitKind::Harvester => {
            Text::plain("Collects scrap and brings it to a Foundry. It also builds and repairs.")
        }
        UnitKind::Sentinel => Text::plain(
            "Cheap front-line unit with a short-range gun. It can also shoot aircraft, but weakly.",
        ),
        UnitKind::Scuttler => Text::plain(
            "Fast, cheap raider with little health. Good for attacking Harvesters and unguarded artillery.",
        ),
        UnitKind::Excavator => {
            Text::plain("Heavy worker that collects scrap, builds quickly, and repairs.")
        }
        UnitKind::Surveyor => Text::with(
            "Fast, cheap, unarmed scout with long sight. Reveals hidden Scuttle Charges within {} tiles.",
            reveal(),
        ),
        UnitKind::Lancer => Text::plain("Long-range gun that hits hard but fires slowly."),
        UnitKind::Bombard => Text::plain(
            "Artillery whose shells arc over rock and damage an area. Its range is longer than its sight, so distant targets need a friendly spotter.",
        ),
        UnitKind::Flakhound => Text::plain("Armored anti-air vehicle."),
        UnitKind::Warden => Text::plain("Armored front-line tank."),
        UnitKind::Sapper => stats.demolition.map_or(
            Text::plain("Runs at its target and explodes."),
            |blast| {
                Text::with(
                    "Runs at its target and explodes. Deals {} damage to a structure and {} to enemy ground units nearby.",
                    [
                        blast.structure_damage.to_string(),
                        blast.splash_damage.to_string(),
                    ],
                )
            },
        ),
        UnitKind::Buzzard => Text::plain("Heavy ground-attack aircraft."),
        UnitKind::Talon => Text::plain("Air-superiority fighter."),
        UnitKind::Kestrel => Text::with(
            "Fast, unarmed scout aircraft with long sight. Reveals hidden Scuttle Charges within {} tiles.",
            reveal(),
        ),
        UnitKind::Condor => Text::plain(
            "Heavy bomber. Drops one large bomb on each pass, then turns around for the next.",
        ),
        UnitKind::Skyhook => Text::plain("Unarmed transport aircraft."),
        UnitKind::Breaker => Text::plain("Heavy assault walker."),
        UnitKind::Avalanche => Text::plain(
            "Long-range missile artillery. Its range is longer than its sight, so distant targets need a friendly spotter.",
        ),
        UnitKind::Mantle => {
            Text::plain("Support vehicle that shoots down enemy shells, missiles, and bombs.")
        }
        UnitKind::Piledriver => {
            Text::plain("Heavy siege gun. It must stop and plant itself before it fires.")
        }
        UnitKind::Maw => Text::plain(
            "Huge siege vehicle that cuts through structures and crushes small units in its path.",
        ),
        UnitKind::Roc => Text::with(
            "Heavy gunship with {} anti-air drones.",
            [count_word(stats.bay.map_or(0, |bay| u32::from(bay.capacity)))],
        ),
        UnitKind::Drone => Text::plain("Anti-air drone carried by a Roc."),
    }
}

fn building_text(kind: BuildingKind) -> Text {
    match kind {
        BuildingKind::Foundry => Text::plain(
            "Headquarters and scrap drop-off. You lose when all your Foundries are destroyed.",
        ),
        BuildingKind::Fabricator => {
            Text::plain("Advanced unit factory. It also unlocks more structures.")
        }
        BuildingKind::Airworks => Text::plain("Aircraft factory."),
        BuildingKind::Crucible => {
            Text::plain("Heavy unit factory. It also unlocks the top upgrades.")
        }
        BuildingKind::Turret => Text::plain("Ground defense turret."),
        BuildingKind::FlakTurret => Text::plain("Anti-air defense turret."),
        BuildingKind::Bastion => Text::plain(
            "Artillery emplacement. Its range is longer than its sight, so distant targets need a friendly spotter.",
        ),
        BuildingKind::Barricade => {
            Text::plain("Wall that blocks ground movement. Shots pass over it.")
        }
        BuildingKind::ScuttleCharge => {
            Text::plain("Hidden explosive that goes off when an enemy ground unit steps on it.")
        }
        BuildingKind::Extractor => {
            Text::plain("Build it on a derelict Extractor frame to make scrap.")
        }
        BuildingKind::Reclaimer => Text::plain("Makes a small amount of scrap on its own."),
        BuildingKind::Array => {
            Text::plain("Sensor tower with radar. Reveals hidden Scuttle Charges.")
        }
        BuildingKind::RepairBay => Text::plain("Repairs your nearby units and structures."),
        BuildingKind::Spire => Text::plain(
            "Missile silo. Launches a warhead at any tile on the map. Only a Lattice can stop it.",
        ),
        BuildingKind::Lattice => Text::with(
            "Shoots down enemy warheads that would land within {} tiles.",
            [tiles(
                kind.battery().map_or(Fx::ZERO, |battery| battery.ring),
            )],
        ),
        BuildingKind::MawCradle => {
            Text::plain("Workers build a Maw here. Every player can see it once work starts.")
        }
        BuildingKind::RocCradle => {
            Text::plain("Workers build a Roc here. Every player can see it once work starts.")
        }
    }
}

/// A distance in tiles, to a tenth, without a trailing `.0`.
pub fn tiles(distance: Fx) -> String {
    let text = format!("{:.1}", distance.to_num::<f32>());
    text.strip_suffix(".0")
        .map_or_else(|| text.clone(), str::to_string)
}

/// A small count as a word, the way prose writes it.
fn count_word(count: u32) -> String {
    const WORDS: [&str; 13] = [
        "no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ];
    WORDS
        .get(count as usize)
        .map_or_else(|| count.to_string(), |word| (*word).to_string())
}

#[cfg(test)]
mod tests;
