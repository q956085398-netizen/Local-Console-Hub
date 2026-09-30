//! The automated half of the T11 end-to-end MVP matrix (#12).
//!
//! Everything below drives the *public* Session Core API the way the running
//! app does, against real processes: a config file on disk is loaded through
//! the same `config::load_from_file` the app's bootstrap uses, the registry is
//! given the same `LogRoots` layout, and the sessions are real PowerShell
//! shells on ConPTY and a real supervised service.
//!
//! It lives in `tests/` rather than beside a module because the properties it
//! asserts are cross-module by nature — they are about what several sessions
//! do to *each other*, and about the path from a config file to a running
//! process. The unit suites next to each module already cover that module's
//! own behaviour; this is the composition.
//!
//! ## What it deliberately does not repeat
//!
//! The process-safety ladder (job-object tree kill, graceful-then-force,
//! restart barrier, "an unrelated same-named process is untouched") is
//! asserted in `process::tests`, and the PTY contract (Unicode, ANSI,
//! Ctrl+C, resize, high-volume output) in `pty::tests` and
//! `session::core::terminal_tests`. Re-asserting them here would only make
//! the matrix slower, not stronger. What this file adds is the part none of
//! those can reach: **several sessions at once**, through the bootstrap path,
//! with the app-data layout attached.
//!
//! ## Windows
//!
//! Gated to Windows the way `pty::tests` is: ConPTY, `cmd.exe` and the
//! PowerShell path below are all Windows facts, and a suite that cannot pass
//! elsewhere should say so by not compiling, not by failing. CI runs
//! `cargo test` on `windows-latest`, so it is always exercised where it
//! matters.
#![cfg(windows)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use local_console_hub_lib::config::{
    load_from_file, AppPaths, DisplayMode, EffectiveLogMode, LifecycleOwner, LogSource,
    SessionConfig, SessionConfigDto, SessionType,
};
use local_console_hub_lib::logging::{session_run_files, BufferSummary, LogRoots, LogState};
use local_console_hub_lib::session::core::SessionCore;
use local_console_hub_lib::session::state::SessionStatus;

/// The absolute path `pty::tests` uses, so these do not depend on how `PATH`
/// happens to be set in the environment the suite runs in.
const POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.exe";

/// A service that stays alive until it is stopped, so a test can observe it
/// while it is genuinely `Running`. The same long-running console child the
/// T03 smoke test and `core::tests` use.
const LONG_RUNNING: &str = "cmd.exe /c ping -n 120 127.0.0.1";

/// Cold shells on a busy machine take a while to render a first prompt.
const STARTUP: Duration = Duration::from_secs(40);

/// A scratch app-data layout that removes itself.
///
/// Repeated here rather than shared: `logging::test_support::TempDir` is
/// `#[cfg(test)]`, and an integration test links the library *without* that
/// cfg, so it cannot see it. The name is unique by construction (process id,
/// a monotonic counter, the clock) for the reason `app::tests::TempConfig`
/// documents — two of these tests run at once, and a name derived from the
/// clock alone can collide on a runner with coarse time resolution.
struct Fixture {
    root: PathBuf,
    paths: AppPaths,
    core: SessionCore,
}

impl Fixture {
    /// Write `config` as the app's `config.yaml`, then load and register it
    /// exactly the way the app's bootstrap does.
    ///
    /// This is the seam the ticket is about: nothing here reaches into Session
    /// Core's internals, and nothing calls a test-only constructor. If the app
    /// could not get from this file to these sessions, neither could this.
    fn new(config: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};

        static SEQUENCE: AtomicU32 = AtomicU32::new(0);

        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after the epoch")
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "lch-t11-{}-{unique}-{sequence}",
            std::process::id()
        ));

        let paths = AppPaths::new(&root, &root);
        std::fs::create_dir_all(
            paths
                .config_file
                .parent()
                .expect("the config file has a folder"),
        )
        .expect("the app-data layout is creatable");
        std::fs::write(&paths.config_file, config).expect("the config file is writable");

        let loaded = load_from_file(&paths.config_file).expect("the config file is readable");
        let core = SessionCore::without_listener().with_log_roots(LogRoots::from_app_paths(&paths));
        for session in loaded.sessions {
            core.register(session).expect("the config registers");
        }

        Fixture { root, paths, core }
    }

    /// The config this fixture was built from, as a map id → config.
    fn config(&self, id: &str) -> SessionConfig {
        self.core
            .configs()
            .into_iter()
            .find(|config| config.id == id)
            .unwrap_or_else(|| panic!("`{id}` is a registered session"))
    }

    fn status(&self, id: &str) -> SessionStatus {
        self.core
            .snapshot(id)
            .unwrap_or_else(|| panic!("`{id}` has a snapshot"))
            .status
    }

    fn pid(&self, id: &str) -> u32 {
        self.core
            .snapshot(id)
            .and_then(|runtime| runtime.pid)
            .unwrap_or_else(|| panic!("`{id}` is running, so it has a pid"))
    }

    fn run_id(&self, id: &str) -> String {
        self.core
            .snapshot(id)
            .and_then(|runtime| runtime.run_id)
            .map(|run_id| run_id.to_string())
            .unwrap_or_else(|| panic!("`{id}` is running, so it has a run id"))
    }

    /// Every `.log` file the Hub has written for `id`.
    fn log_files(&self, id: &str) -> Vec<PathBuf> {
        session_run_files(&self.paths.logs_dir, "log", Some(id))
            .into_iter()
            .map(|file| file.path)
            .collect()
    }

    /// Everything `id`'s terminal has taken from its shell so far.
    fn scrollback(&self, id: &str) -> String {
        self.core
            .terminal_buffer(id)
            .unwrap_or_default()
            .iter()
            .map(|chunk| chunk.text())
            .collect()
    }

    /// The scrollback summary — what a view reads to say "was any of this
    /// lost?" without holding the scrollback itself (spec §4).
    fn buffer_summary(&self, id: &str) -> BufferSummary {
        self.core
            .snapshot(id)
            .unwrap_or_else(|| panic!("`{id}` has a snapshot"))
            .buffer
    }

    /// Send one line the way a terminal sends Enter.
    fn send(&self, id: &str, line: &str) {
        self.core
            .terminal_write(id, format!("{line}\r").as_bytes())
            .unwrap_or_else(|error| panic!("input reaches `{id}`: {error}"));
    }

    /// Wait for `marker` to be rendered by `id`'s shell.
    fn expect_in_scrollback(&self, id: &str, marker: &str) {
        let deadline = Instant::now() + STARTUP;
        while Instant::now() < deadline {
            if self.scrollback(id).contains(marker) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        // The transcript, so a failure says what the shell did instead of only
        // what was expected of it.
        panic!(
            "`{id}` never showed `{marker}`, saw: {:?}",
            self.scrollback(id)
        );
    }

    /// Wait until `check` holds, so an assertion is about what happened rather
    /// than about how long it took.
    fn wait_until(&self, what: &str, mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + STARTUP;
        while !check() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Best effort: a leaked temp folder is untidy, not a failure.
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Where one session stood at a moment in time.
///
/// The multi-session tests ask one question over and over — "did this session
/// keep the *same* run and the *same* process?" — and the three values only
/// mean anything together. As a bare `(String, String, u32)` the reader has to
/// remember which slot is which, twice, including inside a `find` closure.
struct Identity {
    id: String,
    run_id: String,
    pid: u32,
}

impl Identity {
    fn of(fixture: &Fixture, id: &str) -> Self {
        Identity {
            id: id.to_owned(),
            run_id: fixture.run_id(id),
            pid: fixture.pid(id),
        }
    }

    /// `id`'s identity in `set`, or a panic saying it was not running then.
    fn in_set<'a>(set: &'a [Identity], id: &str) -> &'a Identity {
        set.iter()
            .find(|identity| identity.id == id)
            .unwrap_or_else(|| panic!("`{id}` was running before the stop"))
    }

    /// Assert the fixture still reports this run and this process for it.
    fn assert_kept(&self, fixture: &Fixture) {
        assert_eq!(
            (fixture.run_id(&self.id), fixture.pid(&self.id)),
            (self.run_id.clone(), self.pid),
            "`{}` kept its run and its process",
            self.id
        );
    }
}

