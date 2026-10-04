//! Windows readings for one process this module was asked about.
//!
//! The only calls are `OpenProcess`, `GetProcessTimes`, `GetProcessMemoryInfo`,
//! and `CloseHandle`. Nothing here terminates a process. A pid is opened only
//! long enough to compare its creation time with the one the caller already
//! holds; when they differ, the counters are not returned.

use std::mem::{size_of, zeroed};

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_VM_READ,
};

use super::ConfirmedReading;

pub(super) fn read_once(pid: u32, expected_created_at: u64) -> Option<ConfirmedReading> {
    let process = open_for_times(pid)?;
    let times = process_times(process.raw())?;
    if times.created != expected_created_at {
        return None;
    }
    Some(ConfirmedReading {
        pid,
        created_at: times.created,
        cpu_time_100ns: Some(times.cpu),
        memory_bytes: working_set(process.raw())
            .or_else(|| working_set_from_fresh_open(pid, expected_created_at)),
    })
}

struct Times {
    created: u64,
    cpu: u64,
}

struct ProcessHandle(usize);

impl ProcessHandle {
    fn raw(&self) -> isize {
        self.0 as isize
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        // SAFETY: the handle is a non-null `OpenProcess` result, closed once.
        unsafe { CloseHandle(self.0 as _) };
    }
}

fn open_for_times(pid: u32) -> Option<ProcessHandle> {
    open(pid, PROCESS_QUERY_LIMITED_INFORMATION)
}

fn open(pid: u32, access: u32) -> Option<ProcessHandle> {
    // SAFETY: a null result is the failure this function turns into `None`.
    // A non-null result is closed by `ProcessHandle`.
    let handle = unsafe { OpenProcess(access, 0, pid) } as usize;
    if handle == 0 {
        None
    } else {
        Some(ProcessHandle(handle))
    }
}

fn process_times(process: isize) -> Option<Times> {
    let mut created: FILETIME = unsafe { zeroed() };
    let mut exited: FILETIME = unsafe { zeroed() };
    let mut kernel: FILETIME = unsafe { zeroed() };
    let mut user: FILETIME = unsafe { zeroed() };
    // SAFETY: the four out-pointers are live `FILETIME`s, and `process` is an
    // open process handle.
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
    Some(Times {
        created: filetime(created),
        cpu: filetime(kernel).saturating_add(filetime(user)),
    })
}

fn working_set(process: isize) -> Option<u64> {
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { zeroed() };
    counters.cb = u32::try_from(size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    // SAFETY: `counters` is a live struct whose `cb` is its own size, and
    // `process` is an open process handle. Failure leaves the counters unread.
    let read = unsafe { GetProcessMemoryInfo(process as _, &mut counters, counters.cb) };
    if read == 0 {
        return None;
    }
    Some(u64::try_from(counters.WorkingSetSize).unwrap_or(u64::MAX))
}

/// Memory counters need a broader access mask than the creation time.
///
/// The times handle is `PROCESS_QUERY_LIMITED_INFORMATION`, which is enough
/// to confirm identity without asking for rights a later terminate would use.
/// When that handle cannot read the working set, one more open asks for the
/// query and read rights the counter call documents. If that open is refused,
/// the field stays unread.
fn working_set_from_fresh_open(pid: u32, expected_created_at: u64) -> Option<u64> {
    let process = open(
        pid,
        PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_QUERY_LIMITED_INFORMATION,
    )?;
    let times = process_times(process.raw())?;
    if times.created != expected_created_at {
        return None;
    }
    working_set(process.raw())
}

fn filetime(value: FILETIME) -> u64 {
    ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
}
