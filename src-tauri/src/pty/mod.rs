//! PTY layer — the terminal host behind every interactive session.
//!
//! Owned by T02 (#3, Windows PTY/ConPTY validation — RELEASE BLOCKER). One
//! [`Pty`] hosts one interactive terminal: it spawns the shell on a
//! pseudoconsole in the configured working directory, carries bytes both ways,
//! resizes, and reports the shell's exit (`docs/MVP_IMPLEMENTATION_SPEC.md`
//! §6). xterm.js renders the terminal; it does not own the process or the pty,
//! and this layer does not decide logging policy — what happens to terminal
//! output is the logging layer's business (T05).
//!
//! ## Contract
//!
//! ```text
//! Pty::spawn(PtySpec) -> Pty
//!   .write(bytes) / .interrupt()          // input, incl. the Ctrl+C byte
//!   .read_output(timeout) -> Option<Vec<u8>>   // rendered output chunks
//!   .output_ended() / .is_running()
//!   .resize(cols, rows)
//!   .exit_status() / .wait_for_exit(timeout)
//!   .kill()
//! ```
//!
//! ## Backpressure
//!
//! Output is bounded by construction: the reader thread reads the pty into a
//! bounded queue, and when the queue is full the reader stops reading, the
//! pipe fills, and the process itself blocks on its next write. A noisy
//! process can slow its own output down; it can never grow the Hub's memory
//! without limit. Chunks arrive as raw bytes (UTF-8 with ANSI/VT sequences,
//! which is what a terminal renderer consumes) — parsing them is the
//! frontend's job, not this layer's.
//!
//! ## Ctrl+C
//!
//! "Ctrl+C" at this layer is [`Pty::interrupt`]: one `0x03` byte into the pty,
//! which the console host raises as `CTRL_C_EVENT` for the process group on
//! that pseudoconsole — the same event a physical console delivers. It is
//! input to the running program, *not* a stop of the session: closing a
//! terminal session is a different action entirely (`MVP §7`).
//!
//! ## Ownership
//!
//! A [`Pty`] owns its terminal outright: dropping the handle terminates the
//! shell and closes the pseudoconsole, so a terminal session cannot outlive
//! the host that accounts for it. The layer deliberately does **not** manage a
//! process *tree*: descendants the shell starts are reclaimed by closing the
//! pseudoconsole (the console they live on), and full job-object supervision
//! of terminal sessions is a seam this layer leaves to the session runtime —
//! [`Pty::pid`] exists so that wiring can attach ownership later without a
//! contract change.
//!
//! ## Mechanism (Windows)
//!
//! Direct ConPTY over `windows-sys` (D-014): a pseudoconsole per session, two
//! anonymous pipes around it, and the shell attached at process creation via
//! the pseudoconsole attribute — no console window is ever created, which is
//! the point of hosting on a pty in the first place.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::process::ExitStatus;

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
use unsupported as backend;
#[cfg(windows)]
use win as backend;

/// Terminal geometry used when a caller has no configured value.
pub const DEFAULT_COLS: u16 = 80;
pub const DEFAULT_ROWS: u16 = 24;

/// The byte "Ctrl+C" is at this layer: written into the pty, translated by the
/// console host into `CTRL_C_EVENT` for the process group on the terminal.
pub const INTERRUPT_BYTE: u8 = 0x03;

/// Largest terminal dimension this layer accepts. Far beyond any real window,
/// but small enough that a `COORD`-shaped cast can never truncate it.
pub const MAX_DIMENSION: u16 = 1024;

/// Bytes read from the pty per chunk.
const OUTPUT_CHUNK_BYTES: usize = 4096;

/// Chunks kept queued for the consumer before the reader thread stops reading
/// and backpressure sets in: 64 × 4 KiB = 256 KiB of buffered output per
/// session, the layer's hard ceiling on memory for one terminal.
const OUTPUT_QUEUE_CHUNKS: usize = 64;

