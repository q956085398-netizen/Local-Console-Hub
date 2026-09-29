//! Shell layer — handing one target to the operating system.
//!
//! Two actions: open a file or folder with whatever the OS associates with it,
//! and open a URL with whatever handles its scheme. The Logs tab was the first
//! caller (`docs/LOGGING.md` §10 lists "open log" and "open containing
//! folder"); T08 added a session's URL and working directory (spec §9).
//!
//! ## This layer does not decide *what* may be opened
//!
//! Every caller resolves its target first — Session Core turns a log or a
//! session action into a path or URL the session actually owns
//! (`SessionCore::log_file_target`, `SessionCore::session_url`) — and this layer
//! only performs the handoff. Keeping resolution and handoff apart is what stops
//! the IPC surface from growing an `open(anything)` command, which would quietly
//! turn the webview into a general launcher (`docs/DECISIONS.md` D-021).
//!
//! ## Contract
//!
//! ```text
//! open_path(path) -> Result<(), ShellError>
//! open_url(url)   -> Result<(), ShellError>
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
            &path.display().to_string(),
            "the path does not exist — it may have been cleaned up, or the application that \
             writes it has not created it yet",
        ));
    }
    backend::open_path(path, OPERATION)
}

/// Hand `url` to the OS's protocol handler.
///
/// There is nothing to check on disk here — a URL is not a file — so what
/// replaces `open_path`'s existence check is the scheme. Only `http` and
/// `https` are handed over, the same rule the configuration was validated
/// against (`config::validate_url`). Checking it again where a string becomes an
/// OS action is the cheap half of a boundary worth having twice: a `file:`,
/// `javascript:` or custom-scheme URL reaching `ShellExecuteW` would be a way to
/// ask the machine to do something this layer never offered.
pub fn open_url(url: &str) -> Result<(), ShellError> {
    const OPERATION: &str = "opening a URL";
    if !is_openable_url(url) {
        return Err(ShellError::new(
            OPERATION,
            url,
            "only http and https URLs can be opened — this is neither",
        ));
    }
    backend::open_url(url, OPERATION)
}

/// Whether `url` is one this layer will hand to the OS.
///
/// Parsed rather than prefix-matched, so the answer is the same rule the config
/// layer applied (`config::validate_url`): a scheme is what tells the shell how
/// to interpret a target, and `starts_with("http")` would wave through strings
/// the parser does not agree are URLs at all.
fn is_openable_url(url: &str) -> bool {
    match url::Url::parse(url) {
        Ok(parsed) => parsed.scheme() == "http" || parsed.scheme() == "https",
        Err(_) => false,
    }
}

/// Why the OS would not take a target.
///
/// Shaped like the other layers' errors (`docs/DEVELOPMENT.md` §9): the
/// operation, the target, and what a user can do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellError {
    /// The operation, e.g. `opening a path`.
    pub operation: String,
    /// What the operation was about: a path, or a URL.
    pub target: String,
    /// Human-readable cause and advice.
    pub message: String,
}

impl ShellError {
    /// A failure about `target`.
    pub fn new(operation: impl Into<String>, target: &str, message: impl Into<String>) -> Self {
        ShellError {
            operation: operation.into(),
            target: target.to_owned(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} failed for {}: {}",
            self.operation, self.target, self.message
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
        assert!(error.target.ends_with("cleaned-up.log"), "{}", error.target);
        assert!(
            error.message.contains("does not exist"),
            "{}",
            error.message
        );
    }

    /// The scheme check is what stands in for the existence check a path gets,
    /// so it is the one rule this layer enforces on a URL.
    #[test]
    fn only_http_and_https_urls_are_openable() {
        assert!(is_openable_url("http://127.0.0.1:8188"));
        assert!(is_openable_url("https://example.com/app"));

        // Anything that could ask the machine for something other than a page:
        // a file path, a script scheme, a custom protocol, or a string that is
        // not a URL at all.
        for refused in [
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "steam://run/12345",
            "ms-settings:",
            "D:\\Tools\\ComfyUI",
            "",
        ] {
            assert!(!is_openable_url(refused), "{refused} should be refused");
        }
    }

    /// A refused URL comes back as an actionable error naming what was refused.
    #[test]
    fn a_url_that_is_not_http_is_refused_without_reaching_the_os() {
        let error =
            open_url("file:///C:/Windows/System32/calc.exe").expect_err("a file URL is refused");

        assert_eq!(error.operation, "opening a URL");
        assert_eq!(error.target, "file:///C:/Windows/System32/calc.exe");
        assert!(error.message.contains("http"), "{}", error.message);
    }

    // Nothing here opens a URL, or a path that exists: the handoff is a real
    // `ShellExecuteW`, and a test that launched one would open a browser or a
    // program on whoever ran it.
}
