//! Stable, frontend-facing DTOs for the config layer.
//!
//! camelCase serialization matches the IPC conventions established by
//! `crate::ipc` in T00. T04 (Session Core) and T06 (UI shell) consume
//! these shapes without re-mapping; string *values* (`"on_error"`,
//! `"service"`, ...) deliberately keep the YAML vocabulary so config,
//! backend and UI share one set of words.

use serde::Serialize;

use super::model::{EffectiveLogging, SessionConfig};
use super::validate::SessionConfigError;

/// How the application found the user's config file at startup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigFileStatusDto {
    /// No file exists yet; the application can start as a fresh install.
    #[default]
    Missing,
    /// The file was read, even if its contents contain validation errors.
    Loaded,
    /// A file exists but could not be read.
    Unreadable,
    /// The application could not resolve its config directory.
    Unavailable,
}

/// Effective logging state of a session after `auto` resolution — the
/// thing the UI must display ("is persistence active, and where does it
/// write?", `docs/LOGGING.md` §1.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveLoggingDto {
    /// `"off" | "always" | "on_error" | "manual"` — `auto` never appears.
    pub mode: String,
    /// `"none" | "captured" | "external"`.
    pub source: String,
    /// Application-owned log path; present only for `source: "external"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_path: Option<String>,
}

/// One validated session as exposed to the frontend.
///
/// The type-owned fields are present for their own type only (validation
/// guarantees the other side is absent). `purpose` and `closeImpact` belong to
/// both types (D-027) and reach the header and the Details card the same way
/// either way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfigDto {
    pub id: String,
    pub name: String,
    /// `"service" | "terminal"`.
    pub session_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_impact: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_command: Option<String>,
    /// `"internal" | "window"` — where this entry is displayed (#66).
    ///
    /// Always present, unlike the optional fields above: `"internal"` is not
    /// an absence but a fact about the entry (the command runs on a console
    /// the Hub owns), and a UI that has to infer it from a missing key would
    /// be one more place the two display modes could be confused.
    pub display: String,
    /// `"managed" | "independent"` — who ends this entry's run (#66).
    pub lifecycle: String,
    pub logging: EffectiveLoggingDto,
    /// Whether this session lives only in memory (#62): created from the
    /// window rather than loaded from the config file, removable once it has
    /// ended, and never restored after the app exits.
    ///
    /// Omitted when false — a configured session's payload does not change,
    /// and every reader keeps the meaning it already had for it.
    #[serde(skip_serializing_if = "is_false")]
    pub temporary: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl SessionConfigDto {
    /// The same DTO, marked as describing a temporary session.
    ///
    /// A builder rather than a conversion from the session layer: the flag is
    /// registry provenance, not a property of the configuration, so the
    /// config layer is told which one it is describing instead of having to
    /// ask a layer above it.
    pub fn temporary(mut self) -> Self {
        self.temporary = true;
        self
    }
}

/// One actionable configuration problem for the frontend to surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfigErrorDto {
    /// 1-based entry number, or 0 for file-level errors.
    pub index: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}

/// Result of loading a config file: the valid sessions plus per-session
/// errors. One broken entry never removes the others.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigReportDto {
    pub file_status: ConfigFileStatusDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    pub sessions: Vec<SessionConfigDto>,
    pub errors: Vec<SessionConfigErrorDto>,
}

impl From<&EffectiveLogging> for EffectiveLoggingDto {
    fn from(logging: &EffectiveLogging) -> Self {
        EffectiveLoggingDto {
            mode: logging.mode.as_str().to_owned(),
            source: logging.source.as_str().to_owned(),
            external_path: logging.external_path.clone(),
        }
    }
}

impl From<&SessionConfig> for SessionConfigDto {
    fn from(config: &SessionConfig) -> Self {
        SessionConfigDto {
            id: config.id.clone(),
            name: config.name.clone(),
            session_type: config.session_type.as_str().to_owned(),
            cwd: config
                .cwd
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            command: config.command.clone(),
            url: config.url.clone(),
            port: config.port,
            purpose: config.purpose.clone(),
            close_impact: config.close_impact.clone(),
            shell: config.shell.clone(),
            initial_command: config.initial_command.clone(),
            display: config.display.as_str().to_owned(),
            lifecycle: config.lifecycle.as_str().to_owned(),
            logging: EffectiveLoggingDto::from(&config.logging),
            // Provenance is the registry's to state, not the configuration's:
            // a config loaded from the file is never a temporary session, and
            // the one place that registers a temporary one says so.
            temporary: false,
        }
    }
}

