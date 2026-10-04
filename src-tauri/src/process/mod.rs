//! Process layer — process supervision, graceful stop and safe kill-tree.
//!
//! Owned by T03 (#4, process supervisor — SAFETY BLOCKER). One
//! [`ManagedProcess`] owns one run: it knows exactly which process belongs to
//! the run, reclaims the run's descendants, distinguishes a graceful stop from
//! a force kill (`docs/DECISIONS.md` D-007), and never terminates anything
//! outside the managed tree (`docs/MVP_IMPLEMENTATION_SPEC.md` §7, §15).
//!
//! ## Contract
//!
//! ```text
//! spawn(ProcessSpec) -> ManagedProcess
//!   .pid() / .is_running() / .exit_status() / .wait_for_exit(timeout) / .tree_pids()
//!   .stop(timeout) / .force_stop() / .restart(spec, timeout)
//! StopOutcome { Exited | Forced | AlreadyExited }
//! ```
//!
//! `stop` and `force_stop` are **barriers**: when they return `Ok`, the managed
//! tree is confirmed gone. That is what makes "a restart cannot leave duplicate
//! managed instances" a property of this layer instead of something the session
//! state machine has to defend against later. The barrier covers the case a
//! launcher script produces — the run's own process exits while a child it
//! started lives on — because a stop tears down the tree, not the pid
//! (`docs/DEVELOPMENT.md` §6).
//!
//! ## Mechanism (Windows)
//!
//! Every run owns a job object created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
//! and the run's process is assigned to it (the `win` backend). Job membership
//! *is* the ownership boundary, so a force stop cannot reach a process that is
//! not part of the run — including the case where the managed pid has already
//! exited and been reused by an unrelated process, which pid-based termination
//! cannot rule out. A graceful stop is a best-effort `CTRL_BREAK` to the run's
//! process group; when it cannot be delivered (a process with no console has
//! nothing to deliver to) the run falls through to the timeout and then to the
//! force path.
//!
//! ## The run's console
//!
//! A run is a console application, and the console it is given is one the
//! desktop is never asked to show: a console allocated for a run whose parent
//! has none of its own is handed to whatever the machine uses as its default
//! terminal application, which puts a stray window on screen titled after the
//! program being run — one per run (`docs/DECISIONS.md` D-035). The run
//! therefore shares the Hub's console when the Hub has one, and is started with
//! `CREATE_NO_WINDOW` when it does not. What the run keeps is a console: it is
//! what the graceful request above travels through, and removing it would take
//! the stop ladder's first rung with it.
//!
//! A [`ManagedProcess`] owns its run completely: dropping the handle terminates
//! whatever is left of the run, so a session cannot outlive the supervisor that
//! accounts for it.
//!
//! Two environment notes this layer does not hide. A run started from inside a
//! pre-existing job object — some CI runners wrap each build step in one — can
//! only be given a job of its own if that job permits nesting; when it does not,
//! starting the run fails loudly instead of running unsupervised, which is the
//! intended outcome for a process this layer cannot own. And a `CTRL_BREAK` has
//! nowhere to go in a process with no console, which is why graceful delivery is
//! reported (`StopReport::graceful_delivered`) rather than assumed.
//!
//! Output plumbing stops at the pipe: a run can be asked to have its stdout and
//! stderr captured ([`OutputMode::Capture`]), and the supervisor then hands the
//! two streams to its caller through [`ManagedProcess::take_output`]. Nothing
//! here reads, buffers or decides what to do with a byte of it — that is the
//! logging layer's business (T05), and interactive terminals are hosted by the
//! PTY layer (T02). This layer owns lifecycle only.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod win;

/// A run that keeps its own window and console, and outlives the Hub (#66).
pub mod independent;

// The job object that owns a managed process tree, shared with the PTY
// backend: a terminal shell's tree is the same Win32 object supervised under
// the same rules, and `tree` is where the spec puts "kill tree". Test builds
// also reach it through `win`'s failure-injection helpers.
#[cfg(windows)]
pub(crate) mod tree;

#[cfg(not(windows))]
use unsupported as backend;
#[cfg(windows)]
use win as backend;

/// Graceful-stop timeout to use when a caller has no configured value. The stop
/// path is "request gracefully, wait, force" (`docs/PRODUCT_SPEC.md` §4), so
/// this is the wait between the request and the force kill.
pub const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a stop waits for the tree to be observably gone *after* the run's
/// own process has exited.
const TREE_GONE_TIMEOUT: Duration = Duration::from_secs(5);

/// Poll interval of the bounded teardown checks. These only run while a stop is
/// in flight and only against one job object, never as a background scan
/// (D-009).
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// How long the exit watcher retries `try_wait` after the process object has
/// signalled before it gives up on the exit code.
const REAP_TIMEOUT: Duration = Duration::from_millis(500);

/// What to start for one run.
///
/// Deliberately argv-shaped: turning a configured command string into `program`
/// plus `args` (and choosing the shell that runs it) is the caller's decision,
/// not a policy this layer should invent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSpec {
    /// Executable to start. A full path is preferred; `PATH` lookup is left to
    /// the platform.
    pub program: PathBuf,
    /// Arguments, passed through as-is — no shell interpretation.
    pub args: Vec<String>,
    /// Working directory for the run.
    pub cwd: PathBuf,
    /// What to do with the run's stdout and stderr.
    pub output: OutputMode,
}

impl ProcessSpec {
    /// A run of `program` in `cwd` with no arguments.
    pub fn new(program: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        ProcessSpec {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            output: OutputMode::default(),
        }
    }

    /// Builder-style: replace the argument list.
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Builder-style: choose what happens to the run's output.
    pub fn with_output(mut self, output: OutputMode) -> Self {
        self.output = output;
        self
    }
}

/// What happens to a run's stdout and stderr.
///
/// Only two answers, because they are the two the supervisor can honour
/// without knowing anything about logging: leave the streams as they are, or
/// hand them over. Which of them a session gets is decided by its logging
/// policy (`docs/LOGGING.md` §2) — never here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputMode {
    /// The run inherits the Hub's handles, as it did before T05. Nothing can
    /// block, and nothing is captured.
    #[default]
    Inherit,
    /// Both streams are piped back to [`ManagedProcess::take_output`].
    ///
    /// A pipe that nobody drains fills up and stops the process writing to it,
    /// so a captured run **must** have its output taken. The supervisor cannot
    /// enforce that, and does not pretend to: it makes the requirement loud in
    /// this doc, and dropping an untaken pipe unblocks a stuck run rather than
    /// hanging it (see [`ManagedProcess::take_output`]).
    Capture,
}

/// A run's piped output streams, handed over once.
///
/// Plain readers rather than a channel or a callback: the layer that captures
/// output decides how to read it, how much to read at a time, and where it
/// goes, and the supervisor keeps owning nothing but the lifecycle.
pub struct ProcessOutput {
    pub stdout: Option<Box<dyn Read + Send>>,
    pub stderr: Option<Box<dyn Read + Send>>,
}

impl std::fmt::Debug for ProcessOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessOutput")
            .field("stdout", &self.stdout.is_some())
            .field("stderr", &self.stderr.is_some())
            .finish()
    }
}

