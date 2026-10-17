//! The simulation's contact outlines (`oxide_sim::building_contact`) are
//! authored data. For every kind, tier, and construction stage, these tests
//! measure each body frame the field can draw in that state against its
//! outline. A gap is how far the outline stands off the art; a Maw's drum or
//! a worker's tool shows it. A spill is how far the art reaches past the
//! outline. Moving layers drawn over the body are not part of it: turret
//! mounts, the Array's rotor, the Spire's deck, the Lattice's pods, and a
//! cradle's crane.

use super::*;
use crate::sprite_contact::SOLID_ALPHA;
use oxide_sim::building_contact::{Outline, outline};
use oxide_sim::stats::CONSTRUCTION_STAGES;
use std::collections::HashMap;

/// How far an outline may stand off its art, in tiles: the gap a Maw's drum
/// or a worker's tool would show.
const GAP_LIMIT: f64 = 0.025;
/// How far art may reach past its outline, in tiles: a little overlap still
/// reads as contact.
const SPILL_LIMIT: f64 = 0.05;
/// The shares of each limit the suggester traces at, coarsest first: it
/// proposes the simplest outline that keeps within both limits once
/// rounded to whole canvas units.
const SUGGEST_SHARES: [f64; 7] = [0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3];
/// The most vertices an outline may have (the simulation's shape test).
const MAX_VERTICES: usize = 24;
/// The construction scaffold every 128 px site shows at its first stage,
/// snapped symmetric.
const SCAFFOLD: [(i32, i32); 4] = [(9, 16), (119, 16), (119, 117), (9, 117)];

type Point = (f64, f64);
/// An outline's vertices on the 128-unit footprint canvas.
type Units = Vec<(i32, i32)>;

/// One outline and the frames the field draws while it holds.
struct Slot {
    kind: BuildingKind,
    tier: u8,
    /// None standing built, otherwise the construction stage.
    stage: Option<u8>,
    frames: Vec<Rect>,
}

impl Slot {
    fn outline(&self) -> Outline {
        outline(self.kind, self.tier, self.stage)
    }

    fn label(&self) -> String {
        match self.stage {
            None => format!("{:?} tier {} built", self.kind, self.tier),
            Some(stage) => format!("{:?} tier {} stage {stage}", self.kind, self.tier),
        }
    }
}

/// Every outline slot with the body frames the field draws in it: a built
/// rung's hull, work, and charge frames, or the rig base it draws instead;
/// and both machinery phases of each construction stage.
fn slots(atlas: &Manifest) -> Vec<Slot> {
    let art = building_art(atlas).expect("building art loads");
    let array = array_rig(atlas).expect("array rig loads");
    let strategic = strategic_rigs(atlas).expect("strategic rigs load");
    let mut slots = Vec::new();
    for kind in BuildingKind::ALL {
        for (index, rung) in art[kind as usize].iter().enumerate() {
            let tier = u8::try_from(index).expect("ladders are short");
            if kind.assembles().is_none() {
                let frames = match kind {
                    BuildingKind::Array => {
                        vec![array.as_ref().expect("the Array's rig ships").layers(tier)[0].0]
                    }
                    BuildingKind::Spire => {
                        vec![strategic.as_ref().expect("rigs ship").spire.foundation[0]]
                    }
                    BuildingKind::Lattice => {
                        vec![strategic.as_ref().expect("rigs ship").lattice.base]
                    }
                    _ => std::iter::once(rung.hull[0])
                        .chain(rung.work.iter().map(|row| row[0]))
                        .chain(rung.charge.iter().map(|row| row[0]))
                        .collect(),
                };
                slots.push(Slot {
                    kind,
                    tier,
                    stage: None,
                    frames,
                });
            }
            for stage in 0..CONSTRUCTION_STAGES {
                let first = usize::from(stage) * SITE_PHASES;
                slots.push(Slot {
                    kind,
                    tier,
                    stage: Some(stage),
                    frames: rung.construction[first..first + SITE_PHASES]
                        .iter()
                        .map(|row| row[0])
                        .collect(),
                });
            }
        }
    }
    slots
}

