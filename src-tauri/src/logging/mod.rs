//! Logging layer — terminal buffer, captured logs, external logs.
//!
//! Owned by T05 (#6, selective logging core). Keeps the three concepts from
//! `docs/LOGGING.md` separate: bounded in-memory terminal buffer, optional
//! hub-captured persistent log, and external application-owned logs.
//! Defaults: interactive terminals persist nothing; stdin is never
//! persisted; on_error retains a bounded pre-failure buffer. Logging code
//! does not decide session lifecycle.
//!
//! ## What lives here
//!
//! - [`buffer`] — the bounded in-memory scrollback every session has, and the
//!   bounded pre-failure context an `on_error` run keeps.
//! - [`plan`] — the effective logging state: what a session's policy resolves
//!   to for one run, and the [`plan::LogStatus`] the UI reads.
//! - [`run_log`] — one run's file, and the decision to have one at all.
//! - [`metadata`] — the run record written beside its log, which is how a
//!   persisted log is found again; a record survives the sweep that takes its
//!   log, and reports that the file is gone.
//! - [`retention`] — the rules that stop a session's logs growing forever.
//! - [`error`] — what the layer reports when a file cannot be written.
//!
//! ## Boundaries this layer keeps
//!
//! "Logging code does not decide session lifecycle"
//! (`docs/MVP_IMPLEMENTATION_SPEC.md` §3). Nothing here starts, stops or
//! supervises anything: a run log is *handed* to it, output is *appended* to
//! it, and an outcome is *reported* to it. Equally, nothing here decides
//! whether a session should be running.
//!
//! "Terminal display is not the same as persisted logs" (D-004). The buffer
//! and the log file are different types with different lifetimes, and no
//! method makes one imply the other.
//!
//! ## Where the bytes come from
//!
//! The layer does not open a process, read a pipe or know what a PTY is. It
//! exposes [`OutputSink`] and [`pump`]: whoever owns the byte stream hands
//! batches to a sink, and the sink decides what a batch means — append it to
//! the session's buffer, append it to the run's log. Session Core implements
//! that sink for supervised processes; T07 feeds the same sink from a PTY.
//!
//! Batches are read in [`BATCH_BYTES`] units rather than line by line: spec
//! §9 requires terminal output to reach the UI in batches, and a batch per
//! read is what stops a chatty process from turning into an event per line.
//!
//! ## Defaults chosen here
//!
//! `docs/LOGGING.md` §9 leaves the retention numbers to implementation. The
//! values this layer ships are [`buffer::DEFAULT_BUFFER_LIMITS`] (256 KiB /
//! 5000 lines of scrollback), [`buffer::DEFAULT_PRE_FAILURE_LIMITS`] (64 KiB /
//! 2000 lines of pre-failure context), [`run_log::DEFAULT_LOG_LIMITS`] (16 MiB
//! per run file) and [`retention::DEFAULT_RETENTION`] (30 days, 256 MiB per
//! session). None of them is a config field: changing them changes how much
//! disk a user's sessions may use, and that is a decision to make deliberately
//! rather than a knob to discover (`docs/DEVELOPMENT.md` §8).

pub mod buffer;
pub mod clock;
pub mod error;
pub mod layout;
pub mod metadata;
pub mod plan;
pub mod retention;
pub mod run_log;

// Visible to the whole crate in test builds: the session layer's tests write
// real log files too, and every one of them needs a scratch root that is not
// the user's `%LOCALAPPDATA%`.
#[cfg(test)]
pub(crate) mod test_support;

use std::io::Read;

pub use buffer::{
    BufferLimits, BufferSummary, BufferedChunk, Stream, TerminalBuffer, DEFAULT_BUFFER_LIMITS,
    DEFAULT_PRE_FAILURE_LIMITS,
};
pub use error::LogError;
pub use layout::{session_run_files, RunFile};
pub use metadata::{read_run, run_history, write_run, RunHistory, RunHistoryEntry};
pub use plan::{policy_state, LogPlan, LogState, LogStatus, Persistence};
pub use retention::{
    cleanup_plan, cleanup_preview, CleanupReport, LogFile, RetentionPolicy, DEFAULT_RETENTION,
};
pub use run_log::{LogLimits, RunLog, RunLogHandle, RunOutcome, DEFAULT_LOG_LIMITS};

