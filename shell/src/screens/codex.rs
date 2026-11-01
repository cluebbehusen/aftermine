//! The codex (the Roster): every unit and structure in the game, with its
//! sprite, its description, and its figures. One screen object over the shared menu
//! widget: the list is the cursor, the selected row is the page.
//! Windowless update; the coordinator holds the displaced screen and
//! restores it wholesale on leave, as for Settings.

use crate::assets::Sprites;
use crate::description::{building_description, tiles, unit_description};
use crate::game::SoundKind;
use crate::menu::{Label, Line, Menu};
use crate::numeric;
use crate::numeric::Fit;
use crate::panel::{
    building_economy_lines, building_stat_line, building_weapon_lines, cradle_crew_line,
    point_defense_line, unit_stat_line, weapon_lines,
};
use crate::render;
use crate::theme::{
    BORDER_STRONG, CHIP, SURFACE_MENU, Stroke, TEXT_ACCENT, TEXT_BODY, TEXT_PRIMARY, TEXT_SECONDARY,
};
use crate::typography::{entity_name, plural_name};
use aftermine_protocol::{Key, RawEvent};
use aftermine_sim::stats::{BuildingKind, Domain, UnitKind};
use macroquad::prelude::*;
use std::fmt::Write as _;

/// What a codex frame decided.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// Still reading.
    Stay,
    /// Back to wherever the screen was opened from.
    Leave,
}

/// One page of the codex.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entry {
    /// A unit.
    Unit(UnitKind),
    /// A structure.
    Building(BuildingKind),
}

