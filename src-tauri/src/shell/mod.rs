//! Shell layer — handing one path to the operating system.
//!
//! One action: open a file or a folder with whatever the OS associates with it.
//! The Logs tab is the first caller (`docs/LOGGING.md` §10 lists "open log" and
//! "open containing folder"); opening a session's URL or working directory is
//! the next (spec §9).
//!
//! ## This layer does not decide *what* may be opened
//!
//! Every caller resolves its path first — Session Core turns a log action into
//! a path the session actually owns (`SessionCore::log_file_target`) — and this
//! layer only performs the handoff. Keeping resolution and handoff apart is what
//! stops the IPC surface from growing an `open_path(anything)` command, which
//! would quietly turn the webview into a general file launcher.
//!
//! ## Contract
//!
//! ```text
//! open_path(path) -> Result<(), ShellError>
//! ```
//!
//! ## Platform note
//!
//! Windows is the release target (D-001). Elsewhere the layer reports the
//! limitation instead of guessing at a handler, so a non-Windows build fails
//! loudly rather than quietly leaving a button that does nothing.

use std::path::Path;

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
use unsupported as backend;
#[cfg(windows)]
use win as backend;

/// Hand `path` to its default handler: a file opens, a folder opens.
///
/// A path that does not exist is an error rather than a silent nothing. The
/// caller has already resolved it from a session's own state, so "not there"
/// means the log was cleaned up, the application has not written it yet, or the
/// folder is gone — all of which the user needs told rather than left guessing
/// whether the click registered.
pub fn open_path(path: &Path) -> Result<(), ShellError> {
    const OPERATION: &str = "opening a path";
    if !path.exists() {
        return Err(ShellError::new(
            OPERATION,
            path,
            "the path does not exist — it may have been cleaned up, or the application that \
             writes it has not created it yet",
        ));
    }
    backend::open_path(path, OPERATION)
}

/// Why the OS would not take a path.
///
/// Shaped like the other layers' errors (`docs/DEVELOPMENT.md` §9): the
/// operation, the path, and what a user can do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellError {
    /// The operation, e.g. `opening a path`.
    pub operation: String,
    /// The path the operation was about.
    pub path: String,
    /// Human-readable cause and advice.
    pub message: String,
}

impl ShellError {
    /// A failure about `path`.
    pub fn new(operation: impl Into<String>, path: &Path, message: impl Into<String>) -> Self {
        ShellError {
            operation: operation.into(),
            path: path.display().to_string(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} failed for {}: {}",
            self.operation, self.path, self.message
        )
    }
}

impl std::error::Error for ShellError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::test_support::TempDir;

    /// A path the user can no longer see is reported, not ignored.
    #[test]
    fn a_path_that_does_not_exist_is_an_error_with_advice() {
        let dir = TempDir::new();
        let missing = dir.join("svc/2026-09/cleaned-up.log");

        let error = open_path(&missing).expect_err("a missing path is refused");

        assert_eq!(error.operation, "opening a path");
        assert!(error.path.ends_with("cleaned-up.log"), "{}", error.path);
        assert!(
            error.message.contains("does not exist"),
            "{}",
            error.message
        );
    }

    // Nothing here calls `open_path` on a path that exists: the handoff is a
    // real `ShellExecuteW`, and a test that opened a file would launch a
    // program on whoever ran it.
}