fn rect_key(rect: Rect) -> [u32; 4] {
    [rect.x, rect.y, rect.w, rect.h].map(numeric::to_u32)
}

/// Every non-accent atlas name of each region, sorted; the atlas packs
/// identical images once.
fn names(atlas: &Manifest) -> HashMap<[u32; 4], Vec<String>> {
    let mut names: HashMap<[u32; 4], Vec<String>> = HashMap::new();
    for (name, row) in atlas {
        if !name.contains("_accent") {
            names
                .entry(row.map(numeric::to_u32))
                .or_default()
                .push(name.clone());
        }
    }
    for list in names.values_mut() {
        list.sort();
    }
    names
}

/// A frame's solid pixels with every enclosed hole filled: the body a tool
/// meets from outside.
#[derive(Clone, PartialEq, Eq, Hash)]
struct Silhouette {
    width: usize,
    height: usize,
    filled: Vec<bool>,
}

impl Silhouette {
    fn of(image: &macroquad::prelude::Image) -> Self {
        let (width, height) = (usize::from(image.width), usize::from(image.height));
        let solid: Vec<bool> = image
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel[3] >= SOLID_ALPHA)
            .collect();
        let mut exterior = vec![false; width * height];
        let mut stack: Vec<(usize, usize)> = (0..width)
            .flat_map(|x| [(x, 0), (x, height - 1)])
            .chain((0..height).flat_map(|y| [(0, y), (width - 1, y)]))
            .collect();
        while let Some((x, y)) = stack.pop() {
            let index = y * width + x;
            if exterior[index] || solid[index] {
                continue;
            }
            exterior[index] = true;
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x + 1 < width {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y + 1 < height {
                stack.push((x, y + 1));
            }
        }
        Self {
            width,
            height,
            filled: exterior.into_iter().map(|outside| !outside).collect(),
        }
    }

    fn at(&self, x: i64, y: i64) -> bool {
        usize::try_from(x)
            .ok()
            .zip(usize::try_from(y).ok())
            .is_some_and(|(x, y)| {
                x < self.width && y < self.height && self.filled[y * self.width + x]
            })
    }

    fn covers(&self, (x, y): Point) -> bool {
        self.at(floor(x), floor(y))
    }

    /// The outer boundary as unit pixel edges, each directed so the body
    /// lies on its right on screen: clockwise around it.
    fn boundary(&self) -> Vec<((i64, i64), (i64, i64))> {
        let mut edges = Vec::new();
        for y in 0..i64::try_from(self.height).expect("small canvas") {
            for x in 0..i64::try_from(self.width).expect("small canvas") {
                if !self.at(x, y) {
                    continue;
                }
                if !self.at(x, y - 1) {
                    edges.push(((x, y), (x + 1, y)));
                }
                if !self.at(x + 1, y) {
                    edges.push(((x + 1, y), (x + 1, y + 1)));
                }
                if !self.at(x, y + 1) {
                    edges.push(((x + 1, y + 1), (x, y + 1)));
                }
                if !self.at(x - 1, y) {
                    edges.push(((x, y + 1), (x, y)));
                }
            }
        }
        edges
    }

    fn union(&self, other: &Self) -> Self {
        Self {
            filled: self
                .filled
                .iter()
                .zip(&other.filled)
                .map(|(a, b)| *a || *b)
                .collect(),
            ..self.clone()
        }
    }

    fn mirrored(&self) -> Self {
        let mut filled = vec![false; self.filled.len()];
        for y in 0..self.height {
            for x in 0..self.width {
                filled[y * self.width + x] = self.filled[y * self.width + self.width - 1 - x];
            }
        }
        Self {
            filled,
            ..self.clone()
        }
    }

    fn px_per_tile(&self, kind: BuildingKind) -> f64 {
        to_f64(self.width) / f64::from(kind.size().0)
    }

