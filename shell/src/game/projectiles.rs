//! Launch facts retained only through their authoritative payload arrival.

use macroquad::prelude::{Vec2, vec2};
use oxide_sim::state::Shell;
use oxide_sim::{Event, ProjectileKind, State, Target, UnitKind};
use std::collections::HashMap;

#[derive(Debug, Default)]
pub(crate) struct ProjectileReleases {
    releases: HashMap<(Target, u64), Vec<Flight>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LaunchPose {
    pub(crate) heading: Vec2,
    pub(crate) kind: UnitKind,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Flight {
    pub(crate) target: Option<Target>,
    pub(crate) ticks: u64,
    pub(crate) pose: Option<LaunchPose>,
    /// Where it was aimed, which tells apart the record of a shell shot down
    /// in flight from its siblings'.
    impact: chassis::fx::Vec2Fx,
}

impl ProjectileReleases {
    pub(crate) fn observe(&mut self, state: &State, events: &[Event]) {
        // The landing report is read after the arrival tick has completed.
        self.releases
            .retain(|(_, arrival), _| *arrival >= state.current_tick().saturating_sub(1));
        for event in events {
            if let Event::ShellIntercepted {
                shooter,
                impact,
                arrival,
                ..
            } = event
                && let Some(flights) = self.releases.get_mut(&(*shooter, *arrival))
                && let Some(slot) = flights.iter().position(|flight| flight.impact == *impact)
            {
                // Later siblings keep their launch order, so the occurrence
                // index of every shell still in flight finds its own record.
                flights.remove(slot);
            }
            if let Event::ShellLaunched {
                shooter,
                target,
                unit_pose,
                flight,
                to,
                ..
            } = event
            {
                let releases = self
                    .releases
                    .entry((*shooter, state.current_tick() - 1 + flight))
                    .or_default();
                let pose = unit_pose.map(|pose| {
                    let heading = chassis::compass::dir(pose.heading);
                    LaunchPose {
                        heading: vec2(heading.x.to_num::<f32>(), heading.y.to_num::<f32>()),
                        kind: pose.kind,
                    }
                });
                releases.push(Flight {
                    target: *target,
                    ticks: *flight,
                    pose,
                    impact: *to,
                });
            }
        }
    }

    pub(crate) fn flight(&self, shells: &[Shell], index: usize) -> Option<Flight> {
        let shell = shells.get(index)?;
        let releases = self.releases.get(&(shell.shooter, shell.arrival))?;
        // Several shells from one shooter can share an arrival tick. They
        // retain launch order and are removed together.
        let occurrence = shells[..index]
            .iter()
            .filter(|earlier| earlier.shooter == shell.shooter && earlier.arrival == shell.arrival)
            .count();
        releases.get(occurrence).copied()
    }

    pub(crate) fn artillery_heading(&self, shell: &Shell) -> Option<Vec2> {
        self.releases
            .get(&(shell.shooter, shell.arrival))?
            .first()?
            .pose
            .filter(|pose| crate::look::fires(pose.kind, ProjectileKind::Shell))
            .map(|pose| pose.heading)
    }

    pub(crate) fn bomber_pose(
        &self,
        state: &State,
        shells: &[Shell],
        index: usize,
    ) -> Option<LaunchPose> {
        self.release(shells, index).or_else(|| {
            let shell = shells.get(index)?;
            if shell.kind != oxide_sim::ProjectileKind::Bomb {
                return None;
            }
            let oxide_sim::Target::Unit(id) = shell.shooter else {
                return None;
            };
            let kind = state.unit(id)?.kind;
            if !crate::look::fires(kind, oxide_sim::ProjectileKind::Bomb) {
                return None;
            }
            // Seeks without launch history use a stable impact line, never the
            // aircraft's later egress heading.
            Some(LaunchPose {
                heading: vec2(
                    (shell.impact.x - shell.launch.x).to_num::<f32>(),
                    (shell.impact.y - shell.launch.y).to_num::<f32>(),
                )
                .normalize_or_zero(),
                kind,
            })
        })
    }

    pub(crate) fn release(&self, shells: &[Shell], index: usize) -> Option<LaunchPose> {
        (shells.get(index)?.kind == ProjectileKind::Bomb)
            .then(|| self.flight(shells, index)?.pose)?
            .filter(|pose| crate::look::fires(pose.kind, ProjectileKind::Bomb))
    }
}

#[cfg(test)]
mod tests;
