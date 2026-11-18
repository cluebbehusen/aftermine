//! A seat's fog view as compact text, for a client that reads the game
//! instead of seeing it: an agent playing through `driver live`.
//!
//! Everything here comes from one [`FogView`], so it shows exactly what the
//! seat knows. The optional map adds terrain from the scenario file, which
//! every seat knows before the match starts, and never live scrap or wrecks.

use aftermine_protocol::{BuildingView, FogView, PlayedView, UnitView};
use aftermine_sim::map::Map;
use aftermine_sim::vision::ContactTrack;
use chassis::grid::TilePos;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// What an order's target can be named by: the seat's contact tracks by
/// id, so an order aimed at a contact names the unit behind it while the
/// seat sees that unit, and the structures in sight, so an order aimed at a
/// remembered structure names it while the seat sees it.
struct Tracks<'a> {
    contacts: BTreeMap<u64, &'a ContactTrack>,
    buildings: &'a [BuildingView],
}

/// `view` as text: the clock and bank, own units and structures, then
/// everything the seat knows of other seats and the ground, and with
/// `terrain`, a map of all of it.
pub fn render(view: &FogView, terrain: Option<&Map>) -> String {
    let mut out = String::new();
    let tracks = Tracks {
        contacts: view
            .contact_tracks
            .iter()
            .map(|track| (u64::from(track.id.0), track))
            .collect(),
        buildings: &view.buildings,
    };
    let seconds = view.tick / u64::from(aftermine_sim::TICKS_PER_SECOND);
    let _ = writeln!(
        out,
        "TICK {} ({}:{:02}) | SEAT {} | SCRAP {}",
        view.tick,
        seconds / 60,
        seconds % 60,
        view.player,
        view.own_player.scrap
    );
    if view.own_player.resigned || view.own_player.eliminated_at.is_some() {
        let _ = writeln!(out, "OUT OF THE MATCH");
    }

    let (own, others): (Vec<&UnitView>, Vec<&UnitView>) = view
        .units
        .iter()
        .partition(|unit| unit.player == view.player);
    let _ = writeln!(out, "\nOWN UNITS ({})", own.len());
    let mut by_kind: BTreeMap<&str, Vec<&UnitView>> = BTreeMap::new();
    for unit in &own {
        by_kind.entry(unit.kind.name()).or_default().push(unit);
    }
    for (kind, units) in by_kind {
        let _ = writeln!(out, "  {kind} x{}", units.len());
        for unit in units {
            let _ = writeln!(out, "    {}", own_unit(unit, &tracks));
        }
    }
    let idle: Vec<String> = own
        .iter()
        .filter(|unit| unit.kind.stats().harvest.is_some() && is_idle(unit))
        .map(|unit| unit.id.to_string())
        .collect();
    if !idle.is_empty() {
        let _ = writeln!(out, "  idle workers: {}", idle.join(", "));
    }

    let (own_buildings, other_buildings): (Vec<&BuildingView>, Vec<&BuildingView>) = view
        .buildings
        .iter()
        .partition(|building| building.player == view.player);
    let _ = writeln!(out, "\nOWN STRUCTURES ({})", own_buildings.len());
    for building in own_buildings {
        let _ = writeln!(out, "  {}", own_building(building, &tracks));
    }

    section(
        &mut out,
        "OTHER SEATS' UNITS IN SIGHT",
        others.iter(),
        |unit| {
            format!(
                "{} {} seat {} at {} hp{}",
                unit.id,
                unit.kind.name(),
                unit.player,
                position(unit.pos),
                unit.hp
            )
        },
    );
    section(
        &mut out,
        "OTHER SEATS' STRUCTURES IN SIGHT",
        other_buildings.iter(),
        |building| {
            format!(
                "{} {} seat {} at {} hp{}{}",
                building.id,
                building.kind.tier_name(building.tier),
                building.player,
                tile(building.anchor),
                building.hp,
                if building.built {
                    String::new()
                } else {
                    construction(building)
                }
            )
        },
    );
    let remembered = view.ghosts.iter().filter(|ghost| {
        !view.buildings.iter().any(|building| {
            building.player == ghost.owner
                && building.kind == ghost.kind
                && building.anchor == ghost.anchor
        })
    });
    section(
        &mut out,
        "REMEMBERED STRUCTURES (last seen)",
        remembered,
        |ghost| {
            format!(
                "{} seat {} at {} hp{}{}",
                ghost.kind.tier_name(ghost.tier),
                ghost.owner,
                tile(ghost.anchor),
                ghost.hp,
                if ghost.built { "" } else { " unbuilt" }
            )
        },
    );
    section(
        &mut out,
        "PUBLIC STRUCTURES (every seat knows these)",
        view.public_buildings.iter(),
        |public| {
            format!(
                "{} seat {} at {}{}",
                public.kind.tier_name(public.tier),
                public.owner,
                tile(public.anchor),
                public
                    .progress
                    .map_or_else(String::new, |progress| format!(" progress {progress}"))
            )
        },
    );
    let dark_tracks = view
        .contact_tracks
        .iter()
        .filter(|track| track.visible_unit.is_none());
    section(&mut out, "RADAR CONTACTS", dark_tracks, |track| {
        format!("{:?} at ({},{})", track.domain, track.tile.x, track.tile.y).to_lowercase()
    });
    if !view.incoming_shells.is_empty() {
        let tiles: Vec<String> = view.incoming_shells.iter().map(|at| tile(*at)).collect();
        let _ = writeln!(out, "\nINCOMING SHELLS land at {}", tiles.join(" "));
    }
    section(&mut out, "MAGAZINES", view.magazines.iter(), |known| {
        compact(known, &tracks)
    });
    section(&mut out, "WARHEADS", view.warheads.iter(), |known| {
        compact(known, &tracks)
    });
    remembered_tiles(&mut out, "REMEMBERED SCRAP", &view.scrap);
    remembered_tiles(&mut out, "REMEMBERED WRECKS", &view.wrecks);

    let mask = view.mask.concat();
    let _ = writeln!(
        out,
        "\nVISION visible {} | explored but dark {} | never seen {}",
        mask.matches('*').count(),
        mask.matches('.').count(),
        mask.matches(' ').count()
    );
    if let Some(map) = terrain {
        out.push('\n');
        out.push_str(&grid(view, map));
    }
    out
}

