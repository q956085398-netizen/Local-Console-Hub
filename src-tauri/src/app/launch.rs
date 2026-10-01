//! Launch requests — the app layer's half of "one Hub, and later launches hand
//! off to it" (#60, #63).
//!
//! The instance layer settles *whether* this process is the Hub and carries a
//! later launch's request to it ([`crate::instance`]); this module is what the
//! Hub answers with. Both answers are short. A normal open restores the window,
//! and it does it with the tray's own operation, so there is a single
//! definition of what "bring the Hub back" means (spec §2: "窗口、快捷方式与托盘
//! 使用同一应用操作边界"). A new-terminal request adds a terminal through the
//! same Session Core call the in-app entry uses (#62), so the shortcut and the
//! button produce terminals by one path and not two — and it puts the window on
//! the terminal it just made, which is the half of "新建 PowerShell" that only
//! the window can do.
//!
//! ## Why the Hub makes the terminal itself
//!
//! The request could have been handed to the window to carry out, and that
//! would have been less code. It would also make the answer a lie: the process
//! that asked would be told "delivered" before a shell existed, and a shortcut
//! clicked while the page was still loading would be dropped on the floor
//! (spec §3: 冷启动期间有界接收并等待服务可用，不能静默丢弃). Creating the terminal
//! here means the answer is about something that really happened — and a
//! directory that is not there comes back as the reason, to the process whose
//! user asked for it.
//!
//! ## The wait, and why it is not a poll
//!
//! A request can arrive before the Hub has finished starting — the whole point
//! of a cold-start race is that the second launch overlaps the first (spec §5,
//! user story 5). The Hub therefore publishes its own handle once, and a
//! request that arrives first waits on a condition variable for a bounded time
//! (spec §14: a list kept consistent by continuous polling is not the answer).
//!
//! The bound is what makes the answer honest. A request that outlived it is
//! answered with the reason rather than with a hopeful `delivered`, so the
//! process that asked can report it — and, crucially, can *not* start a second
//! Hub to make up for it (spec §3).

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::instance::{Request, RequestHandler, Response};
use crate::session::core::SessionCore;

/// How long a launch request waits for the Hub to finish starting.
///
/// Sized against the slowest honest start — a Hub whose WebView2 has never been
/// initialised on this machine — and against the client's own bound, which is
/// longer ([`crate::instance::DELIVERY_TIMEOUT`]): the usual outcome of a slow
/// start is this Hub's answer rather than that process's timeout.
pub const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// `session-opened` — a launch request made a terminal and the window is to
/// put the user in it (#63).
///
/// A request to the window, not a session event: it says what the user asked
/// for and carries no lifecycle claim. It is deliberately *not* the tray's
/// `session-focus-requested` (`crate::tray`), even though both make the window
/// show a session — a tray row means "let me look at this one", and this means
/// "the entry you clicked made a terminal; land on it". The window does less
/// for the first and more for the second.
pub const SESSION_OPENED: &str = "session-opened";

/// Payload of [`SESSION_OPENED`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOpened {
    pub session_id: String,
}

/// What a new-terminal request is told when the Hub has no registry to add one
/// to.
///
/// The condition [`NO_SESSION_CORE`] names for an application, in the words a
/// terminal request needs — a Hub publishes its handle only after `manage` has
/// run — and answered for the same reason: a hopeful `delivered` would be a lie
/// about a terminal that was never made.
const NO_REGISTRY: &str = "已经有一个 Hub 在运行，但它现在无法新建终端。

                           请从托盘打开它，或退出后重新启动；本次启动不会另开一个 Hub。";

/// What a launch is told when the Hub never got as far as being able to answer.
///
/// Deliberately about the *request* rather than about the window: it answers
/// both operations now (#63), and a sentence that promised a restore to a
/// shortcut that asked for a terminal would misdescribe what did not happen.
const NOT_READY: &str = "已经有一个 Hub 在运行，但它还没有完成启动，未能处理本次请求。\n\n\
                         请稍后重试；本次启动不会另开一个 Hub。";

