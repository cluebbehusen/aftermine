use super::*;

#[test]
fn frames_round_trip_alone_and_against_a_dictionary() {
    let previous: Vec<u8> = (0..4_000u32).flat_map(u32::to_le_bytes).collect();
    let mut next = previous.clone();
    next[1_234] ^= 0xff;
    let alone = compress(&next, None, 3).unwrap();
    let delta = compress(&next, Some(&previous), 3).unwrap();
    assert!(
        delta.len() < alone.len(),
        "the dictionary carries the shared bytes"
    );
    let len = next.len() as u64;
    assert_eq!(decompress(&alone, len, len, None).unwrap(), next);
    assert_eq!(decompress(&delta, len, len, Some(&previous)).unwrap(), next);
}

#[test]
fn untrusted_frames_are_refused() {
    let raw = vec![7u8; 10_000];
    let len = raw.len() as u64;
    let frame = compress(&raw, None, 3).unwrap();
    assert!(
        decompress(&frame, len - 1, len, None).is_err(),
        "short claim"
    );
    assert!(
        decompress(&frame, len + 1, len + 1, None).is_err(),
        "long claim"
    );
    assert!(
        decompress(&frame, len, len - 1, None).is_err(),
        "over its bound"
    );
    let mut trailing = frame.clone();
    trailing.push(0);
    assert!(decompress(&trailing, len, len, None).is_err());
    let mut corrupt = frame.clone();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    assert!(decompress(&corrupt, len, len, None).is_err(), "checksum");
    let unchecked = zstd::bulk::compress(&raw, 3).unwrap();
    assert!(decompress(&unchecked, len, len, None).is_err());
    assert!(decompress(&[], 0, 0, None).is_err());
}
