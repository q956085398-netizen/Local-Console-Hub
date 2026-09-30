# #63 daily-PowerShell shortcut entry: Windows measurements.
#
# What this script measures is what can be measured from outside the app: how
# many Hubs exist, how many shells appear under the one Hub, where the shortcut
# points, what the launch process exits with, and whether the window is on the
# desktop. It builds its own throwaway shortcut in a temp directory and runs the
# real installer against it, so the entry under test is produced by the tool the
# user is told to use.
#
# Usage:
#   powershell -NoProfile -File scripts\verify-shortcut-entry.ps1
#   powershell -NoProfile -File scripts\verify-shortcut-entry.ps1 -App "E:\Local Console Hub\local-console-hub.exe"
#
# Defaults to the release build in this working tree. To measure the installed
# entry, pass -App with the installed exe.
#
# Build it with `npm run tauri build` (or `npm run tauri build -- --no-bundle`):
# a bare `cargo build --release` leaves out the `custom-protocol` feature, and
# the binary then loads `devUrl` instead of the embedded frontend -- its window
# shows a connection error. The process, window and shell counts below are
# unaffected by that, but a run meant to stand for the product should be made on
# the production build.
#
# Precondition: no Hub is running. The script refuses to run otherwise and says
# how to exit the one that is.
#
# What it cannot prove: which session the *window* selected (the workspace's
# selection lives in the WebView and needs the devtools protocol to read), and
# icon appearance. Those are the manual steps in
# docs/SHORTCUT_ENTRY_ACCEPTANCE.md.

[CmdletBinding()]
param(
    # The entry under test. Defaults to this working tree's release build.
    [string] $App,

    # How long a launch may take to put a window on screen.
    [int] $WaitSeconds = 40,

    # Gap between the two launches of the near-simultaneous pair, in ms.
    [int] $RaceDelayMs = 40,

    # Measure this entry on its own, ignoring other builds of the app that
    # happen to be running. See verify-single-instance.ps1 for the reasoning.
    [switch] $AllowOtherBuilds
)

$ErrorActionPreference = 'Stop'

Add-Type -Namespace Hub -Name Win -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
[DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr lParam);
[DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool IsIconic(IntPtr hWnd);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, System.Text.StringBuilder name, int max);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextLength(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
[DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr lParam);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder text, int max);
'@

$WM_CLOSE = 0x0010

$results = [System.Collections.Generic.List[object]]::new()

function Add-Result([string] $Check, [string] $Expected, [string] $Actual, [bool] $Pass) {
    $results.Add([pscustomobject]@{
            Check    = $Check
            Expected = $Expected
            Actual   = $Actual
            Result   = if ($Pass) { 'PASS' } else { 'FAIL' }
        })
    $label = if ($Pass) { 'PASS' } else { 'FAIL' }
    $colour = if ($Pass) { 'Green' } else { 'Red' }
    Write-Host "  [$label] $Check" -ForegroundColor $colour
    Write-Host "         expected: $Expected" -ForegroundColor DarkGray
    Write-Host "         actual:   $Actual" -ForegroundColor DarkGray
}

function Write-Note([string] $Text) {
    Write-Host "  ($Text)" -ForegroundColor DarkGray
}

function Get-AllHubProcesses {
    @(Get-Process -Name 'local-console-hub' -ErrorAction SilentlyContinue)
}

function Get-HubProcesses {
    if ($AllowOtherBuilds) {
        @(Get-AllHubProcesses | Where-Object { $_.Path -eq $script:entryPath })
    } else {
        Get-AllHubProcesses
    }
}

# How many interactive shells are running under this Hub, in total.
#
# A temporary terminal is a ConPTY session: the Hub owns the shell as its
# child, and a `conhost` sits beside it. Shells are counted rather than
# descendants as a whole, because "the shortcut made a terminal" is a statement
# about shells -- and conhost's own process tree is an implementation detail of
# how the pseudoconsole is hosted.
function Get-Shells {
    $all = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
            Select-Object ProcessId, ParentProcessId, Name)
    $stack = [System.Collections.Generic.Stack[int]]::new()
    $stack.Push($script:hubPid)
    $found = [System.Collections.Generic.List[object]]::new()
    while ($stack.Count -gt 0) {
        $current = $stack.Pop()
        foreach ($child in $all | Where-Object { $_.ParentProcessId -eq $current }) {
            if ($child.Name -match '^(powershell|pwsh)\.exe$') {
                $found.Add($child)
            }
            $stack.Push([int]$child.ProcessId)
        }
    }
    return $found
}

