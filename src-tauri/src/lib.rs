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
//! As of T09 every module in `docs/MVP_IMPLEMENTATION_SPEC.md` §3 is filled;
//! none is a placeholder any more.

mod app;
pub mod config;
/// The native message box, for the two moments there is no window to draw in
/// (`docs/MVP_IMPLEMENTATION_SPEC.md` §11; #60).
mod dialog;
mod health;
mod instance;
mod ipc;
pub mod logging;
pub mod process;
pub mod pty;
// Release-manifest guards (T12, #13). Test-only: they pin facts about the
// packaging that no production code reads, and they need `ipc::APP_NAME` and
// `config::APP_DIR_NAME`, which are not public API.
#[cfg(test)]
mod release;
pub mod session;
pub mod shell;
mod tray;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use std::sync::Arc;

    // `manage` lives on the `Manager` trait, not on `App` itself.
    use tauri::Manager;

    use instance::{Request, RequestHandler};

    // What this launch is asking for. Today the entry can only be *opened*, so
    // this is a constant rather than a parse of the command line: the
    // operations that need arguments (#62's temporary terminal, #63's working
    // directory) are the ones that will decide what their arguments mean, and
    // guessing at a grammar now would invent a contract nobody sends.
    let request = Request::Open;

    // Before anything else — before a registry, a window or a supervisor
    // exists. A process that is not the Hub must never start supervising the
    // user's sessions, because the Hub it is handing off to already is
    // (spec §2: 单实例确认先于建立另一份监管器).
    let primary = match instance::claim() {
        // A Hub is already running: give it the request, report what really
        // happened, and end. Nothing below this line is reached — no window, no
        // registry, no second tray icon.
        Ok(instance::Role::HandOff(hand_off)) => match hand_off.deliver(request) {
            Ok(_) => std::process::exit(instance::EXIT_HANDED_OFF),
            Err(error) => {
                dialog::report(error.message());
                std::process::exit(instance::EXIT_NOT_DELIVERED);
            }
        },
        Ok(instance::Role::Primary(primary)) => primary,
        Err(error) => {
            dialog::report(error.message());
            std::process::exit(instance::EXIT_NOT_DELIVERED);
        }
    };

    // The request cannot be answered until the window exists, and the pipe that
    // carries requests has to be listening before it does: the launches that
    // arrive during this Hub's own start-up are exactly the ones a cold-start
    // race produces (spec §5), and they must wait rather than be lost.
    let hub = Arc::new(app::launch::Hub::new());
    primary.serve(hub.clone());

    tauri::Builder::default()
        // Closing the main window hides it; the tray is the way back and the
        // only way out (D-006, spec §11).
        .on_window_event(tray::on_window_event)
        // The session registry is built once, at startup, and shared by every
        // caller. It is not started from here: registering a session is not a
        // lifecycle operation, and nothing gets a process until something asks
        // (spec §3, "UI does not own process truth" — nor does startup).
        .setup(move |app| {
            let (core, config_report) = app::bootstrap(app.handle().clone());
            app.manage(core);
            app.manage(config_report);
            // After `manage`, so the tray's first menu is built from the real
            // registry instead of briefly showing an empty one.
            tray::install(app.handle())?;
            // Last, and only now: the window exists, so a request that was
            // waiting on this can be answered. Before this line the honest
            // answer is "the Hub is not ready", which is what `hub` gives.
            hub.publish(app.handle().clone());
            // This launch's own request, through the same path a handed-off one
            // takes. It is the first thing the pipeline does for real, and the
            // reason the pipeline is not dead code until a second click.
            let _ = hub.handle(request);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::ping,
            ipc::session::get_config_report,
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