/// Two interactive terminals and one supervised service, with the logging
/// modes the MVP defaults call for: terminals persist nothing
/// (`docs/LOGGING.md` §1.2), a captured service persists while it runs.
///
/// `command` is a parameter so a test can choose a quiet service or a noisy
/// one without a second fixture.
///
/// `term-a` carries a `purpose` and a `close_impact` and `term-b` carries
/// neither: both are free-text fields either type may own (D-027), so one
/// terminal exercises "the configured words survive the whole path" and the
/// other "omitting them leaves them absent" rather than each asserting only
/// half the rule. The service keeps both, so the test can still assert that
/// close-impact text is the session's own.
fn config_yaml(service_command: &str) -> String {
    let shell = format!("{POWERSHELL} -NoLogo -NoProfile");
    format!(
        "\
sessions:
  - id: term-a
    name: Terminal A
    type: terminal
    shell: '{shell}'
    cwd: .
    purpose: an interactive terminal
    close_impact: ends this shell only; other managed sessions keep running
    logging:
      mode: off
      source: none

  - id: term-b
    name: Terminal B
    type: terminal
    shell: '{shell}'
    cwd: .
    logging:
      mode: off
      source: none

  - id: svc
    name: Service
    type: service
    cwd: .
    command: '{service_command}'
    port: 8000
    url: http://127.0.0.1:8000
    purpose: supervised service
    close_impact: stops the service its callers depend on
    logging:
      mode: always
      source: captured
"
    )
}

/// A marker that only the *executed* command can produce.
///
/// The shell echoes the command line as it is typed, so a literal marker in
/// the command text would be satisfied by the echo before the command ever
/// ran. Assembling it inside the shell means the typed text never contains it.
fn marker(prefix: &str, suffix: &str) -> String {
    format!(r#"Write-Host ("{prefix}-" + "{suffix}")"#)
}

/// The text a replay carries.
///
/// An attachment's chunks hold *bytes*, base64-encoded, because a chunk
/// boundary can fall inside a multi-byte character and decoding per chunk
/// would render the seam as a replacement character (D-019). A test that only
/// looks for ASCII markers can afford to decode each chunk on its own; the
/// view, which cannot, hands the bytes to the terminal instead.
fn replay_text(chunks: &[local_console_hub_lib::session::terminal::RetainedChunk]) -> String {
    use base64::Engine;

    let mut text = String::new();
    for chunk in chunks {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&chunk.data)
            .expect("a retained chunk carries base64");
        text.push_str(&String::from_utf8_lossy(&bytes));
    }
    text
}

// ---------------------------------------------------------------------------
// The bootstrap path
// ---------------------------------------------------------------------------

/// A config file becomes a set of registered, *stopped* sessions.
///
/// The registration half of the app's startup, driven through the same
/// `AppPaths` → `load_from_file` → `register` path `app::bootstrap` uses.
/// Nothing is started: registering a session is not a lifecycle operation
/// (spec §3).
#[test]
fn the_bootstrap_path_turns_a_config_file_into_stopped_sessions() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));

    let ids: Vec<String> = fixture
        .core
        .configs()
        .into_iter()
        .map(|config| config.id)
        .collect();
    assert_eq!(
        ids,
        vec!["svc", "term-a", "term-b"],
        "every configured session is registered, in id order"
    );

    for id in ["term-a", "term-b", "svc"] {
        assert_eq!(
            fixture.status(id),
            SessionStatus::Stopped,
            "`{id}` is registered but not started"
        );
        assert!(
            fixture.core.snapshot(id).is_some_and(|r| r.pid.is_none()),
            "`{id}` has no process before anything asked for one"
        );
    }

    let summary = fixture.core.summary();
    assert_eq!(summary.total, 3);
    assert_eq!(summary.running, 0, "startup runs nothing");

    // The config half of a session reaches the UI from the same registry the
    // snapshots come from (D-020), so a row the window can render is one
    // Session Core can act on.
    let svc = fixture.config("svc");
    assert_eq!(svc.port, Some(8000));
    assert_eq!(
        svc.close_impact.as_deref(),
        Some("stops the service its callers depend on"),
        "close impact is the session's own text, not filler"
    );

    // A running terminal states the same two things, and they reach the
    // window unchanged (D-027). The header callout and the Details card read
    // `close_impact` without consulting the session type, so a terminal whose
    // config could not carry it rendered `关闭影响 —` in the live window while
    // the fixture workspace rendered the reference's sentence (#38).
    let term = fixture.config("term-a");
    assert_eq!(term.purpose.as_deref(), Some("an interactive terminal"));
    assert_eq!(
        term.close_impact.as_deref(),
        Some("ends this shell only; other managed sessions keep running"),
        "a terminal's close impact is its own text too"
    );

    let plain = fixture.config("term-b");
    assert_eq!(plain.purpose, None, "an omitted purpose stays absent");
    assert_eq!(
        plain.close_impact, None,
        "an omitted close impact stays absent"
    );

    // The window never sees `SessionConfig`: `list_session_configs` maps it
    // through `SessionConfigDto`, and that payload is what the header reads
    // (D-020). A field the model keeps but the DTO drops would render as `—`
    // in the live window, so the hop is asserted rather than assumed.
    let wire =
        serde_json::to_value(SessionConfigDto::from(&term)).expect("the config DTO serializes");
    assert_eq!(wire["purpose"], "an interactive terminal");
    assert_eq!(
        wire["closeImpact"],
        "ends this shell only; other managed sessions keep running"
    );
}

// ---------------------------------------------------------------------------
// Multi-session
// ---------------------------------------------------------------------------

