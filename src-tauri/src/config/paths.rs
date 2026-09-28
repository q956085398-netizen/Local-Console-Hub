//! App-data path resolution and human-readable log path layout.
//!
//! Windows conventions frozen in `docs/MVP_IMPLEMENTATION_SPEC.md` §2 and
//! `docs/LOGGING.md` §5:
//!
//! ```text
//! %APPDATA%\LocalConsoleHub\config.yaml
//! %LOCALAPPDATA%\LocalConsoleHub\logs\<session_id>\<YYYY-MM>\<run files>
//! %LOCALAPPDATA%\LocalConsoleHub\metadata\...
//! %LOCALAPPDATA%\LocalConsoleHub\cache\...
//! ```
//!
//! Runtime logs must never land beside the executable by default. All path
//! construction is pure and offset-explicit so it is testable without a
//! running app; [`AppPaths::from_env`] is the thin environment wrapper.

use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};

/// Whether `value` is safe to use as a single path component (session ids
/// and run ids become directory/file names — D-011).
///
/// Rules: 1..=64 chars, ASCII alphanumeric/`_`/`-` only, must start with an
/// alphanumeric character.
pub fn is_filesystem_safe_component(value: &str) -> bool {
    let bytes = value.as_bytes();
    match bytes.first() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    bytes.len() <= 64
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
}

/// Directory name used under both app-data roots.
pub const APP_DIR_NAME: &str = "LocalConsoleHub";
/// Config file name inside the config root.
pub const CONFIG_FILE_NAME: &str = "config.yaml";

/// Well-known application data locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// Roaming config file, e.g. `%APPDATA%\LocalConsoleHub\config.yaml`.
    pub config_file: PathBuf,
    /// Root for Hub-generated logs (`docs/LOGGING.md` §5).
    pub logs_dir: PathBuf,
    /// Root for run metadata (`docs/LOGGING.md` §6).
    pub metadata_dir: PathBuf,
    /// Root for disposable caches.
    pub cache_dir: PathBuf,
}

impl AppPaths {
    /// Pure constructor from platform roots, for tests and non-env callers.
    pub fn new(config_root: &Path, data_root: &Path) -> Self {
        let config_app_dir = config_root.join(APP_DIR_NAME);
        let data_app_dir = data_root.join(APP_DIR_NAME);
        AppPaths {
            config_file: config_app_dir.join(CONFIG_FILE_NAME),
            logs_dir: data_app_dir.join("logs"),
            metadata_dir: data_app_dir.join("metadata"),
            cache_dir: data_app_dir.join("cache"),
        }
    }

    /// Resolve paths from the platform environment.
    ///
    /// Returns `None` when the OS does not provide the expected base
    /// directories (e.g. `APPDATA`/`LOCALAPPDATA` unset on Windows);
    /// callers turn that into an actionable message rather than guessing a
    /// fallback location.
    pub fn from_env() -> Option<Self> {
        let config_root = dirs::config_dir()?;
        let data_root = dirs::data_local_dir()?;
        Some(Self::new(&config_root, &data_root))
    }
}

/// `logs/<session_id>/<YYYY-MM>` for a run started at `unix_secs` with an
/// explicit UTC offset (local months are what humans browse by, D-011).
///
/// `None` when `session_id` is not a single safe path component or the
/// timestamp is out of range.
pub fn session_log_dir(
    logs_root: &Path,
    session_id: &str,
    unix_secs: i64,
    utc_offset_secs: i32,
) -> Option<PathBuf> {
    if !is_filesystem_safe_component(session_id) {
        return None;
    }
    let offset = FixedOffset::east_opt(utc_offset_secs)?;
    let month = to_local(unix_secs, &offset)?.format("%Y-%m").to_string();
    Some(logs_root.join(session_id).join(month))
}

/// Run log file name like `2026-09-24_09-30-15__run-8f31.log`
/// (`docs/LOGGING.md` §5: timestamped, run-tagged, no database needed to
/// understand ownership).
///
/// `None` when `run_id` is not filesystem-safe or the timestamp is out of
/// range.
pub fn run_log_filename(unix_secs: i64, utc_offset_secs: i32, run_id: &str) -> Option<String> {
    if !is_filesystem_safe_component(run_id) {
        return None;
    }
    let offset = FixedOffset::east_opt(utc_offset_secs)?;
    let stamp = to_local(unix_secs, &offset)?.format("%Y-%m-%d_%H-%M-%S");
    Some(format!("{stamp}__run-{run_id}.log"))
}

/// Full path of one run's log file: `logs/<session>/<YYYY-MM>/<file>`.
pub fn run_log_path(
    logs_root: &Path,
    session_id: &str,
    unix_secs: i64,
    utc_offset_secs: i32,
    run_id: &str,
) -> Option<PathBuf> {
    let dir = session_log_dir(logs_root, session_id, unix_secs, utc_offset_secs)?;
    let file = run_log_filename(unix_secs, utc_offset_secs, run_id)?;
    Some(dir.join(file))
}

fn to_local(unix_secs: i64, offset: &FixedOffset) -> Option<DateTime<FixedOffset>> {
    let utc = DateTime::from_timestamp(unix_secs, 0)?;
    Some(utc.with_timezone(offset))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path_str(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn app_paths_lay_out_roaming_config_and_local_data() {
        let paths = AppPaths::new(Path::new("C:/cfg"), Path::new("D:/data"));
        assert_eq!(
            path_str(&paths.config_file),
            "C:/cfg/LocalConsoleHub/config.yaml"
        );
        assert_eq!(path_str(&paths.logs_dir), "D:/data/LocalConsoleHub/logs");
        assert_eq!(
            path_str(&paths.metadata_dir),
            "D:/data/LocalConsoleHub/metadata"
        );
        assert_eq!(path_str(&paths.cache_dir), "D:/data/LocalConsoleHub/cache");
    }

    #[test]
    fn session_log_dir_groups_by_session_then_month() {
        // 2026-09-24T01:30:15Z == 2026-09-24T09:30:15+08:00.
        let dir = session_log_dir(Path::new("logs"), "comfyui", 1_790_213_415, 8 * 3600)
            .expect("timestamp in range");
        assert_eq!(path_str(&dir), "logs/comfyui/2026-09");
    }

    #[test]
    fn run_log_filename_matches_the_logging_spec_shape() {
        let name = run_log_filename(1_790_213_415, 8 * 3600, "8f31").expect("timestamp in range");
        assert_eq!(name, "2026-09-24_09-30-15__run-8f31.log");
    }

    #[test]
    fn run_log_filename_rejects_unsafe_run_ids() {
        // run ids become file name components; refuse path separators etc.
        assert!(run_log_filename(0, 0, "../escape").is_none());
        assert!(run_log_filename(0, 0, "").is_none());
        assert!(run_log_filename(0, 0, "with space").is_none());
        assert!(run_log_filename(0, 0, "8f31").is_some());
    }

    #[test]
    fn run_log_path_composes_dir_and_file() {
        let path = run_log_path(Path::new("logs"), "st", 0, 0, "ab12").expect("valid inputs");
        assert_eq!(
            path_str(&path),
            "logs/st/1970-01/1970-01-01_00-00-00__run-ab12.log"
        );
    }
}
