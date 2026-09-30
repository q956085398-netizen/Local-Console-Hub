//! The launch-request protocol (#60): what a later invocation asks the Hub to
//! do, and what the Hub answers.
//!
//! The Hub is one instance per Windows session, so clicking its entry a second
//! time is not a second Hub — it is a *request* handed to the one already
//! running (`docs` spec #59 §2–3). This module is the shape of that request and
//! of its answer, kept apart from whatever carries them: the grammar can be
//! read and tested with no pipe, no window and no OS (`super::win` moves these
//! frames, `crate::app::launch` decides what they do).
//!
//! ## One JSON line, bounded
//!
//! A request crosses a process boundary, so both sides have to agree on a
//! byte-level shape and neither may trust the other. One JSON value per
//! `\n`-terminated UTF-8 line, refused past [`MAX_FRAME_BYTES`], is enough for
//! the three operations the spec names, and it fails closed: an operation this
//! build does not know is a deserialization error the Hub *answers*, rather
//! than a request it quietly treats as "open a window" (spec §3: 冷启动期间
//! 有界接收并等待服务可用，不能静默丢弃).
//!
//! The serializer escapes newlines inside strings, so a frame's `\n` is
//! unambiguous even when a message spans lines.

use serde::{Deserialize, Serialize};

/// The largest frame this build will read, terminator excluded.
///
/// A request is a tag and (later) a directory; the bound is not about the
/// requests that exist — it is what keeps a peer from making the Hub buffer an
/// unbounded stream while it waits for a line that never ends.
pub const MAX_FRAME_BYTES: usize = 8 * 1024;

/// The terminator every frame ends with.
const TERMINATOR: u8 = b'\n';

/// What a later invocation is asking the running Hub to do.
///
/// The enum is the extension point the spec asks for (§3, "显式区分三类启动
/// 请求"): #62 adds the temporary terminal, #63 the directory it starts in, and
/// #64–#67 the configured application. Until then there is exactly one
/// operation, and the Hub rejects anything else by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "camelCase")]
pub enum Request {
    /// Bring the Hub's window back, in the state it was hidden in.
    ///
    /// A *normal* open (spec §3): the entry was clicked and nothing else was
    /// asked for. It restores — it does not start a session, and it does not
    /// restart or duplicate anything the Hub is already running.
    Open,
}

/// The Hub's answer to one [`Request`].
///
/// `message` is what the user is shown when the request could not be taken, so
/// it is a sentence rather than a code: the process that asked is about to end,
/// and the sentence is all that is left of the exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub delivered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Response {
    /// The Hub took the request and did what it asked for.
    pub fn delivered() -> Self {
        Response {
            delivered: true,
            message: None,
        }
    }

    /// The Hub is running but did not do it, and this is why.
    pub fn failed(message: impl Into<String>) -> Self {
        Response {
            delivered: false,
            message: Some(message.into()),
        }
    }
}

