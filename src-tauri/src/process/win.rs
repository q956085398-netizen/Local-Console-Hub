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
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_NO_MORE_FILES, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::Console::{
    GenerateConsoleCtrlEvent, GetConsoleProcessList, CTRL_BREAK_EVENT,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, Thread32First, Thread32Next,
    PROCESSENTRY32W, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, GetProcessTimes, OpenProcess, OpenThread, QueryFullProcessImageNameW,
    ResumeThread, WaitForSingleObject, CREATE_NEW_CONSOLE, CREATE_NEW_PROCESS_GROUP,
    CREATE_NO_WINDOW, CREATE_SUSPENDED, INFINITE, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, THREAD_SUSPEND_RESUME,
};

// The command line a process was started with, by way of `ntdll` (#67).
//
// Windows exposes this nowhere in `kernel32`: the documented way to read
// another process's command line is WMI, which means a COM apartment and a
// query round-trip to answer a question asked while the user waits. The
// `ntdll` call is the one every process viewer uses for the same reason, and
// its class number is stable. A failure here is reported as "could not read"
// and never guessed at, which is what keeps an unreadable command line from
// becoming a confident association (spec #59 decision 11).
#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(
        process: isize,
        class: u32,
        info: *mut std::ffi::c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}

/// `ProcessCommandLineInformation` — one `UNICODE_STRING` in the caller's
/// buffer, with the string itself in the bytes that follow it.
const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;

/// The `UNICODE_STRING` `ntdll` writes for the class above.
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}

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

