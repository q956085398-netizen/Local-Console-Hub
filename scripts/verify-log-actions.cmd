@echo off
cd /d "%~dp0.."
powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0prepare-verification-config.ps1"
if errorlevel 1 (
  echo Config preparation failed. Review the error shown above.
  pause
  exit /b 1
)
echo Keep this window open while testing the logging actions.
call npm run tauri -- dev --no-watch
pause
