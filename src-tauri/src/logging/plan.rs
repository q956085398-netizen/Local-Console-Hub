//! The effective logging state of a run: what the configured policy actually
//! resolves to (`docs/LOGGING.md` §1.4, §3).
//!
//! `docs/LOGGING.md` §1.4 puts one requirement above the mechanics: the user
//! must always be able to tell whether output is being recorded, where it goes,
//! and whether their input is included. That requirement is why resolution is a
//! separate, named step here rather than a `match` buried in the writer — the
//! same value answers "what will happen" before a run starts and "what is
//! happening" while it does, and it is what [`LogStatus`] reports to the UI.
//!
//! A policy that cannot be honoured — the Hub has no writable app-data
//! directory, or a configured path is missing — resolves to [`Persistence::Off`]
//! *and* a [`LogError`] saying so. Failing back to "nothing is written" without
//! saying why is exactly the "you thought it was recording" state §1.4 forbids;
//! refusing to start the service over a log file would put logging policy above
//! lifecycle truth (`docs/DEVELOPMENT.md` §3).

use std::path::PathBuf;

use serde::Serialize;

use crate::config::{EffectiveLogMode, EffectiveLogging, LogSource};

use super::buffer::BufferSummary;
use super::error::LogError;

/// What the Hub will do with a run's output.
///
/// One variant per policy, carrying the file it will touch. `External` is a
/// link rather than a write: `docs/DECISIONS.md` D-005 keeps the Hub from
/// duplicating a log the application already owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Persistence {
    /// Nothing is written anywhere. The in-memory buffer still fills
    /// (`docs/LOGGING.md` §8).
    Off,
    /// Everything the run writes goes into `path` as it arrives.
    Capture { path: PathBuf },
    /// Output is held in a bounded buffer and written to `path` only if the
    /// run ends badly (`docs/LOGGING.md` §3).
    OnError { path: PathBuf },
    /// Nothing is written until the user asks for it; then `path`.
    Manual { path: PathBuf },
    /// The application owns the log at `path`; the Hub only points at it.
    External { path: PathBuf },
}

/// The state a UI shows for a session's logging: Off / Capturing / External /
/// On Error (`docs/LOGGING.md` §1.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogState {
    Off,
    Capturing,
    External,
    OnError,
}

/// The state a configured policy implies, for a session that has no run yet.
///
/// The same mapping [`Persistence::initial_state`] applies, expressed over the
/// config because that is all a stopped session has; the two are held together
/// by a test rather than by one calling the other, because the runtime variant
/// carries a resolved path and this one deliberately does not.
pub fn policy_state(source: LogSource, mode: EffectiveLogMode) -> LogState {
    match source {
        // Nothing to persist, whatever the mode says.
        LogSource::None => LogState::Off,
        LogSource::External => LogState::External,
        LogSource::Captured => match mode {
            EffectiveLogMode::Off => LogState::Off,
            EffectiveLogMode::Always => LogState::Capturing,
            // `manual` stays Off until the user asks: the policy permits
            // recording, it does not start it.
            EffectiveLogMode::Manual => LogState::Off,
            EffectiveLogMode::OnError => LogState::OnError,
        },
    }
}

impl Persistence {
    /// The state before anything has been written.
    ///
    /// A `manual` session starts `Off` on purpose: until the user asks for
    /// recording, nothing is being recorded, and saying `Capturing` would be
    /// the exact lie §1.4 exists to prevent.
    pub fn initial_state(&self) -> LogState {
        match self {
            Persistence::Off => LogState::Off,
            Persistence::Capture { .. } => LogState::Capturing,
            Persistence::OnError { .. } => LogState::OnError,
            Persistence::Manual { .. } => LogState::Off,
            Persistence::External { .. } => LogState::External,
        }
    }

    /// Whether the Hub takes the run's stdout/stderr at all.
    ///
    /// True for `off` as well: §8 keeps the in-memory buffer for every session,
    /// and the buffer is fed by the same capture. False only for `external`,
    /// where taking the output would either duplicate the application's log or
    /// leave the application's own file and the Hub's buffer telling different
    /// stories (D-005).
    pub fn captures_output(&self) -> bool {
        !matches!(self, Persistence::External { .. })
    }

    /// The file the Hub itself writes for this run, if any.
    pub fn hub_log_path(&self) -> Option<&PathBuf> {
        match self {
            Persistence::Capture { path }
            | Persistence::OnError { path }
            | Persistence::Manual { path } => Some(path),
            Persistence::External { .. } | Persistence::Off => None,
        }
    }

    /// The application-owned log this session is linked to, if any.
    pub fn external_path(&self) -> Option<&PathBuf> {
        match self {
            Persistence::External { path } => Some(path),
            _ => None,
        }
    }
}

/// A resolved policy plus any reason it could not be honoured in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPlan {
    pub persistence: Persistence,
    /// Set when the configured policy was downgraded to
    /// [`Persistence::Off`]; the run continues without persistence and the
    /// reason reaches the UI through [`LogStatus::last_error`].
    pub problem: Option<LogError>,
}

