//! The Windows half of the instance layer (#60): a named mutex says whether a
//! Hub is already running, and a named pipe is how a later launch reaches it.
//!
//! ## Two objects, because one cannot say both things
//!
//! `CreateMutexW` is the **claim**. It is atomic — of two processes started in
//! the same instant exactly one creates the object and the other is told
//! `ERROR_ALREADY_EXISTS` — and it is the first thing the Hub does, before any
//! session supervisor exists (spec §2). A pipe cannot replace it: two processes
//! can each create *an instance* of the same pipe name, so "my pipe opened"
//! does not mean "I am the only Hub".
//!
//! The pipe is the **channel**. The spec requires a later launch to come back
//! with the real outcome (§3), and a signal with no reply cannot say whether
//! the Hub actually restored its window. It is also where the cold start is
//! absorbed: opening a window takes seconds, and a second launch during that
//! window waits for the Hub instead of being told nothing.
//!
//! ## What protects the channel
//!
//! Both objects are created with `lpSecurityAttributes = NULL`, which gives
//! them the **creator's default DACL** — the Hub's own user, SYSTEM and
//! Administrators — and both live in the `Local\` namespace, which is this
//! Windows session rather than the whole machine. Another user on the same
//! machine therefore cannot open the pipe and ask the Hub for anything, and
//! `PIPE_REJECT_REMOTE_CLIENTS` closes the remote half of the same question.
//!
//! That boundary is deliberate rather than incidental: this surface exists to
//! be extended (#62's temporary terminal would ask the Hub to open a shell), and
//! a request surface that can do that must be reachable by its user and by
//! nobody else.
//!
//! ## The time budget
//!
//! Three bounds have to nest, or the client reports a timeout while the Hub is
//! still working on the request:
//!
//! ```text
//! client  DELIVERY_TIMEOUT    20 s   connect, then wait for the answer
//!   server REQUEST_READ_TIMEOUT 3 s  the peer has connected; where is its frame
//!   server app::launch::READY_TIMEOUT 15 s  the Hub's own start-up
//! ```
//!
//! 3 + 15 = 18 s of server-side work, inside the 20 s the client allows.

use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_NO_DATA,
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
    INVALID_HANDLE_VALUE,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FlushFileBuffers, ReadFile, WriteFile, FILE_SHARE_NONE, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PeekNamedPipe, WaitNamedPipeW,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
    PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, Sleep};

use super::protocol::{self, FrameError, Request, Response};
use super::{InstanceError, RequestHandler};

/// How long, after a connection, the Hub waits for the whole request frame.
///
/// The peer is a process on this machine that has already built its request, so
/// this bound is about a client that connected and then stalled — not about a
/// slow one. It is short so that a stall costs the Hub's listener a second, not
/// a minute.
const REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(3);

/// How long a client waits between polls of an answer that has not arrived.
///
/// A named pipe has no read timeout, so the wait is a peek loop. It costs one
/// syscall per interval on a path that runs when the user opens the Hub, which
/// is nothing next to the work of opening a window.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// One pipe's buffer, in bytes.
///
/// A frame is a few dozen bytes and the largest one this build reads is 8 KiB;
/// the kernel's staging area only has to hold one.
const PIPE_BUFFER_BYTES: u32 = 8 * 1024;

/// The claim on the session's Hub slot, held by the mutex object.
///
/// Dropping it lets the object go, and with it the slot: keeping this alive for
/// the process's lifetime is what makes "one Hub" true for as long as the Hub
/// runs. [`Primary::serve`] therefore hands it to the process rather than to
/// the listener thread — see the comment there.
#[derive(Debug)]
struct Claim {
    _mutex: OwnedHandle,
}

/// The Hub's half of the instance layer: the claim, and the pipe it serves.
///
/// Handed to [`Primary::serve`], which consumes it. There is no way to hold one
/// without serving requests, because a Hub that owns the slot but cannot be
/// reached would make every later launch fail — the outcome the spec calls
/// worse than starting a second Hub (§3).
#[derive(Debug)]
pub struct Primary {
    claim: Claim,
    pipe_name: String,
}

