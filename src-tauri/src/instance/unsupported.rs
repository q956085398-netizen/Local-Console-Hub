//! Non-Windows half of the instance layer (#60) — deliberately not implemented.
//!
//! The Hub is Windows-first (`docs/DECISIONS.md` D-001), and single instance is
//! the one part of it that is *entirely* a property of the platform rather than
//! of the product: another OS has a different way of naming a running instance,
//! and a second implementation guessed at from here would be untested code
//! standing between a user and their session supervisor.
//!
//! So this build makes exactly one claim, and it is the true one: with no
//! identity to look up, every launch is the Hub. Starting a second one is then
//! the user's business rather than a silent hand-off to a process that is not
//! there — the failure mode the other direction would have, where an entry that
//! always "hands off" would open nothing at all.
//!
//! Every item below is part of the interface `mod.rs` drives, so none of it is
//! dead code on this platform either: the module is unreachable at runtime, not
//! unused at compile time.

use std::sync::Arc;
use std::time::Duration;

use super::protocol::{Request, Response};
use super::{InstanceError, RequestHandler};

/// The Hub's half of the instance layer, with nothing to hold.
///
/// There is no mutex here to keep alive, so this type exists to carry the same
/// shape [`Primary::serve`](super::Primary::serve) has on Windows.
#[derive(Debug)]
pub struct Primary;

impl Primary {
    /// Nothing to listen on, and nothing that could connect.
    ///
    /// A request is never delivered to this process because
    /// [`claim`] never reports a Hub for it to hand off *to*, which is the
    /// property that makes "always primary" safe here: no path leads to a
    /// hand-off that cannot be served.
    pub fn serve(self, _handler: Arc<dyn RequestHandler>) {}
}

/// Every launch is the Hub on this platform; see the module comment.
pub fn claim(_mutex_name: &str, _pipe_name: &str) -> Result<Option<Primary>, InstanceError> {
    Ok(Some(Primary))
}

/// Never reached: [`claim`] never reports a Hub to hand a request to.
pub fn deliver(
    _pipe_name: &str,
    _request: Request,
    _timeout: Duration,
) -> Result<Response, InstanceError> {
    Err(InstanceError::NotDelivered(
        "本平台没有单实例入口，请求不会被转交给其它 Hub。".to_owned(),
    ))
}
