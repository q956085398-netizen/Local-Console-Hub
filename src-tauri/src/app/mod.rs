//! App layer — application-level state and wiring shared by IPC, tray and
//! Session Core.
//!
//! Owned by T04 (#5, Session Core). Do not place session lifecycle logic
//! here; this module only hosts cross-cutting state (e.g. the Session Core
//! registry) and application bootstrap.
//!
//! ## Why the registry is built here and not in `session`
//!
//! [`SessionCore`] deliberately knows nothing about Tauri, the config file or
//! where it lives on disk. Assembling it into a running app is the one job
//! that needs all three, so it happens here — and only here, so the layer
//! below stays runnable with no window.

use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, Runtime};

use crate::config::{load_from_file, AppPaths};
use crate::logging::LogRoots;
use crate::session::core::SessionCore;
use crate::session::tauri_sink::TauriSink;

/// Build the session registry for the running app.
///
/// The registry starts from the user's config file. Nothing is started here:
/// registration is not a lifecycle operation, and a session only gets a
/// process when something asks it to start (spec §3, "UI does not own process
/// truth" — nor does startup).
///
/// The app-data layout is attached here too, because this is the one place
/// that knows both where the user's files live and which registry will write
/// them (`docs/LOGGING.md` §5). Without it the registry still works and still
/// buffers, but every policy that needs a file resolves to `off` with a
/// reported reason (T05) — a running app has no business being in that state.
pub fn bootstrap<R: Runtime>(app: AppHandle<R>) -> SessionCore {
    let paths = AppPaths::from_env();
    let core = match &paths {
        Some(paths) => SessionCore::new(Arc::new(TauriSink::new(app)))
            .with_log_roots(LogRoots::from_app_paths(paths)),
        None => SessionCore::new(Arc::new(TauriSink::new(app))),
    };
    register_configured(
        &core,
        paths.as_ref().map(|paths| paths.config_file.as_path()),
    );
    core
}

/// Register every valid session of `config_file` into `core`, returning how
/// many were registered.
///
/// A missing file is a fresh install and registers nothing — T01 models it as
/// an empty session list rather than an error, and the app has to open so it
/// can offer to create one. A file that cannot be read at all also registers
/// nothing rather than failing startup: losing the window would leave the user
/// no way to fix the file. Per-session problems are T01's
/// [`crate::config::LoadedConfig`] report and reach the UI through the config
/// surface, not through a crash.
pub fn register_configured(core: &SessionCore, config_file: Option<&Path>) -> usize {
    let Some(config_file) = config_file else {
        return 0;
    };
    let Ok(loaded) = load_from_file(config_file) else {
        return 0;
    };

    loaded
        .sessions
        .into_iter()
        .filter(|session| core.register(session.clone()).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::state::SessionStatus;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A config file that exists only for one test, so the assertions are about
    /// what `register_configured` did rather than about the user's real config.
    struct TempConfig(PathBuf);

    impl TempConfig {
        fn new(contents: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("lch-t04-{unique}.yaml"));
            fs::write(&path, contents).expect("the temp config is writable");
            TempConfig(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn configured_sessions_are_registered_and_left_stopped() {
        let config = TempConfig::new(
            "sessions:\n  - id: one\n    name: One\n    type: terminal\n    shell: powershell\n",
        );
        let core = SessionCore::without_listener();

        let registered = register_configured(&core, Some(config.path()));

        assert_eq!(registered, 1);
        let runtime = core.snapshot("one").expect("the session is registered");
        assert_eq!(
            runtime.status,
            SessionStatus::Stopped,
            "bootstrap must not start anything"
        );
    }

    /// A fresh install has no config file yet; that is not a failure.
    #[test]
    fn a_missing_config_file_registers_nothing() {
        let core = SessionCore::without_listener();
        let missing = std::env::temp_dir().join("lch-t04-does-not-exist.yaml");

        assert_eq!(register_configured(&core, Some(&missing)), 0);
        assert!(core.snapshots().is_empty());
    }

    #[test]
    fn no_config_location_registers_nothing() {
        let core = SessionCore::without_listener();
        assert_eq!(register_configured(&core, None), 0);
    }

    /// Broken YAML must not stop the app from opening — otherwise the user has
    /// no window in which to fix the file.
    #[test]
    fn a_broken_config_file_registers_nothing_instead_of_failing() {
        let config = TempConfig::new("sessions: [ this is not a list of sessions\n");
        let core = SessionCore::without_listener();

        assert_eq!(register_configured(&core, Some(config.path())), 0);
        assert!(core.snapshots().is_empty());
    }

    /// One bad entry must not hide the valid ones (spec §13, T01's contract).
    #[test]
    fn a_bad_entry_does_not_hide_the_good_ones() {
        let config = TempConfig::new(
            "sessions:\n  - id: broken\n    name: Broken\n    type: nonsense\n  - id: two\n    name: Two\n    type: terminal\n    shell: powershell\n",
        );
        let core = SessionCore::without_listener();

        assert_eq!(register_configured(&core, Some(config.path())), 1);
        assert!(core.snapshot("two").is_some());
        assert!(core.snapshot("broken").is_none());
    }
}