    fn polygon(&self, outline: &[(i32, i32)]) -> Vec<Point> {
        let (sx, sy) = (to_f64(self.width) / 128.0, to_f64(self.height) / 128.0);
        outline
            .iter()
            .map(|&(x, y)| (f64::from(x) * sx, f64::from(y) * sy))
            .collect()
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "canvas coordinates stay far inside i64"
)]
fn floor(value: f64) -> i64 {
    value.floor() as i64
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "canvas units stay far inside i32"
)]
fn round_unit(value: f64) -> i32 {
    value.round() as i32
}

fn to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).expect("small canvas"))
}

fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length_sq).clamp(0.0, 1.0)
    };
    (p.0 - a.0 - dx * t).hypot(p.1 - a.1 - dy * t)
}

fn inside(polygon: &[Point], (x, y): Point) -> bool {
    let mut inside = false;
    for (i, &a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        if (a.1 > y) != (b.1 > y) && x < a.0 + (y - a.1) * (b.0 - a.0) / (b.1 - a.1) {
            inside = !inside;
        }
    }
    inside
}

/// How an outline sits on one silhouette, in canvas pixels.
struct Fit {
    gap: f64,
    gap_at: Point,
    spill: f64,
    spill_at: Point,
}

fn fit(polygon: &[Point], silhouette: &Silhouette) -> Fit {
    let boundary: Vec<(Point, Point)> = silhouette
        .boundary()
        .into_iter()
        .map(|((ax, ay), (bx, by))| ((to_f64_i(ax), to_f64_i(ay)), (to_f64_i(bx), to_f64_i(by))))
        .collect();
    let mut result = Fit {
        gap: 0.0,
        gap_at: (0.0, 0.0),
        spill: 0.0,
        spill_at: (0.0, 0.0),
    };
    for (i, &a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        let steps = ((b.0 - a.0).hypot(b.1 - a.1) * 4.0).ceil().max(1.0);
        let mut step = 0.0;
        while step <= steps {
            let p = (
                a.0 + (b.0 - a.0) * step / steps,
                a.1 + (b.1 - a.1) * step / steps,
            );
            if !silhouette.covers(p) {
                let gap = boundary
                    .iter()
                    .map(|&(c, d)| segment_distance(p, c, d))
                    .fold(f64::INFINITY, f64::min);
                if gap > result.gap {
                    result.gap = gap;
                    result.gap_at = p;
                }
            }
            step += 1.0;
        }
    }
    for &(c, d) in &boundary {
        for p in [c, (f64::midpoint(c.0, d.0), f64::midpoint(c.1, d.1)), d] {
            if inside(polygon, p) {
                continue;
            }
            let spill = (0..polygon.len())
                .map(|i| segment_distance(p, polygon[i], polygon[(i + 1) % polygon.len()]))
                .fold(f64::INFINITY, f64::min);
            if spill > result.spill {
                result.spill = spill;
                result.spill_at = p;
            }
        }
    }
    result
}

fn to_f64_i(value: i64) -> f64 {
    f64::from(i32::try_from(value).expect("small canvas"))
}

/// Each slot's distinct silhouettes, each with every frame name that
/// shows it.
fn silhouettes(atlas: &Manifest, slot: &Slot) -> Vec<(Silhouette, Vec<String>)> {
    let names = names(atlas);
    let mut distinct: Vec<(Silhouette, Vec<String>)> = Vec::new();
    for &rect in &slot.frames {
        let frame_names = &names[&rect_key(rect)];
        let silhouette = Silhouette::of(&super::tests::sprite_image(&frame_names[0]));
        match distinct.iter_mut().find(|(seen, _)| *seen == silhouette) {
            Some((_, seen_names)) => seen_names.extend(frame_names.iter().cloned()),
            None => distinct.push((silhouette, frame_names.clone())),
        }
    }
    distinct
}