impl Primary {
    /// Answer launch requests until the process ends.
    ///
    /// A connection is served on a thread of its own. The listener's next job
    /// is to have the next pipe instance ready, and a peer that connects and
    /// then says nothing (or dies) must not be able to keep the following
    /// launch from being heard.
    pub fn serve(self, handler: Arc<dyn RequestHandler>) {
        let Primary { claim, pipe_name } = self;

        // The claim belongs to the **process**, not to the listener below: a Hub
        // whose listener gave up is still the Hub, and a later launch that found
        // the slot free would start a second one — the outcome this layer exists
        // to prevent. Holding the handle until the process ends is what makes
        // "one Hub" unconditional, and Windows closes it on exit, so there is
        // nothing to leak. (If the listener does give up, the later launch
        // reports that its request was not delivered rather than being told
        // nothing.)
        std::mem::forget(claim);

        std::thread::spawn(move || {
            loop {
                let pipe = match create_pipe_instance(&pipe_name) {
                    Ok(pipe) => pipe,
                    Err(error) => {
                        // The Hub itself is fine; only hand-off is lost, and a
                        // later launch reports that honestly. Retrying would
                        // spin on a condition (a squatted name, a denied DACL)
                        // that does not heal by itself.
                        eprintln!(
                            "{app}: cannot listen for launch requests on {pipe_name}: {error}; \
                             a second launch will report that it could not be delivered",
                            app = crate::ipc::APP_NAME
                        );
                        return;
                    }
                };

                match accept_connection(raw(&pipe)) {
                    Ok(()) => {
                        let handler = Arc::clone(&handler);
                        std::thread::spawn(move || serve_connection(pipe, &*handler));
                    }
                    // The client connected and vanished before we asked for it.
                    // Nothing was asked of the Hub, so nothing is lost by
                    // offering the next instance.
                    Err(error) if closed_by_client(&error) => continue,
                    Err(error) => {
                        eprintln!(
                            "{app}: the launch listener stopped: {error}; a second launch will \
                             report that it could not be delivered",
                            app = crate::ipc::APP_NAME
                        );
                        return;
                    }
                }
            }
        });
    }
}

/// Try to become the Hub, or find out that one is already running.
///
/// `Ok(Some(_))` means this process created the mutex and owns the slot.
/// `Ok(None)` means the object already existed — an ordinary outcome, and the
/// caller's cue to hand its request over. `Err` is the OS refusing to answer at
/// all, which is reported rather than guessed at.
pub fn claim(mutex_name: &str, pipe_name: &str) -> Result<Option<Primary>, InstanceError> {
    let name = wide(mutex_name);
    // SAFETY: `name` is NUL-terminated and outlives the call; the call has no
    // other precondition, and the handle it returns is owned below.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(InstanceError::Unavailable(format!(
            "无法确认是否已有 Hub 在运行（{error}）。为避免同时运行两个 Hub，本次启动已停止，\
             请稍后重试。",
            error = io::Error::last_os_error()
        )));
    }
    // Read immediately: `CreateMutexW` reports "the object was already there"
    // through the thread's last error, and any later call would overwrite it.
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

    // SAFETY: the handle came from `CreateMutexW` above and nothing else owns
    // it, so `OwnedHandle` closes it exactly once.
    let mutex = unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) };
    if already_exists {
        // The object belongs to the Hub that made it. Closing our handle to it
        // is correct — we hold no claim on it.
        return Ok(None);
    }

    Ok(Some(Primary {
        claim: Claim { _mutex: mutex },
        pipe_name: pipe_name.to_owned(),
    }))
}

/// Carry `request` to the Hub behind `pipe_name`, and bring back its answer.
pub fn deliver(
    pipe_name: &str,
    request: Request,
    timeout: Duration,
) -> Result<Response, InstanceError> {
    let deadline = Instant::now() + timeout;
    let pipe = connect(pipe_name, deadline)?;

    write_frame(raw(&pipe), &request).map_err(|error| {
        InstanceError::NotDelivered(format!(
            "正在运行的 Hub 无法接收本次请求（{error}）。本次启动不会另开一个 Hub。"
        ))
    })?;

    let frame = read_frame(raw(&pipe), deadline).map_err(|error| match error {
        FrameError::TimedOut => InstanceError::NotDelivered(format!(
            "已有 Hub 在运行，但它没有在 {} 秒内应答本次请求。\n\n\
             请确认那个 Hub 是否仍然响应，然后重试；Local Console Hub 不会为此另开一个实例。",
            timeout.as_secs()
        )),
        error => InstanceError::NotDelivered(format!(
            "已经运行的 Hub 没有给出可用的应答（{error}）。本次启动不会另开一个 Hub。"
        )),
    })?;

    let response = protocol::decode_response(&frame).map_err(|error| {
        InstanceError::NotDelivered(format!(
            "已经运行的 Hub 给出了无法理解的应答（{error}）。本次启动不会另开一个 Hub。"
        ))
    })?;

    if response.delivered {
        return Ok(response);
    }
    Err(InstanceError::NotDelivered(
        response
            .message
            .unwrap_or_else(|| "正在运行的 Hub 拒绝了本次请求，但没有说明原因。".to_owned()),
    ))
}

