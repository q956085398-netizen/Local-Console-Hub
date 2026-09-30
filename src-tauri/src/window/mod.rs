//! The windows an application owns — finding one, bringing it forward, closing
//! it (#66).
//!
//! An entry configured as `display: window` keeps the window its application
//! provides (`docs/DECISIONS.md` D-034). The Hub's job then is the one it can
//! actually do: remember which run it started, find that run's window again
//! when the user asks for it a second time, and say honestly what happened when
//! it cannot.
//!
//! ## What is here, and what is a platform detail
//!
//! The *selection* — which of a run's windows is the one the user means by
//! "唤起原窗口" — is a product rule, so it is a pure function over an
//! enumeration ([`main_window`]) and is tested without a window server. The
//! enumeration and the two window operations are the platform's, behind a thin
//! backend (`win` / `unsupported`), exactly like `process`, `pty` and `shell`.
//!
//! ## Nothing here searches by name
//!
//! Every lookup is by **process id**, and the caller gets those ids from the
//! run's job object rather than from a remembered number (`crate::process`).
//! A registry of window titles matched against a configuration would be the
//! "同名识别" spec #59 decision 11 rules out for reuse: two applications can be
//! called ComfyUI, and the Hub has no way to tell them apart from a string.

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
use unsupported as backend;
#[cfg(windows)]
use win as backend;

/// A top-level window handle.
///
/// Opaque on purpose: the layers above this one ask *this* module to focus or
/// close the window they picked, so no raw `HWND` travels through Session Core
/// or the IPC contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowHandle(usize);

impl WindowHandle {
    /// Wrap a raw platform handle. Backends only.
    pub(crate) fn from_raw(raw: usize) -> Self {
        WindowHandle(raw)
    }
}

/// One top-level window belonging to an application the Hub knows about.
///
/// A reading, not a handle that keeps anything alive: a window can be
/// destroyed between the enumeration and the call, and every operation below
/// answers what it observed rather than assuming the window is still there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopLevelWindow {
    pub handle: WindowHandle,
    /// The process that owns this window.
    pub pid: u32,
    /// Caption, for telling the user which window was found. Never used to
    /// *find* it.
    pub title: String,
    /// Whether Windows considers the window visible. A minimized window is
    /// visible; a hidden helper window is not.
    pub visible: bool,
    /// Whether the window is minimized — which is a window to restore, not one
    /// to skip.
    pub minimized: bool,
    /// Whether another window owns this one. An owned window is a dialog or a
    /// tool window belonging to something else, so it is not the window the
    /// user means by "the application's window".
    pub owned: bool,
}

/// What bringing a window forward did.
///
/// Two outcomes and no third: Windows either performed the activation or
/// refused it, and a refusal is a fact about the foreground lock rather than a
/// failure this layer may paper over (spec #59 decision 11 — 唤起失败时报告真实
/// 结果，不通过重复启动补救).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusOutcome {
    /// The window was restored if minimized and brought to the foreground.
    Focused,
    /// Windows refused the foreground change. The window was still restored,
    /// so it is visible on screen; the user's next click will land in it.
    Refused,
}

/// The windows of `pids`, as Windows currently reports them.
///
/// The pids come from the run's job object, which is the Hub's ownership
/// boundary: a pid that has been reused by an unrelated process cannot appear
/// here, because that process is not in the job (`crate::process`).
pub fn windows_of(pids: &[u32]) -> Vec<TopLevelWindow> {
    backend::windows_of(pids)
}

/// The console window Windows hosts for `pid`, if that process has a console
/// on screen.
///
/// A console program's window does not belong to it: Windows gives every
/// console a host process of its own, and the window the user sees is the
/// host's. So the ordinary by-pid enumeration cannot find it — and for exactly
/// the applications this mode exists for (a launcher that opens its own console)
/// that window is the one the user means by "唤起原窗口".
///
/// `pid` must be a process the caller has established is still its own run
/// (`crate::process::ProcessIdentity`): this is a lookup *by number*, and a
/// number Windows has reused would answer with a console belonging to somebody
/// else.
pub fn console_window(pid: u32) -> Option<TopLevelWindow> {
    backend::console_window(pid)
}

