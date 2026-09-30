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
    AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, GetConsoleWindow, CTRL_BREAK_EVENT,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, OpenThread, ResumeThread, WaitForSingleObject,
    CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED, INFINITE,
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
/// this run alone (see [`request_graceful_stop`]).
pub fn prepare(command: &mut Command) {
    // Keep the primary thread from running user code until `attach` has put the
    // process in its job. This closes the interval where a fast launcher could
    // create descendants before ownership is established.
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_SUSPENDED);
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
/// briefly. Delivery is best-effort by design: a process with no console at all
/// has nothing to deliver to, and that is not an error, because the force path is
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
