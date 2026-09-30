@echo off
setlocal
cd /d "%~dp0.."
echo Daily PowerShell shortcut entry acceptance - issue 63
echo Builds a throwaway shortcut with the installer, then measures Hubs, shells,
echo window visibility and what an invalid directory does.
echo Exit any running Hub from its tray menu first, or the script refuses to run.
echo.
powershell -NoLogo -NoProfile -File "%~dp0verify-shortcut-entry.ps1" %*
echo.
pause
endlocal
