//! Tray layer — native tray lifecycle.
//!
//! Owned by T09 (#10, tray lifecycle and quick controls): hide-to-tray on
//! window close (`DECISIONS.md` D-006), compact running summary, Show
//! Window / Restart Failed / Stop All / Exit with confirmation. Tray actions
//! call the same Session Core APIs as the main window and never silently
//! destroy active sessions.
