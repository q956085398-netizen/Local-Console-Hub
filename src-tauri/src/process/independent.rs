//! A run the Hub starts but does not own (#66, spec #59 decision 12).
//!
//! A standalone-window application is a third-party program the user runs
//! *through* the Hub: Hub starts it, brings its window back when asked, and —
//! unless the user turned lifecycle management on — leaves it alone when the
//! Hub itself goes away. That is one sentence of product behaviour and three
//! separate properties underneath it:
//!
//! 1. **The tree does not die with the Hub.** The run is still owned by a job
//!    object — that is what makes its descendants enumerable and a stop exact —
//!    but the job is created without `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
//!    ([`crate::process::tree::Job::create_independent`]), so closing the handle
//!    when the Hub exits kills nothing. Independence is a property of the
//!    object from the moment it exists, not a promise made about it afterwards.
//! 2. **The run ends when the tree does, not when the launcher does.** An
//!    application that starts a real service and exits — a `.bat` launcher, a
//!    bootstrapper — leaves a live child behind, exactly as a terminal shell
//!    does (D-028). Reporting `Exited` there would make the next click start a
//!    second copy of something already running, so [`IndependentProcess`] counts
//!    the run as over only once its own process has exited *and* nothing is left
//!    in the tree.
//! 3. **A stop is still exact.** `stop` asks the window to close (the gesture a
//!    user performs), falls back to a console interrupt for a run with no
//!    window, waits, and then terminates the run's tree — never a pid, never a
//!    name (spec §15).
//!
//! What this type deliberately does **not** do is clean up in `Drop`. Every
//! other run in this crate is RAII because it is the Hub's; this one is not, and
//! a `Drop` that stopped it would silently turn "leave my application running"
//! into "close everything when the Hub exits".

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::{
    backend, lock, observe_exit, watch_exit_handle, ExitStatus, OutputMode, ProcessError,
    ProcessIdentity, ProcessSpec, StopOutcome, StopReport, TREE_GONE_TIMEOUT,
};
use crate::window::{self, TopLevelWindow};

/// How often a run whose own process has already exited is checked for the end
/// of its tree (D-009: a bounded, per-run reading, never a scan of the system).
///
/// This wait only happens for the launcher shape — an application that hands a
/// child off and exits itself — and only until the tree it left behind ends.
const TREE_TICK: Duration = Duration::from_millis(500);

/// Longest a caller waits for a standalone application to put a window on
/// screen.
///
/// Reaching "no window yet" immediately would be true and useless: an
/// application takes a moment to build its window, and the user's click means
/// "open it", not "tell me about the millisecond you asked".
pub const WINDOW_WAIT: Duration = Duration::from_secs(5);

/// How many tree readings in a row may fail before the watcher gives up.
///
/// A job object the Hub holds cannot normally fail to answer, so this is the
/// bound on the "cannot happen" path: enough readings (30 s at [`TREE_TICK`])
/// that a transient failure does not end the watch, few enough that a real one
/// does not leave a thread polling forever (D-009).
const MAX_UNREADABLE_TREE_READINGS: u32 = 60;

/// State shared between the handle and its watcher thread.
#[derive(Debug)]
struct Shared {
    /// The run's own process. Holds a process handle for the run's lifetime,
    /// which is also what makes an exit observable without polling.
    child: Mutex<Child>,
    /// The job object that owns the run's tree. Created **without**
    /// kill-on-close: it is here to know the tree, not to end it.
    tree: backend::TreeHandle,
    /// What tells this run apart from a later process Windows gives the same id.
    identity: ProcessIdentity,
    /// The run's own process's exit, recorded by the watcher once it has been
    /// observed. `Some` before the run has ended is normal for a launcher.
    own_exit: Mutex<Option<ExitStatus>>,
    /// The run's end: own process gone *and* tree empty.
    ended: Mutex<Option<ExitStatus>>,
    ended_signal: Condvar,
}

