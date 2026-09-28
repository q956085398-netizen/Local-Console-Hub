//! Windows PTY backend — a direct ConPTY implementation over `windows-sys`.
//!
//! One pseudoconsole per session (`CreatePseudoConsole`), two anonymous pipes
//! around it, and the shell attached at process creation through the
//! `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` attribute list — the shape of the
//! documented ConPTY session, kept in this module so the PTY layer's public
//! surface never names a raw handle. Why direct ConPTY and not portable-pty is
//! recorded in `docs/DECISIONS.md` D-014.
//!
//! The conpty-facing ends of both pipes are closed as soon as the
//! pseudoconsole exists: the pseudoconsole holds what it needs, and keeping the
//! Hub's copies would hide EOF from a terminal that has ended. The Hub keeps
//! exactly the two ends a terminal host needs — the input write end (what
//! [`PtyBackend::write_input`] writes keystrokes into) and the output read end
//! (what the layer's reader thread renders from).
//!
//! Every operation here is a thin wrapper around one Win32 call, mirroring the
//! process layer's `win` backend.

use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Read, Write};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::sync::{Arc, Mutex};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, ResizePseudoConsole, COORD, HPCON,
};
use windows_sys::Win32::System::Pipes::CreatePipe;
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess, GetProcessId,
    InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, EXTENDED_STARTUPINFO_PRESENT, INFINITE,
    LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW,
};

/// The attribute that attaches the spawned process to the pseudoconsole.
/// Spelled here instead of imported so the code does not depend on which
/// windows-sys module carries the constant.
const PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE: usize = 0x0002_0016;

/// Capability gate for the PTY layer: this backend can host terminals, so there
/// is nothing to refuse.
pub fn require_backend(_operation: &'static str) -> Result<(), super::PtyError> {
    Ok(())
}

/// One live ConPTY session: the pseudoconsole, the input write end, and the
/// shell's process handle.
#[derive(Debug)]
pub struct PtyBackend {
    pid: u32,
    con: ConHandle,
    input: Mutex<File>,
    child: ChildHandle,
}

impl PtyBackend {
    /// Start `spec`'s program attached to a new pseudoconsole.
    ///
    /// On success the caller also receives the pty's output stream, which
    /// belongs to a reader thread of its own — the UI render thread must never
    /// be the only thread reading terminal output (`docs/DEVELOPMENT.md` §5).
    pub fn spawn(spec: &super::PtySpec) -> Result<(PtyBackend, OutputReader), String> {
        let (input_read, input_write) = pipe()?;
        let (output_read, output_write) = pipe()?;

        let size = COORD { X: spec.cols as i16, Y: spec.rows as i16 };
        let mut con: HPCON = std::ptr::null_mut();
        let created = unsafe {
            CreatePseudoConsole(size, input_read.0 as _, output_write.0 as _, 0, &mut con)
        };
        if created != 0 {
            return Err(format!(
                "CreatePseudoConsole at {}x{} failed: HRESULT {created:#010x}",
                spec.cols, spec.rows
            ));
        }
        // The pseudoconsole duplicated what it needs; the Hub's copies of the
        // conpty-facing ends must go away now, or a terminal that has ended
        // could never be observed as ended on these pipes.
        drop(input_read);
        drop(output_write);
        let con = ConHandle(con as usize);

        let mut attributes = AttributeList::new(1)?;
        attributes.set_pseudoconsole(&con)?;

        let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        // No handles are inherited — the pseudoconsole attribute carries the
        // console. Naming the stdio handles invalid stops the child from
        // inheriting the Hub's own redirected handles instead of the pty.
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        startup.StartupInfo.hStdError = INVALID_HANDLE_VALUE;
        startup.lpAttributeList = attributes.as_mut_ptr();

        let mut command = wide_command_line(&spec.program, &spec.args);
        let mut environment = environment_block();
        let cwd: Vec<u16> = spec
            .cwd
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let started = unsafe {
            CreateProcessW(
                // The application name is null: the command line names the
                // program, which is what makes a bare `powershell.exe`
                // resolvable through PATH.
                std::ptr::null(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                cwd.as_ptr(),
                std::ptr::addr_of_mut!(startup.StartupInfo).cast(),
                &mut process,
            )
        };
        if started == 0 {
            let failure = last_error("CreateProcessW");
            return Err(format!(
                "CreateProcessW failed for `{}` in `{}`: {failure}",
                spec.program.display(),
                spec.cwd.display()
            ));
        }
        // The main thread handle is never used; the process handle is what the
        // session is anchored on.
        unsafe { CloseHandle(process.hThread) };
        let pid = unsafe { GetProcessId(process.hProcess) };
        if pid == 0 {
            let failure = last_error("GetProcessId");
            unsafe { CloseHandle(process.hProcess) };
            return Err(failure);
        }
        let child = ChildHandle(Arc::new(RawChild(process.hProcess as usize)));

        let backend = PtyBackend {
            pid,
            con,
            input: Mutex::new(input_write.into_file()),
            child,
        };
        let reader = OutputReader {
            file: output_read.into_file(),
        };
        Ok((backend, reader))
    }

    /// The shell's process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// A shareable handle to the shell process, for exit observation.
    pub fn child(&self) -> ChildHandle {
        self.child.clone()
    }

    /// Forward terminal input (keystrokes) into the pty.
    pub fn write_input(&self, bytes: &[u8]) -> io::Result<()> {
        // Poisoning tolerance, same stance as the layer's `lock`: a panic on
        // one path must not turn every later keystroke into a second panic.
        self.input
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .write_all(bytes)
    }

    /// Resize the pseudoconsole. New dimensions reach the shell with the next
    /// console query — nothing is re-flowed by this layer.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let size = COORD { X: cols as i16, Y: rows as i16 };
        let resized = unsafe { ResizePseudoConsole(self.con.0 as _, size) };
        if resized != 0 {
            return Err(format!(
                "ResizePseudoConsole to {cols}x{rows} failed: HRESULT {resized:#010x}"
            ));
        }
        Ok(())
    }

