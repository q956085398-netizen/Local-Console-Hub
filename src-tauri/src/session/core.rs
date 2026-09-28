//! Session Core — the single source of lifecycle truth
//! (`docs/MVP_IMPLEMENTATION_SPEC.md` §3, §5).
//!
//! Everything that wants to change what a session is doing goes through this
//! type: the window, the tray, and any future scheduler. It owns the registry,
//! decides transitions against [`super::state`], drives the T03 supervisor for
//! real runs, and publishes [`SessionEvent`]s to a sink.
//!
//! ## Why the sink is a trait
//!
//! Core publishes to an [`EventSink`] rather than to a Tauri `Emitter`. That
//! keeps the lifecycle runnable with no window at all — which is both what
//! makes it testable and what satisfies "session state is independent of
//! main-window visibility": hiding the window can only mean nothing is
//! listening, never that the state machine stops running.
//!
//! ## Locking
//!
//! One lock per session, held only for the duration of one operation on that
//! session. A `stop` blocks for as long as its stop timeout, and it must not
//! be able to hold up a different session's start or the tray summary, so the
//! registry lock is never held across a lifecycle operation. Events are
//! published after the session lock is released, so a sink can never deadlock
//! against the operation that produced it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use serde::Serialize;

use crate::config::{self, LogSource, SessionConfig, SessionType};
use crate::logging::{
    self, policy_state, BufferLimits, LogError, LogPlan, LogRoots, LogStatus, OutputSink, RunLog,
    RunLogHandle, RunOutcome, Stream, TerminalBuffer, DEFAULT_LOG_LIMITS,
};
use crate::process::{
    ExitStatus, ManagedProcess, OutputMode, ProcessSpec, StopOutcome, StopReport,
};
use crate::pty::{Pty, PtySpec, DEFAULT_COLS, DEFAULT_ROWS, MAX_DIMENSION};

use super::event::{
    AppSummary, AppSummaryChanged, RunRecordUpdated, SessionEvent, SessionStateChanged,
    TerminalOutput,
};
use super::runtime::{RunId, RunRecord, SessionErrorInfo, SessionRuntime, Timestamp};
use super::state::SessionStatus;
use super::terminal::{
    OutputBatch, OutputRelay, RetainedChunk, TerminalAttachment, TerminalPump, PUMP_TICK,
};

/// Something that wants to hear about lifecycle changes.
///
/// Implementations must not call back into the [`SessionCore`] that produced
/// the event: events are published after the session lock is released, but
/// re-entering with an operation that waits on the same session would still be
/// a deadlock waiting to happen.
pub trait EventSink: Send + Sync + 'static {
    fn publish(&self, event: SessionEvent);
}

/// A sink that discards everything, for callers with no listener yet.
pub struct NoopSink;

impl EventSink for NoopSink {
    fn publish(&self, _event: SessionEvent) {}
}

/// Why a lifecycle operation was refused or failed
/// (`docs/DEVELOPMENT.md` §9: the message names the operation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionErrorKind {
    /// No session is registered under that id.
    UnknownSession,
    /// The session already exists; ids come from validated config and are
    /// unique there, so this means two registrations for one id.
    AlreadyRegistered,
    /// The state machine does not allow this move from the session's current
    /// state (spec §5 rule 1).
    InvalidTransition,
    /// The operation is not wired for this session type yet.
    Unsupported,
    /// The operation was allowed by the state machine but could not be carried
    /// out: an unusable command or working directory, a start the supervisor
    /// refused, a stop that could not confirm the tree was gone.
    Failed,
}

/// A refused or failed lifecycle operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionError {
    pub kind: SessionErrorKind,
    pub session_id: String,
    /// The operation that was refused, e.g. `start` or `force_stop`.
    pub operation: String,
    /// Actionable, human-readable message.
    pub message: String,
    /// The state the session was in when the move was refused.
    pub from: Option<SessionStatus>,
}

impl SessionError {
    /// No session is registered under `session_id`.
    pub fn unknown_session(session_id: &str, operation: &str) -> Self {
        SessionError {
            kind: SessionErrorKind::UnknownSession,
            session_id: session_id.to_owned(),
            operation: operation.to_owned(),
            message: format!("no session is registered as `{session_id}`"),
            from: None,
        }
    }

    /// The state machine does not allow the move `from -> to`.
    pub fn invalid_transition(
        session_id: &str,
        operation: &str,
        from: SessionStatus,
        to: SessionStatus,
    ) -> Self {
        SessionError {
            kind: SessionErrorKind::InvalidTransition,
            session_id: session_id.to_owned(),
            operation: operation.to_owned(),
            message: format!(
                "cannot {operation} a session that is {} — that would be a {} -> {} \
                 transition, which the lifecycle does not allow",
                from.as_str(),
                from.as_str(),
                to.as_str()
            ),
            from: Some(from),
        }
    }

    /// The operation is not wired for this session type yet.
    pub fn unsupported(session_id: &str, operation: &str, message: impl Into<String>) -> Self {
        SessionError {
            kind: SessionErrorKind::Unsupported,
            session_id: session_id.to_owned(),
            operation: operation.to_owned(),
            message: message.into(),
            from: None,
        }
    }

    /// The operation was allowed but could not be carried out.
    pub fn failed(
        session_id: &str,
        operation: &str,
        message: impl Into<String>,
        from: Option<SessionStatus>,
    ) -> Self {
        SessionError {
            kind: SessionErrorKind::Failed,
            session_id: session_id.to_owned(),
            operation: operation.to_owned(),
            message: message.into(),
            from,
        }
    }
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} on session `{}` failed: {}",
            self.operation, self.session_id, self.message
        )
    }
}

impl std::error::Error for SessionError {}

/// One session's mutable state, behind its own lock.
struct SessionState {
    config: SessionConfig,
    runtime: SessionRuntime,
    /// The current run, shared with its watcher so the watcher can wait on it
    /// without holding this lock.
    run: Option<Run>,
    /// Bumped on every start. A watcher captures the generation it was started
    /// for and refuses to publish an exit that belongs to a superseded run.
    generation: u64,
    /// The current run's record, open until the run ends (spec §4). Kept here
    /// rather than in the snapshot because the snapshot is the UI's view and
    /// the record is the run-history entry.
    record: Option<RunRecord>,
    /// The bounded scrollback (`docs/LOGGING.md` §8).
    ///
    /// Per session, not per run: restarting a service must not erase what the
    /// user was reading, and §8 keeps the buffer for every session whatever
    /// the logging policy is. Shared rather than owned so a reader outside this
    /// lock can hold the scrollback while a batch arrives.
    buffer: Arc<Mutex<TerminalBuffer>>,
    /// How this session's output becomes the batches the UI is sent (T07).
    ///
    /// Driven under this lock, so the offset it reports and the scrollback it
    /// describes are always read together (`session::terminal`).
    relay: OutputRelay,
    /// The current run's log, present while a run is in flight.
    log: Option<Arc<RunLogHandle>>,
    /// The threads reading the run's pipes, if it was started with capture.
    pump_drain: Option<PumpDrain>,
    /// The thread reading a terminal's output, for a terminal run.
    terminal_pump: Option<TerminalPump>,
    /// The geometry a terminal view has asked for (T07).
    ///
    /// Remembered while no terminal is running: the view knows its own size
    /// before the session starts, and a shell spawned at the default 80×24 and
    /// corrected a moment later would render its first screen at the wrong
    /// width — for a shell that draws a progress bar or wraps a banner, the
    /// corrected screen is not the same screen. `None` means the view has not
    /// said, and the PTY layer's default applies.
    terminal_size: Option<(u16, u16)>,
    /// Why logging is not working as configured, kept after the run's log is
    /// closed: a run whose file never appeared has to stay explainable
    /// (`docs/LOGGING.md` §1.4 — the user must never be left thinking output is
    /// being recorded when it is not).
    log_problem: Option<LogError>,
}

/// One session's current run, whichever kind it is.
///
/// A service runs as a supervised process (T03) and an interactive terminal as
/// a PTY (T02); the lifecycle above them is the same one, so the difference is
/// confined to this enum rather than to two copies of the state machine. The
/// methods here are exactly the questions the lifecycle asks of a run.
///
/// Cloning it clones a handle (`Arc`), never a run: a caller that needs to work
/// on a run without holding the session lock takes one of these, and the run it
/// refers to is the same run.
#[derive(Clone)]
enum Run {
    Process(Arc<ManagedProcess>),
    Terminal(Arc<Pty>),
}

impl Run {
    /// The terminal this run hosts, for the operations only a terminal has
    /// (typing into it, resizing it). `None` for a supervised process, which
    /// has no attached stdin in the MVP.
    fn as_terminal(&self) -> Option<&Arc<Pty>> {
        match self {
            Run::Process(_) => None,
            Run::Terminal(pty) => Some(pty),
        }
    }

    /// Wait up to `timeout` for the run to end on its own.
    fn wait_for_exit(&self, timeout: std::time::Duration) -> Option<ExitStatus> {
        match self {
            Run::Process(run) => run.wait_for_exit(timeout),
            Run::Terminal(pty) => pty.wait_for_exit(timeout),
        }
    }

    fn exit_status(&self) -> Option<ExitStatus> {
        match self {
            Run::Process(run) => run.exit_status(),
            Run::Terminal(pty) => pty.exit_status(),
        }
    }

    /// End the run, waiting at most `timeout` for a graceful exit.
    ///
    /// The two kinds do not share a ladder, and that is the platform's answer
    /// rather than a shortcut here: a supervised service is asked to stop and
    /// then has to be terminated (`docs/DECISIONS.md` D-007), while a terminal's
    /// graceful gesture is *input* — Ctrl+C, which the user sends through the
    /// keyboard and which does not close the session (spec §7). Closing a
    /// terminal is closing the console, so there is nothing to ask gracefully;
    /// [`Pty::kill`] terminates the shell and confirms it is gone.
    fn stop(&self, timeout: std::time::Duration) -> Result<StopReport, String> {
        match self {
            Run::Process(run) => run.stop(timeout).map_err(|error| error.to_string()),
            Run::Terminal(pty) => stop_terminal(pty),
        }
    }

    /// End the run now, without waiting for it to unwind.
    fn force_stop(&self) -> Result<StopReport, String> {
        match self {
            Run::Process(run) => run.force_stop().map_err(|error| error.to_string()),
            Run::Terminal(pty) => stop_terminal(pty),
        }
    }
}

/// Close a terminal and confirm the shell is gone.
///
/// `Pty::kill` is its own barrier — it returns `Ok` only once the shell's exit
/// has been observed (`crate::pty`) — so the report describes an outcome that
/// has already happened. `graceful_delivered` is `false` because no signal was
/// delivered: there is no graceful channel to a console, and claiming one
/// would tell the UI a courtesy was extended that never was (the same reason
/// the process layer reports that flag rather than assuming it).
fn stop_terminal(pty: &Pty) -> Result<StopReport, String> {
    let already_exited = pty.exit_status();
    pty.kill().map_err(|error| error.to_string())?;
    Ok(StopReport {
        outcome: match already_exited {
            Some(_) => StopOutcome::AlreadyExited,
            None => StopOutcome::Exited,
        },
        exit: pty.exit_status().unwrap_or(ExitStatus { code: None }),
        graceful_delivered: false,
    })
}

/// What a start will spawn, decided before anything is mutated.
///
/// Resolving this first is what keeps "a session that cannot start" from
/// passing through `Starting`: an unusable shell or command is refused while
/// the session is still exactly as it was.
#[derive(Debug)]
enum StartSpec {
    Process(ProcessSpec),
    Terminal(PtySpec),
}

/// A run that has been spawned but not yet wired into its session.
enum Spawned {
    Process(ManagedProcess),
    Terminal(Arc<Pty>),
}

impl Spawned {
    fn pid(&self) -> u32 {
        match self {
            Spawned::Process(run) => run.pid(),
            Spawned::Terminal(pty) => pty.pid(),
        }
    }
}

/// What a successful start still has to wire, once the state is `Running`.
///
/// The generation travels with it so the watcher that is started afterwards
/// waits on the run this start produced, not on whatever replaced it.
enum Started {
    Process {
        output: Option<crate::process::ProcessOutput>,
        generation: u64,
    },
    Terminal {
        pty: Arc<Pty>,
        generation: u64,
        /// The configured `initial_command`, typed once the terminal is up.
        initial: Option<String>,
    },
}

impl Started {
    fn generation(&self) -> u64 {
        match self {
            Started::Process { generation, .. } | Started::Terminal { generation, .. } => {
                *generation
            }
        }
    }
}

/// Build the spec for a startable session, or explain why it has none.
///
/// The session's type decides which layer hosts it: a service is a supervised
/// process (T03), an interactive terminal is a PTY (T02, T07). One dispatch
/// rather than a type check inside each builder, so neither builder can be
/// asked to host something it does not own.
fn start_spec(
    config: &SessionConfig,
    terminal_size: Option<(u16, u16)>,
    id: &str,
    planned: &PlannedRun,
) -> Result<StartSpec, SessionError> {
    match config.session_type {
        SessionType::Service => process_spec(config, id, planned).map(StartSpec::Process),
        SessionType::Terminal => terminal_spec(config, terminal_size, id).map(StartSpec::Terminal),
    }
}

/// Build the spec for an interactive terminal.
///
/// `shell` may carry arguments (`pwsh -NoProfile`), split exactly as a
/// service's `command` is, so the two configuration forms behave alike. The
/// geometry is the one a view asked for, or the PTY layer's default before any
/// view has said (T07).
fn terminal_spec(
    config: &SessionConfig,
    terminal_size: Option<(u16, u16)>,
    id: &str,
) -> Result<PtySpec, SessionError> {
    const OPERATION: &str = "start";

    // Validation (T01) guarantees both of these for a terminal session; the
    // checks are here for the same reason `process_spec` has them — the
    // message a user needs if a config ever reaches here through some other
    // path, rather than a panic or a shell in the wrong directory.
    let shell = config
        .shell
        .as_deref()
        .map(str::trim)
        .filter(|shell| !shell.is_empty())
        .ok_or_else(|| {
            SessionError::failed(
                id,
                OPERATION,
                format!("session `{id}` has no `shell` to host on a terminal"),
                None,
            )
        })?;
    let Some(cwd) = config.cwd.clone() else {
        return Err(SessionError::failed(
            id,
            OPERATION,
            format!(
                "session `{id}` has no `cwd`; add one so the terminal opens in a known \
                 directory"
            ),
            None,
        ));
    };
    let (program, args) = split_command(shell).map_err(|reason| {
        SessionError::failed(
            id,
            OPERATION,
            format!("session `{id}` has an unusable shell: {reason}"),
            None,
        )
    })?;

    let (cols, rows) = terminal_size.unwrap_or((DEFAULT_COLS, DEFAULT_ROWS));
    Ok(PtySpec::new(program, cwd)
        .with_args(args)
        .with_size(cols, rows))
}

/// Read an interactive terminal's output into its session, until it stops.
///
/// The loop is the terminal's whole output path: it takes what the console
/// host produced, hands it to the session (which is where the scrollback, the
/// run log and the batches the UI is sent all come from), and publishes what
/// is due on every tick. It ends when the run is over — the caller sets the
/// flag once the run has been finalized — or when the console host itself has
/// gone away.
fn pump_terminal(
    core: SessionCore,
    session_id: String,
    generation: u64,
    pty: Arc<Pty>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;

    loop {
        match pty.read_output(PUMP_TICK) {
            Some(chunk) => core.take_output_batch(&session_id, generation, Stream::Stdout, &chunk),
            // Nothing arrived: this is the tick that publishes output which has
            // waited out its batch window.
            None => core.flush_output(&session_id, generation, false),
        }
        if stop.load(Ordering::Acquire) || pty.output_ended() {
            break;
        }
    }
    // The last bytes of a run must not wait for a window that will never be
    // needed again.
    core.flush_output(&session_id, generation, true);
}

/// The registry plus everything needed to publish.
///
/// Cheap to clone: every clone shares the same sessions, which is how a
/// watcher thread and the IPC layer address the same state.
#[derive(Clone)]
pub struct SessionCore {
    sessions: Arc<Mutex<BTreeMap<String, Arc<Mutex<SessionState>>>>>,
    sink: Arc<dyn EventSink>,
    /// Where Hub-written logs and run metadata go, or `None` for a core with
    /// nowhere to write — a test, or a headless build with no app-data
    /// directory. A rootless core resolves every policy that needs a file to
    /// `off` and says so rather than writing into the current directory.
    roots: Option<LogRoots>,
    /// How much scrollback a session keeps (`docs/LOGGING.md` §8).
    scrollback: BufferLimits,
    /// How large one run's log file may grow before it is closed off.
    limits: logging::LogLimits,
}

