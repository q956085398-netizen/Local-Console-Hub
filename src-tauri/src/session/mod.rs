//! Session layer — Session Core, lifecycle state machine and event model.
//!
//! Owned by T04 (#5, Session Core). Single source of lifecycle truth:
//! Stopped → Starting → Running → Stopping → Exited/Error with invalid
//! transitions rejected, run_id per start, restart waiting for confirmed
//! stop, unexpected exits updating state while the UI is hidden. Tray and
//! window both call the same Session Core APIs.
//!
//! ## Where to start reading
//!
//! - [`state`] — the allowed transitions, transcribed from spec §5.
//! - [`core`] — [`core::SessionCore`], the registry and the orchestration.
//! - [`event`] — the typed events and their IPC names.
//! - [`runtime`] — the snapshots and run records those carry.
//! - [`terminal`] — what a terminal run does with its PTY's output (T07).
//! - [`temporary`] — the shell, directory and identity a session created from
//!   the quick entry gets, because no config file answers them (#62).
//! - [`tauri_sink`] — the one adapter that knows about Tauri.
//!
//! Dependents (T05–T10) must call [`core::SessionCore`] rather than tracking
//! process state themselves (EXECUTION_PLAN §3).

pub mod core;
pub mod event;
pub mod runtime;
pub mod state;
pub mod tauri_sink;
pub mod temporary;
pub mod terminal;
