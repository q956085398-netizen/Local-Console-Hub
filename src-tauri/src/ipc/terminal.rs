//! Terminal commands (`docs/MVP_IMPLEMENTATION_SPEC.md` §6, §9; T07 #8).
//!
//! Three named operations, each one thing a terminal view does: read what it
//! needs to render a session's stream, type into it, and tell it how large the
//! view is. Like every other command here they are thin entry points onto
//! Session Core — no terminal state lives in this layer, so the window, the
//! tray and a future scheduler all act on the same truth.
//!
//! ## Why input and output travel base64-encoded
//!
//! A terminal's bytes are not text. A read from the console host can split a
//! multi-byte character, and both directions have to survive that: input
//! arrives as bytes the view decoded from a keystroke or a paste, and output
//! arrives as bytes the console produced. JSON has no byte string, and a
//! number array would triple the size of the high-volume output spec §14 is
//! about, so base64 is the transport.
//!
//! Output reaches the view as an event rather than a return value: a terminal
//! streams, and a command that returned output would be a poll. The
//! `terminal-output` payload carries the same batches
//! [`TerminalAttachment`] hands over, which is what lets a view attach to a
//! session that is already running without losing or repeating a byte.

use base64::Engine;
use tauri::State;

use crate::session::core::{SessionCore, SessionError, SessionErrorKind};
use crate::session::terminal::TerminalAttachment;

/// Everything a terminal view needs to render a session's stream from now on.
///
/// Answers for a session that is not running too, with the scrollback the last
/// run left behind: the view shows that under its "not running" state rather
/// than pretending there is nothing to see.
#[tauri::command]
pub fn attach_terminal(
    core: State<'_, SessionCore>,
    session_id: String,
) -> Result<TerminalAttachment, SessionError> {
    core.terminal_attachment(&session_id)
}

/// Send input to a session's terminal.
///
/// `data` is base64-encoded bytes: keystrokes, a paste, or the `0x03` byte
/// that is Ctrl+C. Interrupting what the shell is running is not closing the
/// session — closing it is `stop_session` (spec §7).
#[tauri::command]
pub fn terminal_write(
    core: State<'_, SessionCore>,
    session_id: String,
    data: String,
) -> Result<(), SessionError> {
    const OPERATION: &str = "terminal_write";

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&data)
        .map_err(|error| SessionError {
            kind: SessionErrorKind::Failed,
            session_id: session_id.clone(),
            operation: OPERATION.to_owned(),
            message: format!("the input sent to `{session_id}` is not base64-encoded: {error}"),
            from: None,
        })?;

    core.terminal_write(&session_id, &bytes)
}

/// Tell a session's terminal how large its view is.
///
/// A live terminal is resized; one that is not running remembers the size for
/// its next start, so a shell is born at the geometry its view has already
/// measured rather than at a default it will immediately leave.
#[tauri::command]
pub fn terminal_resize(
    core: State<'_, SessionCore>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), SessionError> {
    core.terminal_resize(&session_id, cols, rows)
}
