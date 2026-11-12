//! How a map's theme looks: the grade on its terrain and the props that
//! dress its ground. Grading is atmosphere, never information: units,
//! chrome and the minimap stay ungraded, and the washes stay faint enough
//! that seat colors still read against every theme.

use super::environment::Layer;
use aftermine_sim::scenario::Theme;
use macroquad::prelude::*;

/// Everything one theme changes on the ground.
struct Look {
    /// Multiplies the floor's sprites.
    tint: Color,
    /// Laid over the floor and mixed into walls and pits.
    wash: Color,
    /// The theme's row of ground props in the atlas.
    prop_row: usize,
    /// Seeds where the theme's props fall; changing it moves the dressing
    /// on every map of the theme.
    prop_salt: u32,
}

const fn look(theme: Theme) -> Look {
    match theme {
        Theme::RustedYard => Look {
            tint: Color::new(1.0, 0.95, 0.88, 1.0),
            wash: wash(110, 62, 36, 0.094),
            prop_row: 0,
            prop_salt: 1,
        },
        Theme::ColdCircuitry => Look {
            tint: Color::new(0.89, 0.95, 1.0, 1.0),
            wash: wash(34, 54, 72, 0.11),
            prop_row: 1,
            prop_salt: 2,
        },
        Theme::QuarryDust => Look {
            tint: Color::new(1.0, 0.97, 0.90, 1.0),
            wash: wash(118, 104, 82, 0.083),
            prop_row: 2,
            prop_salt: 25,
        },
        Theme::Basalt => Look {
            tint: Color::new(0.92, 0.91, 1.0, 1.0),
            wash: wash(10, 10, 20, 0.187),
            prop_row: 3,
            prop_salt: 4,
        },
        Theme::Slag => Look {
            tint: Color::new(1.0, 0.92, 0.92, 1.0),
            wash: wash(84, 30, 24, 0.11),
            prop_row: 4,
            prop_salt: 5,
        },
        Theme::Verdigris => Look {
            tint: Color::new(0.90, 1.0, 0.94, 1.0),
            wash: wash(22, 84, 70, 0.11),
            prop_row: 5,
            prop_salt: 6,
        },
    }
}

/// The subtle multiplier `theme` lays on ground sprites and map previews.
pub(crate) fn tint(theme: Option<Theme>) -> Color {
    theme.map_or(WHITE, |theme| look(theme).tint)
}

/// The atlas row of `theme`'s ground props.
pub(crate) fn prop_row(theme: Theme) -> usize {
    look(theme).prop_row
}

/// The salt that places `theme`'s ground props.
pub(crate) fn prop_salt(theme: Theme) -> u32 {
    look(theme).prop_salt
}

/// One theme's terrain color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Grade {
    /// Multiplies the floor's sprites.
    tint: Color,
    /// Laid over the floor and mixed into walls and pits; clear for no theme.
    wash: Color,
}

const fn wash(r: u8, g: u8, b: u8, alpha: f32) -> Color {
    Color::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, alpha)
}

impl Grade {
    pub(super) fn of(theme: Option<Theme>) -> Self {
        Self {
            tint: tint(theme),
            wash: theme.map_or(wash(0, 0, 0, 0.0), |theme| look(theme).wash),
        }
    }

    /// The multiplier for the floor's sprites.
    pub(super) fn tint(self) -> Color {
        self.tint
    }

    /// The color laid over the floor, if the theme has one.
    pub(super) fn floor_wash(self) -> Option<Color> {
        (self.wash.a > 0.0).then_some(self.wash)
    }

    /// `color`, on a wall or in a pit, pulled toward the theme.
    pub(super) fn wall(self, color: Color) -> Color {
        let Color { r, g, b, a } = self.wash;
        Color::new(
            color.r + (r - color.r) * a,
            color.g + (g - color.g) * a,
            color.b + (b - color.b) * a,
            color.a,
        )
    }

    /// A terrace layer's colors pulled toward the theme.
    pub(super) fn layer(self, layer: Layer) -> Layer {
        Layer {
            riser: self.wall(layer.riser),
            top: self.wall(layer.top),
            lip: self.wall(layer.lip),
            ..layer
        }
    }

    /// The multiplier for rocks and blockers, which leans toward the theme's
    /// hue. A multiplier can only darken, so it follows the hue and not the
    /// wash's own brightness.
    pub(super) fn rock(self) -> Color {
        let Color { r, g, b, a } = self.wash;
        let top = r.max(g).max(b);
        if top <= 0.0 {
            return WHITE;
        }
        let lean = (a * 1.4).min(1.0);
        Color::new(
            1.0 + (r / top - 1.0) * lean,
            1.0 + (g / top - 1.0) * lean,
            1.0 + (b / top - 1.0) * lean,
            1.0,
        )
    }
}

#[cfg(test)]
mod tests;