/// Three sessions run at once, each with its own run and its own process.
///
/// The matrix row "at least three concurrent sessions" (spec §16), with the
/// identities that make "independent" checkable rather than merely stated.
#[test]
fn three_concurrent_sessions_hold_distinct_runs_and_processes() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));

    for id in ["term-a", "term-b", "svc"] {
        fixture.core.start(id).expect("start succeeds");
    }

    for id in ["term-a", "term-b", "svc"] {
        assert_eq!(fixture.status(id), SessionStatus::Running, "`{id}` runs");
    }

    let run_ids: Vec<String> = ["term-a", "term-b", "svc"]
        .map(|id| fixture.run_id(id))
        .into_iter()
        .collect();
    let mut distinct = run_ids.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 3, "three runs, three run ids: {run_ids:?}");

    let pids: Vec<u32> = ["term-a", "term-b", "svc"].map(|id| fixture.pid(id)).into();
    let mut distinct = pids.clone();
    distinct.sort();
    distinct.dedup();
    assert_eq!(
        distinct.len(),
        3,
        "three sessions, three processes: {pids:?}"
    );

    assert_eq!(fixture.core.summary().running, 3, "the summary counts them");
}

/// Each terminal answers its own input, and only its own.
///
/// Two shells reading from one process's stdin would show up here as one
/// terminal rendering the other's command — the failure that makes
/// "multi-session" worth testing at all.
#[test]
fn each_terminal_answers_only_its_own_input() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    fixture.core.start("term-a").expect("start succeeds");
    fixture.core.start("term-b").expect("start succeeds");

    fixture.send("term-a", &marker("LCH-A", "ONLYA"));
    fixture.send("term-b", &marker("LCH-B", "ONLYB"));

    fixture.expect_in_scrollback("term-a", "LCH-A-ONLYA");
    fixture.expect_in_scrollback("term-b", "LCH-B-ONLYB");

    assert!(
        !fixture.scrollback("term-a").contains("LCH-B-ONLYB"),
        "term-a rendered the other terminal's output"
    );
    assert!(
        !fixture.scrollback("term-b").contains("LCH-A-ONLYA"),
        "term-b rendered the other terminal's output"
    );
}

/// Stopping one session leaves the others running, with the *same* process.
///
/// The matrix row "stopping one leaves others alive" (spec §16). The second
/// half matters as much as the first: a stop that reached a sibling would
/// show as a changed pid or a dead run even while the status still read
/// `Running`.
#[test]
fn stopping_one_session_leaves_the_others_untouched() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    for id in ["term-a", "term-b", "svc"] {
        fixture.core.start(id).expect("start succeeds");
    }
    let before: Vec<Identity> = ["term-a", "term-b", "svc"]
        .iter()
        .map(|id| Identity::of(&fixture, id))
        .collect();

    fixture.core.stop("term-b").expect("stop succeeds");

    assert!(
        matches!(
            fixture.status("term-b"),
            SessionStatus::Stopped | SessionStatus::Exited
        ),
        "the stopped session ends, saw {:?}",
        fixture.status("term-b")
    );
    assert!(
        fixture
            .core
            .snapshot("term-b")
            .is_some_and(|r| r.pid.is_none()),
        "the stopped session has no process"
    );

    for id in ["term-a", "svc"] {
        assert_eq!(
            fixture.status(id),
            SessionStatus::Running,
            "`{id}` survives"
        );
        Identity::in_set(&before, id).assert_kept(&fixture);
    }
    assert_eq!(fixture.core.summary().running, 2);

    // The survivors are not merely still listed — they still work.
    fixture.send("term-a", &marker("LCH-STILL", "ALIVE"));
    fixture.expect_in_scrollback("term-a", "LCH-STILL-ALIVE");
}

/// A session that floods its terminal does not make another unusable.
///
/// The matrix row "noisy output in one does not make another unusable"
/// (spec §16, §14 "one high-output session must not freeze the whole UI").
/// The noisy session is the one that *stops answering* if its output is
/// mishandled, and the quiet one is what proves the noise stayed in its own
/// session's buffer.
#[test]
fn a_noisy_session_does_not_disturb_the_quiet_one() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    fixture.core.start("term-a").expect("start succeeds");
    fixture.core.start("term-b").expect("start succeeds");

    fixture.expect_in_scrollback("term-a", "PS");
    fixture.expect_in_scrollback("term-b", "PS");

    // 4000 lines is far more than one batch and more than a quiet session
    // will ever hold; it is not enough to reach the 256 KiB ceiling, which is
    // the stress example's job, not this suite's.
    fixture.send(
        "term-a",
        r#"for ($i = 0; $i -lt 4000; $i++) { Write-Host "LCH-NOISE-$i" }; Write-Host ("LCH-NOISE-" + "DONE")"#,
    );

    // The quiet terminal answers while the loud one is still writing.
    fixture.send("term-b", &marker("LCH-QUIET", "WORKS"));
    fixture.expect_in_scrollback("term-b", "LCH-QUIET-WORKS");

    // And the loud one finishes rather than being wedged by its own output.
    fixture.expect_in_scrollback("term-a", "LCH-NOISE-DONE");

    assert_eq!(fixture.status("term-a"), SessionStatus::Running);
    assert_eq!(fixture.status("term-b"), SessionStatus::Running);
}

// ---------------------------------------------------------------------------
// Terminal retention
// ---------------------------------------------------------------------------

/// A terminal keeps its run while nothing is watching, and a view that
/// arrives later is shown what it missed.
///
/// This is what "switching sessions does not destroy the PTY" and "hide to
/// the tray and restore keeps the PTY alive" mean at the layer that owns the
/// session: no view is attached for the whole quiet stretch, the shell keeps
/// running, and the attachment replays the bytes that accumulated (D-019).
///
/// The window being hidden is not the same thing as no view being attached —
/// hiding is the tray's, and it never touches Session Core — but a session
/// that could not survive having no reader would fail the tray case too, and
/// this is the half a test can drive.
#[test]
fn a_terminal_keeps_running_with_no_view_attached_and_replays_on_attach() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    fixture.core.start("term-a").expect("start succeeds");
    fixture.expect_in_scrollback("term-a", "PS");
    let run_id = fixture.run_id("term-a");

    // Work done while nobody is attached.
    fixture.send("term-a", &marker("LCH-UNSEEN", "WORK"));
    fixture.expect_in_scrollback("term-a", "LCH-UNSEEN-WORK");

    // A view appears. What it gets back is the retained scrollback, not just
    // what happens from now on.
    let attached = fixture
        .core
        .terminal_attachment("term-a")
        .expect("the session attaches");
    let replayed = replay_text(&attached.chunks);

    assert!(attached.pty_attached, "the PTY is still there");
    assert!(
        replayed.contains("LCH-UNSEEN-WORK"),
        "the replay covers what happened before the view arrived, saw: {replayed:?}"
    );
    assert!(
        attached.emitted > 0,
        "the attachment names an offset it reaches"
    );
    assert_eq!(
        fixture.run_id("term-a"),
        run_id,
        "the run was never replaced"
    );
    assert_eq!(fixture.status("term-a"), SessionStatus::Running);
}

// ---------------------------------------------------------------------------
// Logging defaults
// ---------------------------------------------------------------------------

