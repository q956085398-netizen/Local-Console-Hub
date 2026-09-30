//! Temporary interactive terminals — the quick "新建 PowerShell" entry (#62,
//! spec #59 decisions 4–6).
//!
//! A temporary terminal is a session like any other: it lives in the same
//! registry, runs the same lifecycle and is closed by the same process-tree
//! rules (#61). What this module decides is only the *outside* of one — which
//! shell to host, which directory to open in, and what identity a brand-new
//! one gets — because those are the questions a config file would otherwise
//! answer, and a temporary terminal deliberately has no config file.
//!
//! ## Why the environment is a parameter
//!
//! "Prefer PowerShell 7, fall back to Windows PowerShell" is a product rule
//! (spec #59 decision 5), and it is testable as one only if the machine's
//! actual shells are injected: asserting the rule against whatever happens to
//! be installed on the test machine would assert the machine, not the rule.
//! [`ShellLookup`] is that seam. Production passes [`SystemLookup`], which
//! searches `PATH` and then the well-known install locations; a test passes a
//! lookup that answers with the shells it names.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// PowerShell 7's executable name — the shell the quick entry prefers.
pub const POWERSHELL_7: &str = "pwsh";

/// Windows PowerShell's executable name — the shell every Windows has.
pub const WINDOWS_POWERSHELL: &str = "powershell";

/// A shell a temporary terminal can host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    /// The resolved program, as an absolute path where one was found: the
    /// session's config carries this, so what the terminal runs is a fact the
    /// registry can report rather than a lookup repeated at every start.
    pub program: PathBuf,
    /// Which PowerShell family this is, for the sentence a failure or a
    /// surface has to say.
    pub flavor: ShellFlavor,
}

/// The two PowerShell families the quick entry knows (spec #59 decision 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFlavor {
    PowerShell7,
    WindowsPowerShell,
}

impl ShellFlavor {
    /// The name a surface shows for this shell.
    pub fn label(&self) -> &'static str {
        match self {
            ShellFlavor::PowerShell7 => "PowerShell 7",
            ShellFlavor::WindowsPowerShell => "Windows PowerShell",
        }
    }
}

/// Whether a named shell exists on this machine, and where.
///
/// `None` means "this machine has no such shell", which is the only answer the
/// preference order needs; where it lives is [`Shell::program`]'s business.
pub trait ShellLookup {
    fn find(&self, name: &str) -> Option<PathBuf>;
}

/// The real machine: `PATH` first, then the locations a standard install uses.
///
/// `PATH` first because that is what a user's own shell resolution does — a
/// `pwsh` shim earlier on `PATH` is the one they would get by typing `pwsh` —
/// and the install locations second, because a non-interactive process can
/// inherit a `PATH` that lacks them (the store and MSI installs both add
/// themselves to the *user's*, not to every process's).
pub struct SystemLookup;

impl ShellLookup for SystemLookup {
    fn find(&self, name: &str) -> Option<PathBuf> {
        on_path(name).or_else(|| installed_at(name))
    }
}

/// Resolve the shell a new terminal should host.
///
/// The order is the spec's: PowerShell 7 when it is installed, Windows
/// PowerShell otherwise (spec #59 decision 5 — "与日常使用的 shell 一致"
/// first, "无需额外安装即可开始使用" second). Failing both is a real failure
/// with both names in it, not a silent fallback to something else: the entry
/// cannot open a terminal, and the user is the one who can fix that.
pub fn resolve_shell(lookup: &dyn ShellLookup) -> Result<Shell, String> {
    if let Some(program) = lookup.find(POWERSHELL_7) {
        return Ok(Shell {
            program,
            flavor: ShellFlavor::PowerShell7,
        });
    }
    if let Some(program) = lookup.find(WINDOWS_POWERSHELL) {
        return Ok(Shell {
            program,
            flavor: ShellFlavor::WindowsPowerShell,
        });
    }
    Err(format!(
        "no PowerShell is available: neither PowerShell 7 (`{POWERSHELL_7}.exe`) nor Windows \
         PowerShell (`{WINDOWS_POWERSHELL}.exe`) could be found. Install PowerShell 7, or check \
         that `{WINDOWS_POWERSHELL}.exe` is on PATH."
    ))
}

/// Resolve the directory a new terminal opens in.
///
/// An entry that names a directory uses it; otherwise the session opens in the
/// user's home directory (spec #59 decision 5), never in whatever directory
/// the app happens to have been started from — a shell that opens in
/// `C:\Windows\System32` because it inherited a service's working directory is
/// the accident that rule exists to prevent. A directory that is not there is
/// refused with its own path rather than replaced by the home directory: a
/// user who asked for `D:\Work` and silently got `C:\Users\...` would run their
/// commands somewhere they did not choose.
pub fn resolve_cwd(requested: Option<&str>, home: Option<PathBuf>) -> Result<PathBuf, String> {
    let Some(requested) = requested else {
        let home = home.ok_or_else(|| {
            "no home directory could be determined for this user; pass an explicit working \
             directory"
                .to_owned()
        })?;
        if !home.is_dir() {
            return Err(format!(
                "the home directory `{}` does not exist or is not a directory; pass an explicit \
                 working directory",
                home.display()
            ));
        }
        return Ok(home);
    };

    let path = PathBuf::from(requested.trim());
    if !path.is_dir() {
        return Err(format!(
            "the working directory `{}` does not exist or is not a directory",
            path.display()
        ));
    }
    Ok(path)
}

