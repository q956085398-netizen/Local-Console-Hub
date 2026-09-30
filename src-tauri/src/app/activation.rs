//! Opening an application and bringing its own window forward (#66, spec #59
//! decisions 10 and 11).
//!
//! [`SessionCore::activate`] answers the lifecycle half of "open this
//! application": nothing running means start it once, something running means
//! that run, something stopping means a refusal rather than a second copy. What
//! it cannot answer is the part that is about a *window* — an application
//! configured as `display: window` presents itself somewhere the Hub does not
//! draw, and "clicking the entry" means the user wants that window in front of
//! them.
//!
//! That composition is this module, and it is one function because there are two
//! callers that must not drift: the window's own start control (`ipc`) and a
//! launch request handed to the Hub from a shortcut (`app::launch`). Both get
//! the same answer, which is also what makes the answer *testable* with no
//! window at all — the two halves are separable and the lifecycle half is
//! Session Core's.
//!
//! ## Failure is reported, never compensated
//!
//! An application with no window on screen, or one Windows refuses to bring to
//! the foreground, is reported as exactly that. Starting a second copy to make
//! the click look like it worked is the one thing spec #59 decision 11 rules
//! out: it would leave two instances of something the user asked to *see*.
//!
//! ## What is running *outside* the Hub (#67)
//!
//! The lifecycle half above only knows about runs the Hub started. An
//! application the user launched by hand — from Explorer, from another
//! terminal — is invisible to it, and clicking its entry would start a second
//! copy of something already running: exactly the duplication this decision is
//! about.
//!
//! So the search comes first, and only when the Hub has nothing of its own for
//! the session. [`crate::app::external`] decides what can be *proved* about
//! what is out there; this module turns each of its three answers into one of
//! the three things a click can mean:
//!
//! - nothing found — the ordinary open, unchanged;
//! - one confirmed instance — associate it and bring its window forward, and
//!   start nothing;
//! - anything else — answer with the question
//!   ([`OpenChoice`]) and change nothing at all, because which of two
//!   running programs is "the" application is the user's to say, and a Hub
//!   that guessed would be guessing about somebody's work.

use std::time::Duration;

use crate::app::external::{self, Outside};
use crate::process::ExternalProcess;
use crate::session::core::{Activation, SessionCore, SessionError};
use crate::session::state::SessionStatus;
use crate::window::{FocusOutcome, TopLevelWindow};

/// How long a just-started application is given to put its window up.
///
/// The same bound the process layer waits with, named once here so a caller of
/// this module reads the number where the decision is: an application that is
/// starting is expected to be slow, and one that has been running for a while
/// is not waited for at all.
const WINDOW_WAIT: Duration = crate::process::independent::WINDOW_WAIT;

/// What opening one application did.
#[derive(Debug, Clone)]
pub struct OpenOutcome {
    /// The lifecycle answer: the run in flight, and whether this call made it.
    pub activation: Activation,
    /// What happened to the application's own window.
    ///
    /// `None` for a session the Hub displays: it has no second window, and the
    /// caller restores the Hub's window itself, which is a different operation
    /// (`tray::focus_session`).
    pub window: Option<WindowStep>,
    /// The Hub found something outside itself it will not silently reuse — or
    /// silently duplicate (#67).
    ///
    /// Present means nothing was started and nothing was associated: what
    /// happened is a question, and it is the caller's to put to the user.
    pub choice: Option<OpenChoice>,
}

/// Why the Hub is asking, and what it is asking about (#67).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenChoice {
    /// What the Hub could not establish, in a sentence.
    pub reason: String,
    /// The instances it found, so the answer is about real things.
    pub candidates: Vec<external::Candidate>,
}

/// The outcome of asking an application's window to come forward.
#[derive(Debug, Clone)]
pub enum WindowStep {
    /// The window was restored if minimized and brought to the foreground.
    Focused(TopLevelWindow),
    /// Windows refused the foreground change. The window was still restored, so
    /// it is on screen; the user's next click lands in it.
    Refused(TopLevelWindow),
    /// The application is running and has no window to bring forward — it is
    /// still starting, or a windowless program.
    NoWindow,
}

