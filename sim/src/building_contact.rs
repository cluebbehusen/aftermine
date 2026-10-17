//! Static work surfaces that follow each building's drawn body. Placement
//! and travel still use rectangular tiles.
use crate::stats::CONSTRUCTION_STAGES;
use crate::{Building, BuildingKind, State};
use chassis::fx::{Fx, Vec2Fx};
use chassis::grid::TilePos;

/// A contact outline: vertices on the footprint's 128-unit canvas,
/// clockwise on screen.
pub type Outline = &'static [(i32, i32)];

/// One outline per construction stage of a site or upgrade.
type Stages = [Outline; CONSTRUCTION_STAGES as usize];

/// The outlines a kind's art shows.
struct Outlines {
    /// One per tier standing built; empty for a kind that never stands.
    built: &'static [Outline],
    /// One set of stages per tier.
    site: &'static [Stages],
}

const FABRICATOR: Outline = &[
    (11, 10),
    (52, 10),
    (52, 33),
    (124, 33),
    (124, 115),
    (11, 115),
];
const FOUNDRY: Outline = &[
    (8, 24),
    (37, 24),
    (37, 7),
    (89, 7),
    (89, 24),
    (120, 24),
    (120, 119),
    (8, 119),
];
const BASTION: Outline = &[(18, 28), (109, 28), (109, 111), (18, 111)];
const REPAIR_BAY: Outline = &[
    (10, 25),
    (29, 14),
    (106, 14),
    (119, 26),
    (119, 105),
    (100, 117),
    (27, 117),
    (10, 107),
];
const TURRET: Outline = &[(14, 25), (114, 25), (114, 110), (14, 110)];
const FLAK_TURRET: Outline = &[(14, 21), (114, 21), (114, 114), (14, 114)];
const ARRAY: Outline = &[(17, 19), (111, 19), (111, 110), (17, 110)];
const BARRICADE: Outline = &[(0, 26), (128, 26), (128, 111), (0, 111)];
const SCUTTLE_CHARGE: Outline = &[(23, 36), (105, 36), (105, 94), (23, 94)];
const PLAIN: Outline = &[(8, 8), (120, 8), (120, 120), (8, 120)];

/// Every stage showing one outline.
const fn steady(outline: Outline) -> Stages {
    [outline; CONSTRUCTION_STAGES as usize]
}

const FOUNDRY_ART: Outlines = Outlines {
    built: &[FOUNDRY],
    site: &[steady(FOUNDRY)],
};
const TURRET_ART: Outlines = Outlines {
    built: &[TURRET, TURRET, TURRET],
    site: &[steady(TURRET), steady(TURRET), steady(TURRET)],
};
const FABRICATOR_ART: Outlines = Outlines {
    built: &[FABRICATOR],
    site: &[steady(FABRICATOR)],
};
const FLAK_TURRET_ART: Outlines = Outlines {
    built: &[FLAK_TURRET, FLAK_TURRET],
    site: &[steady(FLAK_TURRET), steady(FLAK_TURRET)],
};
const BASTION_ART: Outlines = Outlines {
    built: &[BASTION, BASTION],
    site: &[steady(BASTION), steady(BASTION)],
};
const ARRAY_ART: Outlines = Outlines {
    built: &[ARRAY, ARRAY],
    site: &[steady(ARRAY), steady(ARRAY)],
};
const PLAIN_TWO_TIERS_ART: Outlines = Outlines {
    built: &[PLAIN, PLAIN],
    site: &[steady(PLAIN), steady(PLAIN)],
};
const REPAIR_BAY_ART: Outlines = Outlines {
    built: &[REPAIR_BAY],
    site: &[steady(REPAIR_BAY)],
};
const PLAIN_ART: Outlines = Outlines {
    built: &[PLAIN],
    site: &[steady(PLAIN)],
};
const BARRICADE_ART: Outlines = Outlines {
    built: &[BARRICADE],
    site: &[steady(BARRICADE)],
};
const SCUTTLE_CHARGE_ART: Outlines = Outlines {
    built: &[SCUTTLE_CHARGE],
    site: &[steady(SCUTTLE_CHARGE)],
};
const CRADLE_ART: Outlines = Outlines {
    built: &[],
    site: &[steady(PLAIN)],
};

/// The outlines of every tier and stage of `kind`. The match is exhaustive
/// so a new kind cannot ship without them.
fn outlines(kind: BuildingKind) -> Outlines {
    match kind {
        BuildingKind::Foundry => FOUNDRY_ART,
        BuildingKind::Turret => TURRET_ART,
        BuildingKind::Fabricator => FABRICATOR_ART,
        BuildingKind::FlakTurret => FLAK_TURRET_ART,
        BuildingKind::Bastion => BASTION_ART,
        BuildingKind::Array => ARRAY_ART,
        BuildingKind::Reclaimer | BuildingKind::Extractor => PLAIN_TWO_TIERS_ART,
        BuildingKind::RepairBay => REPAIR_BAY_ART,
        BuildingKind::Airworks
        | BuildingKind::Crucible
        | BuildingKind::Spire
        | BuildingKind::Lattice => PLAIN_ART,
        BuildingKind::Barricade => BARRICADE_ART,
        BuildingKind::ScuttleCharge => SCUTTLE_CHARGE_ART,
        BuildingKind::MawCradle | BuildingKind::RocCradle => CRADLE_ART,
    }
}