impl Shared {
    /// Whether the run has ended, reading the process and then the tree.
    fn ended_now(&self) -> Option<ExitStatus> {
        // Drop the read guard before polling: the unrecorded-exit arm writes
        // this same mutex, and a match scrutinee's temporary lives through its
        // arms. Keeping that guard would make the fallback deadlock itself.
        let recorded = *lock(&self.own_exit);
        let own = match recorded {
            Some(exit) => Some(exit),
            None => match lock(&self.child).try_wait() {
                Ok(Some(status)) => {
                    let exit = observe_exit(status);
                    *lock(&self.own_exit) = Some(exit);
                    Some(exit)
                }
                _ => None,
            },
        }?;

        // The own process is gone; the run is over only when nothing it started
        // is left. An unreadable tree is not an empty one: saying "ended" on a
        // failed query is exactly the claim this check exists to prevent.
        match backend::tree_pids(&self.tree) {
            Ok(pids) if pids.is_empty() => Some(own),
            _ => None,
        }
    }
}

/// One run of a standalone-window application.
///
/// `Send + Sync` and observable through `&self`, like [`super::ManagedProcess`],
/// so a session registry can share one handle across IPC, tray and watcher
/// threads.
#[derive(Debug)]
pub struct IndependentProcess {
    shared: Arc<Shared>,
}

impl IndependentProcess {
    /// Start a run and put its process tree under a job that does not end it.
    ///
    /// The run is created with a console of its own and is never started
    /// suspended *into* the Hub's supervision: see [`super::win::prepare_windowed`].
    /// Output capture is refused here rather than ignored — the application
    /// keeps its console, so a pipe would be the Hub taking it away while the
    /// configuration claims it is kept.
    pub fn spawn(spec: ProcessSpec) -> Result<Self, ProcessError> {
        backend::require_backend("starting a standalone-window process")?;
        debug_assert_eq!(
            spec.output,
            OutputMode::Inherit,
            "a standalone run keeps its own console; capture is resolved away before it starts"
        );

        let mut command = Command::new(&spec.program);
        command.args(&spec.args).current_dir(&spec.cwd);
        // Explicit rather than inherited: the application's console is part of
        // what `display: window` means, and a GUI build of the Hub has none to
        // hand down.
        command.stdin(Stdio::null());
        let mut child =
            backend::spawn(&mut command, backend::prepare_windowed).map_err(|source| {
                ProcessError::Spawn {
                    program: spec.program.clone(),
                    cwd: spec.cwd.clone(),
                    source,
                }
            })?;

        let identity = ProcessIdentity::of_child(&child);
        let tree = match backend::attach_independent(&child) {
            Ok(tree) => tree,
            Err(reason) => {
                // The process is running but nothing owns its tree, so it must
                // not be left running: a run this layer cannot account for is
                // not one it may report on (`docs/MVP_IMPLEMENTATION_SPEC.md`
                // §7).
                let _ = child.kill();
                let _ = child.wait();
                return Err(ProcessError::Supervision {
                    operation: "putting the standalone run's process tree under a job object",
                    reason: format!(
                        "process `{}` in working directory `{}`: {reason}",
                        spec.program.display(),
                        spec.cwd.display()
                    ),
                });
            }
        };

        let shared = Arc::new(Shared {
            child: Mutex::new(child),
            tree,
            identity,
            own_exit: Mutex::new(None),
            ended: Mutex::new(None),
            ended_signal: Condvar::new(),
        });
        shared.watch();
        Ok(IndependentProcess { shared })
    }

    /// The run's own process id.
    pub fn pid(&self) -> u32 {
        self.shared.identity.pid()
    }

    /// What tells this run apart from a later process with the same id (#66).
    pub fn identity(&self) -> ProcessIdentity {
        self.shared.identity
    }

    /// Whether anything of the run is still alive.
    pub fn is_running(&self) -> bool {
        self.exit_status().is_none()
    }

