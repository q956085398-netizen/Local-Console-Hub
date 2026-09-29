//! Non-Windows shell backend — deliberately not implemented.
//!
//! v0.1.0 hands paths to the Windows shell only (`docs/DECISIONS.md` D-001).
//! Rather than guessing at a handler on a platform the product does not ship
//! for, every call reports the limitation, so a non-Windows build fails loudly
//! instead of quietly leaving a button that does nothing.
//!
//! The item is part of the interface `mod.rs` drives, so it is unreachable at
//! runtime on this platform rather than unused at compile time.

use std::path::Path;

use super::ShellError;

/// Never reached: [`super::open_path`]'s existence check runs first, and this
/// is the handoff that follows it.
pub fn open_path(path: &Path, operation: &str) -> Result<(), ShellError> {
    Err(ShellError::new(
        operation,
        &path.display().to_string(),
        "opening paths is implemented on Windows only; this build has no shell to hand it to",
    ))
}

/// Never reached: [`super::open_url`]'s scheme check runs first, and this is
/// the handoff that follows it.
pub fn open_url(url: &str, operation: &str) -> Result<(), ShellError> {
    Err(ShellError::new(
        operation,
        url,
        "opening URLs is implemented on Windows only; this build has no shell to hand it to",
    ))
}
