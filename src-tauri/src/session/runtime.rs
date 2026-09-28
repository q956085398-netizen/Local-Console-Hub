//! Session runtime snapshot and run records
//! (`docs/MVP_IMPLEMENTATION_SPEC.md` §4, `docs/LOGGING.md` §6).
//!
//! These are the data contracts Session Core publishes: what a session looks
//! like right now ([`SessionRuntime`]) and what one finished start produced
//! ([`RunRecord`]). They are deliberately plain data — no handles, no locks —
//! so a snapshot can be handed to the UI, written to run metadata, or asserted
//! on in a test without touching the machinery that produced it.

use std::fmt;
use std::time::SystemTime;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::config::{EffectiveLogMode, EffectiveLogging, LogSource};
use crate::logging::BufferSummary;

use super::state::SessionStatus;

/// A wall-clock instant, serialized as RFC 3339 UTC for the IPC surface.
///
/// A newtype rather than a bare `SystemTime` because the serialized form is a
/// contract the frontend reads, and `SystemTime` has no `Serialize` that would
/// produce it.
///
/// T05 reuses this shape for the run metadata written to disk. That is a
/// decision, not an accident: the frontend's run-history list and the file on
/// disk are then one shape interpreted once, so they cannot drift into two
/// answers about the same run. It departs from `docs/LOGGING.md` §6's sketch,
/// which is illustrative snake_case with a local offset; UTC is what makes two
/// machines' records comparable, and `docs/DECISIONS.md` D-016 records the
/// choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp(SystemTime);

impl Timestamp {
    /// The instant now.
    pub fn now() -> Self {
        Timestamp(SystemTime::now())
    }

    /// Wrap an existing instant.
    pub fn from_system_time(instant: SystemTime) -> Self {
        Timestamp(instant)
    }

    /// Whole seconds since the Unix epoch, as the config path helpers take.
    /// `None` only for a time before the epoch, which no run can have.
    pub fn unix_secs(&self) -> Option<i64> {
        self.0
            .duration_since(SystemTime::UNIX_EPOCH)
            .ok()
            .map(|elapsed| elapsed.as_secs() as i64)
    }
}

impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use chrono::{DateTime, SecondsFormat};
        match self
            .unix_secs()
            .and_then(|secs| DateTime::from_timestamp(secs, 0))
        {
            Some(instant) => {
                serializer.serialize_str(&instant.to_rfc3339_opts(SecondsFormat::Secs, true))
            }
            // A pre-epoch instant has no RFC 3339 form to offer; serialize the
            // absence rather than a wrong time.
            None => serializer.serialize_none(),
        }
    }
}

/// Reading back a stored timestamp, for the run metadata T05 writes.
///
/// Only the form above is accepted. A record whose timestamp is anything else
/// is refused rather than guessed at: a run history with a wrong start time is
/// worse than one entry reported as unreadable.
impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use chrono::DateTime;
        use serde::de::Error;

        let text = Option::<String>::deserialize(deserializer)?.ok_or_else(|| {
            D::Error::custom("a stored timestamp must be an RFC 3339 instant, not null")
        })?;
        let instant = DateTime::parse_from_rfc3339(&text)
            .map_err(|error| D::Error::custom(format!("`{text}` is not RFC 3339: {error}")))?;
        let seconds = instant.timestamp();
        if seconds < 0 {
            // `Timestamp` cannot serialize a pre-epoch instant, so one cannot
            // have been written by this app.
            return Err(D::Error::custom(format!(
                "`{text}` is before the Unix epoch"
            )));
        }
        Ok(Timestamp(
            SystemTime::UNIX_EPOCH
                + std::time::Duration::new(seconds as u64, instant.timestamp_subsec_nanos()),
        ))
    }
}

/// What one finished (or ongoing) managed start produced
/// (`docs/LOGGING.md` §6).
///
/// `ended_at` and `exit_code` stay `None` while the run is live, which is what
/// lets the same type serve the run-history list and the in-flight run.
///
/// Deserialization exists for the run metadata T05 writes under
/// `metadata/<session>/<month>/`: the same document is both what the frontend
/// receives and what the Hub reads back, so a run history cannot be two
/// different shapes depending on who asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub run_id: RunId,
    pub session_id: String,
    pub started_at: Timestamp,
    pub ended_at: Option<Timestamp>,
    pub exit_code: Option<u32>,
    pub pid: Option<u32>,
    pub log_mode: EffectiveLogMode,
    pub log_source: LogSource,
    pub log_file: Option<String>,
}

