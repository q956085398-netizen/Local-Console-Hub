//! Local Console Hub — backend entry point.
//!
//! Module layout mirrors the boundaries frozen in
//! `docs/MVP_IMPLEMENTATION_SPEC.md` §3. Each module below owns one
//! responsibility; the boundary rules from that spec apply:
//!
//! - UI does not own process truth;
//! - PTY code does not decide logging policy;
//! - Logging code does not decide session lifecycle;
//! - Tray actions call the same Session Core APIs as the main window;
//! - Session Core is the single source of lifecycle truth.
//!
//! Modules other than `ipc`, `config`, `process`, `pty`, `session`, `logging`,
//! `shell` and `health` are intentionally empty placeholders: they exist so
//! later tickets fill the right boundaries instead of inventing parallel
//! subsystems. No fake process/session/logging behavior lives here (T00
//! out-of-scope).

mod app;
pub mod config;
mod health;
mod ipc;
pub mod logging;
pub mod process;
pub mod pty;
pub mod session;
pub mod shell;
mod tray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // `manage` lives on the `Manager` trait, not on `App` itself.
    use tauri::Manager;

    tauri::Builder::default()
        // The session registry is built once, at startup, and shared by every
        // caller. It is not started from here: registering a session is not a
        // lifecycle operation, and nothing gets a process until something asks
        // (spec §3, "UI does not own process truth" — nor does startup).
        .setup(|app| {
            let core = app::bootstrap(app.handle().clone());
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::ping,
            ipc::session::list_sessions,
            ipc::session::list_session_configs,
            ipc::session::get_session,
            ipc::session::start_session,
            ipc::session::stop_session,
            ipc::session::force_stop_session,
            ipc::session::restart_session,
            ipc::session::open_session_url,
            ipc::session::open_session_cwd,
            ipc::terminal::attach_terminal,
            ipc::terminal::terminal_write,
            ipc::terminal::terminal_resize,
            ipc::logs::get_log_info,
            ipc::logs::get_run_history,
            ipc::logs::save_run_log,
            ipc::logs::set_log_recording,
            ipc::logs::open_log_file,
            ipc::logs::open_log_folder,
            ipc::logs::preview_log_cleanup,
            ipc::logs::cleanup_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Local Console Hub");
}
