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
//!
//! ## Two spellings, one grammar
//!
//! A request reaches the app in one of two ways: a shortcut starts the process
//! and the process reads its own command line, or a running Hub reads a frame
//! off its pipe (#63). Both are the same request, so both are spelled by
//! [`Request`] and parsed here — a second grammar for the command line would be
//! a second place for `--new-terminal` to mean something slightly different.

use std::ffi::OsString;

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
/// 请求"): the normal open, #62's temporary terminal (with the directory #63
/// carries for it) and #64's configured application. An operation this build
/// does not know is rejected by name rather than read as one it does.
///
/// The two spellings of a request — command line and frame — are both checked
/// against this type, so an operation cannot exist in one and be missing from
/// the other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "request", rename_all = "camelCase")]
pub enum Request {
    /// Bring the Hub's window back, in the state it was hidden in.
    ///
    /// A *normal* open (spec §3): the entry was clicked and nothing else was
    /// asked for. It restores — it does not start a session, and it does not
    /// restart or duplicate anything the Hub is already running.
    Open,

    /// Add an interactive terminal to the Hub and put the user in it (#63).
    ///
    /// This is what the user's own PowerShell shortcut asks for: the entry was
    /// not "open the Hub", it was "give me a shell", and the Hub is where that
    /// shell should live rather than a taskbar of its own (spec §17).
    ///
    /// `directory` is the entry's working directory, kept apart from the
    /// request rather than folded into a command string: it is a *parameter*
    /// (spec §5), so a path with a space or a CJK name in it is one value and
    /// not something a shell has to re-split. `None` means the entry named no
    /// directory, and the terminal opens in the user's home directory — never
    /// in whatever directory the Hub happens to have been started from.
    NewTerminal {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        directory: Option<String>,
    },
    /// Open a configured application, by session id (#64).
    ///
    /// The same activation the window's own control performs (spec §2: 窗口、
    /// 快捷方式与托盘使用同一应用操作边界) — start it if nothing is running,
    /// bring the run it already has forward if something is.
    OpenApplication {
        /// The configured session's id, as the config file names it.
        id: String,
    },
}

/// The command-line flag that asks for [`Request::NewTerminal`] (#63).
pub const NEW_TERMINAL_FLAG: &str = "--new-terminal";

/// The command-line flag that names the directory the terminal opens in.
pub const DIRECTORY_FLAG: &str = "--directory";

/// Open a saved configuration through the same activation as the window.
pub const OPEN_APPLICATION_FLAG: &str = "--open-app";

/// The request this process was started with, from the real command line.
///
/// The program name is dropped: what a request is about is never the exe.
pub fn request_from_env() -> Result<Request, String> {
    let arguments: Vec<OsString> = std::env::args_os().skip(1).collect();
    from_command_line(&arguments)
}

/// The request a command line asks for (`argv[0]` already removed).
///
/// The shortcut that starts the Hub is a *second* spelling of the pipe's
/// grammar (module doc), so the flags are handled with the same care the frame
/// reader shows them: an argument this build does not know stops the launch
/// with a sentence naming it, rather than being ignored. Ignoring it would be
/// the worst of the available answers — the user asked their shortcut for a
/// terminal and got a Hub window instead, with nothing saying why.
///
/// Arguments arrive as [`OsString`] because Windows hands a process UTF-16 that
/// a path with a CJK name survives, and turning it into text is the one step
/// that can still go wrong: a directory that is not valid Unicode cannot travel
/// in a JSON frame, so it is refused by its own name rather than lossily
/// converted into a path that does not exist.
pub fn from_command_line(arguments: &[OsString]) -> Result<Request, String> {
    let mut new_terminal = false;
    let mut directory: Option<String> = None;
    let mut application: Option<String> = None;

    let mut index = 0;
    while index < arguments.len() {
        let argument = text(&arguments[index])?;
        match argument.as_str() {
            OPEN_APPLICATION_FLAG => {
                if application.is_some() {
                    return Err(format!("启动参数 `{OPEN_APPLICATION_FLAG}` 不能重复。"));
                }
                index += 1;
                let id = arguments.get(index).ok_or_else(|| {
                    format!("启动参数 `{OPEN_APPLICATION_FLAG}` 后面缺少配置 id。")
                })?;
                let id = text(id)?;
                if !crate::config::is_filesystem_safe_component(&id) {
                    return Err("配置 id 必须以字母或数字开头，且只含字母、数字、下划线或连字符，最多 64 字符。".into());
                }
                application = Some(id);
            }
            NEW_TERMINAL_FLAG => new_terminal = true,
            DIRECTORY_FLAG => {
                index += 1;
                let value = arguments.get(index).ok_or_else(|| {
                    format!(
                        "启动参数 `{DIRECTORY_FLAG}` 后面缺少目录。\n\n\
                         本次启动已停止；请修正快捷方式的目标后再试。"
                    )
                })?;
                directory = Some(text(value)?);
            }
            other => {
                return Err(format!(
                    "无法理解启动参数 `{other}`。\n\n\
                     本次启动已停止；请修正快捷方式的目标后再试。\
                     普通打开不需要任何参数。"
                ))
            }
        }
        index += 1;
    }

    if let Some(id) = application {
        if new_terminal || directory.is_some() {
            return Err(format!("`{OPEN_APPLICATION_FLAG}` 不能与 `{NEW_TERMINAL_FLAG}` 或 `{DIRECTORY_FLAG}` 一起使用。"));
        }
        return Ok(Request::OpenApplication { id });
    }

    match (new_terminal, directory) {
        (true, directory) => Ok(Request::NewTerminal { directory }),
        // No flags at all: the entry was clicked and nothing else was asked
        // for. This is the ordinary open, and it stays the ordinary open.
        (false, None) => Ok(Request::Open),
        // A directory on its own has no operation to belong to. Reading it as
        // "open" would drop what the user asked for; reading it as a terminal
        // would invent a request nobody sent.
        (false, Some(_)) => Err(format!(
            "启动参数 `{DIRECTORY_FLAG}` 只在 `{NEW_TERMINAL_FLAG}` 一起给出时才有意义。\n\n\
             本次启动已停止；请修正快捷方式的目标后再试。"
        )),
    }
}

