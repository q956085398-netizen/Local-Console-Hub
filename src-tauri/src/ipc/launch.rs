//! The window's half of a launch request that made a terminal (#63).
//!
//! A shortcut that starts a Hub with no Hub running is answered from the app's
//! `setup`, which runs before the page has loaded. The terminal is made either
//! way — that is the point of making it in the backend ([`crate::app::launch`])
//! — but the *selection* is a window's to do, and an event published to a page
//! with no listener is gone. So the ask is kept, and this command is the window
//! reading it once as it attaches.
//!
//! Read **after** subscribing, by the caller: a request that arrives between
//! the two is then delivered by the event rather than missed by the read, and
//! one that arrived before it is found here.

use std::sync::Arc;

use tauri::State;

use crate::app::launch::Hub;

/// The session a launch request created and asked the window to show.
///
/// `None` — not an error — is the ordinary answer: most starts are a plain
/// open, and a window that has just read its selection has nothing to do with
/// one. Draining is deliberate: the ask belongs to the moment it was made, and
/// leaving it here would move the selection the next time the page loaded.
#[tauri::command]
pub fn take_launch_focus(hub: State<'_, Arc<Hub>>) -> Option<String> {
    hub.take_pending_focus()
}
