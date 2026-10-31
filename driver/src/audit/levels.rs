//! Authoring rules for tiered maps, beyond what the simulation needs to
//! load one: ramps that read as ramps, peaks and pits standing on one
//! level, cliffs that part levels, and starts that sit level.
//!
//! A ramp is a filled rectangle at least three tiles across, every tile of
//! it joining the same two levels. Ground at its
//! upper level meets one end, ground at its lower level the other, and
//! cliff runs along both sides, so it crosses exactly one cliff band. It
//! keeps a tile clear of peaks, pits, and the map's edge ring, so the art
//! has room to cut it.

use aftermine_sim::Scenario;
use aftermine_sim::map::{Elevation, Map, Terrain};
use aftermine_sim::stats::BuildingKind;
use anyhow::{Context, Result};
use chassis::grid::TilePos;

/// Every way `scenario`'s levels break the authoring rules, as readable
/// lines; empty when they hold, and always on a flat map.
pub fn level_faults(scenario: &Scenario) -> Result<Vec<String>> {
    let (map, anchors) = scenario
        .parse_map_and_anchors()
        .context("parsing the map")?;
    let mut faults = Vec::new();
    if !map.tiered() {
        if !scenario.levels.is_empty() {
            faults.push("a levels layer at a single level; leave it out".to_string());
        }
        return Ok(faults);
    }
    ramp_faults(&map, &mut faults);
    for terrain in [Terrain::Peak, Terrain::Pit] {
        for group in groups(&map, |pos| terrain_at(&map, pos) == Some(terrain)) {
            let levels = distinct(group.iter().filter_map(|&pos| low(&map, pos)));
            if levels.len() > 1 {
                faults.push(format!(
                    "the {terrain:?} at {} stands on levels {levels:?}; keep it on one",
                    at(group[0])
                ));
            }
        }
    }
    for group in groups(&map, |pos| terrain_at(&map, pos) == Some(Terrain::Cliff)) {
        let touched = distinct(
            group
                .iter()
                .flat_map(|&pos| around(pos))
                .filter(|&pos| terrain_at(&map, pos) == Some(Terrain::Ground))
                .filter_map(|pos| map.elevation(pos))
                .flat_map(|level| {
                    let (lo, hi) = level.span();
                    [lo, hi]
                }),
        );
        if touched.len() < 2 {
            faults.push(format!(
                "the cliff at {} parts no levels; make it rock",
                at(group[0])
            ));
        }
    }
    let foundry = BuildingKind::Foundry.size();
    for &(seat, anchor) in &anchors {
        footing_fault(
            &map,
            &format!("seat {}'s Foundry", seat.0),
            anchor,
            foundry,
            &mut faults,
        );
    }
    let frame = BuildingKind::Extractor.size();
    for &anchor in map.extractor_frames() {
        footing_fault(&map, "the Extractor frame", anchor, frame, &mut faults);
    }
    for (index, unit) in scenario.units.iter().enumerate() {
        if map.is_ramp(TilePos::new(unit.x, unit.y)) {
            faults.push(format!(
                "starting unit #{index} stands on the ramp at {}",
                at(TilePos::new(unit.x, unit.y))
            ));
        }
    }
    for (index, building) in scenario.buildings.iter().enumerate() {
        let name = format!("starting structure #{index}");
        footing_fault(
            &map,
            &name,
            TilePos::new(building.x, building.y),
            building.kind.size(),
            &mut faults,
        );
    }
    Ok(faults)
}