/// Lock a mutex, surviving a previous holder's panic.
///
/// A poisoned lock means some other thread panicked mid-operation. Refusing to
/// serve any further request would take the whole app down with it, which is a
/// worse outcome than reading state that is merely stale.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl SessionCore {
    /// A core that publishes to `sink`, with nowhere to write logs.
    ///
    /// A core with no roots cannot persist anything, and a session configured
    /// to persist resolves to `off` with a reported reason rather than writing
    /// a log somewhere unintended. Production builds add the app-data roots
    /// with [`SessionCore::with_log_roots`]; the buffered scrollback every
    /// session has is unaffected either way (D-004).
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        SessionCore {
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
            sink,
            roots: None,
            scrollback: crate::logging::DEFAULT_BUFFER_LIMITS,
            limits: DEFAULT_LOG_LIMITS,
        }
    }

    /// A core with no listener.
    pub fn without_listener() -> Self {
        SessionCore::new(Arc::new(NoopSink))
    }

    /// Give this core the app-data roots its runs write logs and metadata to.
    ///
    /// Builder-style, and applied before anything is registered: the roots are
    /// read when a run starts, so changing them later would give two runs of
    /// one session different layouts.
    pub fn with_log_roots(mut self, roots: LogRoots) -> Self {
        self.roots = Some(roots);
        self
    }

    /// Builder-style: replace the buffer and file-size limits.
    ///
    /// Only tests do this. `docs/LOGGING.md` §9 leaves the numbers to
    /// implementation, and a value a user could change would be a value they
    /// could accidentally set to "keep everything".
    #[cfg(test)]
    fn with_limits(mut self, scrollback: BufferLimits, limits: logging::LogLimits) -> Self {
        self.scrollback = scrollback;
        self.limits = limits;
        self
    }

    /// Add a validated session to the registry.
    ///
    /// Registration is not a lifecycle operation: the session starts
    /// `Stopped`, and no process exists until [`SessionCore::start`]. Duplicate
    /// ids are refused rather than replaced — replacing a running session would
    /// drop the only handle accounting for its process.
    pub fn register(&self, config: SessionConfig) -> Result<SessionRuntime, SessionError> {
        let mut sessions = lock(&self.sessions);
        if sessions.contains_key(&config.id) {
            return Err(SessionError {
                kind: SessionErrorKind::AlreadyRegistered,
                session_id: config.id.clone(),
                operation: "register".to_owned(),
                message: format!("session `{}` is already registered", config.id),
                from: None,
            });
        }

        let runtime = SessionRuntime::stopped(config.id.clone(), config.logging.clone());
        let id = config.id.clone();
        sessions.insert(
            id.clone(),
            Arc::new(Mutex::new(SessionState {
                config,
                runtime: runtime.clone(),
                run: None,
                generation: 0,
                record: None,
                buffer: Arc::new(Mutex::new(TerminalBuffer::new(self.scrollback))),
                relay: OutputRelay::new(),
                log: None,
                pump_drain: None,
                terminal_pump: None,
                terminal_size: None,
                log_problem: None,
            })),
        );
        // Registration creates no directory and no file: a session that has
        // never run has nothing on disk, which is how "opening a shell does not
        // leave litter behind" stays true (D-004, `docs/LOGGING.md` §1.2). The
        // folder a session's logs *will* live in is computed, not created, and
        // reported through its log status (D-011).
        Ok(runtime)
    }

    /// One session's current snapshot.
    pub fn snapshot(&self, session_id: &str) -> Option<SessionRuntime> {
        self.handle(session_id)
            .map(|state| lock(&state).runtime.clone())
    }

    /// Every session's snapshot, in id order.
    pub fn snapshots(&self) -> Vec<SessionRuntime> {
        lock(&self.sessions)
            .values()
            .map(|state| lock(state).runtime.clone())
            .collect()
    }

    /// App-wide counts. Derived from the same state the snapshots come from,
    /// so the tray can never disagree with the window.
    pub fn summary(&self) -> AppSummary {
        let snapshots = self.snapshots();
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

    fn handle(&self, session_id: &str) -> Option<Arc<Mutex<SessionState>>> {
        lock(&self.sessions).get(session_id).cloned()
    }

    /// Publish the session's post-operation snapshot and the summary it
    /// implies.
    ///
    /// Both go out together so a listener tracking only the summary cannot
    /// observe the state change and the count in the opposite order. Callers
    /// must have released the session lock: this reads every session, and the
    /// lock order everywhere else is registry-then-session.
    fn publish_state_and_summary(&self, session_id: &str) {
        let Some(runtime) = self.snapshot(session_id) else {
            return;
        };
        let summary = self.summary();
        self.sink
            .publish(SessionEvent::StateChanged(SessionStateChanged {
                session_id: runtime.session_id.clone(),
                runtime,
            }));
        self.sink
            .publish(SessionEvent::AppSummaryChanged(AppSummaryChanged {
                summary,
            }));
    }

    /// Publish the current run record, if the session has one.
    fn publish_run(&self, session_id: &str) {
        let Some(handle) = self.handle(session_id) else {
            return;
        };
        let record = lock(&handle).record.clone();
        if let Some(run) = record {
            self.sink
                .publish(SessionEvent::RunRecordUpdated(RunRecordUpdated {
                    session_id: run.session_id.clone(),
                    run,
                }));
        }
    }

    /// Announce that a run has ended.
    ///
    /// Every path that ends a run goes through here — a start that failed, a
    /// stop the user asked for, and the watcher seeing a run end on its own.
    /// Splitting these apart is what let a stop close its run record in silence
    /// once already: the watcher declines to report an ending a stop owns, so a
    /// stop that forgot to publish left the closing of the record unreported
    /// everywhere.
    fn publish_ending(&self, session_id: &str) {
        self.publish_state_and_summary(session_id);
        self.publish_run(session_id);
    }

    /// Start a service session, and publish the states it passes through.
    ///
    /// On success the session is `Running` with a fresh run id and the run's
    /// process id. A refusal (unknown session, a transition the state machine
    /// does not allow, an unusable command) leaves the session exactly as it
    /// was; a spawn that fails moves it to `Error` with the reason recorded,
    /// because by then the session really is no longer stopped.
    pub fn start(&self, session_id: &str) -> Result<SessionRuntime, SessionError> {
        let handle = self
            .handle(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, "start"))?;

        // Claim the transition under the lock, then let go of it: a spawn is
        // the slow part, and holding the session lock across it would stall
        // nothing but this session's own watcher — but there is no reason to.
        //
        // Once `Starting` is published no other operation can move this
        // session: `Starting -> Starting` and `Starting -> Stopping` are both
        // outside the table, so a concurrent start or stop is refused rather
        // than interleaved with the spawn.
        let (spec, planned) = {
            let mut state = lock(&handle);

            let from = state.runtime.status;
            if !from.can_transition_to(SessionStatus::Starting) {
                return Err(SessionError::invalid_transition(
                    session_id,
                    "start",
                    from,
                    SessionStatus::Starting,
                ));
            }

            // The run's identity and its logging are settled before anything
            // is started: the run id and start time are what name its log file
            // (D-011), and a session whose policy cannot be honoured should
            // find that out before it has a process, not after.
            let planned = self.plan_run(&state.config, session_id);

            // Refuse before mutating anything: a session that cannot be
            // started has not "tried to start".
            let spec = start_spec(&state.config, state.terminal_size, session_id, &planned)?;

            state.runtime.status = SessionStatus::Starting;
            state.runtime.last_error = None;
            (spec, planned)
        };
        // The listener sees the start in flight, which is what a UI needs to
        // keep the action buttons from lying about what is happening.
        self.publish_state_and_summary(session_id);

        let spawned = match spec {
            StartSpec::Process(spec) => ManagedProcess::spawn(spec)
                .map(Spawned::Process)
                .map_err(|error| error.to_string()),
            StartSpec::Terminal(spec) => Pty::spawn(spec)
                .map(|pty| Spawned::Terminal(Arc::new(pty)))
                .map_err(|error| error.to_string()),
        };

        let outcome = {
            let mut state = lock(&handle);

            match spawned {
                Ok(spawned) => {
                    let pid = spawned.pid();
                    let started_at = planned.started_at;

                    let log = Arc::new(RunLogHandle::new(RunLog::start(
                        planned.plan.persistence.clone(),
                        self.limits,
                    )));

                    let record = RunRecord {
                        run_id: planned.run_id.clone(),
                        session_id: session_id.to_owned(),
                        started_at,
                        ended_at: None,
                        exit_code: None,
                        pid: Some(pid),
                        log_mode: state.config.logging.mode,
                        log_source: state.config.logging.source,
                        // Known for the modes that have a file from the start:
                        // a `capture` run's file exists now, and an `external`
                        // session is linked to the application's own log. The
                        // modes that decide at the end leave this to be filled
                        // in when the run ends.
                        log_file: planned.planned_log_file(&log),
                    };

                    // The generation marks this run; a watcher started for it
                    // must not be able to publish an exit against a later one.
                    state.generation += 1;
                    // The relay's offsets start over with the run they belong
                    // to; a view attached to the previous run must not be able
                    // to confuse its tail with this one's head.
                    let generation = state.generation;
                    state.relay.begin(generation);
                    state.record = Some(record);
                    state.log = Some(Arc::clone(&log));
                    state.log_problem = planned.plan.problem.clone();
                    let buffer = lock(&state.buffer).summary();
                    state.runtime.buffer = buffer;

                    state.runtime.pid = Some(pid);
                    state.runtime.run_id = Some(planned.run_id.clone());
                    state.runtime.started_at = Some(started_at);
                    state.runtime.exit_code = None;
                    state.runtime.status = SessionStatus::Running;

                    let started = match spawned {
                        Spawned::Process(run) => {
                            // Taken before the run goes into the registry: the
                            // pumping threads are started once this lock is
                            // released, and an untaken pipe would leave a
                            // chatty service blocked on its own output.
                            let output = planned
                                .plan
                                .persistence
                                .captures_output()
                                .then(|| run.take_output())
                                .flatten();
                            state.run = Some(Run::Process(Arc::new(run)));
                            state.runtime.pty_attached = false;
                            Started::Process {
                                output,
                                generation: state.generation,
                            }
                        }
                        Spawned::Terminal(pty) => {
                            // The reader is started and registered under this
                            // same lock: a stop arriving between the state
                            // update and the registration would find no reader
                            // to end, and the thread would then outlive the run
                            // it was reading for.
                            let stop = TerminalPump::stop_flag();
                            let reader = Arc::clone(&stop);
                            let core = self.clone();
                            let id = session_id.to_owned();
                            let generation = state.generation;
                            let read = Arc::clone(&pty);
                            let handle = std::thread::spawn(move || {
                                pump_terminal(core, id, generation, read, reader);
                            });
                            state.terminal_pump = Some(TerminalPump::new(stop, handle));
                            state.run = Some(Run::Terminal(Arc::clone(&pty)));
                            state.runtime.pty_attached = true;
                            Started::Terminal {
                                pty,
                                generation: state.generation,
                                initial: state.config.initial_command.clone(),
                            }
                        }
                    };

                    Ok((state.runtime.clone(), started))
                }
                Err(message) => {
                    state.runtime.status = SessionStatus::Error;
                    state.runtime.pid = None;
                    state.runtime.run_id = None;
                    state.runtime.pty_attached = false;
                    state.run = None;
                    state.record = None;
                    state.log = None;
                    state.log_problem = None;
                    state.runtime.last_error = Some(SessionErrorInfo {
                        operation: "start".to_owned(),
                        message: message.clone(),
                    });
                    Err(SessionError::failed(
                        session_id,
                        "start",
                        message,
                        Some(SessionStatus::Starting),
                    ))
                }
            }
        };

        // Published after the session lock is released, so a sink can never
        // deadlock against the operation that produced the event.
        match outcome {
            Ok((runtime, started)) => {
                // The run's output is wired before anything is published or
                // watched: a run can exit instantly, and a watcher that got
                // there first would close the log before the pipes (or the
                // terminal) that feed it were even handed over — losing the
                // whole of a short run's output.
                let generation = started.generation();
                match started {
                    Started::Process { output, .. } => {
                        self.start_capturing(session_id, generation, output);
                    }
                    Started::Terminal { pty, initial, .. } => {
                        self.type_initial_command(session_id, &pty, initial.as_deref());
                    }
                }
                self.publish_ending(session_id);

                // Started only after `Running` is on the wire. A run can end very
                // quickly, and a watcher that got there first would publish
                // `Exited` before `Running` — a listener would then be told a
                // session ended before it was told it started.
                std::thread::spawn({
                    let core = self.clone();
                    let handle = Arc::clone(&handle);
                    let session_id = session_id.to_owned();
                    move || watch_run(core, handle, session_id, generation)
                });
                Ok(runtime)
            }
            Err(error) => {
                self.publish_state_and_summary(session_id);
                Err(error)
            }
        }
    }

    /// Read a run's captured streams into its session, one thread per stream.
    ///
    /// The threads end when the pipes do — for a supervised process, when it
    /// exits and every handle it handed on is closed — so nothing has to stop
    /// them, and nothing has to wait for them: a run's last output arrives
    /// after the run is over, and the session it belongs to is asked for by id
    /// rather than held.
    fn start_capturing(
        &self,
        session_id: &str,
        generation: u64,
        output: Option<crate::process::ProcessOutput>,
    ) {
        let Some(output) = output else {
            return;
        };
        let sink = CapturedOutput {
            core: self.clone(),
            session_id: session_id.to_owned(),
            generation,
        };

        let (drained, done) = std::sync::mpsc::channel();
        let mut started = 0;
        for (reader, stream) in [
            (output.stdout, Stream::Stdout),
            (output.stderr, Stream::Stderr),
        ] {
            if let Some(reader) = reader {
                started += 1;
                let signalled = drained.clone();
                logging::pump(reader, stream, sink.clone(), move || {
                    let _ = signalled.send(());
                });
            }
        }
        drop(drained);

        if started > 0 {
            if let Some(handle) = self.handle(session_id) {
                lock(&handle).pump_drain = Some(PumpDrain {
                    done,
                    expected: started,
                });
            }
        }
    }

    /// Type a terminal's configured `initial_command` into it.
    ///
    /// Sent as input rather than assembled onto the shell's command line: it
    /// works for every shell without the session layer having to learn each
    /// one's "run this, then stay interactive" flag, and it is what actually
    /// happens — the command appears in the terminal as if it had been typed,
    /// which is also why the screen afterwards is explainable.
    ///
    /// A failure here is not a failed start: the terminal is up and the user
    /// can type. It is recorded as the session's last error, where it sits next
    /// to a running terminal that plainly did not run the command.
    fn type_initial_command(&self, session_id: &str, pty: &Pty, command: Option<&str>) {
        let Some(command) = command.map(str::trim).filter(|line| !line.is_empty()) else {
            return;
        };
        let Some(handle) = self.handle(session_id) else {
            return;
        };

        // Enter is carried as a carriage return, which is the byte a terminal
        // sends for the Enter key (`crate::pty`).
        let mut line = command.as_bytes().to_vec();
        line.push(b'\r');

        if let Err(error) = pty.write(&line) {
            lock(&handle).runtime.last_error = Some(SessionErrorInfo {
                operation: "start".to_owned(),
                message: format!(
                    "session `{session_id}` started, but its `initial_command` could not be \
                     sent to the terminal: {error}"
                ),
            });
        }
    }

    /// Take one batch of a run's output.
    ///
    /// Reached from a pumping thread — a service's pipe reader or a terminal's
    /// [`pump_terminal`] — never from a lifecycle call. The generation check is
    /// what keeps a superseded run's last bytes out of the session's buffer:
    /// after a restart, output that was already in the pipe belongs to the run
    /// that produced it, and showing it as the new run's output would be a lie
    /// the user cannot detect.
    ///
    /// The bytes go to three places, in this order, and the order is the
    /// decision D-004 records: the run's log first, then the session's
    /// scrollback, then the UI. A batch that reached the buffer but not the
    /// file would be a scrollback a user can read but cannot recover (memory
    /// is replenishable; a lost log is not), and a batch the UI missed is
    /// recoverable from the scrollback on the next attach.
    fn take_output_batch(&self, session_id: &str, generation: u64, stream: Stream, bytes: &[u8]) {
        let batch = {
            let Some(handle) = self.handle(session_id) else {
                return;
            };
            let mut state = lock(&handle);
            if state.generation != generation {
                return;
            }

            if let Some(log) = state.log.as_ref() {
                log.append(stream, bytes);
            }
            state.runtime.buffer = {
                let mut buffer = lock(&state.buffer);
                buffer.push(stream, bytes);
                buffer.summary()
            };

            // Published after the lock is released: the relay advances the
            // offset an attachment reads at the same moment the scrollback
            // takes these bytes, and a sink must not be able to deadlock
            // against the operation that produced its event.
            state.relay.push(generation, bytes, Instant::now())
        };

        if let Some(batch) = batch {
            self.publish_output(session_id, &batch);
        }
    }

    /// Publish whatever of a session's output is due.
    ///
    /// `force` publishes bytes that have not yet reached their batch window,
    /// which is what a run ending needs: the last lines of a session must not
    /// wait for a window nothing will ever open again.
    ///
    /// A no-op for a generation that is no longer current — checkable, since a
    /// reader thread can outlive the run it was reading for by a few bytes.
    fn flush_output(&self, session_id: &str, generation: u64, force: bool) {
        let Some(handle) = self.handle(session_id) else {
            return;
        };

        let batches = {
            let mut state = lock(&handle);
            if state.generation != generation {
                return;
            }
            let mut batches = Vec::new();
            while let Some(batch) = state.relay.take(Instant::now(), force) {
                batches.push(batch);
            }
            batches
        };

        for batch in &batches {
            self.publish_output(session_id, batch);
        }
    }

    /// Announce one batch of a run's output to the listeners.
    fn publish_output(&self, session_id: &str, batch: &OutputBatch) {
        self.sink
            .publish(SessionEvent::TerminalOutput(TerminalOutput::from_batch(
                session_id, batch,
            )));
    }

    /// Stop a session, giving it the default grace period to unwind.
    pub fn stop(&self, session_id: &str) -> Result<SessionRuntime, SessionError> {
        self.stop_with_timeout(session_id, crate::process::DEFAULT_STOP_TIMEOUT)
    }

    /// Stop a session, waiting at most `timeout` for a graceful exit before
    /// escalating to the force path.
    ///
    /// The state a stopped session lands in reports what actually happened:
    /// a run that answered the request ends `Stopped`, one that had to be
    /// terminated ends `Exited`. That is T03's `StopOutcome` decision, not a
    /// second guess made here.
    pub fn stop_with_timeout(
        &self,
        session_id: &str,
        timeout: std::time::Duration,
    ) -> Result<SessionRuntime, SessionError> {
        self.end_run(session_id, "stop", Some(timeout))
    }

    /// Terminate a session's run without waiting for it to unwind.
    pub fn force_stop(&self, session_id: &str) -> Result<SessionRuntime, SessionError> {
        self.end_run(session_id, "force_stop", None)
    }

    /// Replace a session's run with a fresh one, starting if it is not running.
    ///
    /// Spec §5 rule 2 — a restart must not launch a replacement until the
    /// previous managed process is confirmed stopped — is satisfied by
    /// construction rather than by a check here: the `stop` this calls is
    /// T03's stop barrier, which only returns `Ok` once the managed tree is
    /// observed gone. Starting first and hoping would be exactly the duplicate
    /// instance the rule exists to prevent.
    ///
    /// A session in `Starting` or `Stopping` is refused rather than queued:
    /// neither state can be interrupted, so there is no honest way to restart
    /// mid-flight.
    pub fn restart(&self, session_id: &str) -> Result<SessionRuntime, SessionError> {
        let current = self
            .snapshot(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, "restart"))?;

        if current.status == SessionStatus::Running {
            // The barrier. When this returns, nothing of the old run is left.
            self.stop(session_id)?;
        }

        self.start(session_id)
    }

    fn end_run(
        &self,
        session_id: &str,
        operation: &'static str,
        timeout: Option<std::time::Duration>,
    ) -> Result<SessionRuntime, SessionError> {
        use crate::process::StopOutcome;

        let handle = self
            .handle(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, operation))?;

        // Claim `Stopping` under the lock, then release it: the wait itself is
        // the slow part, and `session` is the only session it may hold up.
        //
        // `Stopping` is published here, before the request and the wait, rather
        // than once at the end. DEVELOPMENT §6 lists the UI update as a later
        // step, and this deliberately departs from that order — the departure
        // is recorded in D-015. What §6 requires of the ordering is unchanged:
        // request gracefully, wait, escalate against the managed tree, and only
        // report a terminal state once the tree is confirmed gone.
        let run = {
            let mut state = lock(&handle);

            let from = state.runtime.status;
            if !from.can_transition_to(SessionStatus::Stopping) {
                return Err(SessionError::invalid_transition(
                    session_id,
                    operation,
                    from,
                    SessionStatus::Stopping,
                ));
            }

            state.runtime.status = SessionStatus::Stopping;
            state.run.clone()
        };
        self.publish_state_and_summary(session_id);

        // A `Running` session with no run should be impossible; if the state
        // machine ever allowed it, saying so beats pretending a stop happened.
        let Some(run) = run else {
            let message = format!("session `{session_id}` owns no run to stop");
            let mut state = lock(&handle);
            state.runtime.status = SessionStatus::Error;
            // The snapshot reports the error, so it has to carry the reason
            // for it: §4 keeps a last structured error precisely so a failure
            // is explainable without a log.
            state.runtime.last_error = Some(SessionErrorInfo {
                operation: operation.to_owned(),
                message: message.clone(),
            });
            drop(state);
            self.publish_state_and_summary(session_id);
            return Err(SessionError::failed(
                session_id,
                operation,
                message,
                Some(SessionStatus::Stopping),
            ));
        };

        let report = match timeout {
            Some(timeout) => run.stop(timeout),
            None => run.force_stop(),
        };

        let outcome = {
            let mut state = lock(&handle);
            match report {
                Ok(report) => {
                    let ended = if report.outcome == StopOutcome::Exited {
                        SessionStatus::Stopped
                    } else {
                        SessionStatus::Exited
                    };
                    state.close_run(ended, report.exit.code);
                    Ok(state.runtime.clone())
                }
                Err(error) => {
                    let message = error.to_string();
                    state.runtime.status = SessionStatus::Error;
                    state.runtime.last_error = Some(SessionErrorInfo {
                        operation: operation.to_owned(),
                        message: message.clone(),
                    });
                    Err(SessionError::failed(
                        session_id,
                        operation,
                        message,
                        Some(SessionStatus::Stopping),
                    ))
                }
            }
        };

        // The run ended, so its record closed with it and has to be announced
        // alongside the state. A stop that failed left the record open and has
        // nothing new to say about it.
        match &outcome {
            Ok(_) => {
                // The user asked for this stop and it was carried out. Whatever
                // exit code the terminated process reported is an answer to
                // being stopped, not a failure (`RunEnding`).
                self.finalize_run(session_id, &handle, RunEnding::Requested);
                self.publish_ending(session_id);
            }
            Err(_) => {
                // The stop could not confirm the tree was gone, so the run may
                // still be alive — but this session is not waiting on it any
                // more, and an `on_error` log that never gets written because a
                // stop failed is the one case where the log matters most.
                self.finalize_run(session_id, &handle, RunEnding::Failed);
                self.publish_state_and_summary(session_id);
            }
        }
        outcome
    }

    /// The effective logging state of one session (`docs/LOGGING.md` §1.4).
    ///
    /// Answered whether or not a run exists: "is this being recorded, and
    /// where?" is a property of the configuration, and a UI that can only
    /// answer it for running sessions would have to guess the rest of the time.
    pub fn log_status(&self, session_id: &str) -> Option<LogStatus> {
        let handle = self.handle(session_id)?;
        let state = lock(&handle);
        Some(state.log_status(session_id, self.roots.as_ref()))
    }

    /// A session's retained scrollback, oldest first.
    ///
    /// Read on demand rather than carried in the snapshot: a snapshot is copied
    /// into every event, and a full scrollback must not make every state change
    /// expensive (spec §14). The frontend reads this when it needs to render a
    /// terminal, not on every tick.
    pub fn terminal_buffer(&self, session_id: &str) -> Option<Vec<logging::BufferedChunk>> {
        let handle = self.handle(session_id)?;
        let state = lock(&handle);
        let buffer = lock(&state.buffer);
        Some(buffer.chunks().cloned().collect())
    }

    /// What a terminal view needs to start rendering a session's stream.
    ///
    /// A terminal view is created when the user selects a session, and again
    /// whenever it is shown after being hidden — but the session itself is not
    /// restarted by either, so the view has to be able to pick up a stream that
    /// is already in flight. This answers with the retained scrollback, the
    /// run's generation, and the byte offset the scrollback reaches: the view
    /// replays the chunks, then appends the batches whose `end` is beyond that
    /// offset and drops the ones it has already shown — append or drop, never
    /// splice.
    ///
    /// That rule only holds if no batch can *straddle* the offset, and the
    /// flush below is what buys it. Output taken but not yet published would
    /// otherwise be published later as a batch starting before this offset and
    /// ending after it, and a view that has already replayed those bytes would
    /// have to cut the batch in half. Publishing what is pending, in the same
    /// lock hold that reads the offset, makes the offset a batch boundary by
    /// construction: every later batch starts exactly where this one ended.
    ///
    /// Answering for a session that is not running is deliberate: the view
    /// renders the last run's scrollback under its "not running" state rather
    /// than an empty panel that pretends there is nothing to see.
    pub fn terminal_attachment(
        &self,
        session_id: &str,
    ) -> Result<TerminalAttachment, SessionError> {
        const OPERATION: &str = "attach_terminal";
        let handle = self
            .handle(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, OPERATION))?;

        // Cut, read and release under one lock (see the note above); the
        // batches go out once it is released, as every publish in this module
        // does.
        let (attached, batches) = {
            let mut state = lock(&handle);

            let mut batches = Vec::new();
            while let Some(batch) = state.relay.take(Instant::now(), true) {
                batches.push(batch);
            }

            let (chunks, summary) = {
                let buffer = lock(&state.buffer);
                let chunks = buffer.chunks().map(RetainedChunk::encode).collect();
                (chunks, buffer.summary())
            };

            let attached = TerminalAttachment {
                session_id: session_id.to_owned(),
                pty_attached: state.runtime.pty_attached,
                generation: state.relay.generation(),
                emitted: state.relay.emitted(),
                chunks,
                buffer: summary,
            };
            (attached, batches)
        };

        for batch in &batches {
            self.publish_output(session_id, batch);
        }
        Ok(attached)
    }

    /// Send input to a session's terminal.
    ///
    /// Keystrokes go to the shell exactly as a terminal would deliver them, so
    /// Ctrl+C is the byte `0x03` travelling this way rather than an operation
    /// of its own: it interrupts what the shell is running, and does not close
    /// the session (spec §7). Only an interactive terminal has this path — a
    /// supervised service in the MVP runs without an attached stdin, and saying
    /// so beats accepting bytes that would go nowhere.
    pub fn terminal_write(&self, session_id: &str, bytes: &[u8]) -> Result<(), SessionError> {
        const OPERATION: &str = "terminal_write";
        let handle = self
            .handle(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, OPERATION))?;

        /// Where input could go for this session right now.
        enum Input {
            Pty(Arc<Pty>),
            /// A supervised run, which has no attached stdin in the MVP.
            Unattached,
            /// Nothing is running.
            Idle,
        }

        let (input, session_type) = {
            let state = lock(&handle);
            // A session accepts input while it is *running*: the handle alone
            // is not enough, because a session that has been stopped keeps it
            // (as a stopped service keeps its process handle) and the shell
            // behind it is gone.
            let input = match (&state.run, state.runtime.status) {
                (Some(Run::Terminal(pty)), SessionStatus::Running) => Input::Pty(Arc::clone(pty)),
                (Some(Run::Process(_)), _) => Input::Unattached,
                _ => Input::Idle,
            };
            (input, state.config.session_type)
        };

        match input {
            Input::Pty(pty) => pty.write(bytes).map_err(|error| {
                SessionError::failed(session_id, OPERATION, error.to_string(), None)
            }),
            Input::Unattached => Err(SessionError::unsupported(
                session_id,
                OPERATION,
                format!(
                    "session `{session_id}` runs as a supervised service, which has no \
                     attached stdin to type into"
                ),
            )),
            Input::Idle => Err(SessionError::failed(
                session_id,
                OPERATION,
                match session_type {
                    SessionType::Terminal => format!(
                        "session `{session_id}` has no running terminal; start it before \
                         typing into it"
                    ),
                    SessionType::Service => format!(
                        "session `{session_id}` is not running, and a service has no \
                         attached stdin to type into"
                    ),
                },
                None,
            )),
        }
    }

    /// Tell a session's terminal how large its view is.
    ///
    /// A live terminal is resized immediately. One that is not running
    /// *remembers* the size instead of refusing it, and that is the interesting
    /// half: a shell is started at the geometry its view has already measured,
    /// so its first screen is drawn at the right width rather than at the
    /// default and re-flowed a moment later. A resize is also not a user
    /// command that can fail — the view reports its own size whenever it
    /// changes — so a size arriving for a stopped session is not an error.
    pub fn terminal_resize(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<(), SessionError> {
        const OPERATION: &str = "terminal_resize";
        let handle = self
            .handle(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, OPERATION))?;

        if cols == 0 || rows == 0 || cols > MAX_DIMENSION || rows > MAX_DIMENSION {
            return Err(SessionError::failed(
                session_id,
                OPERATION,
                format!(
                    "a terminal needs at least 1x1 and at most {MAX_DIMENSION}x{MAX_DIMENSION} \
                     cells, got {cols}x{rows}"
                ),
                None,
            ));
        }

        let live = {
            let mut state = lock(&handle);
            state.terminal_size = Some((cols, rows));
            state.run.as_ref().and_then(Run::as_terminal).cloned()
        };

        if let Some(pty) = live {
            pty.resize(cols, rows).map_err(|error| {
                SessionError::failed(session_id, OPERATION, error.to_string(), None)
            })?;
        }
        Ok(())
    }

    /// Every registered session's validated configuration, in id order.
    ///
    /// The window renders a session from its configuration (name, purpose,
    /// close impact, port, cwd) and its snapshot (state, pid, uptime); this is
    /// the first half, and it is the same set the snapshots come from — one
    /// registry, so a session the window lists is one Session Core can act on.
    pub fn configs(&self) -> Vec<SessionConfig> {
        lock(&self.sessions)
            .values()
            .map(|state| lock(state).config.clone())
            .collect()
    }

    /// Commit the current run's `on_error` log now, instead of waiting for the
    /// run to end (`docs/LOGGING.md` §3: "save this run's log").
    ///
    /// Answers with the logging status afterwards, so a caller can show where
    /// the file went without a second round trip. A session with no run, or one
    /// whose policy does not write on request, is reported as the failed
    /// operation it is.
    pub fn save_run_log(&self, session_id: &str) -> Result<LogStatus, SessionError> {
        const OPERATION: &str = "save_run_log";
        let handle = self
            .handle(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, OPERATION))?;

        {
            let state = lock(&handle);
            let log = state.log.as_ref().ok_or_else(|| {
                SessionError::failed(
                    session_id,
                    OPERATION,
                    format!("session `{session_id}` has no run to save a log for"),
                    None,
                )
            })?;
            log.save_now();
        }
        // A failed save is not a failed operation: the file may simply not be
        // writable, and the status carries that reason (`last_error`) where the
        // UI can show it.
        self.log_status(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, OPERATION))
    }

    /// Start or stop recording a `manual` session's run
    /// (`docs/LOGGING.md` §3).
    ///
    /// Refused for any other mode: `always` is already recording and `off` has
    /// no file to start, so accepting the request would make the UI's switch
    /// mean different things for different sessions.
    pub fn set_log_recording(
        &self,
        session_id: &str,
        recording: bool,
    ) -> Result<LogStatus, SessionError> {
        const OPERATION: &str = "set_log_recording";
        let handle = self
            .handle(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, OPERATION))?;

        {
            let state = lock(&handle);
            if state.config.logging.mode != crate::config::EffectiveLogMode::Manual {
                return Err(SessionError::failed(
                    session_id,
                    OPERATION,
                    format!(
                        "session `{session_id}` is not configured with `logging.mode: manual`, so \
                         recording cannot be switched on and off"
                    ),
                    None,
                ));
            }
            let log = state.log.as_ref().ok_or_else(|| {
                SessionError::failed(
                    session_id,
                    OPERATION,
                    format!("session `{session_id}` has no run to record"),
                    None,
                )
            })?;
            if recording {
                log.start_recording();
            } else {
                log.stop_recording();
            }
        }
        self.log_status(session_id)
            .ok_or_else(|| SessionError::unknown_session(session_id, OPERATION))
    }

    /// Remove the Hub's own log files that retention says are past keeping
    /// (`docs/LOGGING.md` §9).
    ///
    /// The budget is per session, so a noisy service that reaches its own
    /// ceiling stops deleting its own oldest logs and never touches another
    /// session's. Passing a `session_id` narrows the sweep to that session —
    /// the "clean this session's old logs" action §10 lists — and `None`
    /// sweeps every session.
    ///
    /// Nothing is cleaned automatically, and nothing here deletes a file that
    /// is not the Hub's own: deleting a user's files is an action they take,
    /// not one that happens behind them when the app starts. The button that
    /// calls this is T10's, along with showing what a sweep would remove.
    pub fn cleanup_logs(&self, session_id: Option<&str>) -> logging::CleanupReport {
        let Some(roots) = &self.roots else {
            return logging::CleanupReport::default();
        };
        logging::retention::enforce(
            &roots.logs_dir,
            session_id,
            &logging::DEFAULT_RETENTION,
            logging::clock::now(),
        )
    }

    /// A session's completed runs, newest first (`docs/LOGGING.md` §6).
    ///
    /// Read from the run metadata on disk rather than from memory: a run
    /// history that only existed while the app was open would not survive the
    /// restart that a user is most likely to want it after.
    pub fn run_history(&self, session_id: &str) -> logging::RunHistory {
        let Some(roots) = &self.roots else {
            return logging::RunHistory::default();
        };
        logging::run_history(&roots.metadata_dir, session_id)
    }

    /// Close the current run's log, finish its record, and file the record.
    ///
    /// Every path that ends a run calls this — a stop the user asked for, a
    /// failure to stop, and the watcher seeing a run end on its own — because a
    /// run whose log is never closed is a run whose output is still in a
    /// buffer, and one whose record is never filed is missing from the history
    /// the user will look at next time.
    fn finalize_run(&self, session_id: &str, handle: &Arc<Mutex<SessionState>>, ending: RunEnding) {
        // First, with the lock released: let the pipes be read out. A run that
        // exited with output still in flight has not finished producing it, and
        // closing its log now would file a file that is missing exactly the
        // lines a user opens it for.
        if let Some(drain) = lock(handle).pump_drain.take() {
            drain.wait();
        }

        // A terminal's reader is stopped rather than waited for. The console
        // host outlives the shell it hosted (`crate::pty`), so a reader that
        // only stopped when its stream ended would keep reading a terminal
        // that is over until the session's next run replaced it — an invisible
        // terminal still working, which spec §14 rules out. Stopping it takes
        // the last of the output with it: the reader flushes before it returns.
        //
        // Taken out of the lock before it is waited on, and not joined from
        // inside a guard: that flush needs this very lock, so waiting on the
        // thread with it held would wait forever.
        let pump = lock(handle).terminal_pump.take();
        if let Some(pump) = pump {
            pump.stop_and_wait();
        }

        // Anything taken but not yet published is due: the run is over, so no
        // batch window will open for these bytes again.
        {
            let generation = lock(handle).generation;
            self.flush_output(session_id, generation, true);
        }

        let record = {
            let mut state = lock(handle);

            if let Some(log) = state.log.take() {
                let exit_code = state.record.as_ref().and_then(|record| record.exit_code);
                let written = log.finish(ending.outcome(exit_code));

                if let Some(problem) = log.last_error() {
                    state.log_problem = Some(problem);
                }
                // Nothing was produced. An `external` session keeps its link to
                // the application's log — that file exists whether or not the
                // Hub wrote anything — and every other mode's record must stop
                // naming the file it planned but never wrote
                // (`docs/MVP_IMPLEMENTATION_SPEC.md` §4: a path *when one
                // exists*).
                let missing = (state.config.logging.source == LogSource::External)
                    .then(|| state.config.logging.external_path.clone())
                    .flatten();

                if let Some(record) = state.record.as_mut() {
                    record.log_file = written.map(|path| path.display().to_string()).or(missing);
                }
            }
            state.record.clone()
        };

        // Only a run that ended has a history entry. A record with no end is
        // an open run — a stop that could not confirm the tree was gone leaves
        // one — and filing it would put a run in the history that a reader
        // cannot tell from one still going.
        if let Some(record) = record.filter(|record| record.ended_at.is_some()) {
            self.file_run_record(session_id, &record);
        }
    }

    /// Write a finished run's record to the metadata directory
    /// (`docs/LOGGING.md` §6).
    ///
    /// A failure is recorded against the session rather than raised: the run
    /// itself ran fine, and a missing history entry must not be reported as a
    /// failed run. It is also not silent — it reaches the UI through the log
    /// status, next to the log file it is about.
    fn file_run_record(&self, session_id: &str, record: &RunRecord) {
        let Some(roots) = &self.roots else {
            return;
        };
        if let Err(error) =
            logging::write_run(&roots.metadata_dir, record, config::local_utc_offset_secs())
        {
            if let Some(handle) = self.handle(session_id) {
                lock(&handle).log_problem = Some(error);
            }
        }
    }

    /// Decide a run's logging before it starts.
    ///
    /// The run id and start time are minted here because they name the log
    /// file, and the file's path is what the policy is resolved against: a
    /// policy that needs a file the Hub cannot name is downgraded, and the
    /// reason travels with the run (`docs/LOGGING.md` §1.4).
    fn plan_run(&self, config: &SessionConfig, session_id: &str) -> PlannedRun {
        let run_id = RunId::mint();
        let started_at = Timestamp::now();

        let log_file = (config.logging.source == LogSource::Captured)
            .then(|| self.run_log_file(session_id, &run_id, started_at))
            .flatten();

        PlannedRun {
            plan: LogPlan::resolve(&config.logging, log_file),
            run_id,
            started_at,
        }
    }

    /// The path this run's log would live at, or `None` when the Hub has no
    /// writable root or the session id cannot become a directory name.
    fn run_log_file(
        &self,
        session_id: &str,
        run_id: &RunId,
        started_at: Timestamp,
    ) -> Option<PathBuf> {
        let roots = self.roots.as_ref()?;
        config::run_log_path(
            &roots.logs_dir,
            session_id,
            started_at.unix_secs()?,
            config::local_utc_offset_secs(),
            run_id.as_str(),
        )
    }
}

