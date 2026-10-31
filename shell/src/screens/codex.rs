//! The codex: every machine and works in the game, with its sprite, its
//! description, and its figures. One screen object over the shared menu
//! widget: the list is the cursor, the selected row is the page.
//! Windowless update; the coordinator holds the displaced screen and
//! restores it wholesale on leave, as for Settings.

use crate::assets::Sprites;
use crate::game::SoundKind;
use crate::menu::{Label, Line, Menu};
use crate::numeric;
use crate::numeric::Fit;
use crate::panel::{
    building_economy_lines, building_flavor, building_stat_line, building_weapon_lines,
    point_defense_line, unit_flavor, unit_stat_line, weapon_lines,
};
use crate::render;
use crate::theme::{
    BORDER_STRONG, CHIP, SURFACE_MENU, Stroke, TEXT_ACCENT, TEXT_BODY, TEXT_PRIMARY, TEXT_SECONDARY,
};
use crate::typography::entity_name;
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
    /// A machine.
    Unit(UnitKind),
    /// A works.
    Building(BuildingKind),
}

/// The codex in reading order: the factories as the player unlocks
/// them, each followed by what it trains, then every works. The
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
            "BUILDINGS",
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
    /// Opens on the first machine. Where leaving lands is the
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
        self.menu
            .draw("every machine and works, in the order the factories unlock them");
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
                para(unit_flavor(kind), TEXT_BODY, &mut lines);
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
                para(building_flavor(kind), TEXT_BODY, &mut lines);
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

/// What a machine does besides fight, as page lines.
fn unit_notes(kind: UnitKind) -> Vec<String> {
    let stats = kind.stats();
    let mut notes = Vec::new();
    if let Some(harvest) = stats.harvest {
        notes.push(format!(
            "Hauls {} scrap a trip; digs {:.0} scrap/s.",
            harvest.capacity,
            aftermine_sim::TICKS_PER_SECOND as f32 / harvest.ticks_per_scrap as f32
        ));
    }
    if stats.harvest.is_some() {
        notes.push(if stats.build_rate > 1 {
            format!(
                "Welds: repairs machines and raises buildings at {}x pace.",
                stats.build_rate
            )
        } else {
            "Welds: repairs machines and raises buildings.".to_string()
        });
        notes.push(
            "Auto-weld: idle or patrolling, mends wounded machines and buildings nearby."
                .to_string(),
        );
    }
    if stats.transport_capacity > 0 {
        notes.push(format!(
            "Lifts {} sling points of ground machines.",
            stats.transport_capacity
        ));
    }
    if stats.transport_size > 1 {
        notes.push(format!(
            "Takes {} sling points to lift.",
            stats.transport_size
        ));
    }
    if stats.turn_rate > 0 {
        notes.push(
            "Sets down on open ground at the end of a move when nothing is in reach; sits as a ground target until its next order lifts it off."
                .to_string(),
        );
    }
    if stats.demolition.is_some() {
        notes.push("Detonates on its target; always fatal to itself.".to_string());
    }
    if let Some(radar) = stats.radar {
        notes.push(if radar.deploy_ticks > 0 {
            format!(
                "Radar: after standing still {:.0} s, unfolds an antenna that tracks unseen enemies within {} tiles; moving folds it.",
                f32::from(radar.deploy_ticks) / aftermine_sim::TICKS_PER_SECOND as f32,
                radar.radius
            )
        } else {
            format!(
                "Radar: tracks unseen enemies within {} tiles as air or ground contacts.",
                radar.radius
            )
        });
    }
    if stats.target_priority == aftermine_sim::stats::TargetPriority::Heaviest {
        notes.push(
            "Left to itself, picks the machine in reach with the most full health, then the nearest."
                .to_string(),
        );
    }
    if stats.target_priority == aftermine_sim::stats::TargetPriority::Buildings {
        notes.push(
            "Left to itself, cuts the nearest building in reach. Strikes a machine only when one presses against its hull and never chases one, but goes after a building that fires on it."
                .to_string(),
        );
    }
    if stats.structural {
        notes.push(
            "Built like a structure: demolition charges and building cutters deal it their structure damage."
                .to_string(),
        );
    }
    if let aftermine_sim::stats::Mend::Kiln(kiln) = stats.mend {
        notes.push(format!(
            "Takes no welds and no Repair Bay care. Its kiln eats wreck under and beside its hull, {} hp per scrap, one bite every {}.",
            kiln.hp_per_wreck,
            crate::panel::tick_time_label(
                u32::try_from(aftermine_sim::stats::KILN_BITE_PERIOD).unwrap_or(u32::MAX)
            ),
        ));
    }
    if !stats.requires.is_empty() {
        let names: Vec<String> = stats
            .requires
            .iter()
            .map(|b| entity_name(b.name()))
            .collect();
        notes.push(format!("Needs a standing {}.", names.join(" and ")));
    }
    let factory = [
        BuildingKind::Foundry,
        BuildingKind::Fabricator,
        BuildingKind::Airworks,
        BuildingKind::Crucible,
    ]
    .into_iter()
    .find(|b| b.base_stats().produces.contains(&kind));
    if let Some(factory) = factory {
        notes.push(format!("Trained at the {}.", entity_name(factory.name())));
    }
    if let Some(cradle) = BuildingKind::ALL
        .into_iter()
        .find(|b| b.assembles() == Some(kind))
    {
        notes.push(format!("Assembled in a {}.", entity_name(cradle.name())));
    }
    notes
}

