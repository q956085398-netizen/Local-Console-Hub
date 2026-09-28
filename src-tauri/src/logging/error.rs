//! What the logging layer reports when it cannot do what it was asked to.
//!
//! A logging failure is never a lifecycle failure. A service whose log file
//! cannot be opened still starts, still runs and still stops: the policy that
//! says "write this down" is not the same thing as the process being
//! supervised (`docs/DEVELOPMENT.md` §3, `docs/DECISIONS.md` D-004). So this
//! error is carried alongside the run — reported through the log status the UI
//! reads — rather than returned in place of the run.
//!
//! The message names the operation and the path (`docs/DEVELOPMENT.md` §9): a
//! user has to be able to read it and know which file to look at.

use serde::Serialize;

use std::fmt;
use std::path::Path;

/// A logging operation that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogError {
    /// The operation, e.g. `opening the run log`.
    pub operation: String,
    /// Human-readable cause, with the action a user can take.
    pub message: String,
    /// The file the operation was about, when there was one.
    pub path: Option<String>,
}

impl LogError {
    /// A failure without a path (nothing on disk was involved).
    pub fn new(operation: impl Into<String>, message: impl Into<String>) -> Self {
        LogError {
            operation: operation.into(),
            message: message.into(),
            path: None,
        }
    }

    /// A failure about `path`.
    pub fn at(operation: impl Into<String>, path: &Path, message: impl Into<String>) -> Self {
        LogError {
            operation: operation.into(),
            message: message.into(),
            path: Some(path.display().to_string()),
        }
    }
}

impl fmt::Display for LogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.path {
            Some(path) => write!(
                formatter,
                "{} failed for {path}: {}",
                self.operation, self.message
            ),
            None => write!(formatter, "{} failed: {}", self.operation, self.message),
        }
    }
}

impl std::error::Error for LogError {}

/// Turn an [`std::io::Error`] into a logging failure about `path`.
pub(crate) fn io_error(operation: &str, path: &Path, error: std::io::Error) -> LogError {
    LogError::at(operation, path, describe(&error))
}

/// The cause, with the action a user can take where there is one
/// (`docs/DEVELOPMENT.md` §9: "可行动建议（若能确定）").
///
/// The OS message alone is accurate and useless: `拒绝访问。 (os error 5)` tells
/// a user what happened and nothing about what to do, and the logging layer's
/// most likely failure — a log directory the app may not write into — has a
/// one-line answer. The raw message is kept for the kinds that have no advice
/// to give, because inventing advice for an unknown cause is worse than
/// reporting the cause.
fn describe(error: &std::io::Error) -> String {
    use std::io::ErrorKind;

    let advice = match error.kind() {
        ErrorKind::PermissionDenied => {
            "the Hub may not write there — check the folder's permissions, or point the log \
             directory somewhere it may write"
        }
        ErrorKind::NotFound => {
            "the path does not exist — check that the configured log location still exists"
        }
        ErrorKind::AlreadyExists => {
            "something is already in the way of that path — remove or rename it, or configure \
             a different log location"
        }
        // A full disk is the other failure a long-running service reaches.
        ErrorKind::StorageFull => {
            "the disk is full — free space, or lower the session's log retention"
        }
        _ => return error.to_string(),
    };
    format!("{advice} ({error})")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failure names the operation, the path and what to do about it.
    #[test]
    fn a_write_failure_reads_as_an_instruction() {
        let error = io_error(
            "creating the run log",
            Path::new("C:/logs/comfyui/run.log"),
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        );

        let text = error.to_string();
        assert!(text.contains("creating the run log"), "{text}");
        assert!(text.contains("run.log"), "{text}");
        assert!(text.contains("permissions"), "no advice in: {text}");
        assert_eq!(
            error.path.as_deref(),
            Some("C:/logs/comfyui/run.log"),
            "the path is structured as well as printed"
        );
    }

    /// A cause with no advice to give is reported as it is, rather than
    /// dressed up in advice that may not apply.
    #[test]
    fn an_unrecognised_cause_keeps_the_os_message() {
        let error = io_error(
            "writing the run log",
            Path::new("run.log"),
            std::io::Error::other("the device disappeared"),
        );

        assert!(error.message.contains("the device disappeared"));
    }
}
