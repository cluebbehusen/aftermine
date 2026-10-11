use super::*;
use std::time::Duration;

#[test]
fn the_preview_worker_draws_a_scenario_off_the_calling_thread() {
    let worker = PreviewWorker::spawn();
    let skirmish = PreviewKey::Path(None);
    worker
        .requests
        .send((skirmish.clone(), PreviewSource::Path(None)))
        .unwrap();
    let (key, pixels) = worker
        .finished
        .recv_timeout(Duration::from_secs(30))
        .expect("the worker answers");
    assert_eq!(key, skirmish, "the answer names the scenario it drew");
    let pixels = pixels.expect("the embedded skirmish draws");
    assert_eq!(
        pixels.rgba.len(),
        usize::from(pixels.width) * usize::from(pixels.height) * 4
    );
}

#[test]
fn a_map_held_in_memory_draws_like_one_on_disk() {
    let worker = PreviewWorker::spawn();
    let key = PreviewKey::Map(7);
    worker
        .requests
        .send((
            key.clone(),
            PreviewSource::Map(Box::new(Scenario::skirmish())),
        ))
        .unwrap();
    let (answered, pixels) = worker
        .finished
        .recv_timeout(Duration::from_secs(30))
        .expect("the worker answers");
    assert_eq!(answered, key);
    assert!(pixels.is_some());
}

#[test]
fn an_unreadable_scenario_answers_with_no_preview() {
    let worker = PreviewWorker::spawn();
    let missing = PathBuf::from("/nonexistent/oxide-preview.json");
    let key = PreviewKey::Path(Some(missing.clone()));
    worker
        .requests
        .send((key.clone(), PreviewSource::Path(Some(missing))))
        .unwrap();
    let (answered, pixels) = worker
        .finished
        .recv_timeout(Duration::from_secs(30))
        .expect("the worker answers");
    assert_eq!(answered, key);
    assert!(pixels.is_none());
}
