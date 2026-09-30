//! The instance layer — one Hub per Windows session, and a way for a later
//! invocation to reach it (#60).
//!
//! The Hub is meant to *be* the entry point: the user opens it, closes it to
//! the tray, and opens it again — which must return the workspace that is
//! already there, not a second Hub with its own session supervisor
//! (`docs` spec #59, user stories 1–5). This layer is what makes that true. It
//! answers exactly one question — "am I the Hub, or is one already running?" —
//! and, in the second case, carries the request across.
//!
//! ## What it owns, and what it deliberately does not
//!
//! It owns the *claim* and the *delivery*: two OS objects, one protocol
//! ([`protocol`]) and one bounded wait. It knows nothing about windows,
//! sessions or the frontend. What a request *does* is supplied by the caller as
//! a [`RequestHandler`], so `crate::app::launch` can answer with the Hub's real
//! window operations while this layer stays runnable — and testable — with no
//! window at all.
//!
//! ## Order matters
//!
//! [`claim`] must be called before anything else the Hub does, and in
//! particular before a session supervisor exists (spec §2: 单实例确认先于建立
//! 另一份监管器). A process that is not the Hub must never reach the point of
//! holding sessions, because the Hub it is handing off to is already holding
//! them: that is what keeps `config.yaml` from being supervised twice, and the
//! user's log directory from being written by two processes at once.

pub mod protocol;

#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as platform;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
use unsupported as platform;

pub use platform::Primary;
pub use protocol::{Request, Response};

use std::time::Duration;

/// The name that identifies a Hub inside one Windows session.
///
/// `Local\` is the session namespace, and that scope is the honest one to
/// promise: the tray, the window and the shortcut all belong to a desktop, and
/// a logon session is the boundary the user's "open the Hub" gesture lives in.
/// Naming it `Global\` would widen the claim to every session on the machine —
/// and would need `SeCreateGlobalPrivilege`, which an ordinary user does not
/// hold, so it would fail for exactly the user this is for.
pub const MUTEX_NAME: &str = r"Local\LocalConsoleHub.Instance.v1";

/// The pipe a later invocation hands its request to.
///
/// Derived from [`MUTEX_NAME`] on purpose: the two objects answer two halves of
/// one question — "is a Hub there" and "can I reach it" — and a name that
/// drifted from the other would let a second Hub claim a slot whose channel it
/// could not serve.
pub const PIPE_NAME: &str = r"\\.\pipe\LocalConsoleHub.Instance.v1";

/// How long a later invocation waits for the running Hub to answer it.
///
/// The wait has to cover the slowest honest case — a Hub started from cold,
/// whose WebView2 has never been initialised on this machine — and it has to
/// end: a request that waited forever would leave the user with an entry that
/// does nothing and says nothing, which is the failure the spec names
/// ("交付失败报错，不通过新建另一个 Hub 补救"). The Hub's own readiness wait
/// is shorter than this one, so the usual outcome of a slow start is the Hub's
/// answer rather than this process's timeout.
pub const DELIVERY_TIMEOUT: Duration = Duration::from_secs(20);

/// What the running Hub does with a request.
///
/// The seam between "a request arrived" and "here is what it means". It is a
/// trait rather than a function pointer so the app layer can hand over a
/// handler that holds the running app's identity, and so the transport can be
/// exercised against a handler that is nothing but a recording.
pub trait RequestHandler: Send + Sync + 'static {
    fn handle(&self, request: Request) -> Response;
}

/// What this process is: the Hub, or a later launch that found one.
#[derive(Debug)]
pub enum Role {
    /// This process claimed the session's slot and is now the Hub.
    Primary(Primary),
    /// A Hub already holds the slot; the request has to be handed to it.
    HandOff(HandOff),
}

/// Hand a request to the Hub that is already running.
///
/// Created by [`claim`] rather than constructed by the caller, because the only
/// process that may hold one is one that has already seen the Hub's claim.
#[derive(Debug)]
pub struct HandOff {
    pub(crate) pipe_name: String,
    pub(crate) timeout: Duration,
}