    /// The run's exit, once it has ended — own process gone and tree empty.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        if let Some(exit) = *lock(&self.shared.ended) {
            return Some(exit);
        }
        let ended = self.shared.ended_now()?;
        *lock(&self.shared.ended) = Some(ended);
        Some(ended)
    }

    /// Wait up to `timeout` for the run to end.
    pub fn wait_for_exit(&self, timeout: Duration) -> Option<ExitStatus> {
        let ended = lock(&self.shared.ended);
        if let Some(exit) = *ended {
            return Some(exit);
        }
        let (ended, _) = self
            .shared
            .ended_signal
            .wait_timeout_while(ended, timeout, |ended| ended.is_none())
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(exit) = *ended {
            return Some(exit);
        }
        drop(ended);
        self.exit_status()
    }

    /// Ids of every process still assigned to the run.
    ///
    /// Job membership is the ownership boundary, so this list cannot contain a
    /// reused pid: a process is here because it was started by this run, not
    /// because it happens to carry a number the run once used.
    pub fn tree_pids(&self) -> Result<Vec<u32>, ProcessError> {
        backend::tree_pids(&self.shared.tree).map_err(|reason| ProcessError::Supervision {
            operation: "enumerating the standalone run's process tree",
            reason,
        })
    }

    /// The window of the run's application, if it has one on screen now.
    ///
    /// Two shapes count, because a standalone application can be either: a
    /// windowed program owns its top-level window in its own process, while a
    /// console program's console window is hosted for it by Windows on a
    /// process of its own. What the user means by "唤起原窗口" is the same thing
    /// in both cases, so the search is `crate::window::application_window`'s —
    /// the same one an instance the Hub did *not* start is found through (#67).
    pub fn window(&self) -> Option<TopLevelWindow> {
        window::wait_for_application_window(|| self.tree_and_identity(), Duration::ZERO)
    }

    /// The processes to search for the run's window, as the current reading.
    ///
    /// A tree that cannot be read is an empty one rather than a failure: the
    /// console fallback below it is still a real answer, and reporting "no
    /// window" is more honest than reporting the read failure as one.
    fn tree_and_identity(&self) -> window::Processes {
        window::Processes {
            pids: self.tree_pids().unwrap_or_default(),
            lead: self.pid(),
            // A console window found for a pid Windows has since reused would
            // be somebody else's console (spec #59 decision 11).
            lead_is_current: self.shared.identity.matches(),
        }
    }

    /// The run's window, waiting up to `timeout` for one to appear.
    ///
    /// A just-started application has not built its window yet, and the caller
    /// asking for it is a click that means "show me that application". The wait
    /// is bounded and reports honestly when it runs out: no window is a real
    /// outcome for an application that is still starting, and it is not a
    /// reason to start another one (spec #59 decision 11).
    pub fn wait_for_window(&self, timeout: Duration) -> Option<TopLevelWindow> {
        window::wait_for_application_window(|| self.tree_and_identity(), timeout)
    }

    /// Ask the application to finish, then terminate its tree if it outlasts
    /// `timeout`.
    ///
    /// On return with `Ok`, nothing of the run is left. Whether the Hub may
    /// *use* this is a lifecycle question the session layer answers (#66): this
    /// method is the mechanism, and it is the same one for a managed standalone
    /// entry and an explicitly stopped one.
    pub fn stop(&self, timeout: Duration) -> Result<StopReport, ProcessError> {
        if !self.is_running() {
            // Nothing of the run is left, so there is nothing to ask. A stop
            // that had to be confirmed is still a stop — the force path is what
            // confirms, and it reports `AlreadyExited`.
            return self.force_stop();
        }

        let graceful_delivered = self.request_close();
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

    /// Terminate the run's tree without asking it first.
    ///
    /// Unlike a supervised service there is nothing to *confirm* about an
    /// unmanaged application when the Hub leaves — this path only runs when the
    /// user asked the Hub to stop it, or when the entry is explicitly managed
    /// (#66).
    pub fn force_stop(&self) -> Result<StopReport, ProcessError> {
        let already = self.exit_status();
        // Read before the termination, so the report can say which of the two
        // happened: a run that was already gone is not one this call ended.
        let remaining = self.tree_pids()?;
        backend::terminate_tree(&self.shared.tree).map_err(|reason| ProcessError::Supervision {
            operation: "terminating the standalone run's process tree",
            reason,
        })?;

        let exit = match already.or_else(|| self.wait_for_exit(TREE_GONE_TIMEOUT)) {
            Some(exit) => exit,
            None => {
                return Err(ProcessError::Supervision {
                    operation: "confirming the standalone run ended",
                    reason: format!(
                        "no exit status for pid {} after terminating its tree",
                        self.pid()
                    ),
                })
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

    /// Ask the application to close, reporting whether a request was actually
    /// delivered anywhere.
    ///
    /// The window comes first because it is the gesture the user performs on a
    /// windowed application — closing its window, which lets it save its work
    /// and answer dialogs. A run without a window (a console program, or one
    /// whose window has not appeared yet) gets the console interrupt instead,
    /// delivered only while the pid still belongs to this run: a `CTRL_BREAK`
    /// aimed at a number Windows has already handed to somebody else would be a
    /// signal to an unrelated process, which is the pid-reuse mistake spec #59
    /// decision 11 asks this layer to make impossible rather than unlikely.
    fn request_close(&self) -> bool {
        if let Some(window) = self.window() {
            return window.close();
        }
        if !self.shared.identity.matches() {
            return false;
        }
        backend::request_graceful_stop(self.pid())
    }

    /// Barrier behind `stop`/`force_stop`: nothing may remain of the run before
    /// a stop reports success.
    fn confirm_tree_gone(&self) -> Result<(), ProcessError> {
        let deadline = Instant::now() + TREE_GONE_TIMEOUT;
        loop {
            let remaining = self.tree_pids()?;
            if remaining.is_empty() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(ProcessError::Supervision {
                    operation: "confirming the standalone run's process tree is gone",
                    reason: format!("processes still assigned to the run: {remaining:?}"),
                });
            }
            std::thread::sleep(super::POLL_INTERVAL);
        }
    }
}

impl Shared {
    /// Watch the run to its end in a thread of its own.
    ///
    /// The wait itself costs nothing while the application runs: the thread
    /// blocks on the process object, so a tray-resident Hub holds no timer per
    /// standalone application (D-009). The tree is only read once that object
    /// signals — which is also the only moment a launcher-shaped run needs it.
    ///
    /// A watcher that could not be started is not a run this layer has lost:
    /// [`IndependentProcess::exit_status`] reads the process object and the tree
    /// itself, so the ending is still observed by whoever asks. What is missing
    /// is the *wait* — a caller blocks for its own timeout instead of being
    /// woken — and that is a worse latency, not a wrong answer.
    fn watch(self: &Arc<Self>) {
        let shared = Arc::clone(self);
        let _ = std::thread::Builder::new()
            .name(format!("lch-independent-{}", shared.identity.pid()))
            .spawn(move || {
                watch_exit_handle(&shared.child, &shared.own_exit);
                let exit = *lock(&shared.own_exit);

                // Own process gone: the run is over when its tree is, and a
                // launcher's child can outlive it by hours.
                //
                // A tree that cannot be *read* is not an empty one — saying the
                // run ended there would be a claim nothing confirmed — so the
                // loop treats it as "not over yet". It does not treat it as
                // "wait forever" either: readings that keep failing stop the
                // watcher rather than leave a thread polling for the life of
                // the Hub (D-009). Settling then falls to whoever asks next,
                // because `exit_status` reads the process object and the tree
                // itself; what is lost is the wait, not the answer.
                let mut unreadable = 0;
                loop {
                    match backend::tree_pids(&shared.tree) {
                        Ok(pids) if pids.is_empty() => break,
                        Ok(_) => unreadable = 0,
                        Err(_) => {
                            unreadable += 1;
                            if unreadable >= MAX_UNREADABLE_TREE_READINGS {
                                return;
                            }
                        }
                    }
                    std::thread::sleep(TREE_TICK);
                }

                *lock(&shared.ended) = Some(exit.unwrap_or(ExitStatus { code: None }));
                shared.ended_signal.notify_all();
            });
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::Command as StdCommand;
    use std::time::{SystemTime, UNIX_EPOCH};
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    /// A run that stays alive far longer than any test needs: a shell whose own
    /// child does the waiting, so the run has a tree as well as a process.
    fn start() -> IndependentProcess {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let marker = format!("lch-independent-ready-{}-{nonce}.txt", std::process::id());
        let cwd = std::env::temp_dir();
        let ready = cwd.join(&marker);
        // A relative, generated filename needs no nested cmd quoting, even
        // when the user's temp directory contains spaces. No batch file or
        // temporary working directory needs to outlive this function.
        let spec = ProcessSpec::new("cmd.exe", cwd).with_args(vec![
            "/d".to_owned(),
            "/c".to_owned(),
            format!("echo ready > {marker} & ping -n 120 127.0.0.1 > NUL"),
        ]);
        let run = IndependentProcess::spawn(spec).expect("the standalone run starts");
        // CreateProcess returns before Windows Terminal has finished handing
        // off the console. Do not terminate the fixture before its shell has
        // actually started: that can leave a host's failed-launch error tab.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() {
            if Instant::now() >= deadline {
                let cleanup = run.force_stop();
                panic!("the standalone shell did not become ready; cleanup={cleanup:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_file(ready);
        run
    }

    fn stop_timeout() -> Duration {
        Duration::from_secs(3)
    }

    /// A caller can observe exit before the watcher records it. Exercise that
    /// ordering without a watcher so this cannot pass just by winning a race.
    #[test]
    fn an_unrecorded_exit_is_observed_without_waiting_for_the_watcher() {
        let mut command = StdCommand::new("cmd.exe");
        command.args(["/d", "/c", "exit 0"]);
        let child = backend::spawn(&mut command, backend::prepare_windowed)
            .expect("the fixture starts suspended");
        let identity = ProcessIdentity::of_child(&child);
        let tree = backend::attach_independent(&child).expect("the fixture is owned and resumed");
        let child = Mutex::new(child);
        let status = super::super::poll_child(&child, Instant::now() + Duration::from_secs(10));
        let expected = observe_exit(status.expect("the fixture exits before observation"));
        // Windows can keep the console host in the job briefly after cmd exits.
        // Settle the tree too, so the assertion is about the fallback exit read.
        backend::terminate_tree(&tree).expect("the fixture tree ends");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !backend::tree_pids(&tree)
            .expect("the tree is readable")
            .is_empty()
        {
            assert!(Instant::now() < deadline, "the fixture tree settles");
            std::thread::sleep(Duration::from_millis(20));
        }
        let shared = Shared {
            child,
            tree,
            identity,
            own_exit: Mutex::new(None),
            ended: Mutex::new(None),
            ended_signal: Condvar::new(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let run = IndependentProcess {
                shared: Arc::new(shared),
            };
            let first = run.exit_status();
            let second = run.exit_status();
            tx.send((first, second)).expect("the result is awaited");
        });
        let (first, second) = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("observing an unrecorded exit must not deadlock");
        assert_eq!(first, Some(expected));
        assert_eq!(second, first, "the observed exit stays recorded");
    }

    /// A private directory for the handshake scripts these tests use.
    struct FixtureDir(PathBuf);

    impl FixtureDir {
        fn new(tag: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "lch-independent-{tag}-{}-{nonce}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("the fixture directory is created");
            FixtureDir(path)
        }

        fn write(&self, name: &str, contents: &str) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, contents).expect("the fixture file is written");
            path
        }
    }

    impl Drop for FixtureDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Kill a process the test started outside the supervisor, so a failing
    /// assertion cannot leave a pinger behind.
    fn kill_tree(pid: u32) {
        let _ = StdCommand::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    struct KillOnDrop(u32);

    impl Drop for KillOnDrop {
        fn drop(&mut self) {
            kill_tree(self.0);
        }
    }

    /// The property the whole mode exists for: the tree belongs to a job, and
    /// that job does not kill its members when its handle goes away.
    #[test]
    fn the_run_survives_the_handle_being_dropped() {
        let run = start();
        let pid = run.pid();
        let _guard = KillOnDrop(pid);

        assert!(run.is_running());
        assert!(
            run.tree_pids()
                .expect("the tree is observable")
                .contains(&pid),
            "the run's own process is part of the tree it owns"
        );

        // This is what the Hub exiting does: the handle — and with it the job
        // and process handles — goes away.
        drop(run);

        assert!(
            super::super::win::is_process_alive(pid),
            "a standalone run must outlive the handle that started it (spec #59 decision 12)"
        );
    }

    /// An application that hands a child off and exits is still running: the
    /// run is over when its tree is, not when its launcher is.
    #[test]
    fn a_launcher_that_exits_leaves_the_run_running() {
        let fixture = FixtureDir::new("launcher");
        let ready = fixture.write("child-ready.txt", "");
        std::fs::remove_file(&ready).expect("the handshake file starts absent");
        let child = fixture.write(
            "child.ps1",
            "[System.IO.File]::WriteAllText($args[0], [string]$PID)\nStart-Sleep -Seconds 120\n",
        );
        let launcher = fixture.write(
            "launcher.cmd",
            &format!(
                "@echo off\r\nstart \"\" /b powershell.exe -NoProfile -ExecutionPolicy Bypass -File \"{}\" \"{}\"\r\nexit /b 0\r\n",
                child.display(),
                ready.display()
            ),
        );
        let spec = ProcessSpec::new("cmd.exe", fixture.0.clone()).with_args(vec![
            "/d".to_owned(),
            "/c".to_owned(),
            launcher.to_string_lossy().into_owned(),
        ]);

        let run = IndependentProcess::spawn(spec).expect("the launcher starts");
        let _guard = KillOnDrop(run.pid());

        // Wait for the handshake so the assertion is about the shape, not a
        // race: by here the launcher has exited and its child has not.
        let deadline = Instant::now() + Duration::from_secs(30);
        while !ready.exists() {
            assert!(
                Instant::now() < deadline,
                "the launcher child never started"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let child_pid: u32 = std::fs::read_to_string(&ready)
            .expect("the handshake is readable")
            .trim()
            .parse()
            .expect("the handshake holds a pid");

        assert!(
            run.is_running(),
            "a run whose launcher exited but whose child lives is not over"
        );
        assert!(
            run.tree_pids()
                .expect("the tree is observable")
                .contains(&child_pid),
            "the child the launcher left behind is part of the run"
        );

        let report = run.stop(stop_timeout()).expect("the run stops");
        assert_ne!(report.outcome, StopOutcome::AlreadyExited, "{report:?}");
        assert!(!super::super::win::is_process_alive(child_pid));
    }

    /// A stop ends the run's tree and nothing else — never a pid, never a name.
    #[test]
    fn a_stop_ends_the_run_and_leaves_an_unrelated_process_alone() {
        let run = start();
        let mut unrelated = StdCommand::new("cmd.exe")
            .args(["/c", "ping -n 120 127.0.0.1 > NUL"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the unrelated process starts");
        let _guard = KillOnDrop(unrelated.id());

        let tree = run.tree_pids().expect("the tree is observable");
        let report = run.stop(stop_timeout()).expect("the run stops");

        assert_ne!(report.outcome, StopOutcome::AlreadyExited, "{report:?}");
        assert!(
            run.tree_pids().expect("the tree is observable").is_empty(),
            "the run's tree must be gone after a stop"
        );
        for pid in tree {
            assert!(
                !super::super::win::is_process_alive(pid),
                "pid {pid} survived the stop of its run"
            );
        }
        assert!(
            unrelated.try_wait().expect("waitable").is_none(),
            "an unrelated process must survive a stop, even with the same executable"
        );
        // Kill the tree while its root is alive. Killing cmd first prevents
        // the PID guard from finding its ping child and leaves an orphan.
        kill_tree(unrelated.id());
        let _ = unrelated.wait();
    }

    /// Stopping a run whose application has already closed its window and
    /// exited is a no-op that says so rather than a second ending.
    #[test]
    fn stopping_an_ended_run_reports_that_it_had_already_ended() {
        let spec = ProcessSpec::new("cmd.exe", std::env::temp_dir())
            .with_args(vec!["/c".to_owned(), "exit 0".to_owned()]);
        let run = IndependentProcess::spawn(spec).expect("the run starts");
        run.wait_for_exit(Duration::from_secs(30))
            .expect("a run that exits by itself is observed");

        let report = run
            .stop(stop_timeout())
            .expect("stopping an ended run is safe");

        assert_eq!(report.outcome, StopOutcome::AlreadyExited, "{report:?}");
        assert_eq!(report.exit.code, Some(0));
        assert!(
            !report.graceful_delivered,
            "nothing was asked of a run that had ended"
        );
    }

    /// The identity is taken from a real process and matches it (#66).
    #[test]
    fn an_identity_matches_the_process_it_was_taken_from() {
        let run = start();

        let identity = run.identity();

        assert_eq!(identity.pid(), run.pid());
        assert_ne!(identity.created_at(), 0, "Windows reports a creation time");
        assert!(
            identity.matches(),
            "the identity a run holds answers for the run's own process"
        );

        let _ = run.force_stop();
    }

    /// And it does **not** vouch for a process that only shares the number: a
    /// pid Windows has handed to somebody else carries a different creation
    /// time, which is the fact the check turns on (spec #59 decision 11).
    ///
    /// Exercised through a hand-made identity because the mismatch it guards
    /// against — Windows reusing a pid — is not something a test can schedule.
    #[test]
    fn an_identity_does_not_match_a_process_with_a_different_creation_time() {
        let run = start();
        let mine = run.identity();
        let reused = ProcessIdentity::for_test(mine.pid(), mine.created_at().wrapping_add(1));

        assert!(mine.matches(), "the run's own identity matches");
        assert!(
            !reused.matches(),
            "the same pid with a different creation time is somebody else's process"
        );

        let _ = run.force_stop();
    }

    /// The console an application keeps is its own, and it is what the Hub
    /// brings forward for a console-shaped standalone entry: the window belongs
    /// to Windows' console host rather than to the application's own process,
    /// so finding it is a second lookup, not a bigger pid filter.
    ///
    /// Which half of that is observable here is the environment's answer, not
    /// this test's: the lookup borrows the run's console, and a process whose
    /// own console has a window — or which is alone on its console and so could
    /// not be put back on it — does not borrow one (D-037). Both halves are
    /// asserted, so this passes on what the environment allowed rather than on
    /// nothing at all.
    #[test]
    fn a_console_run_presents_its_console_window() {
        let run = start();
        let borrowable = crate::console::can_borrow();

        if !borrowable {
            // A question this process may not ask is refused, not answered with
            // a console window belonging to somebody else.
            let window = run.window();
            let _ = run.force_stop();
            assert!(window.is_none(), "a refused lookup found {window:?}");
            return;
        }

        let deadline = Instant::now() + Duration::from_secs(10);
        let window = loop {
            if let Some(window) = run.window() {
                break window;
            }
            assert!(
                Instant::now() < deadline,
                "a console run's window must be findable while it runs"
            );
            std::thread::sleep(Duration::from_millis(50));
        };

        assert!(
            window.visible,
            "the console window of a running shell is on screen: {window:?}"
        );

        let _ = run.force_stop();
    }

    /// The wait for a window is bounded and answers honestly when nothing
    /// appears: an application that is still starting has no window yet, and
    /// that is a real outcome rather than a reason to start another run
    /// (spec #59 decision 11).
    #[test]
    fn waiting_for_a_window_gives_up_without_inventing_one() {
        // A console program that produces no window at all: `ping` under a
        // shell that exits leaves nothing to focus.
        let spec = ProcessSpec::new("cmd.exe", std::env::temp_dir())
            .with_args(vec!["/c".to_owned(), "exit 0".to_owned()]);
        let run = IndependentProcess::spawn(spec).expect("the run starts");
        run.wait_for_exit(Duration::from_secs(30))
            .expect("the run ends by itself");

        let started = Instant::now();
        let window = run.wait_for_window(Duration::from_millis(200));

        assert!(window.is_none(), "an ended run has no window: {window:?}");
        assert!(
            started.elapsed() >= Duration::from_millis(200),
            "the wait has to use its bound"
        );
    }
}
