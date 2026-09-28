//! Run metadata on disk (`docs/LOGGING.md` §6).
//!
//! A run's metadata is what makes a log file *findable*: it names the session,
//! the run, the process, how the run ended and where its log went. D-011 puts
//! the same requirement on the layout — a person who opens the folder should
//! understand it without the Hub — so the file lives beside the log it
//! describes, under the same session and month, with the same run-tagged stem
//! (`crate::config::run_metadata_path`).
//!
//! ## The shape is the IPC shape
//!
//! The stored document is the [`RunRecord`] this app already publishes to the
//! frontend, serialized as JSON. `docs/LOGGING.md` §6 sketches the fields in
//! snake_case with a local offset; that sketch is illustrative ("suggested
//! metadata"), and reusing the wire shape here is a decision rather than an
//! accident: the run-history list the UI reads and the file on disk are then
//! the same bytes interpreted once, so they cannot drift into two answers
//! about the same run. Timestamps are RFC 3339 UTC, as they are on the wire —
//! a log outlives the time zone it was written in.
//!
//! ## Written once, at the end
//!
//! A record is written when the run ends, not when it starts. The fields that
//! matter (`ended_at`, `exit_code`, `log_file`) are only known then, and a
//! half-written record for a run the Hub died inside would be indistinguishable
//! from a completed one. A run in flight is in the snapshot Session Core
//! publishes; a run in history is one that finished.
//!
//! ## Reading is forgiving
//!
//! One unreadable file does not hide the rest of a session's history: the same
//! rule T01 applies to config entries applies here, because a run history that
//! disappears when one file is truncated is worse than one that says a file
//! could not be read.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::run_metadata_path;
use crate::session::runtime::RunRecord;

use super::error::{io_error, LogError};

/// A session's run history as it exists on disk, newest first.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunHistory {
    /// Completed runs, most recent start first.
    pub runs: Vec<RunRecord>,
    /// Entries that exist but could not be read, so a caller can say the list
    /// is incomplete rather than present it as the whole truth.
    pub unreadable: Vec<LogError>,
}

impl RunHistory {
    pub fn is_empty(&self) -> bool {
        self.runs.is_empty() && self.unreadable.is_empty()
    }

    /// The most recent run, if any.
    pub fn latest(&self) -> Option<&RunRecord> {
        self.runs.first()
    }
}

/// Write `record` under `metadata_root`, returning the file it was written to.
///
/// Written to a temporary name and renamed into place: a reader either sees
/// the previous file or the new one, never a half-written document it would
/// have to report as unreadable.
pub fn write_run(
    metadata_root: &Path,
    record: &RunRecord,
    utc_offset_secs: i32,
) -> Result<PathBuf, LogError> {
    let path = run_path(metadata_root, record, utc_offset_secs).ok_or_else(|| {
        LogError::new(
            "writing the run metadata",
            format!(
                "the run's start time or id cannot become a file name: run {} of {}",
                record.run_id, record.session_id
            ),
        )
    })?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| io_error("creating the metadata directory", parent, error))?;
    }

    let document = serde_json::to_vec_pretty(record)
        .map_err(|error| LogError::at("serializing the run metadata", &path, error.to_string()))?;

    let staging = path.with_extension("json.tmp");
    fs::write(&staging, &document)
        .map_err(|error| io_error("writing the run metadata", &staging, error))?;
    fs::rename(&staging, &path)
        .map_err(|error| io_error("publishing the run metadata", &path, error))?;
    Ok(path)
}

/// Read one run metadata file.
pub fn read_run(path: &Path) -> Result<RunRecord, LogError> {
    let bytes =
        fs::read(path).map_err(|error| io_error("reading the run metadata", path, error))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| LogError::at("parsing the run metadata", path, error.to_string()))
}

/// Every completed run of `session_id`, newest start first.
///
/// A session with no history is an empty history, not an error: a session that
/// has never run is the normal state of a fresh config (the same rule T01
/// applies to a missing config file).
pub fn run_history(metadata_root: &Path, session_id: &str) -> RunHistory {
    let mut history = RunHistory::default();

    for file in super::layout::session_run_files(metadata_root, "json", Some(session_id)) {
        match read_run(&file.path) {
            Ok(record) => history.runs.push(record),
            // The error already names the entry it was about and whether it
            // could not be read or could not be parsed.
            Err(error) => history.unreadable.push(error),
        }
    }

    // The record's own start time, not its file name: a file that was renamed
    // by hand still lists where it belongs. Runs from the same second — a
    // restart loop does that — are ordered by their ids, which are unique.
    history.runs.sort_by(|left, right| {
        let left_key = left.started_at.unix_secs().unwrap_or(0);
        let right_key = right.started_at.unix_secs().unwrap_or(0);
        right_key
            .cmp(&left_key)
            .then_with(|| right.run_id.as_str().cmp(left.run_id.as_str()))
    });
    history
}

