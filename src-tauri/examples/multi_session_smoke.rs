//! Multi-session stress smoke test for the T11 end-to-end gate (#12).
//!
//! The `src-tauri/tests/mvp_matrix.rs` suite is the fast, bounded half of the
//! matrix: it runs on every push and asserts the same *rules* with small
//! inputs. This is the loud half — the run a person does before packaging,
//! where the sessions are pushed past what a unit-sized input reaches:
//!
//! - four managed sessions at once (three terminals, one service);
//! - one terminal flooded past the 256 KiB / 5000-line scrollback ceiling
//!   (`docs/LOGGING.md` §9) while the others are expected to keep answering;
//! - one session stopped and one restarted with the rest still live;
//! - a per-session report at the end — pid, run id, what the buffer kept,
//!   what it discarded, and which log files the run left on disk.
//!
//! Run it on Windows:
//!
//! ```text
//! cargo run --example multi_session_smoke
//! ```
//!
//! Exit code 0 means every step passed. Exit code 1 means a step this ticket
//! exists to prove failed — the failing step is named on stderr, with the
//! evidence it had.
//!
//! It drives [`SessionCore`] directly rather than the window: what this test
//! is about is what several sessions do to each other, and the window adds a
//! webview to the picture without adding a fact. The app-data layout is set
//! up under a scratch directory and removed at the end, so a run leaves the
//! machine's real `%LOCALAPPDATA%\LocalConsoleHub` alone.
//!
//! Every awaited marker is assembled inside the shell at run time
//! (`("A-" + "B")`): the shell echoes the command line as it is typed, and a
//! literal marker in the command text would match the echo before the command
//! ever ran — the same reason `pty_smoke.rs` does this.

use std::path::PathBuf;
use std::process::exit;
use std::time::{Duration, Instant};

use local_console_hub_lib::config::{
    AppPaths, EffectiveLogMode, EffectiveLogging, LogSource, SessionConfig, SessionType,
};
use local_console_hub_lib::logging::{session_run_files, BufferSummary, LogRoots};
use local_console_hub_lib::session::core::SessionCore;
use local_console_hub_lib::session::state::SessionStatus;

/// The absolute path `pty::tests` uses, so this does not depend on `PATH`.
const POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.exe";

/// A service that stays alive until it is stopped: the same long-running
/// console child the T03 smoke test and `session::core::tests` use.
const SERVICE: &str = "cmd.exe /c ping -n 300 127.0.0.1";

/// The scrollback ceiling a session is documented to hold
/// (`docs/LOGGING.md` §9, `DEFAULT_BUFFER_LIMITS`).
const SCROLLBACK_CEILING: usize = 256 * 1024;
const SCROLLBACK_LINES: usize = 5_000;

/// Lines the flood produces. Comfortably past both ceilings, so a session
/// that is keeping everything would be a failure rather than a fast machine.
const FLOOD_LINES: usize = 12_000;

/// Enough padding that the byte ceiling is reached before the line ceiling.
const FLOOD_PAYLOAD: &str = "LCH-STRESS-PADDING-0123456789-0123456789-0123456789";

/// Cold shells on a busy machine take a while to render a first prompt.
const STARTUP: Duration = Duration::from_secs(60);

fn fail(name: &str, why: &str) -> ! {
    eprintln!("FAIL [{name}] {why}");
    exit(1);
}

fn ok(name: &str, detail: &str) {
    println!("{name}: ok — {detail}");
}

fn step(name: &str) -> Instant {
    println!("--- {name}");
    Instant::now()
}

fn config(id: &str, name: &str, session_type: SessionType) -> SessionConfig {
    let shell = format!("{POWERSHELL} -NoLogo -NoProfile");
    SessionConfig {
        id: id.to_owned(),
        name: name.to_owned(),
        session_type,
        cwd: std::env::current_dir().ok(),
        command: match session_type {
            SessionType::Service => Some(SERVICE.to_owned()),
            SessionType::Terminal => None,
        },
        url: None,
        port: None,
        purpose: None,
        close_impact: None,
        shell: match session_type {
            SessionType::Terminal => Some(shell),
            SessionType::Service => None,
        },
        initial_command: None,
        // Each session's persistence is what the report at the end shows:
        // terminals keep their scrollback and write nothing
        // (`docs/LOGGING.md` §1.2), the service writes one file for its run.
        logging: match session_type {
            SessionType::Terminal => EffectiveLogging {
                mode: EffectiveLogMode::Off,
                source: LogSource::None,
                external_path: None,
            },
            SessionType::Service => EffectiveLogging {
                mode: EffectiveLogMode::Always,
                source: LogSource::Captured,
                external_path: None,
            },
        },
    }
}

