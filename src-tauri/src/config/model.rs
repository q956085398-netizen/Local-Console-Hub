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

/// Where a configured application is displayed (#66, spec #59 decision 8).
///
/// The two modes are not two skins of one runtime. `Internal` hosts the
/// command on a console this app owns — a supervised service, or an
/// interactive terminal — and the Hub window is the only place its output
/// appears. `Window` lets the application keep the window *and* the console it
/// provides for itself: the Hub starts it, does not embed it, and has nothing
/// to render for it.
///
/// The mode belongs to the entry, not to the Hub window, because it says what
/// the *application* provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// Hub-internal: the command runs under a Hub-owned console.
    Internal,
    /// Standalone: the application's own window and console are kept.
    Window,
}

impl DisplayMode {
    /// Literal used in YAML and DTOs.
    pub fn as_str(&self) -> &'static str {
        match self {
            DisplayMode::Internal => "internal",
            DisplayMode::Window => "window",
        }
    }

    /// Whether this entry is displayed in its own window.
    pub fn is_window(self) -> bool {
        self == DisplayMode::Window
    }
}

impl Default for DisplayMode {
    /// Absent means Hub-internal, which is what every configuration written
    /// before #66 meant (spec #59 decision 8: "旧配置继续维持原有行为").
    fn default() -> Self {
        DisplayMode::Internal
    }
}

/// Who ends an application's run (#66, spec #59 decision 8).
///
/// A second dimension rather than a third display mode: the same standalone
/// window can be left to manage itself, or be brought under the Hub's stop
/// rules (`docs/DECISIONS.md` D-007). Display mode answers "where does it
/// appear", lifecycle ownership answers "who ends it", and neither can be
/// derived from the other — nor from whether output happens to be captured or
/// whether a port happens to be configured (spec #59 decision 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleOwner {
    /// The Hub's rules apply: Stop All and Exit include the run, and stop,
    /// force stop and restart are the same operations they are for a service.
    Managed,
    /// The application owns its run. `Stop All` and `Exit` leave it alone, and
    /// the Hub refuses to end it (`docs/DECISIONS.md` D-034).
    Independent,
}

impl LifecycleOwner {
    /// Literal used in YAML and DTOs.
    pub fn as_str(&self) -> &'static str {
        match self {
            LifecycleOwner::Managed => "managed",
            LifecycleOwner::Independent => "independent",
        }
    }

    /// What an entry gets when it does not say.
    ///
    /// An entry the Hub hosts (`internal`) was always the Hub's to end, so the
    /// answer for it is unchanged. An entry that keeps its own window is a
    /// third-party application the user runs *through* the Hub; ending it as a
    /// side effect of leaving the Hub is the silent kill spec #59 decision 12
    /// forbids, so the default there is `independent` and management is
    /// something the user turns on explicitly.
    pub fn default_for(display: DisplayMode) -> Self {
        match display {
            DisplayMode::Internal => LifecycleOwner::Managed,
            DisplayMode::Window => LifecycleOwner::Independent,
        }
    }

    /// Whether the Hub's stop rules apply to this run.
    pub fn is_managed(self) -> bool {
        self == LifecycleOwner::Managed
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

impl LogMode {
    /// Literal used in YAML and DTOs; matches the serde rename above and
    /// [`EffectiveLogMode::as_str`], so the mode vocabulary has one spelling
    /// wherever it is written.
    pub fn as_str(&self) -> &'static str {
        match self {
            LogMode::Off => "off",
            LogMode::Always => "always",
            LogMode::OnError => "on_error",
            LogMode::Manual => "manual",
            LogMode::Auto => "auto",
        }
    }
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
    /// Either type: short description of what this session is for.
    pub purpose: Option<String>,
    /// Either type: what closing/stopping it will cost the user.
    pub close_impact: Option<String>,
    /// Terminal: shell executable, e.g. `powershell`.
    pub shell: Option<String>,
    /// Terminal: command run once the shell is ready.
    pub initial_command: Option<String>,
    /// Service: `internal` (Hub-hosted console) or `window` (the
    /// application's own window and console). Absent means `internal`, which
    /// is what every entry written before #66 meant.
    pub display: Option<DisplayMode>,
    /// Service: `managed` (Stop All and Exit include the run) or
    /// `independent`. Absent means [`LifecycleOwner::default_for`] the display
    /// mode.
    pub lifecycle: Option<LifecycleOwner>,
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
/// The fields that only one type owns are populated for that type alone and
/// guaranteed absent on the other by validation. `purpose` and `close_impact`
/// belong to both types (D-027): they describe a session in words, so a
/// terminal states why it exists and what stopping it costs just as a service
/// does.
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
    /// Either type; free-text, no runtime meaning.
    pub purpose: Option<String>,
    /// Either type; free-text, no runtime meaning.
    pub close_impact: Option<String>,
    /// Terminal only.
    pub shell: Option<String>,
    /// Terminal only.
    pub initial_command: Option<String>,
    /// Service only; `internal` for everything that does not say (#66).
    pub display: DisplayMode,
    /// Service only; whether the Hub's stop rules apply to a run (#66).
    pub lifecycle: LifecycleOwner,
    pub logging: EffectiveLogging,
}

impl SessionConfig {
    /// Whether this entry keeps the application's own window and console.
    pub fn is_window(&self) -> bool {
        self.display.is_window()
    }

    /// Whether the Hub owns this session's run lifecycle.
    pub fn is_managed(&self) -> bool {
        self.lifecycle.is_managed()
    }
}
