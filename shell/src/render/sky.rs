//! What a viewer may see of a missile in the sky: the one rule the sky's
//! drawing and its motor loops share.

use macroquad::prelude::Vec2;

/// Whether a missile over `ground`, its world position on the ground
/// track, shows to this viewer now. Everyone sees missiles true to fog,
/// whoever owns them: only while the tile under it is in sight, or in an
/// omniscient view.
pub(crate) fn missile_seen(game: &crate::game::Scene<'_>, ground: Vec2) -> bool {
    missile_seen_by(game.presentation, game.state, ground)
}

/// [`missile_seen`] for a presentation and the state it shows.
pub(crate) fn missile_seen_by(
    presentation: &crate::game::Presentation,
    state: &oxide_sim::State,
    ground: Vec2,
) -> bool {
    presentation.all_seeing()
        || state
            .vision(presentation.human)
            .visible(crate::numeric::tile_at(ground))
}

/// Where a warhead on `line` stands over the ground at presentation `tick`,
/// and how far along its flight it is.
pub(crate) fn warhead_track(line: oxide_sim::strike::WarheadLine, tick: f32) -> (Vec2, f32) {
    let ticks = oxide_sim::stats::WARHEAD.flight_ticks(line.launch, line.impact);
    let launch = crate::game::world_vec(line.launch);
    let impact = crate::game::world_vec(line.impact);
    let launched_at = line.arrival.saturating_sub(ticks) as f32;
    let span = (line.arrival as f32 - launched_at).max(1.0);
    let progress = ((tick - launched_at) / span).clamp(0.0, 1.0);
    (launch + (impact - launch) * progress, progress)
}

#[cfg(test)]
mod tests;