function Wait-ForShellCount([int] $Expected, [int] $Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        $shells = @(Get-Shells)
        if ($shells.Count -ge $Expected) { return $shells }
        Start-Sleep -Milliseconds 250
    }
    return @(Get-Shells)
}

function Get-ProcessWindows([int] $ProcessId) {
    $found = [System.Collections.Generic.List[object]]::new()
    $callback = [Hub.Win+EnumWindowsProc] {
        param($hWnd, $lParam)
        $owner = [uint32]0
        [Hub.Win]::GetWindowThreadProcessId($hWnd, [ref]$owner) | Out-Null
        if ($owner -eq [uint32]$ProcessId) {
            $class = [System.Text.StringBuilder]::new(256)
            [Hub.Win]::GetClassName($hWnd, $class, 256) | Out-Null
            $rect = [Hub.Win+RECT]::new()
            [Hub.Win]::GetWindowRect($hWnd, [ref]$rect) | Out-Null
            $found.Add([pscustomobject]@{
                    Handle  = $hWnd
                    Class   = $class.ToString()
                    Visible = [Hub.Win]::IsWindowVisible($hWnd)
                    Iconic  = [Hub.Win]::IsIconic($hWnd)
                    Width   = $rect.Right - $rect.Left
                    Height  = $rect.Bottom - $rect.Top
                    Titled  = ([Hub.Win]::GetWindowTextLength($hWnd) -gt 0)
                })
        }
        return $true
    }
    [Hub.Win]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    return $found
}

function Get-MainWindow([int] $ProcessId) {
    $windows = @(Get-ProcessWindows $ProcessId |
            Where-Object { $_.Titled -and ($_.Iconic -or ($_.Width -gt 100 -and $_.Height -gt 100)) } |
            Sort-Object -Property { $_.Width * $_.Height } -Descending)
    if ($windows.Count -eq 0) { return $null }
    return $windows[0]
}

function Wait-ForWindow([int] $ProcessId) {
    $deadline = (Get-Date).AddSeconds($WaitSeconds)
    while ((Get-Date) -lt $deadline) {
        $window = Get-MainWindow $ProcessId
        if ($window) { return $window }
        Start-Sleep -Milliseconds 250
    }
    return $null
}

function Wait-ForExit($Process, [int] $Seconds) {
    if ($Process.WaitForExit($Seconds * 1000)) { return $Process.ExitCode }
    return $null
}

# The message box a process is showing, if any.
#
# A request the Hub could not carry out is reported in a native message box
# (`crate::dialog`), so "the user is told why" is measurable from outside: the
# box is a `#32770` window owned by the process that was asked, and its text
# says which directory could not be opened. Reading it is how this acceptance
# tells "refused with a reason" apart from "silently did something else".
function Get-DialogWindows([int] $ProcessId) {
    $found = [System.Collections.Generic.List[object]]::new()
    $callback = [Hub.Win+EnumWindowsProc] {
        param($hWnd, $lParam)
        $owner = [uint32]0
        [Hub.Win]::GetWindowThreadProcessId($hWnd, [ref]$owner) | Out-Null
        if ($owner -eq [uint32]$ProcessId) {
            $class = [System.Text.StringBuilder]::new(256)
            [Hub.Win]::GetClassName($hWnd, $class, 256) | Out-Null
            if ($class.ToString() -eq '#32770') {
                $found.Add([pscustomobject]@{ Handle = $hWnd })
            }
        }
        return $true
    }
    [Hub.Win]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    return $found
}

function Get-DialogText([IntPtr] $Dialog) {
    $text = [System.Text.StringBuilder]::new(4096)
    $callback = [Hub.Win+EnumWindowsProc] {
        param($hWnd, $lParam)
        $child = [System.Text.StringBuilder]::new(4096)
        [Hub.Win]::GetWindowText($hWnd, $child, 4096) | Out-Null
        if ($child.Length -gt 0) { [void]$text.AppendLine($child.ToString()) }
        return $true
    }
    [Hub.Win]::EnumChildWindows($Dialog, $callback, [IntPtr]::Zero) | Out-Null
    return $text.ToString().Trim()
}