/// A header and one indented line per item, or nothing when there are no
/// items.
fn section<T>(
    out: &mut String,
    title: &str,
    items: impl Iterator<Item = T>,
    line: impl Fn(T) -> String,
) {
    let lines: Vec<String> = items.map(line).collect();
    if lines.is_empty() {
        return;
    }
    let _ = writeln!(out, "\n{title} ({})", lines.len());
    for text in lines {
        let _ = writeln!(out, "  {text}");
    }
}

fn remembered_tiles(
    out: &mut String,
    title: &str,
    tiles: &[aftermine_protocol::RememberedTileView],
) {
    if tiles.is_empty() {
        return;
    }
    let total: u64 = tiles.iter().map(|known| u64::from(known.amount)).sum();
    let listed: Vec<String> = tiles
        .iter()
        .map(|known| format!("{}={}", tile(known.tile), known.amount))
        .collect();
    let _ = writeln!(
        out,
        "\n{title} ({} tiles, {total} scrap)\n  {}",
        tiles.len(),
        listed.join(" ")
    );
}

fn own_unit(unit: &UnitView, tracks: &Tracks) -> String {
    let mut line = format!("{} at {} hp{}", unit.id, position(unit.pos), unit.hp);
    if unit.carrying > 0 {
        let _ = write!(line, " carrying {}", unit.carrying);
    }
    if let Some(charges) = unit.charges {
        let _ = write!(line, " charges {charges}");
    }
    if let Some(host) = unit.host {
        let _ = write!(line, " aboard {host}");
    }
    if unit.landed {
        line.push_str(" landed");
    }
    let _ = write!(
        line,
        " | {}",
        unit.order
            .as_ref()
            .map_or_else(|| "idle".to_string(), |order| compact(order, tracks))
    );
    if !unit.queue.is_empty() {
        let queued: Vec<String> = unit
            .queue
            .iter()
            .map(|order| compact(order, tracks))
            .collect();
        let _ = write!(line, " then {}", queued.join(" then "));
    }
    line
}