/// The Hub's side of one connection: read a request, answer it, hang up.
fn serve_connection(pipe: OwnedHandle, handler: &dyn RequestHandler) {
    let handle = raw(&pipe);
    let deadline = Instant::now() + REQUEST_READ_TIMEOUT;

    // Every failure still produces an answer: a client that gets a sentence
    // back can tell the user what went wrong, while one that is left waiting
    // would report a timeout that says nothing about the cause.
    let response = match read_frame(handle, deadline) {
        Ok(frame) => match protocol::decode_request(&frame) {
            Ok(request) => handler.handle(request),
            Err(error) => Response::failed(format!(
                "正在运行的 Hub 不认识本次启动请求（{error}）；它可能来自另一个版本。"
            )),
        },
        Err(FrameError::Closed) => return, // the client gave up; nothing to answer
        Err(error) => Response::failed(format!("本次启动请求没有完整送达 Hub（{error}）。")),
    };

    let _ = write_frame(handle, &response);
    // Written, and now *read*: for a byte-mode pipe, `FlushFileBuffers` blocks
    // until the peer has taken the answer, and `DisconnectNamedPipe` discards
    // whatever is still in the buffer. Flushing first is what keeps the answer
    // from being thrown away the moment it was most needed.
    //
    // SAFETY: both calls take the pipe handle this function owns; neither
    // retains anything.
    unsafe {
        FlushFileBuffers(handle);
        DisconnectNamedPipe(handle);
    }
}

/// Create one instance of the Hub's pipe, ready to be connected to.
fn create_pipe_instance(pipe_name: &str) -> io::Result<OwnedHandle> {
    let name = wide(pipe_name);
    // SAFETY: `name` is NUL-terminated and outlives the call; a null
    // `SECURITY_ATTRIBUTES` asks for the creator's default DACL (see the module
    // comment). The handle is owned below, and closed exactly once.
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            std::ptr::null(),
        )
    };
    if handle == INVALID_HANDLE_VALUE || handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the handle came from `CreateNamedPipeW` and nothing else owns it.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) })
}

/// Wait for a client on one pipe instance.
fn accept_connection(handle: HANDLE) -> io::Result<()> {
    // SAFETY: a null `OVERLAPPED` is the synchronous form — this thread blocks
    // here until a client arrives, which is the listener's whole job.
    let connected = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if connected != 0 {
        return Ok(());
    }
    match io::Error::last_os_error() {
        // A client that connected between `CreateNamedPipeW` and this call is a
        // connection, not a failure.
        error if error.raw_os_error() == Some(ERROR_PIPE_CONNECTED as i32) => Ok(()),
        error => Err(error),
    }
}

/// Whether a connection failure means the peer is already gone.
fn closed_by_client(error: &io::Error) -> bool {
    matches!(
        error.raw_os_error(),
        Some(code)
            if code == ERROR_NO_DATA as i32 || code == ERROR_BROKEN_PIPE as i32
    )
}

