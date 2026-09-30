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

use std::time::Duration;

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
    let activation = core.activate(session_id)?;
    let window = stand_forward(core, session_id, &activation);
    Ok(OpenOutcome { activation, window })
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
