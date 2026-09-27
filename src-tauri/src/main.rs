// Prevents an additional console window on Windows in release builds.
// Do not remove — see https://tauri.app/start/create-project/
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    local_console_hub_lib::run()
}
