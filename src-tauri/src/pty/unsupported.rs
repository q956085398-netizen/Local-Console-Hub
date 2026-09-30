//! Non-Windows PTY backend — deliberately not implemented.
//!
//! v0.1.0 hosts terminals on Windows ConPTY only (`docs/DECISIONS.md` D-001,
//! D-014). Rather than shipping a second, untested pty implementation for a
//! platform the product does not target, every operation reports the
//! limitation explicitly, so a non-Windows caller fails loudly instead of
//! quietly getting a weaker terminal than this layer promises.
//!
//! Every item below is part of the small interface `mod.rs` drives, so none of
//! them is dead code on this platform either — the module is unreachable at
//! runtime, not unused at compile time.

use std::io;

/// Capability gate: there is no PTY backend on this platform.
pub fn require_backend(operation: &'static str) -> Result<(), super::PtyError> {
    Err(super::PtyError::UnsupportedPlatform { operation })
}

/// Never produced: [`require_backend`] fails before a terminal can start.
#[derive(Debug)]
pub struct PtyBackend;

impl PtyBackend {
    /// Never reached: [`require_backend`] fails before a terminal can start.
    pub fn spawn(_spec: &super::PtySpec) -> Result<(PtyBackend, OutputReader), String> {
        Err(unsupported("starting the ConPTY backend"))
    }

    /// Never reached: no terminal exists to ask.
    pub fn pid(&self) -> u32 {
        0
    }

    /// Never reached: no terminal exists to observe.
    pub fn child(&self) -> ChildHandle {
        ChildHandle
    }

    /// Never reached: no terminal exists to write to.
    pub fn write_input(&self, _bytes: &[u8]) -> io::Result<()> {
        Err(io::Error::other(unsupported("writing terminal input")))
    }

    /// Never reached: no terminal exists to resize.
    pub fn resize(&self, _cols: u16, _rows: u16) -> Result<(), String> {
        Err(unsupported("resizing the terminal"))
    }

    /// Never reached: no terminal exists to terminate.
    pub fn terminate(&self) -> Result<(), String> {
        Err(unsupported("terminating the terminal"))
    }

    /// Never reached: no terminal exists to enumerate.
    pub fn tree_pids(&self) -> Result<Vec<u32>, String> {
        Err(unsupported("enumerating the terminal's process tree"))
    }
}

/// Never produced: [`require_backend`] fails before a terminal can start.
#[derive(Debug)]
pub struct OutputReader;

impl OutputReader {
    /// Never reached: no terminal exists to read from.
    pub fn read_chunk(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other(unsupported("reading terminal output")))
    }
}

/// Never produced: [`require_backend`] fails before a terminal can start.
#[derive(Debug, Clone)]
pub struct ChildHandle;

impl ChildHandle {
    /// Never reached: no terminal exists to wait for.
    pub fn wait_signaled(&self) {}

    /// Never reached: no terminal exists to observe.
    pub fn exit_code(&self) -> Option<u32> {
        None
    }
}

fn unsupported(operation: &str) -> String {
    format!("{operation} needs the Windows ConPTY backend (D-001)")
}