function Wait-ForDialog([int] $ProcessId, [int] $Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        $dialogs = @(Get-DialogWindows $ProcessId)
        if ($dialogs.Count -gt 0) { return $dialogs[0] }
        Start-Sleep -Milliseconds 250
    }
    return $null
}

function Close-Dialog([IntPtr] $Dialog) {
    [Hub.Win]::PostMessage($Dialog, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
}

# Launch a shortcut, the way a double-click does.
function Open-Entry([string] $Path) {
    $process = Start-Process -FilePath $Path -PassThru
    $started.Add($process.Id)
    return $process
}

function New-Entry([string] $Path, [string] $Target, [string] $Arguments, [string] $StartIn) {
    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($Path)
    $link.TargetPath = $Target
    $link.Arguments = $Arguments
    $link.WorkingDirectory = $StartIn
    $link.Save()
}

function Read-Entry([string] $Path) {
    $shell = New-Object -ComObject WScript.Shell
    return $shell.CreateShortcut($Path)
}

$started = [System.Collections.Generic.List[int]]::new()

function Stop-Started {
    foreach ($id in @($started)) {
        Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
    }
    for ($i = 0; $i -lt 40 -and (Get-HubProcesses).Count -gt 0; $i++) { Start-Sleep -Milliseconds 250 }
    $started.Clear()
}

# --- The entry's identity ----------------------------------------

Write-Host ''
Write-Host '== #63 daily-PowerShell shortcut entry: Windows measurements ==' -ForegroundColor Cyan
Write-Host ''

$repo = Split-Path -Parent $PSScriptRoot
if (-not $App) {
    $App = Join-Path $repo 'src-tauri\target\release\local-console-hub.exe'
}
if (-not (Test-Path -LiteralPath $App)) {
    Write-Host "The entry under test does not exist: $App" -ForegroundColor Red
    Write-Host 'Build it first (npm run tauri build, or cargo build --release), or point -App at an installed exe.' -ForegroundColor Red
    exit 2
}

$exe = Get-Item -LiteralPath $App
$script:entryPath = $exe.FullName
$installer = Join-Path $PSScriptRoot 'install-powershell-shortcut.ps1'
if (-not (Test-Path -LiteralPath $installer)) {
    Write-Host "The installer under test is missing: $installer" -ForegroundColor Red
    exit 2
}

$startMenu = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Local Console Hub.lnk'
$startMenuTarget = if (Test-Path -LiteralPath $startMenu) { (Read-Entry $startMenu).TargetPath } else { '(not installed)' }

Write-Host 'Entry under test'
Write-Host "  path      : $($exe.FullName)"
Write-Host "  sha256    : $((Get-FileHash -LiteralPath $exe.FullName -Algorithm SHA256).Hash)"
Write-Host "  installer : $installer"
Write-Host "  the user's own Start menu shortcut points at: $startMenuTarget"
Write-Host ''

$foreign = @(Get-AllHubProcesses)
if ($foreign.Count -gt 0 -and -not $AllowOtherBuilds) {
    Write-Host 'A Hub is already running, so this acceptance refuses to run.' -ForegroundColor Yellow
    Write-Host 'Exit that Hub from its tray menu first, then run this script again.' -ForegroundColor Yellow
    $foreign | Select-Object Id, Path | Format-Table -AutoSize | Out-String | Write-Host
    exit 2
}

# --- The throwaway entry, built by the installer ------------------

Write-Host '== The entry, built by scripts\install-powershell-shortcut.ps1 ==' -ForegroundColor Cyan

$root = Join-Path $env:TEMP 'lch-t63-shortcut-entry'
if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }

# The directory name is spelled as code points rather than written out, and
# this script stays pure ASCII like every other one here (the repository rule
# recorded in docs/SINGLE_INSTANCE_ACCEPTANCE.md section 5). Windows PowerShell
# 5.1 reads a BOM-less script with the *ANSI code page*, so a literal CJK
# character in this file would be mojibake in memory -- the entry would name a
# different directory, and a check called "Unicode intact" would be measuring
# the wrong name. U+5DE5 U+4F5C = "work", U+76EE U+5F55 = "directory".
$cjk = [string][char]0x5DE5 + [char]0x4F5C + ' ' + [char]0x76EE + [char]0x5F55
$work = Join-Path $root "lch-t63 $cjk"
New-Item -ItemType Directory -Path $work -Force | Out-Null

