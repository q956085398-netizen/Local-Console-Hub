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

use crate::config::SessionConfigDto;
use crate::session::core::{SessionCore, SessionError};
use crate::session::runtime::SessionRuntime;
use crate::shell;

use super::hand_over_failed;

/// Every session's snapshot, in id order.
#[tauri::command]
pub fn list_sessions(core: State<'_, SessionCore>) -> Vec<SessionRuntime> {
    core.snapshots()
}

/// Every registered session's validated configuration, in id order.
///
/// The window renders a session from two halves: what it *is* — name, type,
/// purpose, close impact, port, cwd, shell — and what it is *doing* (the
/// snapshot above). This is the first half.
///
/// It answers from the same registry the snapshots come from, which is what
/// keeps the two halves describing one set of sessions: a row the window can
/// render is a session Session Core can start, stop and hand a terminal to.
#[tauri::command]
pub fn list_session_configs(core: State<'_, SessionCore>) -> Vec<SessionConfigDto> {
    core.configs().iter().map(SessionConfigDto::from).collect()
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

/// Open a session's configured URL with the OS's default handler (spec §9).
///
/// Takes a session id, never a URL: the session's own configuration is the only
/// place the address comes from, so this command cannot be used to send the
/// machine anywhere the user did not configure. A session without a `url` is
/// refused with what to add, rather than opening nothing (`crate::shell`'s note,
/// `docs/DECISIONS.md` D-021).
#[tauri::command]
pub fn open_session_url(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<(), SessionError> {
    let url = core.session_url(&session_id)?;
    shell::open_url(&url).map_err(|error| hand_over_failed(&session_id, "open_session_url", error))
}

/// Open a session's configured working directory in the OS's file browser
/// (spec §9).
///
/// The same shape as [`open_session_url`], and for the same reason: the folder
/// is resolved from the session, so this is not a general "open a folder"
/// command. A session with no `cwd`, or one whose folder has since been deleted,
/// is reported with an actionable message.
#[tauri::command]
pub fn open_session_cwd(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<(), SessionError> {
    let path = core.session_cwd(&session_id)?;
    shell::open_path(&path)
        .map_err(|error| hand_over_failed(&session_id, "open_session_cwd", error))
}
