//! Session layer — Session Core, lifecycle state machine and event model.
//!
//! Owned by T04 (#5, Session Core). Single source of lifecycle truth:
//! Stopped → Starting → Running → Stopping → Exited/Error with invalid
//! transitions rejected, run_id per start, restart waiting for confirmed
//! stop, unexpected exits updating state while the UI is hidden. Tray and
//! window both call the same Session Core APIs.
