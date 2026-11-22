use super::*;

fn voice(remoteness: f32) -> Voice {
    Voice {
        gain: 0.7,
        pan: 0.25,
        remoteness,
    }
}

fn motor(key: u32) -> Motor<u32> {
    Motor {
        key,
        voice: voice(0.0),
    }
}

#[test]
fn overlapping_loops_stop_independently_and_do_not_retrigger_each_frame() {
    let mut loops = LoopBank::<u32, 16>::default();
    let (a, b) = (motor(1), motor(2));
    let v = voice(0.0);
    assert_eq!(
        loops.reconcile(&[a, b]),
        vec![Change::Start(0, v), Change::Start(1, v)]
    );
    assert_eq!(
        loops.reconcile(&[b]),
        vec![Change::Stop(0), Change::Steer(1, v)]
    );
    assert_eq!(loops.reconcile(&[b]), vec![Change::Steer(1, v)]);
    assert_eq!(loops.reconcile(&[]), vec![Change::Stop(1)]);
    assert!(loops.reconcile(&[]).is_empty());
}

#[test]
fn pause_or_seek_clears_loops_and_resuming_reconstructs_only_live_ones() {
    let mut loops = LoopBank::<u32, 16>::default();
    loops.reconcile(&[motor(1)]);
    assert_eq!(loops.reconcile(&[]), vec![Change::Stop(0)]);
    assert_eq!(
        loops.reconcile(&[motor(2)]),
        vec![Change::Start(0, voice(0.0))]
    );
}

#[test]
fn a_loop_keeps_its_starting_band_but_follows_the_camera_in_level() {
    let near = distance_level(0.0);
    assert!((steered_gain(voice(0.0), near) - 0.7).abs() < 1e-6);
    let far = steered_gain(voice(1.0), near);
    assert!(
        (far - 0.7 * distance_level(1.0) / near).abs() < 1e-6,
        "moving away turns it down by the farther band's level: {far}"
    );
    assert!(far < 0.7);
    let started_far = distance_level(1.0);
    assert!(
        (steered_gain(voice(1.0), started_far) - 0.7).abs() < 1e-6,
        "a loop already on the far band takes no second cut"
    );
}
