//! What the sky over a map allows, derived from its terrain. Peaks are the
//! only terrain aircraft route around, and terrain never changes in a match,
//! so nothing here is serialized, hashed or compared.

use chassis::grid::TilePos;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The sky's components, labelled as [`chassis::path::cardinal_components`]
/// labels air passability, so two tiles share a label exactly when an air
/// route joins them. Built once with the state; clones share them.
#[derive(Clone, Default)]
pub(crate) struct SkyLabels(Arc<[u32]>);

impl SkyLabels {
    /// The labels of `state`'s sky.
    pub(crate) fn of(state: &super::State) -> Self {
        Self(
            chassis::path::cardinal_components(state.map.width(), state.map.height(), |tile| {
                state.passable_for(crate::stats::Domain::Air, tile)
            })
            .into(),
        )
    }

    /// One label per map tile, row-major; 0 under a peak.
    pub(crate) fn labels(&self) -> &[u32] {
        &self.0
    }
}

impl PartialEq for SkyLabels {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for SkyLabels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SkyLabels").finish_non_exhaustive()
    }
}

/// Air routes kept; once full, each new search replaces the oldest.
const AIR_ROUTES: usize = 16;

/// The air routes searched most recently, by start tile and goal. A flyer
/// whose nose meets a peak face drops its route and plans again from a
/// tile it has not yet left, so the same search repeats tick after tick.
///
/// A clone starts empty: it keeps nothing a clone could need, and shares
/// nothing with the original.
#[derive(Default)]
pub(crate) struct AirRoutes {
    recent: Mutex<Recent>,
}

/// A search's start tile and goal, and the route it found.
type AirRoute = ((TilePos, TilePos), Option<Vec<TilePos>>);

#[derive(Default)]
struct Recent {
    entries: Vec<AirRoute>,
    /// The entry the next new search replaces once the memo is full.
    next: usize,
}

impl AirRoutes {
    /// The air route from `from` to `to`, running `search` only when no
    /// recent search answered the same pair. `search` must depend on
    /// nothing but the pair and the sky.
    pub(crate) fn route(
        &self,
        from: TilePos,
        to: TilePos,
        search: impl FnOnce() -> Option<Vec<TilePos>>,
    ) -> Option<Vec<TilePos>> {
        let key = (from, to);
        if let Some((_, route)) = self.recent().entries.iter().find(|(k, _)| *k == key) {
            return route.clone();
        }
        let route = search();
        let mut recent = self.recent();
        let entry = (key, route.clone());
        if recent.entries.len() < AIR_ROUTES {
            recent.entries.push(entry);
        } else {
            let next = recent.next;
            recent.entries[next] = entry;
        }
        recent.next = (recent.next + 1) % AIR_ROUTES;
        route
    }

    /// Entries are written whole after the search, so a panic elsewhere
    /// never leaves a torn one behind.
    fn recent(&self) -> MutexGuard<'_, Recent> {
        self.recent.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Clone for AirRoutes {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for AirRoutes {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl std::fmt::Debug for AirRoutes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AirRoutes").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
