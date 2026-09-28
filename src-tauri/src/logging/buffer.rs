//! Bounded in-memory terminal buffer (`docs/LOGGING.md` §8).
//!
//! The buffer is what makes "I can still scroll back" and "no log file was
//! written" both true at once: every session has one, and it is never the
//! thing that touches the disk. `docs/DECISIONS.md` D-004 keeps the two apart,
//! and this type is the memory half of that decision.
//!
//! It is a ring: output is appended in the order it arrived and the oldest
//! bytes are discarded once the limits are reached. Discarding is deliberate
//! and visible — [`TerminalBuffer::dropped_bytes`] counts what a UI must be
//! able to admit to the user rather than silently showing a scrollback with a
//! hole at the top.
//!
//! Nothing here decides *whether* output is persisted; that is
//! [`super::plan`] and [`super::run_log`]'s business, and the buffer knows
//! nothing about files.

use std::collections::VecDeque;

use serde::{Serialize, Serializer};

/// Which stream a chunk of output came from.
///
/// There is deliberately no `Stdin` variant. `docs/LOGGING.md` §4 and spec §15
/// forbid persisting user input, and a type that cannot express it is the
/// cheapest way to keep that a structural property instead of a rule every
/// call site has to remember.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    /// Literal written into log headers, so a file says which stream a part of
    /// it came from (`docs/LOGGING.md` §7).
    ///
    /// The wire form comes from `Serialize` above, not from here; a test holds
    /// the two together, because a rename in one that missed the other would
    /// silently split the vocabulary a log file and a UI describe the same
    /// stream with.
    pub fn as_str(&self) -> &'static str {
        match self {
            Stream::Stdout => "stdout",
            Stream::Stderr => "stderr",
        }
    }
}

/// How much scrollback a buffer keeps.
///
/// Both bounds are enforced: whichever is reached first starts discarding the
/// oldest output. `docs/LOGGING.md` §8 asks for "maximum lines *or* maximum
/// bytes"; keeping both means a burst of one giant line cannot blow the byte
/// budget and a flood of tiny lines cannot blow the line budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferLimits {
    pub max_bytes: usize,
    pub max_lines: usize,
}

impl BufferLimits {
    pub const fn new(max_bytes: usize, max_lines: usize) -> Self {
        BufferLimits {
            max_bytes,
            max_lines,
        }
    }
}

/// Scrollback kept for a session's terminal view.
///
/// Sized for "a person can still read what just happened" rather than for
/// completeness — anything a user must not lose belongs in a log file, which
/// is a separate decision they make (`docs/LOGGING.md` §1.1).
pub const DEFAULT_BUFFER_LIMITS: BufferLimits = BufferLimits::new(256 * 1024, 5_000);

/// Context retained for `on_error` until the run proves to have failed
/// (`docs/LOGGING.md` §3, §8).
///
/// Smaller than the scrollback on purpose: this one is paid for by every run
/// of every `on_error` session, and its job is to cover the seconds before a
/// failure, not to hold a session's history.
pub const DEFAULT_PRE_FAILURE_LIMITS: BufferLimits = BufferLimits::new(64 * 1024, 2_000);

/// One run of output from a single stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferedChunk {
    pub stream: Stream,
    bytes: Vec<u8>,
}

impl BufferedChunk {
    /// The raw bytes as they arrived, ANSI escapes included.
    ///
    /// This is what reaches a log file and what a terminal must render: the
    /// escape sequences are the difference between a coloured build log and a
    /// wall of `ESC[0m` (`docs/LOGGING.md` §7).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The chunk as displayable text, with invalid UTF-8 replaced.
    ///
    /// Lossy on purpose at this boundary: the buffer holds bytes because they
    /// are written out verbatim, and a UI that needs a string gets the best
    /// rendering of them rather than an error it cannot act on. A chunk that
    /// ends mid-character shows one replacement character at its end; the
    /// bytes themselves are untouched for anything that reads them raw.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    pub fn len_bytes(&self) -> usize {
        self.bytes.len()
    }

    fn newlines(&self) -> usize {
        self.bytes.iter().filter(|byte| **byte == b'\n').count()
    }
}

