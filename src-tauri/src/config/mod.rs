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

pub use dto::{ConfigReportDto, EffectiveLoggingDto, SessionConfigDto, SessionConfigErrorDto};
pub use model::{
    EffectiveLogMode, EffectiveLogging, LogMode, LogSource, LoggingConfig, RawConfigFile,
    RawSessionConfig, SessionConfig, SessionType,
};
pub use paths::{run_log_filename, run_log_path, session_log_dir, AppPaths};
pub use validate::{validate_entry, SessionConfigError};

use std::collections::HashSet;
use std::path::Path;

/// Outcome of loading a config document: the valid sessions plus the
/// per-session problems. Both lists are present whenever the file parsed;
/// only unreadable files or broken YAML produce a file-level error entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoadedConfig {
    /// Validated sessions, in file order.
    pub sessions: Vec<SessionConfig>,
    /// Per-session (or file-level, `index == 0`) errors, in file order.
    pub errors: Vec<SessionConfigError>,
}

impl LoadedConfig {
    /// The report DTO exposed to the frontend (stable contract).
    pub fn to_dto(&self) -> ConfigReportDto {
        ConfigReportDto {
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
        return LoadedConfig::default();
    }
    // A comment-only document deserializes as null; treat it like an
    // empty file rather than a parse error.
    let root: Option<RawConfigFile> = match serde_yaml::from_str(text) {
        Ok(root) => root,
        Err(err) => {
            return LoadedConfig {
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

    LoadedConfig { sessions, errors }
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
    use std::path::PathBuf;

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
            ("mode: always", "needs `source`"),
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
        let missing = std::env::temp_dir().join("lch-t01-definitely-missing-config.yaml");
        let loaded = load_from_file(&missing).expect("missing file is not an error");
        assert!(loaded.sessions.is_empty());
        assert!(loaded.errors.is_empty());
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
}