/// How a run ended, in the terms the logging layer decides with.
///
/// This is not [`SessionStatus`]: a run stopped by request and a run that fell
/// over both leave `Exited` behind when the exit code is zero, and `Error`
/// covers both an unexpected failure and a stop that could not be carried out.
/// The logging layer needs the difference — a stop the user asked for is not
/// evidence of a problem (`docs/LOGGING.md` §3) — so the caller that knows
/// says which it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunEnding {
    /// The user stopped it, and the stop was carried out.
    Requested,
    /// It ended without anyone asking.
    OnItsOwn,
    /// The lifecycle called it a failure: an error state, or a stop that could
    /// not confirm the tree was gone.
    Failed,
}

impl RunEnding {
    /// The outcome `RunEnding` implies for a run that ended with `exit_code`.
    fn outcome(self, exit_code: Option<u32>) -> RunOutcome {
        match self {
            RunEnding::Requested => RunOutcome::stopped(exit_code),
            RunEnding::OnItsOwn => RunOutcome::exited(exit_code),
            RunEnding::Failed => RunOutcome::failed(exit_code),
        }
    }
}

/// Everything a run needs that is decided before it starts.
///
/// The run id and start time are minted here rather than after the spawn
/// because they are what name the run's log file (D-011): a run that starts and
/// immediately fails still has one well-defined file to look for, and whether
/// there is a file at all is settled once, before anything is launched.
struct PlannedRun {
    run_id: RunId,
    started_at: Timestamp,
    plan: LogPlan,
}

