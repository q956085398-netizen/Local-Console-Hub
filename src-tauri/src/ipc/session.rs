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

use serde::Serialize;
use tauri::State;

use crate::config::{ConfigReportDto, SessionConfigDto};
use crate::session::core::{CreatedSession, SessionCore, SessionError};
use crate::session::runtime::SessionRuntime;
use crate::shell;

use super::hand_over_failed;

/// What "新建 PowerShell" answers with (#62).
///
/// Both halves of the session it made, from the same operation that made it:
/// the window selects the new terminal by id and renders it from this pair, so
/// nothing has to follow the answer with a list read that may not have caught
/// up (spec #59 decision 4).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedSessionDto {
    pub config: SessionConfigDto,
    pub runtime: SessionRuntime,
}

impl From<CreatedSession> for CreatedSessionDto {
    fn from(created: CreatedSession) -> Self {
        CreatedSessionDto {
            config: SessionConfigDto::from(&created.config).temporary(),
            runtime: created.runtime,
        }
    }
}

/// The startup config-loading and validation report. The report is captured
/// once during bootstrap; this read-only command never edits or reloads the
/// user's config file.
#[tauri::command]
pub fn get_config_report(report: State<'_, ConfigReportDto>) -> ConfigReportDto {
    report.inner().clone()
}

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
///
/// Since #62 the list also says which rows are temporary — created from the
/// window rather than loaded from the config file — because that is the one
/// difference a row's controls act on (it is the row that can be removed).
#[tauri::command]
pub fn list_session_configs(core: State<'_, SessionCore>) -> Vec<SessionConfigDto> {
    core.entries()
        .iter()
        .map(|entry| {
            let config = SessionConfigDto::from(&entry.config);
            if entry.temporary {
                config.temporary()
            } else {
                config
            }
        })
        .collect()
}

/// Create, register and start a temporary interactive terminal (#62).
///
/// The quick entry behind "新建 PowerShell": no form, no config file, and no
/// second kind of session — what it makes is a terminal session Session Core
/// starts, watches and closes the same way it does every other one. `cwd` is
/// the directory an external entry asked for; the session layer decides what
/// an absent one means (the user's home directory) rather than this thin
/// command doing it (spec #59 decision 5).
#[tauri::command]
pub fn create_temporary_terminal(
    core: State<'_, SessionCore>,
    cwd: Option<String>,
) -> Result<CreatedSessionDto, SessionError> {
    core.create_temporary_terminal(cwd.as_deref())
        .map(CreatedSessionDto::from)
}

/// Remove a temporary session from the registry (#62).
///
/// Session Core refuses a configured session (that one lives in the config
/// file) and a live one (its process tree is still owned by its handle), so
/// this command cannot be used as a way to stop something by deleting it.
#[tauri::command]
pub fn remove_session(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<(), SessionError> {
    core.remove_session(&session_id)
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
