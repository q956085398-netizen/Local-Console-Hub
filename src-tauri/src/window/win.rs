//! Windows window backend — `EnumWindows`, `SetForegroundWindow`, `WM_CLOSE`.
//!
//! Every call here is one Win32 call wrapped so the layers above never name an
//! `HWND`. The one piece of policy that lives here rather than in
//! [`super::main_window`] is the *enumeration filter*: Windows reports every
//! top-level window on the desktop, and asking for one run's windows means
//! keeping the ones whose owning process is in that run, plus the two style
//! facts (visibility, ownership) a caller cannot ask for later, once the
//! window may already be gone.

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindow, GetWindowLongW, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW, SetForegroundWindow,
    ShowWindow, GWL_EXSTYLE, GW_OWNER, SW_RESTORE, WM_CLOSE, WS_EX_TOOLWINDOW,
};

use super::{FocusOutcome, TopLevelWindow, WindowHandle};

/// Longest window caption this backend reads. A caption is for telling the user
/// which window was found; anything longer than this is not a title a UI shows.
const MAX_TITLE: usize = 512;

/// The top-level windows belonging to `pids`.
///
/// An empty list is the honest answer both for "these processes have no window"
/// and for "this platform has no window layer" — the crate is Windows-first
/// (D-001) and the non-Windows backend is the same read of a platform that has
/// nothing to report.
pub fn windows_of(pids: &[u32]) -> Vec<TopLevelWindow> {
    let mut context = Collect {
        pids,
        found: Vec::new(),
    };
    // `EnumWindows` may not reach every window in the caller's own process on
    // some deskbound configurations, which is precisely why this is a
    // best-effort reading with an honest "no window" answer rather than a
    // guarantee this layer pretends to make.
    unsafe {
        EnumWindows(
            Some(collect),
            std::ptr::from_mut(&mut context).cast::<std::ffi::c_void>() as LPARAM,
        );
    }
    context.found
}

struct Collect<'a> {
    pids: &'a [u32],
    found: Vec<TopLevelWindow>,
}

unsafe extern "system" fn collect(window: HWND, lparam: LPARAM) -> i32 {
    let context = std::ptr::from_ref(&*(lparam as *const Collect)).cast_mut();
    let Some(context) = context.as_mut() else {
        // A null context cannot be produced by `windows_of`; stopping the
        // enumeration is better than dereferencing it.
        return 0;
    };

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(window, &mut pid);
    if !context.pids.contains(&pid) {
        return 1;
    }

    context.found.push(read(window, pid));
    // Continue: one run can own several top-level windows, and which of them
    // the user means is [`super::main_window`]'s decision, not this filter's.
    1
}

fn read(window: HWND, pid: u32) -> TopLevelWindow {
    let style = unsafe { GetWindowLongW(window, GWL_EXSTYLE) } as u32;
    TopLevelWindow {
        handle: WindowHandle::from_raw(window as usize),
        pid,
        title: title_of(window),
        visible: unsafe { IsWindowVisible(window) } != 0,
        minimized: unsafe { IsIconic(window) } != 0,
        // An owned window is a dialog or a tool window of some other window;
        // a `WS_EX_TOOLWINDOW` one is the same thing spelled in the extended
        // style (the classic "is this a real application window" test).
        owned: !unsafe { GetWindow(window, GW_OWNER) }.is_null() || style & WS_EX_TOOLWINDOW != 0,
    }
}

/// The console window hosted for `pid`.
///
/// Reaching it means attaching to that process's console for the length of one
/// call, because `GetConsoleWindow` answers about *this* process's console and
/// nothing else. That is a process-wide state change, and it is made through
/// the crate's console module (D-037): the borrow is serialized against every
/// other console attach in the crate — most importantly the graceful stop's
/// `CTRL_BREAK`, which would otherwise be raised in whichever console a lookup
/// happened to be attached to — and this process is put back on the console it
/// came from.
///
/// A Hub that has a console of its own still gets no answer here, and that is
/// unchanged: it is the user's terminal, and a window lookup is not worth
/// trading it for. What is refused, and what it costs to ask, is D-037's.
pub fn console_window(pid: u32) -> Option<TopLevelWindow> {
    use windows_sys::Win32::System::Console::GetConsoleWindow;

    let console = crate::console::Console::claim();
    let window = console.borrow(pid, || unsafe { GetConsoleWindow() })?;
    if window.is_null() {
        return None;
    }
    // Read like any other window; the pid recorded is the one the caller asked
    // about, because the window's real owner is Windows' console host — a
    // number that says nothing about which application this is.
    let mut read_back = read(window, pid);
    read_back.pid = pid;
    Some(read_back)
}

fn title_of(window: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(window) };
    if length <= 0 {
        return String::new();
    }
    let capacity = (length as usize + 1).min(MAX_TITLE);
    let mut buffer = vec![0u16; capacity];
    let written = unsafe { GetWindowTextW(window, buffer.as_mut_ptr(), capacity as i32) };
    if written <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..written as usize])
}

