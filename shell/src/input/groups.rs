//! Control groups: saved selections of own units or own buildings.

use crate::game::{Game, Scene};
use crate::panel::CardIcon;
use macroquad::prelude::{Vec2, vec2};
use oxide_sim::{BuildingId, UnitId};

/// A saved control group. Like a selection, it holds units or buildings,
/// never both, so every command path that reads a selection reads a
/// recalled group the same way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Group {
    Units(Vec<UnitId>),
    Buildings(Vec<BuildingId>),
}

impl Default for Group {
    fn default() -> Self {
        Self::Units(Vec::new())
    }
}

impl Group {
    /// The selection's own members: its units, or its buildings when it
    /// holds no units.
    pub(crate) fn of_selection(game: &Scene<'_>) -> Self {
        let selection = &game.presentation.selection;
        if selection.units.is_empty() && !selection.buildings.is_empty() {
            Self::Buildings(selection.buildings.clone())
        } else {
            Self::Units(selection.units.clone())
        }
        .live(game)
    }

    /// The members still standing and still the viewer's, in saved order.
    /// Ownership, not mere existence: after a session change a stale id
    /// could name anyone's, should one survive `reset_session`.
    pub(crate) fn live(&self, game: &Scene<'_>) -> Self {
        let human = game.presentation.human;
        match self {
            Self::Units(ids) => Self::Units(
                ids.iter()
                    .copied()
                    .filter(|id| game.state.unit(*id).is_some_and(|u| u.player == human))
                    .collect(),
            ),
            Self::Buildings(ids) => Self::Buildings(
                ids.iter()
                    .copied()
                    .filter(|id| game.state.building(*id).is_some_and(|b| b.player == human))
                    .collect(),
            ),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Units(ids) => ids.len(),
            Self::Buildings(ids) => ids.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether both name the same members, in any order.
    pub(crate) fn same_members(&self, other: &Self) -> bool {
        fn sorted<T: Ord + Copy>(ids: &[T]) -> Vec<T> {
            let mut ids = ids.to_vec();
            ids.sort_unstable();
            ids
        }
        match (self, other) {
            (Self::Units(a), Self::Units(b)) => sorted(a) == sorted(b),
            (Self::Buildings(a), Self::Buildings(b)) => sorted(a) == sorted(b),
            _ => self.is_empty() && other.is_empty(),
        }
    }

    /// Replaces the selection with this group.
    pub(crate) fn select(&self, game: &mut Game) {
        let selection = &mut game.presentation.selection;
        match self {
            Self::Units(ids) => {
                selection.units.clone_from(ids);
                selection.buildings.clear();
            }
            Self::Buildings(ids) => {
                selection.units.clear();
                selection.buildings.clone_from(ids);
                selection.buildings.sort_unstable();
            }
        }
    }

    /// The members' average position, in world tiles.
    pub(crate) fn center(&self, game: &Scene<'_>) -> Option<Vec2> {
        let points: Vec<Vec2> = match self {
            Self::Units(ids) => ids
                .iter()
                .filter_map(|id| game.state.unit(*id))
                .map(|u| vec2(u.pos.x.to_num::<f32>(), u.pos.y.to_num::<f32>()))
                .collect(),
            Self::Buildings(ids) => ids
                .iter()
                .filter_map(|id| game.state.building(*id))
                .map(|b| {
                    let center = b.center();
                    vec2(center.x.to_num::<f32>(), center.y.to_num::<f32>())
                })
                .collect(),
        };
        (!points.is_empty()).then(|| points.iter().sum::<Vec2>() / points.len() as f32)
    }

    /// The icon a slot wears: the kind most members share, the kind
    /// holding the lowest id among equals.
    pub(crate) fn lead(&self, game: &Scene<'_>) -> Option<CardIcon> {
        match self {
            Self::Units(ids) => {
                let mut kinds: Vec<_> = ids
                    .iter()
                    .filter_map(|id| game.state.unit(*id))
                    .map(|u| (u.kind, u.id))
                    .collect();
                kinds.sort_unstable_by_key(|&(_, id)| id);
                majority(&kinds).map(CardIcon::Unit)
            }
            Self::Buildings(ids) => {
                let mut members: Vec<_> = ids
                    .iter()
                    .filter_map(|id| game.state.building(*id))
                    .collect();
                members.sort_unstable_by_key(|b| b.id);
                let kinds: Vec<_> = members.iter().map(|b| (b.kind, b.id)).collect();
                let kind = majority(&kinds)?;
                let tier = members.iter().find(|b| b.kind == kind)?.tier;
                Some(CardIcon::Building(kind, tier))
            }
        }
    }
}

/// The most common kind among id-ordered members; the first kind seen
/// wins a tie.
fn majority<K: Copy + PartialEq, I>(members: &[(K, I)]) -> Option<K> {
    let mut counts: Vec<(K, usize)> = Vec::new();
    for &(kind, _) in members {
        match counts.iter_mut().find(|(k, _)| *k == kind) {
            Some((_, n)) => *n += 1,
            None => counts.push((kind, 1)),
        }
    }
    counts
        .iter()
        .rev()
        .max_by_key(|(_, n)| *n)
        .map(|&(kind, _)| kind)
}
