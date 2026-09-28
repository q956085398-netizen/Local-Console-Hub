//! Interactive terminal runs — the PTY behind a `type: terminal` session
//! (T07 #8, `docs/MVP_IMPLEMENTATION_SPEC.md` §6).
//!
//! The PTY layer ([`crate::pty`]) hosts the shell and carries the bytes; this
//! module is about what the *session* does with them. Two things live here:
//! the handle that owns a terminal's reader thread, and the relay that decides
//! how a session's output becomes the batches the UI is sent.
//!
//! ## Why output is batched
//!
//! A terminal shell emits as many chunks as the console host hands it — a
//! compiling build or a `Get-ChildItem` over a large tree produces thousands
//! of small reads a second. One IPC event per read is what makes a UI freeze
//! on high-volume output (spec §14, "one high-output session must not freeze
//! the whole UI"), so the relay coalesces a run's output into batches bounded
//! by both size and time: at most [`BATCH_MAX_BYTES`] per batch, and nothing
//! waits longer than [`BATCH_WINDOW`] once a byte is pending.
//!
//! ## Byte offsets, and why the relay keeps them
//!
//! Every batch names the half-open byte range of the run's stream it covers
//! (`[start, end)`), and the relay counts the bytes it has taken for the
//! current generation in [`OutputRelay::emitted`]. A terminal view that
//! attaches to a running session replays the retained scrollback and then
//! appends live batches; without the offsets it could not tell a batch it has
//! already shown from one it has not, and a reattach would either duplicate
//! output or silently skip it. `emitted` is advanced as bytes are *taken* —
//! not as they are published — so an attachment read under the same lock as
//! the buffer always lands exactly on a batch boundary (see
//! `SessionCore::terminal_attachment`).
//!
//! ## What the offsets do not promise
//!
//! `emitted` counts bytes taken; the scrollback is bounded
//! (`docs/LOGGING.md` §8), so a replay is a *suffix* of the stream, and a
//! session that discarded output says so through its
//! [`BufferSummary`](crate::logging::BufferSummary). Nothing here tries to
//! reconstruct what was discarded.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::logging::{BufferSummary, BufferedChunk, Stream};

/// How often a terminal's reader thread wakes when nothing is arriving.
///
/// It is the terminal's worst-case latency for output that arrives and then
/// falls silent: a shell that echoes one character leaves that character
/// pending until the next wake-up. 16 ms keeps an interactive keystroke under
/// the threshold a person notices while leaving the thread asleep ~98% of the
/// time (the budget D-009 sets for background work).
pub const PUMP_TICK: Duration = Duration::from_millis(16);

/// How long output may wait for company before it is published.
///
/// This exists for the middle ground between "one byte" and "a flood": output
/// arriving steadily but slowly would otherwise either be published per chunk
/// (the cost batching exists to avoid) or wait for the size ceiling (a
/// latency no interactive terminal may have).
pub const BATCH_WINDOW: Duration = Duration::from_millis(16);

/// The most bytes one published batch carries.
///
/// Large enough that a burst costs a handful of events instead of hundreds,
/// small enough that one event cannot stall the UI on deserialization.
pub const BATCH_MAX_BYTES: usize = 64 * 1024;

/// One batch of a run's output, and the range of that run's stream it covers.
///
/// The range is what makes a batch self-describing across a reattach: a view
/// that has already replayed the stream up to `x` appends the batch as a whole
/// when `end > x`, and drops it when `end <= x` (`start` is never straddled by
/// a boundary — see the module note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputBatch {
    /// The run this output belongs to. A view only renders batches for the
    /// generation it attached to; a restart bumps the generation, and the
    /// previous run's last bytes must not appear in the new one.
    pub generation: u64,
    /// First byte of the run's stream this batch covers.
    pub start: u64,
    /// One past the last byte this batch covers.
    pub end: u64,
    /// The bytes, exactly as the shell produced them: UTF-8 with the ANSI/VT
    /// sequences a terminal renderer consumes.
    pub bytes: Vec<u8>,
}

