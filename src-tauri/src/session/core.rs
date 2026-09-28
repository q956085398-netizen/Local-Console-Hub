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

use serde::Serialize;

use crate::config::{SessionConfig, SessionType};
use crate::process::{ManagedProcess, ProcessSpec};

use super::event::{
    AppSummary, AppSummaryChanged, RunRecordUpdated, SessionEvent, SessionStateChanged,
};
use super::runtime::{RunId, RunRecord, SessionErrorInfo, SessionRuntime, Timestamp};
use super::state::SessionStatus;

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
    run: Option<Arc<ManagedProcess>>,
    /// Bumped on every start. A watcher captures the generation it was started
    /// for and refuses to publish an exit that belongs to a superseded run.
    generation: u64,
    /// The current run's record, open until the run ends (spec §4). Kept here
    /// rather than in the snapshot because the snapshot is the UI's view and
    /// the record is the run-history entry.
    record: Option<RunRecord>,
}

/// The registry plus everything needed to publish.
///
/// Cheap to clone: every clone shares the same sessions, which is how a
/// watcher thread and the IPC layer address the same state.
#[derive(Clone)]
pub struct SessionCore {
    sessions: Arc<Mutex<BTreeMap<String, Arc<Mutex<SessionState>>>>>,
    sink: Arc<dyn EventSink>,
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
    /// A core that publishes to `sink`.
    pub fn new(sink: Arc<dyn EventSink>) -> Self {
        SessionCore {
            sessions: Arc::new(Mutex::new(BTreeMap::new())),
            sink,
        }
    }

    /// A core with no listener.
    pub fn without_listener() -> Self {
        SessionCore::new(Arc::new(NoopSink))
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
        sessions.insert(
            config.id.clone(),
            Arc::new(Mutex::new(SessionState {
                config,
                runtime: runtime.clone(),
                run: None,
                generation: 0,
                record: None,
            })),
        );
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
        let spec = {
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
            // Refuse before mutating anything: a session with no usable
            // command has not "tried to start".
            let spec = process_spec(&state.config, session_id)?;

            state.runtime.status = SessionStatus::Starting;
            state.runtime.last_error = None;
            spec
        };
        // The listener sees the start in flight, which is what a UI needs to
        // keep the action buttons from lying about what is happening.
        self.publish_state_and_summary(session_id);

        let spawned = ManagedProcess::spawn(spec);

        let outcome = {
            let mut state = lock(&handle);

            match spawned {
                Ok(run) => {
                    let run_id = RunId::mint();
                    let started_at = Timestamp::now();
                    let pid = run.pid();
                    let record = RunRecord {
                        run_id: run_id.clone(),
                        session_id: session_id.to_owned(),
                        started_at,
                        ended_at: None,
                        exit_code: None,
                        pid: Some(pid),
                        log_mode: state.config.logging.mode,
                        log_source: state.config.logging.source,
                        log_file: None,
                    };

                    // The generation marks this run; a watcher started for it
                    // must not be able to publish an exit against a later one.
                    state.generation += 1;
                    state.run = Some(Arc::new(run));
                    state.record = Some(record);

                    state.runtime.pid = Some(pid);
                    state.runtime.run_id = Some(run_id);
                    state.runtime.started_at = Some(started_at);
                    state.runtime.exit_code = None;
                    state.runtime.status = SessionStatus::Running;

                    Ok(state.runtime.clone())
                }
                Err(error) => {
                    let message = error.to_string();
                    state.runtime.status = SessionStatus::Error;
                    state.runtime.pid = None;
                    state.runtime.run_id = None;
                    state.run = None;
                    state.record = None;
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
        if outcome.is_ok() {
            self.publish_ending(session_id);

            // Started only after `Running` is on the wire. A run can end very
            // quickly, and a watcher that got there first would publish
            // `Exited` before `Running` — a listener would then be told a
            // session ended before it was told it started.
            let generation = lock(&handle).generation;
            std::thread::spawn({
                let core = self.clone();
                let handle = Arc::clone(&handle);
                let session_id = session_id.to_owned();
                move || watch_run(core, handle, session_id, generation)
            });
        } else {
            self.publish_state_and_summary(session_id);
        }
        outcome
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
            Ok(_) => self.publish_ending(session_id),
            Err(_) => self.publish_state_and_summary(session_id),
        }
        outcome
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

        if let Some(record) = self.record.as_mut() {
            record.ended_at = Some(Timestamp::now());
            record.exit_code = code;
        }
    }
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
            Some(run) => Arc::clone(run),
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
    }

    {
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
    }

    core.publish_ending(&session_id);
}

/// Build the spec for a startable session, or explain why it has none.
///
/// Only service sessions get a supervised process here. A terminal session's
/// run is a PTY, which T07 wires to this timeline; spawning a plain process
/// for one now would be a second, competing terminal implementation
/// (EXECUTION_PLAN §2.3).
fn process_spec(config: &SessionConfig, id: &str) -> Result<ProcessSpec, SessionError> {
    const OPERATION: &str = "start";

    if config.session_type != SessionType::Service {
        return Err(SessionError::unsupported(
            id,
            OPERATION,
            format!(
                "session `{id}` is an interactive terminal; its run is a PTY, which T07 \
                 attaches — Session Core does not start one"
            ),
        ));
    }

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

    Ok(ProcessSpec { program, args, cwd })
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

        let spec = process_spec(&config, "svc").expect("a service has a spec");
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

        let error = process_spec(&config, "svc").expect_err("refused");
        assert_eq!(error.kind, SessionErrorKind::Failed);
        assert!(error.message.contains("cwd"), "message: {}", error.message);
    }

    /// `process_spec` is the seam between a configured session and a supervised
    /// run; only services have one until T07 wires terminals.
    #[test]
    fn a_terminal_session_has_no_supervised_process() {
        let mut config = service("term");
        config.session_type = SessionType::Terminal;
        config.command = None;
        config.shell = Some("powershell".to_owned());

        let error = process_spec(&config, "term").expect_err("terminals are T07");
        assert_eq!(error.kind, SessionErrorKind::Unsupported);
        assert_eq!(error.session_id, "term");
        assert!(error.message.contains("T07"), "message: {}", error.message);
    }
}