/// How long [`Pty::kill`] waits for the shell to confirm its termination.
const KILL_TIMEOUT: Duration = Duration::from_secs(10);

/// What to host on a terminal.
///
/// Deliberately argv-shaped, like the process layer's `ProcessSpec`: turning a
/// configured command string into `program` plus `args` (and choosing which
/// shell runs it) is the caller's decision, not a policy this layer invents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySpec {
    /// Shell or program to host. A full path is preferred; `PATH` lookup is
    /// left to the platform.
    pub program: PathBuf,
    /// Arguments, passed through as-is — no shell interpretation.
    pub args: Vec<String>,
    /// Working directory for the shell.
    pub cwd: PathBuf,
    /// Initial terminal width in cells.
    pub cols: u16,
    /// Initial terminal height in cells.
    pub rows: u16,
}

impl PtySpec {
    /// A terminal hosting `program` in `cwd`, at the default geometry.
    pub fn new(program: impl Into<PathBuf>, cwd: impl Into<PathBuf>) -> Self {
        PtySpec {
            program: program.into(),
            args: Vec::new(),
            cwd: cwd.into(),
            cols: DEFAULT_COLS,
            rows: DEFAULT_ROWS,
        }
    }

    /// Builder-style: replace the argument list.
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Builder-style: replace the terminal geometry.
    pub fn with_size(mut self, cols: u16, rows: u16) -> Self {
        self.cols = cols;
        self.rows = rows;
        self
    }
}

/// Structured failure of a terminal operation. Messages name the operation and
/// the paths involved so a user can act on them (`docs/DEVELOPMENT.md` §9).
#[derive(Debug)]
pub enum PtyError {
    /// The terminal could not be started.
    Spawn {
        program: PathBuf,
        cwd: PathBuf,
        cols: u16,
        rows: u16,
        reason: String,
    },
    /// The requested terminal dimensions are not hostable.
    InvalidSize {
        operation: &'static str,
        cols: u16,
        rows: u16,
    },
    /// Input could not be delivered to the terminal — most often, the terminal
    /// has already ended and its input pipe is gone.
    Write { source: std::io::Error },
    /// The terminal could not be resized.
    Resize { cols: u16, rows: u16, reason: String },
    /// The shell could not be terminated, or outlived its termination.
    Kill { pid: u32, reason: String },
    /// This platform has no PTY backend (Windows-first MVP, D-001).
    UnsupportedPlatform { operation: &'static str },
}

impl std::fmt::Display for PtyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PtyError::Spawn {
                program,
                cwd,
                cols,
                rows,
                reason,
            } => write!(
                formatter,
                "failed to host `{}` on a terminal in working directory `{}` at {cols}x{rows}: \
                 {reason}",
                program.display(),
                cwd.display()
            ),
            PtyError::InvalidSize {
                operation,
                cols,
                rows,
            } => write!(
                formatter,
                "{operation} needs terminal dimensions of at least 1x1 and at most \
                 {MAX_DIMENSION}x{MAX_DIMENSION}, got {cols}x{rows}"
            ),
            PtyError::Write { source } => {
                write!(formatter, "sending input to the terminal failed: {source}")
            }
            PtyError::Resize { cols, rows, reason } => {
                write!(formatter, "resizing the terminal to {cols}x{rows} failed: {reason}")
            }
            PtyError::Kill { pid, reason } => {
                write!(formatter, "terminating terminal process {pid} failed: {reason}")
            }
            PtyError::UnsupportedPlatform { operation } => write!(
                formatter,
                "{operation} is not supported on this platform — Local Console Hub hosts \
                 terminals on Windows only (D-001)"
            ),
        }
    }
}