/// The frontend sees text rather than a byte array: the wire payload is a
/// terminal view, and escaping every chunk into a JSON number list would cost
/// more than the lossy conversion it avoids.
impl Serialize for BufferedChunk {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("BufferedChunk", 3)?;
        state.serialize_field("stream", &self.stream)?;
        state.serialize_field("text", &self.text())?;
        state.serialize_field("bytes", &self.bytes.len())?;
        state.end()
    }
}

/// What a session's scrollback currently holds, without the scrollback.
///
/// A snapshot carries this rather than the chunks: the terminal view is a
/// separate, deliberately non-eager read (`docs/MVP_IMPLEMENTATION_SPEC.md`
/// §14 — no high-frequency re-rendering of invisible terminals), while "is
/// there output, and did we lose any?" is part of the state a UI renders from
/// the snapshot it already has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferSummary {
    pub bytes: usize,
    pub lines: usize,
    /// Output discarded to stay inside the limits, since the buffer was
    /// created. Non-zero is the difference between "nothing happened yet" and
    /// "you are looking at a truncated history".
    pub dropped_bytes: u64,
}

impl From<&TerminalBuffer> for BufferSummary {
    fn from(buffer: &TerminalBuffer) -> Self {
        BufferSummary {
            bytes: buffer.bytes(),
            lines: buffer.lines(),
            dropped_bytes: buffer.dropped_bytes(),
        }
    }
}

/// Bounded, in-memory, append-only scrollback.
#[derive(Debug, Clone)]
pub struct TerminalBuffer {
    limits: BufferLimits,
    chunks: VecDeque<BufferedChunk>,
    bytes: usize,
    lines: usize,
    dropped_bytes: u64,
}

impl TerminalBuffer {
    pub fn new(limits: BufferLimits) -> Self {
        TerminalBuffer {
            limits,
            chunks: VecDeque::new(),
            bytes: 0,
            lines: 0,
            dropped_bytes: 0,
        }
    }

    pub fn limits(&self) -> BufferLimits {
        self.limits
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    /// Retained newlines: one less than the number of lines a reader would
    /// count in some cases (a chunk that does not end with a newline still
    /// occupies a line). Close enough for a bound, and exactly reproducible.
    pub fn lines(&self) -> usize {
        self.lines
    }

    pub fn dropped_bytes(&self) -> u64 {
        self.dropped_bytes
    }

    pub fn summary(&self) -> BufferSummary {
        BufferSummary::from(self)
    }

    /// Every retained chunk, oldest first.
    pub fn chunks(&self) -> impl Iterator<Item = &BufferedChunk> {
        self.chunks.iter()
    }

    /// Append one batch of output, discarding the oldest output if that puts
    /// the buffer over its limits.
    pub fn push(&mut self, stream: Stream, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        // A single batch larger than the whole buffer keeps its own tail: the
        // limits are a property of the buffer, so no push can be allowed to
        // exceed them, however large the read that produced it was.
        let bytes = if bytes.len() > self.limits.max_bytes {
            let excess = bytes.len() - self.limits.max_bytes;
            self.dropped_bytes += excess as u64;
            &bytes[excess..]
        } else {
            bytes
        };

        let chunk = BufferedChunk {
            stream,
            bytes: bytes.to_vec(),
        };
        self.bytes += chunk.len_bytes();
        self.lines += chunk.newlines();
        self.chunks.push_back(chunk);

        self.discard_to_fit();
    }

    /// Empty the buffer, as when a session's scrollback is cleared by hand.
    ///
    /// Deliberately not a way to "close" a session: the buffer is memory, and
    /// dropping it cannot lose anything that was going to be persisted — a
    /// log file, once open, is the run log's, not the buffer's.
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.bytes = 0;
        self.lines = 0;
    }

