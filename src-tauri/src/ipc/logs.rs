//! Logging commands (`docs/MVP_IMPLEMENTATION_SPEC.md` §9).
//!
//! The read side of T05's logging core: what a session's policy is doing
//! (`get_log_info`), what it has run (`get_run_history`), and the two actions
//! a user can take on a live run — save it (`save_run_log`, the `on_error`
//! policy's "I want this one after all") and switch recording on or off
//! (`set_log_recording`, the `manual` policy).
//!
//! Like the lifecycle commands, each one names a single Core operation and
//! adds nothing to it: the logs *view* is T10's, and a command that decided
//! anything here would be a second place the UI's behaviour could come from.
//!
//! `docs/LOGGING.md` §10 lists opening the log file and its folder as UI
//! actions. Those are shell actions rather than Session Core operations and
//! belong to the view that offers them (T10); what they need from here —
//! `logFile`, `sessionLogDir` — `get_log_info` already reports.

use tauri::State;

use crate::logging::{LogStatus, RunHistory};
use crate::session::core::{SessionCore, SessionError};

/// What this session's logging is doing, and where it writes
/// (`docs/LOGGING.md` §1.4).
#[tauri::command]
pub fn get_log_info(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<LogStatus, SessionError> {
    core.log_status(&session_id)
        .ok_or_else(|| SessionError::unknown_session(&session_id, "get_log_info"))
}

/// A session's completed runs, newest first (`docs/LOGGING.md` §6).
///
/// A session with no history answers with an empty history rather than an
/// error: never having run is the normal state of a session that was just
/// configured.
#[tauri::command]
pub fn get_run_history(core: State<'_, SessionCore>, session_id: String) -> RunHistory {
    core.run_history(&session_id)
}

/// Commit the current run's `on_error` log now, instead of waiting to see how
/// the run ends (`docs/LOGGING.md` §3).
#[tauri::command]
pub fn save_run_log(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<LogStatus, SessionError> {
    core.save_run_log(&session_id)
}

/// Turn recording on or off for a `manual` session's current run
/// (`docs/LOGGING.md` §3).
#[tauri::command]
pub fn set_log_recording(
    core: State<'_, SessionCore>,
    session_id: String,
    recording: bool,
) -> Result<LogStatus, SessionError> {
    core.set_log_recording(&session_id, recording)
}