    /// Terminate the shell's own process. Descendants the shell started are
    /// reclaimed by closing the pseudoconsole (`ConHandle`'s drop) — which is
    /// also the only cleanup a dropped session performs.
    pub fn terminate(&self) -> Result<(), String> {
        let terminated = unsafe { TerminateProcess(self.child.0.0 as _, 1) };
        if terminated == 0 {
            return Err(last_error("TerminateProcess"));
        }
        Ok(())
    }
}

/// The pty's output stream, handed to the PTY layer's reader thread.
#[derive(Debug)]
pub struct OutputReader {
    file: File,
}

impl OutputReader {
    /// One blocking read of rendered terminal output. `Ok(0)` is EOF: the
    /// pseudoconsole has closed and the stream is over.
    pub fn read_chunk(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }
}

/// Handle to the shell process started on the pty, shareable between the
/// session handle and the exit watcher thread.
#[derive(Debug, Clone)]
pub struct ChildHandle(Arc<RawChild>);

/// The raw process handle, closed when the last sharer goes away.
#[derive(Debug)]
struct RawChild(usize);

impl Drop for RawChild {
    fn drop(&mut self) {
        // Closing the process handle neither terminates the shell nor keeps it
        // alive; it only ends this session's ability to observe it.
        unsafe { CloseHandle(self.0 as _) };
    }
}

impl ChildHandle {
    /// Block until the shell's process object is signalled — its exit. Costs no
    /// CPU while the shell is alive (D-009 — no per-session polling).
    pub fn wait_signaled(&self) {
        let waited = unsafe { WaitForSingleObject(self.0.0 as _, INFINITE) };
        debug_assert_eq!(waited, WAIT_OBJECT_0, "WaitForSingleObject failed");
    }

    /// The exit code once the shell has ended; `None` while it is still
    /// running.
    pub fn exit_code(&self) -> Option<u32> {
        let mut code: u32 = 0;
        let read = unsafe { GetExitCodeProcess(self.0.0 as _, &mut code) };
        // An openable process object outlives the process, so `STILL_ACTIVE` is
        // how a live one answers — the same distinction the process layer
        // makes in its test helper.
        (read != 0 && code != STILL_ACTIVE as u32).then_some(code)
    }
}

/// The pseudoconsole handle. Dropping it closes the pseudoconsole, which ends
/// the console every process of the session lives on.
#[derive(Debug)]
struct ConHandle(usize);

impl Drop for ConHandle {
    fn drop(&mut self) {
        unsafe { ClosePseudoConsole(self.0 as _) };
    }
}

