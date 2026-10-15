//! Public buildings every seat knows of, such as Deep Bores and assembly
//! cradles, join the seat's known enemy buildings as memories. Every decision
//! that reads known enemy buildings (targets, raids, held frames) then treats
//! one it never saw like one it remembers. Read fresh each decision; nothing
//! persists.

use oxide_sim::BuildingId;
use oxide_sim::observation::{BuildingObs, ObservationData};

/// Adds each public building the seat neither sees nor remembers, and brings a
/// remembered one up to date. Its condition is unknown: it counts as built, at
/// its public tier's full health, except a cradle, which counts unbuilt at the
/// health its known progress raised.
pub(crate) fn merge(observation: &mut ObservationData) {
    for public in &observation.public_buildings {
        let (built, hp) = condition(*public);
        let known = observation.enemy_buildings.iter_mut().find(|building| {
            building.player == public.owner
                && building.kind == public.kind
                && building.anchor == public.anchor
        });
        match known {
            Some(building) => {
                if !building.seen {
                    building.tier = public.tier;
                    building.built = built;
                    building.hp = hp;
                }
            }
            None => observation.enemy_buildings.push(BuildingObs {
                provisional: false,
                id: BuildingId(u32::MAX),
                player: public.owner,
                kind: public.kind,
                anchor: public.anchor,
                hp,
                built,
                seen: false,
                tier: public.tier,
            }),
        }
    }
    observation
        .enemy_buildings
        .sort_by_key(|building| (building.anchor.y, building.anchor.x, building.player));
}

/// Whether a public building stands finished, and the health it counts at:
/// a site's starting fifth and the share its work has raised since.
fn condition(public: oxide_sim::PublicBuilding) -> (bool, u32) {
    let stats = public.kind.tier_stats(public.tier);
    let (Some(progress), Some(construction)) = (public.progress, stats.construction) else {
        return (true, stats.max_hp);
    };
    let start = u64::from(stats.max_hp / 5);
    let raised = start
        + (u64::from(stats.max_hp) - start) * u64::from(progress)
            / u64::from(construction.build_ticks.max(1));
    (false, u32::try_from(raised).unwrap_or(stats.max_hp))
}

#[cfg(test)]
mod tests;