/// The MVP logging defaults, end to end on the app-data layout: a terminal
/// persists nothing, a captured service writes one file per run, and the
/// status surface says which is which.
///
/// `docs/LOGGING.md` §1.2 ("交互会话默认…不生成日志文件"), §3 (`always` ⇒
/// everything the run writes goes to a file as it arrives) and §1.4 ("不能
/// 出现「用户以为没记录，但实际上后台一直在写」的情况" — asked in both
/// directions here: nothing is written for the terminal, and the service's
/// status says it is capturing rather than claiming it is off).
#[test]
fn a_terminal_persists_nothing_and_a_captured_service_writes_one_file_per_run() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));

    fixture.core.start("term-a").expect("start succeeds");
    fixture.expect_in_scrollback("term-a", "PS");
    fixture.send("term-a", &marker("LCH-LOG", "TERM"));
    fixture.expect_in_scrollback("term-a", "LCH-LOG-TERM");

    let terminal_status = fixture
        .core
        .log_status("term-a")
        .expect("a registered session has a log status");
    assert_eq!(
        terminal_status.state,
        LogState::Off,
        "an interactive terminal is not being persisted"
    );
    assert!(terminal_status.log_file.is_none());
    assert!(
        fixture.log_files("term-a").is_empty(),
        "an `off` terminal wrote a file anyway: {:?}",
        fixture.log_files("term-a")
    );

    // The service, started and then stopped, leaves exactly one file.
    fixture.core.start("svc").expect("start succeeds");
    fixture.wait_until("the service to be capturing", || {
        fixture
            .core
            .log_status("svc")
            .is_some_and(|status| status.state == LogState::Capturing)
    });

    fixture.core.stop("svc").expect("stop succeeds");
    fixture.wait_until("the run's log to be filed", || {
        !fixture.log_files("svc").is_empty()
    });

    let files = fixture.log_files("svc");
    assert_eq!(files.len(), 1, "one run is one file, saw: {files:?}");
    assert!(
        files[0].starts_with(&fixture.paths.logs_dir),
        "the log lives under the app-data logs root, not beside the executable: {:?}",
        files[0]
    );
    assert!(
        files[0]
            .iter()
            .any(|part| part.to_string_lossy().starts_with("20")),
        "the layout keeps the month directory a human browses by (D-011): {:?}",
        files[0]
    );
}

/// A restart moves a session onto a new run, and the scrollback survives it.
///
/// `docs/LOGGING.md` §8 keeps the buffer across a restart, and D-019 counts
/// the attachment's offset from the *current* run, so a view that attaches
/// after a restart sees the earlier run's bytes and a fresh offset without
/// either being wrong. This is the pairing the two rules describe; the
/// process layer's own restart barrier is asserted in `process::tests`.
#[test]
fn a_restart_starts_a_new_run_and_keeps_the_scrollback() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    fixture.core.start("term-a").expect("start succeeds");
    fixture.expect_in_scrollback("term-a", "PS");
    fixture.send("term-a", &marker("LCH-BEFORE", "RESTART"));
    fixture.expect_in_scrollback("term-a", "LCH-BEFORE-RESTART");

    let old_run = fixture.run_id("term-a");
    let old_pid = fixture.pid("term-a");

    fixture.core.restart("term-a").expect("restart succeeds");

    assert_ne!(fixture.run_id("term-a"), old_run, "a restart is a new run");
    assert_ne!(fixture.pid("term-a"), old_pid, "and a new process");
    assert_eq!(fixture.status("term-a"), SessionStatus::Running);

    fixture.expect_in_scrollback("term-a", "LCH-BEFORE-RESTART");
    let attached = fixture
        .core
        .terminal_attachment("term-a")
        .expect("the session attaches");
    let replayed = replay_text(&attached.chunks);
    assert!(
        replayed.contains("LCH-BEFORE-RESTART"),
        "the scrollback is the session's, not the run's (LOGGING §8)"
    );

    // The new run answers, and its buffer is the same bounded one.
    fixture.send("term-a", &marker("LCH-AFTER", "RESTART"));
    fixture.expect_in_scrollback("term-a", "LCH-AFTER-RESTART");
}

/// The buffer summary answers "was any of this lost?" from a snapshot, and
/// says no while the output fits.
///
/// Spec §4: the snapshot carries a *summary*, not the scrollback, so a window
/// that only needs "is there anything to show, and did we drop any of it?"
/// never asks for the bytes. The positive case — that the ceiling is real and
/// reports itself — is the stress example's, because reaching it means
/// megabytes of process output.
#[test]
fn a_snapshot_reports_an_intact_buffer_without_carrying_it() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    fixture.core.start("term-a").expect("start succeeds");
    fixture.expect_in_scrollback("term-a", "PS");
    fixture.send("term-a", &marker("LCH-BUFFER", "SMALL"));
    fixture.expect_in_scrollback("term-a", "LCH-BUFFER-SMALL");

    let summary = fixture.buffer_summary("term-a");
    assert!(summary.bytes > 0, "the session has taken some output");
    assert!(summary.lines > 0);
    assert_eq!(
        summary.dropped_bytes, 0,
        "nothing was discarded, so a view is not looking at a truncated history"
    );

    // The summary is not the scrollback: the snapshot itself is small and
    // holds no chunk bytes.
    let serialized = serde_json::to_string(
        &fixture
            .core
            .snapshot("term-a")
            .expect("the session has a snapshot"),
    )
    .expect("a snapshot serializes");
    assert!(
        serialized.len() < 4 * 1024,
        "a snapshot stays a summary, saw {} bytes",
        serialized.len()
    );
    assert!(serialized.contains("droppedBytes"));
}

/// The paths a session hands the Logs tab are its own, and a path that does
/// not exist is reported rather than offered.
///
/// The MVP matrix's "user can locate a persisted run" (spec §1), and the rule
/// D-022 added: presence is a read-time fact, so a log removed by hand is
/// answered exactly like one retention swept.
#[test]
fn a_sessions_log_path_is_resolved_from_the_session_not_from_a_caller() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    fixture.core.start("svc").expect("start succeeds");
    fixture.wait_until("the service to be capturing", || {
        fixture
            .core
            .log_status("svc")
            .is_some_and(|status| status.state == LogState::Capturing)
    });
    fixture.core.stop("svc").expect("stop succeeds");
    fixture.wait_until("the run's log to be filed", || {
        !fixture.log_files("svc").is_empty()
    });

    let folder = fixture
        .core
        .log_folder_path("svc", None)
        .expect("the session resolves its own folder");
    assert!(
        folder.starts_with(&fixture.paths.logs_dir),
        "the folder is under this app-data root: {folder:?}"
    );

    let status = fixture
        .core
        .log_status("svc")
        .expect("a registered session has a log status");
    assert_eq!(status.state, LogState::Capturing);
    assert!(
        status.log_file_present,
        "the file this status names is on disk right now"
    );

    // Remove it out from under the Hub: the same question now answers no,
    // because it is asked of the filesystem and not remembered (D-022).
    let file = PathBuf::from(
        status
            .log_file
            .as_deref()
            .expect("a capturing run names its file"),
    );
    assert!(file.exists(), "the named file is the one that was written");
    std::fs::remove_file(&file).expect("the log file is removable");

    let after = fixture
        .core
        .log_status("svc")
        .expect("the status is still answerable");
    assert!(
        !after.log_file_present,
        "a file removed by hand is answered like one retention swept"
    );
    assert_eq!(
        after.log_file.as_deref(),
        status.log_file.as_deref(),
        "the run still names where its log would be"
    );
    // The folder outlives the file, which is what keeps "打开目录" useful on a
    // run whose log is gone (D-022, UI_STYLE_GUIDE §8).
    assert!(
        folder.is_dir(),
        "the folder the log lived in is still openable: {folder:?}"
    );
}

