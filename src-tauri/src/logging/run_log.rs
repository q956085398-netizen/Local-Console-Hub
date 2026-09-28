//! One run's log file, and the decision to have one at all.
//!
//! A [`RunLog`] is created when a run starts and closed when it ends. Between
//! those two moments it is the only thing that turns "output happened" into
//! "output was written", and it does so according to the resolved policy
//! ([`super::plan::Persistence`]) rather than to what the output looks like.
//!
//! ## What each mode does with the bytes
//!
//! | mode | while running | at a clean end | at a failed end |
//! |---|---|---|---|
//! | `off` / `source: none` | buffer only | nothing | nothing |
//! | `always` | file | file stays | file stays |
//! | `on_error` | bounded buffer | buffer discarded | buffer written, then the failure |
//! | `manual` | nothing until asked | file if recording | file if recording |
//! | `external` | nothing (D-005) | nothing | nothing |
//!
//! `docs/LOGGING.md` §7 asks each of these to be answerable in advance: when a
//! file is created, what it holds, when it is closed, how it is found, how it
//! is cleaned, and whether user input is in it. The table is the first four;
//! [`super::retention`] is the fifth; the sixth is that no variant of this type
//! can be handed input, because [`Stream`] has no `stdin`.
//!
//! ## Failure is never fatal
//!
//! Every write here is best-effort. A log file that cannot be opened leaves the
//! run running and records why in [`RunLog::last_error`]: logging policy is not
//! lifecycle truth (`docs/DEVELOPMENT.md` §3), and a service that refuses to
//! start because its log directory is read-only would be a worse product than
//! one that starts and says the log is missing.

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::buffer::{BufferLimits, Stream, TerminalBuffer, DEFAULT_PRE_FAILURE_LIMITS};
use super::clock::{now, rfc3339_utc};
use super::error::{io_error, LogError};
use super::plan::{LogState, Persistence};

/// How large a single run's log may grow, and how much context an `on_error`
/// run holds before it knows whether it needs any.
///
/// The file cap is a backstop, not rotation: rotation is per session across
/// runs (`super::retention`), while this bounds one pathological run that
/// writes faster than anyone can read. Sixteen megabytes is far more than a
/// service log worth reading and far less than a disk-filling accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogLimits {
    pub max_file_bytes: u64,
    pub pre_failure: BufferLimits,
}

pub const DEFAULT_LOG_LIMITS: LogLimits = LogLimits {
    max_file_bytes: 16 * 1024 * 1024,
    pre_failure: DEFAULT_PRE_FAILURE_LIMITS,
};

impl Default for LogLimits {
    fn default() -> Self {
        DEFAULT_LOG_LIMITS
    }
}

/// How a run ended, as far as the logging policy is concerned.
///
/// Separate from [`crate::session::state::SessionStatus`] on purpose: that is
/// the lifecycle's vocabulary, and the logging layer must not need the state
/// machine to decide whether it is looking at a failure.
///
/// The three constructors are the three ways a run ends and they are *not*
/// interchangeable, because `on_error` asks one question of them: is this
/// ending evidence of a problem? A stopped process reporting a non-zero code
/// is the trap — Windows terminates a job's members with exit code 1, and a
/// graceful `CTRL_BREAK` typically ends a console app with a code of its own —
/// so a stop the user asked for would otherwise file an "error" log on every
/// press of Stop, which is exactly the log litter `docs/LOGGING.md` §1.2 is
/// about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOutcome {
    pub exit_code: Option<u32>,
    /// The session reached `Error`, whatever the exit code says.
    pub failed: bool,
    /// Someone asked this run to stop.
    pub requested: bool,
}

impl RunOutcome {
    /// A run that ended on its own with an exit code and no other complaint.
    pub fn exited(exit_code: Option<u32>) -> Self {
        RunOutcome {
            exit_code,
            failed: false,
            requested: false,
        }
    }

    /// A run the lifecycle called a failure.
    pub fn failed(exit_code: Option<u32>) -> Self {
        RunOutcome {
            exit_code,
            failed: true,
            requested: false,
        }
    }

    /// A run that ended because it was asked to.
    ///
    /// A non-zero exit code here is how a stopped process reports being
    /// stopped, not a failure. A stop that could not be carried out is
    /// [`RunOutcome::failed`], and does keep its log.
    pub fn stopped(exit_code: Option<u32>) -> Self {
        RunOutcome {
            exit_code,
            failed: false,
            requested: true,
        }
    }

    /// Whether this is the ending `on_error` exists for.
    ///
    /// A run with no exit code to inspect is not called a failure — the same
    /// rule Session Core applies when it decides between `Exited` and `Error`
    /// (`docs/MVP_IMPLEMENTATION_SPEC.md` §5).
    pub fn is_abnormal(&self) -> bool {
        self.failed || (!self.requested && !matches!(self.exit_code, None | Some(0)))
    }
}