impl From<SessionConfigError> for SessionConfigErrorDto {
    fn from(error: SessionConfigError) -> Self {
        SessionConfigErrorDto {
            index: error.index,
            session_id: error.session_id,
            field: error.field,
            message: error.message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{
        DisplayMode, EffectiveLogMode, LifecycleOwner, LogSource, SessionType,
    };
    use super::*;

    fn sample_session() -> SessionConfig {
        SessionConfig {
            id: "sillytavern".to_owned(),
            name: "SillyTavern".to_owned(),
            session_type: SessionType::Service,
            cwd: Some("D:/Tools/SillyTavern".into()),
            command: Some("node server.js".to_owned()),
            url: Some("http://127.0.0.1:8000/".to_owned()),
            port: Some(8000),
            purpose: Some("聊天前端".to_owned()),
            close_impact: Some("可停止；网页会失联".to_owned()),
            shell: None,
            initial_command: None,
            display: DisplayMode::Internal,
            lifecycle: LifecycleOwner::Managed,
            logging: EffectiveLogging {
                mode: EffectiveLogMode::OnError,
                source: LogSource::Captured,
                external_path: None,
            },
        }
    }

    #[test]
    fn session_dto_serializes_to_the_camel_case_contract() {
        let dto = SessionConfigDto::from(&sample_session());
        let value = serde_json::to_value(&dto).expect("dto serializes");
        for key in [
            "id",
            "name",
            "sessionType",
            "cwd",
            "command",
            "url",
            "port",
            "purpose",
            "closeImpact",
            "display",
            "lifecycle",
            "logging",
        ] {
            assert!(value.get(key).is_some(), "missing {key} in {value}");
        }
        assert!(
            value.get("session_type").is_none() && value.get("close_impact").is_none(),
            "snake_case leaked into {value}"
        );
        assert_eq!(value["sessionType"], "service");
        assert_eq!(value["display"], "internal");
        assert_eq!(value["lifecycle"], "managed");
        assert_eq!(value["logging"]["mode"], "on_error");
        assert_eq!(value["logging"]["source"], "captured");
    }

    /// The two #66 dimensions reach the window as their own values, and
    /// "internal" is stated rather than omitted: a UI that had to read a
    /// missing key as the Hub-internal mode would be guessing at the one thing
    /// the field exists to say.
    #[test]
    fn the_display_and_lifecycle_dimensions_are_always_stated() {
        let mut session = sample_session();
        session.display = DisplayMode::Window;
        session.lifecycle = LifecycleOwner::Independent;

        let value = serde_json::to_value(SessionConfigDto::from(&session)).expect("dto serializes");

        assert_eq!(value["display"], "window");
        assert_eq!(value["lifecycle"], "independent");
    }

    #[test]
    fn absent_optional_fields_are_omitted_not_nulled() {
        let mut session = sample_session();
        session.session_type = SessionType::Terminal;
        session.command = None;
        session.url = None;
        session.port = None;
        session.purpose = None;
        session.close_impact = None;
        session.shell = Some("powershell".to_owned());
        let value = serde_json::to_value(SessionConfigDto::from(&session)).expect("dto serializes");
        for absent in ["command", "url", "port", "purpose", "closeImpact"] {
            assert!(value.get(absent).is_none(), "{absent} should be omitted");
        }
        assert_eq!(value["shell"], "powershell");
        assert_eq!(value["sessionType"], "terminal");
    }

    #[test]
    fn external_logging_keeps_its_path_in_the_dto() {
        let logging = EffectiveLogging {
            mode: EffectiveLogMode::Always,
            source: LogSource::External,
            external_path: Some("D:/Tools/app/data/access.log".to_owned()),
        };
        let value =
            serde_json::to_value(EffectiveLoggingDto::from(&logging)).expect("dto serializes");
        assert_eq!(value["mode"], "always");
        assert_eq!(value["source"], "external");
        assert_eq!(value["externalPath"], "D:/Tools/app/data/access.log");
    }

    /// A temporary session says so (`#62`); a configured one does not, so its
    /// payload is exactly what every existing reader already expects.
    #[test]
    fn only_a_temporary_session_carries_the_flag() {
        let configured = serde_json::to_value(SessionConfigDto::from(&sample_session()))
            .expect("dto serializes");
        assert!(
            configured.get("temporary").is_none(),
            "a configured session must not claim to be temporary: {configured}"
        );

        let temporary = serde_json::to_value(SessionConfigDto::from(&sample_session()).temporary())
            .expect("dto serializes");
        assert_eq!(temporary["temporary"], serde_json::json!(true));
        assert_eq!(temporary["id"], serde_json::json!("sillytavern"));
    }

    #[test]
    fn error_dto_uses_camel_case_session_id() {
        let error = SessionConfigError {
            index: 2,
            session_id: Some("api".to_owned()),
            field: Some("port".to_owned()),
            message: "port 0 is not usable".to_owned(),
        };
        let value =
            serde_json::to_value(SessionConfigErrorDto::from(error)).expect("dto serializes");
        assert_eq!(value["sessionId"], "api");
        assert_eq!(value["index"], 2);
    }

    #[test]
    fn config_report_serializes_file_status_and_path() {
        let report = ConfigReportDto {
            file_status: ConfigFileStatusDto::Unreadable,
            config_path: Some("D:/Users/example/LocalConsoleHub/config.yaml".to_owned()),
            sessions: Vec::new(),
            errors: Vec::new(),
        };

        let value = serde_json::to_value(report).expect("report serializes");

        assert_eq!(value["fileStatus"], "unreadable");
        assert_eq!(
            value["configPath"],
            "D:/Users/example/LocalConsoleHub/config.yaml"
        );
        assert!(value.get("file_status").is_none());
    }
}