/// Open the Hub's pipe, waiting for it to exist within `deadline`.
///
/// Both halves of the wait are real at cold start: the Hub creates the object
/// that says "I am here" before it creates the pipe, so a launch that overlaps
/// its start finds `ERROR_FILE_NOT_FOUND`, and one that arrives while the
/// previous connection is being handed off finds `ERROR_PIPE_BUSY`. Neither is
/// a reason to start a second Hub, so both are waited out rather than reported
/// (spec §3: 冷启动期间有界接收并等待服务可用).
fn connect(pipe_name: &str, deadline: Instant) -> Result<OwnedHandle, InstanceError> {
    let name = wide(pipe_name);
    loop {
        // SAFETY: `name` is NUL-terminated and outlives the call; the remaining
        // parameters are the documented ones for opening an existing pipe for
        // reading and writing. The handle is owned below.
        let handle = unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_NONE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE && !handle.is_null() {
            // SAFETY: the handle came from `CreateFileW` and nothing else owns it.
            return Ok(unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) });
        }

        let error = io::Error::last_os_error();
        let waiting = matches!(
            error.raw_os_error(),
            Some(code) if code == ERROR_FILE_NOT_FOUND as i32 || code == ERROR_PIPE_BUSY as i32
        );
        if !waiting || Instant::now() >= deadline {
            return Err(cannot_connect(&error));
        }

        // `ERROR_FILE_NOT_FOUND` is the cold-start case — the Hub has claimed
        // the slot but has not created its first pipe instance yet — and
        // `ERROR_PIPE_BUSY` is a connection being handed off to its own thread.
        // `WaitNamedPipeW` covers the second and returns immediately on the
        // first, so the sleep is what keeps a name that does not exist yet from
        // turning into a spin.
        //
        // SAFETY: `name` is NUL-terminated and outlives the call; the timeout is
        // a plain millisecond count. A timeout here is not an answer — the loop
        // retries, and `deadline` is what ends it.
        let remaining = deadline.saturating_duration_since(Instant::now());
        unsafe {
            WaitNamedPipeW(
                name.as_ptr(),
                remaining.as_millis().min(u32::MAX as u128) as u32,
            );
            Sleep(POLL_INTERVAL.as_millis().min(u32::MAX as u128) as u32);
        }

        if Instant::now() >= deadline {
            return Err(cannot_connect(&error));
        }
    }
}

/// What a launch that cannot reach the Hub it was told about is shown.
///
/// The sentence has two halves on purpose: what failed, and the fact that this
/// process did *not* react by starting a second Hub. The second half is the
/// part the user cannot see and the part the spec is emphatic about (§3), so it
/// is said rather than assumed.
fn cannot_connect(error: &io::Error) -> InstanceError {
    InstanceError::NotDelivered(format!(
        "已有 Hub 在运行，但无法连接到它（{error}）。\n\n\
         本次启动不会另开一个 Hub；请确认那个 Hub 是否仍然响应，然后重试。"
    ))
}

