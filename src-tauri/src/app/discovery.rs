//! Bounded directory inspection. Files are candidates, not executable instructions.
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::{fs, io::Read};

const MAX_ENTRIES: usize = 600;
const MAX_CANDIDATES: usize = 30;
const MAX_DEPTH: usize = 2;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchCandidate {
    pub path: String,
    pub command: String,
    pub cwd: String,
    pub port: Option<u16>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryScan {
    pub name: String,
    pub cwd: String,
    pub candidates: Vec<LaunchCandidate>,
    pub logs: Vec<String>,
    pub warnings: Vec<String>,
}

/// Quote a literal path using the Windows argument parser's rules.
/// A selected filename is never appended as unquoted shell text.
pub fn command_for(path: &Path) -> String {
    let quoted = format!("\"{}\"", path.to_string_lossy());
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ps1"))
    {
        format!("powershell.exe -NoProfile -File {quoted}")
    } else {
        quoted
    }
}

/// Only an explicit, unique --port argument is a prefill hint.
fn explicit_port(text: &str) -> Option<u16> {
    let words: Vec<_> = text.split_whitespace().collect();
    let mut ports = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let value = if *word == "--port" {
            words.get(index + 1).copied()
        } else {
            word.strip_prefix("--port=")
        };
        if let Some(port) = value
            .and_then(|value| value.trim_matches(['\'', '"']).parse::<u16>().ok())
            .filter(|value| *value > 0)
        {
            ports.push(port);
        }
    }
    ports.sort_unstable();
    ports.dedup();
    (ports.len() == 1).then(|| ports[0])
}

pub fn scan(root: &Path) -> Result<DirectoryScan, String> {
    if !root.is_absolute() {
        return Err("请选择完整的应用目录路径。".into());
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("无法读取应用目录：{error}"))?;
    if !root.is_dir() {
        return Err("所选路径不是目录。".into());
    }
    // canonicalize on Windows adds an extended-length prefix; retain the user's
    // ordinary absolute spelling in the form rather than exposing an API prefix.
    let display_root = plain_path(&root);
    let mut result = DirectoryScan {
        name: root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        cwd: display_root.to_string_lossy().into_owned(),
        candidates: Vec::new(),
        logs: Vec::new(),
        warnings: Vec::new(),
    };
    let mut pending = vec![(root.clone(), 0usize)];
    let mut visited = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if depth == 0 => return Err(format!("无法扫描应用目录：{error}")),
            Err(_) => {
                result.warnings.push("部分子目录无法读取，已跳过。".into());
                continue;
            }
        };
        for entry in entries {
            visited += 1;
            if visited > MAX_ENTRIES {
                break;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let path = entry.path();
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(_) => continue,
            };
            if kind.is_symlink() {
                continue;
            }
            // Junctions may look like directories: resolve each entry and refuse
            // paths outside the selected root before walking or reading a file.
            let resolved = match path.canonicalize() {
                Ok(path) => path,
                Err(_) => continue,
            };
            if !resolved.starts_with(&root) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if kind.is_dir() {
                if depth < MAX_DEPTH
                    && !matches!(
                        name.as_str(),
                        "node_modules"
                            | ".git"
                            | "venv"
                            | ".venv"
                            | "python"
                            | "python_embeded"
                            | "python_embedded"
                            | "models"
                            | "target"
                            | "__pycache__"
                    )
                {
                    pending.push((resolved, depth + 1));
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let extension = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let display = plain_path(&path);
            if extension == "log" && result.logs.len() < 10 {
                result.logs.push(display.to_string_lossy().into_owned());
            }
            if !matches!(extension.as_str(), "exe" | "bat" | "cmd" | "ps1")
                || result.candidates.len() >= MAX_CANDIDATES
            {
                continue;
            }
            if name.starts_with("unins") || name.starts_with("uninstall") {
                continue;
            }
            let port = if matches!(extension.as_str(), "bat" | "cmd" | "ps1") {
                let mut data = Vec::new();
                let _ = fs::File::open(&path)
                    .ok()
                    .and_then(|file| file.take(65537).read_to_end(&mut data).ok());
                if data.len() <= 65536 {
                    std::str::from_utf8(&data).ok().and_then(explicit_port)
                } else {
                    None
                }
            } else {
                None
            };
            result.candidates.push(LaunchCandidate {
                path: display.to_string_lossy().into_owned(),
                command: command_for(&display),
                cwd: plain_path(path.parent().unwrap_or(&root))
                    .to_string_lossy()
                    .into_owned(),
                port,
            });
        }
        if visited > MAX_ENTRIES {
            break;
        }
    }
    if visited > MAX_ENTRIES || result.candidates.len() >= MAX_CANDIDATES {
        result
            .warnings
            .push("目录较大，扫描结果已限制；可直接选择启动文件。".into());
    }
    result.candidates.sort_by(|a, b| a.path.cmp(&b.path));
    result.logs.sort();
    result.warnings.sort();
    result.warnings.dedup();
    Ok(result)
}

fn plain_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(tail) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{tail}"))
    } else if let Some(tail) = text.strip_prefix(r"\\?\") {
        PathBuf::from(tail)
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_a_unique_explicit_port_can_be_prefilled() {
        assert_eq!(explicit_port("python main.py --port 8189"), Some(8189));
        assert_eq!(explicit_port("--port=8189 --port 8189"), Some(8189));
        assert_eq!(explicit_port("--port 8189 --port 8190"), None);
        assert_eq!(explicit_port("--port 0 --port 99999"), None);
    }
    #[test]
    fn scan_returns_candidates_without_running_scripts_or_walking_dependencies() {
        let root = std::env::temp_dir().join(format!(
            "lch-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("node_modules")).unwrap();
        fs::write(
            root.join("Start Here.cmd"),
            "echo DO_NOT_RUN\npython main.py --port 8189",
        )
        .unwrap();
        fs::write(root.join("node_modules/ignored.exe"), "not executable").unwrap();
        fs::write(root.join("app.log"), "").unwrap();
        let scan = scan(&root).unwrap();
        assert_eq!(scan.candidates.len(), 1);
        assert_eq!(scan.candidates[0].port, Some(8189));
        assert!(scan.candidates[0].command.starts_with('"'));
        assert_eq!(scan.logs.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
}