impl WindowStep {
    /// What to tell the user when the window did not come forward.
    ///
    /// `None` when it did: a notice for the ordinary case would be noise. The
    /// two other answers are sentences the user can act on, and both say what
    /// the Hub did *not* do, because "nothing happened" is otherwise
    /// indistinguishable from "the entry is broken" (story 48).
    pub fn notice(&self) -> Option<String> {
        match self {
            WindowStep::Focused(_) => None,
            WindowStep::NoWindow => Some(
                "应用已经在运行，但现在没有可以唤起的窗口。\n\n\
                 它使用的是独立窗口显示；如果窗口还没出现，稍后再点一次即可。\
                 Hub 不会为此再启动一份。"
                    .to_owned(),
            ),
            WindowStep::Refused(window) => Some(format!(
                "已恢复「{}」，但 Windows 拒绝了前台切换；点击它的任务栏按钮即可回到该应用。\n\n\
                 本次打开没有另启一份。",
                described(window)
            )),
        }
    }

    /// The window this step is about, for a caller that wants to name it.
    pub fn window(&self) -> Option<&TopLevelWindow> {
        match self {
            WindowStep::Focused(window) | WindowStep::Refused(window) => Some(window),
            WindowStep::NoWindow => None,
        }
    }
}

/// The name to call a window in a sentence a user reads.
fn described(window: &TopLevelWindow) -> &str {
    if window.title.is_empty() {
        "该应用的窗口"
    } else {
        &window.title
    }
}

/// Open one configured application: activate it, then bring its own window
/// forward when it keeps one (#66).
///
/// Opening is idempotent because that is what [`SessionCore::activate`] is; what
/// this adds is the window step and the two facts it needs — whether the entry
/// keeps its own window at all, and how long to wait for one that has not
/// appeared yet.
pub fn open(core: &SessionCore, session_id: &str) -> Result<OpenOutcome, SessionError> {
    let config = core
        .session_config(session_id)
        .ok_or_else(|| SessionError::unknown_session(session_id, "activate"))?;
    let before = core
        .snapshot(session_id)
        .ok_or_else(|| SessionError::unknown_session(session_id, "activate"))?;

    // Only an entry that keeps its own window can be associated with an
    // instance outside the Hub, and only while the Hub has nothing of its own
    // for this session: an open that is already accounted for is the ordinary
    // activation, and looking outside it would be looking for something to
    // worry about.
    let open_in_hub = core.is_external(session_id)
        || matches!(
            before.status,
            SessionStatus::Starting | SessionStatus::Running | SessionStatus::Stopping
        );
    if !open_in_hub && external::applies_to(&config) {
        match external::find(&config) {
            Outside::Certain(instance) => return attach(core, session_id, instance),
            Outside::Ambiguous(ambiguity) => {
                return Ok(OpenOutcome {
                    // The session is exactly as it was: nothing was started,
                    // and the runtime says so rather than describing a run that
                    // does not exist.
                    activation: Activation {
                        runtime: before,
                        started: false,
                    },
                    window: None,
                    choice: Some(OpenChoice {
                        reason: ambiguity.reason,
                        candidates: ambiguity.candidates,
                    }),
                });
            }
            Outside::None => {}
        }
    }

    open_now(core, session_id)
}

/// Open the Hub's own copy, whatever is running outside (#67).
///
/// The "明确新开" half of [`OpenChoice`]. Releasing the association first is
/// what makes it a *decision* rather than a retry: without it the open would
/// look outside again, find the same instance, and ask the same question
/// forever. The instance itself is not touched — the Hub never owned it — and
/// the session is back where it was, so the open that follows creates the
/// Hub's own run.
pub fn open_new(core: &SessionCore, session_id: &str) -> Result<OpenOutcome, SessionError> {
    core.release_adopted(session_id)?;
    open_now(core, session_id)
}

/// Answer this session's [`OpenChoice`] by associating a candidate (#67).
///
/// The candidate is re-verified rather than trusted: what the window sent back
/// is a pid and a creation time, and both are facts about a moment that has
/// passed. [`external::confirm`] is what turns them back into a statement about
/// a process that exists *now*.
pub fn associate(
    core: &SessionCore,
    session_id: &str,
    pid: u32,
    created_at: u64,
) -> Result<OpenOutcome, SessionError> {
    let config = core
        .session_config(session_id)
        .ok_or_else(|| SessionError::unknown_session(session_id, "activate"))?;

    let instance = external::confirm(&config, pid, created_at)
        .map_err(|reason| SessionError::failed(session_id, "activate", reason, None))?;
    attach(core, session_id, instance)
}