impl LogPlan {
    /// A plan that was honoured exactly as configured.
    pub fn new(persistence: Persistence) -> Self {
        LogPlan {
            persistence,
            problem: None,
        }
    }

    /// Resolve the validated `logging:` block of a session into what will
    /// happen for one run.
    ///
    /// `run_log_file` is the path this run's log would live at, built from the
    /// app-data layout (`crate::config::run_log_path`); it is `None` when the
    /// Hub has no writable location to put one, or when the session id cannot
    /// become a directory name.
    ///
    /// Configuration validation has already rejected contradictory
    /// combinations (T01), so anything still unknown here — an `external`
    /// source with no path, a missing app-data root — is an environment
    /// problem, and is reported as one.
    pub fn resolve(effective: &EffectiveLogging, run_log_file: Option<PathBuf>) -> Self {
        match effective.source {
            LogSource::None => LogPlan::new(Persistence::Off),
            LogSource::External => match &effective.external_path {
                Some(path) => LogPlan::new(Persistence::External {
                    path: PathBuf::from(path),
                }),
                // Validation requires a path with `source: external`, so this
                // is a config built by something other than T01's loader.
                None => LogPlan {
                    persistence: Persistence::Off,
                    problem: Some(LogError::new(
                        "resolving the logging policy",
                        "`source: external` has no `logging.path`, so there is no \
                         application-owned log to link — set the path or use \
                         `source: captured`",
                    )),
                },
            },
            LogSource::Captured => match effective.mode {
                EffectiveLogMode::Off => LogPlan::new(Persistence::Off),
                mode => match run_log_file {
                    Some(path) => LogPlan::new(match mode {
                        EffectiveLogMode::Always => Persistence::Capture { path },
                        EffectiveLogMode::OnError => Persistence::OnError { path },
                        EffectiveLogMode::Manual => Persistence::Manual { path },
                        EffectiveLogMode::Off => Persistence::Off,
                    }),
                    None => LogPlan {
                        persistence: Persistence::Off,
                        problem: Some(LogError::new(
                            "resolving the logging policy",
                            "the Hub has no writable log directory for this run, so \
                             `source: captured` output cannot be persisted",
                        )),
                    },
                },
            },
        }
    }
}