    /// The newest `max_bytes` bytes, oldest chunk first.
    ///
    /// This is what `on_error` hands to the file when a run fails
    /// (`docs/LOGGING.md` §8: "the N lines before the error"), so it keeps
    /// chunk boundaries and stream tags rather than flattening to one blob —
    /// a stack trace the application wrote to stderr must still be
    /// distinguishable from the stdout around it.
    pub fn tail(&self, max_bytes: usize) -> Vec<BufferedChunk> {
        let mut kept: Vec<BufferedChunk> = Vec::new();
        let mut budget = max_bytes;

        for chunk in self.chunks.iter().rev() {
            if budget == 0 {
                break;
            }
            if chunk.len_bytes() <= budget {
                budget -= chunk.len_bytes();
                kept.push(chunk.clone());
            } else {
                let start = chunk.len_bytes() - budget;
                budget = 0;
                kept.push(BufferedChunk {
                    stream: chunk.stream,
                    bytes: chunk.bytes[start..].to_vec(),
                });
            }
        }

        kept.reverse();
        kept
    }

    fn discard_to_fit(&mut self) {
        while self.over_budget() && self.chunks.len() > 1 {
            let front = self.chunks.pop_front().expect("checked above");
            self.bytes -= front.len_bytes();
            self.lines -= front.newlines();
            self.dropped_bytes += front.len_bytes() as u64;
        }

        if self.over_budget() {
            let drop_bytes = self.bytes.saturating_sub(self.limits.max_bytes);
            let drop_lines = self.lines.saturating_sub(self.limits.max_lines);
            if let Some(front) = self.chunks.front_mut() {
                let (bytes, lines) = trim_front(front, drop_bytes, drop_lines);
                self.bytes -= bytes;
                self.lines -= lines;
                self.dropped_bytes += bytes as u64;
            }
        }
    }

    fn over_budget(&self) -> bool {
        self.bytes > self.limits.max_bytes || self.lines > self.limits.max_lines
    }
}