/// What identifies one process beyond the number Windows reuses (#66).
///
/// A pid is an index, not an identity: Windows hands the same number to a later
/// process once the process object behind it is gone. Everything this layer does
/// to a *supervised* run is addressed through a handle it holds, so the number
/// never has to be trusted — but a standalone application is remembered across
/// calls that happen later (its window is searched for, its stop may be
/// requested), and a remembered number is exactly where "the same pid" could
/// mean somebody else's process. Spec #59 decision 11 asks for that to be
/// impossible rather than unlikely, so the run carries the creation timestamp
/// Windows reports and checks it before acting on the number.
///
/// What [`ProcessIdentity::matches`] is not: a liveness test. While any handle
/// to the process object is open — and a run this layer is holding has one —
/// Windows cannot reuse that pid, so the identity of a process that has *ended*
/// still matches its own object. That is the right answer for the thing it
/// guards: a request is delivered to the process this run started, or to nobody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessIdentity {
    pid: u32,
    created_at: u64,
}

impl ProcessIdentity {
    /// Take the identity of a process the supervisor has just started.
    pub(crate) fn of_child(child: &Child) -> Self {
        ProcessIdentity {
            pid: child.id(),
            // A process whose creation time cannot be read is one this layer
            // cannot tell apart later, so its identity is `None`-like: it
            // matches nothing (`matches` compares against a value no process
            // reports).
            created_at: backend::creation_time(child).unwrap_or(u64::MAX),
        }
    }

    /// Record a pid and the creation time just observed for it.
    ///
    /// This does not ask Windows whether the pair is still current.
    /// [`Self::matches`] is that question for the process this identity names.
    /// A tree member is remembered this way so a later reading can require the
    /// same creation time, instead of trusting the pid after Windows has
    /// reused it.
    pub(crate) fn recorded(pid: u32, created_at: u64) -> Self {
        ProcessIdentity { pid, created_at }
    }

    /// The process id, as Windows reported it when this identity was taken.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// When the process was created, in the 100-ns units Windows reports.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Whether the process Windows reports under this id *now* is the one this
    /// identity was taken from.
    pub fn matches(&self) -> bool {
        backend::creation_time_of(self.pid) == Some(self.created_at)
    }

    /// The same identity, taken as if by hand, so the mismatch this guards
    /// against can be exercised without waiting for Windows to reuse a pid.
    #[cfg(test)]
    pub(crate) fn for_test(pid: u32, created_at: u64) -> Self {
        ProcessIdentity { pid, created_at }
    }
}

/// When a process with this identity started, as whole seconds since the Unix
/// epoch (#67).
///
/// Windows reports creation time in 100-ns units from 1601-01-01; the epoch
/// offset is the constant every reader of `FILETIME` uses, and it is written
/// here rather than derived so a wrong answer is one number to check. `None`
/// for an identity taken without a readable creation time, which is the
/// `u64::MAX` sentinel [`ProcessIdentity::of_child`] stores.
pub fn started_unix_secs(created_at: u64) -> Option<i64> {
    const TICKS_PER_SECOND: u64 = 10_000_000;
    /// Seconds between 1601-01-01 and 1970-01-01.
    const EPOCH_OFFSET: i64 = 11_644_473_600;
    if created_at == u64::MAX {
        return None;
    }
    Some((created_at / TICKS_PER_SECOND) as i64 - EPOCH_OFFSET)
}

/// One process, as the operating system currently reports it (#67).
///
/// A reading, not a handle: everything here was true when the process table was
/// read, and the process may be gone — or its number handed to somebody else —
/// by the time a caller acts on it. What keeps an action aimed at *this*
/// process is [`ProcessIdentity`], which the caller takes from `created_at` and
/// checks again before acting (`ProcessIdentity::matches`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessReading {
    pub pid: u32,
    /// When the process was created, in the same units [`ProcessIdentity`]
    /// compares. `None` when it could not be read — and a reading without it is
    /// one no identity can be taken from, which is why a caller that needs to
    /// act later treats it as "could not confirm".
    pub created_at: Option<u64>,
    /// The executable's file name. Present even for a process this user may not
    /// inspect, because it comes from the table rather than from the process.
    pub file_name: String,
    /// The full path of the image that is running, when it could be read.
    ///
    /// `None` is the "could not check" answer — usually permissions — and it is
    /// deliberately not the same value as a path that differs: one says this
    /// process *is* the program, the other says it is not, and "could not
    /// check" is the answer that must not be promoted to either (spec #59
    /// decision 11).
    pub image_path: Option<PathBuf>,
    /// The command line the process was started with, when it could be read.
    pub command_line: Option<String>,
}

impl ProcessReading {
    /// The identity this reading describes.
    ///
    /// `None` when the creation time could not be read, which is also the
    /// answer for a process this user may not open: a reading without one is
    /// one nothing may be remembered about ([`ProcessIdentity`]).
    pub fn identity(&self) -> Option<ProcessIdentity> {
        Some(ProcessIdentity {
            pid: self.pid,
            created_at: self.created_at?,
        })
    }
}

/// Every process whose executable file name is one of `names` (#67).
///
/// A name-shaped filter rather than the whole table: reading a process' image
/// path and command line means opening it, and the caller already knows which
/// name it is looking for. Reading it any wider would be the "全系统扫描"
/// D-009 rules out, on a path a user triggers by clicking an entry.
///
/// `None` means the table **could not be read**, which is a different claim
/// from an empty list — and the difference is the point. A caller asking "is
/// this application already running?" starts a second copy when it hears "no",
/// so "I could not look" must never be reported as "nothing is there".
pub fn processes_named(names: &[String]) -> Option<Vec<ProcessReading>> {
    backend::processes_named(names)
}

/// Every process started by `pid`, directly or through a chain of children
/// (#67).
///
/// The window an application presents is not always its own process's: a
/// launcher hands off to the program that really runs, and the window belongs
/// to that one (`crate::window`). Descendants are how the two are connected
/// without a name — never by title, and never by executable name.
pub fn descendants(pid: u32) -> Vec<u32> {
    backend::descendants(pid)
}

/// When the process currently reported under `pid` was created.
///
/// The same clock [`ProcessIdentity::matches`] compares against, read for a
/// pid this layer did not start. `None` means the process could not be opened
/// or its creation time could not be read — the identity cannot be confirmed.
/// It does not mean the pid is free, and it is not a reason to signal or
/// terminate anything: this only queries the process.
pub fn creation_time_of(pid: u32) -> Option<u64> {
    backend::creation_time_of(pid)
}

/// What [`end_confirmed_tree`] did to the process it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConfirmedEnd {
    /// The verified process was asked to end.
    Ended,
    /// The process had already exited, or the pid is no longer that process.
    /// Nothing was signalled.
    NothingEnded,
}

/// End the process tree `identity` names, after the creation time is read again.
///
/// The pid is not enough: Windows reuses it. When the process has exited, or
/// the creation time now reported for that pid is different, this returns
/// [`ConfirmedEnd::NothingEnded`] and does not signal whoever holds the number.
/// It does not choose a process by port or by executable name, and it does not
/// use the managed-session job. Only the verified process and descendants whose
/// creation times still match the ones just read are signalled.
pub(crate) fn end_confirmed_tree(identity: ProcessIdentity) -> ConfirmedEnd {
    if !identity.matches() {
        return ConfirmedEnd::NothingEnded;
    }
    #[cfg(windows)]
    {
        end_confirmed_tree_windows(identity)
    }
    #[cfg(not(windows))]
    {
        ConfirmedEnd::NothingEnded
    }
}

