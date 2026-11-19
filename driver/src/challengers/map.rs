//! What every seat knows of a map before the match starts: terrain, scrap
//! nodes and Extractor frames as authored, and every seat's starting
//! Foundry. Nothing here changes during a match.

use aftermine_sim::{BuildingKind, PlayerId, Scenario};
use anyhow::{Context, Result, ensure};
use chassis::grid::TilePos;
use std::collections::VecDeque;

/// Steps of a walk, for tiles it cannot reach.
pub(super) const UNREACHED: i32 = i32::MAX;

/// The eight neighbours of a tile, straight ones first.
const NEIGHBOURS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

/// Public facts of one scenario, seen from one seat.
#[derive(Debug, Clone)]
pub(super) struct MapFacts {
    width: i32,
    height: i32,
    open: Vec<bool>,
    /// The tiles routes cross: open ground, or, on a map where no ground
    /// route joins the two Foundries, every tile aircraft fly over.
    route: Vec<bool>,
    /// Authored scrap tiles, row-major.
    pub(super) nodes: Vec<TilePos>,
    /// Authored Extractor frame anchors, row-major.
    pub(super) frames: Vec<TilePos>,
    /// This seat's starting Foundry anchor.
    pub(super) home: TilePos,
    /// The hostile seat whose starting Foundry is the shortest walk away.
    pub(super) enemy_player: PlayerId,
    /// That seat's starting Foundry anchor.
    pub(super) enemy: TilePos,
    from_home: Vec<i32>,
    from_enemy: Vec<i32>,
}

impl MapFacts {
    /// The briefing for `player`, who must hold a starting Foundry with a
    /// hostile seat somewhere on the map. Distances count ground steps, or
    /// flying steps where no ground route joins the two Foundries.
    pub(super) fn new(scenario: &Scenario, player: PlayerId) -> Result<Self> {
        let (map, anchors) = scenario
            .parse_map_and_anchors()
            .context("parsing the scenario's map")?;
        let width = map.width();
        let height = map.height();
        let tiles = || (0..height).flat_map(move |y| (0..width).map(move |x| TilePos::new(x, y)));
        let open: Vec<bool> = tiles().map(|t| map.terrain_passable(t)).collect();
        let sky: Vec<bool> = tiles().map(|t| map.sky_open(t)).collect();
        let nodes = tiles().filter(|&t| map.scrap_at(t) > 0).collect();
        let frames = map.extractor_frames().to_vec();
        let team = |seat: PlayerId| {
            scenario
                .players
                .get(usize::from(seat.0))
                .and_then(|spec| spec.team)
        };
        let home = anchors
            .iter()
            .find(|(seat, _)| *seat == player)
            .map(|(_, anchor)| *anchor)
            .with_context(|| format!("seat {} has no starting Foundry", player.0))?;
        let mut facts = Self {
            width,
            height,
            route: open.clone(),
            open,
            nodes,
            frames,
            home,
            enemy_player: player,
            enemy: home,
            from_home: Vec::new(),
            from_enemy: Vec::new(),
        };
        facts.from_home = facts.walk_from(home, BuildingKind::Foundry.size());
        let mine = team(player);
        let (enemy_player, enemy) = anchors
            .iter()
            .filter(|(seat, _)| *seat != player && (mine.is_none() || team(*seat) != mine))
            .min_by_key(|(seat, anchor)| (facts.steps_from_home(*anchor), seat.0))
            .copied()
            .context("no hostile seat holds a starting Foundry")?;
        if facts.steps_from_home(enemy) == UNREACHED {
            facts.route = sky;
            facts.from_home = facts.walk_from(home, BuildingKind::Foundry.size());
        }
        ensure!(
            facts.steps_from_home(enemy) != UNREACHED,
            "no route joins seat {}'s Foundry to seat {}'s",
            player.0,
            enemy_player.0
        );
        facts.enemy_player = enemy_player;
        facts.enemy = enemy;
        facts.from_enemy = facts.walk_from(enemy, BuildingKind::Foundry.size());
        Ok(facts)
    }

    fn index(&self, tile: TilePos) -> Option<usize> {
        (tile.x >= 0 && tile.y >= 0 && tile.x < self.width && tile.y < self.height)
            .then(|| tile.row_major(self.width))
    }

    /// Whether authored terrain lets a ground unit stand on `tile`.
    pub(super) fn open(&self, tile: TilePos) -> bool {
        self.index(tile).is_some_and(|i| self.open[i])
    }

    /// Whether routes cross `tile`.
    fn crossable(&self, tile: TilePos) -> bool {
        self.index(tile).is_some_and(|i| self.route[i])
    }

    /// Route steps from this seat's starting Foundry.
    pub(super) fn steps_from_home(&self, tile: TilePos) -> i32 {
        self.index(tile)
            .and_then(|i| self.from_home.get(i).copied())
            .unwrap_or(UNREACHED)
    }

    /// Route steps from the enemy's starting Foundry.
    pub(super) fn steps_from_enemy(&self, tile: TilePos) -> i32 {
        self.index(tile)
            .and_then(|i| self.from_enemy.get(i).copied())
            .unwrap_or(UNREACHED)
    }

    /// Route steps between the two starting Foundries.
    pub(super) fn route_length(&self) -> i32 {
        self.steps_from_home(self.enemy)
    }

    /// The centre tile of this seat's starting Foundry.
    pub(super) fn home_centre(&self) -> TilePos {
        self.home.offset(1, 1)
    }

    /// The centre tile of the enemy's starting Foundry.
    pub(super) fn enemy_centre(&self) -> TilePos {
        self.enemy.offset(1, 1)
    }

