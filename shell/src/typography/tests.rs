use super::*;

#[test]
fn clipping_counts_characters_not_bytes() {
    assert_eq!(clip_chars("short", 8), "short");
    assert_eq!(clip_chars("cupréous", 7), "cupr...");
}

#[test]
fn eliding_keeps_the_longest_prefix_that_fits() {
    let measure = |text: &str| text.chars().count() as f32 * 10.0;
    assert_eq!(
        elide_to_width("Long Causeway", 130.0, &measure),
        "Long Causeway"
    );
    assert_eq!(
        elide_to_width("Long Causeway", 100.0, &measure),
        "Long Ca..."
    );
    assert_eq!(
        elide_to_width("Long Causeway", 30.0, &measure),
        "",
        "below four characters nothing reads"
    );
}
