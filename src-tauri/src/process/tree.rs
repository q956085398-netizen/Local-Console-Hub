//! The Windows job object that owns one managed process tree.
//!
//! Both kinds of run the Hub hosts are trees rather than single processes: a
//! supervised service (T03) and an interactive terminal's shell (T02) each
//! start children of their own, and each promises that ending the run ends
//! those children too (`docs/DEVELOPMENT.md` §6, spec #59 decision 13). The
//! Win32 object behind that promise is the same one either way, so it lives
//! here — inside the layer the spec gives "stop / kill tree" to — instead of as
//! a second copy that could drift from this one on the one thing both promise.
//!
//! ## The process is created suspended
//!
//! A job owns what is assigned to it, and a process that is already running can
//! create descendants before `AssignProcessToJobObject` returns. Both backends
//! therefore create their process suspended and resume its primary thread only
//! once it is in the job; a process that cannot be assigned is terminated
//! rather than left running outside the tree (#55).
//!
//! ## The handle *is* the ownership token
//!
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes closing the last handle to a job
//! terminate whatever is still assigned to it, so [`Job`] is RAII: the tree
//! lives exactly as long as this token does.

#[cfg(test)]
use std::cell::Cell;
use std::mem::size_of;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// Processes reported per tree query.
const MAX_TREE_PROCESSES: usize = 512;

#[cfg(test)]
thread_local! {
    static FAIL_START_STEP_FOR_TEST: Cell<Option<StartFailurePointForTest>> = const { Cell::new(None) };
}

/// The steps a start can be made to fail at, so the teardown each one owes is
/// observable rather than asserted about a code path no test can reach.
///
/// Shared by both backends because the steps are: each creates the job, assigns
/// a process that is not yet running, and resumes it. What differs is only
/// where each backend checks them, so what is here is the vocabulary and the
/// switch, not the sites.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartFailurePointForTest {
    JobCreation,
    JobAssignment,
    Resume,
}

/// Make the next start fail at `point`. Thread-local, so parallel tests cannot
/// inject into each other's spawns.
#[cfg(test)]
pub(crate) fn fail_start_step_for_test(point: StartFailurePointForTest) {
    FAIL_START_STEP_FOR_TEST.with(|fail| fail.set(Some(point)));
}

/// Whether this start is the one `fail_start_step_for_test` was aimed at.
///
/// Reading consumes the value: an injection that has been observed cannot be
/// seen by a later start on the same thread, which is what keeps it from
/// escaping the test that set it (tests sharing a thread under
/// `--test-threads=1` included).
#[cfg(test)]
pub(crate) fn fail_start_step(point: StartFailurePointForTest) -> bool {
    FAIL_START_STEP_FOR_TEST.with(|fail| {
        if fail.get() == Some(point) {
            fail.set(None);
            true
        } else {
            false
        }
    })
}

/// Handle to one run's job object.
///
/// Stored as an integer rather than as a `HANDLE` so the token stays `Send +
/// Sync` regardless of how `windows-sys` spells the type, and so nothing
/// outside the Windows backends names a raw handle.
#[derive(Debug)]
pub struct Job(usize);

impl Job {
    /// Create a job that terminates everything still in it when its last handle
    /// closes, and report what could not be configured as the Win32 cause.
    pub fn create() -> Result<Self, String> {
        let created = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) } as usize;
        if created == 0 {
            return Err(last_error("CreateJobObjectW"));
        }
        let job = Job(created);

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
        Ok(job)
    }

    /// Put a process into this job.
    ///
    /// `process` is a process handle. The caller must hand over a process that
    /// has not started executing user code yet: assigning one that is already
    /// running leaves the window in which it could have created descendants
    /// outside the tree.
    ///
    /// Assignment can fail — a process already in a job that forbids nesting is
    /// the usual reason — and the caller owes the process a teardown in that
    /// case, because it is not owned by anything here.
    pub fn assign(&self, process: usize) -> Result<(), String> {
        let assigned = unsafe { AssignProcessToJobObject(self.0 as _, process as _) };
        if assigned == 0 {
            return Err(last_error("AssignProcessToJobObject"));
        }
        Ok(())
    }

    /// Terminate everything still assigned to the job.
    ///
    /// Members exit with code 1; a caller distinguishes a forced stop from a
    /// natural exit through the report it gets back, never through the code.
    /// Terminating a job whose members have all exited already is a no-op that
    /// succeeds, so this is safe to repeat.
    pub fn terminate(&self) -> Result<(), String> {
        let terminated = unsafe { TerminateJobObject(self.0 as _, 1) };
        if terminated == 0 {
            return Err(last_error("TerminateJobObject"));
        }
        Ok(())
    }

    /// Processes currently assigned to the job, including the run's own
    /// process.
    ///
    /// The OS reports at most [`MAX_TREE_PROCESSES`] ids per query, so a larger
    /// tree is truncated. Emptiness — the property a stop barrier relies on —
    /// is still reported faithfully.
    pub fn pids(&self) -> Result<Vec<u32>, String> {
        // The id list is variable-length: two `u32` counters followed by the
        // entries. Sizing the buffer in `usize` units keeps it aligned for
        // them.
        let mut buffer = vec![0usize; 1 + MAX_TREE_PROCESSES];
        let mut returned_bytes: u32 = 0;
        let queried = unsafe {
            QueryInformationJobObject(
                self.0 as _,
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
        // An id list is variable-length: the OS reports how many entries are
        // valid.
        let listed = list.NumberOfProcessIdsInList as usize;
        let count = listed.min(MAX_TREE_PROCESSES);
        let first = list.ProcessIdList.as_ptr();
        let ids = (0..count).map(|index| unsafe { *first.add(index) as u32 });
        Ok(ids.collect())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // The kill-on-close limit is what makes the token RAII: a handle going
        // away terminates whatever is still assigned, so a run cannot outlive
        // the thing that accounts for it.
        unsafe { CloseHandle(self.0 as _) };
    }
}

/// Describe the last Win32 failure as a bare cause — the raw error code and the
/// call it came from. The caller names the *operation* it wrapped this into, so
/// repeating the operation here would read as `X failed: X failed (...)`
/// (`docs/DEVELOPMENT.md` §9).
fn last_error(call: &str) -> String {
    let code = unsafe { GetLastError() };
    format!("Win32 error {code} from {call}")
}