impl PlannedRun {
    /// Whether the Hub takes this run's stdout/stderr at all
    /// (`docs/LOGGING.md` §8: the scrollback is kept for every session; only
    /// `source: external` declines, because the application already has it).
    fn captures_output(&self) -> bool {
        self.plan.persistence.captures_output()
    }

    /// The `log_file` a run's record starts with, from what the run log
    /// actually has.
    ///
    /// Taken from the log itself rather than from the plan: a `capture` run
    /// whose file could not be created must not leave a record naming a path
    /// that does not exist (`docs/MVP_IMPLEMENTATION_SPEC.md` §4 — "persisted
    /// log path **when one exists**"). `external` is the other answer this can
    /// give, and it is a link to a file the application owns and already
    /// wrote. `on_error` and `manual` write nothing until something happens,
    /// so they leave this empty and it is filled in when the run ends — which
    /// is the difference between "there is a log" and "there may be one".
    fn planned_log_file(&self, log: &RunLogHandle) -> Option<String> {
        log.produced_path()
            .or_else(|| self.plan.persistence.external_path().cloned())
            .map(|path| path.display().to_string())
    }
}

/// Routes one run's captured batches to the session that owns it.
///
/// Holds a session id and a generation rather than a session handle: the run
/// may be superseded while its last output is still in the pipe, and the
/// generation is what lets the session refuse bytes that no longer belong to
/// the run it is showing.
#[derive(Clone)]
struct CapturedOutput {
    core: SessionCore,
    session_id: String,
    generation: u64,
}

impl OutputSink for CapturedOutput {
    fn take(&self, stream: Stream, bytes: &[u8]) {
        self.core
            .take_output_batch(&self.session_id, self.generation, stream, bytes);
    }
}

/// How long a run's end waits for its pipes to be read out.
///
/// A process can exit with output still in its pipe, and the run is not over
/// until that output has been taken (`docs/LOGGING.md` §3: an `on_error` run's
/// whole value is the output right before the failure). The wait is bounded
/// because a pipe is only closed when *every* writer lets go, and a service
/// that hands one to a child it outlives would otherwise hold the session
/// here for as long as that child lives.
///
/// The common case returns in microseconds — a pipe's worth of already-written
/// bytes — and the bound only matters for the case that has no answer anyway:
/// output from a run whose pipe is still held will reach the in-memory
/// scrollback, and a log file that has already been closed takes no more.
const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

/// The pumps reading one run's pipes, and how many of them there are.
struct PumpDrain {
    done: std::sync::mpsc::Receiver<()>,
    expected: usize,
}

impl PumpDrain {
    /// Wait, up to [`DRAIN_TIMEOUT`], for every pump to report that its stream
    /// ended.
    ///
    /// Must be called with no session lock held: the pumps need that lock to
    /// hand over their last batches, and waiting for them while holding it
    /// would be a deadlock that ends in the timeout — with exactly the output
    /// this exists to save missing.
    fn wait(self) {
        let deadline = std::time::Instant::now() + DRAIN_TIMEOUT;
        for _ in 0..self.expected {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if self.done.recv_timeout(left).is_err() {
                // Timed out or every sender is gone: either way there is
                // nothing more to wait for.
                return;
            }
        }
    }
}

impl SessionState {
    /// Record that the current run has ended, in the snapshot and in the run
    /// record together.
    ///
    /// One method rather than two updates at each call site: the snapshot is
    /// what the UI renders and the record is what run history keeps, and they
    /// describe the same event. Letting a caller update one and forget the
    /// other is how they drift.
    fn close_run(&mut self, status: SessionStatus, code: Option<u32>) {
        self.runtime.status = status;
        self.runtime.exit_code = code;
        self.runtime.pid = None;
        // Nothing is attached to a session that is not running. The terminal
        // handle stays where it is (as a stopped service's process handle
        // does) and is released when the next run replaces it; what must not
        // survive is the claim that a terminal view could type into it.
        self.runtime.pty_attached = false;

        if let Some(record) = self.record.as_mut() {
            record.ended_at = Some(Timestamp::now());
            record.exit_code = code;
        }
    }

    /// The effective logging state of this session (`docs/LOGGING.md` §1.4).
    fn log_status(&self, session_id: &str, roots: Option<&LogRoots>) -> LogStatus {
        let logging = &self.config.logging;
        // While a run is in flight the run log answers "what is happening
        // now"; between runs the policy answers "what will happen next".
        // `manual` resolves to `off` in both, which is the point of it.
        let state = match (&self.log, self.runtime.status) {
            (
                Some(log),
                SessionStatus::Starting | SessionStatus::Running | SessionStatus::Stopping,
            ) => log.state(),
            _ => policy_state(logging.source, logging.mode),
        };

        // The file this session is writing *now* when it is writing one, and
        // otherwise the file the last run left behind. The two differ for the
        // modes that decide mid-run: an `on_error` run that has been saved, and
        // a `manual` run that is recording, have a file before their record
        // does — and a UI that only knew about the record would report "not
        // recording" about a session that is writing to disk at that moment.
        let live_file = self
            .log
            .as_ref()
            .filter(|log| log.is_writing())
            .and_then(|log| log.hub_log_path())
            .map(|path| path.display().to_string());

        LogStatus {
            session_id: session_id.to_owned(),
            mode: logging.mode,
            source: logging.source,
            state,
            truncated: self.log.as_ref().is_some_and(|log| log.is_truncated()),
            log_file: live_file.or_else(|| {
                // From the run record, so a `capture` run's file stays named
                // after the run has ended and an `on_error` run that failed
                // keeps the file it just wrote.
                self.record
                    .as_ref()
                    .and_then(|record| record.log_file.clone())
            }),
            external_log: logging.external_path.clone(),
            session_log_dir: session_dir(
                roots,
                session_id,
                self.runtime
                    .started_at
                    .or(self.record.as_ref().map(|record| record.started_at)),
            ),
            records_input: false,
            buffer: lock(&self.buffer).summary(),
            last_error: self
                .log
                .as_ref()
                .and_then(|log| log.last_error())
                .or_else(|| self.log_problem.clone()),
        }
    }
}

/// The folder a session's Hub-written logs are in, or `None` without roots or
/// a filesystem-safe id. Computed, never created: an empty directory for a
/// session that has never run is noise (D-004).
///
/// The month is the one the session's *current* run started in, not the one it
/// is now (`docs/LOGGING.md` §5 files runs by the month they started). For a
/// session left running across a month boundary those differ, and a "open the
/// log folder" that opened the wrong month's folder would show a user an empty
/// directory next to a log that exists.
fn session_dir(
    roots: Option<&LogRoots>,
    session_id: &str,
    started_at: Option<Timestamp>,
) -> Option<String> {
    let roots = roots?;
    let at = started_at.unwrap_or_else(Timestamp::now);
    let dir = config::session_log_dir(
        &roots.logs_dir,
        session_id,
        at.unix_secs()?,
        config::local_utc_offset_secs(),
    )?;
    Some(dir.display().to_string())
}

/// How often a run watcher re-checks whether it has been superseded.
///
/// The watcher is blocked on the supervisor's exit condition between ticks, so
/// this costs one wake-up per interval per running session and no CPU worth
/// measuring — well inside the budget D-009 sets for background work.
const WATCH_TICK: std::time::Duration = std::time::Duration::from_millis(100);

/// Notice a run ending without anyone asking, and record it.
///
/// This is the mechanism behind spec §5 rule 4 — an unexpected exit updates
/// session state even while the UI is hidden. Nothing here consults the UI:
/// the watcher owns the run, the session state is the truth, and the sink is
/// where anybody interested finds out.
///
/// It only ever applies to the run it was started for. If the session was
/// restarted, or a stop already took ownership of the ending, the watcher
/// returns without publishing: a second "ended" for a run that is already
/// accounted for would be worse than the missed wake-up it costs.
fn watch_run(
    core: SessionCore,
    handle: Arc<Mutex<SessionState>>,
    session_id: String,
    generation: u64,
) {
    let run = {
        let state = lock(&handle);
        if state.generation != generation {
            return;
        }
        match &state.run {
            Some(run) => run.clone(),
            None => return,
        }
    };

    loop {
        if run.wait_for_exit(WATCH_TICK).is_some() {
            break;
        }
        // Still alive: stop waiting if this watcher no longer owns the run.
        if lock(&handle).generation != generation {
            return;
        }
        // This tick is also the pipeline's slow flush for a run whose output
        // arrives in bursts that never fill a batch: a service's reader has no
        // tick of its own (it blocks on the pipe), so without this the last
        // lines before a quiet spell would sit unpublished. For a terminal the
        // reader ticks faster and this is usually a no-op.
        core.flush_output(&session_id, generation, false);
    }

    let ending = {
        let mut state = lock(&handle);
        // Superseded by a restart, or a stop/report already owns this ending.
        if state.generation != generation || state.runtime.status != SessionStatus::Running {
            return;
        }
        let code = run.exit_status().and_then(|exit| exit.code);

        // Spec §5 allows both `Running -> Exited` and `Running -> Error`, so
        // the two are not interchangeable. A run that ends by itself with a
        // failing status has failed — saying `Exited` would show a crashed
        // service as a cleanly stopped one, leave the error count at zero, and
        // leave §11's "Restart Failed" with nothing to notice. A run with no
        // code to inspect is not called a failure.
        let ended = match code {
            Some(0) | None => SessionStatus::Exited,
            Some(_) => SessionStatus::Error,
        };
        if ended == SessionStatus::Error {
            state.runtime.last_error = Some(SessionErrorInfo {
                operation: "run".to_owned(),
                message: format!(
                    "the run ended on its own with exit code {}",
                    code.unwrap_or_default()
                ),
            });
        }
        state.close_run(ended, code);
        match ended {
            SessionStatus::Error => RunEnding::Failed,
            // `Exited` after nobody asked: a clean exit that was not requested.
            _ => RunEnding::OnItsOwn,
        }
    };

    // The run's log is closed and its record filed before anything is
    // published, so a listener that reacts to the ending already has the path
    // of the file it produced.
    core.finalize_run(&session_id, &handle, ending);
    core.publish_ending(&session_id);
}

