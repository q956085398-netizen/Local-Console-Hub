//! Process layer — process supervision, graceful stop and safe kill-tree.
//!
//! Owned by T03 (#4, process supervisor — SAFETY BLOCKER). Must know exactly
//! which process belongs to a managed session, distinguish graceful stop
//! from force kill (`DECISIONS.md` D-007), and never kill unrelated processes
//! based only on executable name. Default stop path: request graceful stop →
//! wait configured timeout → confirm exit → explicit force-kill of the
//! managed tree only.