/// Coalesces one session's output into the batches the UI is sent.
///
/// Not shared: it lives inside the session's state and is driven under the
/// session lock, so a batch and the scrollback bytes it covers are always
/// taken together.
#[derive(Debug)]
pub struct OutputRelay {
    generation: u64,
    /// Bytes taken for `generation` but not yet published.
    pending: Vec<u8>,
    /// The offset `pending` starts at, i.e. the end of the last batch
    /// published for this generation.
    pending_start: u64,
    /// Bytes taken for `generation`, published or not. This is the offset a
    /// view is told it has been given.
    emitted: u64,
    /// When the oldest pending byte arrived; `None` while nothing is pending.
    opened_at: Option<Instant>,
}

impl Default for OutputRelay {
    fn default() -> Self {
        OutputRelay::new()
    }
}

impl OutputRelay {
    pub fn new() -> Self {
        OutputRelay {
            generation: 0,
            pending: Vec::new(),
            pending_start: 0,
            emitted: 0,
            opened_at: None,
        }
    }

    /// The generation whose output is being relayed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Bytes taken for this generation, whether or not they have been
    /// published yet.
    pub fn emitted(&self) -> u64 {
        self.emitted
    }

    /// Bytes taken but not yet published.
    ///
    /// For the tests that pin the relay's timing: production code asks what is
    /// *due* (`take`/`drain`) and never what is merely pending.
    #[cfg(test)]
    fn pending_bytes(&self) -> usize {
        self.pending.len()
    }

    /// Start relaying a new run, dropping anything the previous one left.
    ///
    /// Dropped, not published: the bytes belong to a run the session has
    /// already left, and a view that attached to the new generation would have
    /// to discard them anyway.
    pub fn begin(&mut self, generation: u64) {
        self.generation = generation;
        self.pending.clear();
        self.pending_start = 0;
        self.emitted = 0;
        self.opened_at = None;
    }

    /// Take a batch of output, publishing it only if it is due.
    ///
    /// Due means the pending window has reached [`BATCH_MAX_BYTES`], or
    /// [`BATCH_WINDOW`] has elapsed since the oldest pending byte arrived. The
    /// caller publishes what this returns and keeps calling it until it stops
    /// returning batches.
    pub fn push(&mut self, generation: u64, bytes: &[u8], now: Instant) -> Option<OutputBatch> {
        if generation != self.generation {
            self.begin(generation);
        }
        if bytes.is_empty() {
            return None;
        }

        if self.pending.is_empty() {
            self.pending_start = self.emitted;
            self.opened_at = Some(now);
        }
        self.pending.extend_from_slice(bytes);
        // Advanced here rather than at publication: this is the moment the
        // bytes join the session's scrollback, and an attachment must never
        // see a buffer that is ahead of the offset it is told about.
        self.emitted += bytes.len() as u64;

        let due = self.pending.len() >= BATCH_MAX_BYTES || self.window_elapsed(now);
        due.then(|| self.cut())
    }

    /// Publish what is pending when it is due — or unconditionally, for a
    /// caller that knows the stream has ended or the view is going away.
    ///
    /// Returns `None` when there is nothing to publish, or when nothing is due
    /// yet; a caller that wants everything should loop until this stops
    /// returning batches.
    pub fn take(&mut self, now: Instant, force: bool) -> Option<OutputBatch> {
        if self.pending.is_empty() {
            return None;
        }
        if !force && !self.window_elapsed(now) {
            return None;
        }
        Some(self.cut())
    }

    fn window_elapsed(&self, now: Instant) -> bool {
        self.opened_at
            .is_some_and(|opened| now.saturating_duration_since(opened) >= BATCH_WINDOW)
    }

    /// Take every batch that is due, oldest first.
    ///
    /// The loop a caller otherwise writes itself, in the one place that can be
    /// sure it terminates: each round takes at least one byte off the front, so
    /// a caller cannot end up spinning on a batch that is not due.
    pub fn drain(&mut self, now: Instant, force: bool) -> Vec<OutputBatch> {
        let mut batches = Vec::new();
        while let Some(batch) = self.take(now, force) {
            batches.push(batch);
        }
        batches
    }