fn ramp_faults(map: &Map, faults: &mut Vec<String>) {
    for group in groups(map, |pos| map.is_ramp(pos)) {
        let (x0, y0) = (
            group.iter().map(|p| p.x).min().unwrap_or(0),
            group.iter().map(|p| p.y).min().unwrap_or(0),
        );
        let (x1, y1) = (
            group.iter().map(|p| p.x).max().unwrap_or(0),
            group.iter().map(|p| p.y).max().unwrap_or(0),
        );
        let corner = at(TilePos::new(x0, y0));
        let area = usize::try_from((x1 - x0 + 1) * (y1 - y0 + 1)).unwrap_or(usize::MAX);
        if group.len() != area {
            faults.push(format!("the ramp at {corner} is not a filled rectangle"));
            continue;
        }
        let Some(Elevation::Ramp { low }) = map.elevation(group[0]) else {
            continue;
        };
        if group
            .iter()
            .any(|&pos| map.elevation(pos) != Some(Elevation::Ramp { low }))
        {
            faults.push(format!(
                "the ramp at {corner} joins more than one pair of levels"
            ));
            continue;
        }
        let line = |from: TilePos, step: (i32, i32), count: i32| -> Vec<TilePos> {
            (0..count)
                .map(|k| from.offset(step.0 * k, step.1 * k))
                .collect()
        };
        let (width, height) = (x1 - x0 + 1, y1 - y0 + 1);
        // Each way the ramp could run: its two ends and its two sides.
        let runs = [
            (
                width,
                line(TilePos::new(x0, y0 - 1), (1, 0), width),
                line(TilePos::new(x0, y1 + 1), (1, 0), width),
                [
                    line(TilePos::new(x0 - 1, y0), (0, 1), height),
                    line(TilePos::new(x1 + 1, y0), (0, 1), height),
                ],
            ),
            (
                height,
                line(TilePos::new(x0 - 1, y0), (0, 1), height),
                line(TilePos::new(x1 + 1, y0), (0, 1), height),
                [
                    line(TilePos::new(x0, y0 - 1), (1, 0), width),
                    line(TilePos::new(x0, y1 + 1), (1, 0), width),
                ],
            ),
        ];
        let ground_at = |tiles: &[TilePos], level: u8| {
            tiles.iter().all(|&pos| {
                terrain_at(map, pos) == Some(Terrain::Ground)
                    && map.elevation(pos) == Some(Elevation::Level(level))
            })
        };
        let joins = |ends: (&[TilePos], &[TilePos])| {
            (ground_at(ends.0, low + 1) && ground_at(ends.1, low))
                || (ground_at(ends.0, low) && ground_at(ends.1, low + 1))
        };
        let Some((across, ..)) = runs.iter().find(|(_, a, b, sides)| {
            joins((a, b))
                && sides.iter().all(|side| {
                    side.iter()
                        .any(|&pos| terrain_at(map, pos) == Some(Terrain::Cliff))
                })
        }) else {
            faults.push(format!(
                "the ramp at {corner} does not run through one cliff band from level {} to level {low}",
                low + 1
            ));
            continue;
        };
        if *across < 3 {
            faults.push(format!(
                "the ramp at {corner} is {across} tiles across; make it at least 3"
            ));
        }
        let crowded = group.iter().flat_map(|&pos| around(pos)).find(|&pos| {
            matches!(terrain_at(map, pos), Some(Terrain::Peak | Terrain::Pit))
                || pos.x <= 0
                || pos.y <= 0
                || pos.x >= map.width() - 1
                || pos.y >= map.height() - 1
        });
        if let Some(pos) = crowded {
            faults.push(format!(
                "the ramp at {corner} touches a peak, pit, or the edge ring at {}",
                at(pos)
            ));
        }
    }
}

/// A footprint must stand off every ramp and on a single level.
fn footing_fault(
    map: &Map,
    name: &str,
    anchor: TilePos,
    (w, h): (i32, i32),
    faults: &mut Vec<String>,
) {
    let tiles: Vec<TilePos> = (0..h)
        .flat_map(|dy| (0..w).map(move |dx| anchor.offset(dx, dy)))
        .collect();
    if tiles.iter().any(|&pos| map.is_ramp(pos)) {
        faults.push(format!("{name} at {} stands on a ramp", at(anchor)));
    }
    let levels = distinct(tiles.iter().filter_map(|&pos| low(map, pos)));
    if levels.len() > 1 {
        faults.push(format!(
            "{name} at {} straddles levels {levels:?}",
            at(anchor)
        ));
    }
}

fn terrain_at(map: &Map, pos: TilePos) -> Option<Terrain> {
    map.tile(pos).map(|tile| tile.terrain)
}

fn low(map: &Map, pos: TilePos) -> Option<u8> {
    map.elevation(pos).map(|level| level.span().0)
}

fn at(pos: TilePos) -> String {
    format!("({}, {})", pos.x, pos.y)
}

fn distinct(levels: impl Iterator<Item = u8>) -> Vec<u8> {
    let mut levels: Vec<u8> = levels.collect();
    levels.sort_unstable();
    levels.dedup();
    levels
}

fn around(pos: TilePos) -> impl Iterator<Item = TilePos> {
    (-1..=1).flat_map(move |dy| (-1..=1).map(move |dx| pos.offset(dx, dy)))
}

/// The 8-connected groups of tiles `member` accepts, each in row-major
/// discovery order.
fn groups(map: &Map, member: impl Fn(TilePos) -> bool) -> Vec<Vec<TilePos>> {
    let (w, h) = (map.width(), map.height());
    let index = |pos: TilePos| usize::try_from(pos.y * w + pos.x).unwrap_or(0);
    let mut seen = vec![false; usize::try_from(w * h).unwrap_or(0)];
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let start = TilePos::new(x, y);
            if seen[index(start)] || !member(start) {
                continue;
            }
            seen[index(start)] = true;
            let mut group = vec![start];
            let mut next = 0;
            while next < group.len() {
                let pos = group[next];
                next += 1;
                for near in around(pos) {
                    if near.x >= 0
                        && near.y >= 0
                        && near.x < w
                        && near.y < h
                        && !seen[index(near)]
                        && member(near)
                    {
                        seen[index(near)] = true;
                        group.push(near);
                    }
                }
            }
            out.push(group);
        }
    }
    out
}

#[cfg(test)]
mod tests;