/// The identity of one brand-new temporary terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporaryTerminal {
    /// The session id: unique for the life of the app *and* across restarts,
    /// filesystem-safe because a session's log directory is named after it
    /// (`docs/LOGGING.md` §5) even though a temporary terminal writes none.
    pub id: String,
    /// The display name, e.g. `PowerShell 3`.
    pub name: String,
}

/// Mint the identity of the next temporary terminal.
///
/// The ordinal is the whole point of the name: a user who clicks the entry
/// three times has three terminals, and three rows reading `PowerShell` would
/// be indistinguishable in the rail. The id carries the same ordinal plus the
/// clock, for the reason [`crate::session::runtime::RunId::mint`] documents —
/// the clock keeps a later app process from reusing an earlier one's ids, and
/// the counter separates two terminals created inside the same millisecond.
pub fn mint() -> TemporaryTerminal {
    static SEQUENCE: AtomicU32 = AtomicU32::new(0);

    let ordinal = SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or(0);
    TemporaryTerminal {
        id: format!("terminal-{millis:x}{ordinal:04x}"),
        name: format!("PowerShell {ordinal}"),
    }
}

/// The `shell` value a session config carries for `program`.
///
/// The config's `shell` is a command line (`powershell -NoProfile`), split
/// into program and arguments by the session layer, so a path with a space in
/// it — `C:\Program Files\PowerShell\7\pwsh.exe` is the normal case, not an
/// exotic one — has to be quoted to survive that split as one token.
pub fn shell_command(program: &Path) -> String {
    let text = program.to_string_lossy();
    if text.contains(char::is_whitespace) {
        format!("\"{text}\"")
    } else {
        text.into_owned()
    }
}

/// The first `name.exe` or `name` on `PATH`.
fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        if directory.as_os_str().is_empty() {
            continue;
        }
        for candidate in [directory.join(format!("{name}.exe")), directory.join(name)] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// The location a standard install of `name` uses.
