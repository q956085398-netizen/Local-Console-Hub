@echo off
setlocal
cd /d "%~dp0.."
echo Session initialization / native tray acceptance - issue 54
echo Uses your existing config; no config is created or replaced.
echo Exit the existing app from its tray menu and close its launch window first.
echo 1: Config list arrives first; runtime snapshot is delayed 30 seconds.
echo 2: Runtime snapshot arrives first; config list is delayed 30 seconds.
choice /C 12 /N /M "Select order (1 or 2): "
if errorlevel 2 (
  set "VITE_VERIFY_SESSION_INITIALIZATION=configs-last"
) else (
  set "VITE_VERIFY_SESSION_INITIALIZATION=runtimes-last"
)
echo Initial loading lasts at least 30 seconds.
echo In the app, Ctrl+Alt+R reloads ONLY the frontend, keeping sessions alive.
echo While loading, use the native tray Stop All or Restart Failed command.
echo Keep this window open while testing.
call npm run tauri -- dev --no-watch
pause
endlocal
