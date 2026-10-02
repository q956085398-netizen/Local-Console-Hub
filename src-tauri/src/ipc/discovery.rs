//! User-selected paths and read-only launch candidates; never starts a program.
use crate::app::discovery::{self, DirectoryScan};

#[tauri::command]
pub async fn scan_application_directory(cwd: String) -> Result<DirectoryScan, String> {
    tauri::async_runtime::spawn_blocking(move || discovery::scan(std::path::Path::new(&cwd)))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn pick_application_path(
    window: tauri::WebviewWindow,
    kind: String,
    cwd: Option<String>,
) -> Result<Option<String>, String> {
    if !matches!(kind.as_str(), "directory" | "program" | "log") {
        return Err("未知的文件选择类型。".into());
    }
    #[cfg(windows)]
    {
        // RFD's async Windows backend owns a fresh STA thread for the modal
        // dialog, instead of borrowing a runtime worker's COM apartment.
        // Native window handles must be acquired on the UI thread.
        let (sender, mut receiver) = tauri::async_runtime::channel(1);
        let parent = window.clone();
        window
            .run_on_main_thread(move || {
                let _ = sender.try_send(rfd::AsyncFileDialog::new().set_parent(&parent));
            })
            .map_err(|error| error.to_string())?;
        let mut dialog = receiver.recv().await.ok_or("无法创建文件选择窗口。")?;
        if let Some(cwd) = cwd.filter(|path| std::path::Path::new(path).is_dir()) {
            dialog = dialog.set_directory(cwd);
        }
        let path = match kind.as_str() {
            "directory" => dialog.set_title("选择应用所在目录").pick_folder().await,
            "program" => {
                dialog
                    .set_title("选择启动程序或脚本")
                    .add_filter("启动文件", &["exe", "bat", "cmd", "ps1"])
                    .pick_file()
                    .await
            }
            _ => dialog.set_title("选择应用日志文件").pick_file().await,
        };
        Ok(path.map(|file| file.path().to_string_lossy().into_owned()))
    }
    #[cfg(not(windows))]
    {
        let _ = (window, cwd);
        Err("当前平台不支持原生文件选择。".into())
    }
}
