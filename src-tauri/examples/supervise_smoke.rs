//! Manual smoke test for the T03 process supervisor.
//!
//! Not an automated test: it starts a real console process, hands control to the
//! operator, then stops the run and reports what the supervisor observed. Run it
//! on Windows to eyeball the safety blocker by hand:
//!
//! ```text
//! cargo run --example supervise_smoke
//! cargo run --example supervise_smoke -- cmd.exe /c ping -n 300 127.0.0.1
//! ```
//!
//! Exit code 0 means the run stopped and nothing was left behind. Exit code 1
//! means the tree outlived the stop — the failure this ticket exists to prevent.

use std::process::exit;

use local_console_hub_lib::process::{ManagedProcess, ProcessSpec, DEFAULT_STOP_TIMEOUT};

fn main() {
    let mut args = std::env::args().skip(1);
    let program = args.next().unwrap_or_else(|| "cmd.exe".to_string());
    let mut rest: Vec<String> = args.collect();
    if rest.is_empty() {
        // Something that stays alive until it is stopped, without needing any
        // install: ping to loopback for about five minutes.
        let ping = ["/c", "ping", "-n", "300", "127.0.0.1"];
        rest = ping.iter().map(|part| (*part).to_owned()).collect();
    }

    let cwd = std::env::current_dir().expect("current directory");
    let spec = ProcessSpec::new(program.as_str(), cwd).with_args(rest);
    println!("starting: {program} {:?}", spec.args);

    let process = ManagedProcess::spawn(spec).expect("spawn");
    let pid = process.pid();
    println!("pid: {pid} (running: {})", process.is_running());
    match process.tree_pids() {
        Ok(pids) => println!("tree before stop: {pids:?}"),
        Err(error) => println!("tree before stop: {error}"),
    }

    wait_for_enter("press Enter to stop the run");

    match process.stop(DEFAULT_STOP_TIMEOUT) {
        Ok(report) => println!(
            "stop: outcome={:?} exit={:?} graceful_delivered={}",
            report.outcome, report.exit, report.graceful_delivered
        ),
        Err(error) => {
            eprintln!("stop failed: {error}");
            exit(1);
        }
    }

    let survivors = process.tree_pids().unwrap_or_default();
    println!("tree after stop: {survivors:?}");
    if survivors.is_empty() {
        println!("OK — the managed tree is gone");
    } else {
        println!("FAIL — {} process(es) survived the stop", survivors.len());
        exit(1);
    }
}

fn wait_for_enter(prompt: &str) {
    use std::io::BufRead;

    println!("{prompt}");
    let stdin = std::io::stdin();
    let mut line = String::new();
    let _ = stdin.lock().read_line(&mut line);
}