impl std::error::Error for PtyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PtyError::Write { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// State shared between the session handle and its reader/exit threads.
#[derive(Debug)]
struct Shared {
    /// The platform backend holding the pseudoconsole session.
    backend: backend::PtyBackend,
    /// Bounded queue of rendered output chunks. Locked only for one
    /// `recv_timeout` at a time — the one-consumer contract of the stream.
    output: Mutex<Receiver<Vec<u8>>>,
    /// Set by the reader thread when the pty's output has ended.
    output_ended: Arc<AtomicBool>,
    /// Filled in by the exit watcher exactly once.
    exit: Mutex<Option<ExitStatus>>,
    exited: Condvar,
}

/// One hosted terminal.
///
/// `Pty` is `Send + Sync` and its observation, input and resize methods take
/// `&self`, so a session registry can share one handle across IPC, tray and
/// watcher threads without wrapping it in another lock.
#[derive(Debug)]
pub struct Pty {
    shared: Arc<Shared>,
}

impl Pty {
    /// Host `spec`'s program on a new terminal.
    pub fn spawn(spec: PtySpec) -> Result<Self, PtyError> {
        backend::require_backend("starting an interactive terminal")?;
        check_size("starting an interactive terminal", spec.cols, spec.rows)?;

        let spawned = backend::PtyBackend::spawn(&spec);
        let (backend, output) = spawned.map_err(|reason| PtyError::Spawn {
            program: spec.program.clone(),
            cwd: spec.cwd.clone(),
            cols: spec.cols,
            rows: spec.rows,
            reason,
        })?;
        let pid = backend.pid();

        let (sender, receiver) = std::sync::mpsc::sync_channel(OUTPUT_QUEUE_CHUNKS);
        let output_ended = Arc::new(AtomicBool::new(false));
        let ended_for_reader = Arc::clone(&output_ended);
        let reader = std::thread::Builder::new()
            .name(format!("lch-pty-read-{pid}"))
            .spawn(move || read_output_forever(output, sender, ended_for_reader));
        if let Err(source) = reader {
            // A terminal whose output cannot be read must not be left hosted.
            let _ = backend.terminate();
            return Err(PtyError::Spawn {
                program: spec.program,
                cwd: spec.cwd,
                cols: spec.cols,
                rows: spec.rows,
                reason: format!("starting the output reader: {source}"),
            });
        }

        let shared = Arc::new(Shared {
            backend,
            output: Mutex::new(receiver),
            output_ended,
            exit: Mutex::new(None),
            exited: Condvar::new(),
        });
        watch_exit(&shared);
        Ok(Pty { shared })
    }

    /// The hosted shell's process id — for display and for the session runtime
    /// that will attach process ownership later (T04).
    pub fn pid(&self) -> u32 {
        self.shared.backend.pid()
    }

    /// Send input (keystrokes) to the terminal. Bytes go to the shell exactly
    /// as a terminal would deliver them — including `\r` for Enter, which
    /// callers must add themselves.
    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.shared
            .backend
            .write_input(bytes)
            .map_err(|source| PtyError::Write { source })
    }

    /// Deliver "Ctrl+C" to the terminal: one [`INTERRUPT_BYTE`] into the pty,
    /// raised as `CTRL_C_EVENT` for the process group on it. Interrupting the
    /// running command is not closing the session (`MVP §7`).
    pub fn interrupt(&self) -> Result<(), PtyError> {
        self.write(&[INTERRUPT_BYTE])
    }