/// Start the run, on the console this process is on.
///
/// A child is given the console of whoever starts it, and what a run is
/// supposed to inherit is *this* process's console (D-035). Another thread can
/// have this process attached to somebody else's console for the length of a
/// window lookup (D-037), so the spawn is made while the console is claimed:
/// otherwise a run started at that moment is handed a console of its own —
/// a window on the desktop, which is the very thing the run's creation flags
/// exist to avoid (measured: a window per run started during a lookup).
pub fn spawn(command: &mut Command, prepare: impl FnOnce(&mut Command)) -> std::io::Result<Child> {
    let _console = crate::console::Console::claim();
    // D-035 chooses flags by reading this process's console. That reading and
    // CreateProcess must see the same console, not two sides of a borrow.
    prepare(command);
    command.spawn()
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
/// briefly. Both attempts are made under one claim on this process's console
/// (D-037): the event is raised in *whichever* console this process is attached
/// to, so a window lookup attached to another run's console at this moment would
/// take the request with it — and a `CTRL_BREAK` that reports success into the
/// wrong console is a rung of the stop ladder signing for a delivery nobody
/// received (D-007).
///
/// Delivery stays best-effort by design: a process with no console at all has
/// nothing to deliver to, and a console this process cannot borrow is one it
/// keeps — either way the force path is what guarantees the stop.
pub fn request_graceful_stop(pid: u32) -> bool {
    let console = crate::console::Console::claim();
    unsafe {
        if GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) != 0 {
            return true;
        }
        // Delivery and restoration are independent facts. Keep the delivery
        // answer even if the caller's console cannot be restored afterwards.
        let mut delivered = false;
        let _ = console.borrow(pid, || {
            delivered = GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, pid) != 0;
        });
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

/// Every process whose executable file name is one of `names` (#67).
///
/// One snapshot of the process table gives every pid and its file name; only
/// the entries whose name is wanted are then opened, because the image path and
/// the command line live inside the process rather than in the table.
pub fn processes_named(names: &[String]) -> Option<Vec<super::ProcessReading>> {
    let wanted: Vec<String> = names.iter().map(|name| name.to_ascii_lowercase()).collect();

    // The snapshot itself is retried (`process_entries`), because taking it can
    // fail outright. What is *not* retried is an empty answer: a caller asking
    // "is this application already running?" must be able to trust "no", and a
    // table that was read and did not contain the name is exactly that answer.
    for _ in 0..SNAPSHOT_ATTEMPTS {
        let Some(entries) = process_entries() else {
            continue;
        };
        return Some(
            entries
                .into_iter()
                .filter_map(|entry| {
                    let name = wide_string(&entry.szExeFile);
                    wanted
                        .contains(&name.to_ascii_lowercase())
                        .then(|| read_process(entry.th32ProcessID, name))
                })
                .collect(),
        );
    }

    // `None` is "the table could not be read, so I do not know" — which is not
    // the same claim as "nothing is running" (spec #59 decision 11).
    None
}

/// Every process started by `pid`, directly or through a chain of children.
pub fn descendants(pid: u32) -> Vec<u32> {
    let Some(entries) = process_entries() else {
        return Vec::new();
    };

    // A breadth-first walk over the parent links of one snapshot. A process
    // table is a graph a launcher can make confusing — a child of a pid that
    // has already exited keeps a parent number that now belongs to somebody
    // else — so the walk refuses to visit anything twice rather than trusting
    // the table to be a tree.
    let mut found: Vec<u32> = Vec::new();
    let mut frontier = vec![pid];
    while let Some(current) = frontier.pop() {
        for entry in &entries {
            let child = entry.th32ProcessID;
            if entry.th32ParentProcessID != current || child == current || child == pid {
                continue;
            }
            if !found.contains(&child) {
                found.push(child);
                frontier.push(child);
            }
        }
    }
    found
}

/// Open the process an identity describes, refusing one that is not it (#67).
pub fn open_external(identity: super::ProcessIdentity) -> Result<usize, super::ProcessError> {
    let handle = unsafe {
        OpenProcess(
            SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            identity.pid(),
        )
    } as usize;
    if handle == 0 {
        return Err(super::ProcessError::Supervision {
            operation: "opening the application already running outside the Hub",
            reason: format!(
                "process {} could not be opened ({}); it may have ended, or it may be running \
                 with rights this Hub does not have",
                identity.pid(),
                last_error("OpenProcess"),
            ),
        });
    }

    // The pid was read from a table that is already history. A process Windows
    // has since ended and replaced would answer `OpenProcess` happily, and
    // everything downstream would then be about somebody else's process — so
    // the creation time is checked against the identity before the handle is
    // kept (spec #59 decision 11).
    if process_times(handle) != Some(identity.created_at()) {
        unsafe { CloseHandle(handle as _) };
        return Err(super::ProcessError::Supervision {
            operation: "opening the application already running outside the Hub",
            reason: format!(
                "process {} is not the one that was found a moment ago — Windows has reused \
                 the number; the Hub will not act on a process it did not verify",
                identity.pid()
            ),
        });
    }
    Ok(handle)
}

/// Wait up to `timeout` for an opened process to end. `true` means it has.
pub fn wait_for_handle_timeout(handle: usize, timeout: std::time::Duration) -> bool {
    // `INFINITE` is `u32::MAX` and is not what a bounded wait means; a timeout
    // longer than that is rounded down rather than turned into "never".
    let millis = timeout.as_millis().min(u32::MAX as u128 - 1) as u32;
    unsafe { WaitForSingleObject(handle as _, millis) == WAIT_OBJECT_0 }
}

/// The code an opened process ended with, once it has ended.
pub fn exit_code(handle: usize) -> Option<u32> {
    let mut code: u32 = 0;
    (unsafe { GetExitCodeProcess(handle as _, &mut code) } != 0).then_some(code)
}

/// Close a handle this module handed out.
pub fn close_handle(handle: usize) {
    if handle != 0 {
        unsafe { CloseHandle(handle as _) };
    }
}

/// How many times a process-table read is attempted before it is given up on.
const SNAPSHOT_ATTEMPTS: usize = 3;

/// One process table snapshot, or `None` when it could not be taken.
///
/// Retried, because a snapshot of a *live* machine routinely comes back short:
/// the list is copied while processes come and go, and Windows answers
/// `ERROR_BAD_LENGTH` rather than block. A truncated list is worse than a
/// failed one here — it would silently omit the very process the caller is
/// looking for, and "nothing is running outside" is the answer that starts a
/// second copy (spec #59 decision 11).
fn process_entries() -> Option<Vec<PROCESSENTRY32W>> {
    for _ in 0..SNAPSHOT_ATTEMPTS {
        if let Some(entries) = one_process_snapshot() {
            return Some(entries);
        }
    }
    None
}

fn one_process_snapshot() -> Option<Vec<PROCESSENTRY32W>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return None;
    }
    let snapshot = ScopedHandle(snapshot as usize);

    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    if unsafe { Process32FirstW(snapshot.0 as _, &mut entry) } == 0 {
        return None;
    }

    let mut entries = Vec::new();
    loop {
        entries.push(entry);
        if unsafe { Process32NextW(snapshot.0 as _, &mut entry) } == 0 {
            // `ERROR_NO_MORE_FILES` is the end of the list; anything else is
            // the walk being cut short by a process appearing or leaving.
            let reached_end = unsafe { GetLastError() } == ERROR_NO_MORE_FILES;
            return reached_end.then_some(entries);
        }
    }
}