/// Open `identity`, refuse it when the handle is not that process or it has
/// already exited, then end the descendants that still match and the process
/// itself. The handle is what keeps the pid from being reused underneath the
/// call.
#[cfg(windows)]
fn end_confirmed_tree_windows(identity: ProcessIdentity) -> ConfirmedEnd {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, GetProcessTimes, OpenProcess, TerminateProcess,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    struct Opened(usize);

    impl Drop for Opened {
        fn drop(&mut self) {
            if self.0 != 0 {
                unsafe { CloseHandle(self.0 as _) };
            }
        }
    }

    fn open_process(pid: u32) -> Option<Opened> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE,
                0,
                pid,
            )
        } as usize;
        (handle != 0).then_some(Opened(handle))
    }

    fn creation_ticks(process: usize) -> Option<u64> {
        let mut created: FILETIME = unsafe { std::mem::zeroed() };
        let mut exited: FILETIME = unsafe { std::mem::zeroed() };
        let mut kernel: FILETIME = unsafe { std::mem::zeroed() };
        let mut user: FILETIME = unsafe { std::mem::zeroed() };
        let read = unsafe {
            GetProcessTimes(
                process as _,
                &mut created,
                &mut exited,
                &mut kernel,
                &mut user,
            )
        };
        (read != 0)
            .then_some(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
    }

    fn still_running(process: usize) -> bool {
        let mut code = 0u32;
        let read = unsafe { GetExitCodeProcess(process as _, &mut code) };
        read != 0 && code == STILL_ACTIVE as u32
    }

    fn terminate_if_same(member: ProcessIdentity) {
        let Some(opened) = open_process(member.pid()) else {
            return;
        };
        if creation_ticks(opened.0) != Some(member.created_at()) || !still_running(opened.0) {
            return;
        }
        unsafe { TerminateProcess(opened.0 as _, 1) };
    }

    let Some(root) = open_process(identity.pid()) else {
        return ConfirmedEnd::NothingEnded;
    };
    if creation_ticks(root.0) != Some(identity.created_at()) || !still_running(root.0) {
        return ConfirmedEnd::NothingEnded;
    }

    let members: Vec<ProcessIdentity> = descendants(identity.pid())
        .into_iter()
        .filter(|pid| *pid != identity.pid())
        .filter_map(|pid| {
            creation_time_of(pid).map(|created| ProcessIdentity::recorded(pid, created))
        })
        .collect();
    for member in members.into_iter().rev() {
        terminate_if_same(member);
    }

    if creation_ticks(root.0) != Some(identity.created_at()) || !still_running(root.0) {
        return ConfirmedEnd::NothingEnded;
    }
    if unsafe { TerminateProcess(root.0 as _, 1) } == 0 {
        return ConfirmedEnd::NothingEnded;
    }
    ConfirmedEnd::Ended
}

/// A handle on a process the Hub did not start (#67).
///
/// An application the user ran outside the Hub is not one the Hub may end, but
/// it *is* one whose ending the Hub has to notice: an associated instance that
/// has closed must stop being reported as running. Opening the process object
/// once gives an efficient wait and an exact answer — the handle is the process
/// itself, so no later reading of the number can be about somebody else.
///
/// Opening verifies the identity it was given, which is the whole reason this
/// takes one rather than a pid: between reading the process table and opening
/// the process, Windows may have ended it and handed its number to another
/// program, and a handle to *that* one would make the Hub report, and later
/// watch, the wrong process (spec #59 decision 11).
#[derive(Debug)]
pub struct ExternalProcess {
    handle: usize,
}

impl ExternalProcess {
    /// Open the process an identity describes, or say why it could not be.
    pub fn open(identity: ProcessIdentity) -> Result<Self, ProcessError> {
        backend::open_external(identity).map(|handle| ExternalProcess { handle })
    }

    /// Wait up to `timeout` for the process to end. `true` means it has.
    pub fn wait(&self, timeout: Duration) -> bool {
        backend::wait_for_handle_timeout(self.handle, timeout)
    }

    /// The code the process ended with, once it has ended.
    pub fn exit_code(&self) -> Option<u32> {
        backend::exit_code(self.handle)
    }
}

impl Drop for ExternalProcess {
    fn drop(&mut self) {
        backend::close_handle(self.handle);
    }
}

/// How a stop ended the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// The run's process exited after a graceful request, within the timeout.
    Exited,
    /// The managed tree had to be terminated: either the run outlasted the
    /// timeout, or its own process had already ended while descendants remained.
    Forced,
    /// Nothing of the run was left alive when it was stopped — own process and
    /// descendants alike.
    AlreadyExited,
}

/// Observed result of one run ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitStatus {
    /// The platform exit code, when one is known. On Windows this is the raw
    /// `u32` exit code that `std::process::ExitStatus::code` reports
    /// sign-extended as `i32`.
    pub code: Option<u32>,
}

/// Result of a stop request (`stop` or `force_stop`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopReport {
    /// Which path ended the run.
    pub outcome: StopOutcome,
    /// Exit status observed once the managed tree was confirmed gone.
    pub exit: ExitStatus,
    /// Whether the graceful request actually reached the run. Always `false`
    /// when no request was attempted — a run that had already ended is never
    /// signalled, so it cannot be a signal sent to a recycled pid.
    pub graceful_delivered: bool,
}

/// Structured failure of a supervised operation. Messages name the operation and
/// the paths involved so a user can act on them (`docs/DEVELOPMENT.md` §9).
#[derive(Debug)]
pub enum ProcessError {
    /// The run could not be started.
    Spawn {
        program: PathBuf,
        cwd: PathBuf,
        source: std::io::Error,
    },
    /// The run started but the supervisor could not take ownership of it, or
    /// could not confirm a teardown. The run is terminated before this is
    /// returned.
    Supervision {
        operation: &'static str,
        reason: String,
    },
    /// This platform has no supervisor backend (Windows-first MVP, D-001).
    UnsupportedPlatform { operation: &'static str },
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::Spawn {
                program,
                cwd,
                source,
            } => write!(
                formatter,
                "failed to start `{}` in working directory `{}`: {source}",
                program.display(),
                cwd.display()
            ),
            ProcessError::Supervision { operation, reason } => {
                write!(formatter, "{operation} failed: {reason}")
            }
            ProcessError::UnsupportedPlatform { operation } => write!(
                formatter,
                "{operation} is not supported on this platform — Local Console Hub \
                 supervises processes on Windows only (D-001)"
            ),
        }
    }
}

impl std::error::Error for ProcessError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProcessError::Spawn { source, .. } => Some(source),
            ProcessError::Supervision { .. } | ProcessError::UnsupportedPlatform { .. } => None,
        }
    }
}

/// State shared between the handle and its exit watcher thread.
#[derive(Debug)]
struct Shared {
    /// The run's own process. Only ever locked for short, non-blocking calls:
    /// the exit watcher blocks on the process object, never while holding this.
    child: Mutex<Child>,
    /// Tree ownership token; closing it terminates whatever is still alive in
    /// the run.
    tree: backend::TreeHandle,
    pid: u32,
    /// Filled in by the exit watcher exactly once.
    exit: Mutex<Option<ExitStatus>>,
    exited: Condvar,
    /// The run's piped streams, until [`ManagedProcess::take_output`] claims
    /// them. `None` for a run that was not started with capture.
    output: Mutex<Option<ProcessOutput>>,
}

/// One supervised run.
///
/// `ManagedProcess` is `Send + Sync` and its observation and stop methods take
/// `&self`, so a session registry can share one handle across IPC, tray and
/// watcher threads without wrapping it in another lock.
#[derive(Debug)]
pub struct ManagedProcess {
    shared: Arc<Shared>,
}

