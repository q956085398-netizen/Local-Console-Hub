//! Which sessions a tray control acts on, and what Exit costs
//! (`docs/MVP_IMPLEMENTATION_SPEC.md` §11, `docs/PRODUCT_SPEC.md` §8).
//!
//! The tray asks Session Core to do things; it does not decide lifecycle. What
//! it *does* decide is **which** sessions a control is about and **whether it
//! may proceed at all**, and both of those are pure questions about the current
//! snapshots. Keeping them here means the answers are asserted directly, and
//! the layer that actually calls Session Core stays a loop with no rules in it.
//!
//! ## Failure is defined, not inferred
//!
//! "Restart Failed" needs a definition of failed, and Session Core already has
//! one: an unexpected end with a non-zero exit code lands in
//! [`SessionStatus::Error`], a clean one in `Exited` (`session::core`,
//! `watch_run`). The tray uses that split rather than inventing a second one
//! from exit codes, or "failed" would mean two things in one app.

use crate::config::SessionConfig;
use crate::session::core::SessionError;
use crate::session::runtime::SessionRuntime;
use crate::session::state::SessionStatus;

/// Sessions "Restart Failed" asks Session Core to restart.
pub fn restart_targets(snapshots: &[SessionRuntime]) -> Vec<String> {
    targets(snapshots, |status| status == SessionStatus::Error)
}

/// Sessions "Stop All" asks Session Core to stop.
///
/// `Running` only, and deliberately: `Stopping` is already on its way out, and
/// the state machine refuses a stop from `Starting` (spec §5). Asking anyway
/// would produce a refusal per session that the tray would then have to
/// explain, which is noise standing in for a rule.
///
/// This is also what `stop` accepts, so the list is exactly the set of calls
/// that can land.
pub fn stop_targets(snapshots: &[SessionRuntime]) -> Vec<String> {
    targets(snapshots, |status| status == SessionStatus::Running)
}

fn targets(snapshots: &[SessionRuntime], wanted: impl Fn(SessionStatus) -> bool) -> Vec<String> {
    snapshots
        .iter()
        .filter(|runtime| wanted(runtime.status))
        .map(|runtime| runtime.session_id.clone())
        .collect()
}

/// What leaving the Hub would cost right now.
///
/// Holds session **ids**, because it is read by logic — whether there is a
/// question to ask, and whether anything is still in the way — and only the
/// messages turn them into the names the user was shown. A plan is also a
/// reading with a shelf life: it answers "what did the registry look like when
/// this was asked", which is why Exit asks again after the user answers
/// (`tray::clear_for_exit`) rather than acting on the list it started with.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExitPlan {
    /// Sessions that must be stopped before the Hub may exit.
    pub stop: Vec<String>,
    /// Sessions that cannot be interrupted, so Exit cannot proceed at all.
    pub blockers: Vec<String>,
}

impl ExitPlan {
    /// Whether anything is alive enough to be worth a question.
    pub fn needs_confirmation(&self) -> bool {
        !self.stop.is_empty()
    }

    /// The question to ask before stopping `stop` and leaving, if there is one.
    ///
    /// §11's MVP-acceptable behaviour is exactly this: "confirm 'Stop managed
    /// sessions and exit' or Cancel". Detach-and-leave-running is offered by
    /// `docs/PRODUCT_SPEC.md` §8 as an option the Hub may not fake, and the MVP
    /// has no safe way to hand a supervised tree to another owner, so it is not
    /// offered here.
    pub fn confirm_prompt(&self, configs: &[SessionConfig]) -> Option<String> {
        if !self.needs_confirmation() {
            return None;
        }
        Some(format!(
            "还有 {} 个受管会话在运行：\n\n{}\n\n停止它们并退出？",
            self.stop.len(),
            names(configs, &self.stop).join("\n")
        ))
    }

    /// The message explaining why Exit cannot even ask its question yet.
    ///
    /// `Starting` and `Stopping` cannot be interrupted (spec §5), so there is
    /// no honest "stop them and exit" to offer: the Hub would either have to
    /// wait without saying so, or kill a process mid-launch. It says what it
    /// is waiting for instead — and says it for however many there are, since
    /// the list is not capped at one.
    pub fn blocked_message(&self, configs: &[SessionConfig]) -> Option<String> {
        if self.blockers.is_empty() {
            return None;
        }
        Some(format!(
            "无法退出：{}。\n\n正在启动或停止中，等稳定后再试一次。",
            names(configs, &self.blockers).join("、")
        ))
    }
}