/// What a works does besides stand, as page lines: its requirements,
/// what it trains, and its upgrade rungs.
fn building_notes(kind: BuildingKind) -> Vec<String> {
    let base = kind.base_stats();
    let mut notes = building_economy_lines(kind);
    if kind == BuildingKind::RepairBay {
        notes.push(format!(
            "Repairs its {} most damaged machines nearby at {:.0}% of max hp per second once they have been out of combat for {:.0} s, or {:.1} hp/s under fire. Buildings repair at the slower rate.",
            aftermine_sim::stats::REPAIR_BAY_PATIENTS,
            aftermine_sim::stats::REPAIR_BAY_FAST_PERMILLE_PER_SECOND as f32 / 10.0,
            aftermine_sim::stats::REPAIR_BAY_CALM_TICKS as f32 / aftermine_sim::TICKS_PER_SECOND as f32,
            aftermine_sim::stats::REPAIR_BAY_STEP as f32 * aftermine_sim::TICKS_PER_SECOND as f32
                / aftermine_sim::stats::REPAIR_BAY_PERIOD as f32
        ));
    }
    if let Some(magazine) = kind.magazine() {
        let seconds = |ticks: u32| ticks as f32 / aftermine_sim::TICKS_PER_SECOND as f32;
        let round = if kind == BuildingKind::Spire {
            "warhead"
        } else {
            "interceptor"
        };
        notes.push(format!(
            "Holds {} {round}{}. Each is ordered and paid for like a unit, {} scrap and {:.0} s, built one at a time. Autofill, {} by default, orders the next whenever there is room and scrap.",
            magazine.capacity,
            if magazine.capacity == 1 { "" } else { "s" },
            magazine.round_cost,
            seconds(magazine.round_ticks),
            if magazine.autofill { "on" } else { "off" },
        ));
    }
    if let Some(machine) = kind.assembles() {
        notes.push(format!(
            "Paid in full when placed. Up to {} workers raise it; a faster worker takes a slower one's place. Every player sees it and its progress once work starts. Finished, it comes apart and a {} stands where it stood.",
            aftermine_sim::stats::ASSEMBLY_CREW_CAP,
            entity_name(machine.name())
        ));
    }
    if kind == BuildingKind::Spire {
        let warhead = aftermine_sim::stats::WARHEAD;
        notes.push(format!(
            "Launches at any tile, seen or not, once its deck has turned to face it and its locks release. The warhead flies {:.0} tiles/s and deals {} damage within {:.1} tiles, fading to nothing at {:.1}. Foundries take half.",
            warhead.speed.to_num::<f32>() * aftermine_sim::TICKS_PER_SECOND as f32,
            warhead.peak_damage,
            warhead.full_radius.to_num::<f32>(),
            warhead.outer_radius.to_num::<f32>(),
        ));
    }
    if let Some(battery) = kind.battery() {
        notes.push(format!(
            "Stops enemy warheads that will land within {:.0} tiles, once sight or radar has held them for {:.1} s.",
            battery.ring.to_num::<f32>(),
            f32::from(aftermine_sim::stats::WARHEAD_TRACK_TICKS + aftermine_sim::stats::WARHEAD_LOCK_TICKS)
                / aftermine_sim::TICKS_PER_SECOND as f32,
        ));
    }
    if let Some(c) = base.construction
        && !c.requires.is_empty()
    {
        let names: Vec<String> = c.requires.iter().map(|b| entity_name(b.name())).collect();
        notes.push(format!("Needs a standing {}.", names.join(" and ")));
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
        let mut line = format!(
            "Upgrade: {} for {} scrap ({:.0} s){} - {} hp",
            entity_name(kind.tier_name(tier.fit::<u8>())),
            c.cost,
            c.build_ticks as f32 / aftermine_sim::TICKS_PER_SECOND as f32,
            if c.requires.is_empty() {
                String::new()
            } else {
                let mut names: Vec<String> =
                    c.requires.iter().map(|b| entity_name(b.name())).collect();
                if kind.upgrade_needs_foundry_support(tier.fit::<u8>() - 1) {
                    names.push(format!(
                        "{} within {} tiles",
                        entity_name(BuildingKind::Foundry.name()),
                        aftermine_sim::stats::EXTRACTOR_SUPPORT_RADIUS
                    ));
                }
                format!(", needs a {}", names.join(" and a "))
            },
            stats.max_hp
        );
        for weapon in building_weapon_lines(kind, tier.fit::<u8>()) {
            let _ = write!(line, "; {weapon}");
        }
        notes.push(line);
    }
    notes
}

#[cfg(test)]
mod tests;
