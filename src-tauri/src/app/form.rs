//! What a save made from a form reports when it refuses (#64, #65).
//!
//! Two entries save a launch configuration, and both ask a form for the words
//! a user can supply: "添加应用" saves a service, and a terminal's "保存启动
//! 配置" saves the shell and directory it is already running. What they refuse
//! with is one shape — a sentence, plus the form input it belongs to when the
//! config layer named one — so it lives here rather than being declared twice.
//!
//! The field is optional because only *some* refusals have one. A validation
//! failure names the input it came from (`cwd`, `port`); a config file that
//! cannot be written names no input at all, and a dialog that put that beside a
//! box would be pointing at an innocent one.

use serde::Serialize;

/// Why a form's save was refused or could not be completed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormError {
    /// The form input the message belongs beside, when there is one.
    ///
    /// The names are the *config layer's* (`close_impact`, not
    /// `closeImpact`), so the two sides share one vocabulary and the dialog
    /// does not re-map anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    pub message: String,
}

impl FormError {
    pub fn new(field: Option<&str>, message: impl Into<String>) -> Self {
        FormError {
            field: field.map(str::to_owned),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FormError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for FormError {}
