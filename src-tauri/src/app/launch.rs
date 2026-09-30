//! Launch requests — the app layer's half of "one Hub, and later launches hand
//! off to it" (#60).
//!
//! The instance layer settles *whether* this process is the Hub and carries a
//! later launch's request to it ([`crate::instance`]); this module is what the
//! Hub answers with. The answer is deliberately short: the one operation that
//! exists today restores the window, and it does it with the tray's own
//! operation, so there is a single definition of what "bring the Hub back"
//! means (spec §2: "窗口、快捷方式与托盘使用同一应用操作边界").
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

use tauri::{AppHandle, Manager, Wry};

use crate::instance::{Request, RequestHandler, Response};
use crate::session::core::SessionCore;

/// How long a launch request waits for the Hub to finish starting.
///
/// Sized against the slowest honest start — a Hub whose WebView2 has never been
/// initialised on this machine — and against the client's own bound, which is
/// longer ([`crate::instance::DELIVERY_TIMEOUT`]): the usual outcome of a slow
/// start is this Hub's answer rather than that process's timeout.
pub const READY_TIMEOUT: Duration = Duration::from_secs(15);

/// What a launch is told when the Hub never got as far as being able to answer.
const NOT_READY: &str = "已经有一个 Hub 在运行，但它还没有完成启动，未能恢复窗口。\n\n\
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

/// The running Hub, as the instance layer's request handler.
///
/// Built before the app exists and published into as the app starts, because
/// the pipe that receives requests has to be listening from the first moment —
/// a Hub that opened its window before it could be reached would lose exactly
/// the launches that arrive during its own start-up.
pub struct Hub {
    ready: Ready<AppHandle<Wry>>,
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
            Request::OpenApplication { id } => open_application(&app, &id),
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
fn open_application(app: &AppHandle<Wry>, id: &str) -> Response {
    let Some(core) = app.try_state::<SessionCore>() else {
        return Response::failed(NO_SESSION_CORE);
    };
    match crate::app::activation::open(&core, id) {
        Ok(_) => {
            // Shown, then pointed at the session, in that order — the same
            // pair the tray uses when a row is clicked (`tray::focus_session`).
            // Whether this call started the run or found it already going is
            // not the answer the caller needs: both are "it is open".
            crate::tray::focus_session(app, id);
            Response::delivered()
        }
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