    /// A tile `steps` ground steps from the enemy Foundry on a near-shortest
    /// route from home: of those within `slack` steps of the shortest route,
    /// the one `lane` picks across the route's width, from 0 (one flank)
    /// through 2 (the other).
    pub(super) fn approach(&self, steps: i32, slack: i32, lane: u8) -> TilePos {
        let total = self.route_length();
        let mut candidates: Vec<(i64, TilePos)> = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let tile = TilePos::new(x, y);
                let (from_home, from_enemy) =
                    (self.steps_from_home(tile), self.steps_from_enemy(tile));
                if from_enemy == steps
                    && from_home != UNREACHED
                    && from_home + from_enemy <= total + slack
                {
                    candidates.push((self.across(tile), tile));
                }
            }
        }
        candidates.sort_unstable();
        match candidates.len() {
            0 => self.enemy_centre(),
            len => {
                let index = match lane {
                    0 => 0,
                    1 => len / 2,
                    _ => len - 1,
                };
                candidates[index].1
            }
        }
    }

    /// Signed offset of `tile` across the line from home to the enemy, in
    /// tile units scaled by the line's length.
    fn across(&self, tile: TilePos) -> i64 {
        let (hx, hy) = (i64::from(self.home.x), i64::from(self.home.y));
        let (ex, ey) = (i64::from(self.enemy.x), i64::from(self.enemy.y));
        let (tx, ty) = (i64::from(tile.x), i64::from(tile.y));
        (ex - hx) * (ty - hy) - (ey - hy) * (tx - hx)
    }

    /// Route steps from a footprint, with diagonal steps that cut no
    /// corner.
    fn walk_from(&self, anchor: TilePos, (w, h): (i32, i32)) -> Vec<i32> {
        let mut steps = vec![UNREACHED; self.open.len()];
        let mut queue = VecDeque::new();
        for dy in 0..h {
            for dx in 0..w {
                let tile = anchor.offset(dx, dy);
                if let Some(i) = self.index(tile) {
                    steps[i] = 0;
                    queue.push_back(tile);
                }
            }
        }
        while let Some(tile) = queue.pop_front() {
            let here = steps[tile.row_major(self.width)];
            for (dx, dy) in NEIGHBOURS {
                let next = tile.offset(dx, dy);
                let Some(i) = self.index(next) else { continue };
                if !self.route[i] || steps[i] != UNREACHED {
                    continue;
                }
                if dx != 0
                    && dy != 0
                    && !(self.crossable(tile.offset(dx, 0)) && self.crossable(tile.offset(0, dy)))
                {
                    continue;
                }
                steps[i] = here + 1;
                queue.push_back(next);
            }
        }
        steps
    }

    /// The tile `steps` route steps from `from` along a shortest route to
    /// `to`: `to` itself when it is nearer, and `from` when no route joins
    /// them.
    pub(super) fn walk_toward(&self, from: TilePos, to: TilePos, steps: i32) -> TilePos {
        let field = self.walk_from(to, (1, 1));
        let at_field = |tile: TilePos| {
            self.index(tile)
                .and_then(|i| field.get(i).copied())
                .unwrap_or(UNREACHED)
        };
        let mut at = from;
        for _ in 0..steps {
            let here = at_field(at);
            if here == 0 || here == UNREACHED {
                break;
            }
            let next = NEIGHBOURS
                .iter()
                .map(|&(dx, dy)| (at.offset(dx, dy), dx, dy))
                .filter(|&(tile, dx, dy)| {
                    self.crossable(tile)
                        && (dx == 0
                            || dy == 0
                            || (self.crossable(at.offset(dx, 0))
                                && self.crossable(at.offset(0, dy))))
                })
                .map(|(tile, _, _)| tile)
                .min_by_key(|&tile| (at_field(tile), tile.y, tile.x));
            match next {
                Some(tile) if at_field(tile) < here => at = tile,
                _ => break,
            }
        }
        at
    }

    /// The authored scrap tiles and Extractor frames within `steps` ground
    /// steps of the enemy Foundry, nearest first: where its workers mine.
    pub(super) fn enemy_harvest_sites(&self, steps: i32) -> Vec<TilePos> {
        let mut sites: Vec<(i32, TilePos)> = self
            .frames
            .iter()
            .map(|&frame| (self.frame_steps_from_enemy(frame), frame.offset(1, 1)))
            .chain(
                self.nodes
                    .iter()
                    .map(|&node| (self.node_steps(node, |t| self.steps_from_enemy(t)), node)),
            )
            .filter(|(d, _)| *d <= steps)
            .collect();
        sites.sort_unstable_by_key(|(d, tile)| (*d, tile.y, tile.x));
        let mut picked: Vec<TilePos> = Vec::new();
        for (_, tile) in sites {
            if picked.iter().all(|p| p.chebyshev(tile) > 3) {
                picked.push(tile);
            }
        }
        picked
    }

    fn frame_steps_from_enemy(&self, frame: TilePos) -> i32 {
        (0..2)
            .flat_map(|dy| (0..2).map(move |dx| frame.offset(dx, dy)))
            .map(|t| self.steps_from_enemy(t))
            .min()
            .unwrap_or(UNREACHED)
    }

    /// Steps to the nearest open tile beside a scrap node, which itself
    /// blocks the ground.
    fn node_steps(&self, node: TilePos, steps: impl Fn(TilePos) -> i32) -> i32 {
        let mut best = UNREACHED;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let tile = node.offset(dx, dy);
                if self.open(tile) {
                    best = best.min(steps(tile).saturating_add(1));
                }
            }
        }
        best
    }
}
