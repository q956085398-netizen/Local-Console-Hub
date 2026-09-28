//! Raw configuration schema and the validated session model.
//!
//! The raw types mirror `docs/MVP_IMPLEMENTATION_SPEC.md` §4: a config file
//! is a list of session entries, each parsed on its own so one broken entry
//! never hides the others. Type-specific and semantic checks live in
//! [`super::validate`]. Nothing in this module launches a process (T01
//! out-of-scope).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Session kind from the implementation spec: `service` or `terminal`.
///
/// The raw YAML `type` key is intentionally parsed as a plain string so an
/// unsupported type becomes a per-session error instead of failing the whole
/// file (spec §13: one bad session must not hide unrelated valid sessions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionType {
    Service,
    Terminal,
}

impl SessionType {
    /// Literal used in YAML config and DTOs.
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionType::Service => "service",
            SessionType::Terminal => "terminal",
        }
    }
}

/// Persistence policy for a session (`docs/LOGGING.md` §3).
///
/// `Auto` is resolved away during validation; it never survives into
/// [`EffectiveLogging`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogMode {
    Off,
    Always,
    OnError,
    Manual,
    Auto,
}

/// Where persisted log content comes from (`docs/LOGGING.md` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogSource {
    None,
    Captured,
    External,
}

/// A log mode after `auto` resolution: the only values the rest of the
/// app (and the UI "is persistence active?" display) needs to understand.
///
/// Deserialization exists for the run metadata T05 writes: the same values
/// appear in the config, on the IPC surface and in the run record on disk, so
/// they round-trip rather than being re-mapped into a third vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveLogMode {
    Off,
    Always,
    OnError,
    Manual,
}

impl EffectiveLogMode {
    /// Literal used in DTOs; matches the YAML vocabulary.
    pub fn as_str(&self) -> &'static str {
        match self {
            EffectiveLogMode::Off => "off",
            EffectiveLogMode::Always => "always",
            EffectiveLogMode::OnError => "on_error",
            EffectiveLogMode::Manual => "manual",
        }
    }
}

impl LogSource {
    /// Literal used in YAML and DTOs.
    pub fn as_str(&self) -> &'static str {
        match self {
            LogSource::None => "none",
            LogSource::Captured => "captured",
            LogSource::External => "external",
        }
    }
}

/// Raw `logging:` block exactly as written by the user.
///
/// `None` fields mean "not specified" (as opposed to explicitly written
/// `none` for the source) — validation and `auto` resolution need that
/// distinction to avoid silently overriding user intent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    pub mode: Option<LogMode>,
    pub source: Option<LogSource>,
    /// Application-owned log file; only valid together with
    /// `source: external` (`docs/LOGGING.md` §11).
    pub path: Option<String>,
}

/// One unparsed-but-shape-checked session entry.
///
/// This is the union of service and terminal fields; which fields are
/// allowed for which type is decided by [`super::validate`], again so a
/// type mismatch stays a per-session error. Unknown fields are rejected
/// so config typos surface as actionable messages instead of being
/// silently ignored.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSessionConfig {
    /// Stable, unique, filesystem-safe session identifier.
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// `service` or `terminal`; validated later, not by serde.
    pub r#type: String,
    /// Working directory; must exist at validation time when present.
    pub cwd: Option<String>,
    /// Service: command line to launch.
    pub command: Option<String>,
    /// Service: URL opened by "Open URL"; http/https only.
    pub url: Option<String>,
    /// Service: TCP port shown in the session header.
    pub port: Option<u16>,
    /// Service: short description of what this session is for.
    pub purpose: Option<String>,
    /// Service: what closing/stopping it will cost the user.
    pub close_impact: Option<String>,
    /// Terminal: shell executable, e.g. `powershell`.
    pub shell: Option<String>,
    /// Terminal: command run once the shell is ready.
    pub initial_command: Option<String>,
    /// Logging policy; defaults are derived per session type.
    pub logging: Option<LoggingConfig>,
}

/// Top-level document shape: `{ sessions: [ ... ] }`.
///
/// Entries stay untyped [`serde_yaml::Value`]s here so each one is
/// deserialized (and can fail) independently.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfigFile {
    pub sessions: Option<Vec<serde_yaml::Value>>,
}

/// Logging configuration after validation and `auto` resolution — the
/// "effective logging state" the UI must be able to show (LOGGING.md §1.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveLogging {
    pub mode: EffectiveLogMode,
    pub source: LogSource,
    /// Path of the application-owned log; present only for
    /// `source: external`.
    #[serde(default)]
    pub external_path: Option<String>,
}

/// A fully validated session configuration.
///
/// Only service fields or only terminal fields are populated, depending on
/// [`SessionConfig::session_type`]; the cross-type fields are guaranteed
/// absent by validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionConfig {
    pub id: String,
    pub name: String,
    pub session_type: SessionType,
    pub cwd: Option<PathBuf>,
    /// Service only.
    pub command: Option<String>,
    /// Service only; normalized absolute-or-as-written http(s) URL string.
    pub url: Option<String>,
    /// Service only; 1..=65535.
    pub port: Option<u16>,
    /// Service only.
    pub purpose: Option<String>,
    /// Service only.
    pub close_impact: Option<String>,
    /// Terminal only.
    pub shell: Option<String>,
    /// Terminal only.
    pub initial_command: Option<String>,
    pub logging: EffectiveLogging,
}