// ---------------------------------------------------------------------------
// The quick entry (#62)
// ---------------------------------------------------------------------------

/// "新建 PowerShell" on top of a workspace that came from a config file.
///
/// The composition this file exists for: a real config file goes through the
/// bootstrap path, a temporary terminal is created *on top of it* and runs a
/// real shell, and removing it takes the workspace back to exactly what the
/// file says — including after a fresh load, which is what "真正退出不恢复未保存
/// 的临时终端" means for a session that was never written down (stories 6–9,
/// 21–23).
#[test]
fn the_quick_entry_adds_a_terminal_on_top_of_a_loaded_workspace() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    let config_before =
        std::fs::read(&fixture.paths.config_file).expect("the config file is readable");

    let created = fixture
        .core
        .create_temporary_terminal(Some(&fixture.root.to_string_lossy()))
        .expect("the quick entry creates a terminal");
    let id = created.config.id.clone();

    // A real shell, in the registry, alongside the configured sessions.
    assert_eq!(created.runtime.status, SessionStatus::Running);
    assert_eq!(
        created.config.cwd.as_ref(),
        Some(&fixture.root),
        "the directory the entry named is the one the shell opened in"
    );
    let entries = fixture.core.entries();
    assert_eq!(
        entries.len(),
        4,
        "three configured sessions and the new one"
    );
    let entry = entries
        .iter()
        .find(|entry| entry.config.id == id)
        .expect("the created session is in the registry");
    assert!(entry.temporary, "the row is the removable kind");
    assert_eq!(
        entries.iter().filter(|entry| !entry.temporary).count(),
        3,
        "the configured sessions did not become temporary"
    );

    // And it is a terminal the user can actually work in (story 18).
    fixture.send(&id, &marker("LCH-T62", "LIVE"));
    fixture.expect_in_scrollback(&id, "LCH-T62-LIVE");

    fixture.send(&id, "exit");
    fixture.wait_until("the temporary shell to end", || {
        fixture.status(&id) == SessionStatus::Exited
    });
    assert!(
        fixture.scrollback(&id).contains("LCH-T62-LIVE"),
        "an ended temporary terminal keeps the output of its run"
    );

    fixture
        .core
        .remove_session(&id)
        .expect("an ended temporary terminal is removable");
    assert_eq!(fixture.core.snapshots().len(), 3);
    assert!(
        fixture.core.terminal_buffer(&id).is_none(),
        "a removed terminal's scrollback is released"
    );

    // The workspace is what the file says: nothing was written to it, and a
    // fresh load of the same file does not bring the temporary terminal back.
    assert_eq!(
        std::fs::read(&fixture.paths.config_file).expect("the config file is readable"),
        config_before,
        "the quick entry must not touch the config file"
    );
    let reloaded = load_from_file(&fixture.paths.config_file).expect("the config file is readable");
    let restored = SessionCore::without_listener();
    for session in reloaded.sessions {
        restored.register(session).expect("the config registers");
    }
    assert_eq!(
        restored.entries().len(),
        3,
        "a temporary terminal is never restored"
    );
    assert!(restored.entries().iter().all(|entry| !entry.temporary));
}

// ---------------------------------------------------------------------------
// Adding an application (#64)
// ---------------------------------------------------------------------------

/// "添加应用" against a real config file, on a workspace that is already using
/// it (#64, spec #59 decisions 10 and 15).
///
/// The composition this file exists for, on the other half of the config file:
/// the entry writes a *new* session into the file the bootstrap reads, a fresh
/// load finds it, opening it twice makes one real process, and the sessions
/// that were already there keep the runs they had — saving must not restart
/// anything (stories 30–34).
#[test]
fn an_added_application_joins_a_real_config_and_opens_once() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    let config_before =
        std::fs::read(&fixture.paths.config_file).expect("the config file is readable");

    // A session that is already running, so "saving does not restart anything"
    // has something real to be about.
    fixture
        .core
        .start("svc")
        .expect("the configured service starts");
    let run_id_before = fixture.run_id("svc");
    let pid_before = fixture.pid("svc");

    let added = local_console_hub_lib::app::applications::add_application(
        &fixture.core,
        Some(&fixture.paths.config_file),
        local_console_hub_lib::app::applications::NewApplication {
            name: "ComfyUI".to_owned(),
            cwd: fixture.root.to_string_lossy().into_owned(),
            command: LONG_RUNNING.to_owned(),
            purpose: Some("图像生成后端".to_owned()),
            close_impact: None,
            port: Some(8188),
            url: Some("http://127.0.0.1:8188/".to_owned()),
            // Left alone: the two #66 dimensions then come out as the
            // Hub-internal, Hub-managed entry every config had before them.
            display: None,
            lifecycle: None,
            logging: None,
        },
    )
    .expect("the application is added");

    let id = added.config.id.clone();
    assert_eq!(id, "comfyui", "the id comes from the name");
    assert_eq!(
        added.runtime.status,
        SessionStatus::Stopped,
        "adding saves a configuration; it does not start anything"
    );

    // Nothing that was already running moved.
    assert_eq!(fixture.status("svc"), SessionStatus::Running);
    assert_eq!(fixture.run_id("svc"), run_id_before);
    assert_eq!(fixture.pid("svc"), pid_before);

    // The file really grew, and the text that was there is still there.
    let after = std::fs::read(&fixture.paths.config_file).expect("the config file is readable");
    let after_text = String::from_utf8(after).expect("the config file is UTF-8");
    let before_text = String::from_utf8(config_before).expect("the config file is UTF-8");
    assert!(
        after_text.starts_with(&before_text[..before_text.len() - 1]),
        "an append must not rewrite what the user had:\n{after_text}"
    );

    // The real loading chain — the one the app's bootstrap runs — finds it.
    let reloaded = load_from_file(&fixture.paths.config_file).expect("the config file is readable");
    assert!(reloaded.errors.is_empty(), "{:?}", reloaded.errors);
    let ids: Vec<&str> = reloaded.sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["term-a", "term-b", "svc", "comfyui"]);
    let restored = SessionCore::without_listener();
    for session in reloaded.sessions {
        restored.register(session).expect("the config registers");
    }
    assert_eq!(
        restored.entries().len(),
        4,
        "the next start has four sessions"
    );

    // Opening it is one real process, however many times it is asked for.
    let opened = fixture
        .core
        .activate(&id)
        .expect("opening a stopped application starts it");
    assert!(opened.started);
    assert_eq!(opened.runtime.status, SessionStatus::Running);
    let pid = opened.runtime.pid.expect("a started run has a process");
    let run_id = opened
        .runtime
        .run_id
        .clone()
        .expect("a started run has an id");

    let again = fixture
        .core
        .activate(&id)
        .expect("opening a running application answers with it");
    assert!(
        !again.started,
        "the second open found the run already there"
    );
    assert_eq!(again.runtime.pid, Some(pid), "no second process");
    assert_eq!(again.runtime.run_id, Some(run_id));

    fixture.core.stop(&id).expect("cleanup");
    fixture.core.stop("svc").expect("cleanup");
}