#[test]
fn every_building_body_sits_on_its_contact_outline() {
    let atlas = super::tests::manifest();
    let mut failures = Vec::new();
    for slot in slots(&atlas) {
        for (silhouette, frame_names) in silhouettes(&atlas, &slot) {
            let scale = silhouette.px_per_tile(slot.kind);
            let fit = fit(&silhouette.polygon(slot.outline()), &silhouette);
            let (gap, spill) = (fit.gap / scale, fit.spill / scale);
            if gap > GAP_LIMIT || spill > SPILL_LIMIT {
                failures.push(format!(
                    "{}: gap {gap:.3} tiles ({:.1} px) at {:?}, spill {spill:.3} tiles ({:.1} px) \
                     at {:?} in {}",
                    slot.label(),
                    fit.gap,
                    fit.gap_at,
                    fit.spill,
                    fit.spill_at,
                    frame_names.join(", ")
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "contact outlines off their art (gap limit {GAP_LIMIT}, spill limit {SPILL_LIMIT} \
         tiles); rerun `suggest_contact_outlines` and update oxide_sim::building_contact:\n{}",
        failures.join("\n")
    );
}

/// The silhouette's outer boundary as one closed, clockwise loop of pixel
/// corners: the longest when the body falls apart into islands.
fn outer_loop(silhouette: &Silhouette) -> Vec<(i64, i64)> {
    let mut next: HashMap<(i64, i64), Vec<(i64, i64)>> = HashMap::new();
    for (a, b) in silhouette.boundary() {
        next.entry(a).or_default().push(b);
    }
    let mut loops = Vec::new();
    while let Some(&start) = next.keys().min() {
        let mut path = vec![start];
        let mut at = start;
        loop {
            let outgoing = next.get_mut(&at).expect("closed boundary");
            let to = outgoing.pop().expect("an outgoing edge");
            if outgoing.is_empty() {
                next.remove(&at);
            }
            if to == start {
                break;
            }
            path.push(to);
            at = to;
        }
        loops.push(path);
    }
    loops.into_iter().max_by_key(Vec::len).expect("a body")
}

/// Ramer-Douglas-Peucker over an open chain of a clockwise loop, keeping both
/// ends. A chord may stand `gap` outside the body (boundary on its right,
/// the body's side) and cut `spill` into it (boundary on its left).
fn simplify(chain: &[Point], gap: f64, spill: f64) -> Vec<Point> {
    if chain.len() < 3 {
        return chain.to_vec();
    }
    let (a, b) = (chain[0], chain[chain.len() - 1]);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx.hypot(dy);
    // Each interior point's breach of its side's limit, as a share of it.
    let breach = |p: Point| {
        if length == 0.0 {
            return segment_distance(p, a, b) / gap;
        }
        let side = (dx * (p.1 - a.1) - dy * (p.0 - a.0)) / length;
        if side > 0.0 {
            side / gap
        } else {
            -side / spill
        }
    };
    let (index, worst) = chain[1..chain.len() - 1]
        .iter()
        .enumerate()
        .map(|(i, &p)| (i + 1, breach(p)))
        .fold(
            (0, 0.0),
            |best, item| if item.1 > best.1 { item } else { best },
        );
    if worst <= 1.0 {
        return vec![a, b];
    }
    let mut left = simplify(&chain[..=index], gap, spill);
    left.pop();
    left.extend(simplify(&chain[index..], gap, spill));
    left
}

/// A traced proposal in whole canvas units, without vertices too close or
/// too straight to keep their faces apart (the simulation's shape test).
fn to_units(polygon: &[Point], silhouette: &Silhouette) -> Units {
    let (sx, sy) = (
        128.0 / to_f64(silhouette.width),
        128.0 / to_f64(silhouette.height),
    );
    let mut units: Units = polygon
        .iter()
        .map(|&(x, y)| (round_unit(x * sx), round_unit(y * sy)))
        .collect();
    // Dropping a convex corner cuts into the body, which only spills;
    // dropping a concave one bridges a recess, which leaves a gap.
    let turn = |units: &[(i32, i32)], i: usize| {
        let n = units.len();
        let (a, b, c) = (units[(i + n - 1) % n], units[i], units[(i + 1) % n]);
        let (ab, bc) = (
            (i64::from(b.0 - a.0), i64::from(b.1 - a.1)),
            (i64::from(c.0 - b.0), i64::from(c.1 - b.1)),
        );
        (
            ab.0 * bc.1 - ab.1 * bc.0,
            ab.0 * ab.0 + ab.1 * ab.1,
            bc.0 * bc.0 + bc.1 * bc.1,
        )
    };
    while units.len() > 3 {
        let n = units.len();
        let short = (0..n).find(|&i| turn(&units, i).2 < 9).map(|i| {
            let (b, c) = (i, (i + 1) % n);
            let (tb, tc) = (turn(&units, b).0, turn(&units, c).0);
            if (tb > 0) == (tc > 0) {
                if tb.abs() <= tc.abs() { b } else { c }
            } else if tb > 0 {
                b
            } else {
                c
            }
        });
        let straight = || {
            (0..n).find(|&i| {
                let (cross, l1, l2) = turn(&units, i);
                cross * cross * 10_000 <= 76 * l1 * l2
            })
        };
        match short.or_else(straight) {
            Some(i) => {
                units.remove(i);
            }
            None => break,
        }
    }
    units
}

/// A proposal traced from `silhouette`: mirror-symmetric when `symmetric`,
/// by tracing its right half from the top of the centre line and mirroring.
fn trace(silhouette: &Silhouette, gap: f64, spill: f64, symmetric: bool) -> Option<Units> {
    let corners: Vec<Point> = outer_loop(silhouette)
        .into_iter()
        .map(|(x, y)| (to_f64_i(x), to_f64_i(y)))
        .collect();
    if !symmetric {
        let far = (1..corners.len())
            .max_by(|&i, &j| {
                let d = |k: usize| (corners[k].0 - corners[0].0).hypot(corners[k].1 - corners[0].1);
                d(i).total_cmp(&d(j))
            })
            .expect("a loop");
        let mut first = simplify(&corners[..=far], gap, spill);
        first.pop();
        let mut rest: Vec<Point> = corners[far..].to_vec();
        rest.push(corners[0]);
        let mut second = simplify(&rest, gap, spill);
        second.pop();
        first.extend(second);
        return Some(to_units(&first, silhouette));
    }
    let middle = to_f64(silhouette.width) / 2.0;
    let on_middle: Vec<usize> = (0..corners.len())
        .filter(|&i| corners[i].0 == middle)
        .collect();
    let top = *on_middle
        .iter()
        .min_by(|&&i, &&j| corners[i].1.total_cmp(&corners[j].1))?;
    let bottom = *on_middle
        .iter()
        .max_by(|&&i, &&j| corners[i].1.total_cmp(&corners[j].1))?;
    let mut half = Vec::new();
    let mut i = top;
    loop {
        half.push(corners[i]);
        if i == bottom {
            break;
        }
        i = (i + 1) % corners.len();
    }
    let right = simplify(&half, gap, spill);
    let mut polygon = right.clone();
    polygon.extend(
        right[1..right.len() - 1]
            .iter()
            .rev()
            .map(|&(x, y)| (2.0 * middle - x, y)),
    );
    Some(to_units(&polygon, silhouette))
}

/// The atlas stem of `kind` as a Rust constant name.
fn const_name(kind: BuildingKind) -> String {
    building_stem(kind).to_uppercase()
}

/// Traces each slot's art into a proposed outline and prints the table for
/// `oxide_sim::building_contact`: the simplest outline within both limits,
/// mirror-symmetric where the art allows, and reusing the shared scaffold or
/// an outline already chosen for the same kind wherever it fits.
#[test]
#[ignore = "prints traced outlines to paste into oxide_sim::building_contact after art changes"]
fn suggest_contact_outlines() {
    let atlas = super::tests::manifest();
    // Each new outline's const name, kind, and vertices.
    let mut chosen: Vec<(String, BuildingKind, Units)> = Vec::new();
    let mut table: Vec<(BuildingKind, u8, Option<u8>, String)> = Vec::new();
    let mut report = Vec::new();
    for slot in slots(&atlas) {
        let distinct = silhouettes(&atlas, &slot);
        let worst = |units: &[(i32, i32)]| {
            distinct
                .iter()
                .fold((0.0_f64, 0.0_f64), |(gap, spill), (silhouette, _)| {
                    let fit = fit(&silhouette.polygon(units), silhouette);
                    let scale = silhouette.px_per_tile(slot.kind);
                    (gap.max(fit.gap / scale), spill.max(fit.spill / scale))
                })
        };
        let fits = |units: &[(i32, i32)]| {
            let (gap, spill) = worst(units);
            gap <= GAP_LIMIT && spill <= SPILL_LIMIT
        };
        let scaffold = distinct[0].0.width == 128 && fits(&SCAFFOLD);
        if let Some(name) = scaffold.then(|| "SCAFFOLD".to_owned()).or_else(|| {
            chosen
                .iter()
                .find(|(_, kind, units)| *kind == slot.kind && fits(units))
                .map(|(name, ..)| name.clone())
        }) {
            table.push((slot.kind, slot.tier, slot.stage, name));
            continue;
        }
        let union = distinct
            .iter()
            .skip(1)
            .fold(distinct[0].0.clone(), |union, (next, _)| union.union(next));
        let mirrored = union.union(&union.mirrored());
        let scale = union.px_per_tile(slot.kind);
        let proposal = SUGGEST_SHARES
            .iter()
            .flat_map(|&share| {
                let (gap, spill) = (GAP_LIMIT * share * scale, SPILL_LIMIT * share * scale);
                [
                    trace(&mirrored, gap, spill, true),
                    trace(&union, gap, spill, false),
                ]
            })
            .flatten()
            .find(|units| units.len() <= MAX_VERTICES && fits(units));
        let Some(units) = proposal else {
            let attempts: Vec<String> = SUGGEST_SHARES
                .iter()
                .filter_map(|&share| {
                    let units = trace(
                        &union,
                        GAP_LIMIT * share * scale,
                        SPILL_LIMIT * share * scale,
                        false,
                    )?;
                    let (gap, spill) = worst(&units);
                    let at = fit(&distinct[0].0.polygon(&units), &distinct[0].0);
                    Some(format!(
                        "{share}: {} vertices, gap {gap:.3} at {:?}, spill {spill:.3} at {:?} \
                         {units:?}",
                        units.len(),
                        at.gap_at,
                        at.spill_at
                    ))
                })
                .collect();
            report.push(format!(
                "{}: no outline within limits; traced {}",
                slot.label(),
                attempts.join("; ")
            ));
            continue;
        };
        let (gap, spill) = worst(&units);
        let suffix = match (slot.tier, slot.stage) {
            (0, None) => String::new(),
            (tier, None) => format!("_T{tier}"),
            (0, Some(stage)) => format!("_STAGE{stage}"),
            (tier, Some(stage)) => format!("_T{tier}_STAGE{stage}"),
        };
        let name = format!("{}{suffix}", const_name(slot.kind));
        report.push(format!(
            "{name}: {} vertices, gap {gap:.3}, spill {spill:.3} tiles",
            units.len()
        ));
        chosen.push((name.clone(), slot.kind, units));
        table.push((slot.kind, slot.tier, slot.stage, name));
    }
    println!("{}\n", report.join("\n"));
    for (name, _, units) in &chosen {
        println!("const {name}: Outline = &{units:?};");
    }
    for kind in BuildingKind::ALL {
        let rows = |stage: Option<u8>| -> Vec<String> {
            table
                .iter()
                .filter(|row| row.0 == kind && row.2 == stage)
                .map(|row| row.3.clone())
                .collect()
        };
        let built = rows(None);
        let tiers = kind.tiers().len();
        let site: Vec<String> = (0..tiers)
            .map(|tier| {
                let stages: Vec<String> = (0..CONSTRUCTION_STAGES)
                    .map(|stage| {
                        table
                            .iter()
                            .find(|row| {
                                row.0 == kind && usize::from(row.1) == tier && row.2 == Some(stage)
                            })
                            .map_or_else(|| "MISSING".to_owned(), |row| row.3.clone())
                    })
                    .collect();
                format!("[{}]", stages.join(", "))
            })
            .collect();
        println!(
            "const {}_ART: Outlines = Outlines {{ built: &[{}], site: &[{}] }};",
            const_name(kind),
            built.join(", "),
            site.join(", ")
        );
    }
}