/// Trim one chunk from its front until it fits, answering with what was
/// removed.
///
/// Only ever reached with a single chunk left, where discarding whole chunks
/// cannot help: the trim has to fall inside it. Bytes and lines are both
/// resolved by taking the larger of the two offsets, so the result satisfies
/// both bounds even when only one of them was exceeded.
fn trim_front(chunk: &mut BufferedChunk, drop_bytes: usize, drop_lines: usize) -> (usize, usize) {
    let mut cut = drop_bytes;
    if drop_lines > 0 {
        let mut seen = 0;
        let mut cut_for_lines = chunk.bytes.len();
        for (index, byte) in chunk.bytes.iter().enumerate() {
            if *byte == b'\n' {
                seen += 1;
                if seen == drop_lines {
                    cut_for_lines = index + 1;
                    break;
                }
            }
        }
        cut = cut.max(cut_for_lines);
    }
    // Never discard the whole chunk: a buffer that kept nothing could not
    // report the output it just received, and the limits are already
    // enforced by `push` before this point.
    cut = cut.min(chunk.bytes.len().saturating_sub(1));

    if cut == 0 {
        return (0, 0);
    }
    let removed_bytes = chunk.bytes.len() - cut;
    let removed_lines = chunk
        .bytes
        .iter()
        .take(cut)
        .filter(|byte| **byte == b'\n')
        .count();
    chunk.bytes.drain(..cut);
    (removed_bytes, removed_lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk_count(buffer: &TerminalBuffer) -> usize {
        buffer.chunks().count()
    }

    /// `docs/LOGGING.md` §4 and spec §15: user input is not persisted, and it
    /// is not persisted because nothing in this layer can be handed it. The
    /// whole vocabulary is asserted here rather than the absence of a variant:
    /// adding `stdin` later must fail this test, because it would need a
    /// decision about §15 before it could be recorded.
    #[test]
    fn the_only_streams_are_the_two_the_run_writes_to() {
        let streams = [Stream::Stdout, Stream::Stderr];

        assert_eq!(streams.len(), 2, "a stream was added to this layer");
        assert_eq!(
            streams.map(|stream| stream.as_str()),
            ["stdout", "stderr"],
            "the stream vocabulary is what the log headers and the UI read"
        );
        // The wire form is a second place the same word is written. Both are
        // asserted against each other so a rename cannot reach only one.
        for stream in streams {
            assert_eq!(
                serde_json::to_value(stream).expect("a stream serializes"),
                serde_json::json!(stream.as_str()),
                "the wire form of {stream:?} diverged from its header form"
            );
        }
    }

    #[test]
    fn output_is_retained_in_order_and_tagged_by_stream() {
        let mut buffer = TerminalBuffer::new(DEFAULT_BUFFER_LIMITS);

        buffer.push(Stream::Stdout, b"starting\n");
        buffer.push(Stream::Stderr, b"warning\n");

        let chunks: Vec<(Stream, String)> = buffer
            .chunks()
            .map(|chunk| (chunk.stream, chunk.text()))
            .collect();
        assert_eq!(
            chunks,
            vec![
                (Stream::Stdout, "starting\n".to_owned()),
                (Stream::Stderr, "warning\n".to_owned()),
            ]
        );
        assert_eq!(buffer.bytes(), 17);
        assert_eq!(buffer.lines(), 2);
        assert_eq!(buffer.dropped_bytes(), 0);
    }

    /// §8: "ring buffer; discard the oldest content when it overflows". The
    /// byte bound is what a noisy service hits, and the newest output is what
    /// a person is looking for when it does.
    #[test]
    fn the_byte_bound_discards_the_oldest_output() {
        let mut buffer = TerminalBuffer::new(BufferLimits::new(10, usize::MAX));

        buffer.push(Stream::Stdout, b"aaaa");
        buffer.push(Stream::Stdout, b"bbbb");
        buffer.push(Stream::Stdout, b"cccc");

        assert!(buffer.bytes() <= 10, "bytes {} exceeded", buffer.bytes());
        let text: String = buffer.chunks().map(BufferedChunk::text).collect();
        assert!(text.ends_with("cccc"), "newest output was dropped: {text}");
        assert_eq!(buffer.dropped_bytes(), 4, "the dropped bytes are counted");
    }

    #[test]
    fn the_line_bound_discards_the_oldest_output() {
        let mut buffer = TerminalBuffer::new(BufferLimits::new(usize::MAX, 3));

        buffer.push(Stream::Stdout, b"one\ntwo\n");
        buffer.push(Stream::Stdout, b"three\nfour\n");

        assert!(buffer.lines() <= 3, "lines {} exceeded", buffer.lines());
        let text: String = buffer.chunks().map(BufferedChunk::text).collect();
        assert!(text.contains("four"), "newest line was dropped: {text}");
        assert!(!text.contains("one\n"), "oldest line survived: {text}");
    }

    /// The bound is a property of the buffer, not of the caller's read size:
    /// one 1 MB read into a 1 KB buffer cannot leave it holding 1 MB.
    #[test]
    fn a_single_oversized_push_keeps_only_its_tail() {
        let mut buffer = TerminalBuffer::new(BufferLimits::new(8, usize::MAX));

        buffer.push(Stream::Stdout, b"0123456789abcdef");

        assert_eq!(buffer.bytes(), 8);
        let text: String = buffer.chunks().map(BufferedChunk::text).collect();
        assert_eq!(text, "89abcdef");
        assert_eq!(buffer.dropped_bytes(), 8);
    }

    /// A single chunk carrying more lines than the whole line budget is
    /// trimmed *inside* the chunk — the case neither whole-chunk eviction nor
    /// the byte bound can resolve on its own.
    #[test]
    fn one_chunk_with_too_many_lines_is_trimmed_from_its_front() {
        let mut buffer = TerminalBuffer::new(BufferLimits::new(usize::MAX, 2));

        buffer.push(Stream::Stdout, b"one\ntwo\nthree\nfour\n");

        assert!(buffer.lines() <= 2, "lines {}", buffer.lines());
        let text: String = buffer.chunks().map(BufferedChunk::text).collect();
        assert!(
            text.ends_with("four\n"),
            "the newest line was dropped: {text}"
        );
        assert!(text.starts_with("three\n"), "too much was kept: {text}");
    }

    #[test]
    fn an_empty_push_changes_nothing() {
        let mut buffer = TerminalBuffer::new(DEFAULT_BUFFER_LIMITS);

        buffer.push(Stream::Stdout, b"");

        assert_eq!(chunk_count(&buffer), 0);
        assert_eq!(buffer.bytes(), 0);
    }

    #[test]
    fn the_tail_stops_at_the_requested_budget() {
        let mut buffer = TerminalBuffer::new(DEFAULT_BUFFER_LIMITS);
        buffer.push(Stream::Stdout, b"0123456789");
        buffer.push(Stream::Stderr, b"abcde");

        let tail = buffer.tail(7);

        let text: String = tail.iter().map(BufferedChunk::text).collect();
        assert_eq!(text, "89abcde", "tail kept the wrong bytes");
        assert_eq!(
            tail.iter().map(|chunk| chunk.stream).collect::<Vec<_>>(),
            vec![Stream::Stdout, Stream::Stderr],
            "the stream tags survive the cut"
        );
    }

    #[test]
    fn the_tail_of_an_empty_buffer_is_empty() {
        let buffer = TerminalBuffer::new(DEFAULT_BUFFER_LIMITS);
        assert!(buffer.tail(1024).is_empty());
    }

    /// The summary is what a snapshot carries, so it has to describe the same
    /// buffer the chunks come from.
    #[test]
    fn the_summary_describes_what_the_buffer_holds() {
        let mut buffer = TerminalBuffer::new(BufferLimits::new(4, usize::MAX));
        buffer.push(Stream::Stdout, b"aaa");
        buffer.push(Stream::Stdout, b"bbb");

        assert_eq!(
            buffer.summary(),
            BufferSummary {
                bytes: buffer.bytes(),
                lines: 0,
                dropped_bytes: buffer.dropped_bytes(),
            }
        );
        assert_eq!(buffer.summary().dropped_bytes, 3);
    }

    /// A UI reads the chunk as text; the wire shape is asserted rather than
    /// derived from whatever `Serialize` happens to produce.
    #[test]
    fn a_chunk_serializes_as_text_with_its_stream() {
        let mut buffer = TerminalBuffer::new(DEFAULT_BUFFER_LIMITS);
        buffer.push(Stream::Stderr, b"bad \xff news\n");

        let chunk = buffer.chunks().next().expect("one chunk");
        let value = serde_json::to_value(chunk).expect("a chunk serializes");

        assert_eq!(value["stream"], serde_json::json!("stderr"));
        assert_eq!(value["text"], serde_json::json!("bad \u{fffd} news\n"));
    }

    /// ANSI escapes are content, not noise: the log file and the terminal both
    /// need them, and only the search view strips them (`docs/LOGGING.md` §7).
    #[test]
    fn ansi_escapes_survive_the_round_trip() {
        let mut buffer = TerminalBuffer::new(DEFAULT_BUFFER_LIMITS);
        buffer.push(Stream::Stdout, b"\x1b[31mred\x1b[0m\n");

        let chunk = buffer.chunks().next().expect("one chunk");
        assert_eq!(chunk.bytes(), b"\x1b[31mred\x1b[0m\n");
        assert_eq!(chunk.text(), "\x1b[31mred\x1b[0m\n");
    }

    /// Clearing is not "discarding": a cleared buffer starts its count again,
    /// which is what a person clearing the scrollback expects to see.
    #[test]
    fn clearing_empties_the_buffer_and_its_counters() {
        let mut buffer = TerminalBuffer::new(DEFAULT_BUFFER_LIMITS);
        buffer.push(Stream::Stdout, b"output\n");

        buffer.clear();

        assert_eq!(buffer.bytes(), 0);
        assert_eq!(buffer.lines(), 0);
        assert_eq!(buffer.dropped_bytes(), 0);
        assert_eq!(chunk_count(&buffer), 0);
    }
}
