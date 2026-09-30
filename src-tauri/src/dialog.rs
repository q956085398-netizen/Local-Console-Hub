//! The one place the app asks the user something, or tells them something
//! (`docs/MVP_IMPLEMENTATION_SPEC.md` §11, `docs/PRODUCT_SPEC.md` §8).
//!
//! Both calls here are native message boxes rather than app-drawn dialogs, and
//! that is the point: they are used exactly when there is no window to draw in.
//! The tray's Exit is usually pressed while the window is *hidden* (§11 names
//! this the MVP-acceptable confirmation), and a launch request that could not be
//! delivered is reported by a process that has no window at all and is about to
//! end (#60) — a release build is a Windows GUI subsystem binary, so it has no
//! console to print to either.
//!
//! The wording of each question and message is decided by its caller, which is
//! testable; this module only carries it to the user and brings the answer back.

/// Ask a yes/no question about something destructive.
///
/// The default answer is **no**: the cancel button carries `MB_DEFBUTTON2`, so
/// a stray Enter on a dialog the user did not mean to open does not stop
/// anything. `true` means the user explicitly accepted.
#[cfg(windows)]
pub fn confirm(message: &str) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDOK, MB_DEFBUTTON2, MB_ICONWARNING, MB_OKCANCEL, MB_SETFOREGROUND, MB_TOPMOST,
    };

    let style = MB_OKCANCEL | MB_ICONWARNING | MB_DEFBUTTON2 | MB_SETFOREGROUND | MB_TOPMOST;
    // SAFETY: `message_box` NUL-terminates both strings, and the call is
    // synchronous: it returns once the box is dismissed, and it retains
    // nothing we pass it.
    let answer = unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message_box(message).as_ptr(),
            message_box(crate::ipc::APP_NAME).as_ptr(),
            style,
        )
    };
    answer == IDOK
}

/// Tell the user something they need to know but did not ask for: that Exit
/// could not proceed, or that a launch was not delivered, and why.
#[cfg(windows)]
pub fn report(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
    };

    let style = MB_OK | MB_ICONWARNING | MB_SETFOREGROUND | MB_TOPMOST;
    // SAFETY: as in `confirm`.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message_box(message).as_ptr(),
            message_box(crate::ipc::APP_NAME).as_ptr(),
            style,
        );
    }
}

/// `text` as the NUL-terminated UTF-16 Win32 wants.
///
/// Windows returns `u16`s but does not hand the allocation back, so the buffer
/// is built here and dropped by the caller. An interior NUL would truncate the
/// message at the ABI boundary; the messages this module is handed are built by
/// the app, never by the user, so that cannot happen — and it is removed
/// anyway, so a future caller that interpolated one would lose a character
/// rather than a sentence.
#[cfg(windows)]
fn message_box(text: &str) -> Vec<u16> {
    text.encode_utf16()
        .filter(|unit| *unit != 0)
        .chain([0])
        .collect()
}

/// Non-Windows builds have no tray either, but the module still has to compile
/// and still has to refuse to answer on the user's behalf.
///
/// Answering `true` here would be the worst possible stub: it would turn "we
/// cannot ask" into "the user said yes, stop everything", which is exactly the
/// silent kill §11 forbids. `false` leaves the sessions running and the Hub
/// open, which is the failure that destroys nothing.
#[cfg(not(windows))]
pub fn confirm(_message: &str) -> bool {
    false
}

#[cfg(not(windows))]
pub fn report(message: &str) {
    eprintln!("{APP_NAME}: {message}", APP_NAME = crate::ipc::APP_NAME);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A message arrives at Win32 as the exact text, NUL-terminated once.
    #[cfg(windows)]
    #[test]
    fn a_message_is_encoded_as_nul_terminated_utf16() {
        let encoded = message_box("停止并退出？");

        assert_eq!(encoded.last(), Some(&0u16));
        assert_eq!(encoded.iter().filter(|unit| **unit == 0).count(), 1);
        assert_eq!(
            String::from_utf16(&encoded[..encoded.len() - 1]).expect("the text survives"),
            "停止并退出？"
        );
    }

    /// The Windows API would read an interior NUL as the end of the string.
    #[cfg(windows)]
    #[test]
    fn an_interior_nul_cannot_truncate_a_message() {
        let encoded = message_box("before\0after");

        let text = String::from_utf16(&encoded[..encoded.len() - 1]).expect("the text survives");
        assert!(text.starts_with("before"), "{text}");
        assert!(text.ends_with("after"), "{text}");
    }
}
