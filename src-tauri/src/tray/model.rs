//! What the tray shows right now, as plain data
//! (`docs/MVP_IMPLEMENTATION_SPEC.md` §11, `docs/UI_STYLE_GUIDE.md` §9).
//!
//! The tray reads Session Core; it never keeps a lifecycle of its own. This
//! module derives one model from the same snapshots the window renders, which
//! is what makes "the tray can never disagree with the window" (spec §3) a
//! property of the data flow rather than a promise about two code paths.
//!
//! Everything here is a pure function of configs and snapshots. That is the
//! seam the tray's tests use: what the user sees is asserted as a value, with
//! no app, no window and no tray icon.

use crate::config::SessionConfig;
use crate::session::event::AppSummary;
use crate::session::runtime::SessionRuntime;
use crate::session::state::SessionStatus;

/// How many session rows the tray lists before it stops listing them.
///
/// §9 asks for a *compact* surface, and a config file can name twenty
/// sessions; an unbounded menu would be a worse copy of the sidebar than no
/// list at all. The cap is on the rows only — the summary line still counts
/// every session, so nothing is silently missing from the numbers.
pub const MAX_ROWS: usize = 8;

/// One session line in the tray menu.
///
/// Carries the id even though the label already names the session: the id is
/// what the click reports, and two sessions may share a display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayRow {
    pub session_id: String,
    /// The configured name, or the id when no config answers for it.
    pub name: String,
    pub status: SessionStatus,
}

impl TrayRow {
    /// The text the menu renders for this row.
    pub fn label(&self) -> String {
        format!("{} · {}", self.name, status_word(self.status))
    }
}

/// The whole tray surface at one instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayModel {
    /// The disabled header line, e.g. `2 运行 · 1 失败`.
    pub summary: String,
    /// The same reading plus the app identity, for the icon's hover text.
    pub tooltip: String,
    /// Running and failed sessions, in Session Core's id order.
    pub rows: Vec<TrayRow>,
    /// How many rows [`MAX_ROWS`] left out.
    pub hidden_rows: usize,
    pub running: usize,
    pub failed: usize,
}

/// Build the tray's model from the registry's configs and snapshots.
///
/// Both lists come from Session Core and describe the same sessions; they are
/// joined by id rather than by position, because nothing promises the same
/// order (the same join the window's view model does). A snapshot whose config
/// is missing is still listed under its id: the session exists and is doing
/// something, and dropping it would make the tray's sum disagree with the
/// process it is describing.
///
/// The counts are [`AppSummary::of`]'s, not a second tally over the same
/// snapshots, so "how many are running" has one answer in this program.
pub fn build(configs: &[SessionConfig], snapshots: &[SessionRuntime]) -> TrayModel {
    let summary_counts = AppSummary::of(snapshots);
    let rows: Vec<TrayRow> = snapshots
        .iter()
        .filter(|snapshot| {
            // §9's list is "running or failed sessions"; a stopped, starting,
            // stopping or cleanly exited one is none of those.
            matches!(
                snapshot.status,
                SessionStatus::Running | SessionStatus::Error
            )
        })
        .map(|snapshot| TrayRow {
            name: display_name(configs, &snapshot.session_id),
            session_id: snapshot.session_id.clone(),
            status: snapshot.status,
        })
        .collect();

    let (running, failed) = (summary_counts.running, summary_counts.error);
    let summary = summary_text(running, failed);
    let hidden_rows = rows.len().saturating_sub(MAX_ROWS);

    TrayModel {
        tooltip: format!("{} — {summary}", crate::ipc::APP_NAME),
        summary,
        rows: rows.into_iter().take(MAX_ROWS).collect(),
        hidden_rows,
        running,
        failed,
    }
}

/// The name a session is shown under, or its id when no config answers for it.
///
/// The one place this lookup happens, so every surface in the tray — a menu
/// row, a confirmation that names what it is about to stop — calls the session
/// the same thing the user configured it as.
pub fn display_name(configs: &[SessionConfig], session_id: &str) -> String {
    configs
        .iter()
        .find(|config| config.id == session_id)
        .map(|config| config.name.clone())
        .unwrap_or_else(|| session_id.to_owned())
}

/// The counting half of the summary, e.g. `2 运行 · 1 失败`.
///
/// Deliberately not "2 running (1 busy)": §11's example names `busy`, and the
/// MVP has no source for it — a port says a service is *reachable*, not that
/// it is *free* (D-023 item 5). The count this surface can answer honestly is
/// how many runs have failed, so that is the one it shows.
pub fn summary_text(running: usize, failed: usize) -> String {
    match (running, failed) {
        (0, 0) => "没有正在运行的会话".to_owned(),
        (0, failed) => format!("{failed} 失败"),
        (running, 0) => format!("{running} 运行"),
        (running, failed) => format!("{running} 运行 · {failed} 失败"),
    }
}

