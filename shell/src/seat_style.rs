//! Viewer-relative presentation identity, prepared once for a live or replay view.
use aftermine_sim::scenario::MAX_PLAYERS;
use aftermine_sim::{PlayerId, State};
use macroquad::prelude::{Color, WHITE, color_u8};

/// Relationship to the viewer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AllegianceCue {
    Mine,
    Ally,
    Hostile,
}

impl AllegianceCue {
    /// How `viewer` sees `owner`.
    pub fn of(state: &State, viewer: PlayerId, owner: PlayerId) -> Self {
        if owner == viewer {
            Self::Mine
        } else if state.hostile(viewer, owner) {
            Self::Hostile
        } else {
            Self::Ally
        }
    }
}

/// Every seat's ownership color as one viewer sees it.
#[derive(Clone, Copy)]
pub(crate) struct SeatStyles([Color; MAX_PLAYERS]);

impl SeatStyles {
    pub fn new(state: &State, viewer: PlayerId, team_colors: bool) -> Self {
        let teams: Vec<u16> = state
            .players()
            .iter()
            .map(|player| u16::from(player.team))
            .collect();
        Self::from_teams(&teams, usize::from(viewer.0), team_colors)
    }

    /// Styles for seats in seat order, where seats with equal `teams` entries
    /// play on one team and `mine` is the viewer's seat. Each enemy team takes
    /// the next family in order of its first seat, and its seats take that
    /// family's shades in seat order, unless `team_colors` gives allies one
    /// color and enemies another.
    pub fn from_teams(teams: &[u16], mine: usize, team_colors: bool) -> Self {
        let my_team = teams.get(mine).copied();
        let has_allies = teams
            .iter()
            .enumerate()
            .any(|(seat, team)| seat != mine && Some(*team) == my_team);
        let families: &[Family] = if has_allies {
            &WITH_ALLIES
        } else {
            &WITHOUT_ALLIES
        };
        let mut allies = 0;
        let mut enemy_teams: Vec<(u16, usize)> = Vec::new();
        let mut styles = [WHITE; MAX_PLAYERS];
        for (seat, (style, team)) in styles.iter_mut().zip(teams).enumerate() {
            *style = if seat == mine {
                self_color(team_colors)
            } else if Some(*team) == my_team {
                if team_colors {
                    TEAM_ALLY
                } else {
                    allies += 1;
                    family_color(&ALLIES, allies - 1)
                }
            } else if team_colors {
                TEAM_ENEMY
            } else {
                let index = enemy_teams
                    .iter()
                    .position(|(known, _)| known == team)
                    .unwrap_or_else(|| {
                        enemy_teams.push((*team, 0));
                        enemy_teams.len() - 1
                    });
                let members = &mut enemy_teams[index].1;
                // Past the last family, teams reuse them in later shades.
                let rank = *members + SHADES * (index / families.len());
                *members += 1;
                family_color(&families[index % families.len()], rank)
            };
        }
        Self(styles)
    }

    pub fn get(&self, owner: PlayerId) -> Color {
        self.0[usize::from(owner.0)]
    }
}

/// Team colors' allies, clear of the enemy and self colors under typical,
/// red-weak, green-weak and blue-weak vision alike.
const TEAM_ALLY: Color = color_u8!(90, 150, 245, 255);
/// Team colors' enemies: a bright red, which holds apart from the near-white
/// self color and the blue allies where the team-game crimson would not.
const TEAM_ENEMY: Color = color_u8!(228, 44, 58, 255);

/// Your own seat's color: a hue neither the ally nor the hostile family uses,
/// so "mine" never reads as a friend or a foe.
pub(crate) fn self_color(team_colors: bool) -> Color {
    // Near-white holds apart from both families under every color-vision type,
    // where the default green collapses toward a warm hue.
    if team_colors {
        color_u8!(245, 245, 240, 255)
    } else {
        color_u8!(40, 190, 110, 255)
    }
}

/// Shades in each family: a team's seats wear them in seat order.
const SHADES: usize = 4;

/// One team's colors: a lead color, then lighter, darker and further shades of
/// the same hue, so a team reads as one and its seats still tell apart. A team
/// of two wears the lead and the lighter shade.
type Family = [Color; SHADES];

/// The viewer's allies: blues, which no enemy team wears beside an ally.
const ALLIES: Family = [
    color_u8!(110, 190, 250, 255),
    color_u8!(195, 230, 255, 255),
    color_u8!(40, 150, 240, 255),
    color_u8!(60, 100, 245, 255),
];

// Light reds read as the self green under red-weak vision, so the reds stay
// mid to dark.
const REDS: Family = [
    color_u8!(228, 44, 58, 255),
    color_u8!(235, 80, 110, 255),
    color_u8!(165, 25, 40, 255),
    color_u8!(140, 25, 65, 255),
];
const BLUES: Family = [
    color_u8!(60, 100, 245, 255),
    color_u8!(110, 150, 255, 255),
    color_u8!(40, 70, 190, 255),
    color_u8!(170, 195, 255, 255),
];
const YELLOWS: Family = [
    color_u8!(250, 210, 60, 255),
    color_u8!(255, 232, 120, 255),
    color_u8!(200, 150, 25, 255),
    color_u8!(255, 245, 175, 255),
];
const PURPLES: Family = [
    color_u8!(150, 70, 220, 255),
    color_u8!(190, 130, 245, 255),
    color_u8!(110, 45, 175, 255),
    color_u8!(215, 180, 255, 255),
];
const ORANGES: Family = [
    color_u8!(245, 130, 40, 255),
    color_u8!(255, 170, 90, 255),
    color_u8!(190, 90, 25, 255),
    color_u8!(255, 205, 150, 255),
];
const TEALS: Family = [
    color_u8!(40, 180, 190, 255),
    color_u8!(110, 215, 220, 255),
    color_u8!(25, 130, 140, 255),
    color_u8!(170, 235, 235, 255),
];
const PINKS: Family = [
    color_u8!(245, 120, 190, 255),
    color_u8!(255, 160, 210, 255),
    color_u8!(195, 70, 145, 255),
    color_u8!(255, 200, 230, 255),
];
const WHITES: Family = [
    color_u8!(245, 245, 240, 255),
    color_u8!(215, 215, 205, 255),
    color_u8!(175, 175, 170, 255),
    color_u8!(140, 140, 135, 255),
];

/// Enemy teams' families in order, for a viewer with allies: blue stays theirs.
const WITH_ALLIES: [Family; 7] = [REDS, YELLOWS, PURPLES, ORANGES, TEALS, PINKS, WHITES];
/// Enemy teams' families in order, for a viewer alone on a team, as in a
/// free-for-all, where each player is a team and wears its family's lead.
const WITHOUT_ALLIES: [Family; 8] = [REDS, BLUES, YELLOWS, PURPLES, ORANGES, TEALS, PINKS, WHITES];

/// The `rank`th seat's color within a family. Past its shades, a family
/// repeats them lightened, then darkened, then lightened further, so every
/// seat of a full match stays distinct.
fn family_color(family: &Family, rank: usize) -> Color {
    let base = family[rank % SHADES];
    let toward = |target: f32, amount: f32| {
        let mix = |channel: f32| channel + (target - channel) * amount;
        Color::new(mix(base.r), mix(base.g), mix(base.b), 1.0)
    };
    match rank / SHADES {
        0 => base,
        1 => toward(1.0, 0.3),
        2 => toward(0.0, 0.3),
        _ => toward(1.0, 0.55),
    }
}

#[cfg(test)]
mod tests;
