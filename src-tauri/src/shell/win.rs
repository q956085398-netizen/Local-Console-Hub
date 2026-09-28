//! Windows shell backend: `ShellExecuteW` with the default verb.
//!
//! The OS's own mechanism — the Hub never guesses at a handler, picks a text
//! editor, or writes a shortcut. No console window is created: `ShellExecuteW`
//! hands the path straight to the shell, which is what makes it preferable to
//! `cmd /c start` for a GUI process (that would flash a console, and would put
//! a path through a command-line parser it does not need to go through).
//!
//! COM note: `ShellExecuteW` is documented to work without an explicit
//! `CoInitialize`, and the call arrives on the Tauri IPC thread — the same
//! apartment WebView2 already initialised. Nothing here does COM by itself.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::ShellError;

/// The default verb: "open", whatever that means for this path.
const OPEN: *const u16 = std::ptr::null();

/// Hand the path to the shell.
///
/// Whether the path exists is [`super::open_path`]'s check, not this one: the
/// two callers that matter (a log file, a log folder) are resolved from session
/// state and want the same answer about a path that is not there.
pub fn open_path(path: &Path, operation: &str) -> Result<(), ShellError> {
    let wide = wide(path);
    // The return value is an `HINSTANCE` for historical reasons: anything above
    // 32 is success, and anything at or below it is one of the `SE_ERR_*`
    // codes. It is not an error code `GetLastError` could explain.
    //
    // No parent window is passed: this is not a dialog the Hub owns, and a
    // modal box anchored to a window the user may have hidden would be worse
    // than one the shell places itself.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            OPEN,
            wide.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };

    let code = result as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(ShellError::new(operation, path, describe(code)))
    }
}

/// The `SE_ERR_*` codes a user can act on, and what to do about them
/// (`docs/DEVELOPMENT.md` §9). Anything else keeps the number, because
/// inventing advice for an unknown cause is worse than reporting it.
fn describe(code: isize) -> String {
    match code {
        0 => "Windows could not start the operation — it may be out of memory".to_owned(),
        2 => "the file is not there any more — it may have been removed since the list was read"
            .to_owned(),
        3 => "the folder is not there any more — it may have been removed since the list was read"
            .to_owned(),
        5 => "Windows refused access to the path — check the file's permissions".to_owned(),
        31 => "no application is associated with this file type — open it once from Explorer and \
             choose one"
            .to_owned(),
        other => format!("Windows refused the request (ShellExecute error {other})"),
    }
}

/// A NUL-terminated UTF-16 copy of `path`, the form every `W` API takes.
///
/// Windows paths are not necessarily valid UTF-8, so the conversion goes
/// through `OsStr` rather than through a `String`: a path with a character
/// outside the current code page still opens.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UTF-16 with a terminator, and the terminator is the only one.
    #[test]
    fn a_path_is_encoded_as_terminated_utf16() {
        let wide = wide(Path::new("C:/logs/run.log"));

        assert_eq!(wide.last(), Some(&0));
        assert!(!wide[..wide.len() - 1].contains(&0));
        assert_eq!(
            String::from_utf16(&wide[..wide.len() - 1]).expect("valid UTF-16"),
            "C:/logs/run.log"
        );
    }

    /// Bytes that are not UTF-8 survive the trip: they are a fact about the
    /// user's disk, not something this layer may refuse.
    #[test]
    fn a_path_outside_the_code_page_is_still_encoded() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        // An unpaired surrogate: a legal Windows filename component and not a
        // legal UTF-8 string at all.
        let odd = OsString::from_wide(&[0x0041, 0xD800, 0x0042]);

        let wide = wide(Path::new(&odd));

        assert_eq!(wide, vec![0x0041, 0xD800, 0x0042, 0]);
    }

    /// Advice where there is advice to give, the code where there is not.
    #[test]
    fn known_failures_are_reported_with_something_to_do() {
        assert!(describe(5).contains("permissions"), "{}", describe(5));
        assert!(describe(31).contains("associated"), "{}", describe(31));
        assert!(describe(1234).contains("1234"), "{}", describe(1234));
    }
}
