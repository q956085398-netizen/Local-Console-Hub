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
//! Modules other than `ipc`, `config` and `process` are intentionally empty
//! placeholders: they exist so later tickets (T02–T09) fill the right
//! boundaries instead of inventing parallel subsystems. No fake
//! process/session/PTY/logging behavior lives here (T00 out-of-scope).

mod app;
pub mod config;
mod health;
mod ipc;
mod logging;
pub mod process;
mod pty;
mod session;
mod tray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![ipc::ping])
        .run(tauri::generate_context!())
        .expect("error while running Local Console Hub");
}