fn own_building(building: &BuildingView, tracks: &Tracks) -> String {
    let mut line = format!(
        "{} {} at {} hp{}",
        building.id,
        building.kind.tier_name(building.tier),
        tile(building.anchor),
        building.hp
    );
    if !building.built {
        line.push_str(&construction(building));
    }
    if let Some(queue) = building.queue.as_ref().filter(|queue| !queue.is_empty()) {
        let names: Vec<&str> = queue.iter().map(|kind| kind.name()).collect();
        let _ = write!(line, " | training {}", names.join(", "));
        if let Some(ticks) = building.ticks_remaining {
            let _ = write!(line, " ({ticks} ticks left)");
        }
    }
    if let Some(rally) = &building.rally {
        let _ = write!(line, " | rally {}", compact(rally, tracks));
    }
    if let Some(focus) = &building.focus {
        let _ = write!(line, " | focus {}", compact(focus, tracks));
    }
    line
}

/// A site's or upgrade's progress against the work its rung needs.
fn construction(building: &BuildingView) -> String {
    let total = building
        .kind
        .tiers()
        .get(usize::from(building.tier))
        .map_or(0, |stats| stats.construction.build_ticks);
    if total == 0 {
        return format!(" under construction {}", building.progress);
    }
    format!(
        " under construction {}/{total} ({}%)",
        building.progress,
        u64::from(building.progress) * 100 / u64::from(total)
    )
}

/// Whether a unit has nothing to do.
fn is_idle(unit: &UnitView) -> bool {
    unit.queue.is_empty()
        && unit
            .order
            .as_ref()
            .is_none_or(|order| matches!(order_tag(order).as_deref(), Some("idle" | "stop")))
}

fn order_tag(order: &aftermine_sim::Order) -> Option<String> {
    match serde_json::to_value(order).ok()? {
        Value::Object(fields) => fields.get("order")?.as_str().map(str::to_string),
        Value::String(tag) => Some(tag),
        _ => None,
    }
}

/// Any serializable value as short text: an object's tag first, then its
/// other fields as `key=value`, a tile or walk goal as `x,y`, and an attack
/// target as the player would name it.
fn compact<T: serde::Serialize>(value: &T, tracks: &Tracks) -> String {
    serde_json::to_value(value).map_or_else(|_| "?".to_string(), |value| flatten(&value, tracks))
}

