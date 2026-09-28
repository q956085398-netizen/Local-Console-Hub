//! Typed frontend events (`docs/MVP_IMPLEMENTATION_SPEC.md` §9).
//!
//! Session Core publishes lifecycle changes; it does not know who is
//! listening. Keeping the payloads as plain serializable values — rather than
//! Tauri `Emitter` calls scattered through the lifecycle code — is what lets
//! the whole state machine run in a test with no window, and is the mechanism
//! behind "session state is independent of main-window visibility": a hidden
//! window only means nobody is on the other end of the sink.
//!
//! Every session-scoped event carries its session id at the top level, as §9
//! requires; consumers must never have to infer which session an event is
//! about.

use serde::Serialize;

use super::runtime::{RunRecord, SessionRuntime};

/// `session-state-changed` — a session moved through the lifecycle.
pub const SESSION_STATE_CHANGED: &str = "session-state-changed";

/// `run-record-updated` — a run started, or ended with its result.
pub const RUN_RECORD_UPDATED: &str = "run-record-updated";

/// `app-summary-changed` — the app-wide counts changed.
pub const APP_SUMMARY_CHANGED: &str = "app-summary-changed";

/// App-wide session counts, for the tray summary and window chrome.
///
/// Deliberately counts rather than the sessions themselves: the summary is
/// meant to answer "is anything running?" without handing a duplicate of the
/// session list to a surface that should not own runtime truth.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSummary {
    pub total: usize,
    pub running: usize,
    pub error: usize,
}

/// Payload of [`SESSION_STATE_CHANGED`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStateChanged {
    pub session_id: String,
    /// The full post-transition snapshot, so a listener never has to hold
    /// stale state and apply a delta to catch up.
    pub runtime: SessionRuntime,
}

/// Payload of [`RUN_RECORD_UPDATED`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecordUpdated {
    pub session_id: String,
    pub run: RunRecord,
}

/// Payload of [`APP_SUMMARY_CHANGED`].
///
/// Carries no session id by design: it is not a session event, it is the
/// aggregate over all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSummaryChanged {
    pub summary: AppSummary,
}

/// One event Session Core can publish.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    StateChanged(SessionStateChanged),
    RunRecordUpdated(RunRecordUpdated),
    AppSummaryChanged(AppSummaryChanged),
}

impl SessionEvent {
    /// The IPC event name this is published under (§9).
    pub fn name(&self) -> &'static str {
        match self {
            SessionEvent::StateChanged(_) => SESSION_STATE_CHANGED,
            SessionEvent::RunRecordUpdated(_) => RUN_RECORD_UPDATED,
            SessionEvent::AppSummaryChanged(_) => APP_SUMMARY_CHANGED,
        }
    }

    /// The session this event is about, if it is session-scoped.
    pub fn session_id(&self) -> Option<&str> {
        match self {
            SessionEvent::StateChanged(event) => Some(&event.session_id),
            SessionEvent::RunRecordUpdated(event) => Some(&event.session_id),
            SessionEvent::AppSummaryChanged(_) => None,
        }
    }

    /// The payload this event is published with.
    ///
    /// One accessor rather than a `match` at each call site: the transport and
    /// the contract test serialize through the same path, so a payload that
    /// stopped matching its documented shape could not pass one and fail the
    /// other.
    pub fn payload(&self) -> serde_json::Value {
        match self {
            SessionEvent::StateChanged(inner) => serde_json::to_value(inner),
            SessionEvent::RunRecordUpdated(inner) => serde_json::to_value(inner),
            SessionEvent::AppSummaryChanged(inner) => serde_json::to_value(inner),
        }
        .unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EffectiveLogMode, EffectiveLogging, LogSource};
    use crate::session::runtime::{RunId, Timestamp};

    fn snapshot(session_id: &str) -> SessionRuntime {
        SessionRuntime::stopped(
            session_id,
            EffectiveLogging {
                mode: EffectiveLogMode::Always,
                source: LogSource::Captured,
                external_path: None,
            },
        )
    }

    fn every_event() -> Vec<SessionEvent> {
        vec![
            SessionEvent::StateChanged(SessionStateChanged {
                session_id: "comfyui".to_owned(),
                runtime: snapshot("comfyui"),
            }),
            SessionEvent::RunRecordUpdated(RunRecordUpdated {
                session_id: "comfyui".to_owned(),
                run: RunRecord {
                    run_id: RunId::mint(),
                    session_id: "comfyui".to_owned(),
                    started_at: Timestamp::now(),
                    ended_at: None,
                    exit_code: None,
                    pid: Some(1234),
                    log_mode: EffectiveLogMode::Always,
                    log_source: LogSource::Captured,
                    log_file: None,
                },
            }),
            SessionEvent::AppSummaryChanged(AppSummaryChanged {
                summary: AppSummary {
                    total: 1,
                    running: 0,
                    error: 0,
                },
            }),
        ]
    }

    /// The names are the wire contract in §9; a rename here silently breaks
    /// every listener, so the literals are asserted rather than the constants
    /// the implementation happens to use.
    #[test]
    fn event_names_are_the_ones_the_ipc_contract_names() {
        let names: Vec<&str> = every_event().iter().map(SessionEvent::name).collect();
        assert_eq!(
            names,
            vec![
                "session-state-changed",
                "run-record-updated",
                "app-summary-changed",
            ]
        );
    }

    /// §9: "Every session event includes the session id."
    #[test]
    fn session_scoped_events_expose_their_session_id() {
        for event in every_event() {
            match event {
                SessionEvent::AppSummaryChanged(_) => {
                    assert_eq!(
                        event.session_id(),
                        None,
                        "the app summary is not a session event"
                    );
                }
                _ => assert_eq!(
                    event.session_id(),
                    Some("comfyui"),
                    "{} lost its session id",
                    event.name()
                ),
            }
        }
    }

    /// The id must survive serialization at the top level of the payload, not
    /// only in the Rust value — that is what the frontend actually reads.
    #[test]
    fn a_serialized_session_event_carries_session_id_at_the_top_level() {
        for event in every_event() {
            let Some(expected) = event.session_id() else {
                continue;
            };
            let payload = event.payload();

            assert_eq!(
                payload["sessionId"],
                serde_json::json!(expected),
                "{} payload lacks sessionId: {payload}",
                event.name()
            );
        }
    }
}
