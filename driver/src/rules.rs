//! The rules sheet: every Roster page as plain text, for a player who reads
//! the game through the debug protocol instead of the Roster screen.

use aftermine_kit::roster::{Entry, Piece, entity_name, page, sections, series, tick_time_label};
use aftermine_sim::stats::{self, WeaponStats};
use std::fmt::Write as _;

/// Rules the Roster leaves to what a player watches happen: how reach,
/// support, repair, automatic welding, orders and progress work, for a
/// player who cannot watch.
fn mechanics() -> String {
    let per_minute = |(amount, period): (u32, u64)| u64::from(amount) * 1200 / period;
    let repair = stats::REPAIR_COST_PERMILLE / 10;
    let foundry_weld = stats::FOUNDRY_REPAIR_TICKS / 20;
    format!(
        "# How the game works

## Reach

Weapon range runs from the centre of the shooter, or the centre of an armed \
structure's footprint, to the centre of a target unit or to the nearest point \
of a target structure's footprint, the tile squares it covers. A target exactly \
at range is in reach, and minimum ranges count to the same point. The Maw and \
the Roc are hit at their edge rather than their centre. Contact weapons such as \
the Scuttler's reach only what their body touches.

## Foundry support

A finished Foundry raises its owner's finished Extractors within \
{support} tiles, counted edge to edge between the two footprints with diagonal \
steps counting as one, so footprints that touch are 1 apart. A supported \
Extractor pays {extractor_supported} scrap a minute instead of {extractor}, and \
a supported Deep Bore {bore_supported} instead of {bore}. More Foundries do not \
add up, and an ally's Foundry does not count. An Extractor pays nothing while it \
upgrades, and starting the upgrade needs that support.

## Repair

Welding costs {repair}% of the price per point of maximum health, for each \
point restored. A unit's price is its cost; a structure's is its current tier's \
build price, and the Foundry's a flat {foundry_price}. Each welder restores \
four fifths of a structure's maximum health over its build time, the Foundry's \
over {foundry_weld} s, and a unit's maximum health over its training time. \
Welders add up, and an Excavator welds no faster than a Harvester. A Repair Bay \
bills its healing at the same rate. An ordered weld with too little scrap in the \
bank stalls.

## Automatic welding

An idle Harvester or Excavator welds on its own: once it has stopped with \
nothing queued, it goes to the nearest wounded own unit standing still, or \
damaged own finished structure, within {tend} tiles of where it stopped, while \
the bank can pay. A stop order leaves a worker idle, so it starts again. Any \
new order interrupts it, and the set_idle_weld command (through send) turns it \
off for chosen workers.

## Armor

The Foundry takes half damage from warheads. Nothing else has armor.

## Public structures

Every seat knows where each Deep Bore stands from the moment its upgrade \
starts, and where each assembly cradle stands, and how far along it is, from \
its first work. The digest lists them under public structures.

## Orders

- A unit ordered to attack a unit or a structure stays on that target while \
  other enemies shoot it, until the target is gone. Only secondary weapons, \
  such as the Sentinel's anti-air gun, fire at others meanwhile. A unit that \
  picked its own fight, idle or hunting, looks again every half second.
- A hunting unit fights enemy units in reach first. It stops for a structure \
  only when no enemy unit in reach is a target, then attacks a visible enemy \
  structure, armed ones first, and marches on afterwards. Units that fire only \
  at aircraft never stop for structures, and the Maw takes only structures.
- New armed units hunt to their producer's rally tile, fighting on the way. \
  New Harvesters and Excavators go there too, and mine it when you know of \
  scrap or a wreck on it. A rally on a unit makes new units escort it.
- A Surveyor's radar comes up once it has stood still for 2 s, and its own \
  driving starts the count again. The Kestrel's radar needs no wait.

## Construction

A site's progress counts builder work: each Harvester at the site adds 1 a tick \
and each Excavator 2, and the structure stands when progress reaches its build \
time, which the digest shows as progress/total. A site's health starts at a \
fifth of its maximum and climbs with progress. Assembly cradles work the same \
way, with at most {crew} workers on their corner pads. An upgrade advances 1 a \
tick on its own.

",
        support = stats::EXTRACTOR_SUPPORT_RADIUS,
        extractor_supported = per_minute(stats::EXTRACTOR_SUPPORTED_YIELD),
        extractor = per_minute(stats::EXTRACTOR_REMOTE_YIELD),
        bore_supported = per_minute(stats::DEEP_BORE_SUPPORTED_YIELD),
        bore = per_minute(stats::DEEP_BORE_REMOTE_YIELD),
        foundry_price = stats::FOUNDRY_REPAIR_PRICE,
        tend = stats::ESCORT_ENGAGE_RADIUS.to_num::<i32>(),
        crew = stats::ASSEMBLY_CREW_CAP,
    )
}

/// How the game works where the Roster is silent, then every Roster page in
/// Roster order, as Markdown: a heading per section,
/// then each kind's name, its domain and price, and its page. Each page
/// ends with how often its weapons fire, which a player of the shell sees
/// and one reading the game through the protocol cannot.
pub fn sheet() -> String {
    let mut out = mechanics();
    for (title, entries) in sections() {
        let _ = writeln!(out, "# {title}\n");
        for entry in entries {
            let _ = writeln!(out, "## {}\n\n{}\n", entry.name(), entry.header());
            for piece in page(entry) {
                match piece {
                    Piece::Paragraph(text, _) | Piece::Line(text, _) => {
                        let _ = writeln!(out, "{text}");
                    }
                }
            }
            for line in fire_intervals(entry) {
                let _ = writeln!(out, "{line}");
            }
            out.push('\n');
        }
    }
    out
}

/// How often each of `entry`'s weapons fires, at every upgrade rung that
/// has weapons.
fn fire_intervals(entry: Entry) -> Vec<String> {
    match entry {
        Entry::Unit(kind) => fire_interval("Fires", kind.stats().weapons)
            .into_iter()
            .collect(),
        Entry::Building(kind) => kind
            .tiers()
            .iter()
            .enumerate()
            .filter_map(|(tier, stats)| {
                let subject = match u8::try_from(tier) {
                    Ok(0) => "Fires".to_string(),
                    Ok(tier) => format!("{} fires", entity_name(kind.tier_name(tier))),
                    Err(_) => return None,
                };
                fire_interval(&subject, stats.weapons)
            })
            .collect(),
    }
}

/// One line for `weapons`, in the order their weapon lines list them; guns
/// listed together share one interval.
fn fire_interval(subject: &str, weapons: &[WeaponStats]) -> Option<String> {
    let mut intervals: Vec<String> = Vec::new();
    let mut last = None;
    for weapon in weapons {
        let gun = (weapon.damage, weapon.range, weapon.cooldown_ticks);
        if last != Some(gun) {
            intervals.push(tick_time_label(weapon.cooldown_ticks));
        }
        last = Some(gun);
    }
    match intervals.as_slice() {
        [] => None,
        [one] => Some(format!("{subject} once every {one}.")),
        many => Some(format!(
            "{subject} its weapons, in the order listed, once every {}.",
            series(many)
        )),
    }
}

#[cfg(test)]
mod tests;