/// One process' image path, command line and creation time, as far as this
/// process may look (#67).
fn read_process(pid: u32, file_name: String) -> super::ProcessReading {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) } as usize;
    if handle == 0 {
        return super::ProcessReading {
            pid,
            created_at: None,
            file_name,
            image_path: None,
            command_line: None,
        };
    }
    let reading = super::ProcessReading {
        pid,
        created_at: process_times(handle),
        file_name,
        image_path: image_path_of(handle),
        command_line: command_line_of(handle),
    };
    unsafe { CloseHandle(handle as _) };
    reading
}

/// The full path of the image a process is running, if it can be read.
fn image_path_of(process: usize) -> Option<std::path::PathBuf> {
    // `MAX_PATH` is not the limit for this call; the documented bound for a
    // long path is 32 767 wide characters, and the size is passed in and out.
    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    let read = unsafe {
        QueryFullProcessImageNameW(
            process as _,
            PROCESS_NAME_WIN32,
            buffer.as_mut_ptr(),
            &mut size,
        )
    };
    if read == 0 {
        return None;
    }
    buffer.truncate(size as usize);
    Some(std::path::PathBuf::from(String::from_utf16_lossy(&buffer)))
}

/// The command line a process was started with, if it can be read.
fn command_line_of(process: usize) -> Option<String> {
    let mut needed: u32 = 0;
    // The sizing call is expected to fail — it is how the length is asked for.
    unsafe {
        NtQueryInformationProcess(
            process as _,
            PROCESS_COMMAND_LINE_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut needed,
        )
    };
    if needed == 0 {
        return None;
    }

    // `u64` rather than `u8`: the answer begins with a `UNICODE_STRING`, and a
    // byte buffer is only aligned for a byte.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(size_of::<u64>())];
    let status = unsafe {
        NtQueryInformationProcess(
            process as _,
            PROCESS_COMMAND_LINE_INFORMATION,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if status < 0 {
        return None;
    }

    // The string lives inside the buffer this call filled; the structure only
    // points at it.
    let command = unsafe { &*(buffer.as_ptr() as *const UnicodeString) };
    if command.buffer.is_null() || command.length == 0 {
        return None;
    }
    let characters =
        unsafe { std::slice::from_raw_parts(command.buffer, (command.length / 2) as usize) };
    Some(String::from_utf16_lossy(characters))
}

/// The NUL-terminated contents of a fixed-width wide string field.
fn wide_string(field: &[u16]) -> String {
    let end = field
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(field.len());
    String::from_utf16_lossy(&field[..end])
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

    #[test]
    fn run_preparation_waits_for_the_console_claim() {
        use std::sync::mpsc;
        use std::time::Duration;

        let console = crate::console::Console::claim();
        let (started_tx, started_rx) = mpsc::channel();
        let (prepared_tx, prepared_rx) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let mut command = Command::new("lch-nonexistent-console-spawn-fixture.exe");
            started_tx.send(()).unwrap();
            spawn(&mut command, |command| {
                prepare(command);
                prepared_tx.send(()).unwrap();
            })
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let prepared_while_borrowed = prepared_rx.recv_timeout(Duration::from_millis(100)).is_ok();
        drop(console);
        let prepared_after_release =
            prepared_while_borrowed || prepared_rx.recv_timeout(Duration::from_secs(5)).is_ok();
        let result = waiter.join().unwrap();
        assert!(
            !prepared_while_borrowed,
            "flag observation must wait for restoration"
        );
        assert!(
            prepared_after_release,
            "preparation must run after the claim is released"
        );
        assert!(
            result.is_err(),
            "the nonexistent fixture must not start a process"
        );
    }

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
        let _console = crate::console::Console::claim();
        // Whichever way the test runner was started, the two readings have to
        // agree with each other: `has_console` is true exactly when the console
        // reports members.
        let mut members = [0u32; 8];
        let listed = unsafe { GetConsoleProcessList(members.as_mut_ptr(), 8) };
        assert_eq!(has_console(), listed != 0);
    }
}
