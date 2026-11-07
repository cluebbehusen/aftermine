use super::*;

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
    crate::platform::assert_keyless_copy(&footer().copy());
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
        HintLine::from("click to choose".to_string())
            .sentence_cased()
            .copy(),
        "Click to choose"
    );
    assert_eq!(
        footer().sentence_cased(),
        footer(),
        "an icon opens the line"
    );
}