#[cfg(all(test, windows))]
type BeforeJobAssignmentTestHook = Box<dyn FnOnce(u32)>;

#[cfg(all(test, windows))]
thread_local! {
    /// Lets the native regression hold the caller at the exact process-start
    /// boundary so an unsuspended launcher can publish its child handshake
    /// before the old post-spawn job assignment runs.
    static BEFORE_JOB_ASSIGNMENT_TEST_HOOK: std::cell::RefCell<Option<BeforeJobAssignmentTestHook>> =
        std::cell::RefCell::new(None);
}

#[cfg(all(test, windows))]
fn run_before_job_assignment_test_hook(child: &Child) {
    BEFORE_JOB_ASSIGNMENT_TEST_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook(child.id());
        }
    });
}

impl ManagedProcess {
    /// Start a run and take ownership of its process tree.
    pub fn spawn(spec: ProcessSpec) -> Result<Self, ProcessError> {
        backend::require_backend("starting a supervised process")?;

        let mut command = Command::new(&spec.program);
        command.args(&spec.args).current_dir(&spec.cwd);
        if spec.output == OutputMode::Capture {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        }
        let mut child = backend::spawn(&mut command, backend::prepare).map_err(|source| {
            ProcessError::Spawn {
                program: spec.program.clone(),
                cwd: spec.cwd.clone(),
                source,
            }
        })?;

        #[cfg(all(test, windows))]
        run_before_job_assignment_test_hook(&child);

        let tree = match backend::attach(&child) {
            Ok(tree) => tree,
            Err(reason) => {
                // A run the supervisor cannot own must not be left alive: the
                // ownership boundary failed, so nothing may rely on it.
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProcessError::Supervision {
                    operation: "assigning the suspended run to its job object before resuming it",
                    reason: format!(
                        "process `{}` in working directory `{}`: {reason}",
                        spec.program.display(),
                        spec.cwd.display()
                    ),
                });
            }
        };

        // Taken out of the child now rather than behind a lock later: the
        // handles belong to the run, not to the child object, and a run whose
        // pipes nobody drains is the one case this layer cannot make safe on
        // its own.
        let output = (spec.output == OutputMode::Capture).then(|| ProcessOutput {
            stdout: child
                .stdout
                .take()
                .map(|stdout| Box::new(stdout) as Box<dyn Read + Send>),
            stderr: child
                .stderr
                .take()
                .map(|stderr| Box::new(stderr) as Box<dyn Read + Send>),
        });

        let pid = child.id();
        let shared = Arc::new(Shared {
            child: Mutex::new(child),
            tree,
            pid,
            exit: Mutex::new(None),
            exited: Condvar::new(),
            output: Mutex::new(output),
        });
        if let Err(source) = watch_exit(&shared) {
            let _ = backend::terminate_tree(&shared.tree);
            return Err(ProcessError::Supervision {
                operation: "starting the exit watcher",
                reason: source.to_string(),
            });
        }
        Ok(ManagedProcess { shared })
    }

    /// The run's own process id.
    pub fn pid(&self) -> u32 {
        self.shared.pid
    }

    /// Take the run's piped streams, if it was started with
    /// [`OutputMode::Capture`].
    ///
    /// Hands them over exactly once: the read ends of a pipe have one owner,
    /// and two readers racing for the same bytes would split a run's output
    /// between them. A second call answers `None`.
    ///
    /// Whoever takes them owns the obligation to read them: a pipe fills, and
    /// a full pipe stops the process writing to it. Dropping the streams
    /// without reading closes this end, which a blocked run sees as a broken
    /// pipe and normally answers by exiting — the outcome this layer prefers
    /// over a run that can never finish.
    pub fn take_output(&self) -> Option<ProcessOutput> {
        lock(&self.shared.output).take()
    }

    /// Whether the run's own process is still alive.
    pub fn is_running(&self) -> bool {
        self.exit_status().is_none()
    }

    /// The exit status once the run has ended, without blocking.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        if let Some(exit) = *lock(&self.shared.exit) {
            return Some(exit);
        }
        // The watcher owns the record, but asking the process object directly
        // keeps the answer correct in the moment before it is written — and
        // correct even if the watcher thread never got to write it.
        match lock(&self.shared.child).try_wait() {
            Ok(Some(status)) => Some(observe_exit(status)),
            _ => None,
        }
    }

    /// Wait up to `timeout` for the run to end. `None` means it was still
    /// running when the timeout expired.
    pub fn wait_for_exit(&self, timeout: Duration) -> Option<ExitStatus> {
        let exit = lock(&self.shared.exit);
        if let Some(recorded) = *exit {
            return Some(recorded);
        }
        let (exit, _) = self
            .shared
            .exited
            .wait_timeout_while(exit, timeout, |recorded| recorded.is_none())
            .unwrap_or_else(|p| p.into_inner());
        if let Some(recorded) = *exit {
            return Some(recorded);
        }
        drop(exit);
        self.exit_status()
    }

    /// Ids of every process currently assigned to the run, including the run's
    /// own process. An empty list means nothing of the run is left.
    pub fn tree_pids(&self) -> Result<Vec<u32>, ProcessError> {
        backend::tree_pids(&self.shared.tree).map_err(|reason| ProcessError::Supervision {
            operation: "enumerating the managed process tree",
            reason,
        })
    }

    /// Ask the run to finish, then force its tree if it outlasts `timeout`.
    ///
    /// On return with `Ok`, the managed tree is gone.
    pub fn stop(&self, timeout: Duration) -> Result<StopReport, ProcessError> {
        if !self.is_running() {
            // The run's own process is gone — its descendants may not be. A
            // launcher that hands a child off and exits leaves exactly that, and
            // the run is not over while any of it is alive: "the parent exited"
            // is not evidence that its children did (`docs/DEVELOPMENT.md` §6).
            // So this is a teardown, not a no-op, and only the force path can
            // confirm it.
            return self.force_stop();
        }

        // Best effort: a run that can finish on its own terms should be allowed
        // to (D-007). The force path below covers every case where it cannot.
        let graceful_delivered = backend::request_graceful_stop(self.shared.pid);
        if let Some(exit) = self.wait_for_exit(timeout) {
            self.confirm_tree_gone()?;
            return Ok(StopReport {
                outcome: StopOutcome::Exited,
                exit,
                graceful_delivered,
            });
        }

        let forced = self.force_stop()?;
        Ok(StopReport {
            graceful_delivered,
            ..forced
        })
    }

    /// Terminate the managed tree without a graceful request.
    ///
    /// On return with `Ok`, the managed tree is gone. Calling it on a run that
    /// has completely ended — own process and descendants — is a no-op that
    /// reports [`StopOutcome::AlreadyExited`].
    pub fn force_stop(&self) -> Result<StopReport, ProcessError> {
        let already = self.exit_status();
        // An empty tree is the supervisor's definition of "already gone": the
        // run's own process is assigned to the job for as long as it exists, so
        // whatever remains here is what a stop still has to reclaim. Reading it
        // before the termination is what lets the report say which of the two
        // happened instead of always claiming the run had already finished.
        let remaining = self.tree_pids()?;
        backend::terminate_tree(&self.shared.tree).map_err(|reason| ProcessError::Supervision {
            operation: "terminating the managed process tree",
            reason,
        })?;

        let exit = match already.or_else(|| self.wait_for_exit(TREE_GONE_TIMEOUT)) {
            Some(exit) => exit,
            None => {
                return Err(ProcessError::Supervision {
                    operation: "confirming the managed process exited",
                    reason: format!(
                        "no exit status for pid {} after terminating its tree",
                        self.shared.pid
                    ),
                });
            }
        };
        self.confirm_tree_gone()?;

        Ok(StopReport {
            outcome: if remaining.is_empty() {
                StopOutcome::AlreadyExited
            } else {
                StopOutcome::Forced
            },
            exit,
            graceful_delivered: false,
        })
    }

    /// Stop this run, then start `spec` as its replacement.
    ///
    /// The stop is a barrier, so the replacement can never overlap the previous
    /// run. On `Err`, the previous run is already gone and no new run exists.
    pub fn restart(&mut self, spec: ProcessSpec, timeout: Duration) -> Result<(), ProcessError> {
        self.stop(timeout)?;
        let replacement = ManagedProcess::spawn(spec)?;
        *self = replacement;
        Ok(())
    }

    /// Barrier behind `stop`/`force_stop`: nothing may remain assigned to the
    /// run before a stop reports success.
    fn confirm_tree_gone(&self) -> Result<(), ProcessError> {
        let deadline = Instant::now() + TREE_GONE_TIMEOUT;
        loop {
            let remaining = self.tree_pids()?;
            if remaining.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(ProcessError::Supervision {
                    operation: "confirming the managed process tree is gone",
                    reason: format!("processes still assigned to the run: {remaining:?}"),
                });
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        // Ownership is RAII: a handle that goes away must not leave a process
        // the Hub can no longer account for. Closing the job handle would end
        // the run either way (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`); stopping it
        // explicitly is what makes the teardown complete before the handle is
        // gone. A run that has completely ended takes the no-op path.
        let _ = self.force_stop();
    }
}