/// What a launch is told when the running Hub has no window to restore.
///
/// A Hub without its main window is not a state this app reaches on its own —
/// closing the window hides it (D-006) — so this is the answer to a Hub that is
/// running but not in a shape a launch can do anything with.
const NO_WINDOW: &str = "已经有一个 Hub 在运行，但它现在没有可以恢复的主窗口。\n\n\
                         请从托盘打开它，或退出后重新启动；本次启动不会另开一个 Hub。";

/// What an application request is told when the Hub has no registry to open it
/// on.
///
/// A Hub publishes its handle only after `manage` has run, so this is the
/// answer to a Hub that is running in a shape a launch cannot act on — the
/// same condition [`NO_WINDOW`] covers for a plain open.
const NO_SESSION_CORE: &str = "已经有一个 Hub 在运行，但它现在无法启动或唤起配置的应用。\n\n\
                               请从托盘打开它；本次启动不会另开一个 Hub。";

/// One value, published once, waited for by anyone who arrives first.
///
/// Generic over the value so the wait can be exercised without an app: the
/// behaviour under test is "arrives before, waits, or gives up", and none of it
/// is about what is being waited for.
pub struct Ready<T> {
    value: Mutex<Option<T>>,
    wake: Condvar,
}

impl<T: Clone> Default for Ready<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Ready<T> {
    pub fn new() -> Self {
        Ready {
            value: Mutex::new(None),
            wake: Condvar::new(),
        }
    }

    /// Hand the value over and wake everyone waiting for it.
    ///
    /// Repeat publishes replace rather than accumulate: there is one Hub per
    /// process, and a second publish would mean this was called twice.
    pub fn publish(&self, value: T) {
        let mut slot = lock(&self.value);
        *slot = Some(value);
        drop(slot);
        self.wake.notify_all();
    }

    /// The value, as soon as it is published, or `None` when `timeout` passes.
    ///
    /// The loop is not an optimisation: `wait_timeout` may return without a
    /// notification, and a single pass would turn that into "the Hub never
    /// became ready" — the one answer that must not be given by accident.
    pub fn wait(&self, timeout: Duration) -> Option<T> {
        let deadline = Instant::now() + timeout;
        let mut slot = lock(&self.value);

        loop {
            if let Some(value) = slot.as_ref() {
                return Some(value.clone());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return None;
            }
            let (next, _) = self
                .wake
                .wait_timeout(slot, remaining)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            slot = next;
        }
    }
}

/// The session the window has been asked to show, and has not been told about.
///
/// A type of its own rather than a field on [`Hub`], and that is not tidiness:
/// it is the one part of this module a test can reach without dragging the
/// Tauri application handle — and, through it, the whole window stack — into
/// the test binary. `cargo test` links that stack into an executable with no
/// comctl32 v6 manifest, and the loader then refuses the image outright
/// (`STATUS_ENTRYPOINT_NOT_FOUND`, measured on Windows 11 10.0.26300). A test
/// that names `Hub` costs every test in the crate its run, so the state worth
/// asserting is kept where tests can hold it without naming `Hub` at all.
#[derive(Default)]
pub struct PendingFocus {
    state: Mutex<FocusState>,
}

/// What [`PendingFocus`] holds, behind one lock.
///
/// One lock rather than two flags, so "has a window attached" cannot be read
/// before the ask and written after it — the interleaving that would leave a
/// value nobody takes.
#[derive(Default)]
struct FocusState {
    /// The ask, until the window that was starting takes it.
    pending: Option<String>,
    /// Whether a window has taken one at least once.
    attached: bool,
}

