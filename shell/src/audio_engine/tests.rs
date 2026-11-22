use super::*;

fn wav(channels: u16, bits: u16, samples: &[i16]) -> Vec<u8> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend(u32::try_from(36 + data.len()).unwrap().to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16u32.to_le_bytes());
    bytes.extend(1u16.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(22_050u32.to_le_bytes());
    bytes.extend((22_050 * u32::from(channels) * 2).to_le_bytes());
    bytes.extend((channels * 2).to_le_bytes());
    bytes.extend(bits.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(u32::try_from(data.len()).unwrap().to_le_bytes());
    bytes.extend(data);
    bytes
}

#[test]
fn mono_samples_play_in_both_ears() {
    let (rate, frames) = decode_wav(&wav(1, 16, &[16_384, -32_768])).unwrap();
    assert_eq!(rate, 22_050);
    assert_eq!(frames, [Frame::from_mono(0.5), Frame::from_mono(-1.0)]);
}

#[test]
fn stereo_samples_keep_their_sides() {
    let (_, frames) = decode_wav(&wav(2, 16, &[16_384, -16_384])).unwrap();
    assert_eq!(frames, [Frame::new(0.5, -0.5)]);
}

#[test]
fn other_encodings_and_truncated_files_are_refused() {
    assert!(decode_wav(&wav(1, 8, &[0])).is_err());
    assert!(decode_wav(&wav(3, 16, &[0, 0, 0])).is_err());
    let whole = wav(1, 16, &[1, 2, 3]);
    assert!(decode_wav(&whole[..whole.len() - 1]).is_err());
    assert!(decode_wav(b"RIFF").is_err());
}

#[test]
fn every_shipped_clip_decodes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/sounds");
    let mut count = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "wav") {
            let (rate, frames) = decode_wav(&std::fs::read(&path).unwrap()).unwrap();
            assert!(matches!(rate, 22_050 | 44_100), "{}", path.display());
            assert!(!frames.is_empty(), "{}", path.display());
            count += 1;
        }
    }
    assert!(count > 0);
}

#[test]
fn gain_maps_to_decibels_and_silence() {
    assert_eq!(decibels(1.0), Decibels::IDENTITY);
    assert!((decibels(0.1).0 + 20.0).abs() < 1e-4);
    assert_eq!(decibels(0.0), Decibels::SILENCE);
    assert_eq!(decibels(-1.0), Decibels::SILENCE);
    assert_eq!(decibels(f32::NAN), Decibels::SILENCE);
    assert_eq!(decibels(1e-9), Decibels::SILENCE);
}