/// A failure Session Core wants the UI to be able to show without parsing a
/// string (`docs/DEVELOPMENT.md` §9: a message names the operation).
///
/// This is the session layer's own shape rather than a `process::ProcessError`
/// so a process-layer error can be recorded without leaking that layer's type
/// through the whole UI contract.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionErrorInfo {
    /// The operation that failed, e.g. `start` or `stop`.
    pub operation: String,
    /// Human-readable, actionable message.
    pub message: String,
}

/// Everything the UI needs to render one session right now
/// (`docs/MVP_IMPLEMENTATION_SPEC.md` §4).
///
/// A snapshot, not a handle: it owns no locks and can be cloned out of Session
/// Core, which is what lets the same value reach the window, the tray and a
/// test without any of them reaching back into lifecycle machinery.
///
/// §4's "at minimum" list is covered, including the **terminal buffer**: T05
/// added [`BufferSummary`], which is the part of the buffer a snapshot can
/// carry without becoming a handle — how much scrollback exists and whether
/// any of it was discarded. The output itself is read on demand
/// (`crate::session::core::SessionCore::terminal_buffer`), because a snapshot
/// is copied into every event and a session with a full scrollback must not
/// make every state change expensive (spec §14).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRuntime {
    pub session_id: String,
    pub status: SessionStatus,
    /// Set while a run is starting or running.
    pub pid: Option<u32>,
    /// The run this snapshot describes; `None` before the first start.
    pub run_id: Option<RunId>,
    pub started_at: Option<Timestamp>,
    /// Known once a run has ended, including an unexpected exit.
    pub exit_code: Option<u32>,
    /// Whether a PTY is currently attached to this session. T04 records the
    /// flag; T07 is what attaches one.
    pub pty_attached: bool,
    pub logging: EffectiveLogging,
    /// The in-memory scrollback this session is holding (`docs/LOGGING.md`
    /// §8). Present whatever the logging policy is: `mode: off` decides what
    /// reaches the disk, never whether the session has a buffer.
    pub buffer: BufferSummary,
    pub last_error: Option<SessionErrorInfo>,
}

impl SessionRuntime {
    /// The snapshot a session has before anything has been started.
    ///
    /// `logging` comes from the validated config, because "is this being
    /// logged?" is answerable before a run exists — the config decides the
    /// policy, the run only obeys it.
    pub fn stopped(session_id: impl Into<String>, logging: EffectiveLogging) -> Self {
        SessionRuntime {
            session_id: session_id.into(),
            status: SessionStatus::Stopped,
            pid: None,
            run_id: None,
            started_at: None,
            exit_code: None,
            pty_attached: false,
            logging,
            buffer: BufferSummary::default(),
            last_error: None,
        }
    }
}

/// Identifier of one managed run, unique within the app and safe to use as a
/// file-name component (`docs/LOGGING.md` §5, §6).
///
/// Minted by Session Core on every start; carried by [`RunRecord`] and by the
/// run's log file (`config::run_log_filename`). The two constraints that shape
/// the format are that it must satisfy
/// [`config::is_filesystem_safe_component`] — it becomes part of a log file
/// name — and that a run must never reuse a previous run's id, including a run
/// from an earlier process, because that would make two runs' log files
/// indistinguishable.
///
/// [`config::is_filesystem_safe_component`]: crate::config::is_filesystem_safe_component
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RunId(String);

impl RunId {
    /// Mint an id for a run starting now.
    ///
    /// Timestamp first, counter second. The timestamp is what keeps a restarted
    /// app from handing out ids a previous process already used — a counter
    /// alone resets to zero on every launch and would collide with the run
    /// history already on disk. The counter is what separates runs that start
    /// inside the same millisecond, which happens whenever restarts are driven
    /// in a loop rather than by a person.
    ///
    /// Hex keeps it a single filesystem-safe token with no separators to
    /// escape, and well inside the 64-byte component limit.
    pub fn mint() -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static SEQUENCE: AtomicU32 = AtomicU32::new(0);

        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis())
            .unwrap_or(0);
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        RunId(format!("{millis:x}{sequence:04x}"))
    }

    /// The id as it appears in run metadata and log file names.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The id a stored record carried — the counterpart of [`RunId::as_str`].
    ///
    /// Deliberately unchecked: an id read back from disk is a fact about a run
    /// that already happened, and the place that turns an id into a file name
    /// ([`crate::config::run_log_filename`]) applies the safety rule again
    /// anyway. Refusing here would only mean a record that cannot be read at
    /// all rather than one whose log path is rejected.
    pub fn from_stored(value: impl Into<String>) -> Self {
        RunId(value.into())
    }
}

impl Serialize for RunId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for RunId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(RunId(String::deserialize(deserializer)?))
    }
}