// ---------------------------------------------------------------------------
// Saving a terminal's launch configuration (#65)
// ---------------------------------------------------------------------------

/// Save the running terminal `id` under `name`, through the app operation the
/// command calls.
fn save_terminal(fixture: &Fixture, id: &str, name: &str) -> SessionConfig {
    local_console_hub_lib::app::terminals::save_terminal_config(
        &fixture.core,
        Some(&fixture.paths.config_file),
        id,
        local_console_hub_lib::app::terminals::SaveTerminal {
            name: name.to_owned(),
            purpose: Some("跑构建的终端".to_owned()),
            close_impact: None,
        },
    )
    .expect("the terminal is saved")
    .config
}

/// "保存启动配置" on a workspace that came from a config file (#65, spec #59
/// decisions 4 and 6).
///
/// The composition this file exists for: a temporary terminal that is really
/// running is saved into the file the bootstrap reads, and the terminal that
/// was running is the *same* process afterwards — same run, same pid, same
/// scrollback, and no second row (H08's 核对原 PID). A fresh load of the file
/// then brings back a terminal that starts again in the saved shell and
/// directory, carrying none of what was typed into the old one (stories 24–25,
/// H08's 保存仅保留启动方式).
#[test]
fn a_saved_terminal_joins_a_real_config_and_keeps_its_run() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    // A service that is already running, so "saving restarts nothing" has
    // something else in the workspace to be about, not just the terminal.
    fixture
        .core
        .start("svc")
        .expect("the configured service starts");
    let service = Identity::of(&fixture, "svc");

    let created = fixture
        .core
        .create_temporary_terminal(Some(&fixture.root.to_string_lossy()))
        .expect("the quick entry creates a terminal");
    let id = created.config.id.clone();
    fixture.send(&id, &marker("LCH-T65", "LIVE"));
    fixture.expect_in_scrollback(&id, "LCH-T65-LIVE");
    let terminal = Identity::of(&fixture, &id);
    let scrollback_before = fixture.scrollback(&id);

    let saved = save_terminal(&fixture, &id, "项目终端");

    // The same session, the same run, the same process (story 25: 不复制、不重放).
    assert_eq!(saved.id, id);
    terminal.assert_kept(&fixture);
    assert_eq!(fixture.status(&id), SessionStatus::Running);

    // …and the same output on screen. Compared up to the marker rather than
    // byte-for-byte: a live shell repaints its prompt as soon as the command
    // it just ran finishes, so the buffer can gain a prompt at any moment —
    // including between the two readings here — and that is the shell's
    // timing, not something saving did. What saving could have changed is
    // everything up to and including what the user was reading when they
    // saved it, and that has to be identical. (A duplicate would show up as a
    // second marker, and a restart as a different run and process — which
    // `assert_kept` above and this comparison together rule out.)
    let through_marker = |text: &str| {
        let end = text
            .find("LCH-T65-LIVE")
            .map(|at| at + "LCH-T65-LIVE".len())
            .expect("the marker the terminal printed before the save");
        text[..end].to_owned()
    };
    assert_eq!(
        through_marker(&fixture.scrollback(&id)),
        through_marker(&scrollback_before),
        "the output the user was reading belongs to the run that produced it"
    );
    service.assert_kept(&fixture);
    assert_eq!(
        fixture.core.entries().len(),
        4,
        "three configured sessions and the saved terminal — not one more"
    );
    assert_eq!(fixture.config(&id).name, "项目终端");
    let entry = fixture
        .core
        .entries()
        .into_iter()
        .find(|entry| entry.config.id == id)
        .expect("the saved session is in the registry");
    assert!(
        !entry.temporary,
        "a saved terminal is no longer the window's to remove"
    );

    // The user ends the shell, the app exits, and the next start reads the file
    // and nothing else.
    fixture.send(&id, "exit");
    fixture.wait_until("the saved terminal to end", || {
        fixture.status(&id) == SessionStatus::Exited
    });
    fixture.core.stop("svc").expect("cleanup");

    let reloaded = load_from_file(&fixture.paths.config_file).expect("the config file is readable");
    assert!(reloaded.errors.is_empty(), "{:?}", reloaded.errors);
    let ids: Vec<&str> = reloaded.sessions.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        ["term-a", "term-b", "svc", id.as_str()],
        "the saved terminal joins the file rather than replacing anything"
    );
    let written = reloaded
        .sessions
        .iter()
        .find(|session| session.id == id)
        .expect("the saved terminal is in the file");
    assert_eq!(written.session_type, SessionType::Terminal);
    assert_eq!(written.name, "项目终端");
    assert_eq!(written.cwd.as_deref(), Some(fixture.root.as_path()));
    assert_eq!(
        written.initial_command, None,
        "a saved terminal must carry nothing to replay"
    );
    assert_eq!(written.logging.mode, EffectiveLogMode::Off);
    assert_eq!(written.logging.source, LogSource::None);

    // And it really starts: a new run, a new process, in the saved directory.
    let restored = SessionCore::without_listener();
    for session in reloaded.sessions {
        restored.register(session).expect("the config registers");
    }
    assert!(
        restored
            .entries()
            .into_iter()
            .find(|entry| entry.config.id == id)
            .is_some_and(|entry| !entry.temporary),
        "a session the file describes comes back configured, not temporary"
    );
    let restarted = restored
        .start(&id)
        .expect("the saved terminal starts again");
    assert_eq!(restarted.status, SessionStatus::Running);
    assert_ne!(
        restarted.run_id.map(|run_id| run_id.to_string()).as_deref(),
        Some(terminal.run_id.as_str()),
        "starting again is a new run, not the old one resumed"
    );
    assert_eq!(
        restored
            .configs()
            .into_iter()
            .find(|config| config.id == id)
            .and_then(|config| config.cwd),
        Some(fixture.root.clone()),
        "the rebuilt terminal opens where it was saved"
    );
    restored.stop(&id).expect("cleanup");
}

