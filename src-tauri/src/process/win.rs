//! Windows supervisor backend — job-object ownership and `CTRL_BREAK` delivery.
//!
//! Every operation here is a thin, explicit wrapper around one Win32 call. The
//! process layer's public surface never names a raw handle, so this module is
//! the only place that touches `HANDLE`-typed values: it hands out an opaque
//! [`TreeHandle`] and stores handles as `usize`, which keeps that token `Send +
//! Sync` whichever way `windows-sys` spells `HANDLE`.

#[cfg(test)]
use super::tree::{fail_start_step, StartFailurePointForTest};
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{
    AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, GetConsoleProcessList, GetConsoleWindow,
    CTRL_BREAK_EVENT,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, OpenThread, ResumeThread, WaitForSingleObject,
    CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED, INFINITE,
    PROCESS_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
};

/// Capability gate for the process layer: this backend can own a process tree,
/// so there is nothing to refuse.
pub fn require_backend(_operation: &'static str) -> Result<(), super::ProcessError> {
    Ok(())
}

/// Handle to a run's job object.
///
/// The job object itself is shared with the PTY backend, which owns a terminal
/// shell's tree the same way (`super::tree`); what this module adds on top is
/// the *service* startup sequence — create suspended, assign, resume.
pub use super::tree::Job as TreeHandle;

/// Put the run in a process group of its own, so a `CTRL_BREAK` can be aimed at
/// this run alone (see [`request_graceful_stop`]), on a console the desktop is
/// never asked to show.
pub fn prepare(command: &mut Command) {
    command.creation_flags(creation_flags_for(has_console()));
}

/// The creation flags a run is started with.
///
/// Split out from [`prepare`] so the rule can be read — and tested — without a
/// console of a particular shape: what the flags depend on is only whether the
/// Hub has a console of its own.
fn creation_flags_for(hub_has_console: bool) -> u32 {
    // `CREATE_NEW_PROCESS_GROUP` is what makes the run a process group of its own
    // — the thing a graceful stop is aimed at. `CREATE_SUSPENDED` keeps the
    // primary thread from running user code until `attach` has put the process in
    // its job, closing the interval where a fast launcher could create
    // descendants before ownership is established.
    let flags = CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED;

    if hub_has_console {
        // The Hub has a console, so the run inherits it: one console, created
        // once, that a `CTRL_BREAK` travels through with nothing to attach to
        // (D-035). `CREATE_NO_WINDOW` here would take the run *off* that console
        // and give it one of its own, which is a stop that has to be delivered by
        // attaching — measured, and not something to give up for a window that
        // already does not exist.
        flags
    } else {
        // The Hub has no console, so Windows would allocate one for the run — and
        // hand its window to whatever this machine uses as its default terminal
        // application, leaving a stray window on the desktop titled after the
        // program being run, one per run (D-035). `CREATE_NO_WINDOW` takes that
        // window away and not the console: the run still gets one, so a
        // `CTRL_BREAK` can still be aimed at its process group, and the stop path
        // reaches that console exactly the way it already had to.
        flags | CREATE_NO_WINDOW
    }
}

/// Pids attached to the console this process is attached to, empty when it has
/// none.
///
/// For tests that have to tell "the run shares this process's console" from
/// "the run has one of its own": which of the two it is decides whether a
/// `CTRL_BREAK` reaches it from here, and the answer is the environment's, not
/// the test's.
#[cfg(test)]
pub fn console_members() -> Vec<u32> {
    let mut probe = [0u32; 1];
    let count = unsafe { GetConsoleProcessList(probe.as_mut_ptr(), 1) };
    if count == 0 {
        return Vec::new();
    }
    let mut members = vec![0u32; count as usize];
    let listed = unsafe { GetConsoleProcessList(members.as_mut_ptr(), count) };
    members.truncate(listed.min(count) as usize);
    members
}

/// Whether this process is attached to a console at all.
///
/// The Hub is a windowless application in a release build and is normally
/// started from a shortcut, so it has none; a Hub started from a shell — the
/// debug build, or a release build launched from a terminal — inherits that
/// shell's console, and has one. What a run does with the answer is
/// [`creation_flags_for`]'s business; this is only the question, asked of the
/// console's own membership list (a process with a console has at least itself
/// on it).
fn has_console() -> bool {
    let mut members = [0u32; 1];
    unsafe { GetConsoleProcessList(members.as_mut_ptr(), 1) != 0 }
}

