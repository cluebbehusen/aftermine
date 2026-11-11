use super::*;

impl HintLine {
    /// The line as copy, each icon spelled as the name it shows.
    pub(crate) fn copy(&self) -> String {
        self.segments
            .iter()
            .map(|segment| match segment {
                Segment::Text(text) => text.as_str(),
                Segment::Pad(icon) => icon.name(),
            })
            .collect()
    }
}

/// Every character a tenth of the size wide, so widths are easy to read.
fn tenth(copy: &str, size: f32) -> f32 {
    copy.chars().count() as f32 * size * 0.1
}

fn footer() -> HintLine {
    HintLine::default()
        .pad(PadIcon::DpadUpDown)
        .text(" select - ")
        .pad(PadIcon::Cross)
        .text(" confirm")
}

#[test]
fn a_line_reads_as_copy_with_each_icon_named() {
    assert_eq!(footer().copy(), "D-pad select - Cross confirm");
    crate::platform::tests::assert_keyless_copy(&footer().copy());
}

#[test]
fn icons_take_room_beside_the_words() {
    let size = 20.0;
    let words = tenth(" select - ", size) + tenth(" confirm", size);
    let width = footer().width_with(size, tenth);
    assert!(
        (width - words - 2.0 * size * ICON_SCALE).abs() < 1e-3,
        "{width}"
    );
}

#[test]
fn a_long_line_shrinks_to_fit_but_not_past_its_floor() {
    let line = footer();
    let wide = line.width_with(18.0, tenth);
    assert_eq!(
        line.fit_with(18.0, wide, 11.0, tenth),
        18.0,
        "it already fits"
    );
    let shrunk = line.fit_with(18.0, wide * 0.8, 11.0, tenth);
    assert!((line.width_with(shrunk, tenth) - wide * 0.8).abs() < 1e-3);
    assert_eq!(
        line.fit_with(18.0, 1.0, 11.0, tenth),
        11.0,
        "the floor holds"
    );
}

#[test]
fn only_opening_copy_is_sentence_cased() {
    assert_eq!(
        HintLine::from("click to choose".to_string()).shown().copy(),
        "Click to choose"
    );
    assert_eq!(*footer().shown(), footer(), "an icon opens the line");
}

#[test]
fn a_hint_from_a_lowercase_template_is_drawn_sentence_cased() {
    let line = crate::menu::hint_line("tap a map to select it - tap it again to play");
    assert_eq!(
        line.shown().copy(),
        "Tap a map to select it - tap it again to play"
    );
    let tenth_of_capitals = |copy: &str, size: f32| {
        copy.chars().filter(char::is_ascii_uppercase).count() as f32 * size * 0.1
    };
    assert_eq!(
        line.width_with(10.0, tenth_of_capitals),
        1.0,
        "it is measured as drawn"
    );
    assert_eq!(
        line.fit_with(10.0, 0.5, 1.0, tenth_of_capitals),
        5.0,
        "and shrinks to fit as drawn"
    );
}

#[test]
fn a_button_is_an_icon_on_a_full_pad_and_a_word_on_the_remote() {
    use aftermine_protocol::{PadButton, PadFamily};
    assert_eq!(
        HintLine::default().button(PadFamily::PlayStation, PadButton::South),
        HintLine::default().pad(PadIcon::Cross)
    );
    assert_eq!(
        HintLine::default().button(PadFamily::Remote, PadButton::South),
        HintLine::default().text("Click")
    );
}
