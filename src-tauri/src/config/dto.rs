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
    pub logging: EffectiveLoggingDto,
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
            logging: EffectiveLoggingDto::from(&config.logging),
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
    use super::super::model::{EffectiveLogMode, LogSource, SessionType};
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
            "logging",
        ] {
            assert!(value.get(key).is_some(), "missing {key} in {value}");
        }
        assert!(
            value.get("session_type").is_none() && value.get("close_impact").is_none(),
            "snake_case leaked into {value}"
        );
        assert_eq!(value["sessionType"], "service");
        assert_eq!(value["logging"]["mode"], "on_error");
        assert_eq!(value["logging"]["source"], "captured");
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
}
