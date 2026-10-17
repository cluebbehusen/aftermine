//! Static work surfaces that follow each building's drawn body, per tier and
//! construction stage. Placement and travel still use rectangular tiles.
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

// Traced from the body art the field draws, by the shell's ignored test
// `suggest_contact_outlines`; its `every_building_body_sits_on_its_contact_outline`
// fails when art and outline drift apart.

/// The scaffold every 128 px site shows at its first stage.
const SCAFFOLD: Outline = &[(9, 16), (119, 16), (119, 117), (9, 117)];
const FOUNDRY: Outline = &[
    (8, 32),
    (15, 24),
    (36, 24),
    (41, 7),
    (85, 7),
    (90, 12),
    (90, 24),
    (113, 24),
    (121, 33),
    (121, 116),
    (116, 121),
    (102, 121),
    (98, 115),
    (84, 115),
    (83, 120),
    (48, 120),
    (47, 115),
    (31, 115),
    (26, 121),
    (12, 121),
    (8, 117),
];
const FOUNDRY_STAGE1: Outline = &[
    (10, 16),
    (37, 17),
    (42, 7),
    (84, 7),
    (89, 17),
    (120, 16),
    (120, 117),
    (117, 120),
    (100, 117),
    (29, 117),
    (12, 120),
    (8, 116),
];
const FOUNDRY_STAGE2: Outline = &[
    (10, 16),
    (37, 17),
    (42, 7),
    (85, 7),
    (90, 17),
    (120, 16),
    (121, 116),
    (116, 121),
    (103, 121),
    (99, 117),
    (84, 117),
    (82, 120),
    (29, 117),
    (26, 121),
    (11, 120),
    (8, 117),
];
const TURRET: Outline = &[
    (97, 25),
    (115, 43),
    (113, 103),
    (64, 111),
    (15, 103),
    (13, 43),
    (31, 25),
];
const TURRET_T1: Outline = &[
    (99, 22),
    (117, 40),
    (113, 103),
    (98, 113),
    (30, 113),
    (15, 103),
    (11, 40),
    (29, 22),
];
const TURRET_T2: Outline = &[
    (101, 29),
    (116, 44),
    (116, 101),
    (100, 117),
    (64, 119),
    (28, 117),
    (12, 101),
    (12, 44),
    (27, 29),
];
const FABRICATOR: Outline = &[
    (9, 14),
    (10, 10),
    (46, 10),
    (53, 17),
    (53, 33),
    (113, 34),
    (126, 50),
    (126, 86),
    (110, 103),
    (103, 116),
    (16, 116),
    (9, 109),
];
const FABRICATOR_STAGE1: Outline = &[
    (9, 16),
    (11, 10),
    (45, 10),
    (51, 17),
    (120, 16),
    (120, 44),
    (125, 50),
    (125, 87),
    (120, 93),
    (120, 117),
    (10, 117),
];
const FLAK_TURRET: Outline = &[
    (81, 38),
    (97, 21),
    (115, 25),
    (92, 52),
    (92, 81),
    (115, 110),
    (98, 115),
    (85, 99),
    (64, 105),
    (43, 99),
    (30, 115),
    (13, 110),
    (36, 81),
    (36, 52),
    (13, 25),
    (31, 21),
    (47, 38),
];
const FLAK_TURRET_T1: Outline = &[
    (81, 38),
    (97, 21),
    (115, 25),
    (98, 46),
    (105, 52),
    (98, 87),
    (115, 110),
    (98, 115),
    (85, 99),
    (64, 105),
    (43, 99),
    (30, 115),
    (13, 110),
    (30, 87),
    (23, 52),
    (30, 46),
    (13, 25),
    (31, 21),
    (47, 38),
];
const BASTION: Outline = &[
    (17, 47),
    (24, 40),
    (23, 28),
    (107, 28),
    (110, 31),
    (107, 38),
    (110, 41),
    (108, 101),
    (110, 107),
    (106, 111),
    (97, 110),
    (24, 111),
    (23, 99),
    (17, 96),
];
const BASTION_STAGE1: Outline = &[
    (10, 16),
    (57, 17),
    (58, 3),
    (71, 3),
    (71, 17),
    (120, 16),
    (120, 117),
    (10, 117),
];
const BASTION_T1: Outline = &[
    (116, 21),
    (123, 32),
    (122, 114),
    (110, 122),
    (18, 122),
    (6, 114),
    (5, 32),
    (12, 21),
];
const BASTION_T1_STAGE1: Outline = &[
    (6, 32),
    (10, 25),
    (10, 16),
    (54, 17),
    (55, 5),
    (74, 5),
    (74, 17),
    (120, 16),
    (120, 117),
    (111, 121),
    (13, 120),
    (6, 109),
];
const ARRAY: Outline = &[
    (103, 19),
    (112, 28),
    (112, 101),
    (102, 111),
    (26, 111),
    (16, 101),
    (16, 28),
    (25, 19),
];
const RECLAIMER: Outline = &[
    (117, 9),
    (122, 16),
    (120, 118),
    (64, 121),
    (8, 118),
    (6, 16),
    (11, 9),
];
const REPAIR_BAY: Outline = &[
    (100, 13),
    (116, 25),
    (116, 105),
    (97, 115),
    (31, 115),
    (12, 105),
    (12, 25),
    (28, 13),
];
const EXTRACTOR: Outline = &[
    (114, 9),
    (122, 17),
    (122, 113),
    (113, 122),
    (15, 122),
    (6, 113),
    (6, 17),
    (14, 9),
];
const EXTRACTOR_T1: Outline = &[
    (82, 5),
    (86, 13),
    (120, 12),
    (124, 16),
    (124, 117),
    (114, 125),
    (14, 125),
    (4, 117),
    (4, 16),
    (8, 12),
    (42, 13),
    (46, 5),
];
const AIRWORKS: Outline = &[
    (111, 12),
    (120, 21),
    (120, 110),
    (110, 120),
    (18, 120),
    (8, 110),
    (8, 21),
    (17, 12),
];
const AIRWORKS_STAGE1: Outline = &[
    (9, 21),
    (18, 12),
    (110, 12),
    (113, 16),
    (120, 16),
    (120, 117),
    (10, 117),
];
const CRUCIBLE: Outline = &[
    (112, 8),
    (121, 17),
    (121, 111),
    (111, 121),
    (17, 121),
    (7, 111),
    (7, 17),
    (16, 8),
];
const CRUCIBLE_STAGE1: Outline = &[
    (8, 17),
    (17, 8),
    (111, 8),
    (120, 16),
    (120, 117),
    (114, 117),
    (112, 120),
    (10, 117),
];
const CRUCIBLE_STAGE2: Outline = &[
    (8, 17),
    (17, 8),
    (112, 8),
    (121, 18),
    (120, 117),
    (114, 117),
    (111, 121),
    (18, 121),
    (14, 117),
    (10, 117),
];
const BARRICADE: Outline = &[
    (128, 26),
    (128, 91),
    (121, 92),
    (121, 108),
    (106, 112),
    (102, 92),
    (26, 92),
    (22, 112),
    (7, 108),
    (7, 92),
    (0, 91),
    (0, 26),
];
const BARRICADE_STAGE1: Outline = &[
    (120, 16),
    (120, 26),
    (128, 27),
    (120, 117),
    (8, 117),
    (0, 27),
    (8, 26),
    (8, 16),
];
const SCUTTLE_CHARGE: Outline = &[
    (92, 36),
    (106, 50),
    (106, 80),
    (91, 95),
    (37, 95),
    (22, 80),
    (22, 50),
    (36, 36),
];
const SPIRE: Outline = &[
    (64, 8),
    (85, 11),
    (101, 21),
    (107, 21),
    (123, 62),
    (121, 82),
    (110, 102),
    (110, 107),
    (86, 121),
    (64, 125),
    (42, 121),
    (18, 107),
    (18, 102),
    (7, 82),
    (5, 62),
    (21, 21),
    (27, 21),
    (43, 11),
];
const SPIRE_STAGE1: Outline = &[
    (6, 62),
    (10, 47),
    (10, 16),
    (34, 17),
    (60, 8),
    (80, 10),
    (94, 17),
    (120, 16),
    (120, 117),
    (91, 117),
    (72, 124),
    (56, 124),
    (37, 117),
    (10, 117),
    (10, 85),
];
const LATTICE: Outline = &[
    (64, 12),
    (93, 29),
    (105, 26),
    (107, 37),
    (113, 39),
    (113, 45),
    (121, 60),
    (120, 70),
    (110, 82),
    (107, 107),
    (95, 108),
    (64, 117),
    (33, 108),
    (21, 107),
    (18, 82),
    (8, 70),
    (7, 60),
    (15, 45),
    (15, 39),
    (21, 37),
    (23, 26),
    (35, 29),
];
const LATTICE_STAGE2: Outline = &[
    (8, 58),
    (10, 54),
    (10, 16),
    (56, 17),
    (63, 12),
    (72, 17),
    (120, 16),
    (120, 117),
    (10, 117),
    (10, 70),
];
const MAW_CRADLE_STAGE0: Outline = &[
    (123, 0),
    (127, 1),
    (128, 5),
    (127, 127),
    (1, 127),
    (0, 5),
    (1, 1),
    (5, 0),
];
const ROC_CRADLE_STAGE0: Outline = &[
    (123, 0),
    (127, 1),
    (128, 5),
    (127, 127),
    (1, 127),
    (0, 5),
    (1, 1),
    (5, 0),
];