fn installed_at(name: &str) -> Option<PathBuf> {
    let candidate = match name {
        // PowerShell 7's MSI and store layouts both put the executable here.
        POWERSHELL_7 => PathBuf::from(std::env::var_os("ProgramFiles")?)
            .join("PowerShell")
            .join("7")
            .join("pwsh.exe"),
        // Windows PowerShell ships with the OS; this is where it has lived
        // since Windows 7, and the path does not depend on `PATH` at all.
        WINDOWS_POWERSHELL => PathBuf::from(std::env::var_os("SystemRoot")?)
            .join("System32")
            .join("WindowsPowerShell")
            .join("v1.0")
            .join("powershell.exe"),
        _ => return None,
    };
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// A machine whose shells are exactly the ones a test names.
    #[derive(Default)]
    struct FakeLookup(HashMap<String, PathBuf>);

    impl FakeLookup {
        fn with(names: &[(&str, &str)]) -> Self {
            FakeLookup(
                names
                    .iter()
                    .map(|(name, path)| ((*name).to_owned(), PathBuf::from(path)))
                    .collect(),
            )
        }
    }

    impl ShellLookup for FakeLookup {
        fn find(&self, name: &str) -> Option<PathBuf> {
            self.0.get(name).cloned()
        }
    }

    /// Spec #59 decision 5: PowerShell 7 when it is there.
    #[test]
    fn powershell_7_wins_when_it_is_installed() {
        let machine = FakeLookup::with(&[
            (POWERSHELL_7, r"C:\Program Files\PowerShell\7\pwsh.exe"),
            (
                WINDOWS_POWERSHELL,
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            ),
        ]);

        let shell = resolve_shell(&machine).expect("a shell is available");

        assert_eq!(
            shell.program,
            PathBuf::from(r"C:\Program Files\PowerShell\7\pwsh.exe")
        );
        assert_eq!(shell.flavor, ShellFlavor::PowerShell7);
    }

    /// …and Windows PowerShell when it is not, so a fresh machine can start.
    #[test]
    fn windows_powershell_is_the_fallback() {
        let machine = FakeLookup::with(&[(
            WINDOWS_POWERSHELL,
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
        )]);

        let shell = resolve_shell(&machine).expect("a shell is available");

        assert_eq!(shell.flavor, ShellFlavor::WindowsPowerShell);
        assert_eq!(shell.flavor.label(), "Windows PowerShell");
    }

    /// A machine with neither says so, by both names, in one message.
    #[test]
    fn a_machine_without_powershell_names_both_shells() {
        let message = resolve_shell(&FakeLookup::default())
            .expect_err("a machine with no PowerShell cannot host a terminal");

        assert!(message.contains(POWERSHELL_7), "{message}");
        assert!(message.contains(WINDOWS_POWERSHELL), "{message}");
        assert!(message.contains("PATH"), "{message}");
    }

    /// No entry directory means the user's home directory (#59 decision 5).
    #[test]
    fn an_entry_without_a_directory_opens_in_the_home_directory() {
        let home = std::env::temp_dir();

        let cwd = resolve_cwd(None, Some(home.clone())).expect("the home directory is usable");

        assert_eq!(cwd, home);
    }

    /// A home directory that is not there is refused with its path rather than
    /// opened anyway — the shell would start somewhere the user did not choose.
    #[test]
    fn a_home_directory_that_is_not_there_is_refused_by_name() {
        let missing = std::env::temp_dir().join("lch-there-is-no-such-home-62");

        let message = resolve_cwd(None, Some(missing.clone()))
            .expect_err("a missing home directory cannot host a terminal");

        assert!(
            message.contains(&missing.display().to_string()),
            "{message}"
        );
    }

    /// An entry that names a directory uses exactly that one — Unicode and
    /// spaces included (story 16, H06).
    #[test]
    fn an_entry_directory_is_used_as_given() {
        let directory = std::env::temp_dir();
        let given = directory.to_string_lossy().into_owned();

        let cwd = resolve_cwd(Some(&given), None).expect("a real directory is usable");

        assert_eq!(cwd, directory);
    }

    /// A directory that is not there is a failure about *that* directory, not
    /// a quiet opening somewhere else.
    #[test]
    fn a_missing_directory_is_refused_rather_than_replaced() {
        let missing = std::env::temp_dir().join("lch-there-is-no-such-directory-62");
        let message = resolve_cwd(Some(&missing.to_string_lossy()), Some(std::env::temp_dir()))
            .expect_err("a missing directory cannot host a terminal");

        assert!(
            message.contains(&missing.display().to_string()),
            "{message}"
        );
        assert!(message.contains("does not exist"), "{message}");
    }

    /// A file is not a directory, however real it is.
    #[test]
    fn a_file_is_not_a_working_directory() {
        let file = std::env::temp_dir().join("lch-t62-not-a-directory.txt");
        std::fs::write(&file, b"x").expect("the fixture file is writable");

        let result = resolve_cwd(Some(&file.to_string_lossy()), None);

        std::fs::remove_file(&file).ok();
        assert!(result.is_err(), "a file cannot be a terminal's cwd");
    }

    /// No home directory to fall back to is its own failure, with the way out
    /// in it.
    #[test]
    fn no_home_directory_is_refused_with_the_alternative() {
        let message =
            resolve_cwd(None, None).expect_err("without a home directory there is no default");

        assert!(message.contains("working directory"), "{message}");
    }

    /// Two terminals created back to back are two terminals, whatever the
    /// clock did: distinct ids *and* distinct names, with ids that can name a
    /// session's log directory (`config::session_log_dir` applies the same
    /// path-component rule a session id has to satisfy — `docs/LOGGING.md` §5).
    #[test]
    fn minted_terminals_are_distinct_and_filesystem_safe() {
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        for _ in 0..1_000 {
            let terminal = mint();
            assert!(
                crate::config::session_log_dir(Path::new("logs"), &terminal.id, 1_790_213_415, 0)
                    .is_some(),
                "id {:?} is not a usable path component",
                terminal.id
            );
            assert!(ids.insert(terminal.id), "an id was minted twice");
            assert!(names.insert(terminal.name), "a name was minted twice");
        }
    }

    /// The name says which terminal it is; the id is not what a rail shows.
    #[test]
    fn a_minted_name_is_the_shell_and_its_ordinal() {
        let terminal = mint();
        let (word, ordinal) = terminal
            .name
            .split_once(' ')
            .expect("the name is a word and an ordinal");

        assert_eq!(word, "PowerShell");
        assert!(
            ordinal.parse::<u32>().is_ok_and(|value| value >= 1),
            "ordinal {ordinal:?} is not a positive number"
        );
        assert!(terminal.id.starts_with("terminal-"), "{}", terminal.id);
    }

    /// A program path with a space survives the config's command-line split as
    /// one token, which is what makes `C:\Program Files\...` usable at all.
    #[test]
    fn a_path_with_a_space_is_quoted_for_the_config() {
        let command = shell_command(Path::new(r"C:\Program Files\PowerShell\7\pwsh.exe"));

        assert_eq!(command, r#""C:\Program Files\PowerShell\7\pwsh.exe""#);
        assert_eq!(
            shell_command(Path::new(r"C:\Windows\pwsh.exe")),
            r"C:\Windows\pwsh.exe"
        );
    }
}
