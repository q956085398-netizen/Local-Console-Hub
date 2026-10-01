//! The console this process is attached to — and borrowing another one's.
//!
//! A console belongs to the **process**, not to a thread and not to a call:
//! `AttachConsole` and `FreeConsole` change which console this process is on, a
//! process is on one at a time, and everything that asks Windows about "the
//! console" is answered about that one. Two places in this crate change it for
//! the length of a single question — finding a `display: window` run's console
//! window ([`crate::window::console_window`]) and delivering a `CTRL_BREAK` to
//! a run on a console of its own ([`crate::process`]'s graceful step) — and
//! both do it through here, for three reasons.
//!
//! ## One of them at a time
//!
//! A `CTRL_BREAK` is raised *in the console this process is attached to*, so a
//! request reported as delivered can still have gone to the wrong run: a window
//! lookup that has attached to another run's console at that moment puts the
//! event in *that* console, and the stop's first rung signs for a delivery
//! nobody received (D-007's ladder is sound only while "delivered" means this
//! run). Every operation here holds one lock for as long as the answer depends
//! on which console this process is on — the attach and detach *and* the
//! questions asked in between.
//!
//! ## Never leaving the caller without its console
//!
//! Losing a console is not the same as never having had one. A process without
//! a console is given one — a *window* of its own to a machine whose default
//! terminal application is Windows Terminal — for every console child it starts
//! afterwards (D-035), so a lookup that quietly gave up the caller's console
//! turns every later run and helper process into a stray window on the desktop.
//! That is what the `display: window` path did (#66): one console lookup during
//! a standalone run's window wait was enough to leave a developer's desktop with
//! a dozen `cmd.exe` windows at the end of a `cargo test` run, most of them from
//! *other* tests' children.
//!
//! So a console is only let go of when it can be taken back, and the way back is
//! read **before** anything is detached. Two conditions keep a caller on what it
//! already has:
//!
//! - a console **with a window** is not given up at all. It is the user's own
//!   terminal, and a window lookup is not worth trading it for even with a way
//!   back — the same refusal [`crate::window::console_window`] has always made;
//! - a console **nobody else is on** is refused because letting go of it
//!   destroys it: the way back is another process still attached, and there is
//!   none.
//!
//! What is left is the case this module exists for — the windowless console a
//! tool-spawned process is given, with the shell that spawned it still on it:
//! the borrow is made, the question asked, and restoration tries every original
//! peer. If they all exit or detach, that console can be destroyed while we are
//! away: restoration is diagnosed as failed, not reported as a successful borrow.
//!
//! ## And nobody starts a process in between
//!
//! A child is given the console of whoever starts it, and what a run is meant
//! to inherit is *this* process's console (D-035). A borrow is a moment when
//! this process is attached to somebody else's console — or, between the detach
//! and the attach, to none at all — so a run started by another thread at that
//! moment is handed a console of its own, which is a window on the desktop and
//! the very thing the run's creation flags exist to avoid. Starting a run
//! therefore takes the same claim (see [`crate::process`]'s backend), and the
//! window lookup waits for it.
//!
//! Windows-only on purpose: the console model is Windows', the two callers are
//! in Windows backends, and the window layer's non-Windows stand-in already
//! answers "no console window" for the platforms that have no such thing.

use std::sync::{Mutex, MutexGuard};

/// A held claim on this process's console state.
///
/// Taken for as long as an answer depends on *which* console this process is
/// attached to, and held by whoever is about to change it. Dropping it releases
/// the claim; nothing is attached or detached by the drop itself.
#[derive(Debug)]
pub(crate) struct Console {
    own_pid: u32,
    /// Released on drop. Never read: holding it *is* the claim.
    _serialized: MutexGuard<'static, ()>,
}

impl Console {
    /// Claim this process's console state.
    pub(crate) fn claim() -> Console {
        static LOCK: Mutex<()> = Mutex::new(());
        Console {
            own_pid: std::process::id(),
            _serialized: LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
        }
    }

