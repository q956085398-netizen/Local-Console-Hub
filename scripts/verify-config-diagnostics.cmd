@echo off
cd /d "%~dp0.."
echo Configuration diagnostics acceptance - no config is created or replaced.
echo Config: %APPDATA%\LocalConsoleHub\config.yaml
echo Exit the existing app from its tray menu and close its launch window first.
echo Missing or unreadable config should still open the app with a diagnostic.
echo Keep this window open while testing.
call npm run tauri -- dev --no-watch
pause
