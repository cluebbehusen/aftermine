//! Running the driver binary and the temporary files its suites hand it.
//! Each test binary includes this file and uses only part of it.
#![allow(dead_code)]

use aftermine_kit::GameReplay;
use serde_json::Value;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

/// The driver binary, ready for its arguments.
pub fn driver() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aftermine-driver"))
}

fn output(command: &mut Command) -> Output {
    command.output().expect("run aftermine-driver")
}

/// Runs `command`, which must succeed, and returns its standard output.
pub fn succeeded(command: &mut Command) -> String {
    let output = output(command);
    assert!(
        output.status.success(),
        "{command:?} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("stdout is UTF-8")
}

/// Runs `command`, which must fail, and returns its standard error.
pub fn refused(command: &mut Command) -> String {
    let output = output(command);
    assert!(!output.status.success(), "{command:?} was accepted");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// One JSON value per line.
pub fn jsonl(text: &str) -> Vec<Value> {
    text.lines()
        .map(|line| serde_json::from_str(line).expect("each line is one JSON value"))
        .collect()
}

fn unique(name: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("aftermine-{name}-{}-{id}", std::process::id()))
}

/// An empty private directory, removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let path = unique(name);
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("create a scratch directory");
        Self(path)
    }
}

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// A replay saved to a private file, removed on drop.
pub struct TempReplay(PathBuf);

impl TempReplay {
    pub fn save(name: &str, replay: &GameReplay) -> Self {
        let path = unique(name).with_extension("json");
        replay.save(&path).expect("save a replay fixture");
        Self(path)
    }
}

impl Deref for TempReplay {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempReplay {
    fn drop(&mut self) {
        std::fs::remove_file(&self.0).ok();
    }
}
