//! Saving a terminal's launch configuration (#65, spec #59 decisions 4 and 6).
//!
//! "新建 PowerShell" makes a terminal that is deliberately not in any file: it
//! goes away when the app exits, which is what makes it safe to open one for a
//! quick command (story 23). This is the other half of that decision — the
//! gesture that *keeps* one, when the directory and shell turn out to be worth
//! reusing (story 24).
//!
//! ## What is saved, and what is deliberately not
//!
//! The launch method: the shell the terminal resolved, the directory it opened
//! in, and the name the user gives the row. Nothing else — not the scrollback,
//! not the run's record, and above all not a word of what the user typed
//! (story 25). A saved terminal is not a recording of a session; it is the
//! recipe for starting one, which is why `initial_command` is left unset: a
//! command there would be replayed on every future start, and a shell that
//! repeats yesterday's commands is a worse bug than any convenience it buys.
//!
//! The `logging:` block is left out too, so the config layer's own default for
//! an interactive terminal applies — `off`/`none` (`docs/LOGGING.md` §3). A
//! saved terminal persists no more than a temporary one did (decision 16).
//!
//! ## Why the session keeps its id and its run
//!
//! The save happens to a terminal that is usually *running*, and a terminal is
//! a live PTY: its id is what the window's attachment, the scrollback and the
//! process handle are all keyed by. So this operation writes a configuration
//! and changes what the session *is*; it never restarts it, copies it, or
//! re-keys it (decision 6 — 保存不为已运行终端额外创建一个重复进程). The
//! identity a saved terminal gets is the one it already had, and the stable
//! part a user sees is the name they chose.
//!
//! ## Order of the two writes
//!
//! The file first, then the registry. The refusal that matters most is the
//! file's: a config file that is broken, concurrently edited or not writable
//! must leave the session exactly as it was — still temporary, still running,
//! still removable after it ends — so nothing is marked saved until the file
//! actually holds the entry. Only then is the save announced, because until
//! then "saved" would be a claim a restart would not keep (decision 15).

use std::path::Path;

use serde::Deserialize;

use crate::config::{save_session, session_ids, validate_entry, RawSessionConfig, SessionConfig};
use crate::session::core::{CreatedSession, SessionCore};

use super::form::FormError;

/// The saved fields a user supplies for a terminal (#65).
///
/// One field is required, and it is the one the Hub cannot know: what this
/// terminal should be *called* the next time the user sees it. The launch
/// method is not on this form at all — it is read from the terminal being
/// saved, which is the whole difference between this entry and "添加应用".
/// `purpose` and `close_impact` are the optional words D-027 gives both
/// session types.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveTerminal {
    pub name: String,
    /// What this terminal is for; free text (D-027).
    pub purpose: Option<String>,
    /// What closing it costs the user; free text (D-027).
    pub close_impact: Option<String>,
}

