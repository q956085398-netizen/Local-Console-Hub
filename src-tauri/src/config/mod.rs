//! Config layer — schema, validation, defaults.
//!
//! Owned by T01 (#2, configuration schema and session model): YAML config
//! loading, Service/Interactive-Terminal session schemas, logging config,
//! per-session actionable validation, and app-data path resolution
//! ([`paths`]). One invalid session never hides unrelated valid sessions
//! (spec §13); no process is ever launched from here.

mod dto;
mod model;
mod paths;
mod validate;

pub use dto::{
    ConfigFileStatusDto, ConfigReportDto, EffectiveLoggingDto, SessionConfigDto,
    SessionConfigErrorDto,
};
pub use model::{
    EffectiveLogMode, EffectiveLogging, LogMode, LogSource, LoggingConfig, RawConfigFile,
    RawSessionConfig, SessionConfig, SessionType,
};
pub use paths::{
    local_utc_offset_secs, run_file_name, run_log_filename, run_log_path, run_metadata_path,
    session_log_dir, AppPaths,
};
// Crate-visible, and only under `cargo test`: the release guards in
// `crate::release` compare the bundle's product name against this. The
// app-data layout is documented for users to read; the constant itself does
// not need to be public API for that, and outside the test build nothing in
// the crate reads it through this path.
#[cfg(test)]
pub(crate) use paths::APP_DIR_NAME;
pub use validate::{validate_entry, SessionConfigError};

use std::collections::HashSet;
use std::path::Path;

/// Outcome of loading a config document: the valid sessions plus the
/// per-session problems. Both lists are present whenever the file parsed;
/// only unreadable files or broken YAML produce a file-level error entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadedConfig {
    /// Whether a file was read or no file exists yet.
    pub file_status: ConfigFileStatusDto,
    /// Validated sessions, in file order.
    pub sessions: Vec<SessionConfig>,
    /// Per-session (or file-level, `index == 0`) errors, in file order.
    pub errors: Vec<SessionConfigError>,
}

impl LoadedConfig {
    /// The report DTO exposed to the frontend (stable contract).
    pub fn to_dto(&self) -> ConfigReportDto {
        ConfigReportDto {
            file_status: self.file_status,
            config_path: None,
            sessions: self.sessions.iter().map(SessionConfigDto::from).collect(),
            errors: self
                .errors
                .iter()
                .cloned()
                .map(SessionConfigErrorDto::from)
                .collect(),
        }
    }
}

/// Parse and validate a config document.
///
/// - An empty/whitespace document loads as an empty session list (fresh
///   install with no config yet is not an error).
/// - Broken YAML yields a single file-level error entry.
/// - Each entry under `sessions:` is parsed and validated independently;
///   failures become per-entry errors while the remaining sessions load.
/// - Duplicate ids reject every entry after the first, keeping the
///   earliest definition valid and pointing the message at both.
pub fn load_from_str(text: &str) -> LoadedConfig {
    if text.trim().is_empty() {
        return LoadedConfig {
            file_status: ConfigFileStatusDto::Loaded,
            ..LoadedConfig::default()
        };
    }
    // A comment-only document deserializes as null; treat it like an
    // empty file rather than a parse error.
    let root: Option<RawConfigFile> = match serde_yaml::from_str(text) {
        Ok(root) => root,
        Err(err) => {
            return LoadedConfig {
                file_status: ConfigFileStatusDto::Loaded,
                sessions: Vec::new(),
                errors: vec![SessionConfigError {
                    index: 0,
                    session_id: None,
                    field: None,
                    message: format!("config file is not valid YAML: {err}"),
                }],
            };
        }
    };

    let entries = root.and_then(|root| root.sessions).unwrap_or_default();
    let mut sessions = Vec::new();
    let mut errors = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    for (offset, entry) in entries.iter().enumerate() {
        let index = offset + 1;
        let raw: RawSessionConfig = match serde_yaml::from_value(entry.clone()) {
            Ok(raw) => raw,
            Err(err) => {
                errors.push(SessionConfigError {
                    index,
                    session_id: None,
                    field: None,
                    message: format!("session entry {index} is invalid: {err}"),
                });
                continue;
            }
        };
        let id = raw.id.trim().to_owned();
        if !id.is_empty() && !seen_ids.insert(id.clone()) {
            errors.push(SessionConfigError {
                index,
                session_id: Some(id.clone()),
                field: Some("id".to_owned()),
                message: format!(
                    "duplicate session id `{id}` — first defined earlier in the file; \
                     session ids must be unique"
                ),
            });
            continue;
        }
        match validate_entry(index, &raw) {
            Ok(config) => sessions.push(config),
            Err(err) => errors.push(err),
        }
    }

    LoadedConfig {
        file_status: ConfigFileStatusDto::Loaded,
        sessions,
        errors,
    }
}