/// A marker only the executed command can produce (see the module note).
fn marker(prefix: &str, suffix: &str) -> String {
    format!(r#"Write-Host ("{prefix}-" + "{suffix}")"#)
}

struct Driver {
    core: SessionCore,
    root: PathBuf,
    paths: AppPaths,
}

impl Driver {
    fn scrollback(&self, id: &str) -> String {
        self.core
            .terminal_buffer(id)
            .unwrap_or_default()
            .iter()
            .map(|chunk| chunk.text())
            .collect()
    }

    fn summary(&self, id: &str) -> BufferSummary {
        self.core
            .snapshot(id)
            .map(|runtime| runtime.buffer)
            .unwrap_or_default()
    }

    fn status(&self, id: &str) -> SessionStatus {
        self.core
            .snapshot(id)
            .map(|runtime| runtime.status)
            .unwrap_or(SessionStatus::Stopped)
    }

    fn run_id(&self, id: &str) -> String {
        self.core
            .snapshot(id)
            .and_then(|runtime| runtime.run_id)
            .map(|run_id| run_id.to_string())
            .unwrap_or_default()
    }

    fn pid(&self, id: &str) -> Option<u32> {
        self.core.snapshot(id).and_then(|runtime| runtime.pid)
    }

    fn send(&self, id: &str, line: &str) {
        self.core
            .terminal_write(id, format!("{line}\r").as_bytes())
            .unwrap_or_else(|error| fail(&format!("write to {id}"), &error.to_string()));
    }

    /// Wait for `id`'s shell to render `marker`, with the transcript on
    /// failure so the report says what the shell did instead of only what was
    /// expected of it.
    fn expect(&self, id: &str, marker: &str) {
        let deadline = Instant::now() + STARTUP;
        while Instant::now() < deadline {
            if self.scrollback(id).contains(marker) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        fail(
            &format!("{id} showing `{marker}`"),
            &format!("timed out; saw {:?}", tail(&self.scrollback(id))),
        );
    }

    /// The same wait, but against a far shorter deadline: used where the
    /// question is "did the *other* sessions keep working while one was busy",
    /// and a generous timeout would answer it wrong.
    fn expect_within(&self, id: &str, marker: &str, budget: Duration) -> Result<Duration, String> {
        let started = Instant::now();
        while started.elapsed() < budget {
            if self.scrollback(id).contains(marker) {
                return Ok(started.elapsed());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        Err(format!("{id} never showed `{marker}` within {:?}", budget))
    }
}

/// Where one session stood at a moment in time.
///
/// The integration suite tracks the same three values and cannot share this
/// with the suite: `tests/` and `examples/` are separate cargo targets with no
/// module between them. The duplication is deliberate rather than overlooked —
/// this one reports step by step and exits on a failure, while the suite
/// asserts and returns, and folding one into the other would cost more than
/// the twenty lines it saves.
struct Identity {
    id: String,
    run_id: String,
    pid: Option<u32>,
}

impl Identity {
    fn of(driver: &Driver, id: &str) -> Self {
        Identity {
            id: id.to_owned(),
            run_id: driver.run_id(id),
            pid: driver.pid(id),
        }
    }

    /// `id`'s identity in `set`, or a failure naming what was missing.
    fn in_set<'a>(set: &'a [Identity], id: &str) -> &'a Identity {
        set.iter()
            .find(|identity| identity.id == id)
            .unwrap_or_else(|| fail("identity lookup", &format!("`{id}` was running before")))
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The last few hundred characters of a transcript, for an error message that
/// stays readable.
fn tail(text: &str) -> String {
    let cut = text.len().saturating_sub(400);
    text[cut..].to_owned()
}

fn main() {
    let root = std::env::temp_dir().join(format!("lch-t11-stress-{}", std::process::id()));
    let paths = AppPaths::new(&root, &root);
    std::fs::create_dir_all(&root).unwrap_or_else(|error| fail("scratch dir", &error.to_string()));

    let core = SessionCore::without_listener().with_log_roots(LogRoots::from_app_paths(&paths));
    let driver = Driver {
        core,
        root: root.clone(),
        paths,
    };

    println!("scratch app-data: {}", root.display());

    // ---------------------------------------------------------------- setup
    step("register four sessions");
    for (id, name, kind) in [
        ("term-a", "Terminal A", SessionType::Terminal),
        ("term-b", "Terminal B", SessionType::Terminal),
        ("term-c", "Terminal C", SessionType::Terminal),
        ("svc", "Service", SessionType::Service),
    ] {
        driver
            .core
            .register(config(id, name, kind))
            .unwrap_or_else(|error| fail("register", &error.to_string()));
    }
    let ids = ["term-a", "term-b", "term-c", "svc"];
    if driver.core.summary().total != ids.len() {
        fail("register", "not every session was registered");
    }
    if driver.core.summary().running != 0 {
        fail("register", "registration started something");
    }
    ok("register four sessions", "4 registered, 0 running");

    // ------------------------------------------------------------ start all
    let started = step("start all four");
    for id in ids {
        driver
            .core
            .start(id)
            .unwrap_or_else(|error| fail(&format!("start {id}"), &error.to_string()));
    }
    for id in ids {
        if driver.status(id) != SessionStatus::Running {
            fail("start all four", &format!("{id} is not running"));
        }
    }
    let run_ids: Vec<String> = ids.map(|id| driver.run_id(id)).into();
    let pids: Vec<Option<u32>> = ids.map(|id| driver.pid(id)).into();
    if pids.iter().any(Option::is_none) {
        fail("start all four", &format!("a session has no pid: {pids:?}"));
    }
    let mut unique = run_ids.clone();
    unique.sort();
    unique.dedup();
    if unique.len() != ids.len() {
        fail("start all four", &format!("run ids repeat: {run_ids:?}"));
    }
    if driver.core.summary().running != ids.len() {
        fail("start all four", "the summary disagrees with the snapshots");
    }
    ok(
        "start all four",
        &format!("{:?} in {:?}", pids, started.elapsed()),
    );

    // ------------------------------------------------------- type into each
    step("each terminal answers its own input");
    for (id, suffix) in [("term-a", "A"), ("term-b", "B"), ("term-c", "C")] {
        driver.expect(id, "PS");
        driver.send(id, &marker("LCH-STRESS", suffix));
    }
    for (id, suffix) in [("term-a", "A"), ("term-b", "B"), ("term-c", "C")] {
        driver.expect(id, &format!("LCH-STRESS-{suffix}"));
    }
    // …and nobody rendered anybody else's.
    for (id, other) in [("term-a", "B"), ("term-b", "C"), ("term-c", "A")] {
        if driver
            .scrollback(id)
            .contains(&format!("LCH-STRESS-{other}"))
        {
            fail(
                "each terminal answers its own input",
                &format!("{id} rendered the output meant for another terminal"),
            );
        }
    }
    ok(
        "each terminal answers its own input",
        "each shell answered its own marker and nobody else's",
    );

    // ------------------------------------------------------------- flooding
    step("flood one terminal");
    let before: Vec<Identity> = ids.iter().map(|id| Identity::of(&driver, id)).collect();
    let flood = format!(
        r#"for ($i = 0; $i -lt {FLOOD_LINES}; $i++) {{ Write-Host "{FLOOD_PAYLOAD}" }}; Write-Host ("LCH-STRESS-" + "FLOODDONE")"#
    );
    let flood_started = Instant::now();
    driver.send("term-c", &flood);

    // The quiet terminals, while the loud one is writing.
    let deadline = Duration::from_secs(30);
    driver.send("term-a", &marker("LCH-STRESS", "QUIET"));
    let answered = driver
        .expect_within("term-a", "LCH-STRESS-QUIET", deadline)
        .unwrap_or_else(|why| fail("a quiet terminal while another floods", &why));
    ok(
        "a quiet terminal while another floods",
        &format!("term-a answered in {answered:?} while term-c was flooding"),
    );

    driver.expect("term-c", "LCH-STRESS-FLOODDONE");
    let flooded = driver.summary("term-c");
    ok(
        "flood completes",
        &format!(
            "{FLOOD_LINES} lines in {:?} — kept {} bytes / {} lines, discarded {} bytes",
            flood_started.elapsed(),
            flooded.bytes,
            flooded.lines,
            flooded.dropped_bytes
        ),
    );

    // The ceiling is a ceiling: a session that kept the whole flood, or that
    // lost bytes without saying so, is the failure this step exists for.
    if flooded.bytes > SCROLLBACK_CEILING {
        fail(
            "bounded scrollback",
            &format!(
                "term-c holds {} bytes, above the {SCROLLBACK_CEILING}-byte ceiling",
                flooded.bytes
            ),
        );
    }
    if flooded.lines > SCROLLBACK_LINES {
        fail(
            "bounded scrollback",
            &format!(
                "term-c holds {} lines, above the {SCROLLBACK_LINES}-line ceiling",
                flooded.lines
            ),
        );
    }
    let produced = FLOOD_LINES * (FLOOD_PAYLOAD.len() + 2);
    if flooded.dropped_bytes == 0 {
        fail(
            "bounded scrollback",
            &format!(
                "term-c reports nothing discarded after ~{produced} bytes of output, so either it \
                 kept it all or the loss was silent"
            ),
        );
    }
    ok(
        "bounded scrollback",
        &format!(
            "discarded {} bytes and said so, instead of growing without limit",
            flooded.dropped_bytes
        ),
    );

    // The flood stayed in its own session.
    for id in ["term-a", "term-b", "svc"] {
        if driver.summary(id).dropped_bytes != 0 {
            fail(
                "the flood stayed in its own session",
                &format!("{id} discarded bytes although it never flooded"),
            );
        }
    }
    ok(
        "the flood stayed in its own session",
        "no other session discarded anything",
    );

    // ------------------------------------------------------------- stop one
    step("stop one session, keep the rest");
    driver
        .core
        .stop("term-b")
        .unwrap_or_else(|error| fail("stop term-b", &error.to_string()));
    if !matches!(
        driver.status("term-b"),
        SessionStatus::Stopped | SessionStatus::Exited
    ) {
        fail("stop one session", "term-b did not end");
    }
    for id in ["term-a", "term-c", "svc"] {
        let held = Identity::in_set(&before, id);
        if driver.status(id) != SessionStatus::Running {
            fail("stop one session", &format!("{id} stopped with it"));
        }
        if driver.run_id(id) != held.run_id || driver.pid(id) != held.pid {
            fail(
                "stop one session",
                &format!("{id} changed run or process, so the stop reached it"),
            );
        }
    }
    ok(
        "stop one session, keep the rest",
        "term-b ended; term-a, term-c and svc kept their own run and pid",
    );

    // ----------------------------------------------------------- restart it
    step("restart the stopped session");
    driver
        .core
        .restart("term-b")
        .unwrap_or_else(|error| fail("restart term-b", &error.to_string()));
    if driver.status("term-b") != SessionStatus::Running {
        fail("restart the stopped session", "term-b did not come back");
    }
    let stopped = Identity::in_set(&before, "term-b");
    if driver.run_id("term-b") == stopped.run_id {
        fail(
            "restart the stopped session",
            "the restarted run kept the old run id",
        );
    }
    if driver.pid("term-b") == stopped.pid {
        fail(
            "restart the stopped session",
            "the restarted run kept the old process",
        );
    }
    // The scrollback is the session's, not the run's (LOGGING §8).
    driver.expect("term-b", "LCH-STRESS-B");
    driver.send("term-b", &marker("LCH-STRESS", "RESTARTED"));
    driver.expect("term-b", "LCH-STRESS-RESTARTED");
    ok(
        "restart the stopped session",
        "a new run and a new process, with the earlier scrollback still readable",
    );

    // -------------------------------------------------------------- report
    step("per-session report");
    for id in ids {
        let summary = driver.summary(id);
        let log_files = session_run_files(&driver.paths.logs_dir, "log", Some(id));
        println!(
            "  {id:<7} {:<9} pid {:<6} run {:<16} buffer {:>7} B / {:>5} lines, dropped {:>7} B, {} log file(s)",
            format!("{:?}", driver.status(id)),
            driver
                .pid(id)
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "-".to_owned()),
            driver.run_id(id),
            summary.bytes,
            summary.lines,
            summary.dropped_bytes,
            log_files.len(),
        );
    }
    ok("per-session report", "printed above");

    // --------------------------------------------------------------- teardown
    step("stop everything");
    for id in ids {
        if driver.status(id) == SessionStatus::Running {
            driver
                .core
                .stop(id)
                .unwrap_or_else(|error| fail("stop all", &error.to_string()));
        }
    }
    if driver.core.summary().running != 0 {
        fail("stop everything", "a session is still running");
    }
    ok("stop everything", "no session is left running");

    println!();
    println!("OK — four sessions ran together, one flooded past the ceiling, one stopped and restarted, and the rest were untouched");
}
