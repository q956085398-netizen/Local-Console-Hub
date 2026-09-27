//! PTY layer — PTY abstraction and the Windows backend.
//!
//! Owned by T02 (#3, Windows PTY/ConPTY validation — RELEASE BLOCKER).
//! The backend abstraction must support spawn with working directory,
//! bidirectional byte stream, resize, Unicode, ANSI, Ctrl+C semantics, and
//! sessions staying alive while hidden. xterm.js renders the terminal; it
//! does not own the process or PTY. PTY code does not decide logging policy.
