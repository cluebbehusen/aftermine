use super::*;
use std::sync::mpsc;
use std::time::Duration;

fn scaled(scale: f32) -> Config {
    Config {
        ui_scale: scale,
        ..Config::default()
    }
}

/// A writer whose writes each wait for a release, or for every release to be
/// dropped, and announce when they begin.
fn held_writer() -> (Writer, mpsc::Receiver<String>, mpsc::Sender<()>) {
    let (began, begun) = mpsc::channel();
    let (release, released) = mpsc::channel::<()>();
    let writer = Writer::spawn(move |text| {
        began.send(text.to_owned()).ok();
        released.recv().ok();
        Ok(())
    });
    (writer, begun, release)
}

#[test]
fn a_save_made_while_one_is_written_replaces_any_still_waiting() {
    let (writer, begun, release) = held_writer();
    writer.save(&scaled(1.0));
    assert_eq!(begun.recv().unwrap(), scaled(1.0).text());
    for scale in [1.1, 1.2, 1.3] {
        writer.save(&scaled(scale));
    }
    drop(release);
    writer.flush();
    let rest: Vec<String> = begun.try_iter().collect();
    assert_eq!(rest, [scaled(1.3).text()]);
}

#[test]
fn a_flush_waits_for_the_write_under_way() {
    let (writer, begun, release) = held_writer();
    let writer = Arc::new(writer);
    writer.save(&scaled(1.1));
    begun.recv().unwrap();
    let (flushed, done) = mpsc::channel();
    let flusher = Arc::clone(&writer);
    std::thread::spawn(move || {
        flusher.flush();
        flushed.send(()).ok();
    });
    assert!(
        done.recv_timeout(Duration::from_millis(100)).is_err(),
        "the flush returned mid-write"
    );
    drop(release);
    done.recv_timeout(Duration::from_secs(10))
        .expect("the flush returns once the write ends");
}

#[test]
fn a_quit_flush_leaves_the_newest_settings_on_disk() {
    let dir = std::env::temp_dir().join(format!(
        "aftermine-settings-writer-{}-{}",
        std::process::id(),
        crate::entropy::bits()
    ));
    let path = dir.join("config.json");
    let writer = Writer::new(Some(path.clone()));
    for scale in [1.0, 1.1, 1.25] {
        writer.save(&scaled(scale));
    }
    writer.flush();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), scaled(1.25).text());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_write_is_reported_once() {
    let writer = Writer::spawn(|_| Err(std::io::Error::other("disk full")));
    writer.save(&scaled(1.0));
    writer.flush();
    assert_eq!(writer.failure().unwrap().to_string(), "disk full");
    assert!(writer.failure().is_none());
}
