//! Non-Windows stand-in for the window backend.
//!
//! The Hub is Windows-first (`docs/DECISIONS.md` D-001), and window discovery
//! is one of the places where "cross-platform abstraction" would mean inventing
//! an answer rather than porting one: another platform's windows belong to a
//! different compositor with different rules about focus. The honest reading is
//! therefore what this module reports — no windows to find, nothing to close,
//! and a refusal rather than a claimed focus — so a hypothetical port fails in
//! front of the user instead of silently pretending.

use super::{FocusOutcome, TopLevelWindow, WindowHandle};

pub fn windows_of(_pids: &[u32]) -> Vec<TopLevelWindow> {
    Vec::new()
}

/// No console host to ask on a platform without the Windows console model.
pub fn console_window(_pid: u32) -> Option<TopLevelWindow> {
    None
}

pub fn focus(_handle: WindowHandle) -> FocusOutcome {
    FocusOutcome::Refused
}

pub fn close(_handle: WindowHandle) -> bool {
    false
}