/// What to do with one batch of output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Nothing: no file is written and no memory is spent.
    Ignore,
    /// Hold it for a possible failure.
    Buffer,
    /// Write it out.
    Write,
}

/// The live logging state of one run.
pub struct RunLog {
    persistence: Persistence,
    limits: LogLimits,
    file: Option<BufWriter<File>>,
    written: u64,
    truncated: bool,
    at_line_start: bool,
    last_stream: Option<Stream>,
    context: TerminalBuffer,
    state: LogState,
    last_error: Option<LogError>,
    /// The user asked for this run's log while it was still running
    /// ([`RunLog::save_now`]). A saved `on_error` run keeps its file however
    /// it ends: the request was a decision, not a prediction about the exit
    /// code.
    saved: bool,
    /// The file this run actually produced, whether or not it is still open.
    ///
    /// Separate from `file` because the two answer different questions: `file`
    /// is "am I writing now?", which a `manual` run's stop and an `on_error`
    /// run's commit both answer "no" to, while the run's record still has to
    /// name the file a user can open. It stays `None` until a file has really
    /// been created, so a record never names a path that a failed
    /// `File::create` meant it to have.
    produced: Option<PathBuf>,
    /// Whether this run has opened its file before — i.e. whether opening it
    /// again continues it or starts it.
    opened: bool,
}

impl RunLog {
    /// Begin a run under `persistence`.
    ///
    /// A mode that writes from the start opens its file here, so the file
    /// exists for the whole run and a service that crashes before writing a
    /// byte still leaves the evidence that it was being captured
    /// (`docs/LOGGING.md` §13 scenario B).
    pub fn start(persistence: Persistence, limits: LogLimits) -> Self {
        let state = persistence.initial_state();
        let mut log = RunLog {
            persistence,
            limits,
            file: None,
            written: 0,
            truncated: false,
            at_line_start: true,
            last_stream: None,
            context: TerminalBuffer::new(limits.pre_failure),
            state,
            last_error: None,
            saved: false,
            produced: None,
            opened: false,
        };

        if matches!(log.persistence, Persistence::Capture { .. }) {
            log.open_file();
        }
        log
    }

    /// What the UI should show right now (`docs/LOGGING.md` §1.4).
    pub fn state(&self) -> LogState {
        self.state
    }

    /// Why logging is not working as configured, if it is not.
    pub fn last_error(&self) -> Option<&LogError> {
        self.last_error.as_ref()
    }

    /// The file this run is (or would be) written to.
    pub fn hub_log_path(&self) -> Option<&Path> {
        self.persistence.hub_log_path().map(PathBuf::as_path)
    }

    /// The file this run produced, once it exists.
    ///
    /// Gone again only if the file is deleted behind the Hub's back; `None`
    /// means nothing was written — not that nothing *would* have been.
    pub fn produced_path(&self) -> Option<&Path> {
        self.produced.as_deref()
    }

    /// Whether output is being written out at this moment.
    pub fn is_writing(&self) -> bool {
        self.file.is_some()
    }

    /// Whether the file cap stopped this run's log short.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Push what is buffered out to the file.
    ///
    /// Log writes are buffered (`docs/MVP_IMPLEMENTATION_SPEC.md` §14), so the
    /// file on disk lags what has been captured by at most one write buffer.
    /// That is the right trade for a service writing thousands of lines a
    /// second, and the wrong one for a user about to open the file — so a
    /// caller showing a *live* run's log calls this first
    /// (`docs/LOGGING.md` §10's "open log").
    pub fn flush(&mut self) {
        let error = match self.file.as_mut() {
            Some(file) => file.flush().err(),
            None => None,
        };
        if let Some(error) = error {
            let path = self.persistence.hub_log_path().cloned().unwrap_or_default();
            self.fail(io_error("flushing the run log", &path, error));
        }
    }

    /// Bytes held for a possible failure (`on_error`).
    pub fn retained_bytes(&self) -> usize {
        self.context.bytes()
    }

    /// Take one batch of output.
    ///
    /// There is no `stdin` to pass: [`Stream`] has no variant for it, which is
    /// how `docs/LOGGING.md` §4's "input is not recorded" stays true without
    /// anyone having to check for it.
    pub fn append(&mut self, stream: Stream, bytes: &[u8]) {
        match self.action() {
            Action::Ignore => {}
            Action::Buffer => self.context.push(stream, bytes),
            Action::Write => self.write(stream, bytes),
        }
    }

    /// Start recording a `manual` run, opening its file.
    ///
    /// Returns whether recording is now on. `true` for a run that was already
    /// recording: the caller asked for a state, not for a transition.
    pub fn start_recording(&mut self) -> bool {
        if !matches!(self.persistence, Persistence::Manual { .. }) {
            return self.is_writing();
        }
        self.open_file();
        self.is_writing()
    }

