//! Public buildings every seat knows of, such as Deep Bores, join the seat's
//! known enemy buildings as memories. Every decision that reads known enemy
//! buildings (targets, raids, held frames) then treats a Deep Bore it never
//! saw like one it remembers. Read fresh each decision; nothing persists.

use oxide_sim::BuildingId;
use oxide_sim::observation::{BuildingObs, ObservationData};

/// Adds each public building the seat neither sees nor remembers, and brings a
/// remembered one up to date. Its condition is unknown: it counts as built, at
/// its public tier's full health.
pub(crate) fn merge(observation: &mut ObservationData) {
    for public in &observation.public_buildings {
        let known = observation.enemy_buildings.iter_mut().find(|building| {
            building.player == public.owner
                && building.kind == public.kind
                && building.anchor == public.anchor
        });
        match known {
            Some(building) => {
                if !building.seen {
                    building.tier = public.tier;
                    building.built = true;
                    building.hp = public.kind.tier_stats(public.tier).max_hp;
                }
            }
            None => observation.enemy_buildings.push(BuildingObs {
                provisional: false,
                id: BuildingId(u32::MAX),
                player: public.owner,
                kind: public.kind,
                anchor: public.anchor,
                hp: public.kind.tier_stats(public.tier).max_hp,
                built: true,
                seen: false,
                tier: public.tier,
            }),
        }
    }
    observation
        .enemy_buildings
        .sort_by_key(|building| (building.anchor.y, building.anchor.x, building.player));
}

#[cfg(test)]
mod tests;
