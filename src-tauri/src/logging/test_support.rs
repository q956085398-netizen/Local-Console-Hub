//! Test-only scaffolding shared by the logging layer's tests.
//!
//! The layer writes real files, and its tests assert on real files: a log path
//! that is only correct as a string proves nothing about whether a log was
//! written. A scratch directory per test keeps the user's `%LOCALAPPDATA%` out
//! of it and makes "the file is gone after cleanup" an assertion a test can
//! actually make.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// A directory that exists for the lifetime of one test.
///
/// The name is unique per creation rather than per test, so a test that needs
/// two app-data roots gets two directories, and a run alongside a previous one
/// cannot collide with it.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new() -> Self {
        static SEQUENCE: AtomicU32 = AtomicU32::new(0);

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("lch-logging-{stamp}-{sequence:04x}"));
        fs::create_dir_all(&path).expect("the scratch directory is creatable");
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A path inside the scratch directory. Nothing is created by calling this.
    pub fn join(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
