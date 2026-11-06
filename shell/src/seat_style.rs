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
        let mut handed_out: Vec<Color> = Vec::new();
        let mut styles = [WHITE; MAX_PLAYERS];
        for (seat, (style, team)) in styles.iter_mut().zip(teams).enumerate() {
            *style = if seat == mine {
                self_color(team_colors)
            } else if Some(*team) == my_team {
                if team_colors {
                    TEAM_ALLY
                } else {
                    allies += 1;
                    family_color(&ALLY_FAMILY, allies - 1)
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
                let family = &families[index % families.len()];
                // A family can come round to a second team, and a big team
                // overflows into a family another team may wear, so a seat takes
                // its family's next shade that no other enemy wears yet.
                let color = (*members..*members + 2 * FAMILY)
                    .map(|rank| family_color(family, rank))
                    .find(|color| !handed_out.contains(color))
                    .unwrap_or_else(|| family_color(family, *members));
                *members += 1;
                handed_out.push(color);
                color
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

/// Hand-picked shades in a family: a lead color, then lighter, darker and
/// further shades of the same hue, so a team reads as one and its seats still
/// tell apart. A team of two wears the lead and the lighter shade.
type Shades = [Color; 4];

/// Colors in a family: its own shades, then a neighboring family's for a team
/// too big for four.
const FAMILY: usize = 8;
type Family = [Color; FAMILY];

const fn family(own: Shades, overflow: Shades) -> Family {
    let mut family = [own[0]; FAMILY];
    let mut shade = 0;
    while shade < own.len() {
        family[shade] = own[shade];
        family[shade + own.len()] = overflow[shade];
        shade += 1;
    }
    family
}

/// The viewer's allies: blues, which no enemy team wears beside an ally.
const ALLIES: Shades = [
    color_u8!(110, 190, 250, 255),
    color_u8!(195, 230, 255, 255),
    color_u8!(40, 150, 240, 255),
    color_u8!(60, 100, 245, 255),
];

// Light reds read as the self green under red-weak vision, so the reds stay
// mid to dark.
const REDS: Shades = [
    color_u8!(228, 44, 58, 255),
    color_u8!(235, 80, 110, 255),
    color_u8!(165, 25, 40, 255),
    color_u8!(140, 25, 65, 255),
];
const BLUES: Shades = [
    color_u8!(60, 100, 245, 255),
    color_u8!(110, 150, 255, 255),
    color_u8!(40, 70, 190, 255),
    color_u8!(170, 195, 255, 255),
];
const YELLOWS: Shades = [
    color_u8!(250, 210, 60, 255),
    color_u8!(255, 232, 120, 255),
    color_u8!(200, 150, 25, 255),
    color_u8!(255, 245, 175, 255),
];
// Purples read as blue under red-weak vision, so they stay mid to dark, below
// the ally blues.
const PURPLES: Shades = [
    color_u8!(150, 70, 220, 255),
    color_u8!(175, 60, 195, 255),
    color_u8!(105, 40, 170, 255),
    color_u8!(125, 45, 135, 255),
];
const ORANGES: Shades = [
    color_u8!(245, 130, 40, 255),
    color_u8!(255, 170, 90, 255),
    color_u8!(190, 90, 25, 255),
    color_u8!(255, 205, 150, 255),
];
const TEALS: Shades = [
    color_u8!(40, 180, 190, 255),
    color_u8!(110, 215, 220, 255),
    color_u8!(25, 130, 140, 255),
    color_u8!(170, 235, 235, 255),
];
const PINKS: Shades = [
    color_u8!(245, 120, 190, 255),
    color_u8!(255, 160, 210, 255),
    color_u8!(195, 70, 145, 255),
    color_u8!(255, 200, 230, 255),
];
const WHITES: Shades = [
    color_u8!(245, 245, 240, 255),
    color_u8!(215, 215, 205, 255),
    color_u8!(175, 175, 170, 255),
    color_u8!(140, 140, 135, 255),
];

const ALLY_FAMILY: Family = family(
    ALLIES,
    [BLUES[1], BLUES[2], BLUES[3], color_u8!(100, 170, 235, 255)],
);

/// Enemy teams' families in order, for a viewer with allies. Blue stays the
/// allies', and teal, between the ally blues and the self green, sits out.
const WITH_ALLIES: [Family; 6] = [
    family(REDS, PINKS),
    family(YELLOWS, ORANGES),
    family(PURPLES, PINKS),
    family(ORANGES, YELLOWS),
    family(PINKS, PURPLES),
    family(WHITES, YELLOWS),
];
/// Enemy teams' families in order, for a viewer alone on a team, as in a
/// free-for-all, where each player is a team and wears its family's lead.
const WITHOUT_ALLIES: [Family; 8] = [
    family(REDS, PINKS),
    family(BLUES, PURPLES),
    family(YELLOWS, ORANGES),
    family(PURPLES, PINKS),
    family(ORANGES, YELLOWS),
    family(TEALS, BLUES),
    family(PINKS, PURPLES),
    family(WHITES, YELLOWS),
];

/// The `rank`th seat's color within a family. Past its colors, a family
/// repeats them darkened, which only a team of more than eight reaches.
fn family_color(family: &Family, rank: usize) -> Color {
    let base = family[rank % FAMILY];
    if rank < FAMILY {
        base
    } else {
        Color::new(base.r * 0.65, base.g * 0.65, base.b * 0.65, 1.0)
    }
}

#[cfg(test)]
mod tests;