/// The outline `kind`'s art shows at `tier`: standing built when `stage`
/// is none, otherwise at that construction stage. Tier and stage clamp to
/// the last authored, as a forged tier's stats do, and a kind that never
/// stands built shows its last stage.
pub fn outline(kind: BuildingKind, tier: u8, stage: Option<u8>) -> Outline {
    let outlines = outlines(kind);
    let tier = usize::from(tier);
    let site = &outlines.site[tier.min(outlines.site.len() - 1)];
    let last = site[site.len() - 1];
    match stage {
        Some(stage) => site[usize::from(stage).min(site.len() - 1)],
        None => outlines
            .built
            .get(tier)
            .or(outlines.built.last())
            .copied()
            .unwrap_or(last),
    }
}

/// A building's contact surface, without a navigation mesh or cache.
#[derive(Clone, Copy)]
pub struct Surface {
    outline: Outline,
    anchor: TilePos,
    size: (i32, i32),
}

impl Surface {
    /// The outline a building's art shows now, at its world position.
    pub fn new(building: &Building) -> Self {
        Self {
            outline: outline(building.kind, building.tier, building.construction_stage()),
            anchor: building.anchor,
            size: building.kind.size(),
        }
    }

    fn vertices(self) -> impl Iterator<Item = Vec2Fx> + Clone {
        self.outline.iter().map(move |&(x, y)| {
            Vec2Fx::new(
                Fx::from_num(self.anchor.x) + Fx::from_num(x * self.size.0) / 128,
                Fx::from_num(self.anchor.y) + Fx::from_num(y * self.size.1) / 128,
            )
        })
    }

    fn edges(self) -> impl Iterator<Item = (Vec2Fx, Vec2Fx)> {
        let vertices = self.vertices();
        vertices.clone().zip(vertices.cycle().skip(1))
    }

    /// Nearest coarse surface, with ties relative to the incoming direction.
    pub fn closest(self, from: Vec2Fx) -> Vec2Fx {
        let center = Vec2Fx::new(Fx::from_num(self.anchor.x), Fx::from_num(self.anchor.y))
            + Vec2Fx::new(Fx::from_num(self.size.0), Fx::from_num(self.size.1)) / Fx::from_num(2);
        let mut best: Option<((Fx, Fx), Vec2Fx)> = None;
        for (a, b) in self.edges() {
            // An edge strictly farther than the nearest so far cannot win,
            // and the first of equally near edges is kept.
            if let Some(((distance, _), _)) = best {
                let gap = box_gap(from, from, a, b) - ROUNDING_MARGIN;
                if gap > Fx::ZERO && gap * gap > distance {
                    continue;
                }
            }
            let p = closest_on_segment(from, a, b);
            let key = (from.dist_sq(p), cross(from - center, p - center));
            if best.is_none_or(|(best, _)| key < best) {
                best = Some((key, p));
            }
        }
        best.expect("nonempty outline").1
    }

    /// Where a tool as wide as its hull meets each face square, `clearance`
    /// out from the face's midpoint, with the face's outward normal. Outlines
    /// wind clockwise on screen, so that normal is the face's direction
    /// turned a quarter turn counterclockwise.
    pub fn square_stances(self, clearance: Fx) -> impl Iterator<Item = (Vec2Fx, Vec2Fx)> {
        self.edges().map(move |(a, b)| {
            let along = b - a;
            let outward = Vec2Fx::new(along.y, -along.x) / along.length();
            ((a + b) / Fx::from_num(2) + outward * clearance, outward)
        })
    }

    /// Final contact point approached from an ordinary perimeter tile.
    pub fn stance(self, from: Vec2Fx, clearance: Fx) -> Vec2Fx {
        let hit = self.closest(from);
        let outward = from - hit;
        if outward == Vec2Fx::ZERO {
            from
        } else {
            hit + outward * (clearance / outward.length())
        }
    }

    fn contains(self, point: Vec2Fx) -> bool {
        let mut inside = false;
        for (a, b) in self.edges() {
            if (a.y > point.y) != (b.y > point.y)
                && point.x < a.x + (point.y - a.y) * (b.x - a.x) / (b.y - a.y)
            {
                inside = !inside;
            }
        }
        inside
    }