    /// Stop recording a `manual` run, closing its file.
    ///
    /// Returns the file if one was written, so the caller can report what the
    /// user now has rather than only that recording stopped.
    pub fn stop_recording(&mut self) -> Option<PathBuf> {
        if !matches!(self.persistence, Persistence::Manual { .. }) {
            return None;
        }
        self.state = LogState::Off;
        self.close()
    }

    /// Commit an `on_error` run's retained context to its file now, without
    /// waiting for the run to end — "save this run's log" from the UI
    /// (`docs/LOGGING.md` §3).
    ///
    /// Returns whether there is now a file. Output after this point is written
    /// as it arrives, so a run saved early still ends up with a complete log.
    pub fn save_now(&mut self) -> bool {
        if !matches!(self.persistence, Persistence::OnError { .. }) {
            return self.is_writing();
        }
        if self.file.is_none() {
            self.open_file();
        }
        if self.file.is_none() {
            return false;
        }
        if self.written == 0 {
            self.write_header(
                "run saved on request; the retained output before this point follows",
            );
            self.flush_context();
        }
        self.saved = true;
        true
    }

    /// End the run, and answer with the file it left behind — `None` when the
    /// policy wrote nothing, which is a normal outcome and not a problem.
    ///
    /// The answer is the file the run *produced*, not the file that is open at
    /// this moment: a `manual` run whose recording the user stopped, or an
    /// `on_error` run that was saved and then went quiet, still has a log on
    /// disk that its run record has to name (`docs/LOGGING.md` §6: the record
    /// is how the file is found again).
    pub fn finish(&mut self, outcome: RunOutcome) -> Option<PathBuf> {
        match &self.persistence {
            Persistence::Off | Persistence::External { .. } => None,
            Persistence::Capture { .. } => self.close(),
            Persistence::Manual { .. } => {
                if self.is_writing() {
                    self.close();
                }
                // `None` until a recording actually started, so a `manual` run
                // nobody recorded names no file.
                self.produced.clone()
            }
            Persistence::OnError { .. } => {
                if !outcome.is_abnormal() && !self.saved {
                    // The whole point of `on_error`: a run that behaved
                    // normally leaves no garbage behind. A run the user asked
                    // to save is the exception — they asked for the file, so
                    // they keep it whatever the exit code turned out to be.
                    self.context.clear();
                    self.state = LogState::Off;
                    return None;
                }
                if self.file.is_none() {
                    self.open_file();
                }
                // Opening can fail, and a failure means there is no file to
                // write the context into or to report.
                self.file.as_ref()?;
                if self.written == 0 {
                    self.write_header(&format!(
                        "run failed ({}); the retained output before the failure follows",
                        describe_outcome(&outcome)
                    ));
                    self.flush_context();
                }
                self.write_header(&format!("run ended: {}", describe_outcome(&outcome)));
                self.state = LogState::Off;
                self.close();
                self.produced.clone()
            }
        }
    }

    fn action(&self) -> Action {
        match &self.persistence {
            Persistence::Off | Persistence::External { .. } => Action::Ignore,
            Persistence::Capture { .. } => Action::Write,
            // Once committed, an `on_error` run writes like any other and the
            // buffer that existed for the decision is not needed again.
            Persistence::OnError { .. } => {
                if self.is_writing() {
                    Action::Write
                } else {
                    Action::Buffer
                }
            }
            Persistence::Manual { .. } => {
                if self.is_writing() {
                    Action::Write
                } else {
                    Action::Ignore
                }
            }
        }
    }