/// Write one frame, terminated, to a pipe handle.
fn write_frame(handle: HANDLE, value: &impl serde::Serialize) -> Result<(), FrameError> {
    let frame = protocol::encode(value)?;
    let mut written = 0u32;
    // SAFETY: `frame` is a live buffer of exactly `frame.len()` bytes and the
    // handle is an open pipe; a null `OVERLAPPED` is the synchronous form.
    let ok = unsafe {
        WriteFile(
            handle,
            frame.as_ptr(),
            frame.len() as u32,
            &mut written,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(FrameError::Io(io::Error::last_os_error()));
    }
    if written as usize != frame.len() {
        return Err(FrameError::Io(io::Error::new(
            io::ErrorKind::WriteZero,
            format!("wrote {written} of {} bytes", frame.len()),
        )));
    }
    Ok(())
}

/// Read one frame's bytes — everything before its terminator — giving up at
/// `deadline`.
///
/// The peek loop is the bound. A named pipe read has no timeout, so reading
/// straight into a `BufReader` would block for as long as the peer felt like
/// it; asking "is there anything yet" and sleeping between the asks is what
/// keeps [`REQUEST_READ_TIMEOUT`] and [`deliver`]'s deadline honest.
fn read_frame(handle: HANDLE, deadline: Instant) -> Result<Vec<u8>, FrameError> {
    let mut frame: Vec<u8> = Vec::new();

    loop {
        let mut available = 0u32;
        // SAFETY: a null buffer with a zero length asks only for the count; the
        // handle is an open pipe and the remaining parameters are out-params.
        let ok = unsafe {
            PeekNamedPipe(
                handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(pipe_error(io::Error::last_os_error()));
        }

        if available > 0 {
            let mut chunk = vec![0u8; available as usize];
            let mut read = 0u32;
            // SAFETY: `chunk` has room for exactly `available` bytes, which is
            // what the peek just reported is there to read.
            let ok = unsafe {
                ReadFile(
                    handle,
                    chunk.as_mut_ptr(),
                    available,
                    &mut read,
                    std::ptr::null_mut(),
                )
            };
            if ok == 0 {
                return Err(pipe_error(io::Error::last_os_error()));
            }
            chunk.truncate(read as usize);
            frame.extend_from_slice(&chunk);

            if let Some(end) = frame.iter().position(|byte| *byte == b'\n') {
                frame.truncate(end);
                return Ok(frame);
            }
            // Bounded while it grows, so a peer that never sends a terminator
            // cannot make the Hub buffer without limit. `decode` enforces the
            // same rule on the assembled frame; this one is what keeps it from
            // being assembled at all.
            if frame.len() > protocol::MAX_FRAME_BYTES {
                return Err(FrameError::TooLong);
            }
            continue;
        }

        if Instant::now() >= deadline {
            return Err(FrameError::TimedOut);
        }
        // SAFETY: `Sleep` takes a millisecond count and has no other effect.
        unsafe { Sleep(POLL_INTERVAL.as_millis().min(u32::MAX as u128) as u32) };
    }
}

/// What a failed pipe read or peek means to a frame reader.
///
/// The two call sites in [`read_frame`] differ in which call failed, not in what
/// the answer means: a peer that went away is [`FrameError::Closed`] — a
/// disconnect to report as such — and anything else is the transport. It is one
/// function so the two cannot drift apart.
fn pipe_error(error: io::Error) -> FrameError {
    if error.raw_os_error() == Some(ERROR_BROKEN_PIPE as i32) {
        FrameError::Closed
    } else {
        FrameError::Io(error)
    }
}

/// The raw handle behind an owned one, for the Win32 calls above.
fn raw(handle: &OwnedHandle) -> HANDLE {
    handle.as_raw_handle() as HANDLE
}

/// `text` as the NUL-terminated UTF-16 the Win32 names are spelled in.
///
/// Interior NULs are removed rather than passed through: the object names are
/// this crate's own constants, but a truncated name would silently become a
/// *different* object, which is the kind of failure that has to be impossible
/// rather than unlikely.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16()
        .filter(|unit| *unit != 0)
        .chain([0])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::HandOff;
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;

    /// How long the tests allow a delivery that is expected to succeed.
    ///
    /// Generous on purpose: the server's pipe instance is created by a thread
    /// this process just started, so a client may spend a few milliseconds
    /// being told the pipe does not exist yet. That retry path is not what any
    /// of these tests are about.
    const TEST_DELIVERY_TIMEOUT: Duration = Duration::from_secs(10);

    /// How long a delivery that is expected to *fail* is allowed to try.
    ///
    /// Short, so the bound is what the test measures: the failure has to arrive
    /// because the deadline passed, not because the test's patience did.
    const TEST_SHORT_TIMEOUT: Duration = Duration::from_millis(300);

    /// A mutex and pipe name that nothing else in this process is using.
    ///
    /// The mutex objects are per-session, not per-process, so two tests sharing
    /// a name would see each other's claim — and the suite runs them in
    /// parallel by default. The sequence and the process id make the name
    /// unique by construction rather than by timing (the same reasoning as
    /// `RunId::mint` and the app module's temp configs).
    fn names(what: &str) -> (String, String) {
        static SEQUENCE: AtomicU32 = AtomicU32::new(0);

        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let tag = format!("lch-test-{}-{}-{what}", std::process::id(), sequence);
        (format!(r"Local\{tag}"), format!(r"\\.\pipe\{tag}"))
    }

    /// A handler that records what it was asked and answers the given response.
    ///
    /// The instance layer's whole contract with the app layer is "here is a
    /// request, here is an answer", so a recording is the honest double here
    /// rather than a mock of something this layer owns.
    struct Recorder {
        seen: mpsc::Sender<Request>,
        answer: Response,
    }

    impl RequestHandler for Recorder {
        fn handle(&self, request: Request) -> Response {
            // The receiver outlives every delivery these tests make; a send that
            // fails means the test already finished.
            let _ = self.seen.send(request);
            self.answer.clone()
        }
    }

    /// Start a Hub on a fresh pair of names, answering `answer` to every
    /// request, and report what it was asked.
    fn start_hub(what: &str, answer: Response) -> (String, mpsc::Receiver<Request>) {
        let (mutex, pipe) = names(what);
        let primary = claim(&mutex, &pipe)
            .expect("the names are usable")
            .expect("nothing else holds the slot");
        let (seen, receiver) = mpsc::channel();
        primary.serve(Arc::new(Recorder { seen, answer }));
        (pipe, receiver)
    }

    /// A later launch aimed at `pipe`.
    fn launch_at(pipe: &str, timeout: Duration) -> HandOff {
        HandOff {
            pipe_name: pipe.to_owned(),
            timeout,
        }
    }

    /// The first claim owns the slot; the second is told a Hub is already there.
    #[test]
    fn a_second_claim_reports_the_hub_that_holds_the_slot() {
        let (mutex, pipe) = names("claimed");
        let first = claim(&mutex, &pipe).expect("the names are usable");
        assert!(first.is_some(), "the first claim owns the slot");

        let second = claim(&mutex, &pipe).expect("the names are usable");
        assert!(second.is_none(), "the second claim has to hand off");
    }

    /// A later launch reaches the running Hub, which sees the request.
    #[test]
    fn a_hand_off_delivers_the_request_to_the_running_hub() {
        let (pipe, receiver) = start_hub("delivered", Response::delivered());

        let response = launch_at(&pipe, TEST_DELIVERY_TIMEOUT)
            .deliver(Request::Open)
            .expect("the Hub answers");

        assert!(response.delivered);
        assert_eq!(
            receiver.recv_timeout(Duration::from_secs(5)),
            Ok(Request::Open),
            "the Hub has to see the operation the launch asked for"
        );
    }

    /// A Hub that answers "no" is a failed delivery, not a delivered one.
    ///
    /// `Ok(Response { delivered: false, .. })` would be the easy shape to write
    /// and the wrong one: the process that asked is about to exit, and it has to
    /// treat "the Hub refused" exactly like "the Hub never answered".
    #[test]
    fn a_hub_that_refuses_is_reported_as_not_delivered() {
        let (pipe, _receiver) = start_hub("refused", Response::failed("Hub 尚未完成启动。"));

        let error = launch_at(&pipe, TEST_DELIVERY_TIMEOUT)
            .deliver(Request::Open)
            .expect_err("a refusal is a failure");

        let InstanceError::NotDelivered(message) = error else {
            panic!("a refusal is reported as not delivered, got {error:?}");
        };
        assert_eq!(message, "Hub 尚未完成启动。");
    }

    /// A launch that finds the slot taken but no Hub behind it gives up inside
    /// its bound, and says so. It does not start a Hub of its own.
    #[test]
    fn a_hand_off_to_a_hub_that_never_answers_gives_up_within_its_bound() {
        let (mutex, pipe) = names("silent");
        // The slot is held — and nothing is listening on the pipe, which is
        // exactly what a later launch sees while the Hub is between claiming
        // and serving.
        let _primary = claim(&mutex, &pipe)
            .expect("the names are usable")
            .expect("nothing else holds the slot");

        let started = Instant::now();
        let error = launch_at(&pipe, TEST_SHORT_TIMEOUT)
            .deliver(Request::Open)
            .expect_err("nothing can answer");
        let elapsed = started.elapsed();

        assert!(
            elapsed >= TEST_SHORT_TIMEOUT,
            "the wait has to be used, or a slow Hub would be reported as a missing one: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "the wait has to end: {elapsed:?}"
        );
        let InstanceError::NotDelivered(message) = error else {
            panic!("the failure names what did not happen, got {error:?}");
        };
        assert!(
            message.contains("Hub"),
            "the user has to be told it was the Hub that did not answer: {message}"
        );
    }

    /// Two launches in the same instant are two deliveries, not one.
    ///
    /// The listener has one pipe instance at a time and creates the next as
    /// soon as a connection arrives; a client that arrives in that gap is told
    /// `ERROR_PIPE_BUSY` and waits. Getting this wrong would look like a Hub
    /// that silently drops every other click.
    #[test]
    fn two_launches_at_once_are_both_answered() {
        let (pipe, receiver) = start_hub("concurrent", Response::delivered());

        let deliveries: Vec<_> = (0..2)
            .map(|_| {
                let pipe = pipe.clone();
                std::thread::spawn(move || {
                    launch_at(&pipe, TEST_DELIVERY_TIMEOUT).deliver(Request::Open)
                })
            })
            .collect();

        for delivery in deliveries {
            let response = delivery
                .join()
                .expect("the delivery thread did not panic")
                .expect("both launches are answered");
            assert!(response.delivered);
        }
        let answered: Vec<Request> = receiver.iter().take(2).collect();
        assert_eq!(answered.len(), 2, "the Hub serves both launches");
    }
}