/// Load the config file at `path`.
///
/// A missing file is a fresh install, not an error: it loads as an empty
/// session list so the app can start and offer to create one. Read errors
/// (permissions and friends) are returned as [`std::io::Error`].
pub fn load_from_file(path: &Path) -> std::io::Result<LoadedConfig> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(load_from_str(&text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(LoadedConfig::default()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Unique existing directory for `cwd` validation tests; created under
    /// the system temp dir and best-effort removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let unique = format!(
                "lch-t01-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock after epoch")
                    .as_nanos()
            );
            let path = std::env::temp_dir().join(unique);
            std::fs::create_dir_all(&path).expect("create temp dir");
            TempDir(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn session_yaml(id: &str, session_type: &str, extra: &str) -> String {
        format!("id: {id}\nname: Session {id}\ntype: {session_type}\n{extra}")
    }

    /// A `sessions:` document whose items are `entries`.
    ///
    /// The `- ` marker already places an item's first key at column 5, so only
    /// the continuation lines need the four-space indent that lines the rest of
    /// the item up under it.
    fn config(entries: &[String]) -> String {
        let mut out = String::from("sessions:\n");
        for entry in entries {
            let mut lines = entry.lines();
            if let Some(first) = lines.next() {
                out.push_str("  - ");
                out.push_str(first);
                out.push('\n');
            }
            for line in lines {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }

    /// Indent the continuation lines of a YAML fragment by two spaces, so every
    /// key of the fragment lines up under its first one.
    fn indent_continuation(fragment: &str) -> String {
        fragment.replace('\n', "\n  ")
    }

    #[test]
    fn empty_document_is_an_empty_session_list() {
        for text in ["", "   \n", "# only a comment\n"] {
            let loaded = load_from_str(text);
            assert!(loaded.sessions.is_empty());
            assert!(loaded.errors.is_empty(), "unexpected errors for {text:?}");
        }
    }

    #[test]
    fn broken_yaml_reports_one_file_level_error() {
        let loaded = load_from_str("sessions: [ uh oh");
        assert!(loaded.sessions.is_empty());
        assert_eq!(loaded.errors.len(), 1);
        assert_eq!(loaded.errors[0].index, 0);
        assert!(loaded.errors[0].message.contains("YAML"));
    }

    #[test]
    fn duplicate_ids_reject_later_entries_and_keep_the_first() {
        let first = session_yaml("api", "service", "command: run api");
        let second = session_yaml("api", "service", "command: run api again");
        let loaded = load_from_str(&config(&[first, second]));
        assert_eq!(loaded.sessions.len(), 1, "first definition stays valid");
        assert_eq!(loaded.sessions[0].command.as_deref(), Some("run api"));
        assert_eq!(loaded.errors.len(), 1);
        assert_eq!(loaded.errors[0].session_id.as_deref(), Some("api"));
        assert!(loaded.errors[0]
            .message
            .contains("duplicate session id `api`"));
    }

    #[test]
    fn one_invalid_session_does_not_hide_valid_ones() {
        let good_dir = TempDir::new("good");
        let good = session_yaml(
            "good",
            "service",
            &format!("command: node server.js\ncwd: {}", good_dir.0.display()),
        );
        let bad_type = session_yaml("bad", "daemon", "command: whatever");
        let loaded = load_from_str(&config(&[good, bad_type]));
        assert_eq!(loaded.sessions.len(), 1, "valid session survives");
        assert_eq!(loaded.sessions[0].id, "good");
        assert_eq!(loaded.errors.len(), 1);
        assert_eq!(loaded.errors[0].session_id.as_deref(), Some("bad"));
        assert_eq!(loaded.errors[0].field.as_deref(), Some("type"));
        assert!(loaded.errors[0]
            .message
            .contains("unsupported type `daemon`"));
    }

    #[test]
    fn service_without_command_is_actionable() {
        let entry = session_yaml("nosvc", "service", "port: 8000");
        let loaded = load_from_str(&config(&[entry]));
        assert!(loaded.sessions.is_empty());
        assert_eq!(loaded.errors[0].field.as_deref(), Some("command"));
        assert!(loaded.errors[0].message.contains("require `command`"));
    }

    #[test]
    fn terminal_without_shell_is_actionable() {
        let entry = session_yaml("noterm", "terminal", "initial_command: ls");
        let loaded = load_from_str(&config(&[entry]));
        assert!(loaded.sessions.is_empty());
        assert_eq!(loaded.errors[0].field.as_deref(), Some("shell"));
        assert!(loaded.errors[0].message.contains("require `shell`"));
    }

    #[test]
    fn port_zero_is_rejected_and_out_of_range_ports_fail_per_session() {
        let zero = session_yaml("svc0", "service", "command: run\nport: 0");
        let loaded = load_from_str(&config(&[zero]));
        assert_eq!(loaded.errors[0].field.as_deref(), Some("port"));
        assert!(loaded.errors[0].message.contains("1-65535"));

        let huge = session_yaml("svcbig", "service", "command: run\nport: 70000");
        let other = session_yaml("other", "service", "command: run");
        let loaded = load_from_str(&config(&[huge, other]));
        assert_eq!(
            loaded.sessions.len(),
            1,
            "out-of-range port must not hide the sibling session"
        );
        assert_eq!(loaded.sessions[0].id, "other");
        assert_eq!(loaded.errors.len(), 1);
        assert!(loaded.errors[0].message.contains("70000"));
    }

    #[test]
    fn malformed_and_non_http_urls_are_rejected() {
        for bad in ["not a url", "ftp://127.0.0.1/x"] {
            let entry = session_yaml("urlsvc", "service", &format!("command: run\nurl: {bad}"));
            let loaded = load_from_str(&config(&[entry]));
            assert_eq!(loaded.errors.len(), 1, "url `{bad}` must be rejected");
            assert_eq!(loaded.errors[0].field.as_deref(), Some("url"));
        }
        let good = session_yaml(
            "urlsvc",
            "service",
            "command: run\nurl: https://127.0.0.1:8443/x",
        );
        let loaded = load_from_str(&config(&[good]));
        assert!(loaded.errors.is_empty());
        assert!(loaded.sessions[0]
            .url
            .as_deref()
            .expect("url kept")
            .starts_with("https://"));
    }

    #[test]
    fn missing_working_directory_is_rejected_with_the_path() {
        let entry = session_yaml(
            "ghost",
            "service",
            "command: run\ncwd: Z:/definitely/not/here/lch-t01",
        );
        let loaded = load_from_str(&config(&[entry]));
        assert!(loaded.sessions.is_empty());
        assert_eq!(loaded.errors[0].field.as_deref(), Some("cwd"));
        assert!(loaded.errors[0]
            .message
            .contains("Z:/definitely/not/here/lch-t01"));
    }

    #[test]
    fn cross_type_fields_are_rejected() {
        let service_with_shell =
            session_yaml("mixed", "service", "command: run\nshell: powershell");
        let loaded = load_from_str(&config(&[service_with_shell]));
        assert_eq!(loaded.errors[0].field.as_deref(), Some("shell"));
        assert!(loaded.errors[0].message.contains("type: terminal"));

        let terminal_with_command =
            session_yaml("mixed2", "terminal", "shell: powershell\ncommand: run");
        let loaded = load_from_str(&config(&[terminal_with_command]));
        assert_eq!(loaded.errors[0].field.as_deref(), Some("command"));
        assert!(loaded.errors[0].message.contains("type: service"));

        // `purpose` and `close_impact` are *not* cross-type: they describe any
        // session in words and carry no runtime meaning (D-027), so a terminal
        // keeping the rest of the service-only fields rejected is the whole
        // rule.
        let terminal_with_port =
            session_yaml("mixed3", "terminal", "shell: powershell\nport: 8000");
        let loaded = load_from_str(&config(&[terminal_with_port]));
        assert_eq!(loaded.errors[0].field.as_deref(), Some("port"));
        assert!(loaded.errors[0].message.contains("type: service"));
    }

    #[test]
    fn a_terminal_may_carry_purpose_and_close_impact() {
        let entry = session_yaml(
            "term",
            "terminal",
            "shell: powershell\npurpose: 日常交互终端，跑一次性命令与 REPL。\n\
             close_impact: 仅结束本终端；不会停止其它受管服务。",
        );
        let loaded = load_from_str(&config(&[entry]));
        assert!(
            loaded.errors.is_empty(),
            "unexpected errors: {:?}",
            loaded.errors
        );
        let term = &loaded.sessions[0];
        assert_eq!(
            term.purpose.as_deref(),
            Some("日常交互终端，跑一次性命令与 REPL。")
        );
        assert_eq!(
            term.close_impact.as_deref(),
            Some("仅结束本终端；不会停止其它受管服务。")
        );
    }

    #[test]
    fn a_terminal_that_omits_purpose_or_close_impact_keeps_them_absent() {
        let entry = session_yaml("term", "terminal", "shell: powershell");
        let loaded = load_from_str(&config(&[entry]));
        assert!(
            loaded.errors.is_empty(),
            "unexpected errors: {:?}",
            loaded.errors
        );
        assert_eq!(loaded.sessions[0].purpose, None);
        assert_eq!(loaded.sessions[0].close_impact, None);
    }

    #[test]
    fn an_empty_terminal_purpose_is_actionable() {
        let entry = session_yaml("term", "terminal", "shell: powershell\npurpose: '   '");
        let loaded = load_from_str(&config(&[entry]));
        assert!(loaded.sessions.is_empty());
        assert_eq!(loaded.errors[0].field.as_deref(), Some("purpose"));
    }

    #[test]
    fn unknown_fields_are_per_session_errors_not_file_errors() {
        let typo = session_yaml("typo", "service", "command: run\nprot: 8000");
        let other = session_yaml("other", "service", "command: run");
        let loaded = load_from_str(&config(&[typo, other]));
        assert_eq!(loaded.sessions.len(), 1);
        assert_eq!(loaded.sessions[0].id, "other");
        assert_eq!(loaded.errors.len(), 1);
        assert!(loaded.errors[0].message.contains("unknown field `prot`"));
    }

    #[test]
    fn unsafe_session_ids_are_rejected() {
        for bad in ["has space", "../escape", "\"\"", "-leading-dash"] {
            let entry = session_yaml(bad, "service", "command: run");
            let loaded = load_from_str(&config(&[entry]));
            assert_eq!(
                loaded.errors.len(),
                1,
                "id {bad} should produce exactly one error, got {:?}",
                loaded.errors
            );
        }
    }

    #[test]
    fn logging_defaults_and_auto_resolution() {
        // Terminal without a logging block: no persistence (LOGGING.md §1.2).
        let term = session_yaml("term", "terminal", "shell: powershell");
        let loaded = load_from_str(&config(&[term]));
        assert!(loaded.errors.is_empty());
        let logging = &loaded.sessions[0].logging;
        assert_eq!(logging.mode, EffectiveLogMode::Off);
        assert_eq!(logging.source, LogSource::None);

        // Service without a logging block: on_error with captured output.
        let svc = session_yaml("svc", "service", "command: run");
        let loaded = load_from_str(&config(&[svc]));
        let logging = &loaded.sessions[0].logging;
        assert_eq!(logging.mode, EffectiveLogMode::OnError);
        assert_eq!(logging.source, LogSource::Captured);

        // Service referencing an application-owned log: link, don't capture.
        let ext = session_yaml(
            "ext",
            "service",
            "command: run\nlogging:\n  mode: auto\n  source: external\n  path: ./data/app.log",
        );
        let loaded = load_from_str(&config(&[ext]));
        assert!(loaded.errors.is_empty());
        let logging = &loaded.sessions[0].logging;
        assert_eq!(logging.mode, EffectiveLogMode::Always);
        assert_eq!(logging.source, LogSource::External);
        assert_eq!(logging.external_path.as_deref(), Some("./data/app.log"));
    }

    #[test]
    fn contradictory_logging_combinations_are_rejected() {
        let cases: &[(&str, &str)] = &[
            // captured output that is never persisted
            ("mode: off\nsource: captured", "contradict"),
            // external link that persists nothing
            ("mode: off\nsource: external\npath: a.log", "contradict"),
            // external link that writes Hub-captured output
            (
                "mode: on_error\nsource: external\npath: a.log",
                "contradict",
            ),
            // persisting mode without a source
            ("mode: always", "needs `source: captured`"),
            // persistence mode with nothing to persist
            ("mode: always\nsource: none", "nothing to persist"),
            // external without a path
            ("mode: always\nsource: external", "requires `logging.path`"),
            // path without external source
            (
                "mode: always\nsource: captured\npath: a.log",
                "source: external",
            ),
        ];
        for (block, expected) in cases {
            let entry = session_yaml(
                "badlog",
                "service",
                &format!("command: run\nlogging:\n  {}", indent_continuation(block)),
            );
            let loaded = load_from_str(&config(&[entry]));
            assert_eq!(
                loaded.errors.len(),
                1,
                "combination `{block}` should produce exactly one error"
            );
            assert!(
                loaded.errors[0].message.contains(expected),
                "message {:?} should mention {expected} for `{block}`",
                loaded.errors[0].message
            );
        }
    }

    #[test]
    fn valid_logging_combinations_load() {
        let cases = [
            "mode: always\nsource: captured",
            "mode: on_error\nsource: captured",
            "mode: manual\nsource: captured",
            "mode: off\nsource: none",
            "mode: always\nsource: external\npath: a.log",
        ];
        for block in cases {
            let entry = session_yaml(
                "oklog",
                "service",
                &format!("command: run\nlogging:\n  {}", indent_continuation(block)),
            );
            let loaded = load_from_str(&config(&[entry]));
            assert!(
                loaded.errors.is_empty(),
                "combination `{block}` should be valid, got {:?}",
                loaded.errors
            );
        }
    }

    #[test]
    fn terminal_cannot_force_captured_auto_logging() {
        let entry = session_yaml(
            "termlog",
            "terminal",
            "shell: powershell\nlogging:\n  mode: auto\n  source: captured",
        );
        let loaded = load_from_str(&config(&[entry]));
        assert_eq!(loaded.errors.len(), 1);
        assert!(loaded.errors[0].message.contains("terminal"));
    }

    #[test]
    fn report_dto_keeps_valid_sessions_and_errors_together() {
        let good = session_yaml("good", "service", "command: run");
        let bad = session_yaml("bad", "service", "");
        let report = load_from_str(&config(&[good, bad])).to_dto();
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].id, "good");
        assert_eq!(report.sessions[0].session_type, "service");
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 2);
    }

    #[test]
    fn missing_config_file_is_a_fresh_install() {
        let root = TempDir::new("missing-config");
        let missing = root.0.join("config.yaml");
        let loaded = load_from_file(&missing).expect("missing file is not an error");
        assert!(loaded.sessions.is_empty());
        assert!(loaded.errors.is_empty());
        assert_eq!(loaded.to_dto().file_status, ConfigFileStatusDto::Missing);

        let empty = root.0.join("empty.yaml");
        std::fs::write(&empty, "\n # intentionally empty\n").expect("write empty config");
        let loaded = load_from_file(&empty).expect("empty config is readable");
        assert!(loaded.sessions.is_empty());
        assert!(loaded.errors.is_empty());
        assert_eq!(loaded.to_dto().file_status, ConfigFileStatusDto::Loaded);
    }

    #[test]
    fn development_fixture_loads_cleanly() {
        let fixture = include_str!("../../../fixtures/dev-config.yaml");
        let loaded = load_from_str(fixture);
        assert!(
            loaded.errors.is_empty(),
            "fixture must load without errors: {:?}",
            loaded.errors
        );
        let ids: Vec<&str> = loaded.sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["sillytavern", "comfyui", "devshell"]);
        let st = &loaded.sessions[0];
        assert_eq!(st.session_type, SessionType::Service);
        assert_eq!(st.port, Some(8000));
        // mode: auto + source: captured on a service resolves to on_error.
        assert_eq!(st.logging.mode, EffectiveLogMode::OnError);
        let shell = loaded.sessions.last().expect("terminal entry");
        assert_eq!(shell.session_type, SessionType::Terminal);
        assert_eq!(shell.shell.as_deref(), Some("powershell"));
        assert_eq!(shell.logging.mode, EffectiveLogMode::Off);
    }

    /// The T11 verification fixture loads cleanly, and still covers the
    /// policies the manual checklist walks.
    ///
    /// The point is not that the file parses — it is that a checklist row
    /// cannot quietly lose its subject. Every assertion below names a row of
    /// `docs/VERIFICATION.md`; deleting the session, or changing its logging
    /// policy to something already covered, fails here rather than being
    /// discovered by whoever runs the checklist next.
    #[test]
    fn verification_fixture_loads_cleanly_and_covers_the_matrix() {
        let fixture = include_str!("../../../fixtures/verification-config.yaml");
        let loaded = load_from_str(fixture);
        assert!(
            loaded.errors.is_empty(),
            "verification fixture must load without errors: {:?}",
            loaded.errors
        );

        let ids: Vec<&str> = loaded.sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "svc-listening",
                "svc-fails",
                "svc-external",
                "term-pwsh",
                "term-manual",
            ]
        );

        let by_id = |id: &str| {
            loaded
                .sessions
                .iter()
                .find(|session| session.id == id)
                .unwrap_or_else(|| panic!("`{id}` is in the fixture"))
        };

        // "a captured service writes one file per run" — `always` is the only
        // policy that writes as output arrives, so at least one service must
        // carry it (LOGGING.md §3).
        let always = by_id("svc-listening");
        assert_eq!(always.logging.mode, EffectiveLogMode::Always);
        assert_eq!(always.logging.source, LogSource::Captured);
        assert_eq!(
            always.port,
            Some(28900),
            "the readiness row needs a service that really listens"
        );

        // "on_error preserves pre-error context" (LOGGING.md 场景 C).
        assert_eq!(by_id("svc-fails").logging.mode, EffectiveLogMode::OnError);

        // "an external log is linked, not duplicated" (LOGGING.md §11): the
        // external source only means anything together with its path.
        let external = by_id("svc-external");
        assert_eq!(external.logging.source, LogSource::External);
        let external_path = external
            .logging
            .external_path
            .as_deref()
            .expect("an `external` session names the file it points at");
        assert!(
            !external_path.is_empty(),
            "an `external` session with no path has nothing to point at"
        );
        // Both of these were real defects in the first version of this fixture,
        // and neither is visible by reading it: `%LOCALAPPDATA%\...` looks like
        // a path and is eleven characters of directory name, and `access.log`
        // looks relative and resolves against the app's working directory.
        assert!(
            !external_path.contains('%'),
            "`{external_path}` is not expanded: the path is taken literally \
             (`resolve_logging`), so this names a directory called `%LOCALAPPDATA%`"
        );
        assert!(
            Path::new(external_path).is_absolute(),
            "`{external_path}` must be absolute: a relative path resolves against the \
             app's working directory, which is not the session's `cwd`"
        );

        // "a plain interactive terminal creates no log file" (LOGGING.md
        // 场景 A) and the `manual` policy the Logs tab's controls act on.
        assert_eq!(by_id("term-pwsh").logging.mode, EffectiveLogMode::Off);
        assert_eq!(by_id("term-manual").logging.mode, EffectiveLogMode::Manual);

        // "close impact is visible before destructive actions" (UI_STYLE_GUIDE
        // §5/§13): a terminal states why it exists and what stopping it costs,
        // exactly as a service does (D-027). `term-pwsh` carries the two
        // sentences the V2 terminal reference shows, so the checklist's
        // terminal rows reproduce `assets/ui/ui-v2-terminal.png` in the live
        // window instead of falling back to `关闭影响 —`.
        let terminal = by_id("term-pwsh");
        assert_eq!(
            terminal.purpose.as_deref(),
            Some("日常交互终端，跑一次性命令与 REPL。")
        );
        assert_eq!(
            terminal.close_impact.as_deref(),
            Some("仅结束本终端；不会停止其它受管服务。")
        );

        // Every command and shell must run on a machine with nothing
        // installed, or the checklist stops being repeatable. `powershell` is
        // the one tool this repo may assume (D-001: Windows-first).
        for session in &loaded.sessions {
            match session.session_type {
                SessionType::Service => {
                    let command = session.command.as_deref().expect("a service has a command");
                    assert!(
                        command.starts_with("powershell "),
                        "`{}` needs something besides Windows itself: {command}",
                        session.id
                    );
                }
                SessionType::Terminal => {
                    assert_eq!(
                        session.shell.as_deref(),
                        Some("powershell"),
                        "`{}` must use the shell every Windows has",
                        session.id
                    );
                }
            }
        }
    }
}