/// Build the spec for a supervised service run, or explain why there is none.
///
/// Reached only for a service session ([`start_spec`] dispatches by type); a
/// terminal's run is a PTY, which T02 hosts and T07 wires to this same
/// timeline.
///
/// The run's resolved logging decides whether its output is piped back: a
/// session whose policy captures (`docs/LOGGING.md` §8 — every session keeps a
/// scrollback) gets [`OutputMode::Capture`], and one linked to an
/// application-owned log does not, because taking its output would duplicate
/// what the application already writes (D-005).
fn process_spec(
    config: &SessionConfig,
    id: &str,
    planned: &PlannedRun,
) -> Result<ProcessSpec, SessionError> {
    const OPERATION: &str = "start";

    let Some(command) = config.command.as_deref() else {
        return Err(SessionError::failed(
            id,
            OPERATION,
            format!("session `{id}` has no `command` to start"),
            None,
        ));
    };
    let Some(cwd) = config.cwd.clone() else {
        // `cwd` is optional in the schema, but starting a service in whatever
        // directory the app happens to inherit is not predictable (DEVELOPMENT
        // §16). Refuse and say what to add rather than guess.
        return Err(SessionError::failed(
            id,
            OPERATION,
            format!(
                "session `{id}` has no `cwd`; add one so the service starts in a known \
                 directory"
            ),
            None,
        ));
    };

    let (program, args) = split_command(command).map_err(|reason| {
        SessionError::failed(
            id,
            OPERATION,
            format!("session `{id}` has an unusable command: {reason}"),
            None,
        )
    })?;

    let output = if planned.captures_output() {
        OutputMode::Capture
    } else {
        OutputMode::Inherit
    };
    Ok(ProcessSpec {
        program,
        args,
        cwd,
        output,
    })
}

