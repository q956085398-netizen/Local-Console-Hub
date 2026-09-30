@echo off
setlocal
cd /d "%~dp0.."
echo Single-instance entry acceptance - issue 60
echo Measures process counts, exit codes, window visibility and the entry's console.
echo Uses no sessions; ends only the Hub processes it starts.
echo Exit any running Hub from its tray menu first, or the script refuses to run.
echo.
powershell -NoLogo -NoProfile -File "%~dp0verify-single-instance.ps1" %*
echo.
pause
endlocal
