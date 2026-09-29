//! The tray menu: a value first, a Windows menu second
//! (`docs/MVP_IMPLEMENTATION_SPEC.md` §11, `docs/UI_STYLE_GUIDE.md` §9).
//!
//! [`plan`] turns a [`TrayModel`] into the list of items the user should see,
//! and nothing else in this file decides what is on the menu — the Tauri layer
//! below it only maps those items onto `muda` objects. That split is what
//! makes the tray's contents testable as data: §11's required controls and
//! §9's "compact, not a miniature copy of the app" are asserted without a
//! running app.
//!
//! ## Why this module is the only one allowed to know item ids
//!
//! A menu item reports a click by its id and nothing else, so the ids *are*
//! the tray's action vocabulary. Keeping them here, next to the labels and the
//! enable rules they belong to, means an id cannot drift from the item that
//! emits it.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Runtime};

use super::model::TrayModel;

/// The ids the tray dispatches on (`super::on_menu_event`).
pub mod ids {
    /// Bring the main window back.
    pub const SHOW_WINDOW: &str = "tray.show-window";
    /// Restart every session in `Error`.
    pub const RESTART_FAILED: &str = "tray.restart-failed";
    /// Stop every running session.
    pub const STOP_ALL: &str = "tray.stop-all";
    /// Leave the Hub, after asking when it would cost a run.
    pub const EXIT: &str = "tray.exit";
    /// Prefix of a session row's id: `tray.session.<session id>`.
    pub const SESSION_PREFIX: &str = "tray.session.";
    /// The disabled summary line. Never dispatched on.
    pub const SUMMARY: &str = "tray.summary";
    /// The disabled "and N more" line. Never dispatched on.
    pub const MORE: &str = "tray.more";
}

/// The four controls §11 names, in the app's own vocabulary.
///
/// §11 names them functionally — Show Window, Restart Failed, Stop All, Exit —
/// and the tracker's acceptance criteria are written in those terms; these are
/// the words the user reads. They are Chinese because every other control in
/// the app is (`src/state/actions.ts`), and a tray whose one Chinese row sits
/// under three English ones would be internally inconsistent in exactly the
/// way `docs/UI_STYLE_GUIDE.md` §10 rules out.
pub const SHOW_WINDOW_LABEL: &str = "显示主窗口";
pub const RESTART_FAILED_LABEL: &str = "重启失败的会话";
pub const STOP_ALL_LABEL: &str = "停止全部";
pub const EXIT_LABEL: &str = "退出";

/// One item of the planned menu, as data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayItem {
    /// A line that states something and cannot be clicked.
    Reading {
        id: &'static str,
        text: String,
    },
    /// A session row; clicking it opens the window on that session.
    Session {
        id: String,
        text: String,
    },
    Separator,
    /// Something the user can ask for. A control whose precondition is not
    /// met is rendered disabled rather than removed: a control that
    /// disappears is one the user has to go looking for.
    Command {
        id: &'static str,
        text: &'static str,
        enabled: bool,
    },
}

/// The menu for `model`, top to bottom.
///
/// The shape is §9's list in the order §9 gives it: the summary, the
/// running/failed sessions, then the four controls, with Exit kept apart
/// because it is the one destructive entry that ends the app itself.
pub fn plan(model: &TrayModel) -> Vec<TrayItem> {
    let mut items = vec![TrayItem::Reading {
        id: ids::SUMMARY,
        text: model.summary.clone(),
    }];

    if !model.rows.is_empty() {
        items.push(TrayItem::Separator);
        for row in &model.rows {
            items.push(TrayItem::Session {
                id: format!("{}{}", ids::SESSION_PREFIX, row.session_id),
                text: row.label(),
            });
        }
        if model.hidden_rows > 0 {
            items.push(TrayItem::Reading {
                id: ids::MORE,
                text: format!("…还有 {} 个会话", model.hidden_rows),
            });
        }
    }

    items.push(TrayItem::Separator);
    items.push(TrayItem::Command {
        id: ids::SHOW_WINDOW,
        text: SHOW_WINDOW_LABEL,
        // Always offered: the window is the only place the rest of the app
        // lives, and it is the one control that is never wrong to offer.
        enabled: true,
    });
    items.push(TrayItem::Command {
        id: ids::RESTART_FAILED,
        text: RESTART_FAILED_LABEL,
        enabled: model.failed > 0,
    });
    items.push(TrayItem::Command {
        id: ids::STOP_ALL,
        text: STOP_ALL_LABEL,
        enabled: model.running > 0,
    });
    items.push(TrayItem::Separator);
    items.push(TrayItem::Command {
        id: ids::EXIT,
        text: EXIT_LABEL,
        enabled: true,
    });

    items
}