/// The complete "is this being logged?" answer for one session
/// (`docs/LOGGING.md` §1.4).
///
/// Reported through `get_log_info`, and republished whenever it changes, so a
/// UI never has to infer persistence from the presence of a file.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogStatus {
    pub session_id: String,
    /// Effective mode from the config (`auto` already resolved away).
    pub mode: EffectiveLogMode,
    /// Effective source from the config.
    pub source: LogSource,
    /// What is happening right now — the one field that changes while a
    /// session runs.
    pub state: LogState,
    /// The file the Hub writes for the current run, if it is writing one.
    pub log_file: Option<String>,
    /// Whether the file this status names for the current run is on disk *right
    /// now* (`docs/DECISIONS.md` D-022).
    ///
    /// The same question the run history answers per entry, asked of the file
    /// the Logs tab's actions would act on: a `captured` session's is
    /// [`LogStatus::log_file`], and an `external` session's is the
    /// application's own ([`LogStatus::external_log`], D-005) — the file a
    /// log action resolves to when it names the current run. Asked on read and
    /// never written down, so a log removed by hand is answered exactly like
    /// one retention swept.
    pub log_file_present: bool,
    /// The application-owned log this session is linked to.
    pub external_log: Option<String>,
    /// Where this session's Hub-written logs live, so the folder can be opened
    /// without the Hub (D-011).
    pub session_log_dir: Option<String>,
    /// Whether user input is persisted. Always false in the MVP: spec §15 and
    /// `docs/LOGGING.md` §4 forbid persisting stdin, and the field exists so
    /// the UI can state it rather than imply it.
    pub records_input: bool,
    /// The in-memory scrollback this session currently holds.
    pub buffer: BufferSummary,
    /// Whether the current run's file hit its size cap and stopped taking
    /// output. A log that quietly ends at a fixed size is indistinguishable
    /// from a run that went quiet, so the UI has to be able to say which.
    pub truncated: bool,
    /// Why logging is not working as configured, if it is not.
    pub last_error: Option<LogError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logging(mode: EffectiveLogMode, source: LogSource) -> EffectiveLogging {
        EffectiveLogging {
            mode,
            source,
            external_path: None,
        }
    }

    fn run_file() -> Option<PathBuf> {
        Some(PathBuf::from("logs/comfyui/2026-09/run.log"))
    }

    #[test]
    fn captured_always_writes_every_run_to_a_run_file() {
        let plan = LogPlan::resolve(
            &logging(EffectiveLogMode::Always, LogSource::Captured),
            run_file(),
        );

        assert_eq!(
            plan.persistence,
            Persistence::Capture {
                path: PathBuf::from("logs/comfyui/2026-09/run.log")
            }
        );
        assert_eq!(plan.persistence.initial_state(), LogState::Capturing);
        assert!(plan.problem.is_none());
        assert!(plan.persistence.captures_output());
    }

    #[test]
    fn captured_on_error_only_commits_to_a_file_on_failure() {
        let plan = LogPlan::resolve(
            &logging(EffectiveLogMode::OnError, LogSource::Captured),
            run_file(),
        );

        assert!(matches!(plan.persistence, Persistence::OnError { .. }));
        assert_eq!(plan.persistence.initial_state(), LogState::OnError);
    }

    /// `manual` records nothing until asked; showing `Capturing` before that
    /// is the state §1.4 exists to prevent.
    #[test]
    fn manual_starts_off_and_has_a_file_waiting_for_it() {
        let plan = LogPlan::resolve(
            &logging(EffectiveLogMode::Manual, LogSource::Captured),
            run_file(),
        );

        assert!(matches!(plan.persistence, Persistence::Manual { .. }));
        assert_eq!(plan.persistence.initial_state(), LogState::Off);
        assert!(plan.persistence.hub_log_path().is_some());
    }

    /// An interactive terminal with the default policy must not produce a file
    /// (spec §16: "terminal mode off creates no persistent log").
    #[test]
    fn off_writes_nothing_and_runs_out_no_file() {
        let plan = LogPlan::resolve(&logging(EffectiveLogMode::Off, LogSource::None), run_file());

        assert_eq!(plan.persistence, Persistence::Off);
        assert_eq!(plan.persistence.initial_state(), LogState::Off);
        assert!(plan.persistence.hub_log_path().is_none());
        assert!(
            plan.persistence.captures_output(),
            "the in-memory buffer is kept even when nothing is persisted"
        );
    }

    #[test]
    fn external_links_the_application_log_instead_of_writing_one() {
        let mut effective = logging(EffectiveLogMode::Always, LogSource::External);
        effective.external_path = Some("D:/Tools/SillyTavern/data/access.log".to_owned());

        let plan = LogPlan::resolve(&effective, run_file());

        assert_eq!(
            plan.persistence,
            Persistence::External {
                path: PathBuf::from("D:/Tools/SillyTavern/data/access.log")
            }
        );
        assert_eq!(plan.persistence.initial_state(), LogState::External);
        assert!(plan.persistence.hub_log_path().is_none());
        assert!(
            !plan.persistence.captures_output(),
            "D-005: the application's log must not be duplicated"
        );
    }

    /// A policy the Hub cannot honour must be reported, not silently ignored:
    /// "recording is on" and "a file appeared" must never be able to disagree.
    #[test]
    fn a_captured_run_with_no_writable_layout_is_off_and_says_why() {
        let plan = LogPlan::resolve(
            &logging(EffectiveLogMode::Always, LogSource::Captured),
            None,
        );

        assert_eq!(plan.persistence, Persistence::Off);
        assert_eq!(plan.persistence.initial_state(), LogState::Off);
        let problem = plan.problem.expect("the downgrade is reported");
        assert_eq!(problem.operation, "resolving the logging policy");
        assert!(
            problem.message.contains("writable log directory"),
            "unhelpful message: {}",
            problem.message
        );
    }

    #[test]
    fn external_without_a_path_is_reported_as_a_configuration_problem() {
        let plan = LogPlan::resolve(
            &logging(EffectiveLogMode::Always, LogSource::External),
            run_file(),
        );

        assert_eq!(plan.persistence, Persistence::Off);
        assert!(plan
            .problem
            .expect("reported")
            .message
            .contains("logging.path"));
    }

    /// The frontend reads these literals (`docs/LOGGING.md` §1.4 names the
    /// states); a rename would silently mislabel the session in the UI.
    #[test]
    fn log_states_serialize_with_the_documented_vocabulary() {
        let states = [
            (LogState::Off, "off"),
            (LogState::Capturing, "capturing"),
            (LogState::External, "external"),
            (LogState::OnError, "on_error"),
        ];

        for (state, expected) in states {
            assert_eq!(
                serde_json::to_value(state).expect("serializes"),
                serde_json::json!(expected)
            );
        }
    }

    /// The policy-level state and the resolved-plan state answer the same
    /// question from different inputs; every combination has to agree, or a
    /// stopped session and a running one would describe the same policy
    /// differently.
    #[test]
    fn the_policy_state_and_the_resolved_plan_state_agree() {
        let modes = [
            EffectiveLogMode::Off,
            EffectiveLogMode::Always,
            EffectiveLogMode::OnError,
            EffectiveLogMode::Manual,
        ];
        let sources = [LogSource::None, LogSource::Captured, LogSource::External];

        for source in sources {
            for mode in modes {
                let mut effective = logging(mode, source);
                effective.external_path =
                    (source == LogSource::External).then(|| "app.log".to_owned());

                let plan = LogPlan::resolve(&effective, run_file());
                assert_eq!(
                    plan.persistence.initial_state(),
                    policy_state(source, mode),
                    "{} / {} disagree between the config and the resolved plan",
                    source.as_str(),
                    mode.as_str()
                );
            }
        }
    }
}
