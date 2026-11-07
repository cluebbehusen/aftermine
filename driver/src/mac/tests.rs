use super::applescript_string;

#[test]
fn applescript_strings_escape_quotes_and_backslashes() {
    assert_eq!(
        applescript_string("/Users/a/aftermine/Aftermine.app"),
        "\"/Users/a/aftermine/Aftermine.app\""
    );
    assert_eq!(applescript_string(r#"a "b" \c"#), r#""a \"b\" \\c""#);
}