/// Lock a mutex while tolerating poisoning: a panic on one path must not turn
/// every later stop into a second panic.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// Turn a platform exit status into the supervisor's own record.
fn observe_exit(status: std::process::ExitStatus) -> ExitStatus {
    let code = status.code().map(|code| code as u32);
    ExitStatus { code }
}

/// Watch one run to its end in a thread of its own, so a tray-resident Hub holds
/// no polling timer per session (D-009).
fn watch_exit(shared: &Arc<Shared>) -> std::io::Result<()> {
    let shared = Arc::clone(shared);
    std::thread::Builder::new()
        .name(format!("lch-process-{}", shared.pid))
        .spawn(move || {
            watch_exit_handle(&shared.child, &shared.exit);
            // Woken only for an exit this watcher recorded: a caller that was
            // answered by a timeout re-reads the process object itself
            // (`ManagedProcess::exit_status`).
            if lock(&shared.exit).is_some() {
                shared.exited.notify_all();
            }
        })
        .map(|_watcher| ())
}

/// Block until one run's process object is signalled, then record its exit.
///
/// Shared with the standalone-window run (#66), which watches its process the
/// same way and then keeps watching the *tree*: the wait for a signalled
/// process object costs no CPU while the application runs, whatever ends up
/// happening afterwards (D-009).
pub(crate) fn watch_exit_handle(child: &Mutex<Child>, exit: &Mutex<Option<ExitStatus>>) {
    let handle = {
        let child = lock(child);
        backend::process_handle(&child)
    };
    // The wait must not hold the child lock: `is_running`, `tree_pids` and the
    // stop path stay answerable while a run is alive.
    if backend::wait_for_handle(handle).is_err() {
        // Waiting on our own child's process object cannot fail in practice. If
        // it somehow does, fall back to a bounded poll rather than one that
        // never ends: an unrecorded exit is recoverable (`exit_status` reads
        // the process object itself), a spinning thread is not.
        let _ = lock(child).try_wait();
    }

    // Read the status now that the object has signalled. `None` means it could
    // not be read, which keeps the watcher from recording an exit that never
    // happened.
    let status = poll_child(child, Instant::now() + REAP_TIMEOUT);
    if let Some(status) = status {
        *lock(exit) = Some(observe_exit(status));
    }
}