/// Save the launch configuration of the terminal `session_id`, and mark that
/// session as a saved one.
///
/// The answer is the same pair a quick entry answers with — the configuration
/// and the state it really has — because the two halves are what the window
/// needs to render the row it just saved, and the runtime is there to prove the
/// run in the answer is the run that was already going.
pub fn save_terminal_config(
    core: &SessionCore,
    config_file: Option<&Path>,
    session_id: &str,
    form: SaveTerminal,
) -> Result<CreatedSession, FormError> {
    let Some(config_file) = config_file else {
        return Err(FormError::new(
            None,
            "无法确定配置文件位置；请检查系统应用数据目录后重启应用。",
        ));
    };

    let entry = core.session_entry(session_id).ok_or_else(|| {
        FormError::new(
            None,
            format!("会话列表里已经没有 `{session_id}` 了；它可能刚被移除。"),
        )
    })?;
    if !entry.temporary {
        return Err(FormError::new(
            None,
            format!(
                "「{}」已经是保存过的配置，不需要再保存；要改名请直接改配置文件。",
                entry.config.name
            ),
        ));
    }

    let raw = saved_entry(&entry.config, &form)?;
    // The entry takes the position after the last one the file defines, which
    // is the number its validation messages are indexed by (0 is file-level).
    let config = validate_entry(file_entries(config_file)? + 1, &raw)
        .map_err(|error| FormError::new(error.field.as_deref(), error.message))?;

    // The file first. Every refusal above and below this line has to leave the
    // terminal usable, and the one that can still be refused *is* this write.
    save_session(config_file, &raw)
        .map_err(|error| FormError::new(None, error.message(config_file)))?;

    let entry = core.mark_saved(session_id, config).map_err(|error| {
        // The file now holds the entry, so this is not "the save failed" —
        // saying that would be the mirror of the lie decision 15 forbids. What
        // is true is that the saved configuration could not be applied to a
        // session that is no longer there, which the next start will not care
        // about and the user should still be told.
        FormError::new(
            None,
            format!(
                "配置已写入 `{}`，但会话列表没能更新：{}",
                config_file.display(),
                error.message
            ),
        )
    })?;

    // The runtime is read *before* the save is announced, because the answer
    // and the event have to agree: a command that failed after the window had
    // been told the row was saved would be the same contradiction decision 15
    // forbids in the other direction — the dialog would report a failure for a
    // row already wearing the saved name. Marking the session saved is what
    // makes this read infallible: a session that is no longer temporary cannot
    // be removed (`SessionCore::remove_session` refuses one), and nothing else
    // takes a session out of the registry.
    let runtime = core.snapshot(session_id).ok_or_else(|| {
        FormError::new(
            None,
            format!("会话 `{session_id}` 在保存过程中消失了；配置已写入，重启 Hub 后仍然可用。"),
        )
    })?;
    core.publish_saved(session_id);
    Ok(CreatedSession {
        config: entry.config,
        runtime,
    })
}

/// The entry to write for `config`, as the user has just described it.
///
/// `shell` and `cwd` are taken from the running session rather than asked for:
/// they are what the user already chose when the terminal opened, and a form
/// that could override them would be a second, subtly different way to add an
/// application.
fn saved_entry(config: &SessionConfig, form: &SaveTerminal) -> Result<RawSessionConfig, FormError> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(FormError::new(
            Some("name"),
            "`name` is empty — set the name this terminal is listed under",
        ));
    }
    let shell = match config.shell.as_deref().map(str::trim) {
        Some(shell) if !shell.is_empty() => shell.to_owned(),
        _ => {
            return Err(FormError::new(
                None,
                "这个会话没有可以保存的 shell，无法作为启动配置保存。",
            ))
        }
    };
    // A terminal always opened somewhere, so this is only reachable for a
    // session this build did not create — and saving *no* directory would mean
    // the next start opens in the home directory instead of the one the user
    // is looking at, which is a different terminal wearing the same name.
    let cwd = match config.cwd.as_ref() {
        Some(cwd) => cwd.to_string_lossy().into_owned(),
        None => {
            return Err(FormError::new(
                None,
                "这个会话没有可以保存的工作目录，无法作为启动配置保存。",
            ))
        }
    };

    Ok(RawSessionConfig {
        id: config.id.clone(),
        name: name.to_owned(),
        r#type: "terminal".to_owned(),
        cwd: Some(cwd),
        command: None,
        url: None,
        port: None,
        purpose: form.purpose.clone(),
        close_impact: form.close_impact.clone(),
        shell: Some(shell),
        // Never: this is the field that would replay a command (story 25).
        initial_command: None,
        // Never: the terminal default is what a saved terminal should keep, and
        // writing `logging: {}` would freeze a policy nobody chose.
        logging: None,
    })
}