    /// Borrow the console `pid` is attached to, for the length of `ask`.
    ///
    /// `None` means the borrow was refused, attachment failed, or restoration
    /// failed (reported to stderr). `Some` means `ask` ran and restoration
    /// succeeded. Peers can all exit or detach while we are away; a console
    /// destroyed that way cannot be restored, so success is not guaranteed.
    pub(crate) fn borrow<T>(&self, pid: u32, ask: impl FnOnce() -> T) -> Option<T> {
        use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole, GetConsoleWindow};

        // A console window is this process's own console, and it is not traded
        // for a lookup.
        if !unsafe { GetConsoleWindow() }.is_null() {
            return None;
        }

        let route = route_back(self.own_pid, &members()?)?;

        unsafe {
            // A process is on one console at a time, so its own has to be let go
            // of before another can be attached — and from here until `Restore`
            // runs, this process is on nobody's.
            FreeConsole();
            let mut restore = Restore(Some(route));
            if AttachConsole(pid) == 0 {
                // The run has no console, or ended while this was being asked.
                // The drop guard still restores the caller.
                return None;
            }
            let answer = ask();
            // Report success only after restoration; Drop covers unwinding.
            restore.finish().then_some(answer)
        }
    }
}

/// Every pid attached to this process's console, empty when it has none.
///
/// This — not `GetConsoleWindow` — is the honest reading of "does this process
/// have a console": one created for a process whose parent had none has no
/// window of its own (`GetConsoleWindow` answers null, D-035) while still being
/// a console a `CTRL_BREAK` travels through and a child would inherit.
fn members() -> Option<Vec<u32>> {
    use windows_sys::Win32::Foundation::{GetLastError, ERROR_INVALID_HANDLE};
    use windows_sys::Win32::System::Console::GetConsoleProcessList;

    let mut list = vec![0u32; MAX_CONSOLE_MEMBERS];
    let count = unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) };
    if count == 0 {
        // No console is a legitimate starting state. Any other read failure
        // must refuse borrowing, not claim there is no console to lose.
        return (unsafe { GetLastError() } == ERROR_INVALID_HANDLE).then(Vec::new);
    }
    listed_members(list, count as usize)
}

/// An oversized read writes no pids at all, not a partial list. Refuse it
/// rather than detaching based on the zero-filled buffer (D-037).
fn listed_members(mut list: Vec<u32>, count: usize) -> Option<Vec<u32>> {
    if count > list.len() {
        return None;
    }
    list.truncate(count);
    (!list.contains(&0)).then_some(list)
}

/// How many console members are read.
const MAX_CONSOLE_MEMBERS: usize = 64;

/// How this process gets back to the console it let go of.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Route {
    /// It had no console of its own, so there is nothing to put back.
    NothingToLose,
    /// These processes were on that console, so attaching to any one of them is
    /// attaching back: the console outlives this process's absence from it
    /// while one of them is still on it.
    Via(Vec<u32>),
}

/// The way back to this process's own console, or `None` when there is none.
///
/// Read *before* anything is detached, because it is the whole condition for
/// detaching at all: a console this process is alone on dies with its last
/// member, so letting go of it is not a borrow but a loss.
///
/// Every other member is a way back rather than one of them, because a console
/// is shared with processes that come and go — a shell's children, a test
/// binary's helpers — and the one that was picked can be gone by the time the
/// borrow ends. `Restore` tries them in turn.
fn route_back(own_pid: u32, members: &[u32]) -> Option<Route> {
    let others: Vec<u32> = members
        .iter()
        .copied()
        .filter(|pid| *pid != own_pid)
        .collect();
    if !others.is_empty() {
        return Some(Route::Via(others));
    }
    // Alone on its console (or on none at all): only the second is a console
    // this process is free to leave.
    members.is_empty().then_some(Route::NothingToLose)
}