    /// Split the front of the pending bytes off as a batch.
    fn cut(&mut self) -> OutputBatch {
        let len = self.pending.len().min(BATCH_MAX_BYTES);
        let bytes: Vec<u8> = self.pending.drain(..len).collect();
        let batch = OutputBatch {
            generation: self.generation,
            start: self.pending_start,
            end: self.pending_start + len as u64,
            bytes,
        };
        self.pending_start += len as u64;
        if self.pending.is_empty() {
            self.opened_at = None;
        }
        batch
    }
}

/// What a terminal view is given when it attaches to a session.
///
/// Read together, under one lock, so the three numbers describe one moment:
/// `chunks` is the scrollback as it stands, `emitted` is how far into the run's
/// stream that scrollback reaches, and `generation` says which run both belong
/// to. A view replays `chunks` and then keeps the batches whose `end` is past
/// `emitted` — see the module note for why that cannot duplicate or skip a byte.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAttachment {
    pub session_id: String,
    /// Whether a live terminal is attached right now. A view uses this to
    /// decide whether typing can go anywhere, and whether the session is
    /// showing a stream or the remains of one.
    pub pty_attached: bool,
    /// The run the offset and the scrollback belong to.
    pub generation: u64,
    /// How much of that run's stream the scrollback reaches, in bytes.
    pub emitted: u64,
    /// The retained scrollback, oldest first.
    pub chunks: Vec<RetainedChunk>,
    /// What the scrollback holds, including how much of it was discarded to
    /// stay inside the limits — a view that is showing a truncated history
    /// must be able to say so.
    pub buffer: BufferSummary,
}

/// One retained chunk of scrollback, as the frontend receives it.
///
/// The bytes, base64-encoded, for the reason [`TerminalOutput`] carries them
/// that way: a replay cut at a chunk boundary has to be able to end inside a
/// multi-byte character without corrupting it.
///
/// [`TerminalOutput`]: crate::session::event::TerminalOutput
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetainedChunk {
    pub stream: Stream,
    /// The chunk's bytes, base64-encoded (standard alphabet, padded).
    pub data: String,
}

impl RetainedChunk {
    /// Encode one retained chunk for the wire.
    pub fn encode(chunk: &BufferedChunk) -> Self {
        use base64::Engine;

        RetainedChunk {
            stream: chunk.stream,
            data: base64::engine::general_purpose::STANDARD.encode(chunk.bytes()),
        }
    }
}

/// The thread reading one terminal's output, and the flag that stops it.
///
/// The PTY outlives its shell (the console host is what the Hub closes), so a
/// reader cannot rely on the output stream ending when the run does: it stops
/// when this flag is set, which happens once the run has been finalized and
/// the last of the queued output has been taken.
pub struct TerminalPump {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl TerminalPump {
    /// Wrap a reader thread started for one terminal.
    pub fn new(stop: Arc<AtomicBool>, handle: JoinHandle<()>) -> Self {
        TerminalPump {
            stop,
            handle: Some(handle),
        }
    }