/// Plan what Exit has to do, from the current snapshots.
pub fn exit_plan(snapshots: &[SessionRuntime]) -> ExitPlan {
    ExitPlan {
        stop: stop_targets(snapshots),
        blockers: targets(snapshots, |status| {
            matches!(status, SessionStatus::Starting | SessionStatus::Stopping)
        }),
    }
}

/// The message shown when a confirmed exit could not stop everything.
///
/// The Hub stays open in this case. Stopping is the whole of what the user
/// agreed to, and leaving with a run still held — inside the job object that
/// dies with this process — is the silent kill §11 forbids.
///
/// Takes Session Core's own failures rather than a message string per session:
/// the failure already knows which session it is about, and naming it a second
/// time is one more place the two could disagree.
pub fn stop_failure_message(configs: &[SessionConfig], failures: &[SessionError]) -> String {
    let lines: Vec<String> = failures
        .iter()
        .map(|failure| {
            format!(
                "{}：{}",
                super::model::display_name(configs, &failure.session_id),
                failure.message
            )
        })
        .collect();
    format!("已取消退出：以下会话未能停止。\n\n{}", lines.join("\n\n"))
}

/// The display names of `session_ids`, for a message the user reads.
///
/// The same lookup the menu rows use ([`super::model::display_name`]), so a
/// confirmation names the sessions the way the tray the user just clicked did.
fn names(configs: &[SessionConfig], session_ids: &[String]) -> Vec<String> {
    session_ids
        .iter()
        .map(|session_id| super::model::display_name(configs, session_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EffectiveLogMode, EffectiveLogging, LogSource};

    fn snapshot(session_id: &str, status: SessionStatus) -> SessionRuntime {
        let mut runtime = SessionRuntime::stopped(
            session_id,
            EffectiveLogging {
                mode: EffectiveLogMode::Off,
                source: LogSource::Captured,
                external_path: None,
            },
        );
        runtime.status = status;
        runtime
    }

    /// A config whose display name is deliberately not its id, so a test fails
    /// if a message prints the id where it should print the name.
    fn config(id: &str, name: &str) -> SessionConfig {
        SessionConfig {
            id: id.to_owned(),
            name: name.to_owned(),
            session_type: crate::config::SessionType::Service,
            cwd: None,
            command: None,
            url: None,
            port: None,
            purpose: None,
            close_impact: None,
            shell: None,
            initial_command: None,
            logging: EffectiveLogging {
                mode: EffectiveLogMode::Off,
                source: LogSource::Captured,
                external_path: None,
            },
        }
    }

    fn failure(session_id: &str, message: &str) -> SessionError {
        SessionError::failed(session_id, "stop", message, None)
    }

    /// One session in every lifecycle state, each named after its state, so a
    /// selection rule can be read off the result.
    fn every_status() -> Vec<SessionRuntime> {
        vec![
            snapshot("stopped", SessionStatus::Stopped),
            snapshot("starting", SessionStatus::Starting),
            snapshot("running", SessionStatus::Running),
            snapshot("stopping", SessionStatus::Stopping),
            snapshot("exited", SessionStatus::Exited),
            snapshot("error", SessionStatus::Error),
        ]
    }

    /// Failed means `Error` and nothing else: a clean `Exited` is a run that
    /// finished, and restarting it would be the tray inventing a problem.
    #[test]
    fn restart_failed_targets_error_sessions_only() {
        assert_eq!(restart_targets(&every_status()), vec!["error"]);
    }

    /// `stop` is refused from every state but `Running` (spec §5), so the tray
    /// must not ask from any other one.
    #[test]
    fn stop_all_targets_running_sessions_only() {
        assert_eq!(stop_targets(&every_status()), vec!["running"]);
    }

    #[test]
    fn an_idle_hub_exits_without_a_question() {
        let plan = exit_plan(&[
            snapshot("a", SessionStatus::Stopped),
            snapshot("b", SessionStatus::Exited),
        ]);

        assert!(!plan.needs_confirmation());
        assert_eq!(plan.confirm_prompt(&[]), None);
        assert_eq!(plan.blocked_message(&[]), None);
        assert!(plan.stop.is_empty());
    }

    /// §11: "If managed sessions are active, Exit must not silently destroy
    /// them." A running session is what "active" means here.
    #[test]
    fn a_running_session_makes_exit_ask_first() {
        let plan = exit_plan(&[snapshot("comfyui", SessionStatus::Running)]);
        let configs = [config("comfyui", "ComfyUI")];

        assert!(plan.needs_confirmation());
        let prompt = plan.confirm_prompt(&configs).expect("a question is owed");
        assert!(prompt.contains("ComfyUI"), "{prompt}");
        assert!(prompt.contains('1'), "{prompt}");
        assert!(plan.blocked_message(&configs).is_none());
    }

    /// The question is asked about the tray the user clicked, so it has to name
    /// sessions the way the tray's rows did — by the configured name.
    #[test]
    fn the_question_names_sessions_the_way_the_menu_does() {
        let plan = exit_plan(&[snapshot("svc-1", SessionStatus::Running)]);

        let prompt = plan
            .confirm_prompt(&[config("svc-1", "SillyTavern")])
            .expect("a question is owed");
        assert!(prompt.contains("SillyTavern"), "{prompt}");
        assert!(
            !prompt.contains("svc-1"),
            "the id is not what the user was shown: {prompt}"
        );
    }

    /// A session with no config behind it is still named, by the only thing
    /// left to name it with.
    #[test]
    fn a_session_without_a_config_is_named_by_its_id() {
        let plan = exit_plan(&[snapshot("orphan", SessionStatus::Running)]);

        let prompt = plan.confirm_prompt(&[]).expect("a question is owed");
        assert!(prompt.contains("orphan"), "{prompt}");
    }

    /// A session mid-flight cannot be stopped, so Exit must not promise to —
    /// and must not offer a question whose "yes" it cannot honour.
    #[test]
    fn a_session_mid_flight_blocks_exit_before_any_question() {
        for status in [SessionStatus::Starting, SessionStatus::Stopping] {
            let plan = exit_plan(&[snapshot("slow", status)]);

            assert!(
                !plan.needs_confirmation(),
                "{status:?} is not a stop target"
            );
            let message = plan
                .blocked_message(&[])
                .expect("being unable to exit must be said");
            assert!(message.contains("slow"), "{message}");
        }
    }

    /// The blocked message has to read for one blocker and for several: the
    /// list is not capped, so a sentence written for one would be wrong the
    /// moment two services are starting at once.
    #[test]
    fn the_blocked_message_reads_for_any_number_of_sessions() {
        let plan = exit_plan(&[
            snapshot("one", SessionStatus::Starting),
            snapshot("two", SessionStatus::Stopping),
        ]);

        let message = plan.blocked_message(&[]).expect("the block must be said");
        assert!(message.contains("one、two"), "{message}");
        assert!(!message.contains("这一个"), "{message}");
    }

    /// Both can be true at once, and the block wins: the question would be a
    /// promise the Hub cannot keep.
    #[test]
    fn a_blocked_exit_is_reported_even_when_another_session_could_be_stopped() {
        let plan = exit_plan(&[
            snapshot("running", SessionStatus::Running),
            snapshot("starting", SessionStatus::Starting),
        ]);

        assert!(plan.needs_confirmation());
        assert!(plan.blocked_message(&[]).is_some());
    }

    /// A confirmed exit that could not stop everything says so, and names the
    /// sessions — the user has to know which ones are still holding a process.
    #[test]
    fn a_failed_stop_names_the_sessions_that_survived_it() {
        let message = stop_failure_message(
            &[config("comfyui", "ComfyUI"), config("api", "API")],
            &[
                failure("comfyui", "the tree is still alive"),
                failure("api", "access denied"),
            ],
        );

        assert!(message.contains("ComfyUI"), "{message}");
        assert!(message.contains("still alive"), "{message}");
        assert!(message.contains("API"), "{message}");
        assert!(message.contains("access denied"), "{message}");
    }
}