/// The Hub's own half of an open, with no look outside (#67).
fn open_now(core: &SessionCore, session_id: &str) -> Result<OpenOutcome, SessionError> {
    let activation = core.activate(session_id)?;
    let window = stand_forward(core, session_id, &activation);
    Ok(OpenOutcome {
        activation,
        window,
        choice: None,
    })
}

/// Associate one identified instance and bring its window forward (#67).
///
/// Opening the process comes first, and it is not a formality: it is what
/// verifies the identity against the process object rather than against the
/// number, and what gives the session a handle to notice the ending through.
/// An instance that ends between being found and being associated is refused
/// here, which is why there is no path from this function that reports a
/// running application that is not.
fn attach(
    core: &SessionCore,
    session_id: &str,
    instance: external::ExternalInstance,
) -> Result<OpenOutcome, SessionError> {
    let process = ExternalProcess::open(instance.identity).map_err(|error| {
        SessionError::failed(
            session_id,
            "activate",
            format!("无法关联已在运行的实例：{error}"),
            None,
        )
    })?;
    let runtime = core.adopt(session_id, instance.identity, process)?;

    let activation = Activation {
        runtime,
        started: false,
    };
    let window = stand_forward(core, session_id, &activation);
    Ok(OpenOutcome {
        activation,
        window,
        choice: None,
    })
}

/// Bring the application's window forward, if this entry has one.
fn stand_forward(
    core: &SessionCore,
    session_id: &str,
    activation: &Activation,
) -> Option<WindowStep> {
    if !core.keeps_own_window(session_id) {
        return None;
    }

    // A run this call started — or one another open is in the middle of
    // starting — is given a moment to build its window. An application that has
    // already been running is asked once: if it has no window *now*, waiting
    // would only delay the same answer.
    let wait = if activation.started || activation.runtime.status == SessionStatus::Starting {
        WINDOW_WAIT
    } else {
        Duration::ZERO
    };

    Some(match core.application_window(session_id, wait) {
        Some(window) => match window.focus() {
            FocusOutcome::Focused => WindowStep::Focused(window),
            FocusOutcome::Refused => WindowStep::Refused(window),
        },
        None => WindowStep::NoWindow,
    })
}

/// What a *click* does with what [`crate::app::external`] found (#67).
///
/// The search and the association each have their own suite; what is pinned
/// here is the composition between them, because that is where the three
/// answers become the three things a click can mean — and where a mistake would
/// show up as the one outcome #67 exists to prevent, a second copy of an
/// application that was already running.
#[cfg(all(test, windows))]
mod native_tests {
    use super::*;
    use crate::app::external::native_tests::{Fixture, KillOnDrop};
    use crate::session::core::SessionCore;
    use crate::session::state::SessionStatus;

    /// A core holding the fixture entry, as a configured application would be.
    fn core_with(fixture: &Fixture) -> SessionCore {
        let core = SessionCore::without_listener();
        core.register(fixture.config())
            .expect("the fixture entry registers");
        core
    }

    /// The headline behaviour: an application already running is associated,
    /// and nothing is started.
    #[test]
    fn an_application_already_running_outside_is_associated_not_started() {
        let fixture = Fixture::new("associate");
        let mut running = KillOnDrop(fixture.start());
        fixture.wait_until_listed(&mut running.0);
        let core = core_with(&fixture);

        let outcome = open(&core, "fixture").expect("opening succeeds");

        assert!(outcome.choice.is_none(), "there was nothing to ask about");
        assert!(
            !outcome.activation.started,
            "the Hub started nothing: the application was already running"
        );
        assert!(outcome.activation.runtime.external);
        assert_eq!(outcome.activation.runtime.pid, Some(running.pid()));
        assert_eq!(
            outcome.activation.runtime.status,
            SessionStatus::Running,
            "the application really is running"
        );
        assert_eq!(
            outcome.activation.runtime.run_id, None,
            "the Hub opened nothing of its own"
        );
        assert!(core.is_external("fixture"));
    }