    /// Resize the terminal. The new size reaches the hosted shell as its
    /// console size; existing screen content is not re-flowed by this layer.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), PtyError> {
        check_size("resizing an interactive terminal", cols, rows)?;
        self.shared
            .backend
            .resize(cols, rows)
            .map_err(|reason| PtyError::Resize { cols, rows, reason })
    }

    /// Wait up to `timeout` for the next chunk of rendered output. `None`
    /// means no chunk arrived in time, or the stream has ended —
    /// [`Pty::output_ended`] tells the two apart.
    pub fn read_output(&self, timeout: Duration) -> Option<Vec<u8>> {
        match lock(&self.shared.output).recv_timeout(timeout) {
            Ok(chunk) => Some(chunk),
            // Both quiet spells and a finished stream yield `None`: the output
            // end is a separate observation, and a consumer loop that asks
            // "ended?" only when a read comes back empty is never confused.
            Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    /// Whether the terminal's output stream has ended: the pseudoconsole
    /// closed. All chunks queued before the end remain readable.
    pub fn output_ended(&self) -> bool {
        self.shared.output_ended.load(Ordering::Acquire)
    }

    /// Whether the hosted shell is still alive.
    pub fn is_running(&self) -> bool {
        self.exit_status().is_none()
    }

    /// The shell's exit status once it has ended, without blocking.
    pub fn exit_status(&self) -> Option<ExitStatus> {
        if let Some(recorded) = *lock(&self.shared.exit) {
            return Some(recorded);
        }
        // The watcher owns the record, but reading the process object directly
        // keeps the answer correct in the moment before it is written — the
        // same stance as the process layer.
        self.shared
            .backend
            .child()
            .exit_code()
            .map(|code| ExitStatus { code: Some(code) })
    }

    /// Wait up to `timeout` for the shell to end. `None` means it was still
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
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(recorded) = *exit {
            return Some(recorded);
        }
        drop(exit);
        self.exit_status()
    }

    /// Terminate the hosted shell. On return with `Ok`, the shell is confirmed
    /// gone; a shell that had already exited takes the no-op path, so repeated
    /// kills are safe.
    ///
    /// There is deliberately no graceful/force ladder here, unlike the process
    /// layer's `stop` (`docs/DECISIONS.md` D-007): a terminal's graceful
    /// gesture is *input* — [`Pty::interrupt`], which only the user chooses to
    /// send — and closing a session is an explicit teardown of the console
    /// itself. The stop ladder of `docs/DEVELOPMENT.md` §6 governs managed
    /// services, not interactive terminals.
    pub fn kill(&self) -> Result<(), PtyError> {
        if self.exit_status().is_some() {
            return Ok(());
        }
        let terminated = self.shared.backend.terminate();
        match self.wait_for_exit(KILL_TIMEOUT) {
            Some(_) => Ok(()),
            None => Err(PtyError::Kill {
                pid: self.pid(),
                reason: match terminated {
                    Err(reason) => reason,
                    // Termination reported success but the exit was never
                    // observed — the report says exactly that rather than
                    // inventing a Win32 cause.
                    Ok(()) => format!("it was still running {KILL_TIMEOUT:?} later"),
                },
            }),
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // Ownership is RAII, the same stance as the process layer: a handle
        // going away must not leave a shell nobody accounts for. Terminating
        // the shell and dropping the pseudoconsole ends the console the
        // session lives on; a session that already ended takes the no-op path.
        // The wait is bounded by KILL_TIMEOUT — a live shell pays the
        // terminate-and-confirm round trip, an ended one returns immediately.
        let _ = self.kill();
    }
}

/// Reject dimensions the layer cannot host, with an operation-named error.
fn check_size(operation: &'static str, cols: u16, rows: u16) -> Result<(), PtyError> {
    let hostable = cols > 0 && rows > 0 && cols <= MAX_DIMENSION && rows <= MAX_DIMENSION;
    if hostable {
        Ok(())
    } else {
        Err(PtyError::InvalidSize { operation, cols, rows })
    }
}

/// Lock a mutex while tolerating poisoning: a panic on one path must not turn
/// every later read into a second panic.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Read the pty's output into the bounded queue until the stream ends.
///
/// This is the layer's own thread — the UI render thread is never the only
/// reader of terminal output (`docs/DEVELOPMENT.md` §5). It holds no share of
/// [`Shared`]: a consumer that goes away drops the queue, the next `send`
/// fails, and this thread exits instead of pinning the session alive.
fn read_output_forever(
    mut reader: backend::OutputReader,
    sender: SyncSender<Vec<u8>>,
    ended: Arc<AtomicBool>,
) {
    let mut buffer = vec![0u8; OUTPUT_CHUNK_BYTES];
    loop {
        match reader.read_chunk(&mut buffer) {
            // EOF and a dead pipe both mean the same thing here: the terminal
            // that produced this output no longer exists.
            Ok(0) | Err(_) => break,
            Ok(bytes) => {
                if sender.send(buffer[..bytes].to_vec()).is_err() {
                    break; // the consumer is gone; nothing is left to read for
                }
            }
        }
    }
    ended.store(true, Ordering::Release);
}

/// Watch one shell to its end in a thread of its own, so the Hub holds no
/// polling timer per terminal (D-009).
fn watch_exit(shared: &Arc<Shared>) {
    let child = shared.backend.child();
    let pid = shared.backend.pid();
    let shared = Arc::clone(shared);
    let watcher = std::thread::Builder::new()
        .name(format!("lch-pty-exit-{pid}"))
        .spawn(move || {
            child.wait_signaled();
            // Once the process object is signalled the shell is over, whatever
            // the exit code reads as: a process that genuinely exits with
            // 259 — the numeric value of `STILL_ACTIVE` — must not be reported
            // as running forever, so the watcher records unconditionally and
            // `exit_code`'s None (a live answer) is only the pre-signal view.
            let exit = match child.exit_code() {
                Some(code) => ExitStatus { code: Some(code) },
                None => ExitStatus { code: None },
            };
            *lock(&shared.exit) = Some(exit);
            shared.exited.notify_all();
        });
    // If the watcher could not start, nothing a caller can see is lost:
    // `exit_status` reads the process object itself, and every wait on the
    // condvar carries a timeout.
    drop(watcher);
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::thread;

    /// Windows PowerShell, present on every supported install. `-NoLogo
    /// -NoProfile` keeps startup fast and deterministic on CI runners.
    const POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.exe";

    /// Cold PowerShell startup on a busy CI runner can take a while before the
    /// first prompt renders.
    const STARTUP: Duration = Duration::from_secs(40);

    /// A PowerShell terminal in the temp directory.
    fn shell() -> PtySpec {
        PtySpec::new(POWERSHELL, std::env::temp_dir())
            .with_args(vec!["-NoLogo".to_owned(), "-NoProfile".to_owned()])
    }

    /// Start a terminal, failing the test if it cannot start.
    fn start() -> Pty {
        let spawned = Pty::spawn(shell());
        spawned.expect("the terminal starts")
    }

    /// Send one command line, the way a terminal sends Enter.
    fn send(pty: &Pty, line: &str) {
        pty.write(format!("{line}\r").as_bytes())
            .expect("input reaches the terminal");
    }

    /// Read output until `marker` renders, and return everything seen so far.
    /// Panics with the partial transcript when it never shows up.
    fn expect_output(pty: &Pty, marker: &str, seconds: u64) -> String {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        let mut seen = String::new();
        while Instant::now() < deadline {
            if let Some(chunk) = pty.read_output(Duration::from_millis(200)) {
                seen.push_str(&String::from_utf8_lossy(&chunk));
                if seen.contains(marker) {
                    return seen;
                }
            }
        }
        panic!("the terminal never showed `{marker}`, saw: {seen:?}");
    }

    /// Poll `probe` until it answers `true`, for up to `seconds`.
    fn eventually(seconds: u64, mut probe: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            if probe() {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_millis(200));
        }
    }

    /// Pids of processes whose command line mentions `marker`, excluding the
    /// probe itself — how a test sees a process it never started. The marker
    /// nonce keeps this immune to parallel tests.
    fn processes_with_marker(marker: &str) -> Vec<u32> {
        let query = format!(
            "Get-CimInstance Win32_Process -Filter \"CommandLine LIKE '%{marker}%'\" | \
             Where-Object {{ $_.ProcessId -ne $PID }} | Select-Object -ExpandProperty ProcessId"
        );
        let probe = std::process::Command::new(POWERSHELL)
            .args(["-NoLogo", "-NoProfile", "-Command", &query])
            .output()
            .expect("the probe runs");
        String::from_utf8_lossy(&probe.stdout)
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect()
    }

    #[test]
    fn handle_is_shareable_across_threads() {
        // The contract the session registry relies on: one handle shared
        // between IPC, tray and watcher threads without another lock.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Pty>();
    }

    #[test]
    fn spawn_reports_a_pid_and_first_output() {
        let pty = start();
        assert_ne!(pty.pid(), 0);
        assert!(pty.is_running());
        assert!(pty.exit_status().is_none());
        // The first prompt is the pty proving it renders: a pipe-only child
        // would never produce it.
        expect_output(&pty, "PS", STARTUP.as_secs());

        pty.kill().expect("cleanup");
    }

    #[test]
    fn commands_roundtrip_through_the_terminal() {
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        send(&pty, "Write-Host LCH-MARKER-7Q");
        expect_output(&pty, "LCH-MARKER-7Q", 20);

        pty.kill().expect("cleanup");
    }

    #[test]
    fn unicode_survives_the_round_trip() {
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        // Chinese text is a first-class scenario here (docs/DEVELOPMENT.md §5),
        // not an edge case: ConPTY must carry it in both directions untouched.
        send(&pty, "Write-Host 你好LCH-世界-Ünïcode-3U");
        expect_output(&pty, "你好LCH-世界-Ünïcode-3U", 20);

        pty.kill().expect("cleanup");
    }

    #[test]
    fn ansi_output_reaches_the_stream() {
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        send(&pty, "Write-Host -ForegroundColor Red LCH-RED-2A");
        let seen = expect_output(&pty, "LCH-RED-2A", 20);
        assert!(
            seen.contains('\u{1b}'),
            "the stream must carry VT/ANSI sequences, saw {seen:?}"
        );

        pty.kill().expect("cleanup");
    }

    #[test]
    fn ctrl_c_interrupts_the_running_command_not_the_shell() {
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        // STARTED proves the minute-long ping has begun; NEVER must not run,
        // because the interrupt cancels the pipeline before it.
        send(
            &pty,
            "Write-Host LCH-STARTED-9C; ping -n 60 127.0.0.1 | Out-Null; Write-Host LCH-NEVER-9C",
        );
        expect_output(&pty, "LCH-STARTED-9C", 20);

        // One 0x03 byte is the whole gesture: the console host raises
        // CTRL_C_EVENT for the process group on the pty.
        pty.interrupt().expect("Ctrl+C reaches the terminal");

        send(&pty, "Write-Host LCH-RESUMED-9C");
        let seen = expect_output(&pty, "LCH-RESUMED-9C", 20);
        assert!(
            !seen.contains("LCH-NEVER-9C"),
            "the interrupted pipeline's tail must not run, saw {seen:?}"
        );

        pty.kill().expect("cleanup");
    }

    #[test]
    fn resize_reaches_the_shell() {
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        pty.resize(120, 30).expect("the terminal resizes");
        // The shell's own console query must report the new geometry in both
        // dimensions — this is the contract xterm.js resize forwarding leans
        // on (T07).
        send(
            &pty,
            "Write-Host LCH-GEO-$($Host.UI.RawUI.BufferSize.Width)x$($Host.UI.RawUI.BufferSize.\
             Height)",
        );
        let seen = expect_output(&pty, "LCH-GEO-", 20);
        assert!(
            seen.contains("LCH-GEO-120x30"),
            "the shell must observe the resized geometry, saw {seen:?}"
        );

        pty.kill().expect("cleanup");
    }

    #[test]
    fn high_volume_output_flows_and_the_shell_stays_responsive() {
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        send(
            &pty,
            "for ($i = 0; $i -lt 2000; $i++) { Write-Host \"LCH-LINE $i\" }; Write-Host \
             LCH-BURST-DONE-4H",
        );
        let mut seen = String::new();
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if let Some(chunk) = pty.read_output(Duration::from_millis(500)) {
                seen.push_str(&String::from_utf8_lossy(&chunk));
            }
            if seen.contains("LCH-BURST-DONE-4H") {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the burst never finished, saw {seen:?}"
            );
        }
        assert!(seen.contains("LCH-LINE 0"), "the burst's head, saw {seen:?}");
        assert!(
            seen.contains("LCH-LINE 1999"),
            "the burst's tail, saw {seen:?}"
        );
        assert!(
            seen.len() > 30_000,
            "a 2000-line burst is more than 30 KB rendered, saw {} bytes",
            seen.len()
        );

        // Flow control must never wedge the terminal: the next command still
        // runs after the flood.
        send(&pty, "Write-Host LCH-AFTER-4H");
        expect_output(&pty, "LCH-AFTER-4H", 20);

        pty.kill().expect("cleanup");
    }

    #[test]
    fn an_idle_unread_terminal_stays_alive() {
        // "Hidden" and "not the selected session" both mean, at this layer,
        // "nobody is reading the output for a while" — and that must never end
        // the terminal (MVP §6: alive while hidden or unselected).
        let pty = start();

        thread::sleep(Duration::from_secs(2));
        send(&pty, "Write-Host LCH-IDLE-5I");
        expect_output(&pty, "LCH-IDLE-5I", 30);

        pty.kill().expect("cleanup");
    }

    #[test]
    fn a_flooding_terminal_stays_alive_while_unread() {
        // The harder hidden-session shape: the process keeps producing while
        // nobody reads. The bounded queue fills, backpressure bites, and the
        // terminal must still be alive — and drain honestly — when the
        // consumer comes back (MVP §6, alive while hidden or unselected).
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        send(
            &pty,
            "for ($i = 0; $i -lt 3000; $i++) { Write-Host (\"LCH-FLOOD-$i-\" + (\"X\" * 100)) }; \
             Write-Host LCH-FLOOD-DONE-8F",
        );
        // ~330 KB of rendered output against a 256 KiB queue: long enough for
        // the queue to fill, the reader to stop reading and the pipe to back
        // up — the state a hidden, noisy session actually sits in.
        thread::sleep(Duration::from_secs(5));

        expect_output(&pty, "LCH-FLOOD-DONE-8F", 90);
        send(&pty, "Write-Host LCH-FLOOD-AFTER-8F");
        expect_output(&pty, "LCH-FLOOD-AFTER-8F", 20);

        pty.kill().expect("cleanup");
    }

    #[test]
    fn an_interactive_prompt_receives_its_answer() {
        // A prompt that blocks until input arrives is the honest definition of
        // "genuinely interactive" — input must be deliverable mid-run, not
        // only at a fresh prompt.
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        send(&pty, "$answer = Read-Host 'LCH-PROMPT-6P'; Write-Host LCH-GOT-$answer");
        expect_output(&pty, "LCH-PROMPT-6P", 20);
        send(&pty, "YES-6P");
        let seen = expect_output(&pty, "LCH-GOT-", 20);
        assert!(
            seen.contains("LCH-GOT-YES-6P"),
            "the prompt's answer must round trip, saw {seen:?}"
        );

        pty.kill().expect("cleanup");
    }

    #[test]
    fn closing_the_pty_reclaims_a_shell_child_process() {
        // A shell that started its own child (DEV §5: shell 内再启动子进程)
        // must not leave that child behind when the session closes: closing
        // the pseudoconsole ends the console the child lives on.
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        send(
            &pty,
            "powershell -NoLogo -NoProfile -Command \"Write-Host LCH-INNER-8G; Start-Sleep \
             -Seconds 300\"",
        );
        expect_output(&pty, "LCH-INNER-8G", 40);
        let live = eventually(10, || !processes_with_marker("LCH-INNER-8G").is_empty());
        assert!(live, "the shell's child should be running before the close");

        drop(pty); // RAII teardown: terminate the shell, close the console

        let gone = eventually(20, || processes_with_marker("LCH-INNER-8G").is_empty());
        assert!(
            gone,
            "closing the pty must reclaim the shell's child, still saw {:?}",
            processes_with_marker("LCH-INNER-8G")
        );
    }

    #[test]
    fn an_externally_killed_shell_is_observed_as_exited() {
        // DEV §5: 程序异常退出. The watcher must turn an abrupt, external
        // death into an observable exit — the session state machine (T04)
        // builds its unexpected-exit handling on exactly this.
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        let killed = std::process::Command::new("taskkill")
            .args(["/PID", &pty.pid().to_string(), "/T", "/F"])
            .output()
            .expect("the external kill runs");
        assert!(killed.status.success(), "the external kill landed");

        let exit = pty
            .wait_for_exit(Duration::from_secs(20))
            .expect("an abrupt exit is still an exit");
        assert!(
            exit.code.is_some(),
            "even a killed shell reports a code, saw {exit:?}"
        );
        assert!(!pty.is_running());
    }

    #[test]
    fn natural_exit_is_observed_with_its_code() {
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        send(&pty, "exit 7");
        let exit = pty
            .wait_for_exit(Duration::from_secs(30))
            .expect("the shell exits on its own");
        assert_eq!(exit.code, Some(7));
        assert_eq!(pty.exit_status(), Some(exit));
        assert!(!pty.is_running());

        // The output stream ends with the terminal, so a consumer loop can
        // finish cleanly instead of polling forever.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !pty.output_ended() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(100));
        }
        assert!(pty.output_ended(), "the output stream must end with the pty");
    }

    #[test]
    fn writing_after_the_end_is_a_structured_error() {
        let pty = start();
        expect_output(&pty, "PS", STARTUP.as_secs());

        send(&pty, "exit 3");
        pty.wait_for_exit(Duration::from_secs(30))
            .expect("the shell exits");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !pty.output_ended() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(100));
        }
        // Let the console host's side of the input pipe finish closing.
        thread::sleep(Duration::from_millis(300));

        let error = pty
            .write(b"Write-Host nope\r")
            .expect_err("a dead terminal rejects input");
        assert!(
            matches!(error, PtyError::Write { .. }),
            "a dead terminal reports a structured write failure, saw {error:?}"
        );
    }

    #[test]
    fn invalid_dimensions_are_rejected() {
        let zero_cols = Pty::spawn(shell().with_size(0, 24));
        let message = zero_cols
            .expect_err("a terminal with no columns cannot start")
            .to_string();
        assert!(message.contains("0x24"), "{message}");

        let zero_rows = Pty::spawn(shell().with_size(80, 0));
        let message = zero_rows
            .expect_err("a terminal with no rows cannot start")
            .to_string();
        assert!(message.contains("80x0"), "{message}");

        let pty = start();
        let error = pty
            .resize(80, 0)
            .expect_err("a resize to nothing is not hostable");
        assert!(error.to_string().contains("80x0"), "{error:?}");
        pty.kill().expect("cleanup");
    }

    #[test]
    fn missing_program_is_an_actionable_error() {
        let missing = "C:/definitely/not/here/lch-t02/missing.exe";
        let error = Pty::spawn(PtySpec::new(missing, std::env::temp_dir()))
            .expect_err("a missing program cannot start");
        let message = error.to_string();
        assert!(message.contains("missing.exe"), "{message}");
        assert!(message.contains("lch-t02"), "{message}");
    }
}
