//! How a map's theme colors its terrain. Grading is atmosphere, never
//! information: units, chrome and the minimap stay ungraded, and the washes
//! stay faint enough that seat colors still read against every theme.

use super::environment::Layer;
use macroquad::prelude::*;

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
    pub(super) fn of(theme: &str) -> Self {
        let wash = match theme {
            "rusted-yard" => wash(110, 62, 36, 0.094),
            "cold-circuitry" => wash(34, 54, 72, 0.11),
            "quarry-dust" => wash(118, 104, 82, 0.083),
            "basalt" => wash(10, 10, 20, 0.187),
            "slag" => wash(84, 30, 24, 0.11),
            "verdigris" => wash(22, 84, 70, 0.11),
            _ => wash(0, 0, 0, 0.0),
        };
        Self {
            tint: super::theme_tint(theme),
            wash,
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