    /// Nothing outside means the ordinary open, unchanged: this is the path
    /// every entry took before #67, and it must stay exactly as it was.
    #[test]
    fn nothing_outside_opens_the_hubs_own_copy() {
        let fixture = Fixture::new("start");
        let core = core_with(&fixture);

        let outcome = open(&core, "fixture").expect("opening succeeds");

        assert!(outcome.choice.is_none());
        assert!(outcome.activation.started, "the Hub started its own run");
        assert!(!outcome.activation.runtime.external);
        assert!(
            outcome.activation.runtime.run_id.is_some(),
            "a run the Hub started has a run id"
        );

        core.force_stop("fixture").expect("cleanup");
    }

    /// Two instances are a question, and answering it is the user's: nothing
    /// was started, nothing was associated, and the session is exactly as it
    /// was — which is also what cancelling leaves behind.
    #[test]
    fn two_instances_ask_and_change_nothing() {
        let fixture = Fixture::new("ask");
        let mut first = KillOnDrop(fixture.start());
        let mut second = KillOnDrop(fixture.start());
        fixture.wait_until_listed(&mut first.0);
        fixture.wait_until_listed(&mut second.0);
        let core = core_with(&fixture);

        let outcome = open(&core, "fixture").expect("opening succeeds");

        let choice = outcome.choice.expect("the Hub asks instead of deciding");
        let pids: Vec<u32> = choice.candidates.iter().map(|one| one.pid).collect();
        assert!(pids.contains(&first.pid()), "{pids:?}");
        assert!(pids.contains(&second.pid()), "{pids:?}");
        assert!(outcome.window.is_none(), "nothing was brought forward");
        let runtime = core.snapshot("fixture").expect("the session is registered");
        assert_eq!(runtime.status, SessionStatus::Stopped);
        assert_eq!(runtime.pid, None);
        assert!(!runtime.external);
    }

    /// The two answers to the question. Associating takes the instance the user
    /// picked; starting new goes through the Hub's own path and leaves the
    /// instance the user rejected exactly as it was.
    #[test]
    fn the_user_can_associate_the_one_they_picked_or_start_a_new_one() {
        let mut fixture = Fixture::new("resolve");
        let mut running = KillOnDrop(fixture.start());
        fixture.wait_until_listed(&mut running.0);
        let core = core_with(&fixture);

        // What the dialog would have been shown for a confirmed instance.
        let instance = match external::find(&fixture.config()) {
            Outside::Certain(instance) => instance,
            other => panic!("expected one confirmed instance, got {other:?}"),
        };

        let associated = associate(
            &core,
            "fixture",
            instance.identity.pid(),
            instance.identity.created_at(),
        )
        .expect("associating the instance succeeds");
        assert_eq!(associated.activation.runtime.pid, Some(running.pid()));
        assert!(associated.activation.runtime.external);
        assert!(running.is_alive(), "associating must not end the instance");

        // 明确新开: the Hub lets go of that instance and creates its own run.
        let started = open_new(&core, "fixture").expect("starting a new copy succeeds");
        assert!(started.activation.started);
        assert!(!started.activation.runtime.external);
        assert_ne!(started.activation.runtime.pid, Some(running.pid()));
        assert!(
            running.is_alive(),
            "明确新开 leaves the instance the user rejected alone"
        );
        let _ = &mut fixture;

        core.force_stop("fixture").expect("cleanup");
    }

    /// Once the Hub has its own run, a later open is the ordinary one — the
    /// question is not asked again about an instance the session already
    /// accounts for.
    #[test]
    fn an_open_after_the_hub_started_its_own_copy_asks_nothing() {
        let fixture = Fixture::new("settled");
        let core = core_with(&fixture);
        open(&core, "fixture").expect("the first open starts the Hub's copy");

        let again = open(&core, "fixture").expect("the second open succeeds");

        assert!(again.choice.is_none());
        assert!(!again.activation.started, "nothing second was started");
        core.force_stop("fixture").expect("cleanup");
    }
}