/// Put a run that keeps its own window and console in a process group of its
/// own, and give it the console Windows will put that window beside (#66).
///
/// `CREATE_NEW_CONSOLE` is the point of the mode rather than a detail: an
/// application configured as `display: window` keeps the console it provides,
/// and inheriting the Hub's — the Hub has none, being a GUI build — would make
/// it invisible instead. `CREATE_SUSPENDED` is the same start-owned-before-run
/// rule every other run follows ([`attach_independent`]).
pub fn prepare_windowed(command: &mut Command) {
    command.creation_flags(CREATE_NEW_CONSOLE | CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);
}

/// Create the run's job object, assign the suspended process, then let its
/// initial thread execute.
///
/// The process cannot create descendants before job membership is established.
/// If assignment or resumption fails, dropping `job` terminates any assigned
/// process and the caller kills/reaps the child handle.
pub fn attach(child: &Child) -> Result<TreeHandle, String> {
    attach_to(child, TreeHandle::create)
}

/// The same startup sequence for a run the Hub must not end by exiting (#66):
/// the job owns the tree exactly as above, but without the kill-on-close limit,
/// so closing the last handle only closes a handle.
pub fn attach_independent(child: &Child) -> Result<TreeHandle, String> {
    attach_to(child, TreeHandle::create_independent)
}

fn attach_to(
    child: &Child,
    create: impl FnOnce() -> Result<TreeHandle, String>,
) -> Result<TreeHandle, String> {
    #[cfg(test)]
    if fail_start_step(StartFailurePointForTest::JobCreation) {
        return Err("test-injected CreateJobObjectW failure".to_owned());
    }

    let job = create()?;

    #[cfg(test)]
    if fail_start_step(StartFailurePointForTest::JobAssignment) {
        return Err("test-injected AssignProcessToJobObject failure".to_owned());
    }

    job.assign(child.as_raw_handle() as usize)?;

    resume_initial_thread(child.id())?;
    Ok(job)
}

/// Resume the only thread a process can have before its suspended primary
/// thread starts. Tool Help identifies it by owner PID; opening only the
/// suspend/resume right keeps the handle narrow and startup-only.
fn resume_initial_thread(process_id: u32) -> Result<(), String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(last_error("CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD)"));
    }
    let snapshot = ScopedHandle(snapshot as usize);

    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    if unsafe { Thread32First(snapshot.0 as _, &mut entry) } == 0 {
        return Err(last_error("Thread32First"));
    }

    loop {
        if entry.th32OwnerProcessID == process_id {
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if thread.is_null() {
                return Err(last_error("OpenThread(THREAD_SUSPEND_RESUME)"));
            }
            let thread = ScopedHandle(thread as usize);

            // CREATE_SUSPENDED adds exactly one suspend count. A different
            // count means another actor changed the thread; fail closed so the
            // job's kill-on-close limit tears the process down.
            #[cfg(test)]
            if fail_start_step(StartFailurePointForTest::Resume) {
                return Err("test-injected ResumeThread failure".to_owned());
            }

            let previous_count = unsafe { ResumeThread(thread.0 as _) };
            if previous_count == u32::MAX {
                return Err(last_error("ResumeThread"));
            }
            if previous_count != 1 {
                return Err(format!(
                    "unexpected initial-thread suspend count for process {process_id}: {previous_count}"
                ));
            }
            return Ok(());
        }

        if unsafe { Thread32Next(snapshot.0 as _, &mut entry) } == 0 {
            break;
        }
    }

    Err(format!(
        "no initial thread found for suspended process {process_id}"
    ))
}

/// Temporary Win32 handle used for Tool Help snapshots and the initial thread.
#[derive(Debug)]
struct ScopedHandle(usize);

impl Drop for ScopedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0 as _) };
    }
}

/// Terminate everything still assigned to the job.
///
/// Members exit with code 1; a caller distinguishes a forced stop from a natural
/// exit through the report it gets back, never through the code.
pub fn terminate_tree(tree: &TreeHandle) -> Result<(), String> {
    tree.terminate()
}

/// Processes currently assigned to the job, including the run's own process.
/// An empty list means nothing of the run is left.
pub fn tree_pids(tree: &TreeHandle) -> Result<Vec<u32>, String> {
    tree.pids()
}

/// The value `WaitForSingleObject` can block on for this run's process.
pub fn process_handle(child: &Child) -> usize {
    child.as_raw_handle() as usize
}

/// Block until the process object is signalled. Costs no CPU while a run is
/// alive (D-009 — no per-session polling).
pub fn wait_for_handle(handle: usize) -> Result<(), String> {
    let waited = unsafe { WaitForSingleObject(handle as _, INFINITE) };
    if waited == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err(last_error("WaitForSingleObject"))
    }
}

