//! App layer — application-level state and wiring shared by IPC, tray and
//! Session Core.
//!
//! Owned by T04 (#5, Session Core). Do not place session lifecycle logic
//! here; this module only hosts cross-cutting state (e.g. the Session Core
//! registry) and application bootstrap.