const FOUNDRY_ART: Outlines = Outlines {
    built: &[FOUNDRY],
    site: &[[SCAFFOLD, FOUNDRY_STAGE1, FOUNDRY_STAGE2]],
};
const TURRET_ART: Outlines = Outlines {
    built: &[TURRET, TURRET_T1, TURRET_T2],
    site: &[
        [SCAFFOLD, SCAFFOLD, SCAFFOLD],
        [SCAFFOLD, SCAFFOLD, SCAFFOLD],
        [SCAFFOLD, SCAFFOLD, SCAFFOLD],
    ],
};
const FABRICATOR_ART: Outlines = Outlines {
    built: &[FABRICATOR],
    site: &[[SCAFFOLD, FABRICATOR_STAGE1, FABRICATOR_STAGE1]],
};
const FLAK_TURRET_ART: Outlines = Outlines {
    built: &[FLAK_TURRET, FLAK_TURRET_T1],
    site: &[
        [SCAFFOLD, SCAFFOLD, SCAFFOLD],
        [SCAFFOLD, SCAFFOLD, SCAFFOLD],
    ],
};
const BASTION_ART: Outlines = Outlines {
    built: &[BASTION, BASTION_T1],
    site: &[
        [SCAFFOLD, BASTION_STAGE1, BASTION_STAGE1],
        [SCAFFOLD, BASTION_T1_STAGE1, BASTION_T1_STAGE1],
    ],
};
const ARRAY_ART: Outlines = Outlines {
    built: &[ARRAY, ARRAY],
    site: &[
        [SCAFFOLD, SCAFFOLD, SCAFFOLD],
        [SCAFFOLD, SCAFFOLD, SCAFFOLD],
    ],
};
const RECLAIMER_ART: Outlines = Outlines {
    built: &[RECLAIMER, RECLAIMER],
    site: &[
        [SCAFFOLD, RECLAIMER, RECLAIMER],
        [SCAFFOLD, RECLAIMER, RECLAIMER],
    ],
};
const REPAIR_BAY_ART: Outlines = Outlines {
    built: &[REPAIR_BAY],
    site: &[[SCAFFOLD, SCAFFOLD, SCAFFOLD]],
};
const EXTRACTOR_ART: Outlines = Outlines {
    built: &[EXTRACTOR, EXTRACTOR_T1],
    site: &[
        [SCAFFOLD, EXTRACTOR, EXTRACTOR],
        [SCAFFOLD, EXTRACTOR_T1, EXTRACTOR_T1],
    ],
};
const AIRWORKS_ART: Outlines = Outlines {
    built: &[AIRWORKS],
    site: &[[SCAFFOLD, AIRWORKS_STAGE1, AIRWORKS_STAGE1]],
};
const CRUCIBLE_ART: Outlines = Outlines {
    built: &[CRUCIBLE],
    site: &[[SCAFFOLD, CRUCIBLE_STAGE1, CRUCIBLE_STAGE2]],
};
const BARRICADE_ART: Outlines = Outlines {
    built: &[BARRICADE],
    site: &[[SCAFFOLD, BARRICADE_STAGE1, BARRICADE_STAGE1]],
};
const SCUTTLE_CHARGE_ART: Outlines = Outlines {
    built: &[SCUTTLE_CHARGE],
    site: &[[SCAFFOLD, SCAFFOLD, SCAFFOLD]],
};
const SPIRE_ART: Outlines = Outlines {
    built: &[SPIRE],
    site: &[[SCAFFOLD, SPIRE_STAGE1, SPIRE_STAGE1]],
};
const LATTICE_ART: Outlines = Outlines {
    built: &[LATTICE],
    site: &[[SCAFFOLD, SCAFFOLD, LATTICE_STAGE2]],
};
const MAW_CRADLE_ART: Outlines = Outlines {
    built: &[],
    site: &[[MAW_CRADLE_STAGE0, MAW_CRADLE_STAGE0, MAW_CRADLE_STAGE0]],
};
const ROC_CRADLE_ART: Outlines = Outlines {
    built: &[],
    site: &[[ROC_CRADLE_STAGE0, ROC_CRADLE_STAGE0, ROC_CRADLE_STAGE0]],
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
        BuildingKind::Reclaimer => RECLAIMER_ART,
        BuildingKind::RepairBay => REPAIR_BAY_ART,
        BuildingKind::Extractor => EXTRACTOR_ART,
        BuildingKind::Airworks => AIRWORKS_ART,
        BuildingKind::Crucible => CRUCIBLE_ART,
        BuildingKind::Barricade => BARRICADE_ART,
        BuildingKind::ScuttleCharge => SCUTTLE_CHARGE_ART,
        BuildingKind::Spire => SPIRE_ART,
        BuildingKind::Lattice => LATTICE_ART,
        BuildingKind::MawCradle => MAW_CRADLE_ART,
        BuildingKind::RocCradle => ROC_CRADLE_ART,
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
                // Boxes this far apart keep every point of one segment
                // farther than the radius from the other.
                if box_gap(from, to, a, b) > radius.abs() + ROUNDING_MARGIN {
                    return true;
                }
                if segments_cross(from, to, a, b) {
                    return false;
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
