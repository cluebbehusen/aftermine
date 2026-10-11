use super::*;

#[test]
fn a_snapshot_splits_into_parts_and_comes_back_whole() {
    let bytes: Vec<u8> = (0..(2 * PART_BYTES + 5))
        .map(|index| (index % 251).to_le_bytes()[0])
        .collect();
    let lines = parts(&bytes);
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line.len() <= 2 * PART_BYTES));
    let mut assembler = Assembler::new(bytes.len() as u64, 3).unwrap();
    assert_eq!(assembler.push(&lines[0]), Ok(None));
    assert_eq!(assembler.push(&lines[1]), Ok(None));
    assert_eq!(assembler.push(&lines[2]), Ok(Some(bytes)));
}

#[test]
fn announcements_that_do_not_add_up_are_refused() {
    assert!(Assembler::new(0, 0).is_none());
    assert!(Assembler::new(10, 2).is_none(), "too many parts");
    assert!(
        Assembler::new(PART_BYTES as u64 + 1, 1).is_none(),
        "too few"
    );
    assert!(
        Assembler::new(
            MAX_SNAPSHOT_BYTES + 1,
            u32::try_from(part_count(MAX_SNAPSHOT_BYTES + 1)).unwrap()
        )
        .is_none(),
        "too large"
    );
}

#[test]
fn parts_that_do_not_fit_the_announcement_are_refused() {
    let short = parts(&[1, 2, 3]);
    let mut extra = Assembler::new(3, 1).unwrap();
    assert_eq!(extra.push(&short[0]), Ok(Some(vec![1, 2, 3])));
    assert_eq!(
        extra.push(&short[0]),
        Err(BadPart),
        "no part after the last"
    );

    let mut long = Assembler::new(2, 1).unwrap();
    assert_eq!(long.push(&short[0]), Err(BadPart), "more than announced");

    let mut early = Assembler::new(PART_BYTES as u64 + 3, 2).unwrap();
    assert_eq!(
        early.push(&short[0]),
        Err(BadPart),
        "only the last part may be short"
    );

    for garbage in ["0", "zz", ""] {
        let mut assembler = Assembler::new(3, 1).unwrap();
        assert_eq!(assembler.push(garbage), Err(BadPart), "{garbage:?}");
    }
}
