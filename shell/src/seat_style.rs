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
        Self::from_cues(
            (0..state.players().len())
                .map(|seat| AllegianceCue::of(state, viewer, PlayerId::from_index(seat))),
            team_colors,
        )
    }

    /// Styles for seats in seat order whose relationship to the viewer is
    /// already known. Allies and hostiles each rank by seat order within
    /// their own family, unless `team_colors` gives each family one color.
    pub fn from_cues(cues: impl IntoIterator<Item = AllegianceCue>, team_colors: bool) -> Self {
        let cues: Vec<_> = cues.into_iter().collect();
        let enemies = hostile_palette(cues.contains(&AllegianceCue::Ally));
        let mut allies = 0;
        let mut hostiles = 0;
        let mut styles = [WHITE; MAX_PLAYERS];
        for (style, cue) in styles.iter_mut().zip(cues) {
            *style = match cue {
                AllegianceCue::Mine => self_color(team_colors),
                AllegianceCue::Ally if team_colors => TEAM_ALLY,
                AllegianceCue::Hostile if team_colors => TEAM_ENEMY,
                AllegianceCue::Ally => {
                    allies += 1;
                    family_color(&ally_palette(), allies - 1)
                }
                AllegianceCue::Hostile => {
                    hostiles += 1;
                    family_color(&enemies, hostiles - 1)
                }
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
/// Team colors' enemies, brighter than the default palette's first enemy so
/// that they hold apart from the near-white self color and the blue allies.
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

/// Light blues. Red against green collapses under red-weak and green-weak
/// vision, so allies part from enemies by being lighter and bluer.
fn ally_palette() -> [Color; 8] {
    [
        color_u8!(110, 190, 250, 255),
        color_u8!(195, 230, 255, 255),
        color_u8!(165, 140, 250, 255),
        color_u8!(40, 150, 240, 255),
        color_u8!(170, 210, 255, 255),
        color_u8!(150, 170, 255, 255),
        color_u8!(70, 190, 245, 255),
        color_u8!(135, 205, 250, 255),
    ]
}

/// Enemies by rank. A viewer with an ally sees only dark colors, below the
/// lightness of every ally and of the self green; bright oranges and yellows
/// sit on the green under red-weak vision. With no ally to confuse, later ranks
/// reach into light and cool colors.
fn hostile_palette(has_allies: bool) -> [Color; 8] {
    if has_allies {
        [
            color_u8!(190, 30, 50, 255),
            color_u8!(150, 120, 30, 255),
            color_u8!(120, 60, 210, 255),
            color_u8!(130, 40, 110, 255),
            color_u8!(170, 60, 30, 255),
            color_u8!(140, 90, 40, 255),
            color_u8!(160, 40, 90, 255),
            color_u8!(130, 30, 50, 255),
        ]
    } else {
        [
            color_u8!(190, 30, 50, 255),
            color_u8!(245, 245, 240, 255),
            color_u8!(60, 90, 245, 255),
            color_u8!(190, 175, 255, 255),
            color_u8!(30, 135, 180, 255),
            color_u8!(50, 70, 175, 255),
            color_u8!(130, 40, 110, 255),
            color_u8!(195, 230, 255, 255),
        ]
    }
}

/// The `rank`th seat's color within a family; a family larger than its
/// palette reuses it lightened.
fn family_color(palette: &[Color; 8], rank: usize) -> Color {
    let base = palette[rank % palette.len()];
    if rank < palette.len() {
        base
    } else {
        Color::new(
            base.r * 0.7 + 0.3,
            base.g * 0.7 + 0.3,
            base.b * 0.7 + 0.3,
            1.0,
        )
    }
}

#[cfg(test)]
mod tests;