/// The codex in reading order: the factories as the player unlocks
/// them, each followed by what it trains, then every structure. The
/// codex's section order is the tech order, so a new player reads the
/// game the way a match unfolds.
fn sections() -> Vec<(&'static str, Vec<Entry>)> {
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

/// The codex screen: the list and what each row opens.
pub struct CodexScreen {
    /// The live list: section headers and one row per kind, each
    /// standing for its page.
    pub menu: Menu<Entry>,
    back: crate::button::BackButton,
}

impl CodexScreen {
    /// Opens on the first unit. Where leaving lands is the
    /// coordinator's business.
    pub fn open() -> Self {
        let mut lines = Vec::new();
        for (title, section) in sections() {
            lines.push(Line::Header(title.to_string()));
            for entry in section {
                let name = match entry {
                    Entry::Unit(kind) => kind.name(),
                    Entry::Building(kind) => kind.name(),
                };
                lines.push(Line::Row(Label::Text(entity_name(name)), entry));
            }
        }
        // The list shifts left to make room for the page beside it.
        let mut menu = Menu::new("ROSTER", lines);
        menu.shift = -0.24;
        Self {
            menu,
            back: crate::button::BackButton::default(),
        }
    }

    /// The debug protocol's stable mode name.
    #[expect(
        clippy::unused_self,
        reason = "other screens answer this from their state"
    )]
    pub fn mode_name(&self) -> &'static str {
        "codex"
    }

    /// The page under the cursor, if the cursor is on a kind.
    pub fn selected_entry(&self) -> Option<Entry> {
        self.menu.value().copied()
    }

    /// Applies a frame's events. Rows are pages, not verbs: moving the
    /// cursor turns the page, and only the BACK button or Escape acts.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        let (back, events) = self.back.route(events);
        if back {
            sounds.push((SoundKind::Click, None));
            return Out::Leave;
        }
        if events
            .iter()
            .any(|e| matches!(e, RawEvent::KeyDown { key: Key::Escape }))
        {
            return Out::Leave;
        }
        if self.menu.handle(&events, mouse).is_some() {
            sounds.push((SoundKind::Click, None));
        }
        Out::Stay
    }

    /// Draws the list and the selected page — the caller draws the
    /// veil first, so both land above it.
    pub fn draw(&self, sprites: &Sprites) {
        self.menu.draw("");
        let Some(entry) = self.selected_entry() else {
            return;
        };
        let s = render::ui_scale();
        let view = render::viewport();
        // The page sits right of the shifted list, between the title
        // zone and the hint line, and fits its own content.
        let x = view.x * 0.53;
        let w = (view.x * 0.44).min(560.0 * s);
        let top = (view.y * 0.36).max(view.y * 0.28 + 64.0 * s);
        let bottom = view.y - 64.0 * s;
        let pad = 14.0 * s;
        let plate = 84.0 * s;
        let plate_x = x + pad;
        let plate_y = top + pad;

        let name = match entry {
            Entry::Unit(kind) => entity_name(kind.name()),
            Entry::Building(kind) => entity_name(kind.name()),
        };
        let role = match entry {
            Entry::Unit(kind) => {
                let stats = kind.stats();
                let domain = match stats.domain {
                    Domain::Ground => "ground",
                    Domain::Air => "air",
                };
                format!("{domain} | {} scrap", stats.cost)
            }
            Entry::Building(kind) => kind
                .base_stats()
                .construction
                .map_or_else(String::new, |c| format!("{} scrap", c.cost)),
        };

        // The page: description, figures, weapons, and what else the
        // kind does — the same lines the training tooltip shows.
        let body_size = crate::theme::Type::Body.at(s);
        let line_h = 20.0 * s;
        let body_w = w - pad * 2.0;
        let measure = |t: &str| measure_text(t, None, numeric::font_size(body_size), 1.0).width;
        let mut lines: Vec<(String, Color)> = Vec::new();
        let para = |text: &str, color: Color, lines: &mut Vec<(String, Color)>| {
            for line in render::wrap_words(text, measure, body_w) {
                lines.push((line, color));
            }
        };
        match entry {
            Entry::Unit(kind) => {
                para(&unit_description(kind), TEXT_BODY, &mut lines);
                lines.push((String::new(), TEXT_BODY));
                lines.push((unit_stat_line(kind), TEXT_PRIMARY));
                for line in weapon_lines(kind)
                    .into_iter()
                    .chain(point_defense_line(kind))
                {
                    lines.push((line, TEXT_SECONDARY));
                }
                for line in unit_notes(kind) {
                    para(&line, TEXT_SECONDARY, &mut lines);
                }
            }
            Entry::Building(kind) => {
                para(&building_description(kind), TEXT_BODY, &mut lines);
                lines.push((String::new(), TEXT_BODY));
                lines.push((building_stat_line(kind), TEXT_PRIMARY));
                for line in building_weapon_lines(kind, 0) {
                    lines.push((line, TEXT_SECONDARY));
                }
                for line in building_notes(kind) {
                    para(&line, TEXT_SECONDARY, &mut lines);
                }
            }
        }

        // The box fits its page: never shorter than the plate row,
        // never past the hint line.
        let text_top = plate_y + plate + pad + line_h;
        let box_bottom = (text_top + lines.len() as f32 * line_h + pad * 0.5).min(bottom);
        draw_rectangle(x, top, w, box_bottom - top, SURFACE_MENU);
        draw_rectangle_lines(
            x,
            top,
            w,
            box_bottom - top,
            Stroke::Edge.at(s),
            BORDER_STRONG,
        );
        draw_rectangle(plate_x, plate_y, plate, plate, CHIP);
        let dest = Rect::new(plate_x, plate_y, plate, plate);
        match entry {
            Entry::Unit(kind) => {
                sprites.draw_portrait(dest, &[(sprites.unit(kind), WHITE)]);
            }
            Entry::Building(kind) => {
                let mut layers = vec![(sprites.building(kind), WHITE)];
                if let Some(mount) = sprites.defense_mount(kind, 0) {
                    layers.push((mount, WHITE));
                }
                sprites.draw_portrait(dest, &layers);
            }
        }
        let text_x = plate_x + plate + pad;
        draw_text(
            &name,
            text_x,
            plate_y + 30.0 * s,
            crate::theme::Type::Title.at(s),
            TEXT_PRIMARY,
        );
        draw_text(
            &role,
            text_x,
            plate_y + 54.0 * s,
            crate::theme::Type::Body.at(s),
            TEXT_ACCENT,
        );
        let mut y = text_top;
        for (line, color) in lines {
            if y > box_bottom - pad * 0.5 {
                break;
            }
            draw_text(&line, x + pad, y, body_size, color);
            y += line_h;
        }
    }
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
            "Its radar reaches {} tiles. It reveals Scuttle Charges within {} tiles.",
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
    if let Some(c) = base.construction
        && !c.requires.is_empty()
    {
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
        let Some(c) = stats.construction else {
            continue;
        };
        let tier = tier.fit::<u8>();
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
            "reveals Scuttle Charges within {} tiles",
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

/// A name with its article: "a Crucible", "an Airworks".
fn article(name: &str) -> String {
    let name = entity_name(name);
    let vowel = name.starts_with(['A', 'E', 'I', 'O', 'U']);
    format!("{} {name}", if vowel { "an" } else { "a" })
}

/// "x", "x and y", or "x, y, and z".
fn series(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [first, second] => format!("{first} and {second}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests;
