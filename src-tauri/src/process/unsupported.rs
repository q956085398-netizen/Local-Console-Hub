//! Non-Windows supervisor backend — deliberately not implemented.
//!
//! v0.1.0 supervises processes on Windows only (`docs/DECISIONS.md` D-001).
//! Rather than inventing a second, untested process-tree strategy for a platform
//! the product does not target, every operation reports the limitation
//! explicitly, so a non-Windows caller fails loudly instead of quietly getting
//! weaker safety guarantees than this layer promises.
//!
//! Every item below is part of the small interface `mod.rs` drives, so none of
//! them is dead code on this platform either — the module is unreachable at
//! runtime, not unused at compile time.

use std::process::{Child, Command};

/// Capability gate: there is no process-tree ownership on this platform.
pub fn require_backend(operation: &'static str) -> Result<(), super::ProcessError> {
    Err(super::ProcessError::UnsupportedPlatform { operation })
}

/// Never produced: [`require_backend`] fails before a run can be started.
#[derive(Debug)]
pub struct TreeHandle;

/// Never reached: [`require_backend`] fails before a run can be started.
pub fn prepare(_command: &mut Command) {}

/// Never reached: [`require_backend`] fails before a run can be started.
pub fn prepare_windowed(_command: &mut Command) {}

/// Never reached: [`require_backend`] fails before a run can be started.
pub fn attach(_child: &Child) -> Result<TreeHandle, String> {
    Err(unsupported("attaching a run to its job object"))
}

/// Never reached: [`require_backend`] fails before a run can be started.
pub fn attach_independent(_child: &Child) -> Result<TreeHandle, String> {
    Err(unsupported("attaching a run to its job object"))
}

/// There is no process to identify: [`require_backend`] fails before a run can
/// be started, so no identity is ever taken.
pub fn creation_time(_child: &Child) -> Option<u64> {
    None
}

/// No process answers for a pid on a platform with no process layer (D-001).
pub fn creation_time_of(_pid: u32) -> Option<u64> {
    None
}

/// Never reached: [`require_backend`] fails before a run can be started.
pub fn terminate_tree(_tree: &TreeHandle) -> Result<(), String> {
    Err(unsupported("terminating a managed process tree"))
}

/// Never reached: [`require_backend`] fails before a run can be started.
pub fn tree_pids(_tree: &TreeHandle) -> Result<Vec<u32>, String> {
    Err(unsupported("enumerating a managed process tree"))
}

/// Never reached: [`require_backend`] fails before a run can be started.
pub fn process_handle(_child: &Child) -> usize {
    0
}

/// Never reached: [`require_backend`] fails before a run can be started.
pub fn wait_for_handle(_handle: usize) -> Result<(), String> {
    Err(unsupported("waiting for a managed process to exit"))
}

/// Never reached: [`require_backend`] fails before a run can be started.
pub fn request_graceful_stop(_pid: u32) -> bool {
    false
}

fn unsupported(operation: &str) -> String {
    format!("{operation} needs the Windows supervisor backend (D-001)")
}