    /// A short, swept chassis movement cannot cross the occupied body.
    pub(crate) fn clear(self, from: Vec2Fx, to: Vec2Fx, radius: Fx) -> bool {
        if from == to {
            return !self.contains(from) && from.dist_sq(self.closest(from)) >= radius * radius;
        }
        !self.contains(from)
            && !self.contains(to)
            && self.edges().all(|(a, b)| {
                if segments_cross(from, to, a, b) {
                    return false;
                }
                // Boxes this far apart keep every point of one segment
                // farther than the radius from the other.
                if box_gap(from, to, a, b) > radius.abs() + ROUNDING_MARGIN {
                    return true;
                }
                [
                    from.dist_sq(closest_on_segment(from, a, b)),
                    to.dist_sq(closest_on_segment(to, a, b)),
                    a.dist_sq(closest_on_segment(a, from, to)),
                    b.dist_sq(closest_on_segment(b, from, to)),
                ]
                .into_iter()
                .all(|d| d >= radius * radius)
            })
    }

    pub(crate) fn covers(self, tile: TilePos) -> bool {
        tile.x >= self.anchor.x
            && tile.x < self.anchor.x + self.size.0
            && tile.y >= self.anchor.y
            && tile.y < self.anchor.y + self.size.1
    }
}

/// Slack for the rounding in projected points and squared distances, far
/// above any fixed-point error and far below any surface feature.
const ROUNDING_MARGIN: Fx = Fx::lit("0.00390625");

/// The larger axis gap between the bounding boxes of segments `p q` and
/// `a b`, or zero when the boxes overlap: a lower bound on the distance
/// between any point of one segment and any point of the other.
fn box_gap(p: Vec2Fx, q: Vec2Fx, a: Vec2Fx, b: Vec2Fx) -> Fx {
    let gap = |low: Fx, high: Fx, other_low: Fx, other_high: Fx| {
        (other_low - high).max(low - other_high).max(Fx::ZERO)
    };
    gap(p.x.min(q.x), p.x.max(q.x), a.x.min(b.x), a.x.max(b.x)).max(gap(
        p.y.min(q.y),
        p.y.max(q.y),
        a.y.min(b.y),
        a.y.max(b.y),
    ))
}

fn cross(a: Vec2Fx, b: Vec2Fx) -> Fx {
    a.x * b.y - a.y * b.x
}
fn dot(a: Vec2Fx, b: Vec2Fx) -> Fx {
    a.x * b.x + a.y * b.y
}
fn closest_on_segment(p: Vec2Fx, a: Vec2Fx, b: Vec2Fx) -> Vec2Fx {
    let d = b - a;
    if dot(d, d) == Fx::ZERO {
        a
    } else {
        a + d * (dot(p - a, d) / dot(d, d)).clamp(Fx::ZERO, Fx::ONE)
    }
}
fn segments_cross(a: Vec2Fx, b: Vec2Fx, c: Vec2Fx, d: Vec2Fx) -> bool {
    let opposite = |x: Fx, y: Fx| (x < Fx::ZERO && y > Fx::ZERO) || (x > Fx::ZERO && y < Fx::ZERO);
    opposite(cross(b - a, c - a), cross(b - a, d - a))
        && opposite(cross(d - c, a - c), cross(d - c, b - c))
}

impl State {
    /// Whether a worker's tool can reach the actual building surface.
    pub fn in_building_work_reach(&self, unit: &crate::Unit, building: crate::BuildingId) -> bool {
        let Some(b) = self.building(building) else {
            return false;
        };
        let reach =
            unit.kind.stats().radius + crate::stats::WORK_FOOTPRINT_GAP + crate::stats::WORK_REACH
                - crate::stats::WORK_APPROACH_GAP
                + const { Fx::lit("0.04") };
        let surface = self.contact_surface(b);
        unit.pos.dist_sq(surface.closest(unit.pos)) <= reach * reach
            && surface.clear(unit.pos, unit.pos, unit.kind.stats().radius)
            && self
                .crew_pads_for(unit, b)
                .is_none_or(|pads| pads.contains(&unit.tile()))
    }

    /// The pads a worker raising `site` must stand on: its crew pads while
    /// every pad is open ground. `None` when the worker may work from
    /// anywhere along the site, as at any site without a crew, or once a
    /// pad is lost to rock or a building.
    pub fn crew_pads_for(
        &self,
        unit: &crate::Unit,
        site: &Building,
    ) -> Option<[chassis::grid::TilePos; crate::stats::ASSEMBLY_CREW_CAP]> {
        if unit.order != (crate::Order::Build { site: site.id }) {
            return None;
        }
        let pads = site
            .kind
            .crew_pads()?
            .map(|(dx, dy)| site.anchor.offset(dx, dy));
        pads.iter().all(|&pad| self.passable(pad)).then_some(pads)
    }

    /// Contact artwork and tools use this surface; ranged distance remains rectangular.
    pub fn contact_surface(&self, building: &Building) -> Surface {
        Surface::new(building)
    }
}

#[cfg(test)]
mod tests;
