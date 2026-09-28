//! Delivery of [`SessionEvent`]s to the frontend through Tauri.
//!
//! Session Core is deliberately ignorant of Tauri — it publishes to an
//! [`EventSink`] — so this is the one adapter that knows about both. Keeping
//! the coupling to a single file is what lets the whole lifecycle run headless
//! in tests while the real app still gets typed events on the wire.
//!
//! Nothing here decides *whether* to publish or *what* the payload is: it
//! takes the name and payload an event already carries and hands them to
//! Tauri unchanged (`MVP_IMPLEMENTATION_SPEC.md` §9).

use tauri::{AppHandle, Emitter, Runtime};

use super::core::EventSink;
use super::event::SessionEvent;

/// Forwards session events to a Tauri app handle's event system.
///
/// This is the only piece of T04 that cannot be unit-tested here: emitting
/// needs a running app. It is compile-checked and thin by design — everything
/// that could be wrong about the payload or the naming lives in
/// [`SessionEvent`], which is tested.
pub struct TauriSink<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriSink<R> {
    /// A sink publishing through `app`.
    pub fn new(app: AppHandle<R>) -> Self {
        TauriSink { app }
    }
}

impl<R: Runtime> EventSink for TauriSink<R> {
    fn publish(&self, event: SessionEvent) {
        // A failed emit means the window is gone or the payload would not
        // serialize. Neither is something a lifecycle operation can act on,
        // and neither may take the session down with it: the state change has
        // already happened and the next listener to attach reads it from the
        // snapshot.
        let _ = self.app.emit(event.name(), event.payload());
    }
}