impl fmt::Display for RunId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::run_log_filename;
    use std::collections::HashSet;
    use std::time::{Duration, UNIX_EPOCH};

    fn captured_logging() -> EffectiveLogging {
        EffectiveLogging {
            mode: EffectiveLogMode::Always,
            source: LogSource::Captured,
            external_path: None,
        }
    }

    /// The instant `config::paths`'s own fixture uses, so a timestamp that
    /// disagreed with the path helpers would show up here too.
    fn start_instant() -> Timestamp {
        Timestamp::from_system_time(UNIX_EPOCH + Duration::from_secs(1_790_213_415))
    }

    #[test]
    fn a_timestamp_serializes_as_rfc_3339_utc() {
        let value = serde_json::to_value(start_instant()).expect("timestamp serializes");
        assert_eq!(value, serde_json::json!("2026-09-24T01:30:15Z"));
    }

    /// `docs/LOGGING.md` §6 names the metadata keys; the UI and the on-disk run
    /// metadata both read this shape.
    #[test]
    fn a_run_record_serializes_to_the_documented_metadata_shape() {
        let record = RunRecord {
            run_id: RunId("8f31".to_owned()),
            session_id: "comfyui".to_owned(),
            started_at: start_instant(),
            ended_at: None,
            exit_code: None,
            pid: Some(12345),
            log_mode: EffectiveLogMode::Always,
            log_source: LogSource::Captured,
            log_file: Some("logs/comfyui/2026-09/run-8f31.log".to_owned()),
        };

        let value = serde_json::to_value(&record).expect("run record serializes");
        for key in [
            "runId",
            "sessionId",
            "startedAt",
            "endedAt",
            "exitCode",
            "pid",
            "logMode",
            "logSource",
            "logFile",
        ] {
            assert!(value.get(key).is_some(), "missing {key} in {value}");
        }
        // Enum values keep the YAML vocabulary from LOGGING.md §6.
        assert_eq!(value["logMode"], serde_json::json!("always"));
        assert_eq!(value["logSource"], serde_json::json!("captured"));
        assert_eq!(value["sessionId"], serde_json::json!("comfyui"));
        assert_eq!(value["runId"], serde_json::json!("8f31"));
    }

    /// A live run has no end yet; JSON `null` is how the UI tells "still
    /// running" from "ended with code 0".
    #[test]
    fn a_live_run_reports_no_end_and_no_exit_code() {
        let record = RunRecord {
            run_id: RunId("8f31".to_owned()),
            session_id: "comfyui".to_owned(),
            started_at: start_instant(),
            ended_at: None,
            exit_code: None,
            pid: Some(12345),
            log_mode: EffectiveLogMode::Always,
            log_source: LogSource::Captured,
            log_file: None,
        };
        let value = serde_json::to_value(&record).expect("run record serializes");
        assert!(
            value["endedAt"].is_null(),
            "endedAt should be null: {value}"
        );
        assert!(
            value["exitCode"].is_null(),
            "exitCode should be null: {value}"
        );
    }

    /// Before a start there is no run to describe. The UI must not find a PID
    /// or run id here to render as if something were alive.
    #[test]
    fn a_session_that_has_never_started_claims_no_run() {
        let runtime = SessionRuntime::stopped("comfyui", captured_logging());
        let value = serde_json::to_value(&runtime).expect("snapshot serializes");

        assert_eq!(value["status"], serde_json::json!("stopped"));
        assert!(value["pid"].is_null(), "pid should be null: {value}");
        assert!(value["runId"].is_null(), "runId should be null: {value}");
        assert!(
            value["startedAt"].is_null(),
            "startedAt should be null: {value}"
        );
        assert!(
            value["lastError"].is_null(),
            "lastError should be null: {value}"
        );
        assert_eq!(value["sessionId"], serde_json::json!("comfyui"));
    }

    /// The id becomes a file-name component, so the function that actually
    /// applies the safety rule is the oracle here — `None` means the logging
    /// layer would have no file to write, which is the failure that matters.
    #[test]
    fn minted_run_ids_are_usable_as_log_file_name_components() {
        for _ in 0..1_000 {
            let id = RunId::mint();
            assert!(
                run_log_filename(1_790_213_415, 8 * 3600, id.as_str()).is_some(),
                "run id {id:?} produced no log file name"
            );
        }
    }

    /// Runs minted back to back land in the same millisecond, and a fresh
    /// process must not reuse the ids a previous one handed out. Both are the
    /// same requirement from the caller's side: two runs never share an id.
    #[test]
    fn minted_run_ids_are_unique() {
        let mut seen = HashSet::new();
        for _ in 0..10_000 {
            let id = RunId::mint();
            assert!(
                seen.insert(id.as_str().to_owned()),
                "run id {id:?} was minted twice"
            );
        }
    }
}