/// Why a frame could not be written, read or understood.
#[derive(Debug)]
pub enum FrameError {
    /// The peer went away without sending a whole frame.
    Closed,
    /// The frame was longer than [`MAX_FRAME_BYTES`].
    TooLong,
    /// The frame was not a value this build understands.
    Malformed(String),
    /// The peer connected but said nothing before the deadline.
    TimedOut,
    /// The transport itself failed.
    Io(std::io::Error),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Closed => write!(formatter, "the peer closed the connection first"),
            FrameError::TooLong => write!(
                formatter,
                "the frame was longer than {MAX_FRAME_BYTES} bytes"
            ),
            FrameError::Malformed(reason) => {
                write!(formatter, "the frame was not understood: {reason}")
            }
            FrameError::TimedOut => write!(formatter, "the peer did not answer in time"),
            FrameError::Io(error) => write!(formatter, "the connection failed: {error}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// A value as the bytes one frame is made of: its JSON, then the terminator.
///
/// The trailing `\n` is part of the frame rather than something the writer
/// adds, so the reader and the writer cannot disagree about where a frame ends.
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, FrameError> {
    let mut frame = serde_json::to_vec(value)
        .map_err(|error| FrameError::Malformed(format!("it could not be written: {error}")))?;
    frame.push(TERMINATOR);
    Ok(frame)
}

/// The request one frame (terminator already removed) carries.
pub fn decode_request(frame: &[u8]) -> Result<Request, FrameError> {
    decode(frame)
}

/// The answer one frame (terminator already removed) carries.
pub fn decode_response(frame: &[u8]) -> Result<Response, FrameError> {
    decode(frame)
}

/// Shared by the two decoders: length first, then the payload.
///
/// The bound is checked here as well as at the transport so that a frame which
/// was assembled some other way (a test, a future in-process caller) is refused
/// by the same rule the pipe enforces.
fn decode<T: for<'de> Deserialize<'de>>(frame: &[u8]) -> Result<T, FrameError> {
    if frame.len() > MAX_FRAME_BYTES {
        return Err(FrameError::TooLong);
    }
    serde_json::from_slice(frame).map_err(|error| FrameError::Malformed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_survives_a_round_trip() {
        let frame = encode(&Request::Open).expect("a request encodes");

        assert_eq!(frame.last(), Some(&TERMINATOR));
        assert_eq!(
            decode_request(&frame[..frame.len() - 1]).expect("it decodes back"),
            Request::Open
        );
    }

    /// The tag is what a later build's operation is called, so it is part of
    /// the contract rather than an implementation detail.
    #[test]
    fn a_request_carries_its_operation_by_name() {
        let frame = encode(&Request::Open).expect("a request encodes");

        assert_eq!(
            std::str::from_utf8(&frame[..frame.len() - 1]).expect("frames are UTF-8"),
            r#"{"request":"open"}"#
        );
    }

    /// An operation this build does not know is refused *by name*, rather than
    /// being read as the one operation it does know: a newer entry asking for a
    /// terminal must not get a window instead (spec §3).
    #[test]
    fn an_unknown_operation_is_refused_rather_than_read_as_open() {
        let error = decode_request(br#"{"request":"newTerminal"}"#).expect_err("it is refused");

        let FrameError::Malformed(reason) = error else {
            panic!("an unknown operation is malformed, not {error:?}");
        };
        assert!(
            reason.contains("newTerminal"),
            "the refusal has to name what was asked for: {reason}"
        );
    }

    #[test]
    fn a_frame_without_its_operation_is_refused() {
        assert!(matches!(
            decode_request(b"{}"),
            Err(FrameError::Malformed(_))
        ));
    }

    #[test]
    fn a_frame_that_is_not_json_is_refused() {
        assert!(matches!(
            decode_request(b"open"),
            Err(FrameError::Malformed(_))
        ));
    }

    #[test]
    fn an_oversized_request_is_refused() {
        let mut frame = Vec::from(&b"{"[..]);
        frame.resize(MAX_FRAME_BYTES + 1, b' ');

        assert!(matches!(decode_request(&frame), Err(FrameError::TooLong)));
    }

    /// The bound is a limit, not a threshold: a frame that is exactly as long
    /// as it may be is still read.
    #[test]
    fn a_request_at_the_length_bound_is_accepted() {
        let mut frame = Vec::from(&b"{"[..]);
        frame.resize(MAX_FRAME_BYTES - 1, b' ');
        frame.push(b'}');
        assert_eq!(frame.len(), MAX_FRAME_BYTES);

        // Still malformed as JSON — what is under test is that the *length*
        // rule let it through to the parser rather than refusing it outright.
        assert!(matches!(
            decode_request(&frame),
            Err(FrameError::Malformed(_))
        ));
    }

    #[test]
    fn a_delivered_answer_survives_a_round_trip() {
        let frame = encode(&Response::delivered()).expect("a response encodes");

        let response = decode_response(&frame[..frame.len() - 1]).expect("it decodes back");
        assert_eq!(response, Response::delivered());
        assert!(response.delivered);
        assert_eq!(response.message, None);
    }

    #[test]
    fn a_failed_answer_carries_its_sentence() {
        let frame = encode(&Response::failed("Hub 尚未完成启动。")).expect("a response encodes");

        let response = decode_response(&frame[..frame.len() - 1]).expect("it decodes back");
        assert!(!response.delivered);
        assert_eq!(response.message.as_deref(), Some("Hub 尚未完成启动。"));
    }

    /// A reason may be several lines — the tray's exit failures already are —
    /// and a newline inside it must not be mistaken for the end of the frame.
    #[test]
    fn a_multiline_sentence_stays_inside_one_frame() {
        let frame = encode(&Response::failed("第一行\n第二行")).expect("a response encodes");

        assert_eq!(
            frame.iter().filter(|byte| **byte == TERMINATOR).count(),
            1,
            "only the terminator is a raw newline: {:?}",
            String::from_utf8_lossy(&frame)
        );
        let response = decode_response(&frame[..frame.len() - 1]).expect("it decodes back");
        assert_eq!(response.message.as_deref(), Some("第一行\n第二行"));
    }

    /// A delivered answer says so in the frame, so a reader that ignores the
    /// boolean cannot read "delivered" out of a failure's shape.
    #[test]
    fn the_answer_states_whether_it_was_delivered() {
        let delivered = encode(&Response::delivered()).expect("a response encodes");
        let failed = encode(&Response::failed("no")).expect("a response encodes");

        assert_eq!(
            std::str::from_utf8(&delivered).expect("frames are UTF-8"),
            "{\"delivered\":true}\n"
        );
        assert_eq!(
            std::str::from_utf8(&failed).expect("frames are UTF-8"),
            "{\"delivered\":false,\"message\":\"no\"}\n"
        );
    }
}