impl HandOff {
    /// Carry `request` to the running Hub, and bring back what it answered.
    ///
    /// An `Ok` answer is the Hub's own: it took the request and did what it
    /// asked for. A Hub that answered "no" is reported as an error rather than
    /// as an `Ok` with a flag in it, because from this process's point of view
    /// a request the Hub refused and a request that never arrived are the same
    /// thing: the user asked for something and it did not happen, and this
    /// process is about to exit either way.
    ///
    /// A delivery is attempted **once**, and that is what keeps "相同交付的重试
    /// 避免重复执行" (spec §3) true now that an operation is not idempotent:
    /// restoring a window twice is restoring it once, but adding a terminal
    /// twice is two terminals, so a caller must not retry on this process's
    /// behalf. Nobody does — the call is made once and its outcome decides the
    /// exit code — and the sentence a timeout produces says the *Hub* did not
    /// answer rather than that nothing happened, because a request the Hub took
    /// and answered too late has already been carried out.
    ///
    /// Should a retry ever be wanted, it is this signature that grows the
    /// identity a retry is recognised by; inventing one now would be inventing
    /// it for a request nobody sends.
    pub fn deliver(&self, request: Request) -> Result<Response, InstanceError> {
        platform::deliver(&self.pipe_name, request, self.timeout)
    }
}

/// Claim the session's Hub slot, or find out that it is taken.
///
/// The claim is settled before this returns: either this process owns the slot
/// — and may go on to build a Hub, pipes and all — or a Hub is already running,
/// and the only thing left to do is hand the request over and exit.
///
/// The failure case is not "a Hub exists" — that is [`Role::HandOff`], an
/// ordinary outcome. It is the OS refusing to tell us either way, which is
/// reported rather than assumed, because both assumptions are wrong: assuming
/// "no Hub" would start the duplicate this whole layer exists to prevent, and
/// assuming "a Hub" would refuse to start at all.
pub fn claim() -> Result<Role, InstanceError> {
    match platform::claim(MUTEX_NAME, PIPE_NAME)? {
        Some(primary) => Ok(Role::Primary(primary)),
        None => Ok(Role::HandOff(HandOff {
            pipe_name: PIPE_NAME.to_owned(),
            timeout: DELIVERY_TIMEOUT,
        })),
    }
}

/// Why a launch could not be settled or delivered.
///
/// Both variants carry the sentence the user is shown, because both end the
/// same way: the process that could not settle its role (or could not reach the
/// Hub) is about to exit, and the sentence is all that is left of the attempt.
#[derive(Debug)]
pub enum InstanceError {
    /// The OS would not say whether a Hub is running.
    Unavailable(String),
    /// A Hub is running, and it did not take the request.
    NotDelivered(String),
}

impl InstanceError {
    /// The sentence to show the user.
    pub fn message(&self) -> &str {
        match self {
            InstanceError::Unavailable(message) | InstanceError::NotDelivered(message) => message,
        }
    }
}

impl std::fmt::Display for InstanceError {
    /// The diagnostic form, in the shape this repo's other errors use: an
    /// English sentence naming the operation, wrapping the message the caller
    /// supplied ([`Self::message`] is the message alone, which is what the user
    /// is shown — the two are separate on purpose, as elsewhere in the crate).
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstanceError::Unavailable(message) => {
                write!(
                    formatter,
                    "the Hub could not claim this session's slot: {message}"
                )
            }
            InstanceError::NotDelivered(message) => {
                write!(formatter, "the launch was not delivered: {message}")
            }
        }
    }
}

impl std::error::Error for InstanceError {}

/// The exit code of a later launch whose request the Hub took.
///
/// Zero is deliberate: from the shell's point of view the launch succeeded —
/// the Hub is running and has the request. Nothing about the process that ended
/// is a failure.
pub const EXIT_HANDED_OFF: i32 = 0;

/// The exit code of a launch that could not be delivered.
///
/// Distinct from [`EXIT_HANDED_OFF`] so that a script — or the acceptance run
/// for this ticket — can tell "the Hub took it" from "nobody did" without
/// reading a message box.
pub const EXIT_NOT_DELIVERED: i32 = 1;