/// Poll a run's process object until it reports something or `deadline` passes.
fn poll_child(child: &Mutex<Child>, deadline: Instant) -> Option<std::process::ExitStatus> {
    loop {
        if let Ok(Some(status)) = lock(child).try_wait() {
            return Some(status);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Stdio;
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A run that stays alive far longer than any test needs, and that writes no
    /// noise into the test log. `ping` is a real child of `cmd`, so the run has
    /// a tree rather than a single process.
    fn long_running() -> ProcessSpec {
        ProcessSpec::new("cmd.exe", std::env::temp_dir()).with_args(vec![
            "/c".to_owned(),
            "ping -n 60 127.0.0.1 > NUL".to_owned(),
        ])
    }

    /// Stop/restart timeout for tests: long enough for a graceful request to land,
    /// short enough to keep the suite quick.
    const STOP_TIMEOUT: Duration = Duration::from_secs(3);

    /// Start a long-running run, failing the test if it cannot start.
    fn start() -> ManagedProcess {
        let spawn = ManagedProcess::spawn(long_running());
        spawn.expect("the run starts")
    }

    /// Restart `run` with another long run, failing the test if it cannot.
    fn restart(run: &mut ManagedProcess) {
        let restarted = run.restart(long_running(), STOP_TIMEOUT);
        restarted.expect("the run restarts");
    }

    /// A run that ends on its own while leaving a descendant behind: the shell
    /// hands a child off and exits, so the run's own process is gone while part
    /// of the run is not.
    fn launcher() -> ProcessSpec {
        // `start` returns as soon as the child exists and the child inherits the
        // shell's job object, which is the shape a launcher script has when it
        // detaches a service and ends.
        let args = vec!["/c".to_owned(), "start /b ping -n 60 127.0.0.1".to_owned()];
        ProcessSpec::new("cmd.exe", std::env::temp_dir()).with_args(args)
    }

    /// A private directory for the batch-file handshake used by the fast
    /// launcher regression. The child uses the PowerShell executable shipped
    /// with Windows, so no test helper binary needs to be built or installed.
    struct ProcessFixtureDir(PathBuf);

    impl ProcessFixtureDir {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("the system clock is after the Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "lch-process-startup-{}-{nonce}",
                std::process::id()
            ));
            fs::create_dir(&path).expect("the process fixture directory is created");
            ProcessFixtureDir(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for ProcessFixtureDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// The launcher signals when its detached child is actually alive, then
    /// exits while that child keeps running. This is the startup window in
    /// which a process created before job assignment could escape the run.
    fn coordinated_launcher(directory: &std::path::Path) -> (ProcessSpec, PathBuf) {
        let child = directory.join("child.cmd");
        let child_script = directory.join("child.ps1");
        let launcher = directory.join("launcher.cmd");
        let ready = directory.join("child-ready.txt");
        fs::write(
            &child_script,
            "[System.IO.File]::WriteAllText($args[0], [string]$PID)\nStart-Sleep -Seconds 60\n",
        )
        .expect("the child handshake script is written");
        fs::write(
            child,
            "@echo off\r\nstart \"\" /b powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"%~dp0child.ps1\" \"%~dp0child-ready.txt\"\r\n",
        )
        .expect("the child launcher script is written");
        fs::write(
            &launcher,
            "@echo off\r\ncall \"%~dp0child.cmd\"\r\nexit /b 0\r\n",
        )
        .expect("the parent launcher script is written");

        let spec = ProcessSpec::new("cmd.exe", directory).with_args(vec![
            "/d".to_owned(),
            "/c".to_owned(),
            launcher.to_string_lossy().into_owned(),
        ]);
        (spec, ready)
    }

    /// Stop a helper process named by the readiness handshake. This also
    /// cleans up the escaped child when the regression deliberately fails on
    /// the old post-spawn assignment path.
    fn stop_handshake_process(ready: &std::path::Path) {
        let Ok(pid) = fs::read_to_string(ready) else {
            return;
        };
        let Ok(pid) = pid.trim().parse::<u32>() else {
            return;
        };
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    struct HandshakeProcessGuard(PathBuf);

    impl Drop for HandshakeProcessGuard {
        fn drop(&mut self) {
            stop_handshake_process(&self.0);
        }
    }

    struct UnrelatedProcessGuard(Child);

    impl Drop for UnrelatedProcessGuard {
        fn drop(&mut self) {
            kill_tree(&mut self.0);
        }
    }

    /// Coordinate a real child at the create/assign boundary. This hook is
    /// thread-local so parallel process tests cannot delay one another.
    fn before_job_assignment(hook: impl FnOnce(u32) + 'static) {
        BEFORE_JOB_ASSIGNMENT_TEST_HOOK.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(hook));
        });
    }

    /// A process with the same executable name that the supervisor never sees.
    fn start_unrelated() -> Child {
        std::process::Command::new("cmd.exe")
            .args(["/c", "ping -n 60 127.0.0.1 > NUL"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the unrelated process starts")
    }

    /// Whether a process the supervisor never started is still running.
    fn still_running(child: &mut Child) -> bool {
        let state = child.try_wait().expect("the process is waitable");
        state.is_none()
    }

    /// Best-effort teardown for processes a test started outside the supervisor,
    /// so a failing test cannot leave a pinger behind.
    fn kill_tree(child: &mut Child) {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = child.wait();
    }

    /// The run's tree, which is the thing every safety assertion here is about:
    /// what a stop promises is that this comes back empty.
    fn managed_tree(run: &ManagedProcess) -> Vec<u32> {
        run.tree_pids().expect("the tree is observable")
    }

    /// Poll the run's tree until it holds at least `count` processes: the shell
    /// starts its own child a moment after the run itself appears.
    fn tree_with_at_least(run: &ManagedProcess, count: usize) -> Vec<u32> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let pids = managed_tree(run);
            if pids.len() >= count || Instant::now() >= deadline {
                return pids;
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    #[test]
    fn handle_is_shareable_across_threads() {
        // The contract the session registry relies on: one handle can be shared
        // between IPC, tray and watcher threads without another lock around it.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ManagedProcess>();
    }

    #[test]
    fn spawn_reports_a_pid_and_owns_the_run() {
        let run = start();
        assert_ne!(run.pid(), 0);
        assert!(run.is_running());
        assert!(run.exit_status().is_none());

        let tree = tree_with_at_least(&run, 1);
        assert!(
            tree.contains(&run.pid()),
            "the run must own its own process, saw {tree:?}"
        );

        // The timeout contract: a run that is still alive yields no status.
        assert!(run.wait_for_exit(Duration::from_millis(50)).is_none());

        run.force_stop().expect("cleanup");
    }

    #[test]
    fn missing_program_is_an_actionable_error() {
        let missing = "C:/definitely/not/here/lch-t03/missing.exe";
        let spec = ProcessSpec::new(missing, std::env::temp_dir());
        let spawn = ManagedProcess::spawn(spec);
        let error = spawn.expect_err("a missing program cannot start");
        let message = error.to_string();
        assert!(message.contains("missing.exe"), "{message}");
        assert!(message.contains("lch-t03"), "{message}");
    }

    #[test]
    fn natural_exit_reports_the_exit_code() {
        // `cmd /c exit 3` is a literal in the Windows shell: the run must report
        // exactly 3, observed without the supervisor asking it to stop.
        let spec = ProcessSpec::new("cmd.exe", std::env::temp_dir())
            .with_args(vec!["/c".to_owned(), "exit 3".to_owned()]);
        let run = ManagedProcess::spawn(spec).expect("the run starts");

        let exit = run
            .wait_for_exit(Duration::from_secs(30))
            .expect("a run that exits on its own is observed");

        assert_eq!(exit.code, Some(3));
        assert_eq!(run.exit_status(), Some(exit));
        assert!(!run.is_running());
    }

    #[test]
    fn stop_ends_a_live_run_and_leaves_nothing_in_the_tree() {
        let run = start();
        let tree_before = tree_with_at_least(&run, 1);
        assert!(!tree_before.is_empty());

        let report = run.stop(STOP_TIMEOUT).expect("the run stops");

        assert_ne!(report.outcome, StopOutcome::AlreadyExited);
        assert!(
            report.exit.code.is_some(),
            "the exit code must be observable, saw {report:?}"
        );
        // Whether the request can be delivered depends on the console the Hub
        // itself runs with, which a CI runner may not have — in that case the
        // run is force-stopped after the timeout. When it *is* delivered, the run
        // must end without the force path: a delivered request followed by a
        // force kill would mean the CTRL_BREAK never reached the run.
        if report.graceful_delivered {
            assert_eq!(report.outcome, StopOutcome::Exited, "saw {report:?}");
        }

        assert!(!run.is_running());
        assert!(
            managed_tree(&run).is_empty(),
            "the managed tree must be gone after a stop"
        );
        for pid in tree_before {
            assert!(
                !super::win::is_process_alive(pid),
                "pid {pid} survived the stop of its run"
            );
        }
    }

    /// Re-entered as a child process by the graceful-stop regression. The
    /// readiness line is emitted only after the real CTRL_BREAK handler exists.
    #[test]
    #[ignore = "child-process fixture for the graceful-stop regression"]
    fn graceful_stop_ready_fixture() {
        use std::io::Write;
        use windows_sys::Win32::System::Console::{SetConsoleCtrlHandler, CTRL_BREAK_EVENT};

        unsafe extern "system" fn stop_on_break(event: u32) -> i32 {
            if event == CTRL_BREAK_EVENT {
                std::process::exit(0);
            }
            0
        }

        assert_ne!(unsafe { SetConsoleCtrlHandler(Some(stop_on_break), 1) }, 0);
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(b"issue82-ready\n").unwrap();
        stdout.flush().unwrap();
        drop(stdout);
        loop {
            thread::park();
        }
    }

    #[test]
    fn a_graceful_stop_still_reaches_a_run_whose_console_has_no_window() {
        // The stop ladder's first rung is a `CTRL_BREAK` aimed at the run's
        // process group (D-007), and a `CTRL_BREAK` only travels inside one
        // console. What every run is given is therefore a console — one it
        // shares with the Hub where the Hub has one, and a windowless one of its
        // own where it does not (D-035) — and this pins the half of that trade
        // the flag could have broken. `CREATE_NO_WINDOW` applied unconditionally
        // is what this test refuses: it makes `GenerateConsoleCtrlEvent` report
        // a request it raised only in the Hub's own console, with the run on
        // another one, which the report below reads as `graceful_delivered:
        // true` for a stop that then had to be forced.
        // Wait for the actual control handler before stopping the run. An exit during
        // Windows initialization (for example 0xC0000142) is not evidence that a
        // graceful request reached a live run.
        let hub_has_console = !super::win::console_members().is_empty();
        let spec = ProcessSpec::new(std::env::current_exe().unwrap(), std::env::temp_dir())
            .with_args(vec![
                "process::tests::graceful_stop_ready_fixture".to_owned(),
                "--exact".to_owned(),
                "--ignored".to_owned(),
                "--nocapture".to_owned(),
                "--test-threads=1".to_owned(),
            ])
            .with_output(OutputMode::Capture);
        let run = ManagedProcess::spawn(spec).expect("the run starts");
        let output = run.take_output().expect("the run exposes its output");
        let stdout = output.stdout.expect("the fixture stdout is captured");
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
        thread::spawn(move || {
            use std::io::{BufRead, BufReader};
            let mut reader = BufReader::new(stdout);
            let ready = loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break Ok(false),
                    Ok(_) if line.trim_end().ends_with("issue82-ready") => break Ok(true),
                    Ok(_) => {}
                    Err(error) => break Err(error),
                }
            };
            let _ = ready_tx.send(ready);
        });
        let ready = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the fixture answers within the startup deadline")
            .expect("the fixture readiness line can be read");
        assert!(
            ready,
            "the control handler must be ready before graceful stop is measured; exit={:?}",
            run.exit_status()
        );
        assert!(run.is_running(), "the ready fixture is still alive");

        // Which console the run ended up on is the environment's answer, not
        // this test's: it shares the Hub's when the Hub had one to inherit at
        // the moment of the spawn, and has a windowless one of its own when it
        // did not. Another test in this binary changes that answer under this
        // one — `crate::window`'s console lookup attaches and detaches this
        // process's console for the length of a call, and it does so once per
        // poll while it waits for a standalone run's window — so the strong half
        // is asserted only where the run is observably on this console.
        let shares_hub_console = super::win::console_members().contains(&run.pid());

        let report = run.stop(STOP_TIMEOUT).expect("the run stops");
        eprintln!(
            "hub_has_console={hub_has_console}; shares_hub_console={shares_hub_console}; \
             stop_report={report:?}"
        );

        if report.graceful_delivered {
            assert_eq!(
                report.outcome,
                StopOutcome::Exited,
                "a request reported as delivered must end the run without the \
                 force path: {report:?}"
            );
            assert_eq!(
                report.exit.code,
                Some(0),
                "the CTRL_BREAK handler exits with 0"
            );
        }
        if shares_hub_console {
            assert!(
                report.graceful_delivered,
                "the run is on the Hub's console, so the request had somewhere \
                 to arrive: {report:?}"
            );
            assert_eq!(report.outcome, StopOutcome::Exited, "saw {report:?}");
        }
        assert!(!run.is_running());
        assert!(managed_tree(&run).is_empty());
    }

    #[test]
    fn stop_reclaims_a_descendant_left_behind_by_an_exited_run() {
        // A run whose own process ends can still have live descendants, and a
        // stop that reported success here would let a restart start a second
        // instance of a service that never stopped (`docs/DEVELOPMENT.md` §6:
        // a parent exiting is not evidence that its children ended).
        let spawn = ManagedProcess::spawn(launcher());
        let run = spawn.expect("the run starts");
        let exited = run.wait_for_exit(Duration::from_secs(30));
        exited.expect("the launcher shell exits on its own");

        let leftover = tree_with_at_least(&run, 1);
        assert!(
            !leftover.is_empty(),
            "the run should have left a descendant behind"
        );

        let report = run.stop(STOP_TIMEOUT).expect("the run stops");
        assert_eq!(report.outcome, StopOutcome::Forced, "saw {report:?}");
        assert!(
            managed_tree(&run).is_empty(),
            "the managed tree must be gone after a stop"
        );
        for pid in leftover {
            assert!(
                !super::win::is_process_alive(pid),
                "pid {pid} survived the stop of its run"
            );
        }
    }

    #[test]
    fn an_early_descendant_stays_in_the_run_after_its_launcher_exits() {
        let fixture = ProcessFixtureDir::new();
        let (spec, ready) = coordinated_launcher(fixture.path());
        let _handshake_process = HandshakeProcessGuard(ready.clone());
        let mut unrelated = UnrelatedProcessGuard(start_unrelated());
        let ready_before_assignment = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let ready_before_assignment_for_hook = Arc::clone(&ready_before_assignment);
        let ready_for_hook = ready.clone();
        before_job_assignment(move |_pid| {
            let deadline = Instant::now() + Duration::from_secs(2);
            while !ready_for_hook.exists() && Instant::now() < deadline {
                thread::sleep(POLL_INTERVAL);
            }
            ready_before_assignment_for_hook.store(
                ready_for_hook.exists(),
                std::sync::atomic::Ordering::Release,
            );
        });

        let run = match ManagedProcess::spawn(spec) {
            Ok(run) => run,
            Err(error) => panic!("the coordinated launcher starts: {error}"),
        };
        assert!(
            !ready_before_assignment.load(std::sync::atomic::Ordering::Acquire),
            "the service must not create a child before its process belongs to the managed job"
        );

        let handshake_deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() && Instant::now() < handshake_deadline {
            thread::sleep(POLL_INTERVAL);
        }
        assert!(
            ready.exists(),
            "the launcher child must publish its readiness handshake"
        );

        let exit = run
            .wait_for_exit(Duration::from_secs(10))
            .expect("the launcher exits after starting its child");
        assert!(exit.code.is_some(), "the launcher's exit is observable");

        let remaining = managed_tree(&run);
        assert!(
            !remaining.is_empty(),
            "the live child created during startup must remain in the managed job after its parent exits"
        );
        assert!(
            !remaining.contains(&run.pid()),
            "the launcher itself should have exited before the child is checked"
        );

        run.force_stop().expect("the remaining child is stopped");
        assert!(
            managed_tree(&run).is_empty(),
            "stopping the run removes its early child"
        );
        for pid in remaining {
            assert!(
                !super::win::is_process_alive(pid),
                "early child pid {pid} survived the managed stop"
            );
        }
        assert!(
            still_running(&mut unrelated.0),
            "stopping the early child must leave the unrelated sentinel running"
        );
    }

    #[test]
    fn startup_failures_clean_up_the_suspended_process() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let failure_points = [
            super::tree::StartFailurePointForTest::JobCreation,
            super::tree::StartFailurePointForTest::JobAssignment,
            super::tree::StartFailurePointForTest::Resume,
        ];

        for point in failure_points {
            let created_pid = Arc::new(AtomicU32::new(0));
            let created_pid_for_hook = Arc::clone(&created_pid);
            before_job_assignment(move |pid| {
                created_pid_for_hook.store(pid, Ordering::Release);
                super::tree::fail_start_step_for_test(point);
            });

            let error = ManagedProcess::spawn(long_running())
                .expect_err("a startup ownership failure refuses the run");
            let message = error.to_string();
            assert!(
                message.contains("cmd.exe") && message.contains("working directory"),
                "the startup error includes its command and working directory: {message}"
            );

            let pid = created_pid.load(Ordering::Acquire);
            assert_ne!(pid, 0, "the test observed the created process id");
            assert!(
                !super::win::is_process_alive(pid),
                "the process survived injected startup failure at {point:?}"
            );
        }
    }

    #[test]
    fn force_stop_removes_the_managed_tree_but_not_an_unrelated_process() {
        let run = start();
        let mut unrelated = start_unrelated();

        let managed = tree_with_at_least(&run, 2);
        assert!(
            managed.len() >= 2,
            "the run should own its shell and the shell's child, saw {managed:?}"
        );

        run.force_stop().expect("the run is force-stopped");

        assert!(
            managed_tree(&run).is_empty(),
            "the managed tree must be gone after a force stop"
        );
        for pid in managed {
            assert!(
                !super::win::is_process_alive(pid),
                "pid {pid} of the managed tree survived the force stop"
            );
        }
        assert!(
            still_running(&mut unrelated),
            "a process the supervisor never owned must survive, even with the same \
             executable name"
        );

        kill_tree(&mut unrelated);
    }

    #[test]
    fn repeated_stop_is_safe() {
        let run = start();
        let mut unrelated = start_unrelated();

        let first = run.stop(STOP_TIMEOUT).expect("the first stop");
        assert_ne!(first.outcome, StopOutcome::AlreadyExited);

        let second = run.stop(STOP_TIMEOUT).expect("the second stop");
        assert_eq!(second.outcome, StopOutcome::AlreadyExited);
        assert_eq!(second.exit, first.exit, "the exit must not change");

        assert!(
            still_running(&mut unrelated),
            "repeating a stop must not signal anything else"
        );

        kill_tree(&mut unrelated);
    }

    #[test]
    fn concurrent_stops_do_not_race() {
        let run = start();

        let reports = thread::scope(|scope| {
            let first = scope.spawn(|| run.stop(STOP_TIMEOUT));
            let second = scope.spawn(|| run.stop(STOP_TIMEOUT));
            [first.join(), second.join()]
        });

        let reports: Vec<StopReport> = reports
            .into_iter()
            .map(|joined| joined.expect("no panic under a concurrent stop"))
            .map(|stop| stop.expect("both stops succeed"))
            .collect();

        assert!(
            reports
                .iter()
                .any(|report| report.outcome != StopOutcome::AlreadyExited),
            "at least one of the concurrent stops ended the run, saw {reports:?}"
        );
        assert!(!run.is_running());
        assert!(managed_tree(&run).is_empty());
    }

    #[test]
    fn restart_leaves_exactly_one_run_alive() {
        let mut run = start();
        let mut retired = Vec::new();

        for _ in 0..3 {
            // Retain the original process and job objects, not just their PID.
            // Windows may recycle a retired PID for another parallel test;
            // opening that number later cannot prove this run survived.
            retired.push(Arc::clone(&run.shared));
            restart(&mut run);
        }

        assert!(run.is_running());
        let tree = tree_with_at_least(&run, 1);
        assert!(
            tree.contains(&run.pid()),
            "the replacement run owns its own process"
        );
        for stale in retired {
            assert!(
                lock(&stale.child)
                    .try_wait()
                    .expect("the original process object can be observed")
                    .is_some(),
                "restart left the original run alive: {}",
                stale.pid
            );
            let remaining = backend::tree_pids(&stale.tree).expect("the original job is readable");
            assert!(
                remaining.is_empty(),
                "restart left descendants in the original job: {remaining:?}"
            );
        }

        run.force_stop().expect("cleanup");
    }

    /// A process this test started, ended on drop so a failure cannot leave it.
    #[cfg(windows)]
    struct ConfirmEndSleeper(std::process::Child);

    #[cfg(windows)]
    impl ConfirmEndSleeper {
        fn spawn() -> Self {
            use std::os::windows::process::CommandExt;
            let child = std::process::Command::new("cmd.exe")
                .args(["/c", "ping -n 60 127.0.0.1 > NUL"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x0800_0000)
                .spawn()
                .expect("the test process starts");
            ConfirmEndSleeper(child)
        }

        fn pid(&self) -> u32 {
            self.0.id()
        }

        fn identity(&self) -> ProcessIdentity {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            loop {
                if let Some(created) = creation_time_of(self.pid()) {
                    let identity = ProcessIdentity::recorded(self.pid(), created);
                    if identity.matches() {
                        return identity;
                    }
                }
                if std::time::Instant::now() >= deadline {
                    panic!("creation time of {} was not readable", self.pid());
                }
                thread::sleep(std::time::Duration::from_millis(20));
            }
        }
    }

    #[cfg(windows)]
    impl Drop for ConfirmEndSleeper {
        fn drop(&mut self) {
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &self.0.id().to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = self.0.wait();
        }
    }

    #[cfg(windows)]
    fn descendant_identities(root: u32) -> Vec<ProcessIdentity> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            let found: Vec<_> = descendants(root)
                .into_iter()
                .filter_map(|pid| {
                    creation_time_of(pid).map(|created| ProcessIdentity::recorded(pid, created))
                })
                .collect();
            if !found.is_empty() || std::time::Instant::now() >= deadline {
                return found;
            }
            thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[cfg(windows)]
    fn wait_until(mut done: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if done() {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[cfg(windows)]
    fn original_is_gone(identity: ProcessIdentity) -> bool {
        !identity.matches() || !backend::is_process_alive(identity.pid())
    }

    #[cfg(windows)]
    fn still_that_process(identity: ProcessIdentity) -> bool {
        identity.matches() && backend::is_process_alive(identity.pid())
    }

    /// The process that is ended is the one just verified, and its tree.
    /// Another process this test started, with the same executable name, stays.
    #[cfg(windows)]
    #[test]
    fn confirm_end_stops_only_the_verified_tree() {
        let target = ConfirmEndSleeper::spawn();
        let other = ConfirmEndSleeper::spawn();
        let target_identity = target.identity();
        let other_identity = other.identity();
        let members = descendant_identities(target.pid());
        assert!(
            !members.is_empty(),
            "the sleeper is a tree, not a single process"
        );
        assert!(
            members.iter().all(|member| member.pid() != other.pid()),
            "the other process is not in the target tree"
        );

        assert_eq!(
            super::end_confirmed_tree(target_identity),
            super::ConfirmedEnd::Ended
        );
        assert!(
            wait_until(|| original_is_gone(target_identity)),
            "the verified process is gone"
        );
        for member in &members {
            assert!(
                wait_until(|| original_is_gone(*member)),
                "a verified descendant is still the live process {}",
                member.pid()
            );
        }
        assert!(
            still_that_process(other_identity),
            "a different process was ended"
        );
    }

    /// A creation time that is not the live process must not end that process.
    #[cfg(windows)]
    #[test]
    fn confirm_end_refuses_a_different_creation_time() {
        let sleeper = ConfirmEndSleeper::spawn();
        let identity = sleeper.identity();
        let reused =
            ProcessIdentity::for_test(identity.pid(), identity.created_at().wrapping_add(1));
        assert!(!reused.matches());

        assert_eq!(
            super::end_confirmed_tree(reused),
            super::ConfirmedEnd::NothingEnded
        );
        assert!(
            still_that_process(identity),
            "the live process was ended on a creation time that was not its own"
        );
    }

    /// A process that has already exited is not a reason to signal its pid.
    #[cfg(windows)]
    #[test]
    fn confirm_end_refuses_an_already_exited_process() {
        let mut sleeper = ConfirmEndSleeper::spawn();
        let identity = sleeper.identity();
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &sleeper.pid().to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = sleeper.0.wait();
        assert!(
            wait_until(|| !backend::is_process_alive(identity.pid())),
            "the test process is still running"
        );
        assert!(
            identity.matches(),
            "the exited process object is still the one that was started"
        );

        assert_eq!(
            super::end_confirmed_tree(identity),
            super::ConfirmedEnd::NothingEnded
        );
    }
}