/// Split a configured command line into a program and its arguments.
///
/// The command is run directly rather than through `cmd.exe /C`. That keeps
/// the run's process id the service's own id — which is what the session
/// header shows and what the user will find in Task Manager — instead of a
/// short-lived shell's. Ownership and kill-tree behaviour do not depend on
/// this choice: the supervisor owns the run's job object either way.
///
/// Double quotes group a run of characters that would otherwise split, which
/// is what makes a path like `"C:\Program Files\node\node.exe"` usable. There
/// is no escape handling beyond that: a literal quote inside an argument is
/// not expressible yet, and is refused rather than silently mis-split.
fn split_command(command: &str) -> Result<(PathBuf, Vec<String>), String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut in_token = false;

    for character in command.chars() {
        match character {
            '"' => {
                quoted = !quoted;
                // An empty quoted argument (`""`) is still an argument.
                in_token = true;
            }
            c if c.is_whitespace() && !quoted => {
                if in_token {
                    tokens.push(std::mem::take(&mut current));
                    in_token = false;
                }
            }
            c => {
                current.push(c);
                in_token = true;
            }
        }
    }

    if quoted {
        return Err(format!("unbalanced quote in `{command}`"));
    }
    if in_token {
        tokens.push(current);
    }

    let mut tokens = tokens.into_iter();
    let Some(program) = tokens.next() else {
        return Err("the command is empty".to_owned());
    };
    Ok((PathBuf::from(program), tokens.collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EffectiveLogMode, EffectiveLogging, LogSource, SessionType};
    use std::path::PathBuf;

    /// A sink that keeps what it was told, so a test can assert on the event
    /// stream the same way the frontend would consume it.
    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<SessionEvent>>,
    }

    impl RecordingSink {
        fn names(&self) -> Vec<&'static str> {
            lock(&self.events).iter().map(SessionEvent::name).collect()
        }

        fn events(&self) -> Vec<SessionEvent> {
            lock(&self.events).clone()
        }
    }

    impl EventSink for RecordingSink {
        fn publish(&self, event: SessionEvent) {
            lock(&self.events).push(event);
        }
    }

    /// A command that stays alive until it is stopped, so a test can observe a
    /// session while it is genuinely `Running`. `ping` is the same long-running
    /// console child the T03 smoke test uses.
    const LONG_RUNNING: &str = "cmd.exe /c ping -n 120 127.0.0.1";

    fn service(id: &str) -> SessionConfig {
        SessionConfig {
            id: id.to_owned(),
            name: format!("Service {id}"),
            session_type: SessionType::Service,
            cwd: Some(test_cwd()),
            command: Some("cmd.exe /c exit 0".to_owned()),
            url: None,
            port: None,
            purpose: None,
            close_impact: None,
            shell: None,
            initial_command: None,
            logging: EffectiveLogging {
                mode: EffectiveLogMode::Always,
                source: LogSource::Captured,
                external_path: None,
            },
        }
    }

    fn test_cwd() -> PathBuf {
        std::env::current_dir().expect("the test process has a working directory")
    }

    fn core_with(id: &str) -> (SessionCore, Arc<RecordingSink>) {
        core_with_command(id, "cmd.exe /c exit 0")
    }

    fn core_with_command(id: &str, command: &str) -> (SessionCore, Arc<RecordingSink>) {
        let sink = Arc::new(RecordingSink::default());
        let core = SessionCore::new(sink.clone());
        let mut config = service(id);
        config.command = Some(command.to_owned());
        core.register(config).expect("registration succeeds");
        (core, sink)
    }

    /// The plan a run of `config` would get, resolved against no writable
    /// root — the shape every unit test here works with.
    fn planned_for(config: &SessionConfig) -> PlannedRun {
        PlannedRun {
            run_id: RunId::mint(),
            started_at: Timestamp::now(),
            plan: LogPlan::resolve(&config.logging, None),
        }
    }

    #[test]
    fn starting_a_service_reports_running_with_a_pid_and_run_id() {
        let (core, _sink) = core_with_command("svc", LONG_RUNNING);

        let runtime = core.start("svc").expect("start succeeds");
        assert_eq!(runtime.status, SessionStatus::Running);
        assert!(runtime.pid.is_some(), "a running session has a pid");
        assert!(runtime.run_id.is_some(), "a run has an id");
        assert!(runtime.started_at.is_some(), "a run has a start time");
        assert_eq!(runtime.exit_code, None, "it has not ended");
        assert_eq!(runtime.last_error, None);

        let summary = core.summary();
        assert_eq!(summary.total, 1);
        assert_eq!(summary.running, 1);
        assert_eq!(summary.error, 0);

        core.force_stop("svc").expect("cleanup");
    }

    /// The listener has to see `Starting` before `Running`: a UI that only
    /// ever heard `Running` could not show that a start was in flight.
    #[test]
    fn starting_publishes_the_states_it_passed_through() {
        let (core, sink) = core_with_command("svc", LONG_RUNNING);

        core.start("svc").expect("start succeeds");

        let statuses: Vec<SessionStatus> = sink
            .events()
            .iter()
            .filter_map(|event| match event {
                SessionEvent::StateChanged(inner) => Some(inner.runtime.status),
                _ => None,
            })
            .collect();
        assert_eq!(
            statuses,
            vec![SessionStatus::Starting, SessionStatus::Running]
        );
        assert_eq!(
            sink.names(),
            vec![
                "session-state-changed",
                "app-summary-changed",
                "session-state-changed",
                "app-summary-changed",
                "run-record-updated",
            ],
            "start should announce the run it created"
        );

        core.force_stop("svc").expect("cleanup");
    }

    #[test]
    fn the_run_record_carries_the_run_identity() {
        let (core, sink) = core_with_command("svc", LONG_RUNNING);

        let runtime = core.start("svc").expect("start succeeds");

        let record = sink
            .events()
            .into_iter()
            .find_map(|event| match event {
                SessionEvent::RunRecordUpdated(inner) => Some(inner.run),
                _ => None,
            })
            .expect("a run record was published");

        assert_eq!(record.session_id, "svc");
        assert_eq!(record.pid, runtime.pid);
        assert_eq!(Some(record.run_id), runtime.run_id);
        assert_eq!(record.exit_code, None, "the run is still open");
        assert_eq!(record.ended_at, None, "the run is still open");
        assert_eq!(record.log_source, LogSource::Captured);

        core.force_stop("svc").expect("cleanup");
    }

    /// Whether the OS still lists a process under this id. Used to check that
    /// a replaced run is really gone rather than merely forgotten by Session
    /// Core, which is the half of "restart cannot leave duplicates" that the
    /// in-process state cannot answer on its own.
    fn pid_is_alive(pid: u32) -> bool {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .expect("tasklist runs");
        String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
    }

    /// Spec §5 rule 2 and T04's acceptance criterion 4.
    #[test]
    fn restarting_replaces_the_run_and_leaves_exactly_one_alive() {
        let (core, _sink) = core_with_command("svc", LONG_RUNNING);
        let first = core.start("svc").expect("start succeeds");
        let first_pid = first.pid.expect("a running session has a pid");

        let second = core.restart("svc").expect("restart succeeds");

        assert_eq!(second.status, SessionStatus::Running);
        assert_ne!(second.run_id, first.run_id, "a restart is a new run");
        assert_ne!(second.pid, first.pid, "a restart is a new process");
        assert_eq!(core.summary().running, 1, "one session, one run");
        assert!(
            !pid_is_alive(first_pid),
            "the replaced run (pid {first_pid}) outlived the restart"
        );

        core.force_stop("svc").expect("cleanup");
    }

    /// A restart of something that is not running is just a start — that is
    /// what makes the button usable on a `Stopped` or failed session.
    #[test]
    fn restarting_a_session_that_is_not_running_starts_it() {
        let (core, _sink) = core_with_command("svc", LONG_RUNNING);

        let runtime = core.restart("svc").expect("restart starts it");
        assert_eq!(runtime.status, SessionStatus::Running);
        assert!(runtime.pid.is_some());

        core.force_stop("svc").expect("cleanup");
    }

    #[test]
    fn restarting_an_unknown_session_is_refused() {
        let (core, _sink) = core_with("svc");

        let error = core.restart("nope").expect_err("refused");
        assert_eq!(error.kind, SessionErrorKind::UnknownSession);
    }

    /// T04 acceptance criterion 5: session state does not depend on a window
    /// being visible.
    ///
    /// The layer has no window to consult — that is the point — so the test
    /// that pins it is a core with *nobody* listening, which is exactly what a
    /// hidden window is from Session Core's side. Lifecycle proceeds and the
    /// state converges regardless.
    #[test]
    fn lifecycle_runs_with_no_listener_at_all() {
        let core = SessionCore::without_listener();
        core.register(service("svc"))
            .expect("registration succeeds");

        let started = core.start("svc").expect("start succeeds");
        assert_eq!(started.status, SessionStatus::Running);

        let stopped = core.force_stop("svc").expect("force stop succeeds");
        assert_eq!(stopped.status, SessionStatus::Exited);
        assert_eq!(core.summary().running, 0);
    }

    /// And the same for the ending nobody asked for: with no listener, an
    /// unexpected exit still has to land in the state.
    #[test]
    fn an_unexpected_exit_lands_with_no_listener_at_all() {
        let core = SessionCore::without_listener();
        let mut config = service("svc");
        config.command = Some("cmd.exe /c ping -n 2 127.0.0.1".to_owned());
        core.register(config).expect("registration succeeds");
        core.start("svc").expect("start succeeds");

        wait_for_status(
            &core,
            "svc",
            SessionStatus::Exited,
            std::time::Duration::from_secs(30),
        );
        assert_eq!(core.summary().running, 0);
    }

    /// Spec §5 rule 1: a start while already running is refused.
    #[test]
    fn starting_a_session_that_is_already_running_is_refused() {
        let (core, _sink) = core_with_command("svc", LONG_RUNNING);
        core.start("svc").expect("first start succeeds");

        let error = core.start("svc").expect_err("second start is refused");
        assert_eq!(error.kind, SessionErrorKind::InvalidTransition);
        assert_eq!(error.from, Some(SessionStatus::Running));
        assert!(
            error.message.contains("running"),
            "message names the state: {error}"
        );

        core.force_stop("svc").expect("cleanup");
    }

    #[test]
    fn starting_an_unknown_session_is_refused() {
        let (core, _sink) = core_with("svc");

        let error = core.start("nope").expect_err("refused");
        assert_eq!(error.kind, SessionErrorKind::UnknownSession);
        assert!(error.message.contains("nope"), "message: {error}");
    }

    /// A program that cannot be launched leaves the session in `Error` with the
    /// reason — not silently back in `Stopped`, which would hide the failure.
    #[test]
    fn a_start_that_cannot_spawn_ends_in_error_with_the_reason() {
        let (core, sink) = core_with_command("svc", "definitely-not-a-real-program-lch --serve");

        let error = core.start("svc").expect_err("the spawn fails");
        assert_eq!(error.kind, SessionErrorKind::Failed);

        let runtime = core.snapshot("svc").expect("registered");
        assert_eq!(runtime.status, SessionStatus::Error);
        assert_eq!(runtime.pid, None);
        let recorded = runtime.last_error.expect("the reason is recorded");
        assert_eq!(recorded.operation, "start");
        assert!(
            recorded
                .message
                .contains("definitely-not-a-real-program-lch"),
            "message names the program: {}",
            recorded.message
        );

        assert!(
            sink.names().contains(&"session-state-changed"),
            "the failure is announced: {:?}",
            sink.names()
        );
        assert_eq!(core.summary().error, 1);
    }

    /// Poll until the session reaches `want`, so a test asserts on the state
    /// the watcher produced rather than on how long it took to produce it.
    fn wait_for_status(
        core: &SessionCore,
        session_id: &str,
        want: SessionStatus,
        timeout: std::time::Duration,
    ) -> SessionRuntime {
        let deadline = std::time::Instant::now() + timeout;
        loop {
            let runtime = core.snapshot(session_id).expect("session is registered");
            if runtime.status == want {
                return runtime;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "session `{session_id}` never reached {} within {timeout:?}; stuck at {}",
                want.as_str(),
                runtime.status.as_str()
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// Spec §5 rule 4, and T04's acceptance criterion: a run that ends on its
    /// own is reflected in session state, with no caller asking and no UI
    /// involved.
    #[test]
    fn a_run_that_ends_on_its_own_updates_the_session_without_being_asked() {
        let (core, sink) = core_with_command("svc", "cmd.exe /c ping -n 2 127.0.0.1");
        core.start("svc").expect("start succeeds");

        let runtime = wait_for_status(
            &core,
            "svc",
            SessionStatus::Exited,
            std::time::Duration::from_secs(30),
        );

        assert_eq!(runtime.pid, None, "nothing is running any more");
        assert_eq!(
            runtime.exit_code,
            Some(0),
            "the run's own exit code is what gets recorded"
        );
        assert_eq!(
            runtime.last_error, None,
            "ending on its own is not an error"
        );
        assert_eq!(core.summary().running, 0);

        let record = sink
            .events()
            .into_iter()
            .rev()
            .find_map(|event| match event {
                SessionEvent::RunRecordUpdated(inner) => Some(inner.run),
                _ => None,
            })
            .expect("the ending was published");
        assert_eq!(record.exit_code, Some(0));
        assert!(record.ended_at.is_some(), "the run record is closed");
    }

    /// Spec §5 lists `Running -> Error` as well as `Running -> Exited`, so the
    /// two must not be the same outcome. A service that falls over on its own
    /// has failed; reporting it as a clean `Exited` would leave
    /// `AppSummary.error` at zero and give the tray's "Restart Failed" nothing
    /// to key on (spec §11).
    #[test]
    fn a_run_that_fails_on_its_own_ends_in_error_not_exited() {
        let (core, _sink) = core_with_command("svc", "cmd.exe /c exit 3");
        core.start("svc").expect("start succeeds");

        let runtime = wait_for_status(
            &core,
            "svc",
            SessionStatus::Error,
            std::time::Duration::from_secs(30),
        );

        assert_eq!(runtime.exit_code, Some(3), "the failing code is kept");
        assert_eq!(runtime.pid, None);
        assert!(
            runtime.last_error.is_some(),
            "a failure has to carry a reason the UI can show"
        );
        assert_eq!(core.summary().error, 1);
        assert_eq!(core.summary().running, 0);
    }

    /// A run that was stopped on purpose has already been accounted for. The
    /// watcher waking up afterwards must not publish a second ending, which
    /// would show a session stopping twice in the UI.
    #[test]
    fn a_stopped_run_is_not_reported_again_by_its_watcher() {
        let (core, sink) = core_with_command("svc", LONG_RUNNING);
        core.start("svc").expect("start succeeds");
        let stopped = core.force_stop("svc").expect("force stop succeeds");
        assert_eq!(stopped.status, SessionStatus::Exited);

        let settled = sink.events().len();
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert_eq!(
            sink.events().len(),
            settled,
            "the watcher published after the stop: {:?}",
            sink.names()
        );
        assert_eq!(
            core.snapshot("svc").expect("registered").status,
            SessionStatus::Exited
        );
    }

    /// Spec §5 rule 5 keeps the exit context in the run record, and §9 has a
    /// `run-record-updated` event to carry it. A stop the user asked for is
    /// still a run ending, so it has to be announced: the watcher refuses to
    /// report an ending a stop already owns, which would otherwise leave the
    /// closing of the record unpublished for every stop and force-stop.
    #[test]
    fn stopping_publishes_the_closed_run_record() {
        let (core, sink) = core_with_command("svc", LONG_RUNNING);
        core.start("svc").expect("start succeeds");

        core.force_stop("svc").expect("force stop succeeds");

        let closed = sink
            .events()
            .into_iter()
            .filter_map(|event| match event {
                SessionEvent::RunRecordUpdated(inner) => Some(inner.run),
                _ => None,
            })
            .next_back()
            .expect("the ending was announced");

        assert!(closed.ended_at.is_some(), "the run record is closed");
        assert!(closed.exit_code.is_some(), "the exit code is recorded");
    }

    #[test]
    fn stopping_a_running_service_ends_it_and_records_the_exit_code() {
        let (core, _sink) = core_with_command("svc", LONG_RUNNING);
        core.start("svc").expect("start succeeds");

        let runtime = core
            .stop_with_timeout("svc", std::time::Duration::from_secs(2))
            .expect("stop succeeds");

        assert!(
            matches!(
                runtime.status,
                SessionStatus::Stopped | SessionStatus::Exited
            ),
            "a stopped run ends Stopped or Exited, saw {}",
            runtime.status.as_str()
        );
        assert_eq!(runtime.pid, None, "nothing is running any more");
        assert!(
            runtime.exit_code.is_some(),
            "the termination result is observable"
        );
        assert_eq!(core.summary().running, 0);
    }

    #[test]
    fn force_stop_ends_the_run_without_waiting_for_it() {
        let (core, _sink) = core_with_command("svc", LONG_RUNNING);
        core.start("svc").expect("start succeeds");

        let runtime = core.force_stop("svc").expect("force stop succeeds");

        assert_eq!(runtime.status, SessionStatus::Exited);
        assert_eq!(runtime.pid, None);
        assert!(runtime.exit_code.is_some());
        assert_eq!(core.summary().running, 0);
    }

    /// Restarting a stopped session has to work, and stopping one that is not
    /// running has to be a clear refusal rather than a silent success.
    #[test]
    fn stopping_a_session_that_is_not_running_is_refused() {
        let (core, _sink) = core_with("svc");

        let error = core.stop("svc").expect_err("refused");
        assert_eq!(error.kind, SessionErrorKind::InvalidTransition);
        assert_eq!(error.from, Some(SessionStatus::Stopped));
        assert!(error.message.contains("stopped"), "message: {error}");

        let forced = core.force_stop("svc").expect_err("refused");
        assert_eq!(forced.kind, SessionErrorKind::InvalidTransition);
    }

    /// A refused operation must not have moved the session.
    #[test]
    fn a_refused_start_leaves_the_session_untouched() {
        let (core, sink) = core_with("svc");

        let error = core
            .stop("svc")
            .expect_err("stopping a stopped session is refused");
        assert_eq!(error.kind, SessionErrorKind::InvalidTransition);

        let runtime = core.snapshot("svc").expect("registered");
        assert_eq!(runtime.status, SessionStatus::Stopped);
        assert_eq!(runtime.pid, None);
        assert_eq!(runtime.run_id, None);
        assert!(
            sink.names().is_empty(),
            "a refusal publishes nothing, saw {:?}",
            sink.names()
        );
    }

    #[test]
    fn a_registered_session_is_stopped_with_its_configured_logging() {
        let (core, _sink) = core_with("svc");

        let runtime = core.snapshot("svc").expect("session is registered");
        assert_eq!(runtime.status, SessionStatus::Stopped);
        assert_eq!(runtime.pid, None, "no run exists yet");
        assert_eq!(runtime.run_id, None, "no run exists yet");
        assert_eq!(runtime.logging.mode, EffectiveLogMode::Always);
        assert_eq!(runtime.logging.source, LogSource::Captured);
    }

    #[test]
    fn an_unknown_session_has_no_snapshot() {
        let (core, _sink) = core_with("svc");
        assert!(core.snapshot("nope").is_none());
    }

    #[test]
    fn registering_the_same_id_twice_is_refused() {
        let (core, _sink) = core_with("svc");

        let error = core
            .register(service("svc"))
            .expect_err("duplicate is refused");
        assert_eq!(error.kind, SessionErrorKind::AlreadyRegistered);
        assert_eq!(error.session_id, "svc");
        assert!(error.message.contains("svc"), "message names it: {error}");
    }

    #[test]
    fn the_summary_counts_registered_sessions() {
        let sink = Arc::new(RecordingSink::default());
        let core = SessionCore::new(sink);
        core.register(service("a")).expect("registration succeeds");
        core.register(service("b")).expect("registration succeeds");

        let summary = core.summary();
        assert_eq!(summary.total, 2);
        assert_eq!(summary.running, 0, "nothing has been started");
        assert_eq!(summary.error, 0);
    }

    #[test]
    fn registration_does_not_publish_a_lifecycle_event() {
        // Registering is not a lifecycle transition; a listener should not see
        // a state change for a session that has not moved.
        let (core, sink) = core_with("svc");
        assert!(sink.names().is_empty(), "saw {:?}", sink.names());

        let _ = core.snapshot("svc");
        assert!(sink.names().is_empty());
    }

    /// The command string is user-authored, so the cases that matter are the
    /// ones where splitting could quietly produce the wrong program or the
    /// wrong argument boundary.
    #[test]
    fn a_command_splits_into_a_program_and_arguments() {
        let (program, args) = split_command("node server.js --port 8000").expect("splits");
        assert_eq!(program, PathBuf::from("node"));
        assert_eq!(args, vec!["server.js", "--port", "8000"]);
    }

    #[test]
    fn a_quoted_program_keeps_its_spaces_and_extra_whitespace_is_ignored() {
        let (program, args) =
            split_command("  \"C:\\Program Files\\node\\node.exe\"   server.js  ").expect("splits");
        assert_eq!(program, PathBuf::from("C:\\Program Files\\node\\node.exe"));
        assert_eq!(args, vec!["server.js"]);
    }

    /// A quoted argument keeps its inner spacing without becoming two
    /// arguments, which is the whole reason quotes are honoured.
    #[test]
    fn a_quoted_argument_stays_one_argument() {
        let (program, args) = split_command("app.exe --title \"My Service\"").expect("splits");
        assert_eq!(program, PathBuf::from("app.exe"));
        assert_eq!(args, vec!["--title", "My Service"]);
    }

    #[test]
    fn an_empty_quoted_argument_is_still_an_argument() {
        let (program, args) = split_command("app.exe \"\"").expect("splits");
        assert_eq!(program, PathBuf::from("app.exe"));
        assert_eq!(args, vec![""]);
    }

    /// Refused rather than guessed at: a mis-split command would start the
    /// wrong program, and the user would have no idea why.
    #[test]
    fn an_unbalanced_quote_is_refused() {
        let error = split_command("app.exe \"unterminated").expect_err("refused");
        assert!(error.contains("quote"), "message: {error}");
    }

    #[test]
    fn an_empty_command_is_refused() {
        assert!(split_command("   ").is_err());
        assert!(split_command("").is_err());
    }

    #[test]
    fn a_service_session_maps_to_a_supervised_process() {
        let mut config = service("svc");
        config.cwd = Some(PathBuf::from("C:\\work"));

        let spec =
            process_spec(&config, "svc", &planned_for(&config)).expect("a service has a spec");
        assert_eq!(spec.program, PathBuf::from("cmd.exe"));
        assert_eq!(spec.args, vec!["/c", "exit", "0"]);
        assert_eq!(spec.cwd, PathBuf::from("C:\\work"));
    }

    /// `cwd` is optional in the schema but cannot be defaulted predictably, so
    /// the refusal has to name what is missing.
    #[test]
    fn a_service_without_a_working_directory_is_refused_with_a_reason() {
        let mut config = service("svc");
        config.cwd = None;

        let error = process_spec(&config, "svc", &planned_for(&config)).expect_err("refused");
        assert_eq!(error.kind, SessionErrorKind::Failed);
        assert!(error.message.contains("cwd"), "message: {}", error.message);
    }

    /// The seam between a configured session and the run it gets: a service is
    /// supervised (T03), an interactive terminal is hosted on a PTY (T02,
    /// wired here by T07 #8). The dispatch is by session type, so neither
    /// builder can be asked to host something it does not own.
    #[test]
    fn a_terminal_is_hosted_on_a_terminal_and_a_service_on_a_supervised_process() {
        let mut terminal = service("term");
        terminal.session_type = SessionType::Terminal;
        terminal.command = None;
        terminal.shell = Some("powershell".to_owned());
        terminal.cwd = Some(PathBuf::from("C:\\work"));

        match start_spec(&terminal, None, "term", &planned_for(&terminal)).expect("a terminal spec")
        {
            StartSpec::Terminal(spec) => {
                assert_eq!(spec.program, PathBuf::from("powershell"));
                assert_eq!(spec.cwd, PathBuf::from("C:\\work"));
                assert_eq!(
                    (spec.cols, spec.rows),
                    (DEFAULT_COLS, DEFAULT_ROWS),
                    "a view that has not measured itself gets the PTY default"
                );
            }
            StartSpec::Process(_) => panic!("a terminal must not run as a supervised process"),
        }

        let service = service("svc");
        match start_spec(&service, None, "svc", &planned_for(&service)).expect("a service spec") {
            StartSpec::Process(spec) => assert_eq!(spec.program, PathBuf::from("cmd.exe")),
            StartSpec::Terminal(_) => panic!("a service must not be hosted on a terminal"),
        }
    }

    /// A view measures itself before its session starts, and that measurement
    /// is what the shell is born with (T07): a shell started at the default and
    /// corrected a moment later draws its first screen at the wrong width.
    #[test]
    fn a_terminal_is_hosted_at_the_geometry_its_view_asked_for() {
        let mut terminal = service("term");
        terminal.session_type = SessionType::Terminal;
        terminal.command = None;
        terminal.shell = Some("powershell".to_owned());
        terminal.cwd = Some(PathBuf::from("C:\\work"));

        match start_spec(&terminal, Some((120, 30)), "term", &planned_for(&terminal))
            .expect("a terminal spec")
        {
            StartSpec::Terminal(spec) => assert_eq!((spec.cols, spec.rows), (120, 30)),
            StartSpec::Process(_) => panic!("a terminal must not run as a supervised process"),
        }
    }

    /// A terminal's `shell` is required for the same reason a service's
    /// `command` is: the refusal names what is missing rather than hosting
    /// something arbitrary.
    #[test]
    fn a_terminal_without_a_shell_is_refused_with_a_reason() {
        let mut config = service("term");
        config.session_type = SessionType::Terminal;
        config.command = None;
        config.shell = None;

        let error = start_spec(&config, None, "term", &planned_for(&config)).expect_err("refused");
        assert_eq!(error.kind, SessionErrorKind::Failed);
        assert_eq!(error.session_id, "term");
        assert!(
            error.message.contains("shell"),
            "message: {}",
            error.message
        );
    }

    /// The pipe decision is made from the resolved policy, not from the
    /// session type: a `captured` service has its output taken (its scrollback
    /// is filled even when nothing is written), and one linked to the
    /// application's own log does not (D-005).
    #[test]
    fn a_run_is_piped_only_when_its_policy_captures() {
        let captured = service("svc");
        let mut external = service("app");
        external.logging = EffectiveLogging {
            mode: EffectiveLogMode::Always,
            source: LogSource::External,
            external_path: Some("D:/app/access.log".to_owned()),
        };

        let piped = process_spec(&captured, "svc", &planned_for(&captured)).expect("a spec");
        let linked = process_spec(&external, "app", &planned_for(&external)).expect("a spec");

        assert_eq!(piped.output, OutputMode::Capture);
        assert_eq!(linked.output, OutputMode::Inherit);
    }

    // ---- T05 (#6): logging ------------------------------------------------

    mod logging {
        use super::*;
        use crate::config::{EffectiveLogMode, EffectiveLogging, LogSource};
        use crate::logging::test_support::TempDir;
        use crate::logging::LogRoots;
        use std::fs;
        use std::path::Path as FsPath;

        /// A core that writes under a scratch app-data root, with one session
        /// registered.
        fn core_logging_to(
            dir: &TempDir,
            config: SessionConfig,
        ) -> (SessionCore, Arc<RecordingSink>) {
            let sink = Arc::new(RecordingSink::default());
            let core = SessionCore::new(sink.clone()).with_log_roots(LogRoots {
                logs_dir: dir.join("logs"),
                metadata_dir: dir.join("metadata"),
            });
            core.register(config).expect("registration succeeds");
            (core, sink)
        }

        fn captured(mode: EffectiveLogMode) -> EffectiveLogging {
            EffectiveLogging {
                mode,
                source: LogSource::Captured,
                external_path: None,
            }
        }

        fn service_printing(id: &str, command: &str, logging: EffectiveLogging) -> SessionConfig {
            let mut config = service(id);
            config.command = Some(command.to_owned());
            config.logging = logging;
            config
        }

        /// Poll `check` until it holds, so a test asserts on what the pumping
        /// and watching threads produced rather than on how long they took.
        fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            while !check() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for {what}"
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }

        fn read(path: &FsPath) -> String {
            fs::read_to_string(path).unwrap_or_default()
        }

        /// The run's log file, named the moment the run starts.
        fn announced_log(core: &SessionCore, session_id: &str) -> PathBuf {
            let status = core.log_status(session_id).expect("the session exists");
            PathBuf::from(
                status
                    .log_file
                    .expect("the run's file is named while it runs"),
            )
        }

        /// T05's central criterion for a service: `captured` + `always` puts
        /// the run's output in a run-specific file, and the run history is how
        /// that file is found again (`docs/LOGGING.md` §6).
        #[test]
        fn a_captured_run_writes_a_file_its_run_record_points_at() {
            let dir = TempDir::new();
            let config = service_printing(
                "svc",
                "cmd.exe /c echo listening-on-8188",
                captured(EffectiveLogMode::Always),
            );
            let (core, _sink) = core_logging_to(&dir, config);

            let runtime = core.start("svc").expect("start succeeds");
            let file = announced_log(&core, "svc");
            assert!(file.exists(), "the run file is created when the run starts");

            wait_for_status(
                &core,
                "svc",
                SessionStatus::Exited,
                std::time::Duration::from_secs(30),
            );
            wait_until("the run's output to reach its file", || {
                read(&file).contains("listening-on-8188")
            });

            let history = core.run_history("svc");
            let run = history.latest().expect("the run is in the history");
            assert_eq!(Some(run.run_id.clone()), runtime.run_id);
            assert_eq!(
                run.log_file.as_deref(),
                Some(file.to_string_lossy().as_ref()),
                "the history must name the file the run wrote"
            );
            assert!(
                PathBuf::from(run.log_file.clone().expect("a log file")).exists(),
                "the file the record names does not exist"
            );
            assert_eq!(run.exit_code, Some(0));
            assert!(run.ended_at.is_some(), "the run is closed");
        }

        /// Both streams are captured, and the file says which one each part
        /// came from (`docs/LOGGING.md` §7).
        #[test]
        fn a_run_file_carries_both_streams_tagged() {
            let dir = TempDir::new();
            let config = service_printing(
                "svc",
                "cmd.exe /c echo to-stdout & echo to-stderr 1>&2",
                captured(EffectiveLogMode::Always),
            );
            let (core, _sink) = core_logging_to(&dir, config);

            core.start("svc").expect("start succeeds");
            let file = announced_log(&core, "svc");
            wait_for_status(
                &core,
                "svc",
                SessionStatus::Exited,
                std::time::Duration::from_secs(30),
            );
            wait_until("both streams to reach the file", || {
                let text = read(&file);
                text.contains("to-stdout") && text.contains("to-stderr")
            });

            let text = read(&file);
            assert!(
                text.contains("to-stdout") && text.contains("stdout"),
                "stdout is untagged: {text}"
            );
            assert!(
                text.contains("to-stderr") && text.contains("stderr"),
                "stderr is untagged: {text}"
            );
        }

        /// §8 and D-004 together: `off` writes nothing anywhere, and the
        /// session still has its bounded scrollback.
        #[test]
        fn an_off_run_leaves_no_file_and_still_has_a_scrollback() {
            let dir = TempDir::new();
            let config = service_printing(
                "term",
                "cmd.exe /c echo interactive-output",
                EffectiveLogging {
                    mode: EffectiveLogMode::Off,
                    source: LogSource::None,
                    external_path: None,
                },
            );
            let (core, _sink) = core_logging_to(&dir, config);

            core.start("term").expect("start succeeds");
            wait_for_status(
                &core,
                "term",
                SessionStatus::Exited,
                std::time::Duration::from_secs(30),
            );
            wait_until("the scrollback to fill", || {
                core.snapshot("term")
                    .expect("the session exists")
                    .buffer
                    .bytes
                    > 0
            });

            assert!(
                !dir.join("logs").exists(),
                "an `off` session created a log directory"
            );
            let status = core.log_status("term").expect("status");
            assert_eq!(status.state, crate::logging::LogState::Off);
            assert!(status.log_file.is_none());
            assert!(
                !status.records_input,
                "user input is never recorded by default"
            );

            let chunks = core.terminal_buffer("term").expect("a buffer");
            let text: String = chunks.iter().map(|chunk| chunk.text()).collect();
            assert!(
                text.contains("interactive-output"),
                "the scrollback lost the output: {text}"
            );
            // The record is filed just after the state changes, so the history
            // is polled rather than assumed to be there the moment the session
            // reads `Exited`.
            wait_until("the run to be filed", || {
                !core.run_history("term").runs.is_empty()
            });
            assert!(
                core.run_history("term")
                    .latest()
                    .expect("the run is filed")
                    .log_file
                    .is_none(),
                "an `off` run's record must name no file"
            );
        }

        /// §13 scenario C: a clean run leaves nothing, a failed run keeps the
        /// context from before the failure.
        #[test]
        fn on_error_keeps_the_context_of_a_failing_run_only() {
            let dir = TempDir::new();
            let clean = service_printing(
                "clean",
                "cmd.exe /c echo all-good",
                captured(EffectiveLogMode::OnError),
            );
            let (core, _sink) = core_logging_to(&dir, clean);
            core.start("clean").expect("start succeeds");
            wait_for_status(
                &core,
                "clean",
                SessionStatus::Exited,
                std::time::Duration::from_secs(30),
            );
            wait_until("the clean run to be filed", || {
                !core.run_history("clean").runs.is_empty()
            });
            assert!(
                core.run_history("clean")
                    .latest()
                    .expect("a run record")
                    .log_file
                    .is_none(),
                "a clean `on_error` run must not name a file"
            );

            let failing = service_printing(
                "failing",
                "cmd.exe /c echo before-the-crash & exit 3",
                captured(EffectiveLogMode::OnError),
            );
            let (core, _sink) = core_logging_to(&dir, failing);
            core.start("failing").expect("start succeeds");
            wait_for_status(
                &core,
                "failing",
                SessionStatus::Error,
                std::time::Duration::from_secs(30),
            );
            wait_until("the failure's log to appear", || {
                core.run_history("failing")
                    .latest()
                    .and_then(|run| run.log_file.clone())
                    .map(|path| read(FsPath::new(&path)).contains("before-the-crash"))
                    .unwrap_or(false)
            });

            let history = core.run_history("failing");
            let run = history.latest().expect("a run");
            assert_eq!(run.exit_code, Some(3));
            let text = read(FsPath::new(run.log_file.as_deref().expect("a log file")));
            assert!(
                text.contains("exit code 3"),
                "the log does not say how the run ended: {text}"
            );
        }

        /// D-005: an application-owned log is linked, not duplicated. The Hub
        /// must not capture the output and must not write a file of its own,
        /// while the run record still points a user at the application's file.
        #[test]
        fn an_external_session_links_the_application_log_and_captures_nothing() {
            let dir = TempDir::new();
            let external = dir.join("app/access.log").display().to_string();
            let config = service_printing(
                "app",
                "cmd.exe /c echo output-the-app-logs-itself",
                EffectiveLogging {
                    mode: EffectiveLogMode::Always,
                    source: LogSource::External,
                    external_path: Some(external.clone()),
                },
            );
            let (core, _sink) = core_logging_to(&dir, config);

            core.start("app").expect("start succeeds");
            wait_for_status(
                &core,
                "app",
                SessionStatus::Exited,
                std::time::Duration::from_secs(30),
            );
            wait_until("the run to be filed", || {
                !core.run_history("app").runs.is_empty()
            });

            assert!(
                !dir.join("logs").exists(),
                "an external session must not write a Hub log"
            );
            let status = core.log_status("app").expect("status");
            assert_eq!(status.state, crate::logging::LogState::External);
            assert_eq!(status.external_log.as_deref(), Some(external.as_str()));
            assert_eq!(status.log_file.as_deref(), Some(external.as_str()));
            assert_eq!(
                core.run_history("app")
                    .latest()
                    .expect("a run")
                    .log_file
                    .as_deref(),
                Some(external.as_str())
            );
        }

        /// A session that has never run still answers "am I being recorded?" —
        /// including the answer "I cannot be, and here is why".
        #[test]
        fn a_session_with_nowhere_to_write_says_why_it_is_not_logging() {
            let (core, _sink) = core_with_command("svc", "cmd.exe /c exit 0");
            core.start("svc").expect("start succeeds");

            let status = core.log_status("svc").expect("status");

            assert_eq!(status.mode, EffectiveLogMode::Always);
            assert_eq!(status.source, LogSource::Captured);
            assert!(status.log_file.is_none(), "nothing can be written");
            let problem = status.last_error.expect("the reason is reported");
            assert!(
                problem.message.contains("writable log directory"),
                "unhelpful message: {}",
                problem.message
            );
            core.force_stop("svc").expect("cleanup");
        }

        /// A terminal session's default policy is `off`/`none` (T01's `auto`
        /// resolution), and its status has to say so before it ever runs —
        /// T07 attaches the PTY, and the UI shows the policy meanwhile.
        #[test]
        fn a_terminal_session_reports_its_policy_before_it_has_ever_run() {
            let dir = TempDir::new();
            let mut config = service("shell");
            config.session_type = SessionType::Terminal;
            config.command = None;
            config.shell = Some("powershell".to_owned());
            config.logging = EffectiveLogging {
                mode: EffectiveLogMode::Off,
                source: LogSource::None,
                external_path: None,
            };
            let (core, _sink) = core_logging_to(&dir, config);

            let status = core.log_status("shell").expect("status");

            assert_eq!(status.state, crate::logging::LogState::Off);
            assert!(status.log_file.is_none());
            assert!(!status.records_input);
            assert!(
                status.session_log_dir.is_some(),
                "the folder a run would write into is known before any run"
            );
        }

        /// §3's "save this run's log": an `on_error` run can be committed while
        /// it is still going, and it keeps its file even if it later exits
        /// cleanly — the user asked for it.
        #[test]
        fn saving_a_running_on_error_log_commits_it() {
            let dir = TempDir::new();
            let config = service_printing(
                "svc",
                "cmd.exe /c ping -n 120 127.0.0.1",
                captured(EffectiveLogMode::OnError),
            );
            let (core, _sink) = core_logging_to(&dir, config);
            core.start("svc").expect("start succeeds");

            let status = core.save_run_log("svc").expect("the save succeeds");

            let file = PathBuf::from(status.log_file.expect("a file was committed"));
            assert!(file.exists(), "the saved log does not exist");
            core.force_stop("svc").expect("cleanup");
            assert!(file.exists(), "a forced stop deleted the saved log");
        }

        /// §3's `manual` mode: nothing is recorded until the user asks, and
        /// switching recording on is refused for a mode that does not have the
        /// question.
        #[test]
        fn manual_recording_is_switched_on_and_off_by_request_only() {
            let dir = TempDir::new();
            let manual = service_printing(
                "shell",
                "cmd.exe /c ping -n 120 127.0.0.1",
                captured(EffectiveLogMode::Manual),
            );
            let (core, _sink) = core_logging_to(&dir, manual);
            core.start("shell").expect("start succeeds");

            let idle = core.log_status("shell").expect("status");
            assert_eq!(
                idle.state,
                crate::logging::LogState::Off,
                "manual records nothing until asked"
            );

            let recording = core
                .set_log_recording("shell", true)
                .expect("recording starts");
            assert_eq!(recording.state, crate::logging::LogState::Capturing);
            let file = PathBuf::from(recording.log_file.expect("a file is open"));
            assert!(file.exists());

            let stopped = core
                .set_log_recording("shell", false)
                .expect("recording stops");
            assert_eq!(stopped.state, crate::logging::LogState::Off);

            core.force_stop("shell").expect("cleanup");

            let always = service_printing(
                "svc",
                "cmd.exe /c ping -n 120 127.0.0.1",
                captured(EffectiveLogMode::Always),
            );
            let (core, _sink) = core_logging_to(&dir, always);
            core.start("svc").expect("start succeeds");
            let refused = core
                .set_log_recording("svc", true)
                .expect_err("only manual sessions can be switched");
            assert!(
                refused.message.contains("manual"),
                "unhelpful message: {}",
                refused.message
            );
            core.force_stop("svc").expect("cleanup");
        }

        /// The scrollback is bounded per session, not per run: restarting does
        /// not throw away what the user was reading (§8), and the snapshot
        /// reports how much of it there is without carrying it.
        #[test]
        fn the_scrollback_survives_a_restart_and_stays_bounded() {
            let dir = TempDir::new();
            // Longer than the 64-byte scrollback below, so the bound is
            // reached on the first run.
            let noise = "0123456789".repeat(20);
            let config = service_printing(
                "svc",
                &format!("cmd.exe /c echo {noise}"),
                captured(EffectiveLogMode::Off),
            );
            let sink = Arc::new(RecordingSink::default());
            let core = SessionCore::new(sink)
                .with_log_roots(LogRoots {
                    logs_dir: dir.join("logs"),
                    metadata_dir: dir.join("metadata"),
                })
                .with_limits(
                    crate::logging::BufferLimits::new(64, usize::MAX),
                    crate::logging::DEFAULT_LOG_LIMITS,
                );
            core.register(config).expect("registration succeeds");

            core.start("svc").expect("start succeeds");
            wait_until("the first run's output", || {
                core.snapshot("svc").expect("the session").buffer.bytes > 0
            });
            let after_first = core.snapshot("svc").expect("the session").buffer;

            assert!(
                after_first.bytes <= 64,
                "the scrollback is not bounded: {} bytes",
                after_first.bytes
            );
            assert!(
                after_first.dropped_bytes > 0,
                "output was silently dropped without being counted"
            );

            // A second run of the same session: the buffer belongs to the
            // session, so what it already held is still there and its counters
            // carry on rather than starting again.
            core.restart("svc").expect("restart succeeds");
            wait_until("the second run's output", || {
                core.snapshot("svc")
                    .expect("the session")
                    .buffer
                    .dropped_bytes
                    > after_first.dropped_bytes
            });

            let after_second = core.snapshot("svc").expect("the session").buffer;
            assert!(
                after_second.bytes <= 64,
                "the scrollback is not bounded after a restart: {} bytes",
                after_second.bytes
            );
        }

        /// §9's cleanup, through the entry point that owns it: a sweep never
        /// takes a file that is still inside the retention window, and never
        /// reaches outside the Hub's own log root.
        #[test]
        fn a_cleanup_leaves_logs_inside_the_retention_window_alone() {
            let dir = TempDir::new();
            let config = service_printing(
                "svc",
                "cmd.exe /c echo keep-me",
                captured(EffectiveLogMode::Always),
            );
            let (core, _sink) = core_logging_to(&dir, config);
            core.start("svc").expect("start succeeds");
            let file = announced_log(&core, "svc");
            wait_for_status(
                &core,
                "svc",
                SessionStatus::Exited,
                std::time::Duration::from_secs(30),
            );

            let report = core.cleanup_logs(Some("svc"));

            assert_eq!(report.removed_count(), 0, "{:?}", report.removed);
            assert!(file.exists(), "a fresh run's log was deleted");
        }

        /// A `manual` session the user recorded and then stopped still has a
        /// log, and the run record has to name it — that is what makes the file
        /// findable afterwards (`docs/LOGGING.md` §6).
        #[test]
        fn a_stopped_manual_recording_still_names_its_file() {
            let dir = TempDir::new();
            let manual = service_printing(
                "shell",
                "cmd.exe /c ping -n 120 127.0.0.1",
                captured(EffectiveLogMode::Manual),
            );
            let (core, _sink) = core_logging_to(&dir, manual);
            core.start("shell").expect("start succeeds");
            let status = core
                .set_log_recording("shell", true)
                .expect("recording starts");
            let file = PathBuf::from(status.log_file.expect("a file is open"));

            core.set_log_recording("shell", false)
                .expect("recording stops");
            core.force_stop("shell").expect("cleanup");

            wait_until("the run to be filed", || {
                !core.run_history("shell").runs.is_empty()
            });
            assert_eq!(
                core.run_history("shell")
                    .latest()
                    .expect("a run")
                    .log_file
                    .as_deref(),
                Some(file.to_string_lossy().as_ref()),
                "the record lost the file the user recorded"
            );
            assert!(file.exists());
        }

        /// Stopping an `on_error` session on purpose is not a failure. The
        /// terminated process reports a non-zero code, and a log written for
        /// every press of Stop is exactly the litter `docs/LOGGING.md` §1.2 is
        /// about.
        #[test]
        fn stopping_an_on_error_run_on_purpose_writes_no_log() {
            let dir = TempDir::new();
            let config = service_printing(
                "svc",
                "cmd.exe /c ping -n 120 127.0.0.1",
                captured(EffectiveLogMode::OnError),
            );
            let (core, _sink) = core_logging_to(&dir, config);
            core.start("svc").expect("start succeeds");
            wait_for_status(
                &core,
                "svc",
                SessionStatus::Running,
                std::time::Duration::from_secs(30),
            );

            core.force_stop("svc").expect("cleanup");

            let history = core.run_history("svc");
            assert_eq!(
                history.latest().expect("a run").log_file,
                None,
                "a requested stop filed an error log"
            );
            assert!(
                !dir.join("logs").exists(),
                "a requested stop wrote a log directory"
            );
        }

        /// A record names a log file only when one exists (spec §4). A run
        /// whose log directory cannot be written is still filed — the run
        /// happened — but it must not point at a path that does not exist.
        #[test]
        fn a_run_whose_log_cannot_be_written_names_no_file() {
            let dir = TempDir::new();
            // A file where the `logs` directory would have to be.
            fs::write(dir.join("logs"), b"not a directory").expect("the blocker is writable");
            let config = service_printing(
                "svc",
                "cmd.exe /c echo nowhere-to-write",
                captured(EffectiveLogMode::Always),
            );
            let (core, _sink) = core_logging_to(&dir, config);

            core.start("svc").expect("start succeeds");
            wait_for_status(
                &core,
                "svc",
                SessionStatus::Exited,
                std::time::Duration::from_secs(30),
            );
            wait_until("the run to be filed", || {
                !core.run_history("svc").runs.is_empty()
            });

            let status = core.log_status("svc").expect("status");
            assert!(status.log_file.is_none(), "a file that does not exist");
            assert!(
                status.last_error.is_some(),
                "the failure to write has to be reported somewhere"
            );
            assert!(core
                .run_history("svc")
                .latest()
                .expect("a run")
                .log_file
                .is_none());
        }

        /// A run that ends before its output is read is still captured: the
        /// pipe holds the bytes, and the run is not over until they have been
        /// taken (`docs/LOGGING.md` §3).
        #[test]
        fn a_run_that_exits_immediately_still_gets_its_output_written() {
            let dir = TempDir::new();
            // Nothing that waits: the process writes and exits at once.
            let config = service_printing(
                "svc",
                "cmd.exe /c echo gone-already",
                captured(EffectiveLogMode::Always),
            );
            let (core, _sink) = core_logging_to(&dir, config);

            core.start("svc").expect("start succeeds");
            let file = announced_log(&core, "svc");

            wait_for_status(
                &core,
                "svc",
                SessionStatus::Exited,
                std::time::Duration::from_secs(30),
            );
            wait_until("the short run's output to reach its file", || {
                read(&file).contains("gone-already")
            });
        }
    }

    /// The terminal half of Session Core (T07 #8).
    ///
    /// These host a real PowerShell, like T02's own tests: the PTY contract was
    /// validated there, so what is under test here is what the *session* makes
    /// of it — which kind of run a terminal session starts, where its output
    /// goes, what the offsets an attached view reads actually promise, and what
    /// happens to the shell when the session is stopped.
    mod terminal_tests {
        use super::*;
        use crate::pty::INTERRUPT_BYTE;

        /// The absolute path T02's tests use, so these do not depend on how
        /// PATH happens to be set in the environment the tests run in.
        const POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.exe";

        /// Cold PowerShell on a busy machine can take a while to render its
        /// first prompt.
        const STARTUP: std::time::Duration = std::time::Duration::from_secs(40);

        fn terminal(id: &str) -> SessionConfig {
            SessionConfig {
                id: id.to_owned(),
                name: format!("Terminal {id}"),
                session_type: SessionType::Terminal,
                cwd: Some(test_cwd()),
                command: None,
                url: None,
                port: None,
                purpose: None,
                close_impact: None,
                shell: Some(format!("{POWERSHELL} -NoLogo -NoProfile")),
                initial_command: None,
                // A terminal's default policy: nothing is persisted
                // (`docs/LOGGING.md` §1.2), while its scrollback is still kept.
                logging: EffectiveLogging {
                    mode: EffectiveLogMode::Off,
                    source: LogSource::None,
                    external_path: None,
                },
            }
        }

        fn core_with_terminal(
            id: &str,
            configure: impl FnOnce(&mut SessionConfig),
        ) -> (SessionCore, Arc<RecordingSink>) {
            let sink = Arc::new(RecordingSink::default());
            let core = SessionCore::new(sink.clone());
            let mut config = terminal(id);
            configure(&mut config);
            core.register(config).expect("registration succeeds");
            (core, sink)
        }

        /// Everything the session has taken from the shell so far.
        fn scrollback(core: &SessionCore, id: &str) -> String {
            core.terminal_buffer(id)
                .unwrap_or_default()
                .iter()
                .map(|chunk| chunk.text())
                .collect()
        }

        /// Poll until `check` holds, so a test asserts on what the terminal
        /// actually produced rather than on how long it took.
        fn wait_until(what: &str, mut check: impl FnMut() -> bool) {
            let deadline = std::time::Instant::now() + STARTUP;
            while !check() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for {what}"
                );
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }

        /// Wait for `marker` to be rendered by the hosted shell.
        ///
        /// The marker strings in these tests are assembled inside the shell at
        /// run time, so only the executed command's output can satisfy the
        /// wait — the echoed command line cannot (`LCH-ECHO-` + `1A` never
        /// appears literally in what was sent).
        fn expect_in_scrollback(core: &SessionCore, id: &str, marker: &str) {
            wait_until(&format!("`{marker}` to render"), || {
                scrollback(core, id).contains(marker)
            });
        }

        /// Send one command line, the way a terminal sends Enter.
        fn send(core: &SessionCore, id: &str, line: &str) {
            core.terminal_write(id, format!("{line}\r").as_bytes())
                .expect("input reaches the terminal");
        }

        #[test]
        fn starting_a_terminal_hosts_a_shell_and_reports_it_attached() {
            let (core, _sink) = core_with_terminal("term", |_| {});

            let runtime = core.start("term").expect("start succeeds");

            assert_eq!(runtime.status, SessionStatus::Running);
            assert!(runtime.pid.is_some(), "a running terminal has a pid");
            assert!(runtime.run_id.is_some(), "a run has an id");
            assert!(
                runtime.pty_attached,
                "a terminal run reports the PTY the view types into"
            );
            assert_eq!(runtime.last_error, None);

            let attached = core
                .terminal_attachment("term")
                .expect("the session attaches");
            assert!(attached.pty_attached);
            assert!(attached.generation > 0, "the attachment names a run");

            core.stop("term").expect("cleanup");
        }

        /// The whole point of T07: what the shell prints reaches the session
        /// the UI renders, through the terminal the user is actually driving.
        #[test]
        fn a_command_in_the_hosted_shell_reaches_the_sessions_scrollback() {
            let (core, _sink) = core_with_terminal("term", |_| {});
            core.start("term").expect("start succeeds");

            send(&core, "term", "Write-Host (\"LCH-ECHO-\" + \"1A\")");

            expect_in_scrollback(&core, "term", "LCH-ECHO-1A");
            core.stop("term").expect("cleanup");
        }

        /// Spec §7: "For terminal sessions, Ctrl+C is not the same action as
        /// closing the session." The interrupt travels as input (T02's
        /// `INTERRUPT_BYTE`), so the session must still be running after it and
        /// must still accept the next command.
        #[test]
        fn ctrl_c_is_input_and_does_not_close_the_session() {
            let (core, _sink) = core_with_terminal("term", |_| {});
            core.start("term").expect("start succeeds");
            expect_in_scrollback(&core, "term", "PS");

            // Interrupt whatever is (or is about to be) running, then prove the
            // shell still answers.
            core.terminal_write("term", &[INTERRUPT_BYTE])
                .expect("the interrupt reaches the terminal");
            std::thread::sleep(std::time::Duration::from_millis(500));
            send(&core, "term", "Write-Host (\"LCH-AFTER-\" + \"CTRLC\")");

            expect_in_scrollback(&core, "term", "LCH-AFTER-CTRLC");
            let runtime = core.snapshot("term").expect("the session is registered");
            assert_eq!(
                runtime.status,
                SessionStatus::Running,
                "Ctrl+C must not close the session"
            );
            assert!(runtime.pty_attached);

            core.stop("term").expect("cleanup");
        }

        /// Stopping a terminal closes the console, and the shell with it: the
        /// session is not stopped while a shell it accounted for keeps running.
        #[test]
        fn stopping_a_terminal_ends_the_run_and_closes_the_shell() {
            let (core, _sink) = core_with_terminal("term", |_| {});
            let started = core.start("term").expect("start succeeds");
            let pid = started.pid.expect("a running terminal has a pid");

            let stopped = core.stop("term").expect("stop succeeds");

            assert_eq!(stopped.status, SessionStatus::Stopped);
            assert!(!stopped.pty_attached, "nothing is attached after a stop");
            assert_eq!(stopped.pid, None);
            wait_until("the hosted shell to be gone", || !pid_is_alive(pid));

            // The session is still there, and says why it cannot be typed into
            // rather than accepting bytes that would go nowhere.
            let error = core
                .terminal_write("term", b"echo hi\r")
                .expect_err("a stopped terminal takes no input");
            assert!(
                error.message.contains("no running terminal"),
                "unexpected message: {}",
                error.message
            );
        }

        #[test]
        fn restarting_a_terminal_replaces_the_run_and_leaves_one_shell() {
            let (core, _sink) = core_with_terminal("term", |_| {});
            let first = core.start("term").expect("start succeeds");
            let first_pid = first.pid.expect("a running terminal has a pid");

            let second = core.restart("term").expect("restart succeeds");

            assert_eq!(second.status, SessionStatus::Running);
            assert_ne!(second.run_id, first.run_id, "a restart is a new run");
            assert_ne!(second.pid, first.pid, "a restart is a new shell");
            assert!(second.pty_attached);
            wait_until("the replaced shell to be gone", || !pid_is_alive(first_pid));

            core.stop("term").expect("cleanup");
        }

        /// A supervised service has no attached stdin in the MVP; saying so is
        /// the difference between a UI that explains itself and one that sends
        /// keystrokes into nothing.
        #[test]
        fn input_into_a_service_is_refused() {
            let (core, _sink) = core_with_command("svc", LONG_RUNNING);
            core.start("svc").expect("start succeeds");

            let error = core
                .terminal_write("svc", b"hello\r")
                .expect_err("a service takes no terminal input");
            assert_eq!(error.kind, SessionErrorKind::Unsupported);
            assert!(
                error.message.contains("supervised service"),
                "unexpected message: {}",
                error.message
            );

            core.force_stop("svc").expect("cleanup");
        }

        #[test]
        fn input_into_a_session_that_never_started_is_refused() {
            let (core, _sink) = core_with_terminal("term", |_| {});

            let error = core
                .terminal_write("term", b"echo hi\r")
                .expect_err("nothing is running");
            assert!(
                error.message.contains("no running terminal"),
                "unexpected message: {}",
                error.message
            );
        }

        /// An attachment is the whole handover: the scrollback so far, the
        /// generation it belongs to, and the offset it reaches. A view replays
        /// the chunks and appends what comes after the offset — which is only
        /// sound if the offset is where the *next* batch begins.
        #[test]
        fn every_batch_after_an_attachment_begins_where_the_attachment_ended() {
            let (core, sink) = core_with_terminal("term", |_| {});
            core.start("term").expect("start succeeds");
            // Some output before the attach, so the scrollback is not empty and
            // the offset is not zero.
            send(&core, "term", "Write-Host (\"LCH-BEFORE-\" + \"ATTACH\")");
            expect_in_scrollback(&core, "term", "LCH-BEFORE-ATTACH");

            let attached = core
                .terminal_attachment("term")
                .expect("the session attaches");
            let after_attach = sink.events().len();
            assert!(attached.emitted > 0, "the replay covers what was printed");
            let replayed: usize = attached
                .chunks
                .iter()
                .map(|chunk| {
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD
                        .decode(&chunk.data)
                        .expect("chunks arrive as base64")
                        .len()
                })
                .sum();
            assert_eq!(
                replayed as u64, attached.emitted,
                "the offset must be exactly what the replay covers"
            );

            send(&core, "term", "Write-Host (\"LCH-AFTER-\" + \"ATTACH\")");
            expect_in_scrollback(&core, "term", "LCH-AFTER-ATTACH");

            // The scrollback takes bytes before the batch that carries them is
            // published, so wait for the publication rather than assume it.
            let published = || -> Vec<TerminalOutput> {
                sink.events()
                    .into_iter()
                    .skip(after_attach)
                    .filter_map(|event| match event {
                        SessionEvent::TerminalOutput(output) => Some(output),
                        _ => None,
                    })
                    .collect()
            };
            wait_until("the later output to be published", || {
                published().iter().any(|batch| batch.end > attached.emitted)
            });

            let live: Vec<TerminalOutput> = published()
                .into_iter()
                .filter(|batch| batch.end > attached.emitted)
                .collect();
            for batch in &live {
                assert!(
                    batch.start >= attached.emitted,
                    "batch {}..{} straddles the attachment offset {}, so a view \
                     that replayed the scrollback would have to splice it",
                    batch.start,
                    batch.end,
                    attached.emitted
                );
                assert_eq!(
                    batch.generation, attached.generation,
                    "a batch from another run must not be appended"
                );
            }

            core.stop("term").expect("cleanup");
        }

        /// A view measures itself before the session starts, and a shell
        /// started at the default 80×24 and corrected a moment later is not the
        /// same screen — so the size is remembered rather than dropped.
        #[test]
        fn a_resize_before_the_start_geometries_the_shell() {
            let (core, _sink) = core_with_terminal("term", |_| {});
            core.terminal_resize("term", 120, 30)
                .expect("a stopped session accepts a size");

            core.start("term").expect("start succeeds");
            send(
                &core,
                "term",
                "Write-Host (\"LCH-GEO-\" + \
                 \"$($Host.UI.RawUI.WindowSize.Width)x$($Host.UI.RawUI.WindowSize.Height)\")",
            );

            expect_in_scrollback(&core, "term", "LCH-GEO-120x30");
            core.stop("term").expect("cleanup");
        }

        #[test]
        fn a_resize_of_a_live_terminal_reaches_the_shell() {
            let (core, _sink) = core_with_terminal("term", |_| {});
            core.start("term").expect("start succeeds");
            expect_in_scrollback(&core, "term", "PS");

            core.terminal_resize("term", 100, 25)
                .expect("the terminal resizes");
            // A keystroke racing the console host's resize reinitialization can
            // be dropped by conhost — the ConPTY quirk noted on #3, and the
            // reason a view debounces its resize forwarding. Let the resize
            // settle before typing.
            std::thread::sleep(std::time::Duration::from_millis(500));
            send(
                &core,
                "term",
                "Write-Host (\"LCH-GEO-\" + \
                 \"$($Host.UI.RawUI.WindowSize.Width)x$($Host.UI.RawUI.WindowSize.Height)\")",
            );

            expect_in_scrollback(&core, "term", "LCH-GEO-100x25");
            core.stop("term").expect("cleanup");
        }

        #[test]
        fn an_unhostable_size_is_refused() {
            let (core, _sink) = core_with_terminal("term", |_| {});

            for (cols, rows) in [(0, 24), (80, 0), (MAX_DIMENSION + 1, 24)] {
                let error = core
                    .terminal_resize("term", cols, rows)
                    .expect_err("the size is not hostable");
                assert!(
                    error.message.contains("cells"),
                    "unexpected message for {cols}x{rows}: {}",
                    error.message
                );
            }
        }

        #[test]
        fn an_initial_command_is_typed_into_the_terminal() {
            let (core, _sink) = core_with_terminal("term", |config| {
                config.initial_command = Some("Write-Host (\"LCH-INIT-\" + \"2B\")".to_owned());
            });

            core.start("term").expect("start succeeds");

            expect_in_scrollback(&core, "term", "LCH-INIT-2B");
            core.stop("term").expect("cleanup");
        }

        /// A view is not what keeps a terminal alive.
        ///
        /// Selecting another session, switching to the Logs tab or hiding the
        /// window all look the same from here: nobody is attached. The shell
        /// must keep running, keep producing output, and be there — with
        /// everything it printed — when a view comes back (spec §6: "terminal
        /// session remaining alive while hidden or while another session is
        /// selected").
        #[test]
        fn a_terminal_keeps_running_while_no_view_is_attached() {
            let (core, _sink) = core_with_terminal("term", |_| {});
            let started = core.start("term").expect("start succeeds");
            let pid = started.pid.expect("a running terminal has a pid");

            send(&core, "term", "Write-Host (\"LCH-UNSEEN-\" + \"1\")");
            expect_in_scrollback(&core, "term", "LCH-UNSEEN-1");

            // Nothing reads the terminal from here on.
            std::thread::sleep(std::time::Duration::from_millis(300));
            send(&core, "term", "Write-Host (\"LCH-LATER-\" + \"2\")");
            expect_in_scrollback(&core, "term", "LCH-LATER-2");

            let runtime = core.snapshot("term").expect("the session is registered");
            assert_eq!(
                runtime.status,
                SessionStatus::Running,
                "an unwatched terminal must keep running"
            );
            assert_eq!(runtime.pid, Some(pid), "it is still the same shell");

            // A view arriving afterwards is given what it missed.
            let attached = core
                .terminal_attachment("term")
                .expect("the session attaches");
            assert!(attached.pty_attached);
            let replayed: String = attached.chunks.iter().map(chunk_text).collect();
            assert!(
                replayed.contains("LCH-UNSEEN-1") && replayed.contains("LCH-LATER-2"),
                "the replay must cover output from while nobody was watching: {replayed:?}"
            );

            core.stop("term").expect("cleanup");
        }

        /// The acceptance criterion behind the byte transport: a shell's output
        /// is UTF-8 plus ANSI, and it has to survive the trip through the
        /// session without being decoded as text on the way.
        ///
        /// The command prints the characters from their code points rather than
        /// sending them as input, so this measures the output path (what T07
        /// owns) and not the console's input code page.
        #[test]
        fn non_ascii_output_survives_the_session() {
            let (core, _sink) = core_with_terminal("term", |_| {});
            core.start("term").expect("start succeeds");

            send(
                &core,
                "term",
                "Write-Host (\"LCH-\" + [char]0x4E2D + [char]0x6587 + \"-\" + [char]0x2713)",
            );

            expect_in_scrollback(&core, "term", "LCH-中文-✓");
            core.stop("term").expect("cleanup");
        }

        /// Spec §14: "one high-output session must not freeze the whole UI",
        /// and §9: output must be "batched enough that high-volume output does
        /// not create an expensive UI event per line".
        ///
        /// The burst is 2 000 lines. What is asserted is not how fast it
        /// arrived but its *shape*: every batch is inside the size ceiling,
        /// batches are contiguous (no byte is skipped or repeated), and the
        /// whole burst is far fewer events than it is lines.
        #[test]
        fn a_burst_of_output_is_published_in_bounded_contiguous_batches() {
            let (core, sink) = core_with_terminal("term", |_| {});
            core.start("term").expect("start succeeds");
            expect_in_scrollback(&core, "term", "PS");
            let before = sink.events().len();

            send(
                &core,
                "term",
                "for ($i = 0; $i -lt 2000; $i++) { Write-Host \"LCH-LINE $i\" }; \
                 Write-Host (\"LCH-BURST-\" + \"END\")",
            );
            expect_in_scrollback(&core, "term", "LCH-BURST-END");

            let batches: Vec<TerminalOutput> = sink
                .events()
                .into_iter()
                .skip(before)
                .filter_map(|event| match event {
                    SessionEvent::TerminalOutput(output) => Some(output),
                    _ => None,
                })
                .collect();

            assert!(!batches.is_empty(), "the burst produced no output events");
            for pair in batches.windows(2) {
                assert_eq!(
                    pair[0].end, pair[1].start,
                    "batches must be contiguous: {}..{} then {}..{}",
                    pair[0].start, pair[0].end, pair[1].start, pair[1].end
                );
            }
            for batch in &batches {
                let covered = batch.end - batch.start;
                assert_eq!(
                    covered,
                    batch_bytes(batch) as u64,
                    "a batch's range must be exactly the bytes it carries"
                );
                assert!(
                    covered <= crate::session::terminal::BATCH_MAX_BYTES as u64,
                    "batch {}..{} is larger than the ceiling",
                    batch.start,
                    batch.end
                );
            }
            assert!(
                batches.len() < 200,
                "a 2 000-line burst must not become one event per line; got {} events",
                batches.len()
            );

            let text = scrollback(&core, "term");
            assert!(
                text.contains("LCH-LINE 0") && text.contains("LCH-LINE 1999"),
                "the whole burst must reach the scrollback"
            );
            core.stop("term").expect("cleanup");
        }

        /// One retained chunk as text, for assertions about a replay.
        fn chunk_text(chunk: &RetainedChunk) -> String {
            use base64::Engine;
            String::from_utf8_lossy(
                &base64::engine::general_purpose::STANDARD
                    .decode(&chunk.data)
                    .expect("chunks arrive as base64"),
            )
            .into_owned()
        }

        /// The bytes one published batch carries.
        fn batch_bytes(batch: &TerminalOutput) -> usize {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode(&batch.data)
                .expect("batches arrive as base64")
                .len()
        }
    }
}
