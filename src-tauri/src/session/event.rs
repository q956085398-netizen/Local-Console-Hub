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
use super::terminal::OutputBatch;

/// `session-state-changed` — a session moved through the lifecycle.
pub const SESSION_STATE_CHANGED: &str = "session-state-changed";

/// `run-record-updated` — a run started, or ended with its result.
pub const RUN_RECORD_UPDATED: &str = "run-record-updated";

/// `app-summary-changed` — the app-wide counts changed.
pub const APP_SUMMARY_CHANGED: &str = "app-summary-changed";

/// `terminal-output` — a batch of a run's terminal output (T07).
///
/// Batched rather than one event per read: §9 asks for exactly that, and the
/// relay behind it (`crate::session::terminal`) is where the batching happens.
pub const TERMINAL_OUTPUT: &str = "terminal-output";

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

impl AppSummary {
    /// Count `snapshots`.
    ///
    /// The one place that decides what is counted, so the window's numbers and
    /// the tray's cannot drift: both read their counts through here rather than
    /// each keeping a rule of its own. "Failed" and `error` are the same count
    /// under two names — the wire name came first (spec §9), and the tray's
    /// wording is the display side of it.
    pub fn of(snapshots: &[SessionRuntime]) -> Self {
        use super::state::SessionStatus;

        AppSummary {
            total: snapshots.len(),
            running: snapshots
                .iter()
                .filter(|runtime| runtime.status == SessionStatus::Running)
                .count(),
            error: snapshots
                .iter()
                .filter(|runtime| runtime.status == SessionStatus::Error)
                .count(),
        }
    }
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
    TerminalOutput(TerminalOutput),
}

impl SessionEvent {
    /// The IPC event name this is published under (§9).
    pub fn name(&self) -> &'static str {
        match self {
            SessionEvent::StateChanged(_) => SESSION_STATE_CHANGED,
            SessionEvent::RunRecordUpdated(_) => RUN_RECORD_UPDATED,
            SessionEvent::AppSummaryChanged(_) => APP_SUMMARY_CHANGED,
            SessionEvent::TerminalOutput(_) => TERMINAL_OUTPUT,
        }
    }

    /// The session this event is about, if it is session-scoped.
    pub fn session_id(&self) -> Option<&str> {
        match self {
            SessionEvent::StateChanged(event) => Some(&event.session_id),
            SessionEvent::RunRecordUpdated(event) => Some(&event.session_id),
            SessionEvent::AppSummaryChanged(_) => None,
            SessionEvent::TerminalOutput(event) => Some(&event.session_id),
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
            SessionEvent::TerminalOutput(inner) => serde_json::to_value(inner),
        }
        .unwrap_or(serde_json::Value::Null)
    }
}

/// Payload of [`TERMINAL_OUTPUT`].
///
/// The bytes travel base64-encoded, not as text: a terminal's output is UTF-8
/// *plus* ANSI/VT sequences, and a chunk boundary can fall inside a multi-byte
/// character. Decoding a chunk as text would turn one such boundary into two
/// replacement characters, which is exactly the Unicode acceptance criterion
/// failing on a technicality. The view hands the decoded bytes to the terminal
/// renderer, which decodes UTF-8 across chunk boundaries the way a terminal
/// must.
///
/// `start`/`end` are the byte range of the run's stream this batch covers, and
/// `generation` names the run: see `crate::session::terminal` for why a view
/// needs both to append live output to a replayed scrollback without
/// duplicating or skipping anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutput {
    pub session_id: String,
    pub generation: u64,
    pub start: u64,
    pub end: u64,
    /// The batch's bytes, base64-encoded (standard alphabet, padded).
    pub data: String,
}

impl TerminalOutput {
    /// The payload for one batch of a session's output.
    ///
    /// Built from the batch rather than from loose offsets so the range and
    /// the bytes cannot disagree.
    pub fn from_batch(session_id: impl Into<String>, batch: &OutputBatch) -> Self {
        use base64::Engine;

        TerminalOutput {
            session_id: session_id.into(),
            generation: batch.generation,
            start: batch.start,
            end: batch.end,
            data: base64::engine::general_purpose::STANDARD.encode(&batch.bytes),
        }
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

    fn with_status(
        session_id: &str,
        status: crate::session::state::SessionStatus,
    ) -> SessionRuntime {
        let mut runtime = snapshot(session_id);
        runtime.status = status;
        runtime
    }

    /// The counting rule, asserted where it now lives: `running` and `error`
    /// over the whole list, every other state counted by nobody, and a total
    /// that is the list's length rather than the sum.
    #[test]
    fn a_summary_counts_running_and_error_over_every_session() {
        use crate::session::state::{SessionStatus, ALL_STATUSES};

        let snapshots: Vec<SessionRuntime> = ALL_STATUSES
            .iter()
            .map(|status| with_status("s", *status))
            .collect();

        let summary = AppSummary::of(&snapshots);

        assert_eq!(summary.total, ALL_STATUSES.len());
        assert_eq!(summary.running, 1);
        assert_eq!(summary.error, 1);
        assert!(
            summary.running + summary.error < summary.total,
            "four states are counted by neither number"
        );
        assert_eq!(
            AppSummary::of(&[with_status("a", SessionStatus::Running)]).running,
            1
        );
    }

    /// A Hub with no sessions has nothing running and nothing failed — the
    /// reading the tray shows as an idle summary.
    #[test]
    fn an_empty_list_summarizes_to_zero() {
        assert_eq!(AppSummary::of(&[]), AppSummary::default());
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
            SessionEvent::TerminalOutput(TerminalOutput::from_batch(
                "comfyui",
                &OutputBatch {
                    generation: 1,
                    start: 0,
                    end: 4,
                    bytes: b"$ ls".to_vec(),
                },
            )),
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
                "terminal-output",
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

    /// The "Unicode works" acceptance criterion starts at this boundary: a
    /// batch travels as bytes, so a read that splits a multi-byte character —
    /// routine for a console host, not exotic — cannot corrupt it. A payload
    /// carrying text would fail here on the first keystroke of a CJK name.
    #[test]
    fn terminal_output_survives_a_chunk_boundary_inside_a_character() {
        use base64::Engine;

        // "你好" split after its first byte: the case a per-chunk UTF-8 decode
        // would render as two replacement characters.
        let split_character = &"你好".as_bytes()[..1];
        let payload = TerminalOutput::from_batch(
            "term",
            &OutputBatch {
                generation: 7,
                start: 0,
                end: 1,
                bytes: split_character.to_vec(),
            },
        );

        let value = serde_json::to_value(&payload).expect("payload serializes");
        assert_eq!(value["generation"], serde_json::json!(7));
        assert_eq!(value["start"], serde_json::json!(0));
        assert_eq!(value["end"], serde_json::json!(1));
        assert_eq!(value["sessionId"], serde_json::json!("term"));

        let data = value["data"].as_str().expect("data is a string");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .expect("data is base64");
        assert_eq!(
            decoded, split_character,
            "the byte must survive the wire unchanged"
        );
    }
}
