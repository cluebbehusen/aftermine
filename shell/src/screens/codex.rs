//! The codex (the Roster): every unit and structure in the game, with its
//! sprite, its description, and its figures. One screen object over the shared menu
//! widget: the list is the cursor, the selected row is the page.
//! Windowless update; the coordinator holds the displaced screen and
//! restores it wholesale on leave, as for Settings.

use crate::assets::Sprites;
use crate::game::SoundKind;
use crate::menu::{Label, Line, Menu};
use crate::numeric;
use crate::render;
use crate::render::prim::stroke_rect;
use crate::theme::{
    BORDER_STRONG, CHIP, SURFACE_MENU, Stroke, TEXT_ACCENT, TEXT_BODY, TEXT_PRIMARY,
    TEXT_SECONDARY, Type,
};
use aftermine_kit::roster::Entry;
use aftermine_kit::roster::{Piece, Tone, page, sections};
use aftermine_protocol::RawEvent;
use macroquad::prelude::*;

/// What a codex frame decided.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    /// Still reading.
    Stay,
    /// Back to wherever the screen was opened from.
    Leave,
}

/// The codex screen: the list and what each row opens.
pub struct CodexScreen {
    /// The live list: section headers and one row per kind, each
    /// standing for its page.
    pub menu: Menu<Entry>,
    back: crate::button::BackButton,
}

impl CodexScreen {
    /// Opens on the first unit. Where leaving lands is the
    /// coordinator's business.
    pub fn open() -> Self {
        let mut lines = Vec::new();
        for (title, section) in sections() {
            lines.push(Line::Header(title.to_string()));
            for entry in section {
                lines.push(Line::Row(Label::Text(entry.name()), entry));
            }
        }
        let mut menu = Menu::new("ROSTER", lines);
        menu.heading = crate::menu::Heading::Page { band: 0.0 };
        menu.shift = crate::menu::BESIDE_PAGE_SHIFT;
        Self {
            menu,
            back: crate::button::BackButton::default(),
        }
    }

    /// The page under the cursor, if the cursor is on a kind.
    pub fn selected_entry(&self) -> Option<Entry> {
        self.menu.value().copied()
    }

    /// Applies a frame's events. Rows are pages, not verbs: moving the
    /// cursor turns the page, and only the BACK button or Escape acts.
    pub fn update(
        &mut self,
        events: &[RawEvent],
        mouse: &mut Vec2,
        sounds: &mut Vec<(SoundKind, Option<Vec2>)>,
    ) -> Out {
        let (back, events) = self.back.route(events);
        if back {
            sounds.push((SoundKind::Click, None));
            return Out::Leave;
        }
        if crate::nav::escaped(&events) {
            return Out::Leave;
        }
        if self.menu.handle(&events, mouse).is_some() {
            sounds.push((SoundKind::Click, None));
        }
        Out::Stay
    }

    /// Draws the list and the selected page — the caller draws the
    /// veil first, so both land above it.
    pub fn draw(&self, sprites: &Sprites) {
        self.menu.draw(sprites, "");
        let Some(entry) = self.selected_entry() else {
            return;
        };
        let s = render::ui_scale();
        // The page fits its own content within the column.
        let frame = PageFrame::at(s);
        let (x, w, top, bottom) = (
            frame.column.x,
            frame.column.w,
            frame.column.y,
            frame.column.bottom(),
        );
        let pad = frame.pad;
        let plate = frame.plate;
        let plate_x = x + pad;
        let plate_y = top + pad;

        let name = entry.name();
        let role = entry.header();

        // The box fits its page: never shorter than the plate row,
        // never past the hint line.
        let body_top = frame.body_top();
        let body = page_body(
            entry,
            frame.body_width(),
            frame.body_room(),
            s,
            &|text, size| measure_text(text, None, numeric::font_size(size), 1.0).width,
        );
        let box_bottom =
            (body_top + (body.lines.len() + 1) as f32 * body.line_h + pad * 0.5).min(bottom);
        draw_rectangle(x, top, w, box_bottom - top, SURFACE_MENU);
        stroke_rect(
            Rect::new(x, top, w, box_bottom - top),
            Stroke::Edge.at(s),
            BORDER_STRONG,
        );
        draw_rectangle(plate_x, plate_y, plate, plate, CHIP);
        let dest = Rect::new(plate_x, plate_y, plate, plate);
        match entry {
            Entry::Unit(kind) => {
                sprites.draw_portrait(dest, &[(sprites.unit(kind), WHITE)]);
            }
            Entry::Building(kind) => {
                let mut layers = vec![(sprites.building(kind), WHITE)];
                if let Some(mount) = sprites.defense_mount(kind, 0) {
                    layers.push((mount, WHITE));
                }
                sprites.draw_portrait(dest, &layers);
            }
        }
        let text_x = plate_x + plate + pad;
        draw_text(
            &name,
            text_x,
            plate_y + 30.0 * s,
            crate::theme::Type::Row.at(s),
            TEXT_PRIMARY,
        );
        draw_text(
            &role,
            text_x,
            plate_y + 54.0 * s,
            crate::theme::Type::Body.at(s),
            TEXT_ACCENT,
        );
        let mut y = body_top;
        for (line, color) in body.lines {
            y += body.line_h;
            draw_text(&line, x + pad, y, body.size, color);
        }
    }
}

