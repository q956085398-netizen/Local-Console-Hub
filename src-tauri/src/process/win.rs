//! Windows supervisor backend — job-object ownership and `CTRL_BREAK` delivery.
//!
//! Every operation here is a thin, explicit wrapper around one Win32 call. The
//! process layer's public surface never names a raw handle, so this module is
//! the only place that touches `HANDLE`-typed values: it hands out an opaque
//! [`TreeHandle`] and stores handles as `usize`, which keeps that token `Send +
//! Sync` whichever way `windows-sys` spells `HANDLE`.

use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{
    AttachConsole, FreeConsole, GenerateConsoleCtrlEvent, GetConsoleWindow, CTRL_BREAK_EVENT,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Threading::{
    WaitForSingleObject, CREATE_NEW_PROCESS_GROUP, INFINITE,
};

/// Capability gate for the process layer: this backend can own a process tree,
/// so there is nothing to refuse.
pub fn require_backend(_operation: &'static str) -> Result<(), super::ProcessError> {
    Ok(())
}

/// Processes reported per tree query.
const MAX_TREE_PROCESSES: usize = 512;

/// Handle to a run's job object.
///
/// Stored as an integer rather than as a `HANDLE` so the token stays `Send +
/// Sync` regardless of how `windows-sys` spells the type, and so nothing outside
/// this module names a raw handle.
#[derive(Debug)]
pub struct TreeHandle(usize);

impl Drop for TreeHandle {
    fn drop(&mut self) {
        // `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: closing the last handle to a job
        // terminates whatever is still assigned to it. That is the backstop
        // behind `ManagedProcess`'s RAII ownership, so the supervisor's handle
        // has to live exactly as long as the run should.
        unsafe { CloseHandle(self.0 as _) };
    }
}

/// Put the run in a process group of its own, so a `CTRL_BREAK` can be aimed at
/// this run alone (see [`request_graceful_stop`]).
pub fn prepare(command: &mut Command) {
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

/// Create the run's job object and assign the run's process to it.
///
/// Job membership is what makes a later termination incapable of reaching a
/// process outside the run.
pub fn attach(child: &Child) -> Result<TreeHandle, String> {
    let created = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) } as usize;
    if created == 0 {
        return Err(last_error("CreateJobObjectW"));
    }
    let job = TreeHandle(created);

    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let sized = size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
    let configured = unsafe {
        SetInformationJobObject(
            job.0 as _,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            sized,
        )
    };
    if configured == 0 {
        return Err(last_error("SetInformationJobObject"));
    }

    let assigned = unsafe { AssignProcessToJobObject(job.0 as _, child.as_raw_handle() as _) };
    if assigned == 0 {
        return Err(last_error("AssignProcessToJobObject"));
    }
    Ok(job)
}

/// Terminate everything still assigned to the job.
///
/// Members exit with code 1; a caller distinguishes a forced stop from a natural
/// exit through the report it gets back, never through the code.
pub fn terminate_tree(tree: &TreeHandle) -> Result<(), String> {
    let terminated = unsafe { TerminateJobObject(tree.0 as _, 1) };
    if terminated == 0 {
        return Err(last_error("TerminateJobObject"));
    }
    Ok(())
}

/// Processes currently assigned to the job, including the run's own process.
///
/// The OS reports at most `MAX_TREE_PROCESSES` ids per query, so a larger tree is
/// truncated. Emptiness — the property a stop barrier relies on — is still
/// reported faithfully.
pub fn tree_pids(tree: &TreeHandle) -> Result<Vec<u32>, String> {
    // The id list is variable-length: two `u32` counters followed by the
    // entries. Sizing the buffer in `usize` units keeps it aligned for them.
    let mut buffer = vec![0usize; 1 + MAX_TREE_PROCESSES];
    let mut returned_bytes: u32 = 0;
    let queried = unsafe {
        QueryInformationJobObject(
            tree.0 as _,
            JobObjectBasicProcessIdList,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * size_of::<usize>()) as u32,
            &mut returned_bytes,
        )
    };
    if queried == 0 {
        return Err(last_error("QueryInformationJobObject"));
    }

    let list = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
    // An id list is variable-length: the OS reports how many entries are valid.
    let listed = list.NumberOfProcessIdsInList as usize;
    let count = listed.min(MAX_TREE_PROCESSES);
    let first = list.ProcessIdList.as_ptr();
    let ids = (0..count).map(|index| unsafe { *first.add(index) as u32 });
    Ok(ids.collect())
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
    read != 0 && code == STILL_ACTIVE
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
