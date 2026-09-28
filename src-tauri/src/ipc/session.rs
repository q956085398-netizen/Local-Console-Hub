//! Session lifecycle commands (`docs/MVP_IMPLEMENTATION_SPEC.md` §9).
//!
//! Each command is a thin, named entry point onto Session Core — no logic
//! lives here beyond naming the operation. That is deliberate: the window and
//! the tray are meant to call the same Core APIs (§3), so a command must not
//! be able to do something the tray could not.
//!
//! Commands return the post-operation [`SessionRuntime`] so a caller has the
//! new state without waiting for the event it will also receive; a refusal or
//! a failure returns the structured [`SessionError`], which names the
//! operation and the reason (`docs/DEVELOPMENT.md` §9).
//!
//! Tauri maps the frontend's camelCase arguments onto these snake_case
//! parameters, so the frontend calls `startSession({ sessionId })`.

use tauri::State;

use crate::session::core::{SessionCore, SessionError};
use crate::session::runtime::SessionRuntime;

/// Every session's snapshot, in id order.
#[tauri::command]
pub fn list_sessions(core: State<'_, SessionCore>) -> Vec<SessionRuntime> {
    core.snapshots()
}

/// One session's snapshot.
#[tauri::command]
pub fn get_session(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<SessionRuntime, SessionError> {
    core.snapshot(&session_id).ok_or_else(|| SessionError {
        kind: crate::session::core::SessionErrorKind::UnknownSession,
        session_id: session_id.clone(),
        operation: "get_session".to_owned(),
        message: format!("no session is registered as `{session_id}`"),
        from: None,
    })
}

/// Start a session.
#[tauri::command]
pub fn start_session(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<SessionRuntime, SessionError> {
    core.start(&session_id)
}

/// Stop a session, giving it the default grace period to unwind.
#[tauri::command]
pub fn stop_session(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<SessionRuntime, SessionError> {
    core.stop(&session_id)
}

/// Terminate a session's run without waiting for it to unwind.
#[tauri::command]
pub fn force_stop_session(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<SessionRuntime, SessionError> {
    core.force_stop(&session_id)
}

/// Replace a session's run with a fresh one.
#[tauri::command]
pub fn restart_session(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<SessionRuntime, SessionError> {
    core.restart(&session_id)
}
