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

pub mod activation;
pub mod applications;
pub mod discovery;
pub mod external;
pub mod form;
pub mod launch;
pub mod recommend;
pub mod terminals;

use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, Runtime};

use crate::config::{
    load_from_file, AppPaths, ConfigFileStatusDto, ConfigReportDto, SessionConfigErrorDto,
};
use crate::logging::LogRoots;
use crate::session::core::{EventSink, FanoutSink, SessionCore, SessionEntry};
use crate::session::tauri_sink::TauriSink;
use crate::tray::TraySink;

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
///
/// Session Core publishes to both of the app's listeners from here: the event
/// transport the window reads, and the tray (T09). Neither is the other's
/// caller: the tray reads the registry it is handed and calls the same
/// operations the window does, so a hidden window changes what the tray can
/// see, never what it can do.
///
/// The tray half is wired here rather than after the registry is managed
/// because [`FanoutSink`] is a sink, not a listener registry — it is built
/// with the core and cannot be reopened afterwards. `TraySink` therefore looks
/// the core up through the app handle when an event arrives, and events cannot
/// arrive before `manage`, because registration publishes none.
pub fn bootstrap<R: Runtime>(app: AppHandle<R>) -> (SessionCore, ConfigReportDto) {
    let paths = AppPaths::from_env();
    let sink = Arc::new(FanoutSink::new(vec![
        Arc::new(TauriSink::new(app.clone())) as Arc<dyn EventSink>,
        Arc::new(TraySink::new(app)),
    ]));
    let core = match &paths {
        Some(paths) => SessionCore::new(sink).with_log_roots(LogRoots::from_app_paths(paths)),
        None => SessionCore::new(sink),
    };
    let report = register_configured(
        &core,
        paths.as_ref().map(|paths| paths.config_file.as_path()),
    );
    (core, report)
}

/// Register valid sessions from the user's config file and retain its report
/// for the window. Read and parse failures are data, not startup failures, so
/// the user can open the app and fix their config.
///
/// A missing file is a fresh install and registers nothing — T01 models it as
/// an empty session list rather than an error, and the app has to open so it
/// can offer to create one. A file that cannot be read at all also registers
/// nothing rather than failing startup: losing the window would leave the user
/// no way to fix the file. Per-session problems are T01's
/// [`crate::config::LoadedConfig`] report and reach the UI through the config
/// surface, not through a crash.
pub fn register_configured(core: &SessionCore, config_file: Option<&Path>) -> ConfigReportDto {
    let Some(config_file) = config_file else {
        return config_problem(
            ConfigFileStatusDto::Unavailable,
            None,
            "无法确定配置文件位置；请检查系统应用数据目录后重启应用。".to_owned(),
        );
    };
    let loaded = match load_from_file(config_file) {
        Ok(loaded) => loaded,
        Err(error) => {
            return config_problem(
                ConfigFileStatusDto::Unreadable,
                Some(config_file.to_string_lossy().into_owned()),
                format!(
                    "无法读取配置文件 `{}`：{error}。请检查文件权限后重启应用。",
                    config_file.display()
                ),
            );
        }
    };

    let mut report = loaded.to_dto();
    report.config_path = Some(config_file.to_string_lossy().into_owned());
    for session in loaded.sessions {
        if let Err(error) = core.register(session) {
            report.errors.push(SessionConfigErrorDto {
                index: 0,
                session_id: Some(error.session_id),
                field: None,
                message: format!("有效配置无法注册到会话列表：{}", error.message),
            });
        }
    }
    // The registry is authoritative for what the UI can act on. A config
    // entry that fails registration must not appear as a usable session here —
    // and since #62 the registry can also hold sessions the file never
    // described, so the listing is read with its provenance rather than from
    // the configurations alone.
    report.sessions = core
        .entries()
        .iter()
        .map(SessionEntry::config_dto)
        .collect();
    report
}