/// Where a page sits: the column beside the list, its padding, and the
/// portrait plate's side.
struct PageFrame {
    column: Rect,
    pad: f32,
    plate: f32,
}

impl PageFrame {
    fn at(s: f32) -> Self {
        Self {
            column: crate::menu::page_column(s),
            pad: 14.0 * s,
            plate: 84.0 * s,
        }
    }

    /// The top of the text under the plate.
    fn body_top(&self) -> f32 {
        self.column.y + self.pad * 2.0 + self.plate
    }

    fn body_width(&self) -> f32 {
        self.column.w - self.pad * 2.0
    }

    /// The height the text may fill, its last baseline clear of the box.
    fn body_room(&self) -> f32 {
        self.column.bottom() - self.pad * 0.5 - self.body_top()
    }
}

/// A Roster page's text under its plate, set to fit.
struct PageBody {
    size: f32,
    /// The distance from one baseline to the next.
    line_h: f32,
    lines: Vec<(String, Color)>,
}

/// Sets `entry`'s page `width` wide in `room` of height, stepping down the
/// type scale until every line fits. A page too long even for the smallest
/// size ends its last line in an ellipsis, so nothing goes missing unseen.
fn page_body(
    entry: Entry,
    width: f32,
    room: f32,
    s: f32,
    measure: &dyn Fn(&str, f32) -> f32,
) -> PageBody {
    let pieces = page(entry);
    let set = |size: f32| -> Vec<(String, Color)> {
        let mut lines = Vec::new();
        for piece in &pieces {
            match piece {
                Piece::Paragraph(text, tone) => {
                    for line in render::wrap_words(text, |t| measure(t, size), width) {
                        lines.push((line, tone_color(*tone)));
                    }
                }
                Piece::Line(line, tone) => lines.push((line.clone(), tone_color(*tone))),
            }
        }
        lines
    };
    let sizes = [Type::Body, Type::Small, Type::Caption];
    let mut body = PageBody {
        size: 0.0,
        line_h: 0.0,
        lines: Vec::new(),
    };
    for step in sizes {
        let size = step.at(s);
        body = PageBody {
            size,
            line_h: size * LINE_SPACING,
            lines: set(size),
        };
        if body.lines.len() as f32 * body.line_h <= room {
            return body;
        }
    }
    let fits = numeric::to_usize((room / body.line_h).floor()).max(1);
    body.lines.truncate(fits);
    if let Some((last, _)) = body.lines.last_mut() {
        *last = crate::typography::elide_to_width(&format!("{last}..."), width, &|text| {
            measure(text, body.size)
        });
    }
    body
}

/// The color a page piece of `tone` is drawn in.
fn tone_color(tone: Tone) -> Color {
    match tone {
        Tone::Body => TEXT_BODY,
        Tone::Primary => TEXT_PRIMARY,
        Tone::Secondary => TEXT_SECONDARY,
    }
}

/// A page's baseline-to-baseline distance, as a multiple of its type size.
const LINE_SPACING: f32 = 1.25;

#[cfg(test)]
mod tests;