impl PendingFocus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask for `session_id` to be shown, until a window has attached.
    ///
    /// Replacing rather than queueing: a window that is not attached yet has
    /// one page load coming, and the last terminal the user asked for is the
    /// one it should land on.
    ///
    /// Not kept once a window has attached, and that is what makes [`take`]'s
    /// promise true rather than approximate. The window subscribes and *then*
    /// reads (see `ipc::launch`), so a window that has read is one that will
    /// hear the next event — keeping a copy for it as well would mean the next
    /// page load selected a terminal nobody asked about. The narrow case this
    /// leaves open is a launch that lands while a page is loading *after* an
    /// earlier page already attached (a WebView reload): the ask is neither
    /// stored nor heard, so the terminal is made and shown without being
    /// selected. That is the rarer of the two, and the quieter one — the Hub
    /// still opens on the terminal the user asked for.
    ///
    /// [`take`]: Self::take
    fn ask(&self, session_id: &str) {
        let mut state = lock(&self.state);
        if !state.attached {
            state.pending = Some(session_id.to_owned());
        }
    }

    /// The ask, taken rather than copied — and the window is attached from here
    /// on, whether or not there was anything to take.
    pub fn take(&self) -> Option<String> {
        let mut state = lock(&self.state);
        state.attached = true;
        state.pending.take()
    }
}

/// The running Hub, as the instance layer's request handler.
///
/// Built before the app exists and published into as the app starts, because
/// the pipe that receives requests has to be listening from the first moment —
/// a Hub that opened its window before it could be reached would lose exactly
/// the launches that arrive during its own start-up.
pub struct Hub {
    ready: Ready<AppHandle<Wry>>,
    /// The terminal the most recent launch request asked the window to show.
    ///
    /// See [`Hub::open_session`] for why an ask that is *also* published as an
    /// event has to be kept here as well.
    focus: PendingFocus,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        Hub {
            ready: Ready::new(),
            focus: PendingFocus::new(),
        }
    }

    /// Announce that the Hub can answer requests from here on.
    ///
    /// Called once, from the app's `setup`, after the window exists: what is
    /// published is the ability to *do* something with a request, so publishing
    /// earlier would replace one honest answer ("not ready yet") with a
    /// dishonest one.
    pub fn publish(&self, app: AppHandle<Wry>) {
        self.ready.publish(app);
    }

    /// The session the window has been asked to show and has not been told
    /// about yet.
    pub fn take_pending_focus(&self) -> Option<String> {
        self.focus.take()
    }

    /// What a new-terminal request does once the Hub can act on it.
    fn new_terminal(&self, app: &AppHandle<Wry>, directory: Option<String>) -> Response {
        let Some(core) = app.try_state::<SessionCore>() else {
            return Response::failed(NO_REGISTRY);
        };

        match core.create_temporary_terminal(directory.as_deref()) {
            Ok(created) => {
                self.open_session(app, &created.config.id);
                Response::delivered()
            }
            Err(error) => {
                // The window comes forward even so: the user clicked an entry
                // and is owed the sight of the Hub whatever it has to say, and
                // the sentence itself is carried back to the process that
                // asked (`lib.rs` shows it when this Hub is the one that
                // answered its own start-up request).
                crate::tray::show_window(app);
                Response::failed(format!("新建终端失败：{}", error.message))
            }
        }
    }

    /// Open the window on `session_id`, and keep the ask for a window that
    /// cannot hear it yet.
    ///
    /// The event is what a window that is already listening acts on — the
    /// shortcut clicked while the Hub sits in the tray, and the second click of
    /// a pair. It is not enough on its own: a shortcut that *starts* the Hub is
    /// answered from `setup`, before the page has loaded, and an event
    /// published to a page that has no listener yet is simply gone. So the ask
    /// is stored first and published second, which covers both orders — a
    /// window that subscribes and then reads ([`crate::ipc::launch`]) is caught
    /// by whichever half it arrives between.
    ///
    /// [`SESSION_OPENED`] rather than the tray's event, because the two asks are
    /// not the same size: a tray row means "let me look at this session", while
    /// this means "put me in the terminal you just made" — and the window acts
    /// on them differently (see `src/app/App.tsx`).
    fn open_session(&self, app: &AppHandle<Wry>, session_id: &str) {
        self.focus.ask(session_id);
        crate::tray::show_window(app);
        let _ = app.emit(
            SESSION_OPENED,
            SessionOpened {
                session_id: session_id.to_owned(),
            },
        );
    }
}