fn run_path(metadata_root: &Path, record: &RunRecord, utc_offset_secs: i32) -> Option<PathBuf> {
    run_metadata_path(
        metadata_root,
        &record.session_id,
        record.started_at.unix_secs()?,
        utc_offset_secs,
        record.run_id.as_str(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EffectiveLogMode, LogSource};
    use crate::logging::test_support::TempDir;
    use crate::session::runtime::{RunId, Timestamp};
    use std::time::{Duration, UNIX_EPOCH};

    /// The fixture instant the path helpers use, so a timestamp that disagreed
    /// with them would show up here too.
    fn start_instant() -> Timestamp {
        Timestamp::from_system_time(UNIX_EPOCH + Duration::from_secs(1_790_213_415))
    }

    fn record(run_id: &str, log_file: Option<&str>) -> RunRecord {
        RunRecord {
            run_id: RunId::from_stored(run_id),
            session_id: "comfyui".to_owned(),
            started_at: start_instant(),
            ended_at: Some(start_instant()),
            exit_code: Some(0),
            pid: Some(1234),
            log_mode: EffectiveLogMode::Always,
            log_source: LogSource::Captured,
            log_file: log_file.map(str::to_owned),
        }
    }

    /// §6, and the acceptance criterion behind it: a persisted log must be
    /// findable *from the run metadata*. The stored record carries the log
    /// path, so a reader needs nothing but the metadata directory.
    #[test]
    fn a_written_record_carries_the_log_path_that_was_persisted() {
        let dir = TempDir::new();
        let record = record(
            "8f31",
            Some("logs/comfyui/2026-09/2026-09-24_09-30-15__run-8f31.log"),
        );

        let path = write_run(dir.path(), &record, 8 * 3600).expect("the record is written");
        let read_back = read_run(&path).expect("the record is readable");

        assert_eq!(read_back, record);
        assert!(path
            .to_string_lossy()
            .replace('\\', "/")
            .ends_with("comfyui/2026-09/2026-09-24_09-30-15__run-8f31.json"));
    }

    /// D-011: the file name says which session, which day and which run without
    /// anyone opening it.
    #[test]
    fn the_metadata_file_name_is_the_run_tag_and_the_time() {
        let dir = TempDir::new();

        let path = write_run(dir.path(), &record("8f31", None), 8 * 3600).expect("written");

        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("2026-09-24_09-30-15__run-8f31.json")
        );
    }

    #[test]
    fn a_session_that_has_never_run_has_an_empty_history() {
        let dir = TempDir::new();

        let history = run_history(dir.path(), "comfyui");

        assert!(history.is_empty());
        assert!(history.latest().is_none());
    }

    #[test]
    fn history_lists_completed_runs_newest_first() {
        let dir = TempDir::new();
        let mut older = record("aaaa", None);
        older.started_at = Timestamp::from_system_time(UNIX_EPOCH);
        let newer = record("bbbb", None);

        write_run(dir.path(), &older, 0).expect("older is written");
        write_run(dir.path(), &newer, 0).expect("newer is written");

        let history = run_history(dir.path(), "comfyui");

        assert_eq!(history.runs.len(), 2);
        assert_eq!(history.latest().expect("a run").run_id, newer.run_id);
        assert_eq!(history.runs[1].run_id, older.run_id);
    }

    /// One damaged entry must not hide the rest of a session's history, and the
    /// reader must not pretend the list is complete.
    #[test]
    fn an_unreadable_entry_is_reported_without_losing_the_others() {
        let dir = TempDir::new();
        write_run(dir.path(), &record("aaaa", None), 0).expect("a good record");
        let month = dir.join("comfyui/1970-01");
        fs::create_dir_all(&month).expect("the month folder is creatable");
        fs::write(month.join("broken__run-zzzz.json"), b"{ not json").expect("the broken file");

        let history = run_history(dir.path(), "comfyui");

        assert_eq!(history.runs.len(), 1, "the readable run is still listed");
        assert_eq!(history.unreadable.len(), 1);
        let failure = &history.unreadable[0];
        assert_eq!(failure.operation, "parsing the run metadata");
        let reported = failure.path.clone().unwrap_or_default().replace('\\', "/");
        assert!(
            reported.ends_with("comfyui/1970-01/broken__run-zzzz.json"),
            "the report must name the entry a reader could not use: {reported}"
        );
    }

    /// A run whose id or start time cannot become a file name is refused with a
    /// message rather than written somewhere unexpected.
    #[test]
    fn a_run_that_cannot_name_its_file_is_refused() {
        let dir = TempDir::new();
        let mut record = record("8f31", None);
        record.session_id = "../escape".to_owned();

        let error = write_run(dir.path(), &record, 0).expect_err("refused");

        assert_eq!(error.operation, "writing the run metadata");
        assert!(error.message.contains("cannot become a file name"));
    }

    /// The document is pretty-printed JSON: §6 asks for a small, structured,
    /// human-readable record, not a binary blob.
    #[test]
    fn the_stored_document_is_readable_json() {
        let dir = TempDir::new();
        let path = write_run(dir.path(), &record("8f31", None), 0).expect("written");

        let text = fs::read_to_string(&path).expect("the document is text");
        assert!(text.contains('\n'), "the document is formatted: {text}");
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(value["runId"], serde_json::json!("8f31"));
        assert_eq!(value["sessionId"], serde_json::json!("comfyui"));
        assert_eq!(value["exitCode"], serde_json::json!(0));
    }

    /// Nothing temporary is left behind: the staging file is what makes the
    /// write atomic, and it must not accumulate next to the real records.
    #[test]
    fn writing_leaves_no_staging_file_behind() {
        let dir = TempDir::new();
        let path = write_run(dir.path(), &record("8f31", None), 0).expect("written");

        let leftovers: Vec<_> = fs::read_dir(path.parent().expect("a month folder"))
            .expect("readable")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".tmp"))
            .collect();

        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
    }
}
