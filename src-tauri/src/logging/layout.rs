//! Walking the Hub's own log layout (`docs/LOGGING.md` §5).
//!
//! Two readers walk `<root>/<session_id>/<YYYY-MM>/` and differ only in which
//! files they want: the run history wants the metadata, retention wants the
//! logs. Keeping one walk here means the two cannot drift apart — a reader that
//! found a folder the writer does not use, or missed one it does, would show a
//! user an incomplete history or leave logs uncleaned.
//!
//! The walk is deliberately shallow and unfollowing: it reads one level of
//! session folders and one level of month folders, which is the whole layout
//! (`docs/DECISIONS.md` D-011 — the layout *is* the index). Nothing deeper is
//! the Hub's, and a folder a person created by hand under a session simply
//! contributes whatever run files it holds.

use std::path::{Path, PathBuf};

/// One file found under a session's month folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFile {
    /// The session folder the file was found under.
    pub session_id: String,
    pub path: PathBuf,
}

/// Every `.<extension>` file under `<root>/<session>/<month>/`, optionally for
/// one session only.
///
/// A missing root, unreadable folder or unreadable entry contributes nothing
/// rather than failing the walk: a session that has never run has no folder,
/// and one damaged entry must not hide the rest of a history (the same rule T01
/// applies to config entries).
pub fn session_run_files(root: &Path, extension: &str, only_session: Option<&str>) -> Vec<RunFile> {
    let mut found = Vec::new();
    let Ok(sessions) = std::fs::read_dir(root) else {
        return found;
    };

    for session in sessions.flatten() {
        let session_path = session.path();
        if !session_path.is_dir() {
            continue;
        }
        let Some(session_id) = session_path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if only_session.is_some_and(|wanted| wanted != session_id) {
            continue;
        }

        let Ok(months) = std::fs::read_dir(&session_path) else {
            continue;
        };
        for month in months.flatten() {
            let month_path = month.path();
            if !month_path.is_dir() {
                continue;
            }
            let Ok(entries) = std::fs::read_dir(&month_path) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if !has_extension(&path, extension) {
                    continue;
                }
                found.push(RunFile {
                    session_id: session_id.to_owned(),
                    path,
                });
            }
        }
    }
    found
}

fn has_extension(path: &Path, extension: &str) -> bool {
    matches!(path.extension(), Some(found) if found == extension)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logging::test_support::TempDir;
    use std::fs;

    fn month(root: &Path, session: &str, month: &str, file: &str) -> PathBuf {
        let dir = root.join(session).join(month);
        fs::create_dir_all(&dir).expect("the month folder is creatable");
        let path = dir.join(file);
        fs::write(&path, b"x").expect("the file is writable");
        path
    }

    #[test]
    fn the_walk_finds_the_files_of_every_session() {
        let dir = TempDir::new();
        let first = month(dir.path(), "comfyui", "2026-09", "run-a.log");
        let second = month(dir.path(), "comfyui", "2026-10", "run-b.log");
        let other = month(dir.path(), "sillytavern", "2026-09", "run-c.log");

        let mut found: Vec<PathBuf> = session_run_files(dir.path(), "log", None)
            .into_iter()
            .map(|file| file.path)
            .collect();
        found.sort();

        assert_eq!(found, vec![first, second, other]);
    }

    #[test]
    fn the_walk_can_be_narrowed_to_one_session() {
        let dir = TempDir::new();
        month(dir.path(), "comfyui", "2026-09", "run-a.log");
        month(dir.path(), "sillytavern", "2026-09", "run-b.log");

        let found = session_run_files(dir.path(), "log", Some("comfyui"));

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].session_id, "comfyui");
    }

    /// The extension is what tells a log from the metadata describing it, and
    /// neither reader may pick up the other's files.
    #[test]
    fn the_walk_only_reports_the_extension_it_was_asked_for() {
        let dir = TempDir::new();
        month(dir.path(), "comfyui", "2026-09", "run.json");
        let log = month(dir.path(), "comfyui", "2026-09", "run.log");

        let found = session_run_files(dir.path(), "log", None);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, log);
    }

    #[test]
    fn a_missing_root_is_an_empty_walk() {
        let dir = TempDir::new();
        assert!(session_run_files(&dir.join("never-used"), "log", None).is_empty());
    }
}