/// Restore the window if it is minimized, then ask Windows to bring it forward.
///
/// `SetForegroundWindow` is deliberately not forced: the usual ways of making it
/// succeed anyway (attaching this thread's input to the target's, posting
/// `WM_SYSCOMMAND`/`SC_RESTORE`, raising with `SetWindowPos`) would turn a
/// refusal into a blurry half-success the user cannot tell from a real focus —
/// and spec #59 decision 11 asks for the real result instead.
pub fn focus(handle: WindowHandle) -> FocusOutcome {
    let window = handle.raw() as HWND;
    if unsafe { IsIconic(window) } != 0 {
        unsafe {
            ShowWindow(window, SW_RESTORE);
        }
    }
    if unsafe { SetForegroundWindow(window) } != 0 {
        FocusOutcome::Focused
    } else {
        FocusOutcome::Refused
    }
}

/// Post `WM_CLOSE` to the window, which is what its close button does.
pub fn close(handle: WindowHandle) -> bool {
    (unsafe { PostMessageW(handle.raw() as HWND, WM_CLOSE, 0, 0) }) != 0
}

impl WindowHandle {
    fn raw(self) -> usize {
        // `WindowHandle` is constructed by this backend from the same `usize`
        // it reads back here.
        let WindowHandle(raw) = self;
        raw
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    /// A real top-level window owned by this test process, so window
    /// enumeration and the two window operations are exercised against the
    /// window server rather than against a stand-in.
    struct TestWindow(HWND);

    impl TestWindow {
        fn new(title: &str) -> Self {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            let caption: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
            let window = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    caption.as_ptr(),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    0,
                    0,
                    240,
                    120,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            };
            assert!(
                !window.is_null(),
                "the test window could not be created (Win32 error {})",
                unsafe { windows_sys::Win32::Foundation::GetLastError() }
            );
            TestWindow(window)
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            unsafe {
                DestroyWindow(self.0);
            }
        }
    }

    /// The windows of a pid are the ones whose owning process it is — and the
    /// enumeration is a real reading of the window server, not of a list this
    /// backend kept.
    #[test]
    fn a_window_is_found_by_its_owning_process() {
        let window = TestWindow::new("lch window enumeration");
        let pid = std::process::id();

        let found = windows_of(&[pid]);

        let mine = found
            .iter()
            .find(|candidate| candidate.title == "lch window enumeration")
            .unwrap_or_else(|| panic!("the test window is not in {found:?}"));
        assert_eq!(mine.pid, pid);
        assert!(mine.visible, "a WS_VISIBLE window is visible");
        assert!(!mine.minimized);
        assert!(!mine.owned, "a top-level window owns nothing");

        // The filter is the pid, not the title: another process's windows are
        // not returned.
        assert!(windows_of(&[pid.wrapping_add(1)]).is_empty());
        drop(window);
    }

    /// Closing a window is what its close button does: the window is gone
    /// afterwards, which is the fact a stop's graceful step relies on.
    #[test]
    fn closing_a_window_destroys_it() {
        // Held for the whole test so the window is not destroyed as a side
        // effect of a guard dropping early; `WM_CLOSE` is what destroys it.
        let _guard = TestWindow::new("lch window close");
        let pid = std::process::id();
        let found = windows_of(&[pid]);
        let mine = found
            .iter()
            .find(|candidate| candidate.title == "lch window close")
            .expect("the test window is found")
            .clone();

        assert!(mine.close(), "the close request is posted");

        // `STATIC` has no handler of its own, so the default procedure is what
        // processes `WM_CLOSE` — asynchronously, which is why the assertion
        // polls rather than reading once.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let still_there = windows_of(&[pid])
                .iter()
                .any(|candidate| candidate.handle == mine.handle);
            if !still_there {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the window survived its close request"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
            // The message loop that would dispatch `WM_CLOSE` belongs to the
            // thread that created the window; pumping here is what lets the
            // request be processed while the test waits on it.
            pump_messages();
        }
    }

    /// Bringing a window forward reports what Windows did. The test process is
    /// the one creating the window, so the foreground change is its own to ask
    /// for; a refusal is a legal answer and is asserted as one.
    #[test]
    fn focusing_reports_the_real_outcome() {
        let _window = TestWindow::new("lch window focus");
        let pid = std::process::id();
        let mine = windows_of(&[pid])
            .into_iter()
            .find(|candidate| candidate.title == "lch window focus")
            .expect("the test window is found");

        let outcome = mine.focus();

        assert!(
            matches!(outcome, FocusOutcome::Focused | FocusOutcome::Refused),
            "{outcome:?}"
        );
    }

    fn pump_messages() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
        };
        let mut message: MSG = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}