fn config_problem(
    file_status: ConfigFileStatusDto,
    config_path: Option<String>,
    message: String,
) -> ConfigReportDto {
    ConfigReportDto {
        file_status,
        config_path,
        sessions: Vec::new(),
        errors: vec![SessionConfigErrorDto {
            index: 0,
            session_id: None,
            field: Some("configPath".to_owned()),
            message,
        }],
    }
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
        /// A temp config at a path nothing else can be using.
        ///
        /// The clock alone is not enough: `SystemTime`'s resolution is the
        /// platform's, and on a CI runner it can be coarse enough for two of
        /// these tests (which run in parallel) to derive the *same* nanosecond
        /// and write over each other's file. That is not hypothetical — it is
        /// what a red `windows-build` run looked like: the registration test
        /// asserted one session was registered and then found none under its own
        /// id, which is exactly what the sibling `a_bad_entry_*` fixture
        /// produces, so it had loaded that test's file. A counter and the
        /// process id make the name unique by construction rather than by
        /// timing (the same reasoning as `RunId::mint`).
        fn new(contents: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};

            static SEQUENCE: AtomicU32 = AtomicU32::new(0);

            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos();
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "lch-t04-{}-{unique}-{sequence}.yaml",
                std::process::id()
            ));
            fs::write(&path, contents).expect("the temp config is writable");
            TempConfig(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    /// Two configs made back to back never share a path.
    ///
    /// Costs nothing and pins the property the CI failure turned on: that the
    /// name is unique by construction, not by timing. (It cannot *reproduce*
    /// that failure — this machine's clock is finer than the runner's, so the
    /// collision needs a coarse `SystemTime`, not a tight loop.)
    #[test]
    fn temporary_configs_never_share_a_path() {
        let configs: Vec<TempConfig> = (0..64)
            .map(|_| {
                TempConfig::new(
                    "sessions: []
",
                )
            })
            .collect();
        let paths: std::collections::HashSet<&Path> =
            configs.iter().map(|config| config.path()).collect();

        assert_eq!(
            paths.len(),
            configs.len(),
            "two temp configs shared a path, so one test can read another's file"
        );
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

        let report = register_configured(&core, Some(config.path()));

        assert_eq!(report.sessions.len(), 1);
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
        let config = TempConfig::new("sessions: []\n");
        let missing = config.path().with_file_name("missing.yaml");

        let report = register_configured(&core, Some(&missing));
        assert_eq!(report.file_status, ConfigFileStatusDto::Missing);
        assert_eq!(
            report.config_path.as_deref(),
            Some(missing.to_string_lossy().as_ref())
        );
        assert!(report.errors.is_empty());
        assert!(core.snapshots().is_empty());
    }

    #[test]
    fn no_config_location_registers_nothing() {
        let core = SessionCore::without_listener();
        let report = register_configured(&core, None);
        assert_eq!(report.file_status, ConfigFileStatusDto::Unavailable);
        assert_eq!(report.errors.len(), 1);
        assert!(core.snapshots().is_empty());
    }

    /// Broken YAML must not stop the app from opening — otherwise the user has
    /// no window in which to fix the file.
    #[test]
    fn a_broken_config_file_registers_nothing_instead_of_failing() {
        let config = TempConfig::new("sessions: [ this is not a list of sessions\n");
        let core = SessionCore::without_listener();

        let report = register_configured(&core, Some(config.path()));
        assert_eq!(report.file_status, ConfigFileStatusDto::Loaded);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 0);
        assert!(report.errors[0].message.contains("YAML"));
        assert!(report.errors[0].message.contains("line"));
        assert!(core.snapshots().is_empty());
    }

    #[test]
    fn a_config_read_error_reports_the_file_and_repair_context() {
        let config = TempConfig::new("sessions: []\n");
        let directory = config.path().parent().expect("temporary config parent");
        let core = SessionCore::without_listener();

        let report = register_configured(&core, Some(directory));

        assert_eq!(report.file_status, ConfigFileStatusDto::Unreadable);
        assert_eq!(
            report.config_path.as_deref(),
            Some(directory.to_string_lossy().as_ref())
        );
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 0);
        assert_eq!(report.errors[0].field.as_deref(), Some("configPath"));
        assert!(report.errors[0]
            .message
            .contains(&directory.display().to_string()));
        assert!(report.errors[0].message.contains("权限"));
        assert!(core.snapshots().is_empty());
    }

    /// One bad entry must not hide the valid ones (spec §13, T01's contract).
    #[test]
    fn a_bad_entry_does_not_hide_the_good_ones() {
        let config = TempConfig::new(
            "sessions:\n  - id: broken\n    name: Broken\n    type: nonsense\n  - id: two\n    name: Two\n    type: terminal\n    shell: powershell\n",
        );
        let core = SessionCore::without_listener();

        let report = register_configured(&core, Some(config.path()));
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.errors.len(), 1);
        assert!(core.snapshot("two").is_some());
        assert!(core.snapshot("broken").is_none());
    }

    #[test]
    fn config_report_keeps_valid_sessions_and_names_each_invalid_entry() {
        let config = TempConfig::new(
            "sessions:\n  - id: good\n    name: Good\n    type: terminal\n    shell: powershell\n  - id: bad\n    name: Bad\n    type: service\n    command: run\n    port: 0\n",
        );
        let core = SessionCore::without_listener();

        let report = register_configured(&core, Some(config.path()));

        assert_eq!(
            report.file_status,
            crate::config::ConfigFileStatusDto::Loaded
        );
        assert_eq!(
            report.config_path.as_deref(),
            Some(config.path().to_string_lossy().as_ref())
        );
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].id, "good");
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 2);
        assert_eq!(report.errors[0].session_id.as_deref(), Some("bad"));
        assert_eq!(report.errors[0].field.as_deref(), Some("port"));
        assert!(report.errors[0].message.contains("1-65535"));
        assert!(core.snapshot("good").is_some());
        assert!(core.snapshot("bad").is_none());
    }
}
