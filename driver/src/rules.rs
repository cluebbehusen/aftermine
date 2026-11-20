//! The rules sheet: every Roster page as plain text, for a player who reads
//! the game through the debug protocol instead of the Roster screen.

use aftermine_kit::roster::{Entry, Piece, entity_name, page, sections, series, tick_time_label};
use aftermine_sim::stats::WeaponStats;
use std::fmt::Write as _;

/// Every Roster page in Roster order, as Markdown: a heading per section,
/// then each kind's name, its domain and price, and its page. Each page
/// ends with how often its weapons fire, which a player of the shell sees
/// and one reading the game through the protocol cannot.
pub fn sheet() -> String {
    let mut out = String::new();
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
