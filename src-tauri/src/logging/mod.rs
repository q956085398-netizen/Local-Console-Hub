//! Logging layer — terminal buffer, captured logs, external logs.
//!
//! Owned by T05 (#6, selective logging core). Keeps the three concepts from
//! `docs/LOGGING.md` separate: bounded in-memory terminal buffer, optional
//! hub-captured persistent log, and external application-owned logs.
//! Defaults: interactive terminals persist nothing; stdin is never
//! persisted; on_error retains a bounded pre-failure buffer. Logging code
//! does not decide session lifecycle.