    /// The flag a reader thread watches.
    pub fn stop_flag() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    /// Ask the reader to finish, and wait for the bytes it is still taking.
    ///
    /// Bounded by the reader's own wake-up: it checks this flag once per
    /// [`PUMP_TICK`] and flushes before it returns, so the wait cannot outlast
    /// a tick plus the flush. Must be called with no session lock held — the
    /// reader needs that lock to hand over its last bytes.
    pub fn stop_and_wait(mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(millis: u64) -> Instant {
        // A fixed epoch: the relay only ever compares instants, so the tests
        // never depend on the wall clock.
        Instant::now() + Duration::from_millis(millis)
    }

    #[test]
    fn output_within_the_window_is_published_as_one_batch() {
        let mut relay = OutputRelay::new();
        let base = at(0);

        assert_eq!(relay.push(1, b"first ", base), None, "not due yet");
        assert_eq!(
            relay.push(1, b"second", base + Duration::from_millis(4)),
            None,
            "still inside the window"
        );

        let batch = relay
            .take(base + BATCH_WINDOW, false)
            .expect("the window has elapsed");
        assert_eq!(batch.bytes, b"first second");
        assert_eq!((batch.start, batch.end), (0, 12));
        assert_eq!(batch.generation, 1);
    }

    #[test]
    fn a_chunk_arriving_after_the_window_is_published_with_the_pending_bytes() {
        let mut relay = OutputRelay::new();
        let base = at(0);

        assert_eq!(relay.push(1, b"before", base), None);
        let batch = relay
            .push(1, b"after", base + BATCH_WINDOW)
            .expect("the window elapsed with bytes pending");
        assert_eq!(batch.bytes, b"beforeafter");
    }

    #[test]
    fn output_is_cut_at_the_batch_ceiling_and_the_rest_waits() {
        let mut relay = OutputRelay::new();
        let base = at(0);
        let flood = vec![b'x'; BATCH_MAX_BYTES + 10];

        let batch = relay
            .push(1, &flood, base)
            .expect("the ceiling makes the batch due immediately");
        assert_eq!(batch.bytes.len(), BATCH_MAX_BYTES);
        assert_eq!((batch.start, batch.end), (0, BATCH_MAX_BYTES as u64));
        assert_eq!(relay.pending_bytes(), 10);

        let rest = relay.take(base, true).expect("the tail is still pending");
        assert_eq!(rest.bytes.len(), 10);
        assert_eq!(
            (rest.start, rest.end),
            (BATCH_MAX_BYTES as u64, BATCH_MAX_BYTES as u64 + 10),
            "the tail continues the stream rather than restarting it"
        );
    }

    #[test]
    fn nothing_is_published_before_the_window_and_nothing_is_forced_early() {
        let mut relay = OutputRelay::new();
        let base = at(0);

        assert_eq!(relay.push(1, b"x", base), None);
        assert_eq!(relay.take(base, false), None, "not due");
        assert!(
            relay.take(base + BATCH_WINDOW, false).is_some(),
            "the window has elapsed"
        );
        assert_eq!(
            relay.take(base + BATCH_WINDOW, false),
            None,
            "the queue is empty"
        );
    }

    #[test]
    fn an_empty_push_publishes_nothing_and_opens_no_window() {
        let mut relay = OutputRelay::new();
        let base = at(0);

        assert_eq!(relay.push(1, b"", base), None);
        assert_eq!(relay.pending_bytes(), 0);
        // Nothing pending means nothing to take, even long after the window.
        assert_eq!(relay.take(base + BATCH_WINDOW * 10, false), None);
    }

    /// The invariant an attachment depends on: the offset a view is told about
    /// covers everything that has joined the scrollback, published or not.
    #[test]
    fn the_emitted_offset_covers_bytes_that_are_still_pending() {
        let mut relay = OutputRelay::new();
        let base = at(0);

        relay.push(1, b"abc", base);
        relay.push(1, b"defg", base);
        assert_eq!(
            relay.emitted(),
            7,
            "an attachment must not be told less than the buffer holds"
        );
        assert_eq!(relay.pending_bytes(), 7);

        let batch = relay.take(base + BATCH_WINDOW, false).expect("due");
        assert_eq!(batch.end, 7);
        assert_eq!(relay.emitted(), 7, "publishing moves no offset");
    }

    #[test]
    fn a_new_generation_starts_the_offsets_over_and_drops_the_old_window() {
        let mut relay = OutputRelay::new();
        let base = at(0);

        relay.push(1, b"old output", base);
        assert_eq!(relay.emitted(), 10);

        assert_eq!(
            relay.push(2, b"new", base),
            None,
            "the new run's bytes are not due"
        );
        assert_eq!(relay.generation(), 2);
        assert_eq!(relay.emitted(), 3);
        assert_eq!(relay.pending_bytes(), 3, "the superseded bytes are gone");

        let batch = relay.take(base + BATCH_WINDOW, false).expect("due");
        assert_eq!(batch.bytes, b"new");
        assert_eq!((batch.start, batch.end), (0, 3));
    }

    #[test]
    fn stopping_a_pump_is_observed_by_its_reader() {
        let stop = TerminalPump::stop_flag();
        let reader = Arc::clone(&stop);
        let pump = TerminalPump::new(
            Arc::clone(&stop),
            std::thread::spawn(move || {
                while !reader.load(Ordering::Acquire) {
                    std::thread::sleep(PUMP_TICK / 4);
                }
            }),
        );

        assert!(!stop.load(Ordering::Acquire));
        pump.stop_and_wait();
        assert!(stop.load(Ordering::Acquire));
    }
}
