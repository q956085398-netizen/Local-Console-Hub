//! Manual smoke test for the T02 PTY backend.
//!
//! Scripted, not interactive: it hosts a real PowerShell on a ConPTY, drives
//! it through the acceptance-critical gestures (echo round trip, Unicode, ANSI
//! color, a 60-second ping interrupted by Ctrl+C, resize, a 1000-line burst,
//! clean exit with a code) and reports each step. Run it on Windows:
//!
//! ```text
//! cargo run --example pty_smoke
//! cargo run --example pty_smoke -- C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.exe -NoLogo -NoProfile
//! ```
//!
//! Exit code 0 means every step passed. Exit code 1 means the pty failed one
//! of the behaviors this ticket exists to prove — the failing step is named on
//! stderr.

use std::process::exit;
use std::time::{Duration, Instant};

use local_console_hub_lib::pty::{Pty, PtySpec};

/// The gesture every step shares: send a command, wait for its marker.
fn step(pty: &Pty, name: &str, command: &str, marker: &str) {
    pty.write(format!("{command}\r").as_bytes())
        .unwrap_or_else(|error| fail(name, &format!("sending the command: {error}")));
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = String::new();
    while Instant::now() < deadline {
        if let Some(chunk) = pty.read_output(Duration::from_millis(200)) {
            seen.push_str(&String::from_utf8_lossy(&chunk));
            if seen.contains(marker) {
                println!("{name}: ok");
                return;
            }
        }
    }
    fail(
        name,
        &format!("expected `{marker}` within 30s, saw:\n{seen}"),
    );
}

fn fail(name: &str, why: &str) -> ! {
    eprintln!("FAIL [{name}] {why}");
    exit(1);
}

fn main() {
    let mut args = std::env::args().skip(1);
    let program = args
        .next()
        .unwrap_or_else(|| r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.exe".to_owned());
    let rest: Vec<String> = args.collect();
    let shell_args = if rest.is_empty() {
        vec!["-NoLogo".to_owned(), "-NoProfile".to_owned()]
    } else {
        rest
    };

    let spec = PtySpec::new(program, std::env::current_dir().expect("cwd"))
        .with_args(shell_args)
        .with_size(100, 30);
    println!("hosting: {} {:?}", spec.program.display(), spec.args);

    let pty = Pty::spawn(spec).unwrap_or_else(|error| fail("spawn", &error.to_string()));
    println!("spawn: ok (pid {})", pty.pid());

    step(&pty, "first prompt", "", "PS");

    step(
        &pty,
        "echo round trip",
        "Write-Host LCH-SMOKE-ECHO",
        "LCH-SMOKE-ECHO",
    );

    step(
        &pty,
        "unicode round trip",
        "Write-Host LCH-SMOKE-你好-世界",
        "LCH-SMOKE-你好-世界",
    );

    step(
        &pty,
        "ansi color",
        "Write-Host -ForegroundColor Red LCH-SMOKE-RED",
        "LCH-SMOKE-RED",
    );

    // Ctrl+C: the ping would otherwise hold the console for a minute.
    step(
        &pty,
        "long command started",
        "Write-Host LCH-SMOKE-STARTED; ping -n 60 127.0.0.1 | Out-Null; Write-Host \
         LCH-SMOKE-NEVER",
        "LCH-SMOKE-STARTED",
    );
    pty.interrupt()
        .unwrap_or_else(|error| fail("ctrl+c", &format!("delivering the interrupt: {error}")));
    step(
        &pty,
        "ctrl+c resumes the shell",
        "Write-Host LCH-SMOKE-RESUMED",
        "LCH-SMOKE-RESUMED",
    );

    pty.resize(120, 34)
        .unwrap_or_else(|error| fail("resize", &error.to_string()));
    step(
        &pty,
        "resize reaches the shell",
        "Write-Host LCH-SMOKE-W-$($Host.UI.RawUI.BufferSize.Width)",
        "LCH-SMOKE-W-120",
    );

    step(
        &pty,
        "high-volume output",
        "for ($i = 0; $i -lt 1000; $i++) { Write-Host \"LCH-SMOKE-LINE $i\" }; Write-Host \
         LCH-SMOKE-BURST-DONE",
        "LCH-SMOKE-BURST-DONE",
    );

    pty.write(b"exit 5\r")
        .unwrap_or_else(|error| fail("exit", &format!("sending exit: {error}")));
    let exit = pty
        .wait_for_exit(Duration::from_secs(30))
        .unwrap_or_else(|| fail("exit", "the shell never exited"));
    if exit.code != Some(5) {
        fail("exit code", &format!("expected 5, saw {:?}", exit.code));
    }
    println!("exit code: ok ({:?})", exit.code);

    println!("OK — the pty passed every scripted step");
}