$entry = Join-Path $root 'Daily PowerShell.lnk'
$dailyShell = (Get-Process -Id $PID).Path
# What the user's shortcut looks like *before*: a shell, starting in a
# directory they chose. The installer's job is to replace exactly this.
New-Entry -Path $entry -Target $dailyShell -Arguments '-NoProfile' -StartIn $work

# The installer runs as a real child process, exactly as the user would run
# it, and its output is read from a file rather than a pipeline: `Write-Host`
# goes to the console host, which a pipeline does not capture.
$installerLog = Join-Path $root 'installer.log'
$install = Start-Process -FilePath 'powershell' -PassThru -Wait `
    -ArgumentList "-NoLogo -NoProfile -File `"$installer`" -Shortcut `"$entry`" -Hub `"$($exe.FullName)`"" `
    -RedirectStandardOutput $installerLog
$installerExit = $install.ExitCode
$installerOutput = if (Test-Path -LiteralPath $installerLog) { Get-Content -LiteralPath $installerLog -Raw } else { '' }
$installed = Read-Entry $entry
Write-Note "installer exit code: $installerExit"
Write-Note "installer said: $(($installerOutput -split "`r?`n" | Where-Object { $_ -match '^\s+(now|starts in|saved)' }) -join ' | ')"

Add-Result 'the installer rewrites the chosen shortcut at the Hub' $exe.FullName `
    "$($installed.TargetPath)" ($installerExit -eq 0 -and $installed.TargetPath -eq $exe.FullName)
Add-Result 'the request is a new-terminal request' '--new-terminal' "$($installed.Arguments)" `
    ($installed.Arguments -match '--new-terminal')
Add-Result 'the directory travels as one quoted argument (space and Unicode intact)' $work `
    "$($installed.Arguments)" ($installed.Arguments -match [Regex]::Escape($work))
Add-Result 'the original shortcut was saved beside itself' 'a .lch-original.lnk beside the entry' `
    "$(Test-Path -LiteralPath (Join-Path $root 'Daily PowerShell.lch-original.lnk'))" `
    (Test-Path -LiteralPath (Join-Path $root 'Daily PowerShell.lch-original.lnk'))
Add-Result "the user's own Start menu shortcut is untouched" $startMenuTarget `
    "$(if (Test-Path -LiteralPath $startMenu) { (Read-Entry $startMenu).TargetPath } else { '(not installed)' })" `
    ((Read-Entry $startMenu).TargetPath -eq $startMenuTarget -or -not (Test-Path -LiteralPath $startMenu))

if ($installerExit -ne 0) {
    # Everything below measures *the entry the installer built*. If the
    # installer refused, there is no such entry, and carrying on would report a
    # pile of failures about the Hub instead of the one real problem.
    Write-Host ''
    Write-Host 'Stopping: the installer did not produce an entry, so there is nothing to measure.' -ForegroundColor Red
    Write-Host $installerOutput -ForegroundColor Red
    Stop-Started
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
    exit 1
}
Write-Host ''

# --- The entry the user actually has, copied ---------------------

Write-Host "== The user's own PowerShell entry, copied and converted ==" -ForegroundColor Cyan

# A copy of the machine's real PowerShell shortcut, so the installer is run
# against the shape a user's entry really has. Its "Start in" is
# `%HOMEDRIVE%%HOMEPATH%` rather than a path -- which is exactly the case the
# installer has to expand, and the one a hand-made fixture would never cover.
# The original is only read, never written: the entry the user chose is the one
# this acceptance must not touch.
$daily = @(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'),
    (Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs')
) |
    Where-Object { Test-Path -LiteralPath $_ } |
    ForEach-Object { Get-ChildItem -LiteralPath $_ -Recurse -Filter '*PowerShell*.lnk' -ErrorAction SilentlyContinue } |
    Where-Object { (Read-Entry $_.FullName).TargetPath -match '(powershell|pwsh)\.exe$' } |
    Select-Object -First 1

if (-not $daily) {
    Add-Result "the entry the user actually has" 'a Windows PowerShell shortcut on this machine' '(none found)' $false
} else {
    $copy = Join-Path $root $daily.Name
    Copy-Item -LiteralPath $daily.FullName -Destination $copy -Force
    $original = Read-Entry $copy
    $converted = Start-Process -FilePath 'powershell' -PassThru -Wait `
        -ArgumentList "-NoLogo -NoProfile -File `"$installer`" -Shortcut `"$copy`" -Hub `"$($exe.FullName)`"" `
        -RedirectStandardOutput (Join-Path $root 'copy-installer.log')
    $after = Read-Entry $copy
    $named = if ($after.Arguments -match '--directory\s+(.+)$') { $Matches[1].Trim('"') } else { '' }

    Add-Result "the user's entry keeps its meaning: its start directory is expanded" `
        'an existing directory, not the variable text' `
        "was `"$($original.WorkingDirectory)`", now directory=`"$named`"" `
        ($converted.ExitCode -eq 0 -and $named -ne '' -and (Test-Path -LiteralPath $named -PathType Container) -and $named -notmatch '%')
    Add-Result "the user's own shortcut file is still theirs" 'unchanged on disk' `
        "$($daily.FullName) still points at $((Read-Entry $daily.FullName).TargetPath)" `
        ((Read-Entry $daily.FullName).TargetPath -eq $original.TargetPath)
}
Write-Host ''

# --- H04: cold start ---------------------------------------------

Write-Host '== H04 cold start: the shortcut with no Hub running ==' -ForegroundColor Cyan

$cold = Open-Entry $entry
Start-Sleep -Seconds 1
$window = Wait-ForWindow $cold.Id
$script:hubPid = $cold.Id
$shells = @(Wait-ForShellCount 1 $WaitSeconds)

$hubCount = (Get-HubProcesses).Count
Add-Result 'H04 the Hub starts, exactly one process' '1' "$hubCount" ($hubCount -eq 1)
Add-Result 'H04 the window is on the desktop' 'a visible, titled top-level window' `
    $(if ($window) { "hwnd=$($window.Handle) visible=$($window.Visible)" } else { '(none)' }) `
    ($null -ne $window)
Add-Result 'H04 the shortcut created one terminal inside it' '1 shell' `
    "$($shells.Count)$(if ($shells.Count) { ': ' + (($shells | ForEach-Object { "$($_.Name)($($_.ProcessId))" }) -join ', ') })" `
    ($shells.Count -eq 1)
Write-Host ''

# --- H04: a Hub that is already running --------------------------

Write-Host '== H04 with the Hub already running ==' -ForegroundColor Cyan

$second = Open-Entry $entry
$exit = Wait-ForExit $second $WaitSeconds
$shells = @(Wait-ForShellCount 2 $WaitSeconds)

Add-Result 'H04 the launch is handed to the running Hub and ends' 'exit code 0' `
    $(if ($null -eq $exit) { "still running after $WaitSeconds s" } else { "exit code $exit" }) ($exit -eq 0)