impl RequestHandler for Hub {
    fn handle(&self, request: Request) -> Response {
        let Some(app) = self.ready.wait(READY_TIMEOUT) else {
            return Response::failed(NOT_READY);
        };
        match request {
            Request::Open => {
                // The answer says what the Hub did, not what the screen
                // shows: the restore is issued here and drawn afterwards
                // (see `tray::show_window`). "There was no window to
                // restore" is the failure that can be told apart without
                // racing the window thread, so it is the one reported
                // instead of a `delivered` that observed nothing.
                if crate::tray::show_window(&app) {
                    Response::delivered()
                } else {
                    Response::failed(NO_WINDOW)
                }
            }
            // A normal open must stay a normal open: this arm is the whole of
            // what a shortcut asking for a shell does differently (stories 10
            // and 12, and H01's "no terminal comes with an open").
            Request::NewTerminal { directory } => self.new_terminal(&app, directory),
            Request::OpenApplication { id } => {
                let response = open_application(&app, &id);
                if response.delivered {
                    // A cold launch precedes the frontend listener, just as
                    // a new-terminal request does. Retain its selection too.
                    self.open_session(&app, &id);
                }
                response
            }
        }
    }
}

/// Open one configured application and bring the window to it (#64, #66).
///
/// The opening itself is the app layer's ([`crate::app::activation::open`],
/// which is Session Core's [`SessionCore::activate`] plus the window step the
/// entries that keep their own window need) — the same call the window's own
/// control makes, so the two entries cannot drift apart (spec #59 §2). What is
/// decided here is only what a *launch request* adds: the Hub's window comes
/// forward on the application it opened, and a refusal is reported as the
/// reason it was refused rather than as a delivered request that did nothing.
///
/// A standalone application whose own window could not be brought forward is
/// still a delivered request — the Hub did what was asked, and the answer says
/// what it observed. Failing the request would tell the shortcut's launcher the
/// Hub could not open the application at all, which is not what happened.
///
/// ## The one answer a launch request cannot give (#67)
///
/// When the Hub finds something outside itself it will not decide about, the
/// question has to go to the user — and a shortcut has no dialog to put it in.
/// So this is refused with the reason, and the Hub's window comes forward: the
/// user is told that an instance is already running and that the Hub did not
/// start a second copy, and the choice itself is one click away, on the entry
/// they can now see. A `delivered` here would be a lie about an application
/// nothing was done with, and starting a copy anyway is the duplication the
/// whole of #67 exists to prevent.
fn open_application(app: &AppHandle<Wry>, id: &str) -> Response {
    let Some(core) = app.try_state::<SessionCore>() else {
        return Response::failed(NO_SESSION_CORE);
    };
    match crate::app::activation::open(&core, id) {
        Ok(outcome) if outcome.choice.is_some() => {
            let reason = outcome
                .choice
                .map(|choice| choice.reason)
                .unwrap_or_default();
            crate::tray::focus_session(app, id);
            Response::failed(format!(
                "{reason}\n\nHub 没有为此再启动一份。请在 Hub 里打开这个应用，\
                 那里可以关联已经在运行的那个实例，或者明确新开一份。"
            ))
        }
        Ok(_) => Response::delivered(),
        Err(error) => Response::failed(error.message),
    }
}