/// Ask the run to finish on its own, reporting whether the request was delivered
/// to the run's process group.
///
/// A `CTRL_BREAK` only reaches processes that share a console with the caller, so
/// a run living on a console of its own is reached by attaching to that console
/// briefly. A run only has one of its own when the Hub has no console to inherit
/// (`creation_flags_for`), which is the case this second half exists for.
///
/// Delivery stays best-effort by design: a process with no console at all has
/// nothing to deliver to, and that is not an error, because the force path is
/// what guarantees the stop (`docs/DECISIONS.md` D-007).
pub fn request_graceful_stop(pid: u32) -> bool {
    unsafe {
        if GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) != 0 {
            return true;
        }
        if GetConsoleWindow() as usize != 0 {
            // The Hub has a console of its own and the run is not on it, so
            // there is nothing left to deliver the request to.
            return false;
        }
        // A windowless build shares no console with the run; attach to the
        // run's own console just long enough to deliver the request.
        FreeConsole();
        if AttachConsole(pid) == 0 {
            return false;
        }
        let delivered = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) != 0;
        FreeConsole();
        delivered
    }
}

/// When this run's process was created, read from the handle the supervisor
/// already holds (#66).
///
/// A pid is not an identity: Windows reuses the number once the process is
/// gone, and the Hub keeps a number for a run it does not supervise. The
/// creation timestamp is what a remembered pid can be checked against
/// ([`super::ProcessIdentity`]).
pub fn creation_time(child: &Child) -> Option<u64> {
    process_times(child.as_raw_handle() as usize)
}

/// When the process Windows currently reports under `pid` was created, or
/// `None` when no process answers for that id.
pub fn creation_time_of(pid: u32) -> Option<u64> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) } as usize;
    if process == 0 {
        return None;
    }
    let created = process_times(process);
    unsafe { CloseHandle(process as _) };
    created
}

fn process_times(process: usize) -> Option<u64> {
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
    if read == 0 {
        return None;
    }
    Some(((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64)
}

/// Liveness of an arbitrary pid, for tests that have to check a process the
/// supervisor never owned.
#[cfg(test)]
pub fn is_process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::STILL_ACTIVE;
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) } as usize;
    if process == 0 {
        // No process object answers for this pid, so nothing is running under it.
        return false;
    }
    // An openable process object outlives the process, so the exit code is what
    // separates a live process from one that has ended: a running process reports
    // `STILL_ACTIVE`, an ended one reports the code it ended with.
    let mut code: u32 = 0;
    let read = unsafe { GetExitCodeProcess(process as _, &mut code) };
    unsafe { CloseHandle(process as _) };
    read != 0 && code == STILL_ACTIVE as u32
}

/// Describe the last Win32 failure as a bare cause — the raw error code and the
/// call it came from. The process layer names the *operation* it wrapped this
/// into, so repeating it here would read as `X failed: X failed (...)`.
///
/// `Win32 error {code}` is the code `GetLastError` reports; it is what a user
/// needs to look the failure up (`docs/DEVELOPMENT.md` §9).
fn last_error(call: &str) -> String {
    let code = unsafe { GetLastError() };
    format!("Win32 error {code} from {call}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule the flags encode: a run is always its own process group and
    /// always starts suspended, and it is denied a console *window* only when
    /// the Hub has no console for it to inherit.
    #[test]
    fn a_run_gets_no_console_window_only_when_it_has_no_console_to_inherit() {
        let with_hub_console = creation_flags_for(true);
        assert_eq!(
            with_hub_console & CREATE_NO_WINDOW,
            0,
            "a run of a Hub that has a console keeps sharing it"
        );
        assert_eq!(
            with_hub_console & (CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED),
            CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED
        );

        let without_hub_console = creation_flags_for(false);
        assert_ne!(
            without_hub_console & CREATE_NO_WINDOW,
            0,
            "a run of a windowless Hub is given a console with no window"
        );
        assert_eq!(
            without_hub_console & (CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED),
            CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED
        );
    }

    /// The question [`creation_flags_for`] is asked, answered the way a process
    /// with a console answers it: the console's membership list is never empty
    /// for a process that has one.
    #[test]
    fn having_a_console_is_read_from_the_consoles_membership() {
        // Whichever way the test runner was started, the two readings have to
        // agree with each other: `has_console` is true exactly when the console
        // reports members.
        let mut members = [0u32; 8];
        let listed = unsafe { GetConsoleProcessList(members.as_mut_ptr(), 8) };
        assert_eq!(has_console(), listed != 0);
    }
}
