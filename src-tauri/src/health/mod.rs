//! Health layer — port and HTTP readiness checks.
//!
//! Owned by T08 (#9, service actions and health hooks): process-alive
//! signal, low-frequency TCP port check, and the health abstraction hook.
//! Health must stay separate from lifecycle truth (`DECISIONS.md` D-008:
//! state cannot be reduced to "PID exists") and checks must be cancellable
//! with session lifecycle.