/// Build the native menu for `model`.
///
/// Menu items are created and appended one at a time and their handles are
/// dropped at the end of the loop. That is safe, and deliberately so: a muda
/// leaf item owns no container of its own, so dropping the handle removes
/// nothing from the menu it was appended to — only its (unused here)
/// JavaScript channel registration goes away.
///
/// Every call below proxies to the main thread when it is made from another
/// one, which is why this may be called from the tray's refresh path.
pub fn build<R: Runtime>(app: &AppHandle<R>, model: &TrayModel) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    for item in plan(model) {
        match item {
            TrayItem::Reading { id, text } => {
                menu.append(&MenuItem::with_id(app, id, text, false, None::<&str>)?)?;
            }
            TrayItem::Session { id, text } => {
                menu.append(&MenuItem::with_id(app, id, text, true, None::<&str>)?)?;
            }
            TrayItem::Separator => {
                menu.append(&PredefinedMenuItem::separator(app)?)?;
            }
            TrayItem::Command { id, text, enabled } => {
                menu.append(&MenuItem::with_id(app, id, text, enabled, None::<&str>)?)?;
            }
        }
    }
    Ok(menu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::state::SessionStatus;

    fn model(running: usize, failed: usize) -> TrayModel {
        let mut rows: Vec<super::super::model::TrayRow> = Vec::new();
        for index in 0..running {
            rows.push(super::super::model::TrayRow {
                session_id: format!("run{index}"),
                name: format!("Run {index}"),
                status: SessionStatus::Running,
            });
        }
        for index in 0..failed {
            rows.push(super::super::model::TrayRow {
                session_id: format!("bad{index}"),
                name: format!("Bad {index}"),
                status: SessionStatus::Error,
            });
        }
        TrayModel {
            summary: super::super::model::summary_text(running, failed),
            tooltip: "Local Console Hub".to_owned(),
            hidden_rows: 0,
            rows,
            running,
            failed,
        }
    }

    fn command(items: &[TrayItem], id: &str) -> Option<(bool, &'static str)> {
        items.iter().find_map(|item| match item {
            TrayItem::Command {
                id: found,
                text,
                enabled,
            } if *found == id => Some((*enabled, *text)),
            _ => None,
        })
    }

    fn ids_of(items: &[TrayItem]) -> Vec<&str> {
        items
            .iter()
            .filter_map(|item| match item {
                TrayItem::Reading { id, .. } => Some(*id),
                TrayItem::Session { id, .. } => Some(id.as_str()),
                TrayItem::Command { id, .. } => Some(*id),
                TrayItem::Separator => None,
            })
            .collect()
    }

    /// §11's list, and its order: summary, sessions, Show Window, Restart
    /// Failed, Stop All, Exit.
    #[test]
    fn the_menu_carries_every_control_the_spec_names() {
        let items = plan(&model(1, 1));

        assert!(command(&items, ids::SHOW_WINDOW).is_some());
        assert!(command(&items, ids::RESTART_FAILED).is_some());
        assert!(command(&items, ids::STOP_ALL).is_some());
        assert!(command(&items, ids::EXIT).is_some());
    }

    /// The controls that can be asked for when there is nothing to act on must
    /// still be listed — disabled, not missing.
    #[test]
    fn the_action_controls_are_disabled_rather_than_absent_when_idle() {
        let items = plan(&model(0, 0));

        assert_eq!(
            command(&items, ids::RESTART_FAILED),
            Some((false, RESTART_FAILED_LABEL))
        );
        assert_eq!(
            command(&items, ids::STOP_ALL),
            Some((false, STOP_ALL_LABEL))
        );
        assert_eq!(command(&items, ids::EXIT), Some((true, EXIT_LABEL)));
        assert_eq!(
            command(&items, ids::SHOW_WINDOW),
            Some((true, SHOW_WINDOW_LABEL))
        );
    }

    /// Each control is enabled exactly by its own precondition, so a click can
    /// never ask Session Core for something the state machine will refuse.
    #[test]
    fn each_control_is_enabled_by_its_own_precondition() {
        let running = plan(&model(2, 0));
        assert_eq!(
            command(&running, ids::STOP_ALL),
            Some((true, STOP_ALL_LABEL))
        );
        assert_eq!(
            command(&running, ids::RESTART_FAILED),
            Some((false, RESTART_FAILED_LABEL))
        );

        let failed = plan(&model(0, 2));
        assert_eq!(
            command(&failed, ids::RESTART_FAILED),
            Some((true, RESTART_FAILED_LABEL))
        );
        assert_eq!(
            command(&failed, ids::STOP_ALL),
            Some((false, STOP_ALL_LABEL))
        );
    }

    /// A session row reports its session id, which is what the click needs.
    #[test]
    fn a_session_row_is_addressed_by_its_session_id() {
        let items = plan(&model(1, 0));

        let row = items
            .iter()
            .find_map(|item| match item {
                TrayItem::Session { id, text } => Some((id.clone(), text.clone())),
                _ => None,
            })
            .expect("the running session takes a row");

        assert_eq!(row.0, "tray.session.run0");
        assert_eq!(row.1, "Run 0 · Running");
    }

    /// The summary is the menu's first line, and it is not clickable.
    #[test]
    fn the_summary_leads_and_states_rather_than_acts() {
        let items = plan(&model(2, 1));

        assert_eq!(
            items.first(),
            Some(&TrayItem::Reading {
                id: ids::SUMMARY,
                text: "2 运行 · 1 失败".to_owned(),
            })
        );
    }

    /// Elided sessions must be accounted for on the menu, or the list would
    /// simply be wrong about what exists.
    #[test]
    fn elided_rows_are_reported() {
        let mut capped = model(2, 0);
        capped.hidden_rows = 5;

        let items = plan(&capped);

        assert!(items.iter().any(|item| matches!(
            item,
            TrayItem::Reading { id: ids::MORE, text } if text == "…还有 5 个会话"
        )));
    }

    /// Windows draws two separators in a row as a thicker gap, and a leading
    /// or trailing one as a stray line. The plan must never produce either.
    #[test]
    fn separators_never_double_up_or_sit_at_the_edges() {
        for (running, failed) in [(0, 0), (1, 0), (0, 1), (3, 2)] {
            let items = plan(&model(running, failed));
            assert!(
                !matches!(items.first(), Some(TrayItem::Separator)),
                "leading separator with {running}/{failed}: {items:?}"
            );
            assert!(
                !matches!(items.last(), Some(TrayItem::Separator)),
                "trailing separator with {running}/{failed}: {items:?}"
            );
            for pair in items.windows(2) {
                assert!(
                    !matches!(pair, [TrayItem::Separator, TrayItem::Separator]),
                    "double separator with {running}/{failed}: {items:?}"
                );
            }
        }
    }

    /// Two menu items sharing an id is not a cosmetic problem: the click would
    /// be ambiguous. This is the invariant the id scheme has to keep.
    #[test]
    fn menu_item_ids_are_unique() {
        let items = plan(&model(4, 2));
        let ids = ids_of(&items);

        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate ids in {ids:?}");
    }
}