    /// Create the file — or reopen it, continuing where it left off.
    ///
    /// A `manual` run can be switched on and off more than once, and a
    /// recording that resumed by truncating the file would destroy the very
    /// output the user asked to keep. So the first open of a run starts its
    /// file and every later open appends to it, which is also why the byte cap
    /// is measured from the file's own length rather than from zero.
    fn open_file(&mut self) {
        let Some(path) = self.persistence.hub_log_path().cloned() else {
            return;
        };
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                self.fail(io_error("creating the log directory", parent, error));
                return;
            }
        }

        let opening = if self.opened {
            fs::OpenOptions::new().append(true).create(true).open(&path)
        } else {
            File::create(&path)
        };
        match opening {
            Ok(file) => {
                if self.opened {
                    // Continue the file's own accounting rather than the
                    // session's: the cap is about how large the file may grow.
                    // A file whose length cannot be read is treated as empty,
                    // which at worst lets the cap be reached twice.
                    self.written = file.metadata().map(|meta| meta.len()).unwrap_or(0);
                } else {
                    self.written = 0;
                    self.at_line_start = true;
                    self.last_stream = None;
                }
                self.opened = true;
                self.produced = Some(path);
                self.file = Some(BufWriter::new(file));
                self.state = match self.persistence {
                    Persistence::Manual { .. } => LogState::Capturing,
                    Persistence::OnError { .. } => LogState::OnError,
                    _ => LogState::Capturing,
                };
            }
            Err(error) => self.fail(io_error("creating the run log", &path, error)),
        }
    }

    /// Flush and close, answering with the file that was written.
    fn close(&mut self) -> Option<PathBuf> {
        if let Some(mut file) = self.file.take() {
            if let Err(error) = file.flush() {
                let path = self.produced.clone().unwrap_or_default();
                self.fail(io_error("flushing the run log", &path, error));
            }
            return self.produced.clone();
        }
        None
    }

    /// Write the retained pre-failure context, newest stream tags intact.
    fn flush_context(&mut self) {
        let chunks: Vec<(Stream, Vec<u8>)> = self
            .context
            .chunks()
            .map(|chunk| (chunk.stream, chunk.bytes().to_vec()))
            .collect();
        for (stream, bytes) in chunks {
            self.write(stream, &bytes);
        }
        self.context.clear();
    }

    fn write(&mut self, stream: Stream, bytes: &[u8]) {
        if bytes.is_empty() || self.truncated || self.file.is_none() {
            return;
        }

        // A run's stdout and stderr are interleaved into one file, so the file
        // has to say which stream each part came from and when it switched
        // (`docs/LOGGING.md` §7). The header is written on a line of its own —
        // a switch in the middle of a partially written line would otherwise
        // leave the previous stream's text attached to the new header.
        let mut framed = Vec::with_capacity(bytes.len() + 64);
        if self.last_stream != Some(stream) {
            if !self.at_line_start {
                framed.push(b'\n');
            }
            framed.extend_from_slice(self.stamped_header(stream.as_str()).as_bytes());
            self.last_stream = Some(stream);
        }
        framed.extend_from_slice(bytes);

        // The file cap. Truncating mid-stream and saying so beats a log that
        // silently stops, which is indistinguishable from a service that
        // quietly died.
        let cap = self.limits.max_file_bytes;
        if self.written + framed.len() as u64 > cap {
            let room = cap.saturating_sub(self.written) as usize;
            framed.truncate(room);
            self.truncated = true;
        }
        self.push_bytes(&framed);

        if self.truncated {
            let marker = format!(
                "\n--- log truncated at {cap} bytes; later output is not in this file ---\n"
            );
            self.push_bytes(marker.as_bytes());
        }
    }

    /// A header line explaining what the file is about to contain.
    fn write_header(&mut self, text: &str) {
        let line = self.stamped_header(text);
        if !self.at_line_start {
            self.push_bytes(b"\n");
        }
        self.push_bytes(line.as_bytes());
    }

    fn stamped_header(&self, text: &str) -> String {
        match rfc3339_utc(now()) {
            Some(stamp) => format!("[{stamp} {text}]\n"),
            None => format!("[{text}]\n"),
        }
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() || self.file.is_none() {
            return;
        }
        let written = match self.file.as_mut() {
            Some(file) => file.write_all(bytes),
            None => return,
        };
        match written {
            Ok(()) => {
                self.written += bytes.len() as u64;
                self.at_line_start = bytes.last() == Some(&b'\n');
            }
            Err(error) => {
                let path = self.persistence.hub_log_path().cloned().unwrap_or_default();
                self.fail(io_error("writing to the run log", &path, error));
            }
        }
    }

    /// Record that logging stopped working.
    ///
    /// The file is dropped rather than kept: retrying a write that just failed
    /// on every subsequent batch would turn one broken log into a permanently
    /// failing write path, and the run itself has to keep going.
    fn fail(&mut self, error: LogError) {
        self.last_error = Some(error);
        self.file = None;
        self.state = LogState::Off;
    }
}

fn describe_outcome(outcome: &RunOutcome) -> String {
    match (outcome.failed, outcome.exit_code) {
        (true, Some(code)) => format!("error state, exit code {code}"),
        (true, None) => "error state".to_owned(),
        (false, Some(code)) => format!("exit code {code}"),
        (false, None) => "no exit code reported".to_owned(),
    }
}

/// One run's log, shared between the threads that touch it.
///
/// Two threads write to a run's log and neither is the other's caller: the
/// pumping thread takes batches as the process produces them, and the lifecycle
/// thread closes the log when the run ends. Cloning the handle is how they
/// share it — a run log has no other owner, and a clone that outlives the run
/// (a pump thread finishing its last read) appends to a closed log rather than
/// resurrecting it.
///
/// ## Locking
///
/// The lock inside is the *second* lock in the order Session Core uses: a
/// session lock is always taken before a run log's, never the other way round.
/// Nothing here calls back out, so a handle cannot deadlock against the
/// session that holds it.
#[derive(Clone)]
pub struct RunLogHandle {
    inner: Arc<Mutex<RunLog>>,
}