/// A mutex guard that outlives the panic that poisoned it.
///
/// The value here is an `Option` of something `Clone`; nothing under this lock
/// can panic, so a poisoned lock would be another thread's panic being paid for
/// by a launch that only wanted to know whether the Hub was up.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A request that arrives after start-up gets the value, not a timeout.
    #[test]
    fn a_wait_returns_the_value_once_it_is_published() {
        let ready: Ready<&'static str> = Ready::new();
        ready.publish("hub");

        assert_eq!(ready.wait(Duration::from_millis(50)), Some("hub"));
    }

    /// A request that arrives *during* start-up waits for it rather than being
    /// told the Hub is missing — the cold-start race, in one thread.
    #[test]
    fn a_wait_that_arrives_first_is_woken_by_the_publish() {
        let ready: std::sync::Arc<Ready<u32>> = std::sync::Arc::new(Ready::new());
        let publisher = {
            let ready = std::sync::Arc::clone(&ready);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                ready.publish(7);
            })
        };

        let started = Instant::now();
        let value = ready.wait(Duration::from_secs(10));
        let elapsed = started.elapsed();
        publisher.join().expect("the publisher did not panic");

        assert_eq!(value, Some(7), "the publish is what ends the wait");
        assert!(
            elapsed >= Duration::from_millis(100),
            "the wait cannot return before the value exists: {elapsed:?}"
        );
    }

    /// The bound is real: a Hub that never publishes produces `None` inside
    /// [`READY_TIMEOUT`], which is what becomes the "not ready" answer instead
    /// of a hopeful success.
    #[test]
    fn a_wait_gives_up_when_nothing_is_ever_published() {
        let ready: Ready<u32> = Ready::new();

        let started = Instant::now();
        let value = ready.wait(Duration::from_millis(150));
        let elapsed = started.elapsed();

        assert_eq!(value, None);
        assert!(
            elapsed >= Duration::from_millis(150),
            "the wait has to use its bound: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "the wait has to end: {elapsed:?}"
        );
    }

    /// Nothing has been asked for on a window that has just opened. The window
    /// reads this on every start, so an invented id here would move the
    /// selection on a plain launch.
    #[test]
    fn nothing_is_pending_until_something_is_asked_for() {
        assert_eq!(PendingFocus::new().take(), None);
    }

    /// The ask is *taken*, not copied: it belongs to the window that was
    /// starting when the terminal was made, and a value that survived a second
    /// read would drag the next page load onto an old terminal.
    #[test]
    fn a_pending_focus_is_taken_once() {
        let focus = PendingFocus::new();
        focus.ask("terminal-1");

        assert_eq!(focus.take().as_deref(), Some("terminal-1"));
        assert_eq!(focus.take(), None, "the second read has to find nothing");
    }

    /// The latest ask wins: a window that is not attached yet has one page load
    /// coming, and the terminal made last is the one it should land on.
    #[test]
    fn the_latest_ask_replaces_the_earlier_one() {
        let focus = PendingFocus::new();
        focus.ask("terminal-1");
        focus.ask("terminal-2");

        assert_eq!(focus.take().as_deref(), Some("terminal-2"));
        assert_eq!(focus.take(), None);
    }

    /// Once a window has taken an ask, later asks are not kept for it: it
    /// hears them as events, and a copy left behind would be taken by the next
    /// page load and move the selection to a terminal nobody asked about.
    #[test]
    fn an_ask_after_the_window_attached_is_not_kept() {
        let focus = PendingFocus::new();

        assert_eq!(focus.take(), None, "the first read attaches the window");
        focus.ask("terminal-1");

        assert_eq!(
            focus.take(),
            None,
            "a window that has already read hears the event instead"
        );
    }

    /// The failure sentence tells the user what to do and what *not* to expect,
    /// because a Hub that is still starting is the case where a second Hub
    /// would look like a fix and is not one.
    #[test]
    fn the_not_ready_answer_says_the_hub_is_there_and_will_not_be_duplicated() {
        assert!(
            NOT_READY.contains("不会另开一个 Hub"),
            "the sentence has to say what does not happen: {NOT_READY}"
        );
        assert!(
            NOT_READY.contains("稍后重试"),
            "the sentence has to say what the user can do: {NOT_READY}"
        );
    }
}
