//! The report shown once, before the first Home: a few lines on black, then a
//! cut to Home with its match already running behind the title.

use crate::theme;
use macroquad::prelude::{clear_background, draw_text};

const HEADER: &str = "SITE 14";
const HEADER_AT: f32 = 0.6;

/// Label, value and the second the row appears. The long wait before the last
/// row gives it room to land.
const ROWS: [(&str, &str, f32); 4] = [
    ("ORE", "EXHAUSTED", 1.5),
    ("HEAD OFFICE", "NO RESPONSE, 61 YEARS", 2.4),
    ("STANDING ORDER", "RECOVER SCRAP", 3.5),
    ("NEAREST SCRAP", "SITE 15", 5.2),
];

/// Seconds until the cut to Home.
const END: f32 = 7.6;

/// The report's clock.
#[derive(Debug, Default)]
pub struct ColdOpenScreen {
    age: f32,
}

impl ColdOpenScreen {
    /// Advances the report, returning whether it is over. Any press skips it.
    pub fn update(&mut self, pressed: bool, dt: f32) -> bool {
        self.age += dt;
        pressed || self.age >= END
    }

    /// Draws the lines shown so far, laid out from the whole report so nothing
    /// shifts as they appear.
    pub fn draw(&self) {
        clear_background(theme::PAGE);
        let ui = crate::render::ui_scale();
        let view = crate::render::viewport();
        let size = theme::Type::Heading.at(ui);
        let measure = |text: &str| crate::typography::measure(text, size).width;
        let label_width = ROWS.iter().map(|row| measure(row.0)).fold(0.0, f32::max);
        let value_width = ROWS.iter().map(|row| measure(row.1)).fold(0.0, f32::max);
        let gap = size * 1.5;
        let line = size * 1.7;
        let left = ((view.x - label_width - gap - value_width) / 2.0).max(0.0);
        let mut y = (view.y - line * ROWS.len() as f32) / 2.0;
        if self.age >= HEADER_AT {
            draw_text(HEADER, left, y, size, theme::TEXT_TITLE);
        }
        for (label, value, at) in ROWS {
            y += line;
            if self.age >= at {
                draw_text(label, left, y, size, theme::TEXT_SECONDARY);
                draw_text(
                    value,
                    left + label_width + gap,
                    y,
                    size,
                    theme::TEXT_PRIMARY,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests;