impl RunLogHandle {
    /// Take ownership of `log` and hand out handles to it.
    pub fn new(log: RunLog) -> Self {
        RunLogHandle {
            inner: Arc::new(Mutex::new(log)),
        }
    }

    pub fn state(&self) -> LogState {
        self.with(RunLog::state)
    }

    pub fn last_error(&self) -> Option<LogError> {
        self.with(|log| log.last_error().cloned())
    }

    pub fn is_writing(&self) -> bool {
        self.with(RunLog::is_writing)
    }

    pub fn hub_log_path(&self) -> Option<PathBuf> {
        self.with(|log| log.hub_log_path().map(Path::to_path_buf))
    }

    /// The file this run produced, once it exists.
    ///
    /// Answers after the file is closed too, which is what a run record needs:
    /// a `manual` run whose recording stopped and an `on_error` run that was
    /// saved both still have a log for the record to name.
    pub fn produced_path(&self) -> Option<PathBuf> {
        self.with(|log| log.produced_path().map(Path::to_path_buf))
    }

    pub fn is_truncated(&self) -> bool {
        self.with(RunLog::truncated)
    }

    /// Take one batch of a captured stream.
    pub fn append(&self, stream: Stream, bytes: &[u8]) {
        self.with_mut(|log| log.append(stream, bytes));
    }

    /// Start recording a `manual` run; answers whether it is recording now.
    pub fn start_recording(&self) -> bool {
        self.with_mut(RunLog::start_recording)
    }

    /// Stop recording a `manual` run, answering with what was written.
    pub fn stop_recording(&self) -> Option<PathBuf> {
        self.with_mut(RunLog::stop_recording)
    }

    /// Commit an `on_error` run's context now; answers whether a file exists.
    pub fn save_now(&self) -> bool {
        self.with_mut(RunLog::save_now)
    }

    /// Push what is buffered out to the file.
    pub fn flush(&self) {
        self.with_mut(RunLog::flush);
    }

    /// End the run, answering with the file it left behind.
    pub fn finish(&self, outcome: RunOutcome) -> Option<PathBuf> {
        self.with_mut(|log| log.finish(outcome))
    }

    /// Run `read` against the locked log.
    fn with<T>(&self, read: impl FnOnce(&RunLog) -> T) -> T {
        let log = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        read(&log)
    }

    /// Run `update` against the locked log.
    fn with_mut<T>(&self, update: impl FnOnce(&mut RunLog) -> T) -> T {
        let mut log = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        update(&mut log)
    }
}