/// A save the config file refuses changes nothing (#65, decision 15).
///
/// The file is the user's, and it is the one thing that can refuse: while it is
/// broken, the terminal it was going to describe has to keep running, keep its
/// configuration, and stay removable after it ends — a refused save that marked
/// it saved would be the "虚报已保存" the decision forbids.
#[test]
fn a_refused_save_leaves_a_running_terminal_temporary() {
    let fixture = Fixture::new(&config_yaml(LONG_RUNNING));
    let created = fixture
        .core
        .create_temporary_terminal(Some(&fixture.root.to_string_lossy()))
        .expect("the quick entry creates a terminal");
    let id = created.config.id.clone();
    let terminal = Identity::of(&fixture, &id);

    // Someone edits the file into something this build cannot append to.
    let broken = "sessions: [ uh oh\n";
    std::fs::write(&fixture.paths.config_file, broken).expect("the fixture file is writable");

    let error = local_console_hub_lib::app::terminals::save_terminal_config(
        &fixture.core,
        Some(&fixture.paths.config_file),
        &id,
        local_console_hub_lib::app::terminals::SaveTerminal {
            name: "项目终端".to_owned(),
            purpose: None,
            close_impact: None,
        },
    )
    .expect_err("a broken file cannot be appended to");

    assert!(error.message.contains("YAML"), "{}", error.message);
    assert_eq!(
        std::fs::read_to_string(&fixture.paths.config_file).expect("readable"),
        broken,
        "a refusal must leave the user's file exactly as it was"
    );
    terminal.assert_kept(&fixture);
    assert_eq!(fixture.status(&id), SessionStatus::Running);
    assert_eq!(
        fixture.config(&id).name,
        created.config.name,
        "the refused save must not have renamed the terminal"
    );
    assert!(
        fixture
            .core
            .entries()
            .into_iter()
            .find(|entry| entry.config.id == id)
            .is_some_and(|entry| entry.temporary),
        "the terminal is still the window's to remove once it ends"
    );

    fixture.core.stop(&id).expect("cleanup");
}

/// A standalone-window application, from the real config file to the process
/// that outlives the registry that started it (#66, spec #59 decisions 11 and
/// 12).
///
/// The things this pins are the ones the ticket asks for, and none of them can
/// be shown by a stand-in: the entry loads from `config.yaml` like any other;
/// opening it starts exactly one process and a second open finds that one; the
/// run presents a window the Hub can find again; and when the Hub's registry
/// goes away — which is what exiting the app does — the application *and the
/// child it started* are still working.
#[test]
fn a_standalone_application_keeps_its_window_and_outlives_the_hub() {
    let work = StandaloneWork::new("lives-on");
    let fixture = Fixture::new(&work.config("launcher", "ComfyUI 启动器", None));

    let loaded = fixture.config("launcher");
    assert_eq!(loaded.display, DisplayMode::Window);
    assert_eq!(
        loaded.lifecycle,
        LifecycleOwner::Independent,
        "a standalone entry the user did not ask the Hub to manage is independent"
    );
    assert_eq!(
        loaded.logging.source,
        LogSource::None,
        "nothing about the application's console is captured"
    );

    // Opening it starts it once — and it is not hosted on a Hub console.
    let opened = fixture
        .core
        .activate("launcher")
        .expect("the application opens");
    assert!(
        opened.started,
        "nothing was running, so this call started it"
    );
    assert_eq!(opened.runtime.status, SessionStatus::Running);
    assert!(
        !opened.runtime.pty_attached,
        "a standalone application is not attached to a Hub terminal"
    );
    assert_eq!(
        fixture.buffer_summary("launcher").bytes,
        0,
        "no output is taken from it, because it kept its own console"
    );
    let pid = fixture.pid("launcher");

    // The instance the Hub holds is the one a second open answers with: no
    // second copy, whatever the user clicks.
    let again = fixture
        .core
        .activate("launcher")
        .expect("the second open answers");
    assert!(!again.started, "the run was already there");
    assert_eq!(
        again.runtime.pid,
        Some(pid),
        "the same process, not another"
    );

    // It presents a window the Hub can find again by the run it holds — the
    // console this application opened for itself (H10's held-instance part,
    // H12's "does not repeat a console the application already provides").
    fixture.wait_until("the application to present a window", || {
        fixture
            .core
            .application_window("launcher", Duration::ZERO)
            .is_some()
    });

    // The application and its child are both working: the child writes a tick
    // every second, so activity is observable rather than assumed.
    let running_ticks = work.wait_for_ticks(3);

    // Exiting the Hub: the registry — and with it every handle the Hub held on
    // this run — goes away. Nothing else is done; no stop, no detach.
    let core = fixture.core.clone();
    drop(fixture);
    drop(core);

    assert!(
        is_alive(pid),
        "a standalone application must still be running after the Hub exits"
    );
    let after_exit = work.wait_for_ticks(running_ticks + 3);
    assert!(
        after_exit > running_ticks,
        "the child it started must still be working after the Hub exits: \
         {running_ticks} ticks before, {after_exit} after"
    );

    // Cleanup, so the test leaves nothing behind.
    kill_tree(pid);
}

/// A standalone GUI application: the Hub finds the window it opened, and the
/// application ends itself by that window being closed (#66, spec #59 decisions
/// 11 and 12; the controllable-GUI half of H10/H12/H13).
///
/// This is the shape the mode exists for. The application has a window of its
/// own, so the Hub renders nothing for it and embeds nothing of it; the run it
/// started is the thing it can point at, and the window is found through that
/// run rather than through a name or a title. Closing the window is the
/// application ending *itself* — the one lifecycle action an unmanaged entry
/// has — and the Hub reports what happened instead of pretending it did it.
#[test]
fn a_standalone_gui_application_is_found_by_its_run_and_ends_through_its_own_window() {
    // A GUI program every Windows installation ships, with a top-level window
    // of its own and no save prompt on close. Absolute, like `POWERSHELL`
    // above, so the test does not depend on how `PATH` is set — and with
    // forward slashes, because this path lands in a YAML double-quoted scalar,
    // where a backslash is an escape (Windows accepts either separator).
    const GUI_APP: &str = "C:/Windows/System32/charmap.exe";

    let work = StandaloneWork::new("gui");
    let config = format!(
        "sessions:\n  - id: gui-app\n    name: 字符映射表\n    type: service\n    \
         cwd: {cwd}\n    command: \"{GUI_APP}\"\n    display: window\n",
        cwd = work
            .cwd()
            .display()
            .to_string()
            .replace(std::path::MAIN_SEPARATOR, "/"),
    );
    let fixture = Fixture::new(&config);

    fixture
        .core
        .activate("gui-app")
        .expect("the application opens");
    let pid = fixture.pid("gui-app");

    // The window it opened belongs to the run the Hub is holding — found by
    // process, not by title, and reported with the facts a caller can act on.
    let deadline = Instant::now() + STARTUP;
    let window = loop {
        match fixture
            .core
            .application_window("gui-app", Duration::from_millis(500))
        {
            Some(window) => break window,
            None if Instant::now() < deadline => continue,
            None => panic!("the GUI application never presented a window"),
        }
    };
    assert_eq!(
        window.pid, pid,
        "the window belongs to the run's own process"
    );
    assert!(window.visible, "it is on screen: {window:?}");
    assert!(!window.owned, "a top-level window is owned by nothing");

    // Bringing it forward is a real request with a real answer; Windows may
    // refuse the foreground change, and a refusal is an answer, not a failure.
    let outcome = window.focus();
    assert!(
        matches!(
            outcome,
            local_console_hub_lib::window::FocusOutcome::Focused
                | local_console_hub_lib::window::FocusOutcome::Refused
        ),
        "{outcome:?}"
    );

    // Closing the application's own window is the application ending itself.
    assert!(window.close(), "the close request is delivered");

    fixture.wait_until("the application to end itself", || {
        !is_alive(pid) && fixture.status("gui-app") != SessionStatus::Running
    });
    assert!(
        matches!(
            fixture.status("gui-app"),
            SessionStatus::Exited | SessionStatus::Error
        ),
        "a run that ends on its own is Exited (or Error with a failing code), saw {:?}",
        fixture.status("gui-app")
    );
    // Nothing was left behind by the ending: the tree is gone as well.
    assert!(!is_alive(pid));
    kill_tree(pid);
}

