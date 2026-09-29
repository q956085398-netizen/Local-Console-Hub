; Local Console Hub — NSIS language strings (English), bundled via
; `bundle.windows.nsis.customLanguageFiles` in `src-tauri/tauri.conf.json`.
;
; This is Tauri's own `English.nsh` for this bundler version, copied verbatim
; except for `deleteAppData` at the bottom: the label of the checkbox the
; uninstaller adds to its confirm page. Upstream's text is "Delete the
; application data", which promises the app's config and logs while the
; uninstaller in fact removes the WebView2 user-data directory
; (`%LOCALAPPDATA%\com.localconsolehub.hub`) and its own registry values. The
; behaviour is upstream's and is left alone; only the promise is corrected
; (see `docs/RELEASE.md` §2.2 and #47).
;
; `customLanguageFiles` REPLACES the bundler's copy for this language rather
; than merging with it, so this file has to stay complete: when a bundler
; version adds a `LangString`, re-copy its `English.nsh` and re-apply this one
; wording change. A string that goes missing is quiet — `makensis` warns
; (`LangString "x" is not set in language table of language English`) and exits
; 0, and CI never runs the bundler — so `cargo test` counts the strings, guards
; the wiring, and guards this wording (`src-tauri/src/release.rs`).
;
; Do not add a UTF-8 BOM here: the bundler writes its own when it copies this
; file into the NSIS build directory, and two BOMs make `makensis` fail with
; `Invalid command: ";"` on line 1 (hit once, 2026-09-29).

LangString addOrReinstall ${LANG_ENGLISH} "Add/Reinstall components"
LangString alreadyInstalled ${LANG_ENGLISH} "Already Installed"
LangString alreadyInstalledLong ${LANG_ENGLISH} "${PRODUCTNAME} ${VERSION} is already installed. Select the operation you want to perform and click Next to continue."
LangString appRunning ${LANG_ENGLISH} "{{product_name}} is running! Please close it first then try again."
LangString appRunningOkKill ${LANG_ENGLISH} "{{product_name}} is running!$\nClick OK to kill it"
LangString chooseMaintenanceOption ${LANG_ENGLISH} "Choose the maintenance option to perform."
LangString choowHowToInstall ${LANG_ENGLISH} "Choose how you want to install ${PRODUCTNAME}."
LangString createDesktop ${LANG_ENGLISH} "Create desktop shortcut"
LangString dontUninstall ${LANG_ENGLISH} "Do not uninstall"
LangString dontUninstallDowngrade ${LANG_ENGLISH} "Do not uninstall (Downgrading without uninstall is disabled for this installer)"
LangString failedToKillApp ${LANG_ENGLISH} "Failed to kill {{product_name}}. Please close it first then try again"
LangString installingWebview2 ${LANG_ENGLISH} "Installing WebView2..."
LangString newerVersionInstalled ${LANG_ENGLISH} "A newer version of ${PRODUCTNAME} is already installed! It is not recommended that you install an older version. If you really want to install this older version, it's better to uninstall the current version first. Select the operation you want to perform and click Next to continue."
LangString older ${LANG_ENGLISH} "older"
LangString olderOrUnknownVersionInstalled ${LANG_ENGLISH} "An $R4 version of ${PRODUCTNAME} is installed on your system. It's recommended that you uninstall the current version before installing. Select the operation you want to perform and click Next to continue."
LangString silentDowngrades ${LANG_ENGLISH} "Downgrades are disabled for this installer, can't proceed with the silent installer, please use the graphical interface installer instead.$\n"
LangString unableToUninstall ${LANG_ENGLISH} "Unable to uninstall!"
LangString uninstallApp ${LANG_ENGLISH} "Uninstall ${PRODUCTNAME}"
LangString uninstallBeforeInstalling ${LANG_ENGLISH} "Uninstall before installing"
LangString unknown ${LANG_ENGLISH} "unknown"
LangString webview2AbortError ${LANG_ENGLISH} "Failed to install WebView2! The app can't run without it. Try restarting the installer."
LangString webview2DownloadError ${LANG_ENGLISH} "Error: Downloading WebView2 Failed - $0"
LangString webview2DownloadSuccess ${LANG_ENGLISH} "WebView2 bootstrapper downloaded successfully"
LangString webview2Downloading ${LANG_ENGLISH} "Downloading WebView2 bootstrapper..."
LangString webview2InstallError ${LANG_ENGLISH} "Error: Installing WebView2 failed with exit code $1"
LangString webview2InstallSuccess ${LANG_ENGLISH} "WebView2 installed successfully"
LangString deleteAppData ${LANG_ENGLISH} "Delete WebView2 browser profile (not your config or logs)"