Add-Result 'H04 still exactly one Hub' '1' "$((Get-HubProcesses).Count)" ((Get-HubProcesses).Count -eq 1)
Add-Result 'H04 the second click added exactly one more terminal' '2 shells' "$($shells.Count)" ($shells.Count -eq 2)
Write-Host ''

# --- H04: a Hub hidden in the tray -------------------------------

Write-Host '== H04 with the Hub hidden in the tray ==' -ForegroundColor Cyan

$window = Get-MainWindow $script:hubPid
if (-not $window) {
    Add-Result 'H04 the shortcut restores the window and adds a terminal' 'the Hub from the cold start is still there' '(no window to hide; the Hub is gone)' $false
    $failed = @($results | Where-Object { $_.Result -eq 'FAIL' })
    Write-Host ''
    Write-Host ("{0} checks: {1} PASS / {2} FAIL" -f $results.Count, ($results.Count - $failed.Count), $failed.Count) -ForegroundColor Red
    Write-Host 'Stopping: nothing below can be measured without a running Hub.' -ForegroundColor Red
    Stop-Started
    exit 1
}
[Hub.Win]::PostMessage($window.Handle, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Milliseconds 1500
$hidden = Get-MainWindow $script:hubPid
Add-Result 'the Hub is off the desktop (closed to the tray)' 'visible=False, still one Hub' `
    "$(if ($hidden) { "visible=$($hidden.Visible)" } else { '(none)' }), Hub processes=$((Get-HubProcesses).Count)" `
    ($null -ne $hidden -and -not $hidden.Visible -and (Get-HubProcesses).Count -eq 1)

$third = Open-Entry $entry
$exit = Wait-ForExit $third $WaitSeconds
Start-Sleep -Milliseconds 1500
$restored = Get-MainWindow $script:hubPid
$shells = @(Wait-ForShellCount 3 $WaitSeconds)

Add-Result 'H04 the shortcut restores the window and adds a terminal' 'visible again, exit code 0, 3 shells' `
    "$(if ($restored) { "visible=$($restored.Visible)" } else { '(none)' }) exit code=$exit shells=$($shells.Count)" `
    ($null -ne $restored -and $restored.Visible -and $exit -eq 0 -and $shells.Count -eq 3)
Write-Host ''

# --- H04: two clicks at almost the same moment -------------------

Write-Host '== H04 two independent clicks, almost together ==' -ForegroundColor Cyan

$first = Open-Entry $entry
Start-Sleep -Milliseconds $RaceDelayMs
$another = Open-Entry $entry
$firstExit = Wait-ForExit $first $WaitSeconds
$anotherExit = Wait-ForExit $another $WaitSeconds
$shells = @(Wait-ForShellCount 5 $WaitSeconds)

Add-Result 'each independent click adds its own shell' '5 shells, both launches exit 0' `
    "$($shells.Count) shells, exit codes $(if ($null -eq $firstExit) { 'running' } else { $firstExit })/$(if ($null -eq $anotherExit) { 'running' } else { $anotherExit })" `
    ($shells.Count -eq 5 -and $firstExit -eq 0 -and $anotherExit -eq 0)
Add-Result 'the single-instance rule did not swallow the second request' '1 Hub' `
    "$((Get-HubProcesses).Count)" ((Get-HubProcesses).Count -eq 1)
Write-Host ''

# --- H06: a directory that is not there --------------------------

Write-Host '== H06 a directory that does not exist ==' -ForegroundColor Cyan

$missing = Join-Path $root 'lch-t63 no such directory'
$badEntry = Join-Path $root 'Stale PowerShell.lnk'
# A shortcut that named a directory which has since been deleted -- what a
# user's own entry looks like after they move or remove the folder.
New-Entry -Path $badEntry -Target $exe.FullName `
    -Arguments ('--new-terminal --directory "{0}"' -f $missing) -StartIn $root

$before = @(Get-Shells).Count
$bad = Open-Entry $badEntry
$dialog = Wait-ForDialog $bad.Id 20
$dialogText = if ($dialog) { Get-DialogText $dialog.Handle } else { '(no dialog)' }
if ($dialog) { Close-Dialog $dialog.Handle }
$badExit = Wait-ForExit $bad 20
Start-Sleep -Seconds 2
$after = @(Get-Shells).Count

Add-Result 'a directory that is not there is refused with the reason' 'a message box naming that directory' `
    "$(if ($dialog) { "shown: $($dialogText -replace '\s+', ' ')" } else { '(no dialog)' })" `
    ($null -ne $dialog -and $dialogText.Contains($missing))
Add-Result 'the refusal is reported to the process that asked' 'exit code 1' `
    "$(if ($null -eq $badExit) { 'still running' } else { "exit code $badExit" })" ($badExit -eq 1)
Add-Result 'nothing was opened somewhere else instead' 'no new shell' `
    "shells $before -> $after" ($after -eq $before)
Write-Host ''

# The same entry against a Hub that is not running yet: the Hub starts, says
# why it could not make the terminal, and stays open. A Hub that exited or
# opened a shell anyway would be the silent fallback spec #59 decision 5
# forbids.
Write-Host '== H06 the same entry with no Hub running ==' -ForegroundColor Cyan

Stop-Started
Add-Result 'cleared before the cold start' '0 Hubs' "$((Get-HubProcesses).Count)" ((Get-HubProcesses).Count -eq 0)

$coldBad = Open-Entry $badEntry
$script:hubPid = $coldBad.Id
$window = Wait-ForWindow $coldBad.Id
$dialog = Wait-ForDialog $coldBad.Id 20
$dialogText = if ($dialog) { Get-DialogText $dialog.Handle } else { '(no dialog)' }
if ($dialog) { Close-Dialog $dialog.Handle }
Start-Sleep -Seconds 2

Add-Result 'cold: the Hub opens, names the directory, and starts no terminal' `
    'window on screen, a message box naming the directory, 0 shells' `
    "window=$(if ($window) { 'yes' } else { 'none' }), dialog=$(if ($dialog) { $dialogText -replace '\s+', ' ' } else { 'none' }), shells=$(@(Get-Shells).Count)" `
    ($null -ne $window -and $null -ne $dialog -and $dialogText.Contains($missing) -and @(Get-Shells).Count -eq 0)
Add-Result 'cold: the Hub stays open after refusing' '1 Hub, still running' `
    "$((Get-HubProcesses).Count) ($(if ($coldBad.HasExited) { 'exited' } else { 'running' }))" `
    ((Get-HubProcesses).Count -eq 1 -and -not $coldBad.HasExited)
Write-Host ''

# --- The ordinary entry is still the ordinary open ---------------

Write-Host '== The ordinary Hub entry still only restores the window ==' -ForegroundColor Cyan

$before = @(Get-Shells).Count
$plain = Open-Entry $exe.FullName
$plainExit = Wait-ForExit $plain $WaitSeconds
Start-Sleep -Seconds 2
$after = @(Get-Shells).Count

Add-Result 'opening the Hub itself starts no terminal' 'no new shell' `
    "exit code=$(if ($null -eq $plainExit) { 'running' } else { $plainExit }), shells $before -> $after" `
    ($plainExit -eq 0 -and $after -eq $before)
Add-Result 'opening the Hub itself leaves exactly one Hub' '1' "$((Get-HubProcesses).Count)" ((Get-HubProcesses).Count -eq 1)
Write-Host ''

# --- H17: everything else keeps opening its own PowerShell -------

Write-Host '== H17 PowerShell started by anything else is untouched ==' -ForegroundColor Cyan

$shellHash = (Get-FileHash -LiteralPath $dailyShell -Algorithm SHA256).Hash
$probe = Start-Process -FilePath $dailyShell -ArgumentList '-NoProfile', '-Command', 'exit 7' -PassThru -Wait
$owner = (Get-CimInstance Win32_Process -Filter "ProcessId=$($probe.Id)" -ErrorAction SilentlyContinue).ParentProcessId

Add-Result 'the shell executable itself is unchanged' 'the same file, same contents' `
    "$dailyShell sha256=$($shellHash.Substring(0, 12))..." ($shellHash -eq (Get-FileHash -LiteralPath $dailyShell -Algorithm SHA256).Hash)
Add-Result 'a PowerShell started outside the Hub behaves as it always did' 'exit code 7, its own process' `
    "exit code $($probe.ExitCode)" ($probe.ExitCode -eq 7)
Add-Result 'the Hub is not its handler' 'not a child of the Hub' `
    "parent pid=$owner, Hub pid=$($script:hubPid)" ($owner -ne $script:hubPid)
Write-Host ''

# --- Cleanup -----------------------------------------------------

Write-Host 'Cleanup: ending the Hub processes this script started.' -ForegroundColor DarkGray
Stop-Started
# Tolerant on purpose: a terminal the Hub made keeps its shell's working
# directory open for a moment after the forced end, and a temp directory that
# outlives the run is not a failing acceptance criterion.
Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
Add-Result 'no Hub process left behind' '0' "$((Get-HubProcesses).Count)" ((Get-HubProcesses).Count -eq 0)

# --- Summary -----------------------------------------------------

Write-Host ''
Write-Host '== Summary ==' -ForegroundColor Cyan
$failed = @($results | Where-Object { $_.Result -eq 'FAIL' })
$results | Format-Table -AutoSize Check, Expected, Actual, Result | Out-String -Width 220 | Write-Host
Write-Host ("{0} checks: {1} PASS / {2} FAIL" -f $results.Count, ($results.Count - $failed.Count), $failed.Count) `
    -ForegroundColor $(if ($failed.Count -eq 0) { 'Green' } else { 'Red' })
Write-Host ''
Write-Host 'What this proves: what the installed shortcut points at, how many Hubs and shells one' -ForegroundColor DarkGray
Write-Host 'click produces from cold, warm and hidden states, what an invalid directory does, and' -ForegroundColor DarkGray
Write-Host 'that a shell started outside the Hub is untouched.' -ForegroundColor DarkGray
Write-Host 'What it cannot prove: which session the window selected (needs the devtools protocol),' -ForegroundColor DarkGray
Write-Host 'and icon appearance. Those are the manual steps in docs/SHORTCUT_ENTRY_ACCEPTANCE.md.' -ForegroundColor DarkGray

if ($failed.Count -gt 0) { exit 1 } else { exit 0 }