impl std::fmt::Debug for RunLogHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RunLogHandle")
            .field("state", &self.state())
            .field("writing", &self.is_writing())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::test_support::TempDir;

    fn limits() -> LogLimits {
        LogLimits {
            max_file_bytes: 4096,
            pre_failure: BufferLimits::new(1024, 16),
        }
    }

    /// A captured log at `dir/run.log`, with the path the test can assert on.
    fn capture_at(dir: &TempDir) -> (RunLog, PathBuf) {
        let path = dir.join("logs/svc/2026-09/run.log");
        let log = RunLog::start(Persistence::Capture { path: path.clone() }, limits());
        (log, path)
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("{} is unreadable: {error}", path.display()))
    }

    /// `docs/LOGGING.md` §13 scenario B: a service configured `always` creates
    /// a run file and keeps writing to it.
    #[test]
    fn always_creates_the_run_file_and_writes_output_into_it() {
        let dir = TempDir::new();
        let (mut log, path) = capture_at(&dir);

        assert!(path.exists(), "the run file is created when the run starts");
        log.append(Stream::Stdout, b"listening on 8188\n");
        log.append(Stream::Stdout, b"ready\n");

        let written = log.finish(RunOutcome::exited(Some(0))).expect("a file");
        assert_eq!(written, path);
        let text = read(&path);
        assert!(text.contains("listening on 8188"), "{text}");
        assert!(text.contains("ready"), "{text}");
    }

    /// The file is created at start, so a service that produces nothing is
    /// still visibly "being captured" rather than indistinguishable from `off`.
    #[test]
    fn always_creates_the_file_before_any_output_exists() {
        let dir = TempDir::new();

        let (log, path) = capture_at(&dir);

        assert!(path.exists());
        assert_eq!(fs::metadata(&path).expect("readable").len(), 0);
        assert_eq!(log.state(), LogState::Capturing);
    }

    #[test]
    fn a_capture_run_with_no_output_still_reports_its_file() {
        let dir = TempDir::new();
        let (mut log, path) = capture_at(&dir);

        assert_eq!(log.finish(RunOutcome::exited(Some(0))), Some(path));
    }

    /// `docs/LOGGING.md` §1.2 and §13 scenario A: an interactive terminal with
    /// the default policy must leave nothing on disk.
    #[test]
    fn off_writes_no_file_however_much_output_arrives() {
        let dir = TempDir::new();
        let mut log = RunLog::start(Persistence::Off, limits());

        log.append(Stream::Stdout, b"a lot of output\n".repeat(100).as_slice());

        assert!(!log.is_writing());
        assert_eq!(log.finish(RunOutcome::failed(None)), None);
        assert!(!dir.join("logs").exists(), "nothing may be created");
    }

    /// D-005: when the application owns the log, the Hub must not write a
    /// second copy of the same output.
    #[test]
    fn external_captures_nothing() {
        let dir = TempDir::new();
        let mut log = RunLog::start(
            Persistence::External {
                path: dir.join("app/access.log"),
            },
            limits(),
        );

        log.append(Stream::Stdout, b"output the application already logs\n");

        assert_eq!(log.state(), LogState::External);
        assert_eq!(log.finish(RunOutcome::failed(Some(1))), None);
        assert!(!dir.join("app/access.log").exists(), "no duplicate capture");
    }

    /// §13 scenario C: normal runs leave nothing, failures leave the context.
    #[test]
    fn on_error_writes_nothing_when_the_run_ends_cleanly() {
        let dir = TempDir::new();
        let path = dir.join("logs/svc/2026-09/run-2.log");
        let mut log = RunLog::start(Persistence::OnError { path: path.clone() }, limits());

        log.append(Stream::Stdout, b"all good\n");

        assert_eq!(log.finish(RunOutcome::exited(Some(0))), None);
        assert!(!path.exists(), "a clean run must leave no file behind");
        assert_eq!(log.retained_bytes(), 0, "the context is released");
    }

    #[test]
    fn on_error_writes_the_pre_failure_context_when_the_run_fails() {
        let dir = TempDir::new();
        let path = dir.join("logs/svc/2026-09/run-3.log");
        let mut log = RunLog::start(Persistence::OnError { path: path.clone() }, limits());

        log.append(Stream::Stdout, b"starting up\n");
        log.append(Stream::Stderr, b"fatal: port in use\n");

        let written = log.finish(RunOutcome::exited(Some(1))).expect("a file");
        assert_eq!(written, path);
        let text = read(&path);
        assert!(
            text.contains("starting up"),
            "pre-failure context lost: {text}"
        );
        assert!(text.contains("fatal: port in use"), "{text}");
        assert!(
            text.contains("exit code 1"),
            "the ending is recorded: {text}"
        );
        assert!(
            text.find("starting up").unwrap() < text.find("fatal").unwrap(),
            "the context is written in the order it happened: {text}"
        );
    }

    /// A run the lifecycle called a failure is a failure even with exit code 0
    /// — the two ways into `Error` are not interchangeable (spec §5).
    #[test]
    fn on_error_treats_an_error_state_as_a_failure() {
        let dir = TempDir::new();
        let path = dir.join("logs/svc/2026-09/run-4.log");
        let mut log = RunLog::start(Persistence::OnError { path: path.clone() }, limits());
        log.append(Stream::Stderr, b"broke\n");

        let written = log.finish(RunOutcome::failed(Some(0)));

        assert_eq!(written, Some(path.clone()));
        assert!(read(&path).contains("error state"));
    }

    /// §8: the context is bounded, so a very noisy `on_error` service cannot
    /// turn a rare failure into an unbounded memory bill.
    #[test]
    fn the_pre_failure_context_is_bounded() {
        let dir = TempDir::new();
        let mut log = RunLog::start(
            Persistence::OnError {
                path: dir.join("logs/svc/run.log"),
            },
            limits(),
        );

        for _ in 0..200 {
            log.append(Stream::Stdout, b"0123456789\n");
        }

        assert!(
            log.retained_bytes() <= limits().pre_failure.max_bytes,
            "retained {} bytes",
            log.retained_bytes()
        );
        assert!(log.retained_bytes() > 0);
    }

    /// "Save this run's log" commits the context immediately and keeps the file
    /// open, so the log the user asked for stays complete.
    #[test]
    fn saving_an_on_error_run_commits_the_context_and_keeps_recording() {
        let dir = TempDir::new();
        let path = dir.join("logs/svc/run-5.log");
        let mut log = RunLog::start(Persistence::OnError { path: path.clone() }, limits());
        log.append(Stream::Stdout, b"before\n");

        assert!(log.save_now(), "a file exists after the request");
        log.append(Stream::Stdout, b"after\n");

        assert_eq!(log.finish(RunOutcome::exited(Some(0))), Some(path.clone()));
        let text = read(&path);
        assert!(text.contains("before"), "{text}");
        assert!(text.contains("after"), "{text}");
    }

    /// `manual` is the "not right now, thank you" mode: nothing is written
    /// until asked, and the file only exists once it is.
    #[test]
    fn manual_writes_nothing_until_recording_starts() {
        let dir = TempDir::new();
        let path = dir.join("logs/shell/run-6.log");
        let mut log = RunLog::start(Persistence::Manual { path: path.clone() }, limits());

        assert_eq!(
            log.state(),
            LogState::Off,
            "manual starts off, not capturing"
        );
        log.append(Stream::Stdout, b"not recorded\n");
        assert_eq!(log.finish(RunOutcome::exited(Some(0))), None);
        assert!(!path.exists());

        let mut log = RunLog::start(Persistence::Manual { path: path.clone() }, limits());
        assert!(log.start_recording());
        assert_eq!(log.state(), LogState::Capturing);
        log.append(Stream::Stdout, b"recorded\n");

        assert_eq!(log.stop_recording(), Some(path.clone()));
        assert!(read(&path).contains("recorded"));
    }

    /// Recording stopped by hand is not a failure, and what was recorded before
    /// it stopped is the log the user gets at the end of the run.
    #[test]
    fn a_manual_run_closes_its_file_at_the_end() {
        let dir = TempDir::new();
        let path = dir.join("logs/shell/run-7.log");
        let mut log = RunLog::start(Persistence::Manual { path: path.clone() }, limits());
        log.start_recording();
        log.append(Stream::Stdout, b"captured\n");

        assert_eq!(log.finish(RunOutcome::exited(Some(0))), Some(path));
    }

    /// The file is the run's, not the recording's: stopping recording and
    /// starting again has to continue it. Truncating would destroy the output
    /// the user asked to keep, which is the entire reason they asked.
    #[test]
    fn recording_again_continues_the_file_instead_of_replacing_it() {
        let dir = TempDir::new();
        let path = dir.join("logs/shell/run-8.log");
        let mut log = RunLog::start(Persistence::Manual { path: path.clone() }, limits());

        log.start_recording();
        log.append(Stream::Stdout, b"first stretch\n");
        log.stop_recording();
        log.start_recording();
        log.append(Stream::Stdout, b"second stretch\n");
        log.finish(RunOutcome::exited(Some(0)));

        let text = read(&path);
        assert!(
            text.contains("first stretch"),
            "the first stretch went: {text}"
        );
        assert!(text.contains("second stretch"), "{text}");
    }

    /// A file that was produced stays the run's answer even after it is closed
    /// — that is what a run record names when the run ends.
    #[test]
    fn a_closed_file_is_still_the_file_the_run_produced() {
        let dir = TempDir::new();
        let path = dir.join("logs/shell/run-9.log");
        let mut log = RunLog::start(Persistence::Manual { path: path.clone() }, limits());

        log.start_recording();
        log.append(Stream::Stdout, b"recorded\n");
        assert_eq!(log.stop_recording(), Some(path.clone()));
        assert!(!log.is_writing(), "recording is off");
        assert_eq!(log.produced_path(), Some(path.as_path()));

        assert_eq!(log.finish(RunOutcome::exited(Some(0))), Some(path));
    }

    /// A `manual` run nobody asked to record names no file, and neither does a
    /// file that could not be created: the record names a log *when one
    /// exists* (`docs/MVP_IMPLEMENTATION_SPEC.md` §4).
    #[test]
    fn nothing_produced_means_nothing_to_name() {
        let dir = TempDir::new();
        let path = dir.join("logs/shell/run-10.log");
        let mut unrecorded = RunLog::start(Persistence::Manual { path: path.clone() }, limits());
        assert_eq!(unrecorded.finish(RunOutcome::exited(Some(0))), None);
        assert!(unrecorded.produced_path().is_none());

        let blocker = dir.join("blocked");
        fs::write(&blocker, b"not a directory").expect("the blocker is writable");
        let mut unwritable = RunLog::start(
            Persistence::Capture {
                path: blocker.join("svc/run-11.log"),
            },
            limits(),
        );

        assert_eq!(unwritable.finish(RunOutcome::exited(Some(0))), None);
        assert!(
            unwritable.produced_path().is_none(),
            "a file that was never created must not be named"
        );
    }

    /// A run the user stopped is not a failure, however the terminated process
    /// reports it: Windows kills job members with code 1, and a graceful
    /// `CTRL_BREAK` leaves a code of its own. Writing an error log every time
    /// someone presses Stop is the log litter `docs/LOGGING.md` §1.2 is about.
    #[test]
    fn a_stop_the_user_asked_for_writes_no_error_log() {
        let dir = TempDir::new();
        let path = dir.join("logs/svc/run-12.log");
        let mut log = RunLog::start(Persistence::OnError { path: path.clone() }, limits());
        log.append(Stream::Stdout, b"was running fine\n");

        assert_eq!(log.finish(RunOutcome::stopped(Some(1))), None);
        assert!(!path.exists(), "a requested stop left a log behind");
    }

    /// The other half of the same rule: a stop that could *not* be carried out
    /// is a failure, and keeps the context.
    #[test]
    fn a_stop_that_failed_keeps_the_context() {
        let dir = TempDir::new();
        let path = dir.join("logs/svc/run-13.log");
        let mut log = RunLog::start(Persistence::OnError { path: path.clone() }, limits());
        log.append(Stream::Stdout, b"stuck\n");

        assert_eq!(log.finish(RunOutcome::failed(None)), Some(path.clone()));
        assert!(read(&path).contains("stuck"));
    }

    /// §7: a log keeps the timestamps and the stream a line came from, so
    /// "which of these two messages was on stderr?" is answerable from the
    /// file alone.
    #[test]
    fn a_stream_change_is_marked_in_the_file() {
        let dir = TempDir::new();
        let (mut log, path) = capture_at(&dir);
        log.append(Stream::Stdout, b"out\n");
        log.append(Stream::Stderr, b"err\n");

        log.finish(RunOutcome::exited(Some(0)));
        let text = read(&path);

        assert!(text.contains("stdout"), "{text}");
        assert!(text.contains("stderr"), "{text}");
        assert!(
            text.contains("T") && text.contains("Z"),
            "a line carries a UTC timestamp: {text}"
        );
    }

    /// A stream switch in the middle of a partial line must not glue the new
    /// header onto the previous stream's text.
    #[test]
    fn a_header_never_lands_mid_line() {
        let dir = TempDir::new();
        let (mut log, path) = capture_at(&dir);
        log.append(Stream::Stdout, b"no trailing newline");
        log.append(Stream::Stderr, b"next\n");

        log.finish(RunOutcome::exited(Some(0)));
        let text = read(&path);

        assert!(
            text.contains("no trailing newline\n["),
            "the header shares a line with the previous stream: {text}"
        );
    }

    /// The cap is a backstop against one runaway run; the log says it stopped
    /// rather than just ending, which is what distinguishes it from a service
    /// that died quietly.
    #[test]
    fn the_file_cap_truncates_instead_of_growing_without_limit() {
        let dir = TempDir::new();
        let path = dir.join("logs/svc/run-10.log");
        let mut log = RunLog::start(
            Persistence::Capture { path: path.clone() },
            LogLimits {
                max_file_bytes: 64,
                pre_failure: limits().pre_failure,
            },
        );

        for _ in 0..100 {
            log.append(Stream::Stdout, b"0123456789\n");
        }
        assert!(log.truncated(), "the cap was never reached");
        // The buffered writer holds the tail until the run ends; reading the
        // file before that would assert on a file that is still being written.
        log.finish(RunOutcome::exited(Some(0)));

        let text = read(&path);
        assert!(text.contains("log truncated"), "{text}");
        assert!(
            fs::metadata(&path).expect("readable").len() < 512,
            "the file kept growing past its cap"
        );
    }

    /// A log that cannot be opened must not stop the run: the file is missing,
    /// the reason is reported, and every later write is a no-op rather than a
    /// permanent error path.
    #[test]
    fn an_unopenable_file_is_reported_and_does_not_panic() {
        // A file where the directory would have to be.
        let dir = TempDir::new();
        let blocker = dir.join("logs");
        fs::write(&blocker, b"not a directory").expect("the blocker is writable");

        let mut log = RunLog::start(
            Persistence::Capture {
                path: blocker.join("svc/run-11.log"),
            },
            limits(),
        );

        log.append(Stream::Stdout, b"output\n");

        assert!(!log.is_writing());
        assert_eq!(log.state(), LogState::Off);
        let error = log.last_error().expect("the failure is reported");
        assert_eq!(error.operation, "creating the log directory");
        assert_eq!(log.finish(RunOutcome::exited(Some(0))), None);
    }

    /// A failure to create the file must leave the path visible: the UI says
    /// where it meant to write, not that there is no policy at all.
    #[test]
    fn a_failed_open_still_reports_the_intended_path() {
        let dir = TempDir::new();
        let blocker = dir.join("logs");
        fs::write(&blocker, b"not a directory").expect("the blocker is writable");
        let intended = blocker.join("svc/run-12.log");

        let log = RunLog::start(
            Persistence::Capture {
                path: intended.clone(),
            },
            limits(),
        );

        assert_eq!(log.hub_log_path(), Some(intended.as_path()));
    }
}