fn flatten(value: &Value, tracks: &Tracks) -> String {
    match value {
        Value::Object(fields) => {
            // A tile, or a walk's goal: the clicked tile, plus how the walk
            // spreads around it, which only the simulation needs.
            if let (Some(x), Some(y)) = (fields.get("x"), fields.get("y"))
                && fields
                    .keys()
                    .all(|key| matches!(key.as_str(), "x" | "y" | "aim" | "endpoint" | "march"))
            {
                return format!("{x},{y}");
            }
            if let (Some(kind), Some(id), 2) = (
                fields.get("kind").and_then(Value::as_str),
                fields.get("id"),
                fields.len(),
            ) && let Some(target) = target(kind, id, tracks)
            {
                return target;
            }
            let tag = ["order", "type", "kind"]
                .iter()
                .find_map(|key| fields.get(*key).and_then(Value::as_str));
            let mut parts: Vec<String> = tag.map(str::to_string).into_iter().collect();
            for (key, field) in fields {
                if tag.is_some_and(|tag| field.as_str() == Some(tag)) || field.is_null() {
                    continue;
                }
                parts.push(format!("{key}={}", flatten(field, tracks)));
            }
            parts.join(" ")
        }
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(|item| flatten(item, tracks)).collect();
            format!("[{}]", parts.join(" "))
        }
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// An attack target as a player names it. A contact is a unit the seat
/// tracks: while in sight it is that unit, and in the dark only a place.
fn target(kind: &str, id: &Value, tracks: &Tracks) -> Option<String> {
    Some(match kind {
        "unit" => format!("unit {id}"),
        "building" => format!("structure {id}"),
        "contact" => match tracks.contacts.get(&id.as_u64()?) {
            Some(track) => match track.visible_unit {
                Some(unit) => format!("unit {}", unit.0),
                None => format!("contact at ({},{})", track.tile.x, track.tile.y),
            },
            None => "a contact no longer tracked".to_string(),
        },
        "remembered_building" => {
            let anchor = id.get("anchor")?;
            let (x, y) = (anchor.get("x")?.as_i64()?, anchor.get("y")?.as_i64()?);
            let kind = id.get("building_kind")?.as_str()?;
            let owner = id.get("owner")?.as_u64()?;
            let in_sight = tracks.buildings.iter().find(|building| {
                u64::from(building.player) == owner
                    && i64::from(building.anchor[0]) == x
                    && i64::from(building.anchor[1]) == y
                    && serde_json::to_value(building.kind)
                        .is_ok_and(|name| name.as_str() == Some(kind))
            });
            match in_sight {
                Some(building) => format!(
                    "structure {} {} at ({x},{y})",
                    building.id,
                    building.kind.tier_name(building.tier)
                ),
                None => format!("remembered {} at ({x},{y})", kind.replace('_', " ")),
            }
        }
        _ => return None,
    })
}

fn tile(at: [i32; 2]) -> String {
    format!("({},{})", at[0], at[1])
}

fn position(pos: [f64; 2]) -> String {
    format!("({:.1},{:.1})", pos[0], pos[1])
}

/// The map as the seat knows it, one character per tile, with a ruler.
/// Terrain: `#` rock, `^` peak, `~` pit, `=` cliff, `/` ramp, `E` an empty
/// Extractor frame's anchor, where an Extractor is built, and `e` the rest
/// of its frame. Ground: `.` in sight now, `-` explored but dark, blank
/// never seen. `s` remembered scrap, `w` remembered wreck. `o` own unit,
/// `O` own structure, `x` another seat's unit in sight, `X` its structure
/// in sight, `g` a remembered structure, `?` a radar contact.
fn grid(view: &FogView, map: &Map) -> String {
    let (width, height) = (map.width(), map.height());
    let mut rows: Vec<Vec<char>> = (0..height)
        .map(|y| {
            let mask: Vec<char> = view
                .mask
                .get(usize::try_from(y).unwrap_or(usize::MAX))
                .map_or_else(Vec::new, |row| row.chars().collect());
            (0..width)
                .map(|x| {
                    let pos = TilePos::new(x, y);
                    let sight = mask
                        .get(usize::try_from(x).unwrap_or(usize::MAX))
                        .copied()
                        .unwrap_or(' ');
                    terrain_char(map, pos, sight)
                })
                .collect()
        })
        .collect();
    let mut mark = |at: [i32; 2], glyph: char| {
        if let Some(cell) = usize::try_from(at[1])
            .ok()
            .and_then(|y| rows.get_mut(y))
            .and_then(|row| usize::try_from(at[0]).ok().and_then(|x| row.get_mut(x)))
        {
            *cell = glyph;
        }
    };
    for known in &view.scrap {
        mark(known.tile, 's');
    }
    for known in &view.wrecks {
        mark(known.tile, 'w');
    }
    for track in &view.contact_tracks {
        mark([track.tile.x, track.tile.y], '?');
    }
    let footprint = |kind: aftermine_sim::BuildingKind, anchor: [i32; 2]| {
        let (w, h) = kind.size();
        (0..h).flat_map(move |dy| (0..w).map(move |dx| [anchor[0] + dx, anchor[1] + dy]))
    };
    for ghost in &view.ghosts {
        for at in footprint(ghost.kind, ghost.anchor) {
            mark(at, 'g');
        }
    }
    for building in &view.buildings {
        let glyph = if building.player == view.player {
            'O'
        } else {
            'X'
        };
        for at in footprint(building.kind, building.anchor) {
            mark(at, glyph);
        }
    }
    for unit in &view.units {
        mark(
            unit.tile,
            if unit.player == view.player { 'o' } else { 'x' },
        );
    }

    let mut out = String::from(
        "MAP: # rock, ^ peak, ~ pit, = cliff, / ramp, E Extractor frame anchor (build an \
         Extractor here), e rest of the frame; \
         . in sight, - explored but dark, blank never seen; s scrap, w wreck; \
         o own unit, O own structure, x other seat's unit, X its structure, \
         g remembered structure, ? radar contact\n",
    );
    let ruler = |digit: fn(i32) -> i32| -> String {
        (0..width)
            .map(|x| char::from_digit(digit(x).unsigned_abs() % 10, 10).unwrap_or(' '))
            .collect()
    };
    let _ = writeln!(out, "    {}", ruler(|x| x / 10));
    let _ = writeln!(out, "    {}", ruler(|x| x % 10));
    for (y, row) in rows.iter().enumerate() {
        let line: String = row.iter().collect();
        let _ = writeln!(out, "{y:>3} {}", line.trim_end());
    }
    out
}

fn terrain_char(map: &Map, pos: TilePos, sight: char) -> char {
    use aftermine_sim::map::Terrain;
    if map.is_extractor_frame(pos) {
        return 'E';
    }
    if map.tile_in_extractor_frame(pos) {
        return 'e';
    }
    match map.tile(pos).map(|tile| tile.terrain) {
        Some(Terrain::Rock) => '#',
        Some(Terrain::Peak) => '^',
        Some(Terrain::Pit) => '~',
        Some(Terrain::Cliff) => '=',
        Some(Terrain::Ground) if map.is_ramp(pos) => '/',
        Some(Terrain::Ground) => match sight {
            '*' => '.',
            '.' => '-',
            _ => ' ',
        },
        None => ' ',
    }
}

/// A playtest advance as text: how far the clock moved, the seat's income,
/// then what it saw happen, tick by tick, and the result once there is one.
pub fn played(view: &PlayedView) -> String {
    use aftermine_sim::{Event, GameResult};
    let seat = view.player;
    let clock = |tick: u64| {
        let seconds = tick / u64::from(aftermine_sim::TICKS_PER_SECOND);
        format!("{}:{:02}", seconds / 60, seconds % 60)
    };
    let at = |pos: chassis::fx::Vec2Fx| {
        let tile = TilePos::containing(pos);
        format!("({},{})", tile.x, tile.y)
    };
    let whose = |player: aftermine_sim::PlayerId| {
        if player == seat {
            "your".to_string()
        } else {
            format!("seat {}'s", player.0)
        }
    };
    let mut out = String::new();
    let per_minute = (u64::from(view.income) * 60 * u64::from(aftermine_sim::TICKS_PER_SECOND))
        .checked_div(view.ticks)
        .unwrap_or(0);
    let _ = writeln!(
        out,
        "ADVANCED {} ticks to {} ({}) | income {} scrap, {per_minute} a minute",
        view.ticks,
        view.tick,
        clock(view.tick),
        view.income
    );
    for seen in &view.log {
        let line = match &seen.event {
            Event::UnitDied {
                unit,
                kind,
                player,
                pos,
                ..
            } => format!(
                "{} {} {} died at {}",
                whose(*player),
                kind.name(),
                unit.0,
                at(*pos)
            ),
            Event::BuildingDestroyed {
                building,
                kind,
                player,
                tier,
                pos,
            } => format!(
                "{} {} {} was destroyed at {}",
                whose(*player),
                kind.tier_name(*tier),
                building.0,
                at(*pos)
            ),
            Event::BuildingCompleted {
                building,
                kind,
                tier,
                ..
            } => format!("your {} {} is finished", kind.tier_name(*tier), building.0),
            Event::UnitAssembled {
                unit, kind, pos, ..
            } => {
                format!("your {} {} stood up at {}", kind.name(), unit.0, at(*pos))
            }
            Event::BuildCancelled {
                building, refund, ..
            } => format!("your site {} was cancelled, refunding {refund}", building.0),
            Event::CommandRejected { reason, .. } => {
                format!("the game rejected an order when it ran: {reason:?}")
            }
            Event::OrderStalled {
                unit, reason, pos, ..
            } => format!("your unit {} at {} stalled: {reason:?}", unit.0, at(*pos)),
            Event::DamageTaken { pos, .. } => {
                format!("your units or structures took fire near {}", at(*pos))
            }
            Event::WarheadLanded {
                player, at: impact, ..
            } => {
                format!("{} warhead landed at {}", whose(*player), at(*impact))
            }
            Event::ChargeDetonated {
                player, at: blast, ..
            } => {
                format!(
                    "{} Scuttle Charge went off at {}",
                    whose(*player),
                    at(*blast)
                )
            }
            Event::PlayerResigned { player } => format!("seat {} surrendered", player.0),
            Event::GameOver { result } => match result {
                GameResult::Draw => "the match is over: a draw".to_string(),
                GameResult::Victory { team } => format!("the match is over: team {team} won"),
            },
            other => format!("{other:?}"),
        };
        let _ = writeln!(out, "  {} ({}) {line}", seen.tick, clock(seen.tick));
    }
    if let Some(result) = view.result {
        let verdict = match result {
            GameResult::Draw => "a draw".to_string(),
            GameResult::Victory { team } if team == view.team => "you won".to_string(),
            GameResult::Victory { .. } => "you lost".to_string(),
        };
        let _ = writeln!(
            out,
            "MATCH OVER at tick {} ({}): {verdict}. Advancing is refused from here.",
            view.tick,
            clock(view.tick)
        );
    }
    out
}

#[cfg(test)]
mod tests;