/// The two app-data roots the logging layer writes under.
///
/// Taken from the app-data layout (`crate::config::AppPaths`) rather than
/// invented: `docs/DECISIONS.md` D-011 fixes where logs live, and a second
/// answer to "where does a log go" is how a user ends up with two folders they
/// cannot tell apart. Session Core holds these because it is the layer that
/// knows when a run starts and ends; the logging layer only receives paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogRoots {
    /// Root for Hub-written logs, `logs/<session>/<month>/`.
    pub logs_dir: std::path::PathBuf,
    /// Root for run metadata, `metadata/<session>/<month>/`.
    pub metadata_dir: std::path::PathBuf,
}

impl LogRoots {
    /// The logging roots inside an app-data layout.
    pub fn from_app_paths(paths: &crate::config::AppPaths) -> Self {
        LogRoots {
            logs_dir: paths.logs_dir.clone(),
            metadata_dir: paths.metadata_dir.clone(),
        }
    }
}

/// How much of a captured stream one read takes.
///
/// The unit a UI ends up seeing (spec §9) and the unit a log file is written
/// in. Large enough that a chatty process is cheap, small enough that a burst
/// is readable as it happens rather than in one lump when the pipe closes.
pub const BATCH_BYTES: usize = 8 * 1024;

/// Somewhere a batch of captured output goes.
///
/// Implemented by the layer that owns the run's state, which is the only place
/// that knows both what a session's buffer is and which run the bytes belong
/// to. Calls arrive on the pumping thread — never on the thread running a
/// lifecycle operation — so an implementation must not assume it holds any
/// lock the lifecycle needs, and must not block for long: it is the only
/// reader of the pipe, and a stalled sink stalls the process writing to it.
pub trait OutputSink: Send + Sync + 'static {
    /// Take one batch, in the order the stream produced it.
    fn take(&self, stream: Stream, bytes: &[u8]);
}

/// A shared sink is a sink.
///
/// A session's output has one reader per stream but one destination, so the
/// destination is held behind an `Arc` and the pumping threads each take a
/// clone; without this the caller would have to wrap every sink in a newtype
/// before it could be shared.
impl<T: OutputSink + ?Sized> OutputSink for std::sync::Arc<T> {
    fn take(&self, stream: Stream, bytes: &[u8]) {
        (**self).take(stream, bytes);
    }
}

