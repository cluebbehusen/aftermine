use super::*;

const RATE: u32 = 48_000;

fn limiter() -> Limiter {
    Limiter::new(
        LimiterBuilder {
            ceiling: 0.5,
            lookahead: Duration::from_millis(5),
            release: Duration::from_millis(50),
        },
        RATE,
    )
}

fn compressor() -> Compressor {
    Compressor::new(
        CompressorBuilder {
            threshold_db: -20.0,
            ratio: 4.0,
            knee_db: 0.0,
            attack: Duration::from_millis(1),
            release: Duration::from_millis(20),
        },
        RATE,
    )
}

/// A deterministic, loud, spiky signal: a sine with isolated hits far over
/// the ceiling, some a single frame long.
fn spiky(frames: usize) -> Vec<Frame> {
    (0..frames)
        .map(|index| {
            let t = index as f32 / RATE as f32;
            let tone = 0.4 * (std::f32::consts::TAU * 220.0 * t).sin();
            let hit = match index % 4_000 {
                0 => 6.0,
                1..=40 => 2.0,
                _ => 0.0,
            };
            Frame::new(tone + hit, tone - hit * 0.5)
        })
        .collect()
}

#[test]
fn the_limiter_never_lets_a_frame_past_its_ceiling() {
    let mut limiter = limiter();
    let mut frames = spiky(RATE as usize);
    for block in frames.chunks_mut(128) {
        limiter.process_frames(block);
    }
    let loudest = frames.iter().map(|frame| level(*frame)).fold(0.0, f32::max);
    assert!(loudest <= 0.5 + 1.0e-6, "{loudest}");
    assert!(
        loudest > 0.45,
        "the hits still reach the ceiling: {loudest}"
    );
}

#[test]
fn the_limiter_passes_quiet_audio_unchanged_after_its_lookahead() {
    let mut limiter = limiter();
    let input: Vec<Frame> = (0..2_000)
        .map(|index| Frame::new(0.3 * (index as f32 * 0.01).sin(), -0.2))
        .collect();
    let mut output = input.clone();
    limiter.process_frames(&mut output);
    let delay = limiter.length;
    assert_eq!(delay, 240);
    assert!(output[..delay].iter().all(|frame| *frame == Frame::ZERO));
    assert_eq!(output[delay..], input[..input.len() - delay]);
}

#[test]
fn the_limiter_ramps_down_before_a_peak_instead_of_clipping_it() {
    let mut limiter = limiter();
    let mut frames = vec![Frame::from_mono(0.4); 1_000];
    frames[600] = Frame::from_mono(4.0);
    limiter.process_frames(&mut frames);
    let peak_out = 600 + limiter.length;
    assert!((level(frames[peak_out]) - 0.5).abs() < 1.0e-5);
    let gains: Vec<f32> = (peak_out - limiter.length..peak_out)
        .map(|index| frames[index].left / 0.4)
        .collect();
    assert!(gains.windows(2).all(|pair| pair[1] <= pair[0] + 1.0e-6));
    assert!(gains[0] > 0.99, "the ramp starts from full gain");
}

#[test]
fn the_compressor_settles_at_its_ratio_above_threshold() {
    let mut compressor = compressor();
    // A square wave holds its peak, so the detector sees a steady level.
    let input = 10.0_f32.powf(-10.0 / 20.0);
    let mut frames: Vec<Frame> = (0..RATE)
        .map(|index| Frame::from_mono(if index % 100 < 50 { input } else { -input }))
        .collect();
    compressor.process_frames(&mut frames);
    let settled = 20.0 * level(*frames.last().unwrap()).log10();
    // 10 dB over a -20 dB threshold at 4:1 leaves 2.5 dB over.
    assert!((settled - -17.5).abs() < 0.05, "{settled}");
}

#[test]
fn the_compressor_leaves_audio_under_its_threshold_alone() {
    let mut compressor = compressor();
    let mut frames = vec![Frame::new(0.05, -0.05); 4_000];
    compressor.process_frames(&mut frames);
    assert!(frames.iter().all(|frame| *frame == Frame::new(0.05, -0.05)));
}

#[test]
fn both_processors_turn_the_two_channels_down_together() {
    let mut frames = vec![Frame::new(0.9, 0.09); 4_000];
    compressor().process_frames(&mut frames);
    let last = *frames.last().unwrap();
    assert!(last.left < 0.9);
    assert!((last.left / last.right - 10.0).abs() < 1.0e-3);

    let mut frames = vec![Frame::new(0.9, 0.09); 4_000];
    limiter().process_frames(&mut frames);
    let last = *frames.last().unwrap();
    assert!((last.left - 0.5).abs() < 1.0e-5);
    assert!((last.left / last.right - 10.0).abs() < 1.0e-3);
}