/// A freshly created anonymous pipe pair, as (read end, write end).
fn pipe() -> Result<(KernelHandle, KernelHandle), String> {
    let mut read: HANDLE = std::ptr::null_mut();
    let mut write: HANDLE = std::ptr::null_mut();
    let created = unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 0) };
    if created == 0 {
        return Err(last_error("CreatePipe"));
    }
    Ok((KernelHandle(read as usize), KernelHandle(write as usize)))
}

/// RAII ownership of one kernel `HANDLE`, stored as an integer so it stays
/// `Send + Sync` whichever way `windows-sys` spells the type.
#[derive(Debug)]
struct KernelHandle(usize);

impl KernelHandle {
    /// Wrap the handle in a [`File`], transferring close ownership to it.
    fn into_file(self) -> File {
        let raw = self.0;
        std::mem::forget(self);
        File::from_raw_handle(raw as _)
    }
}

impl Drop for KernelHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0 as _) };
    }
}

/// The `PROC_THREAD_ATTRIBUTE_LIST` buffer that attaches a spawned process to
/// the pseudoconsole. The Win32 calls maintain the contents; the vector only
/// owns the bytes, in `u64` units so the buffer keeps pointer alignment.
struct AttributeList(Vec<u64>);

impl AttributeList {
    fn new(capacity: u32) -> Result<Self, String> {
        // Sizing call: with a null list the API only reports the bytes needed.
        let mut needed: usize = 0;
        unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), capacity, 0, &mut needed)
        };
        let mut words = vec![0u64; needed.div_ceil(size_of::<u64>())];
        let initialized = unsafe {
            InitializeProcThreadAttributeList(words.as_mut_ptr().cast(), capacity, 0, &mut needed)
        };
        if initialized == 0 {
            return Err(last_error("InitializeProcThreadAttributeList"));
        }
        Ok(AttributeList(words))
    }

    fn as_mut_ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.0.as_mut_ptr().cast()
    }

    fn set_pseudoconsole(&mut self, con: &ConHandle) -> Result<(), String> {
        let updated = unsafe {
            UpdateProcThreadAttribute(
                self.as_mut_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
                con.0 as _,
                size_of::<usize>(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if updated == 0 {
            return Err(last_error("UpdateProcThreadAttribute"));
        }
        Ok(())
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.as_mut_ptr()) };
    }
}

/// The child's environment: the Hub's own environment with the terminal
/// identity set, so console applications can query what kind of terminal they
/// are talking to (`TERM` is a property of the terminal host, not a policy of
/// whoever started the session).
fn environment_block() -> Vec<u16> {
    let mut block: Vec<u16> = Vec::new();
    for (key, value) in std::env::vars_os() {
        if key.as_os_str() == OsStr::new("TERM") {
            continue; // replaced by the layer's own value below
        }
        block.extend(key.encode_wide());
        block.push('=' as u16);
        block.extend(value.encode_wide());
        block.push(0);
    }
    block.extend("TERM=xterm-256color".encode_utf16());
    block.push(0);
    block.push(0); // the block ends with an empty entry
    block
}

/// The `CreateProcessW` command line: quoted program followed by quoted
/// arguments, all NUL-terminated UTF-16.
fn wide_command_line(program: &Path, args: &[String]) -> Vec<u16> {
    let mut line = quote_argument(&program.to_string_lossy());
    for argument in args {
        line.push(' ');
        line.push_str(&quote_argument(argument));
    }
    line.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Quote one argv entry for a `CreateProcessW` command line. An entry is quoted
/// when it is empty or contains a space, tab or quote; embedded quotes are
/// backslash-escaped. (Full MSVCRT quoting rules are not implemented — the
/// Hub's own configuration is the argv source, not an arbitrary shell line.)
fn quote_argument(argument: &str) -> String {
    let tricky = argument.is_empty()
        || argument
            .bytes()
            .any(|byte| matches!(byte, b' ' | b'\t' | b'"'));
    if !tricky {
        return argument.to_owned();
    }
    let mut quoted = String::with_capacity(argument.len() + 2);
    quoted.push('"');
    for character in argument.chars() {
        if character == '"' {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

/// Describe the last Win32 failure as a bare cause — the raw error code and the
/// call it came from. The PTY layer names the operation it wrapped this into,
/// so repeating the operation here would read as `X failed: X failed (...)`
/// (`docs/DEVELOPMENT.md` §9).
fn last_error(call: &str) -> String {
    let code = unsafe { GetLastError() };
    format!("Win32 error {code} from {call}")
}