/// Read `reader` to its end, handing each batch to `sink`.
///
/// Returns the thread doing the reading. It ends when the stream ends — for a
/// captured process, when the last writer closes the pipe — so a caller does
/// not have to stop it: there is nothing here that waits on the session
/// lifecycle, and a run's last output arrives after the process exits.
///
/// `drained` is called once, on that thread, when the stream has ended. It
/// exists for the race a fast-exiting run creates: a process can exit with
/// output still sitting in the pipe, and a caller that closes the run's log the
/// moment the process is reaped would write a file that is missing the very
/// output it was capturing. Waiting for this signal is how a caller knows the
/// pipe has been read out before it decides the run is over.
///
/// A read error ends the pump and is not reported as a logging failure: the
/// pipe closing is how every captured process ends, and distinguishing it from
/// a genuine error would mean reading the OS error code to learn that the
/// writer is gone. What matters to the layer above is that the batches already
/// taken were correct and that the run's lifecycle continues independently.
pub fn pump<R: Read + Send + 'static>(
    mut reader: R,
    stream: Stream,
    sink: impl OutputSink,
    drained: impl FnOnce() + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut batch = vec![0u8; BATCH_BYTES];
        loop {
            match reader.read(&mut batch) {
                Ok(0) => break,
                Ok(read) => sink.take(stream, &batch[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        drained();
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSink {
        batches: Mutex<Vec<(Stream, Vec<u8>)>>,
    }

    impl OutputSink for RecordingSink {
        fn take(&self, stream: Stream, bytes: &[u8]) {
            self.batches
                .lock()
                .expect("the sink is not poisoned")
                .push((stream, bytes.to_vec()));
        }
    }

    /// The pump hands the whole stream to the sink and stops when it ends.
    #[test]
    fn the_pump_forwards_the_whole_stream_and_then_stops() {
        let sink = std::sync::Arc::new(RecordingSink::default());
        let reader = std::io::Cursor::new(b"line one\nline two\n".to_vec());

        pump(reader, Stream::Stdout, std::sync::Arc::clone(&sink), || {})
            .join()
            .expect("the pump thread ends cleanly");

        let batches = sink.batches.lock().expect("not poisoned");
        let text: Vec<u8> = batches
            .iter()
            .flat_map(|(_, bytes)| bytes.clone())
            .collect();
        assert_eq!(text, b"line one\nline two\n");
        assert!(batches.iter().all(|(stream, _)| *stream == Stream::Stdout));
    }

    /// The stream tag travels with the batch: the same pump is used for both
    /// pipes, and the sink has to be able to tell them apart.
    #[test]
    fn the_pump_reports_which_stream_it_was_given() {
        let sink = std::sync::Arc::new(RecordingSink::default());

        pump(
            std::io::Cursor::new(b"err\n".to_vec()),
            Stream::Stderr,
            std::sync::Arc::clone(&sink),
            || {},
        )
        .join()
        .expect("the pump thread ends cleanly");

        let batches = sink.batches.lock().expect("not poisoned");
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].0, Stream::Stderr);
    }

    /// A stream longer than one batch arrives as several, in order: the batch
    /// boundary is an implementation detail of reading, not of the content.
    #[test]
    fn a_long_stream_arrives_in_batches_in_order() {
        let payload: Vec<u8> = (0..BATCH_BYTES * 3 + 7)
            .map(|index| (index % 251) as u8)
            .collect();
        let sink = std::sync::Arc::new(RecordingSink::default());

        pump(
            std::io::Cursor::new(payload.clone()),
            Stream::Stdout,
            std::sync::Arc::clone(&sink),
            || {},
        )
        .join()
        .expect("the pump thread ends cleanly");

        let batches = sink.batches.lock().expect("not poisoned");
        assert!(batches.len() > 1, "{} batches", batches.len());
        let rebuilt: Vec<u8> = batches
            .iter()
            .flat_map(|(_, bytes)| bytes.clone())
            .collect();
        assert_eq!(rebuilt, payload);
        assert!(
            batches.iter().all(|(_, bytes)| bytes.len() <= BATCH_BYTES),
            "a batch exceeded the read size"
        );
    }

    /// A failing reader ends the pump rather than spinning on the error: the
    /// layer above only needs to know that no more output is coming.
    #[test]
    fn a_failing_reader_ends_the_pump() {
        struct Failing {
            first: bool,
        }

        impl Read for Failing {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if self.first {
                    self.first = false;
                    buffer[..2].copy_from_slice(b"ok");
                    return Ok(2);
                }
                Err(std::io::Error::other("the pipe broke"))
            }
        }

        let sink = std::sync::Arc::new(RecordingSink::default());

        pump(
            Failing { first: true },
            Stream::Stdout,
            std::sync::Arc::clone(&sink),
            || {},
        )
        .join()
        .expect("the pump thread ends cleanly");

        let batches = sink.batches.lock().expect("not poisoned");
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].1, b"ok");
    }

    /// The drain signal is how a caller learns the pipe has been read out.
    ///
    /// It has to arrive *after* the last batch, not before: a run whose output
    /// is still in the pipe when its process exits would otherwise have its log
    /// closed around the very output it was capturing.
    #[test]
    fn the_drain_signal_arrives_after_the_last_batch() {
        let sink = std::sync::Arc::new(RecordingSink::default());
        let (drained, seen) = std::sync::mpsc::channel();
        let sink_for_pump = std::sync::Arc::clone(&sink);

        let handle = pump(
            std::io::Cursor::new(b"last words\n".to_vec()),
            Stream::Stdout,
            sink_for_pump,
            move || {
                let _ = drained.send(());
            },
        );

        seen.recv_timeout(std::time::Duration::from_secs(5))
            .expect("the pump reports that it drained");
        handle.join().expect("the pump thread ends cleanly");

        let batches = sink.batches.lock().expect("not poisoned");
        let text: Vec<u8> = batches
            .iter()
            .flat_map(|(_, bytes)| bytes.clone())
            .collect();
        assert_eq!(
            text, b"last words\n",
            "the signal arrived before the output did"
        );
    }

    // Whether a batch reaches the sink before its stream ends — the property a
    // long-running service depends on — is asserted against a real process
    // pipe in `crate::session::core`, where the process that writes it also
    // stays alive to prove the point.
}