/// How many entries the config file already defines.
///
/// Read for the position a new entry takes, and read leniently for the same
/// reason [`session_ids`] is: a file this build cannot parse is refused by
/// `save_session` with its own message, and a file that is not there is a fresh
/// install whose first entry is number one.
fn file_entries(config_file: &Path) -> Result<usize, FormError> {
    match std::fs::read_to_string(config_file) {
        Ok(text) => Ok(session_ids(&text).len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(FormError::new(
            None,
            format!("无法读取配置文件 `{}`：{error}", config_file.display()),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{load_from_file, ConfigFileStatusDto, EffectiveLogMode, LogSource};
    use crate::session::core::SessionCore;
    use crate::session::state::SessionStatus;
    use crate::session::temporary::{self, ShellLookup};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A config file inside a directory of this test's own, so nothing here
    /// can touch the user's real one.
    struct TempConfig {
        _directory: PathBuf,
        path: PathBuf,
    }

    impl TempConfig {
        fn with(contents: Option<&str>) -> Self {
            static SEQUENCE: AtomicU32 = AtomicU32::new(0);

            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos();
            let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir()
                .join(format!("lch-65-{}-{unique}-{sequence}", std::process::id()));
            fs::create_dir_all(&directory).expect("the directory is creatable");
            let path = directory.join("config.yaml");
            if let Some(contents) = contents {
                fs::write(&path, contents).expect("the config is writable");
            }
            TempConfig {
                _directory: directory,
                path,
            }
        }

        fn path(&self) -> &Path {
            &self.path
        }

        fn read(&self) -> String {
            fs::read_to_string(&self.path).expect("the config is readable")
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self._directory);
        }
    }

    /// Windows PowerShell, which every Windows has — the one machine fact these
    /// tests depend on, so nothing here needs PowerShell 7 to be installed.
    struct OneShell;

    impl ShellLookup for OneShell {
        fn find(&self, name: &str) -> Option<PathBuf> {
            (name == temporary::WINDOWS_POWERSHELL).then(|| {
                PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.exe")
            })
        }
    }

    /// A sink that remembers what it was told, so a test can assert on what the
    /// window would have been sent.
    #[derive(Default)]
    struct RecordingSink {
        events: std::sync::Mutex<Vec<crate::session::event::SessionEvent>>,
    }

    impl crate::session::core::EventSink for RecordingSink {
        fn publish(&self, event: crate::session::event::SessionEvent) {
            self.events
                .lock()
                .expect("no other thread poisoned the sink")
                .push(event);
        }
    }

    impl RecordingSink {
        /// The saves announced so far — the event a refusal must not produce.
        fn saves(&self) -> Vec<crate::config::SessionConfigDto> {
            self.events
                .lock()
                .expect("no other thread poisoned the sink")
                .iter()
                .filter_map(|event| match event {
                    crate::session::event::SessionEvent::Saved(saved) => Some(saved.config.clone()),
                    _ => None,
                })
                .collect()
        }
    }

    /// One running terminal in the test process's own directory.
    ///
    /// A real one, so the entry's `cwd` is a directory that exists at
    /// validation time — the saved config has to pass the same rules any
    /// hand-written one does.
    fn running_terminal(core: &SessionCore) -> CreatedSession {
        let directory = std::env::current_dir().expect("the test process has a working directory");
        let requested = directory.to_string_lossy().into_owned();
        core.create_temporary_terminal_with(Some(&requested), &OneShell, Some(directory))
            .expect("the quick entry creates a terminal")
    }

    fn form(name: &str) -> SaveTerminal {
        SaveTerminal {
            name: name.to_owned(),
            purpose: None,
            close_impact: None,
        }
    }

    /// The whole promise of the entry: the running terminal becomes a
    /// configuration the file holds, without the terminal noticing.
    #[test]
    fn a_running_terminal_is_saved_into_the_config_file_as_it_is_running() {
        let config = TempConfig::with(None);
        let sink = std::sync::Arc::new(RecordingSink::default());
        let core = SessionCore::new(sink.clone());
        let created = running_terminal(&core);
        let id = created.config.id.clone();

        let saved = save_terminal_config(&core, Some(config.path()), &id, form("项目终端"))
            .expect("a running terminal can be saved");

        assert_eq!(saved.config.id, id, "the terminal keeps its identity");
        assert_eq!(saved.config.name, "项目终端");
        assert_eq!(
            saved.runtime.run_id, created.runtime.run_id,
            "saving must not start, restart or copy the run"
        );
        assert_eq!(saved.runtime.pid, created.runtime.pid);
        assert_eq!(saved.runtime.status, SessionStatus::Running);

        let entry = core
            .entries()
            .into_iter()
            .find(|entry| entry.config.id == id)
            .expect("the saved session is still in the registry");
        assert!(!entry.temporary, "the row is now a configured one");
        assert_eq!(entry.config.name, "项目终端");
        assert_eq!(core.entries().len(), 1, "one row, not one row plus a copy");

        let loaded = load_from_file(config.path()).expect("the file loads");
        assert_eq!(loaded.file_status, ConfigFileStatusDto::Loaded);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);
        assert_eq!(loaded.sessions.len(), 1);
        let written = &loaded.sessions[0];
        assert_eq!(written.id, id);
        assert_eq!(written.name, "项目终端");
        assert_eq!(written.session_type, crate::config::SessionType::Terminal);
        assert_eq!(
            written.cwd.as_deref(),
            created.config.cwd.as_deref(),
            "the saved directory is the one the terminal opened in"
        );
        assert_eq!(written.shell.as_deref(), created.config.shell.as_deref());
        // Story 25 and H07: nothing to replay, and nothing persisted.
        assert_eq!(
            written.initial_command, None,
            "a saved terminal must not carry a command to replay"
        );
        assert_eq!(written.logging.mode, EffectiveLogMode::Off);
        assert_eq!(written.logging.source, LogSource::None);

        // The window is told what the row became, and told it as saved: a row
        // still filed as temporary is one the user is offered to remove.
        let saves = sink.saves();
        match saves.as_slice() {
            [announced] => {
                assert_eq!(announced.id, id);
                assert_eq!(announced.name, "项目终端");
                assert!(
                    !announced.temporary,
                    "the announcement has to carry the saved configuration"
                );
            }
            other => panic!("exactly one save is announced, got {other:?}"),
        }

        core.stop(&id).expect("cleanup");
    }

    /// The written text is the launch method and the user's words — the
    /// assertion that no command, no scrollback and no run state leaked into
    /// the file (story 25).
    #[test]
    fn what_is_written_is_the_launch_method_and_nothing_else() {
        let config = TempConfig::with(None);
        let core = SessionCore::without_listener();
        let created = running_terminal(&core);
        let id = created.config.id.clone();
        let mut written_form = form("项目终端");
        written_form.purpose = Some("跑构建的终端".to_owned());
        written_form.close_impact = Some("停止会结束它启动的子进程。".to_owned());

        save_terminal_config(&core, Some(config.path()), &id, written_form).expect("saves");

        let text = config.read();
        for expected in [
            "type: terminal",
            "name: 项目终端",
            "purpose: 跑构建的终端",
            "close_impact: 停止会结束它启动的子进程。",
            "shell:",
        ] {
            assert!(text.contains(expected), "`{expected}` missing from {text}");
        }
        for absent in ["initial_command", "command:", "logging:", "url:", "port:"] {
            assert!(
                !text.contains(absent),
                "`{absent}` must not be written: {text}"
            );
        }

        core.stop(&id).expect("cleanup");
    }

    /// An existing hand-maintained config keeps everything it had, and the
    /// saved terminal joins it — the same append rule "添加应用" follows.
    #[test]
    fn saving_a_terminal_keeps_the_rest_of_the_file() {
        let original = "# 我的会话\nsessions:\n  - id: existing\n    name: Existing\n    type: terminal\n    shell: powershell\n";
        let config = TempConfig::with(Some(original));
        let core = SessionCore::without_listener();
        let created = running_terminal(&core);
        let id = created.config.id.clone();

        save_terminal_config(&core, Some(config.path()), &id, form("项目终端")).expect("saves");

        let updated = config.read();
        assert!(
            updated.starts_with(original),
            "the user's text must survive untouched: {updated}"
        );
        let loaded = load_from_file(config.path()).expect("the file loads");
        let ids: Vec<&str> = loaded.sessions.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["existing", id.as_str()]);
        assert!(loaded.errors.is_empty(), "{:?}", loaded.errors);

        core.stop(&id).expect("cleanup");
    }

    /// A config file this build cannot edit safely refuses the save, and the
    /// terminal is left exactly as it was: still temporary, still running, and
    /// still usable (decision 15 — 失败不虚报已保存).
    #[test]
    fn a_refused_save_leaves_the_terminal_temporary_and_running() {
        let broken = "sessions: [ uh oh\n";
        let config = TempConfig::with(Some(broken));
        let sink = std::sync::Arc::new(RecordingSink::default());
        let core = SessionCore::new(sink.clone());
        let created = running_terminal(&core);
        let id = created.config.id.clone();

        let error = save_terminal_config(&core, Some(config.path()), &id, form("项目终端"))
            .expect_err("a broken file cannot be appended to");

        assert!(error.message.contains("YAML"), "{}", error.message);
        assert!(
            sink.saves().is_empty(),
            "a save the file refused must not be announced as saved (decision 15)"
        );
        assert_eq!(config.read(), broken, "the user's file must survive");
        let entry = core
            .entries()
            .into_iter()
            .find(|entry| entry.config.id == id)
            .expect("the terminal is still listed");
        assert!(entry.temporary, "a refused save must not mark it saved");
        assert_eq!(
            core.snapshot(&id).map(|runtime| runtime.status),
            Some(SessionStatus::Running)
        );
        assert_eq!(
            core.snapshot(&id).and_then(|runtime| runtime.run_id),
            created.runtime.run_id,
            "a refused save must not disturb the run"
        );

        core.stop(&id).expect("cleanup");
    }

    /// A config file Windows will not let this process replace: the entry is
    /// not written, the original keeps its bytes, and the terminal is untouched.
    #[test]
    fn an_unwritable_config_leaves_the_terminal_temporary() {
        let original = "sessions:\n";
        let config = TempConfig::with(Some(original));
        let sink = std::sync::Arc::new(RecordingSink::default());
        let core = SessionCore::new(sink.clone());
        let created = running_terminal(&core);
        let id = created.config.id.clone();
        let writable = fs::metadata(config.path())
            .expect("the config has metadata")
            .permissions();
        let mut read_only = writable.clone();
        read_only.set_readonly(true);
        fs::set_permissions(config.path(), read_only).expect("the config can be marked read-only");

        let result = save_terminal_config(&core, Some(config.path()), &id, form("项目终端"));

        let after = config.read();
        fs::set_permissions(config.path(), writable).expect("the original permissions come back");
        let error = result.expect_err("a read-only config cannot be replaced");
        assert!(!error.message.is_empty());
        assert_eq!(after, original, "the file must be exactly as it was");
        assert!(
            core.entries()
                .into_iter()
                .find(|entry| entry.config.id == id)
                .is_some_and(|entry| entry.temporary),
            "a failed write must not have marked the terminal saved"
        );
        assert!(
            sink.saves().is_empty(),
            "a write that failed must not be announced as a save"
        );

        core.stop(&id).expect("cleanup");
    }

    /// A field the config layer refuses keeps its own message and its own
    /// field, so the dialog can put the sentence beside the input.
    #[test]
    fn a_blank_name_is_refused_beside_the_name_field() {
        let config = TempConfig::with(None);
        let core = SessionCore::without_listener();
        let created = running_terminal(&core);
        let id = created.config.id.clone();

        let error = save_terminal_config(&core, Some(config.path()), &id, form("   "))
            .expect_err("a terminal needs a name to be listed under");

        assert_eq!(error.field.as_deref(), Some("name"), "{}", error.message);
        assert!(
            !config.path().exists(),
            "nothing may be written for a blank name"
        );
        assert!(core
            .entries()
            .into_iter()
            .find(|entry| entry.config.id == id)
            .is_some_and(|entry| entry.temporary));

        core.stop(&id).expect("cleanup");
    }

    /// Saving twice is refused, and the name the first save wrote is the one
    /// the file keeps.
    #[test]
    fn saving_the_same_terminal_twice_is_refused_without_touching_the_file() {
        let config = TempConfig::with(None);
        let core = SessionCore::without_listener();
        let created = running_terminal(&core);
        let id = created.config.id.clone();
        save_terminal_config(&core, Some(config.path()), &id, form("项目终端"))
            .expect("the first save lands");
        let after_first = config.read();

        let error = save_terminal_config(&core, Some(config.path()), &id, form("另一个名字"))
            .expect_err("a saved terminal is not saved again");

        assert!(
            error.message.contains("已经是保存过的配置"),
            "{}",
            error.message
        );
        assert_eq!(config.read(), after_first, "the refused save wrote nothing");

        core.stop(&id).expect("cleanup");
    }

    /// A session the registry does not hold cannot be saved, and nothing is
    /// written for it.
    #[test]
    fn an_unknown_session_is_refused() {
        let config = TempConfig::with(None);
        let core = SessionCore::without_listener();

        let error = save_terminal_config(&core, Some(config.path()), "terminal-nope", form("X"))
            .expect_err("there is no such terminal");

        assert!(error.message.contains("terminal-nope"), "{}", error.message);
        assert!(error.field.is_none());
        assert!(!config.path().exists());
    }

    /// Where there is no config file location, the entry says so rather than
    /// saving somewhere unintended.
    #[test]
    fn no_config_location_is_refused() {
        let core = SessionCore::without_listener();
        let created = running_terminal(&core);
        let id = created.config.id.clone();

        let error = save_terminal_config(&core, None, &id, form("项目终端"))
            .expect_err("there is nowhere to save");

        assert!(error.message.contains("配置文件"), "{}", error.message);
        assert!(core
            .entries()
            .into_iter()
            .find(|entry| entry.config.id == id)
            .is_some_and(|entry| entry.temporary));

        core.stop(&id).expect("cleanup");
    }

    /// The saved configuration is what the next start reads back: a terminal
    /// that can be started again from the file alone, in the directory and
    /// shell it was saved with (story 24, H08).
    #[test]
    fn a_saved_terminal_starts_again_from_the_file_alone() {
        let config = TempConfig::with(None);
        let core = SessionCore::without_listener();
        let created = running_terminal(&core);
        let id = created.config.id.clone();
        let directory = created.config.cwd.clone();
        save_terminal_config(&core, Some(config.path()), &id, form("项目终端")).expect("saves");
        // The app exits: a new registry reads the file and nothing else.
        core.stop(&id).expect("cleanup");

        let fresh = SessionCore::without_listener();
        let loaded = load_from_file(config.path()).expect("the file loads");
        for session in loaded.sessions {
            fresh
                .register(session)
                .expect("the saved session registers");
        }
        // The row a restart produces is the saved one, and it is not temporary:
        // a terminal that came back only to be removable again would be a
        // configuration that does not survive its own restart.
        assert!(fresh
            .entries()
            .into_iter()
            .find(|entry| entry.config.id == id)
            .is_some_and(|entry| !entry.temporary));

        let runtime = fresh.start(&id).expect("the saved terminal starts");

        assert_eq!(runtime.status, SessionStatus::Running);
        assert!(runtime.pty_attached, "a real shell is attached");
        assert_eq!(
            fresh.configs()[0].cwd,
            directory,
            "the rebuilt terminal opens where it was saved"
        );
        fresh.stop(&id).expect("cleanup");
    }
}