/// A standalone entry the user asked the Hub to manage is the Hub's to stop —
/// and one they did not is not.
#[test]
fn only_a_managed_standalone_entry_can_be_stopped_from_the_hub() {
    let work = StandaloneWork::new("managed");
    let config = [
        work.config("self-managed", "自己管理", None),
        work.entry("hub-managed", "Hub 管理", "    lifecycle: managed\n"),
    ]
    .concat();
    let fixture = Fixture::new(&config);
    assert_eq!(
        fixture.config("self-managed").lifecycle,
        LifecycleOwner::Independent
    );
    assert_eq!(
        fixture.config("hub-managed").lifecycle,
        LifecycleOwner::Managed
    );

    fixture.core.activate("self-managed").expect("it opens");
    fixture.core.activate("hub-managed").expect("it opens");
    let independent_pid = fixture.pid("self-managed");
    let managed_pid = fixture.pid("hub-managed");

    // The refusal is the answer for the independent one, and it names the way
    // out rather than only the reason.
    let refused = fixture
        .core
        .stop("self-managed")
        .expect_err("the Hub does not stop what it does not manage");
    assert!(refused.message.contains("lifecycle: managed"), "{refused}");
    assert_eq!(fixture.status("self-managed"), SessionStatus::Running);
    assert!(is_alive(independent_pid));

    // The managed one stops by the ordinary rules: asked gracefully first, and
    // the tree confirmed gone before the session reports it. Which of the two
    // endings it is (`Stopped` for a run that answered the request, `Exited`
    // for one that had to be terminated) is the stop's own business — a
    // console application's close is answered by the console host, and Windows
    // allows it the same seconds this stop waits before escalating (`D-007`).
    fixture
        .core
        .stop("hub-managed")
        .expect("a managed entry stops");
    assert!(
        !is_alive(managed_pid),
        "a managed standalone run is stopped exactly"
    );
    assert!(
        matches!(
            fixture.status("hub-managed"),
            SessionStatus::Stopped | SessionStatus::Exited
        ),
        "a stopped run is Stopped or Exited, saw {:?}",
        fixture.status("hub-managed")
    );

    kill_tree(independent_pid);
}

/// A configured application whose command is a batch launcher: the launch
/// method most portable applications actually ship.
///
/// Two files, because "the application is running" and "the process the Hub
/// started is running" are different facts and the standalone mode is about the
/// first one: the launcher starts a ticker and then stays alive itself, so the
/// run has both an own process and a child.
struct StandaloneWork {
    dir: PathBuf,
    ticks: PathBuf,
}

impl StandaloneWork {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "lch-standalone-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("the clock is after the epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("the scratch directory is created");
        let ticks = dir.join("ticks.txt");

        // The child: one line appended per second, for a bounded number of
        // iterations so a failing test cannot leave a ticker running forever.
        // `ping` is the sleep, because `timeout` refuses a redirected stdin.
        std::fs::write(
            dir.join("ticker.cmd"),
            format!(
                "@echo off\r\nfor /l %%i in (1,1,600) do (\r\n  \
                 echo tick>>\"{ticks}\"\r\n  ping -n 2 127.0.0.1 >nul\r\n)\r\n",
                ticks = ticks.display()
            ),
        )
        .expect("the ticker is written");

        // The launcher: starts the ticker detached, then keeps its own console
        // window open — which is what a user sees for a launcher-shaped
        // application, and what the Hub must be able to find again.
        std::fs::write(
            dir.join("launcher.cmd"),
            format!(
                "@echo off\r\nstart \"\" /b \"{child}\"\r\nping -n 300 127.0.0.1 >nul\r\n",
                child = dir.join("ticker.cmd").display()
            ),
        )
        .expect("the launcher is written");

        StandaloneWork { dir, ticks }
    }

    /// One entry for this fixture's launcher, in the phrase each test needs.
    ///
    /// Forward slashes in the paths: they land in a YAML document, and Windows
    /// accepts them in a path, so the config layer never sees a difference.
    fn entry(&self, id: &str, name: &str, extra: &str) -> String {
        let path = |path: PathBuf| {
            path.display()
                .to_string()
                .replace(std::path::MAIN_SEPARATOR, "/")
        };
        format!(
            "  - id: {id}\n    name: {name}\n    type: service\n    cwd: {cwd}\n    \
             command: \"{command}\"\n    display: window\n{extra}",
            cwd = path(self.dir.clone()),
            command = path(self.dir.join("launcher.cmd")),
        )
    }

    /// The scratch directory a configured command runs in.
    fn cwd(&self) -> &std::path::Path {
        &self.dir
    }

    /// A whole config file with this fixture's launcher as its only entry.
    fn config(&self, id: &str, name: &str, extra: Option<&str>) -> String {
        format!("sessions:\n{}", self.entry(id, name, extra.unwrap_or("")))
    }

    /// How many ticks the ticker has written so far.
    fn ticks(&self) -> usize {
        std::fs::read_to_string(&self.ticks)
            .map(|text| text.lines().count())
            .unwrap_or(0)
    }

    /// Wait for the ticker to reach `wanted` ticks, and answer with what it got.
    fn wait_for_ticks(&self, wanted: usize) -> usize {
        let deadline = Instant::now() + STARTUP;
        while self.ticks() < wanted {
            assert!(
                Instant::now() < deadline,
                "the ticker never reached {wanted} ticks, saw {}",
                self.ticks()
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        self.ticks()
    }
}

impl Drop for StandaloneWork {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Whether a pid belongs to a live process, for the checks that are about
/// processes the Hub is not holding.
///
/// `tasklist` rather than an `OpenProcess` call: this test links the library
/// from outside, and liveness of a foreign process is not something the library
/// offers (its own checks are internal). Asking the OS by name is the honest
/// version of the same question.
fn is_alive(pid: u32) -> bool {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .expect("tasklist runs");
    String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
}

/// End a process and everything it started, so a failed assertion cannot leave
/// a pinger or a ticker behind.
fn kill_tree(pid: u32) {
    let _ = std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}
