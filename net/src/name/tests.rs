use super::*;

#[test]
fn a_name_is_trimmed_printable_ascii_within_the_cap() {
    assert_eq!(
        PlayerName::new("  Connor L. ").unwrap().as_str(),
        "Connor L."
    );
    assert_eq!(
        PlayerName::new(&"x".repeat(PlayerName::MAX_CHARS))
            .unwrap()
            .as_str()
            .len(),
        PlayerName::MAX_CHARS
    );
    assert_eq!(PlayerName::new("   "), Err(NameError::Empty));
    assert_eq!(
        PlayerName::new(&"x".repeat(PlayerName::MAX_CHARS + 1)),
        Err(NameError::TooLong)
    );
    for refused in ["tab\there", "caf\u{e9}", "line\nbreak"] {
        assert_eq!(PlayerName::new(refused), Err(NameError::Character));
    }
}

#[test]
fn the_wire_form_is_a_plain_string_checked_on_decode() {
    let name = PlayerName::new("Alex").unwrap();
    assert_eq!(serde_json::to_string(&name).unwrap(), r#""Alex""#);
    assert_eq!(
        serde_json::from_str::<PlayerName>(r#"" Alex ""#).unwrap(),
        name
    );
    for refused in [r#""""#, r#""\u0007""#, r#""seventeen chars!!""#] {
        assert!(serde_json::from_str::<PlayerName>(refused).is_err());
    }
}

#[test]
fn every_refusal_reads_as_a_sentence() {
    for error in [NameError::Empty, NameError::TooLong, NameError::Character] {
        let text = error.to_string();
        assert!(text.starts_with(char::is_uppercase), "{text}");
    }
}
