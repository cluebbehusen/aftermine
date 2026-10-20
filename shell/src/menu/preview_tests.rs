use super::*;
use std::time::Duration;

/// The map list's thumbnail box on an iPad.
const IPAD_THUMB: PixelBox = (523, 245);

#[test]
fn the_preview_worker_draws_a_scenario_off_the_calling_thread() {
    let worker = PreviewWorker::spawn();
    let skirmish = PreviewKey::Path(None);
    worker
        .requests
        .send((skirmish.clone(), IPAD_THUMB, PreviewSource::Path(None)))
        .unwrap();
    let (key, size, pixels) = worker
        .finished
        .recv_timeout(Duration::from_secs(30))
        .expect("the worker answers");
    assert_eq!(key, skirmish, "the answer names the scenario it drew");
    assert_eq!(size, IPAD_THUMB, "and the box it drew for");
    let pixels = pixels.expect("the embedded skirmish draws");
    assert_eq!(
        pixels.rgba.len(),
        usize::from(pixels.width) * usize::from(pixels.height) * 4
    );
    let (width, height) = (u32::from(pixels.width), u32::from(pixels.height));
    assert!(
        width <= IPAD_THUMB.0 && height <= IPAD_THUMB.1,
        "{width}x{height} fits the box"
    );
    assert!(
        width == IPAD_THUMB.0 || height == IPAD_THUMB.1,
        "{width}x{height} spans one side of the box, so it draws pixel for pixel"
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
            IPAD_THUMB,
            PreviewSource::Map(Box::new(Scenario::skirmish())),
        ))
        .unwrap();
    let (answered, _, pixels) = worker
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
        .send((key.clone(), IPAD_THUMB, PreviewSource::Path(Some(missing))))
        .unwrap();
    let (answered, _, pixels) = worker
        .finished
        .recv_timeout(Duration::from_secs(30))
        .expect("the worker answers");
    assert_eq!(answered, key);
    assert!(pixels.is_none());
}

#[test]
fn a_slot_asks_once_and_shows_the_preview_once_drawn() {
    let mut slot = PreviewSlot::default();
    assert!(slot.want(IPAD_THUMB), "first sight asks the worker");
    assert!(
        !slot.want(IPAD_THUMB),
        "asking again waits for that drawing"
    );
    assert!(slot.shown().is_none());
    slot.finish(IPAD_THUMB, Some("ipad"));
    assert!(!slot.want(IPAD_THUMB), "a drawn box is not drawn again");
    assert_eq!(slot.shown(), Some(&(IPAD_THUMB, "ipad")));
}

#[test]
fn a_resized_box_shows_the_old_preview_until_the_new_one_is_drawn() {
    let mut slot = PreviewSlot::default();
    slot.want((523, 245));
    slot.finish((523, 245), Some("old"));
    assert!(slot.want((530, 248)), "a new box asks the worker");
    assert!(
        !slot.want((540, 252)),
        "a window still resizing waits for the drawing under way"
    );
    assert_eq!(slot.shown(), Some(&((523, 245), "old")));
    slot.finish((530, 248), Some("newer"));
    assert_eq!(
        slot.shown(),
        Some(&((530, 248), "newer")),
        "the newest drawing shows while the last box is drawn"
    );
    assert!(
        slot.want((540, 252)),
        "then the box wanted now is asked for"
    );
}

#[test]
fn a_slot_keeps_the_map_list_and_setup_previews_and_drops_older_ones() {
    let (list, setup, resized) = ((523, 245), (1140, 1140), (530, 248));
    let mut slot = PreviewSlot::default();
    slot.want(list);
    slot.finish(list, Some("list"));
    slot.want(setup);
    slot.finish(setup, Some("setup"));
    assert!(
        !slot.want(list),
        "back on the list, its preview is still drawn"
    );
    assert_eq!(slot.shown(), Some(&(list, "list")));
    assert!(!slot.want(setup), "and so is the setup screen's");
    assert_eq!(slot.shown(), Some(&(setup, "setup")));
    slot.want(resized);
    slot.finish(resized, Some("resized"));
    assert_eq!(slot.drawn.len(), BOXES_KEPT);
    assert!(
        slot.drawn.iter().all(|(size, _)| *size != list),
        "the box wanted longest ago is dropped"
    );
}

#[test]
fn a_map_that_cannot_be_drawn_is_not_asked_for_again() {
    let mut slot: PreviewSlot<&str> = PreviewSlot::default();
    assert!(slot.want(IPAD_THUMB));
    slot.finish(IPAD_THUMB, None);
    assert!(!slot.want(IPAD_THUMB));
    assert!(!slot.want((261, 122)), "no box can draw it");
    assert!(slot.shown().is_none());
}

#[test]
fn a_preview_drawn_for_its_box_maps_texel_for_pixel_from_a_whole_pixel_corner() {
    // A 96-tile square map in an iPad map list card: 245 pixels at 2x.
    let area = Rect::new(52.3, 224.75, 261.5, 122.75);
    let fitted = pixel_box(area, 2.0).unwrap();
    assert_eq!(fitted, IPAD_THUMB);
    let rect = preview_rect((245.0, 245.0), fitted, area, 2.0);
    assert_eq!((rect.w * 2.0, rect.h * 2.0), (245.0, 245.0));
    assert_eq!((rect.x * 2.0).fract(), 0.0, "the corner sits on a pixel");
    assert_eq!((rect.y * 2.0).fract(), 0.0);
    assert!((rect.x + rect.w * 0.5 - (area.x + area.w * 0.5)).abs() <= 0.5);
    assert!((rect.y + rect.h * 0.5 - (area.y + area.h * 0.5)).abs() <= 0.5);
}

#[test]
fn a_preview_from_another_box_or_short_of_its_box_is_scaled_to_fit() {
    let area = Rect::new(0.0, 0.0, 300.0, 150.0);
    let stale = preview_rect((245.0, 245.0), (523, 245), area, 1.0);
    assert_eq!(
        (stale.w, stale.h),
        (150.0, 150.0),
        "an old box's preview fills the new one"
    );
    let short = preview_rect((60.0, 30.0), (300, 150), area, 1.0);
    assert_eq!(
        (short.w, short.h),
        (300.0, 150.0),
        "a preview that stopped at full scale is enlarged to fill"
    );
}

#[test]
fn an_area_with_no_whole_pixel_has_no_box() {
    assert_eq!(pixel_box(Rect::new(0.0, 0.0, 0.4, 100.0), 2.0), None);
    assert_eq!(pixel_box(Rect::new(0.0, 0.0, 100.0, 0.0), 1.0), None);
    assert_eq!(
        pixel_box(Rect::new(0.0, 0.0, 100.6, 50.2), 1.0),
        Some((100, 50))
    );
}