/// One argument as the text the request carries.
///
/// A lone surrogate in the command line would be a path no API can open, so it
/// is refused rather than replaced: `to_string_lossy` would hand the Hub a
/// different path from the one the shortcut names, and a shell that opened
/// somewhere else is exactly what spec §5 forbids.
fn text(argument: &OsString) -> Result<String, String> {
    argument.to_str().map(str::to_owned).ok_or_else(|| {
        format!(
            "启动参数 `{}` 不是有效的 Unicode 文本，无法作为目录使用。\n\n\
             本次启动已停止；请修正快捷方式的目标后再试。",
            argument.to_string_lossy()
        )
    })
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
    /// being read as the one operation it does know: a newer entry asking to
    /// open a configured application must not get a window instead (spec §3).
    #[test]
    fn an_unknown_operation_is_refused_rather_than_read_as_open() {
        let error = decode_request(br#"{"request":"openApp"}"#).expect_err("it is refused");

        let FrameError::Malformed(reason) = error else {
            panic!("an unknown operation is malformed, not {error:?}");
        };
        assert!(
            reason.contains("openApp"),
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

    /// A command line as the process receives it, `argv[0]` already dropped.
    fn command_line(arguments: &[&str]) -> Vec<OsString> {
        arguments.iter().map(OsString::from).collect()
    }

    /// Clicking the Hub's own entry asks for nothing but the window (story 2,
    /// H01): the ordinary open stays the ordinary open.
    #[test]
    fn a_plain_command_line_is_the_ordinary_open() {
        assert_eq!(from_command_line(&command_line(&[])), Ok(Request::Open));
    }

    /// The daily PowerShell shortcut's request (stories 10–11, H04).
    #[test]
    fn the_new_terminal_flag_asks_the_hub_for_a_terminal() {
        assert_eq!(
            from_command_line(&command_line(&[NEW_TERMINAL_FLAG])),
            Ok(Request::NewTerminal { directory: None })
        );
    }

    /// Story 16 and H06 in one assertion: the directory is carried as the one
    /// value the shortcut named, spaces and CJK included, and is not split,
    /// trimmed or otherwise reinterpreted on the way.
    #[test]
    fn the_directory_flag_carries_a_path_verbatim() {
        let directory = r"D:\工作 目录\项目 A";

        assert_eq!(
            from_command_line(&command_line(&[
                NEW_TERMINAL_FLAG,
                DIRECTORY_FLAG,
                directory
            ])),
            Ok(Request::NewTerminal {
                directory: Some(directory.to_owned())
            })
        );
    }

    /// A shortcut is built by a dialog that may write the arguments in either
    /// order; which one it picked is not part of what the user asked for.
    #[test]
    fn the_flags_do_not_depend_on_their_order() {
        let directory = r"C:\Program Files";

        assert_eq!(
            from_command_line(&command_line(&[
                DIRECTORY_FLAG,
                directory,
                NEW_TERMINAL_FLAG
            ])),
            from_command_line(&command_line(&[
                NEW_TERMINAL_FLAG,
                DIRECTORY_FLAG,
                directory
            ]))
        );
    }

    /// A directory with no operation to belong to is refused rather than read
    /// as one of the two: the user asked for something specific and the honest
    /// answer is that this build cannot tell what it was.
    #[test]
    fn a_directory_without_the_new_terminal_flag_is_refused() {
        let reason = from_command_line(&command_line(&[DIRECTORY_FLAG, r"D:\Work"]))
            .expect_err("a directory on its own is not a request");

        assert!(reason.contains(DIRECTORY_FLAG), "{reason}");
        assert!(reason.contains(NEW_TERMINAL_FLAG), "{reason}");
    }

    /// A flag whose value never arrived stops the launch by name rather than
    /// being read as "no directory was named" — that reading would open a
    /// terminal in the home directory while the user was looking at a
    /// shortcut that names theirs.
    #[test]
    fn the_directory_flag_needs_its_value() {
        let reason = from_command_line(&command_line(&[NEW_TERMINAL_FLAG, DIRECTORY_FLAG]))
            .expect_err("a flag without its value is not a request");

        assert!(reason.contains(DIRECTORY_FLAG), "{reason}");
    }

    /// An argument this build does not know is refused *by name*, like an
    /// unknown frame: newer entries exist, and silently ignoring half of one
    /// would open the wrong thing.
    #[test]
    fn an_unknown_flag_stops_the_launch_by_name() {
        let reason = from_command_line(&command_line(&["--unknown", "comfyui"]))
            .expect_err("this build has no such request");

        assert!(reason.contains("--unknown"), "{reason}");
    }

    /// The wire form of the new request, asserted where a rename would be
    /// caught: `newTerminal`, and the directory under its own key.
    #[test]
    fn a_new_terminal_request_carries_its_directory_on_the_wire() {
        let request = Request::NewTerminal {
            directory: Some(r"D:\Work".to_owned()),
        };

        let frame = encode(&request).expect("a request encodes");
        assert_eq!(
            std::str::from_utf8(&frame[..frame.len() - 1]).expect("frames are UTF-8"),
            r#"{"request":"newTerminal","directory":"D:\\Work"}"#
        );
        assert_eq!(
            decode_request(&frame[..frame.len() - 1]).expect("it decodes back"),
            request
        );
    }

    /// An entry that named no directory says nothing about one, rather than
    /// saying `null`: the two ends of the pipe are different builds on a
    /// machine where the app has just been replaced, and an absent field is
    /// the shape both of them read.
    #[test]
    fn a_new_terminal_request_without_a_directory_carries_no_directory() {
        let frame = encode(&Request::NewTerminal { directory: None }).expect("a request encodes");

        assert_eq!(
            std::str::from_utf8(&frame[..frame.len() - 1]).expect("frames are UTF-8"),
            r#"{"request":"newTerminal"}"#
        );
        assert_eq!(
            decode_request(&frame[..frame.len() - 1]).expect("it decodes back"),
            Request::NewTerminal { directory: None }
        );
    }

    /// A terminal request and a window request are told apart on the wire —
    /// the whole point of the second operation (stories 10 and 12: the same
    /// shortcut must not be read as "just open the Hub").
    #[test]
    fn the_two_operations_are_not_confused_on_the_wire() {
        let terminal = encode(&Request::NewTerminal { directory: None }).expect("it encodes");
        let open = encode(&Request::Open).expect("it encodes");

        assert_ne!(terminal, open);
        assert_ne!(
            decode_request(&terminal[..terminal.len() - 1]).expect("it decodes"),
            decode_request(&open[..open.len() - 1]).expect("it decodes")
        );
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

    #[test]
    fn a_configured_application_entry_carries_its_stable_id() {
        assert_eq!(
            from_command_line(&command_line(&["--open-app", "comfyui"])),
            Ok(Request::OpenApplication {
                id: "comfyui".into()
            })
        );
    }

    #[test]
    fn application_entries_reject_missing_invalid_duplicate_and_conflicting_targets() {
        for arguments in [
            vec!["--open-app"],
            vec!["--open-app", ""],
            vec!["--open-app", "app;calc.exe"],
            vec!["--open-app", "--new-terminal"],
            vec!["--open-app", "comfyui", "--open-app", "other"],
            vec!["--open-app", "comfyui", "--new-terminal"],
            vec!["--new-terminal", "--open-app", "comfyui"],
            vec!["--directory", "D:\\Work", "--open-app", "comfyui"],
        ] {
            assert!(
                from_command_line(&command_line(&arguments)).is_err(),
                "{arguments:?}"
            );
        }
    }
    /// The configured-application request carries the session it names, so the
    /// Hub knows *which* application to open — and its operation name is what
    /// an older build refuses by.
    #[test]
    fn an_open_application_request_carries_the_session_it_names() {
        let frame = encode(&Request::OpenApplication {
            id: "comfyui".to_owned(),
        })
        .expect("a request encodes");

        assert_eq!(
            std::str::from_utf8(&frame[..frame.len() - 1]).expect("frames are UTF-8"),
            r#"{"request":"openApplication","id":"comfyui"}"#
        );
        assert_eq!(
            decode_request(&frame[..frame.len() - 1]).expect("it decodes back"),
            Request::OpenApplication {
                id: "comfyui".to_owned()
            }
        );
    }

    /// The two operations are told apart by name, not by shape: a request that
    /// names an application must never be read as a plain open.
    #[test]
    fn an_open_application_is_never_read_as_a_normal_open() {
        assert_ne!(
            Request::OpenApplication {
                id: "comfyui".to_owned()
            },
            Request::Open
        );
        assert!(matches!(
            decode_request(br#"{"request":"open"}"#),
            Ok(Request::Open)
        ));
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
