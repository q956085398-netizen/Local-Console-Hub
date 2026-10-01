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
pub fn spawn(command: &mut Command, prepare: impl FnOnce(&mut Command)) -> std::io::Result<Child> {
    prepare(command);
    command.spawn()
}

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

/// No process table to read on a platform with no process layer (D-001).
///
/// `None` — "could not read it" — rather than an empty list, which would be the
/// claim that nothing is running. There is no way to check on this platform,
/// and a caller deciding whether to start a second copy has to be told that.
pub fn processes_named(_names: &[String]) -> Option<Vec<super::ProcessReading>> {
    None
}

/// No parent links to walk on a platform with no process layer (D-001).
pub fn descendants(_pid: u32) -> Vec<u32> {
    Vec::new()
}

/// There is no process object to open on a platform with no process layer.
pub fn open_external(_identity: super::ProcessIdentity) -> Result<usize, super::ProcessError> {
    Err(super::ProcessError::UnsupportedPlatform {
        operation: "watching an application the Hub did not start",
    })
}

/// Never reached: no handle is ever handed out by [`open_external`].
pub fn wait_for_handle_timeout(_handle: usize, _timeout: std::time::Duration) -> bool {
    true
}

/// Never reached: no handle is ever handed out by [`open_external`].
pub fn exit_code(_handle: usize) -> Option<u32> {
    None
}

/// Nothing to close: no handle is ever handed out by [`open_external`].
pub fn close_handle(_handle: usize) {}

fn unsupported(operation: &str) -> String {
    format!("{operation} needs the Windows supervisor backend (D-001)")
}
