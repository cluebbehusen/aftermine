use super::*;

#[test]
fn a_token_travels_as_32_hex_digits_and_reads_back() {
    let token = Token::new(0x0123_4567_89ab_cdef_0011_2233_4455_6677);
    assert_eq!(token.to_hex(), "0123456789abcdef0011223344556677");
    assert_eq!(Token::from_hex(&token.to_hex()), Some(token));
    let json = serde_json::to_string(&token).unwrap();
    assert_eq!(json, r#""0123456789abcdef0011223344556677""#);
    assert_eq!(serde_json::from_str::<Token>(&json).unwrap(), token);
    assert_eq!(
        Token::from_hex("0123456789ABCDEF0011223344556677"),
        Some(token)
    );
}

#[test]
fn anything_but_32_hex_digits_is_refused() {
    for text in [
        "",
        "abc",
        "0123456789abcdef001122334455667",
        "g123456789abcdef0011223344556677",
    ] {
        assert_eq!(Token::from_hex(text), None, "{text:?}");
        assert!(serde_json::from_str::<Token>(&format!("{text:?}")).is_err());
    }
}

#[test]
fn a_token_never_prints() {
    assert_eq!(format!("{:?}", Token::new(7)), "Token(..)");
}