/// Puts this process back on the console it came from when it goes out of
/// scope — including while a panic is unwinding.
#[derive(Debug)]
struct Restore(Option<Route>);

impl Restore {
    fn finish(&mut self) -> bool {
        use windows_sys::Win32::System::Console::{AttachConsole, FreeConsole};

        let Some(route) = self.0.take() else {
            return true;
        };
        unsafe { FreeConsole() };
        let restored = restore_via(&route, |pid| unsafe { AttachConsole(pid) != 0 });
        if !restored {
            // All peers may exit or detach during a borrow. That destroys the
            // old console: it cannot be reconstructed by allocating a new one.
            use std::io::Write;
            // A closed stderr must not cause a second panic during unwinding.
            let _ = writeln!(std::io::stderr().lock(), "Local Console Hub: could not restore the caller's console after borrowing; no original console member could be attached");
        }
        restored
    }
}

impl Drop for Restore {
    fn drop(&mut self) {
        self.finish();
    }
}

fn restore_via(route: &Route, mut attach: impl FnMut(u32) -> bool) -> bool {
    match route {
        Route::NothingToLose => true,
        Route::Via(candidates) => candidates.iter().copied().any(&mut attach),
    }
}

/// Whether a console could be borrowed right now — that is, whether [`borrow`]
/// would answer `Some` for a console this process has none of.
///
/// [`borrow`]: Console::borrow
///
/// For tests. Whether a lookup through somebody else's console is a question
/// this process can ask at all is the *environment's* answer — a process whose
/// console has a window, or which is alone on its console, cannot ask — so a
/// test that needs a borrowed answer has to find out which of the two it is
/// running under instead of assuming one.
#[cfg(test)]
pub(crate) fn can_borrow() -> bool {
    use windows_sys::Win32::System::Console::GetConsoleWindow;

    let console = Console::claim();
    unsafe { GetConsoleWindow() }.is_null()
        && members().is_some_and(|members| route_back(console.own_pid, &members).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::sync::mpsc;
    use std::time::Duration;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    /// This process's console membership, read under the claim.
    fn own_members() -> Vec<u32> {
        let _claimed = Console::claim();
        members().expect("the caller's console membership can be read")
    }

    #[test]
    fn an_oversized_console_list_is_not_a_partial_list() {
        for count in [63, 64] {
            assert_eq!(listed_members(vec![7; 64], count).unwrap().len(), count);
        }
        assert_eq!(listed_members(vec![0; 64], 65), None);
        assert_eq!(listed_members(vec![0; 64], 1), None);
    }

    #[test]
    fn restoration_tries_surviving_members_and_reports_total_failure() {
        let route = Route::Via(vec![8, 9]);
        let mut tried = Vec::new();
        assert!(restore_via(&route, |pid| {
            tried.push(pid);
            pid == 9
        }));
        assert_eq!(tried, [8, 9]);
        assert!(!restore_via(&route, |_| false));
        assert!(restore_via(&Route::NothingToLose, |_| {
            panic!("a caller with no console must not attach to one")
        }));
    }

    /// A console this process is alone on is one it cannot be put back on, so
    /// asking is refused rather than paid for with the caller's console.
    #[test]
    fn a_console_nobody_else_is_on_is_not_given_up() {
        assert_eq!(route_back(7, &[]), Some(Route::NothingToLose));
        assert_eq!(route_back(7, &[8, 7]), Some(Route::Via(vec![8])));
        assert_eq!(
            route_back(7, &[8, 9, 7]),
            Some(Route::Via(vec![8, 9])),
            "every other member is a way back, not just the first"
        );
        assert_eq!(
            route_back(7, &[7]),
            None,
            "the last member of a console cannot give it up and get it back"
        );
    }

    /// The property the defect rested on: whatever this process's console is,
    /// a borrow leaves it on that same console. The child is given a console of
    /// its own (`CREATE_NO_WINDOW`: a real console, no window on the desktop),
    /// so what is borrowed is genuinely somebody else's console and the
    /// membership read while attached names the child — not this process.
    #[test]
    fn a_borrow_leaves_this_process_on_its_own_console() {
        let before = own_members();
        let borrowable = can_borrow();

        let mut child = std::process::Command::new("ping.exe")
            .args(["-n", "30", "127.0.0.1"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .expect("the fixture console process starts");
        let pid = child.id();

        // A console is not there the instant its process is: the first attach
        // to a just-started one fails, and succeeds a moment later. Waiting for
        // it is the same shape as the window wait above the run — the question
        // is bounded and its answer is honest either way.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let borrowed = loop {
            let console = Console::claim();
            let borrowed =
                console.borrow(pid, || members().expect("the borrowed console is readable"));
            drop(console);
            if !borrowable || borrowed.is_some() || std::time::Instant::now() >= deadline {
                break borrowed;
            }
            std::thread::sleep(Duration::from_millis(50));
        };

        let failed_attach = {
            let console = Console::claim();
            console.borrow(0, || panic!("an invalid target must not be queried"))
        };
        let panicked = if borrowed.is_some() {
            Some(std::panic::catch_unwind(|| {
                let console = Console::claim();
                console.borrow(pid, || panic!("fixture panic during a console borrow"))
            }))
        } else {
            None
        };
        let after = own_members();
        let _ = child.kill();
        let _ = child.wait();
        let own_pid = std::process::id();

        assert!(failed_attach.is_none(), "attachment to pid 0 must fail");
        if let Some(panicked) = panicked {
            assert!(panicked.is_err(), "the borrowed ask must have unwound");
        }
        if borrowable {
            let borrowed = borrowed.expect("a borrowable console is borrowed");
            assert!(
                borrowed.contains(&pid),
                "the borrow was on the fixture's console: {borrowed:?}"
            );
        }

        // Which members the console has *besides* this process is not this
        // test's to pin — a `cargo test` run has other console children joining
        // and leaving it the whole time. What is this test's is this process:
        // still on a console if it started on one, on none if it started on
        // none, and not left on the one it borrowed.
        if before.is_empty() {
            assert!(
                after.is_empty(),
                "a process with no console of its own must not come back with one: {after:?}"
            );
        } else {
            assert!(
                after.contains(&own_pid),
                "a borrow must leave this process on a console: {after:?}"
            );
        }
        if !before.contains(&pid) {
            assert!(
                !after.contains(&pid),
                "a borrow must not leave this process on the console it borrowed: {after:?}"
            );
        }
    }

    /// The window lookup and the graceful stop change the same process-wide
    /// state, so one of them waits for the other: a `CTRL_BREAK` raised while
    /// another thread has attached elsewhere lands in the wrong console.
    #[test]
    fn the_console_is_held_by_one_asker_at_a_time() {
        let (held_tx, held_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();

        // Another consulter of the console, holding the claim until this test
        // releases it — the shape of a window lookup in flight.
        let holder = std::thread::spawn(move || {
            let _console = Console::claim();
            held_tx.send(()).expect("the holder is awaited");
            release_rx.recv().expect("the holder is released");
        });
        held_rx.recv().expect("the holder took the console");

        let waiter = std::thread::spawn(move || {
            let _console = Console::claim();
            done_tx.send(()).expect("the asker is awaited");
        });

        let got_in_while_held = done_rx.recv_timeout(Duration::from_millis(500)).is_ok();
        release_tx.send(()).expect("the holder is released");
        // An asker that never got in while the claim was held has to get in
        // once it is free; one that did is the failure reported below.
        let got_in_at_all =
            got_in_while_held || done_rx.recv_timeout(Duration::from_secs(5)).is_ok();
        let _ = holder.join();
        let _ = waiter.join();

        assert!(
            got_in_at_all,
            "the console has to be claimable again once the holder lets go"
        );
        assert!(
            !got_in_while_held,
            "two askers of one process-wide console must not run at once"
        );
    }
}