/// The window a running application presents, given the processes that belong
/// to it (#66, #67).
///
/// An application's window is not always its own process's: a launcher hands
/// off to the program that really runs, and a console program's window is
/// hosted for it by Windows on a process of its own ([`console_window`]). Both
/// shapes mean the same thing to a user asking to be shown the application, so
/// both are tried — the by-pid enumeration over everything the application
/// started, then the console host.
///
/// `lead` is the process the application is identified by, and
/// `lead_is_current` says whether that number still answers for the process it
/// was taken from. It gates only the console lookup, which is a lookup *by
/// number*: a pid Windows has reused would answer with a console belonging to
/// somebody else (spec #59 decision 11).
pub fn application_window(
    pids: &[u32],
    lead: u32,
    lead_is_current: bool,
) -> Option<TopLevelWindow> {
    let windows = windows_of(pids);
    if let Some(found) = main_window(&windows) {
        return Some(found.clone());
    }
    if !lead_is_current {
        return None;
    }
    console_window(lead)
}

/// How often the bounded wait looks again.
const WINDOW_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// [`application_window`], waiting up to `timeout` for one to appear.
///
/// The caller supplies the process list on each round rather than once, because
/// the list is a reading that goes stale: a run that is still starting can add
/// processes, and a window drawn by the program a launcher handed off to must
/// still be found. `timeout` of zero is a single read, which is what a caller
/// asking about something that has been running for a while wants.
pub fn wait_for_application_window(
    processes: impl Fn() -> (Vec<u32>, u32, bool),
    timeout: std::time::Duration,
) -> Option<TopLevelWindow> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let (pids, lead, lead_is_current) = processes();
        if let Some(window) = application_window(&pids, lead, lead_is_current) {
            return Some(window);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(WINDOW_POLL);
    }
}

/// The window a run's application presents to the user.
///
/// A visible, unowned, titled window is what "唤起原窗口" means; a visible
/// unowned window without a caption is the fallback. An invisible window is not
/// a candidate at all: bringing a window nobody can see to the foreground would
/// report a success the user cannot observe, and the run would look open while
/// nothing was on screen.
pub fn main_window(windows: &[TopLevelWindow]) -> Option<&TopLevelWindow> {
    let unowned = || windows.iter().filter(|window| !window.owned);
    unowned()
        .find(|window| window.visible && !window.title.is_empty())
        .or_else(|| unowned().find(|window| window.visible))
}

impl TopLevelWindow {
    /// Bring this window forward, reporting what Windows did.
    pub fn focus(&self) -> FocusOutcome {
        backend::focus(self.handle)
    }

    /// Ask this window to close, as clicking its close button would.
    ///
    /// This is the graceful request for a windowed application — it is what the
    /// user does by hand — and it is best-effort by design: an application with
    /// unsaved work can answer with a dialog instead of exiting, and the stop
    /// timeout is what then decides (`crate::process`).
    pub fn close(&self) -> bool {
        backend::close(self.handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(pid: u32, title: &str, visible: bool, owned: bool) -> TopLevelWindow {
        TopLevelWindow {
            handle: WindowHandle::from_raw(pid as usize),
            pid,
            title: title.to_owned(),
            visible,
            minimized: false,
            owned,
        }
    }

    /// The ordinary case: one visible window with a caption.
    #[test]
    fn a_visible_titled_window_is_the_one_the_user_means() {
        let windows = vec![
            window(7, "", true, false),
            window(7, "ComfyUI", true, false),
            window(7, "hidden", false, false),
        ];

        let main = main_window(&windows).expect("a window is found");

        assert_eq!(main.title, "ComfyUI");
    }

    /// A dialog owned by another window is not the application's window, even
    /// when it is the only visible thing on screen.
    #[test]
    fn an_owned_window_is_not_the_applications_window() {
        let windows = vec![
            window(7, "保存更改？", true, true),
            window(7, "ComfyUI", true, false),
        ];

        assert_eq!(
            main_window(&windows).expect("a window is found").title,
            "ComfyUI"
        );
    }

    /// A minimized window is a window to restore, not one to skip.
    #[test]
    fn a_minimized_window_is_still_the_window() {
        let mut minimized = window(7, "ComfyUI", true, false);
        minimized.minimized = true;

        let windows = [minimized];
        let found = main_window(&windows).expect("a window is found");

        assert!(found.minimized);
    }

    /// An application with only hidden windows has no window to bring forward,
    /// and saying so is the answer — not bringing an invisible window "up".
    #[test]
    fn hidden_windows_are_not_candidates() {
        let windows = vec![
            window(7, "helper", false, false),
            window(7, "", false, false),
        ];

        assert!(main_window(&windows).is_none());
    }

    /// A visible window with no caption is still better than reporting that the
    /// application has no window at all.
    #[test]
    fn an_untitled_visible_window_is_the_fallback() {
        let windows = vec![window(7, "", true, false)];

        assert!(main_window(&windows).is_some());
    }
}
