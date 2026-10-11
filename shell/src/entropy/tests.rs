use super::*;

#[test]
fn draws_never_repeat() {
    let draws: Vec<u128> = (0..64).map(|_| bits()).collect();
    for (index, draw) in draws.iter().enumerate() {
        assert!(!draws[index + 1..].contains(draw), "draw {index} repeated");
        assert_ne!(draw >> 64, 0, "both halves are filled");
    }
    assert_ne!(token(), token());
}
