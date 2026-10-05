//! The release Hub has no console. A stop borrows the service's console;
//! subsequent starts must still work, including batch launchers with spaces.
#![cfg(windows)]

use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use local_console_hub_lib::config::{load_from_file, AppPaths};
use local_console_hub_lib::logging::LogRoots;
use local_console_hub_lib::session::core::SessionCore;
use local_console_hub_lib::session::state::SessionStatus;
use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::{
    FreeConsole, GetConsoleProcessList, GetStdHandle, SetStdHandle, STD_ERROR_HANDLE,
    STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, GetExitCodeProcess, TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW,
    PROCESS_INFORMATION, STARTUPINFOW,
};

#[test]
fn a_consoleless_hub_can_start_and_restart_after_stopping_a_batch_service() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("lch restart {} {stamp}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let mut command: Vec<u16> = format!(
        "\"{}\" consoleless_service_restart_fixture --exact --ignored --nocapture --test-threads=1",
        std::env::current_exe().unwrap().display()
    )
    .encode_utf16()
    .chain(Some(0))
    .collect();
    let directory: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    // Rust's Command sets STARTF_USESTDHANDLES. Explorer's GUI launch does
    // not, and AttachConsole changes standard handles only in the latter case.
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut child: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    assert_ne!(
        unsafe {
            CreateProcessW(
                std::ptr::null(),
                command.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                CREATE_NO_WINDOW,
                std::ptr::null(),
                directory.as_ptr(),
                &startup,
                &mut child,
            )
        },
        0,
        "the isolated Hub-shaped process starts: {}",
        std::io::Error::last_os_error()
    );
    let waited = unsafe { WaitForSingleObject(child.hProcess, 30_000) };
    let mut exit_code = u32::MAX;
    unsafe {
        if waited != WAIT_OBJECT_0 {
            TerminateProcess(child.hProcess, 1);
            WaitForSingleObject(child.hProcess, 5_000);
        }
        GetExitCodeProcess(child.hProcess, &mut exit_code);
        CloseHandle(child.hThread);
        CloseHandle(child.hProcess);
    }
    let report =
        fs::read_to_string(root.join("result.txt")).unwrap_or_else(|error| error.to_string());
    let _ = fs::remove_dir_all(&root);
    assert_eq!(
        waited, WAIT_OBJECT_0,
        "service stop/start probe timed out: {report}"
    );
    assert_eq!(exit_code, 0, "Hub-shaped stop/start failed: {report}");
    assert_eq!(report, "ok");
}

#[test]
#[ignore = "isolated child fixture for the consoleless service restart regression"]
fn consoleless_service_restart_fixture() {
    let root = std::env::current_dir().unwrap();
    // No other test shares this process. Match the GUI release executable's
    // lack of an attached console without altering the test runner's console.
    unsafe {
        FreeConsole();
        for stream in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            SetStdHandle(stream, std::ptr::null_mut());
        }
    }
    let result = exercise_service(&root);
    fs::write(
        root.join("result.txt"),
        result.as_ref().err().map_or("ok", String::as_str),
    )
    .unwrap();
    assert!(result.is_ok(), "{result:?}");
}

fn exercise_service(root: &Path) -> Result<(), String> {
    let streams = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
    let original_handles = streams.map(|stream| unsafe { GetStdHandle(stream) });
    let mut member = 0;
    if unsafe { GetConsoleProcessList(&mut member, 1) } != 0 {
        return Err("the fixture must have no attached console".to_owned());
    }
    fs::write(
        root.join("Start service.bat"),
        "@echo off\r\necho service-ready\r\nping -n 120 127.0.0.1 > NUL\r\n",
    )
    .map_err(|error| error.to_string())?;
    let paths = AppPaths::new(root, root);
    fs::create_dir_all(paths.config_file.parent().unwrap()).map_err(|error| error.to_string())?;
    let cwd = root.to_string_lossy().replace('\'', "''");
    fs::write(&paths.config_file, format!(
        "sessions:\n  - id: service\n    name: Batch service\n    type: service\n    cwd: '{cwd}'\n    command: '\"{cwd}\\Start service.bat\"'\n    logging:\n      mode: on_error\n      source: captured\n"
    )).map_err(|error| error.to_string())?;
    let loaded = load_from_file(&paths.config_file).map_err(|error| error.to_string())?;
    if !loaded.errors.is_empty() {
        return Err(format!("invalid test config: {:?}", loaded.errors));
    }
    let core = SessionCore::without_listener().with_log_roots(LogRoots::from_app_paths(&paths));
    for config in loaded.sessions {
        core.register(config).map_err(|error| error.to_string())?;
    }
    for cycle in 0..3 {
        let running = core
            .start("service")
            .map_err(|error| format!("start {cycle}: {error}"))?;
        if running.status != SessionStatus::Running {
            return Err(format!("start {cycle} did not reach Running"));
        }
        wait_for_output(&core, cycle + 1)?;
        core.stop_with_timeout("service", Duration::from_millis(200))
            .map_err(|error| format!("stop {cycle}: {error}"))?;
        if streams.map(|stream| unsafe { GetStdHandle(stream) }) != original_handles {
            return Err(format!("stop {cycle} changed the Hub's standard handles"));
        }
        if core.snapshot("service").unwrap().pid.is_some() {
            return Err(format!("stop {cycle} retained a live process"));
        }
    }
    core.start("service")
        .map_err(|error| format!("start before restart: {error}"))?;
    wait_for_output(&core, 4)?;
    core.restart("service")
        .map_err(|error| format!("restart: {error}"))?;
    wait_for_output(&core, 5)?;
    core.stop_with_timeout("service", Duration::from_millis(200))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn wait_for_output(core: &SessionCore, count: usize) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let text: String = core
            .terminal_buffer("service")
            .unwrap_or_default()
            .iter()
            .map(|chunk| chunk.text())
            .collect();
        if text.matches("service-ready").count() >= count {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "service run {count} did not produce output; runtime={:?}; output={text}",
                core.snapshot("service")
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