/// The word the menu renders for a lifecycle state.
///
/// The same vocabulary the window's status badges use
/// (`src/state/derivations.ts`), so a session cannot be "Running" in the
/// sidebar and something else in the tray. Every state is named, not just the
/// two that reach a row: a row type that could render a blank for a state
/// nobody expected is a row that hides a session.
fn status_word(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::Stopped => "Stopped",
        SessionStatus::Starting => "Starting",
        SessionStatus::Running => "Running",
        SessionStatus::Stopping => "Stopping",
        SessionStatus::Exited => "Exited",
        SessionStatus::Error => "Error",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EffectiveLogMode, EffectiveLogging, LogSource};
    use crate::session::state::ALL_STATUSES;

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

    fn snapshot(id: &str, status: SessionStatus) -> SessionRuntime {
        let mut runtime = SessionRuntime::stopped(
            id,
            EffectiveLogging {
                mode: EffectiveLogMode::Off,
                source: LogSource::Captured,
                external_path: None,
            },
        );
        runtime.status = status;
        runtime
    }

    /// A `Starting`/`Stopping` count must not appear anywhere in the summary:
    /// the tray shows what is running and what failed, and a count it has no
    /// row for would be a number the user cannot resolve.
    #[test]
    fn the_summary_counts_running_and_failed_sessions() {
        assert_eq!(summary_text(3, 0), "3 运行");
        assert_eq!(summary_text(3, 1), "3 运行 · 1 失败");
        assert_eq!(summary_text(0, 1), "1 失败");
        assert_eq!(summary_text(0, 0), "没有正在运行的会话");
    }

    /// §11's example is `3 running (1 busy)`; the model must not claim a
    /// `busy` count it has no source for (D-023 item 5).
    #[test]
    fn the_summary_never_invents_a_busy_count() {
        let model = build(
            &[config("a", "A")],
            &[snapshot("a", SessionStatus::Running)],
        );
        assert!(!model.summary.contains("busy"), "{}", model.summary);
        assert!(!model.summary.contains("忙碌"), "{}", model.summary);
    }

    /// §9: "running or failed sessions". Everything else is the window's job.
    #[test]
    fn only_running_and_failed_sessions_are_listed() {
        let snapshots: Vec<SessionRuntime> = ALL_STATUSES
            .iter()
            .map(|status| snapshot("s", *status))
            .collect();
        let configs = vec![config("s", "Session")];

        let model = build(&configs, &snapshots);

        let listed: Vec<SessionStatus> = model.rows.iter().map(|row| row.status).collect();
        assert_eq!(
            listed,
            vec![SessionStatus::Running, SessionStatus::Error],
            "a stopped, starting, stopping or exited session must not take a row"
        );
        assert_eq!(model.running, 1);
        assert_eq!(model.failed, 1);
    }

    /// The row is what a click reports back, so the id has to survive the join
    /// with the config — the name is only the label.
    #[test]
    fn a_row_carries_its_session_id_and_the_configured_name() {
        let model = build(
            &[config("comfyui", "ComfyUI")],
            &[snapshot("comfyui", SessionStatus::Running)],
        );

        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].session_id, "comfyui");
        assert_eq!(model.rows[0].label(), "ComfyUI · Running");
        assert_eq!(model.tooltip, format!("{} — 1 运行", crate::ipc::APP_NAME));
    }

    /// A snapshot with no config behind it still describes something that is
    /// doing work. Hiding it would make the summary count a session the menu
    /// cannot show.
    #[test]
    fn a_session_without_a_config_is_still_listed_under_its_id() {
        let model = build(&[], &[snapshot("orphan", SessionStatus::Error)]);

        assert_eq!(model.rows.len(), 1);
        assert_eq!(model.rows[0].session_id, "orphan");
        assert_eq!(model.rows[0].label(), "orphan · Error");
        assert_eq!(model.failed, 1);
    }

    /// The cap is a display limit, never a counting limit (spec §14's "compact"
    /// is about the surface, and a hidden session must still be counted).
    #[test]
    fn rows_are_capped_while_the_counts_keep_every_session() {
        let snapshots: Vec<SessionRuntime> = (0..MAX_ROWS + 3)
            .map(|index| snapshot(&format!("s{index}"), SessionStatus::Running))
            .collect();

        let model = build(&[], &snapshots);

        assert_eq!(model.rows.len(), MAX_ROWS);
        assert_eq!(model.hidden_rows, 3);
        assert_eq!(model.running, MAX_ROWS + 3);
    }

    /// An empty registry is the fresh-install state, and it must read as
    /// "nothing is running" rather than as a zero count.
    #[test]
    fn an_empty_registry_says_nothing_is_running() {
        let model = build(&[], &[]);

        assert_eq!(model.summary, "没有正在运行的会话");
        assert!(model.rows.is_empty());
        assert_eq!(model.hidden_rows, 0);
        assert_eq!((model.running, model.failed), (0, 0));
    }

    /// The model is compared to decide whether the menu needs rebuilding
    /// (`super::TraySink`), so two readings of the same state must be equal.
    #[test]
    fn the_same_state_produces_the_same_model() {
        let configs = vec![config("a", "A")];
        let snapshots = vec![snapshot("a", SessionStatus::Running)];

        assert_eq!(build(&configs, &snapshots), build(&configs, &snapshots));
    }
}
