# #60 single-instance entry: Windows measurements.
#
# This script measures the part that can be measured: process counts, exit codes,
# window visibility, whether a terminal came with it, and the entry's own PE
# subsystem and console. It never touches a session and never ends a Hub it did
# not start.
#
# Usage (or right-click -> Run with PowerShell):
#   powershell -NoProfile -File scripts\verify-single-instance.ps1
#   powershell -NoProfile -File scripts\verify-single-instance.ps1 -App "E:\Local Console Hub\local-console-hub.exe"
#
# Defaults to the release build in this working tree. To verify the *installed*
# daily entry (what the Start menu shortcut points at), reinstall first and pass
# -App, or double-click verify-single-instance.cmd.
#
# Precondition: no Hub is running. The script refuses to run otherwise and says
# how to exit the one that is.

[CmdletBinding()]
param(
    # The entry under test. Defaults to this working tree's release build.
    [string] $App,

    # How long a launch may take to put a window on screen. A first WebView2
    # start on a cold machine is the slow case.
    [int] $WaitSeconds = 40,

    # Gap between the two launches of the cold-start race, in milliseconds.
    [int] $RaceDelayMs = 40,

    # Measure this entry on its own, ignoring other builds of the app that happen
    # to be running (another working tree's development instance, an older copy).
    #
    # Every count below then means "processes of the entry under test", and the
    # refusal at the top becomes a warning. Off by default: with nothing else
    # running, the strict reading -- one Hub process for the whole session -- is
    # the one worth measuring, and it is what the spec asks for.
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
[DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int cmd);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hWnd, System.Text.StringBuilder name, int max);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextLength(IntPtr hWnd);
[DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
'@

$SW_MINIMIZE = 6
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

# The Hub processes this run is about.
#
# Strictly that is every process of that name: the claim is one Hub for the
# whole session. With -AllowOtherBuilds it is narrowed to the entry under test,
# which is the honest scope when another build happens to be running -- a
# development instance from another working tree, or an older copy that predates
# this change and could not take part in the claim even if it wanted to.
function Get-HubProcesses {
    if ($AllowOtherBuilds) {
        @(Get-AllHubProcesses | Where-Object { $_.Path -eq $script:entryPath })
    } else {
        Get-AllHubProcesses
    }
}

# Every top-level window of one process.
#
# `MainWindowHandle` only looks at a "main" window and is unreliable for hidden
# ones; what this acceptance has to look at is precisely the hidden or minimized
# window, so the windows are enumerated directly.
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

# The main window: the largest top-level window with a title that looks like a
# real window.
#
# Measured on Windows 11: a Tauri process owns nine top-level windows. Besides
# the main one there are the tray's message window, tao's event-target window,
# and several input-method helpers -- MSCTFIME UI, IME, SoPY_*, Sogou_* -- and
# those carry non-empty titles too. Selecting on "has a title" alone picks one of
# them (this acceptance picked the IME window until it did not).
#
# So: a sensible size counts, and so does being minimized (a minimized window
# reports the icon-sized rect from GetWindowRect, so `iconic` is the only way to
# recognize it). The 0x0 helpers satisfy neither.
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

# Does this Hub own a console host or a shell process?
#
# The measurable form of "a normal open starts no PowerShell": walk this Hub's
# descendants and look for conhost / powershell / pwsh / cmd. Scoped to the Hub
# itself, so shells that were already open on this machine cannot affect the
# answer.
function Get-TerminalDescendants([int] $ProcessId) {
    $all = @(Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
            Select-Object ProcessId, ParentProcessId, Name)
    $stack = [System.Collections.Generic.Stack[int]]::new()
    $stack.Push($ProcessId)
    $found = [System.Collections.Generic.List[string]]::new()
    while ($stack.Count -gt 0) {
        $current = $stack.Pop()
        foreach ($child in $all | Where-Object { $_.ParentProcessId -eq $current }) {
            if ($child.Name -match '^(powershell|pwsh|cmd|conhost)\.exe$') {
                $found.Add("$($child.Name)($($child.ProcessId))")
            }
            $stack.Push([int]$child.ProcessId)
        }
    }
    return $found
}

# Does this process own a console object?
#
# Asked in a *child* PowerShell: answering it means FreeConsole + AttachConsole,
# and that would detach this process's own console -- the parent still has
# results to print. The child answers with its exit code: 0 = has a console,
# 1 = has none.
#
# The child's code travels as `-EncodedCommand` (base64): that bypasses command
# line quoting entirely, so the double quotes inside the DllImport attributes
# survive (`-Command` strips a layer, and Add-Type then reports "type name
# kernel32 could not be found").
function Test-HasConsole([int] $ProcessId) {
    $code = @"
Add-Type -Namespace P -Name N -MemberDefinition '[DllImport("kernel32.dll", SetLastError=true)] public static extern bool FreeConsole(); [DllImport("kernel32.dll", SetLastError=true)] public static extern bool AttachConsole(uint pid);'
[P.N]::FreeConsole() | Out-Null
if ([P.N]::AttachConsole([uint32]$ProcessId)) { exit 0 }
exit 1
"@
    $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($code))
    $global:LASTEXITCODE = 2
    try {
        & powershell -NoProfile -EncodedCommand $encoded 2>&1 | Out-Null
    } catch {
        return $null
    }
    switch ($LASTEXITCODE) {
        0 { return $true }
        1 { return $false }
        default { return $null }
    }
}

function Wait-ForExit($Process, [int] $Seconds) {
    if ($Process.WaitForExit($Seconds * 1000)) { return $Process.ExitCode }
    return $null
}

# The Hub processes this script started.
#
# Cleanup ends these pids only, never a sweep by process name: a same-named
# process may be the user's own Hub (the installed one the Start menu shortcut
# points at), and that one may be supervising their sessions. The refusal at the
# top of the script is the other half of the same rule.
$started = [System.Collections.Generic.List[int]]::new()

# Cleanup is a forced end, not the tray's Exit: this script never starts a
# session, so the Hubs it ends have only ever been opened. The real exit path
# (stop everything, confirm, leave) is covered by the #54/#57 acceptance.
function Stop-Started {
    foreach ($id in @($started)) {
        Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
    }
    for ($i = 0; $i -lt 40 -and (Get-HubProcesses).Count -gt 0; $i++) { Start-Sleep -Milliseconds 250 }
    $started.Clear()
}

function Open-Hub([System.IO.FileInfo] $Exe) {
    $process = Start-Process -FilePath $Exe.FullName -PassThru
    $started.Add($process.Id)
    return $process
}

# --- The entry's identity ----------------------------------------

Write-Host ''
Write-Host '== #60 single-instance entry: Windows measurements ==' -ForegroundColor Cyan
Write-Host ''

if (-not $App) {
    $repo = Split-Path -Parent $PSScriptRoot
    $App = Join-Path $repo 'src-tauri\target\release\local-console-hub.exe'
}

if (-not (Test-Path -LiteralPath $App)) {
    Write-Host "The entry under test does not exist: $App" -ForegroundColor Red
    Write-Host 'Build it first (npm run tauri build, or cargo build --release), or point -App at an installed exe.' -ForegroundColor Red
    exit 2
}

$exe = Get-Item -LiteralPath $App
$bytes = [System.IO.File]::ReadAllBytes($exe.FullName)
$peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
$optionalHeader = $peOffset + 24
$magic = [BitConverter]::ToUInt16($bytes, $optionalHeader)
$subsystem = [BitConverter]::ToUInt16($bytes, $optionalHeader + $(if ($magic -eq 0x20b) { 68 } else { 66 }))
$subsystemName = switch ($subsystem) { 2 { 'WINDOWS_GUI' } 3 { 'WINDOWS_CUI' } default { "other($subsystem)" } }

$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Local Console Hub.lnk'
$shortcutTarget = $null
if (Test-Path -LiteralPath $shortcut) {
    $shell = New-Object -ComObject WScript.Shell
    $shortcutTarget = $shell.CreateShortcut($shortcut).TargetPath
}

Write-Host 'Entry under test'
Write-Host "  path      : $($exe.FullName)"
Write-Host "  size      : $($exe.Length) bytes"
Write-Host "  sha256    : $((Get-FileHash -LiteralPath $exe.FullName -Algorithm SHA256).Hash)"
Write-Host "  subsystem : $subsystemName"
Write-Host "  Start menu shortcut points at: $(if ($shortcutTarget) { $shortcutTarget } else { '(not installed)' })"
Write-Host ''

$script:entryPath = $exe.FullName
$foreign = @(Get-AllHubProcesses)
if ($foreign.Count -gt 0 -and -not $AllowOtherBuilds) {
    Write-Host 'A Hub is already running, so this acceptance refuses to run.' -ForegroundColor Yellow
    Write-Host 'Exit that Hub from its tray menu first (it handles its sessions by its own rules), then run this script.' -ForegroundColor Yellow
    Write-Host 'If it is another build and you want this entry measured on its own, pass -AllowOtherBuilds.' -ForegroundColor Yellow
    $foreign | Select-Object Id, Path | Format-Table -AutoSize | Out-String | Write-Host
    exit 2
}
if ($foreign.Count -gt 0) {
    Write-Host '-AllowOtherBuilds: another build is running; every count below is scoped to this entry.' -ForegroundColor Yellow
    $foreign | Select-Object Id, Path | Format-Table -AutoSize | Out-String | Write-Host
}

# --- H01: opening the daily entry --------------------------------

Write-Host '== H01 opening the daily entry ==' -ForegroundColor Cyan

$first = Open-Hub $exe
$window = Wait-ForWindow $first.Id
Start-Sleep -Seconds 2
$first.Refresh()

$hubCount = (Get-HubProcesses).Count
Add-Result 'H01 exactly one Hub process after opening' '1' "$hubCount" ($hubCount -eq 1)
Add-Result 'H01 the Hub process is alive and responding' 'HasExited=False, Responding=True' `
    "HasExited=$($first.HasExited), Responding=$($first.Responding)" `
    (-not $first.HasExited -and $first.Responding)
Add-Result 'H01 the entry is not a console program' 'WINDOWS_GUI' "$subsystemName" ($subsystem -eq 2)
Add-Result 'H01 a window exists' 'at least one titled top-level window' `
    $(if ($window) { "hwnd=$($window.Handle) class=$($window.Class) $($window.Width)x$($window.Height)" } else { '(none)' }) `
    ($null -ne $window)

$hasConsole = Test-HasConsole $first.Id
Add-Result 'H01 the Hub process owns no console (no extra terminal on the taskbar)' 'False' "$hasConsole" ($hasConsole -eq $false)

# The other half of that check: no console host or shell in its own tree either.
# Scoped to this Hub, so shells already open elsewhere on the machine neither
# count nor flake the result.
$own = @(Get-TerminalDescendants $first.Id)
Add-Result 'H01 no conhost or shell in the Hub process tree' '0' "$($own.Count)$(if ($own.Count) { ': ' + ($own -join ', ') })" ($own.Count -eq 0)

$parent = (Get-CimInstance Win32_Process -Filter "ProcessId=$($first.Id)").ParentProcessId
Write-Note "process identity: pid=$($first.Id) parent=$parent path=$($exe.FullName)"
Write-Note "entry used: the script started `"$($exe.FullName)`"; the Start menu shortcut points at `"$shortcutTarget`""
Write-Host ''

# --- H02: opening the same entry again ---------------------------

Write-Host '== H02 opening the same entry again ==' -ForegroundColor Cyan

$terminalsBefore = @(Get-TerminalDescendants $first.Id)

$second = Open-Hub $exe
$exit = Wait-ForExit $second $WaitSeconds
Add-Result 'H02 the second launch ends within the bound' 'exit code 0 (handed to the running Hub)' `
    $(if ($null -eq $exit) { "still running after $WaitSeconds s" } else { "exit code $exit" }) ($exit -eq 0)

Start-Sleep -Seconds 2
$hubCount = (Get-HubProcesses).Count
Add-Result 'H02 still exactly one Hub process' '1' "$hubCount" ($hubCount -eq 1)

$window = Get-MainWindow $first.Id
Add-Result 'H02 the normal open restored the same Hub window' 'a window of the same pid, visible' `
    $(if ($window) { "hwnd=$($window.Handle) visible=$($window.Visible)" } else { '(none)' }) `
    ($null -ne $window -and $window.Visible)

$terminalsAfter = @(Get-TerminalDescendants $first.Id)
Add-Result 'H02 the normal open started no terminal' '0 terminal processes' `
    "$($terminalsAfter.Count)$(if ($terminalsAfter.Count) { ': ' + ($terminalsAfter -join ', ') })" `
    ($terminalsAfter.Count -eq $terminalsBefore.Count)
Write-Host ''

# --- The normal restore path: minimized and hidden ---------------

Write-Host '== The normal restore path: minimized and hidden ==' -ForegroundColor Cyan

# The two gestures differ in how "off the desktop" reads, because Windows means
# different things by them: a minimized window still reports IsWindowVisible =
# True (WS_VISIBLE is intact, the window is just collapsed to an icon), while a
# hidden one reports False. One predicate for both would be a check that is
# always red.
$gestures = @(
    [pscustomobject]@{
        Name        = 'minimized'
        Action      = 'ShowWindow(SW_MINIMIZE)'
        Concealed   = 'iconic=True'
        IsConcealed = { param($w) $w.Iconic }
        Conceal     = { param($h) [Hub.Win]::ShowWindow($h, $SW_MINIMIZE) | Out-Null }
    },
    [pscustomobject]@{
        Name        = 'closed (hidden to the tray)'
        Action      = 'PostMessage(WM_CLOSE)'
        Concealed   = 'visible=False'
        IsConcealed = { param($w) -not $w.Visible }
        Conceal     = { param($h) [Hub.Win]::PostMessage($h, $WM_CLOSE, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }
    }
)

foreach ($gesture in $gestures) {
    $window = Get-MainWindow $first.Id
    if (-not $window) {
        Add-Result "$($gesture.Name) then open again restores the window" 'a window exists' '(none)' $false
        continue
    }

    & $gesture.Conceal $window.Handle
    Start-Sleep -Milliseconds 1200
    $before = Get-MainWindow $first.Id
    $stillRunning = @(Get-HubProcesses).Count
    $concealed = $null -ne $before -and (& $gesture.IsConcealed $before)
    Add-Result "$($gesture.Name) takes the window off the desktop, the Hub keeps running" "$($gesture.Concealed), Hub still running" `
        "$(if ($before) { "visible=$($before.Visible) iconic=$($before.Iconic)" } else { '(none)' }), Hub processes=$stillRunning" `
        ($concealed -and $stillRunning -eq 1)
    Write-Note "$($gesture.Name): $($gesture.Action)"

    $again = Open-Hub $exe
    $exit = Wait-ForExit $again $WaitSeconds
    Start-Sleep -Milliseconds 1500
    $after = Get-MainWindow $first.Id

    $restored = $null -ne $after -and $after.Visible -and (-not $after.Iconic)
    Add-Result "$($gesture.Name) then open again restores the window" 'visible and not minimized, delivered with exit code 0' `
        "$(if ($after) { "visible=$($after.Visible) iconic=$($after.Iconic)" } else { '(none)' }) exit code=$exit" `
        ($restored -and $exit -eq 0)

    $hubCount = (Get-HubProcesses).Count
    Add-Result "$($gesture.Name) leaves exactly one Hub process" '1' "$hubCount" ($hubCount -eq 1)
}

Write-Host ''

# --- H02: the cold-start race ------------------------------------

Write-Host '== H02 cold-start race: two processes started almost together ==' -ForegroundColor Cyan

Stop-Started
Add-Result 'cleared before the race' '0' "$((Get-HubProcesses).Count)" ((Get-HubProcesses).Count -eq 0)

$share = Open-Hub $exe
Start-Sleep -Milliseconds $RaceDelayMs
$other = Open-Hub $exe

# The one started second has to find the mutex taken, hand its request over and
# end; the one started first survives.
$otherExit = Wait-ForExit $other $WaitSeconds
$shareExit = Wait-ForExit $share 3

$survivors = Get-HubProcesses
Add-Result 'exactly one Hub survives the race' '1' `
    "$($survivors.Count) (pid $(($survivors | ForEach-Object { $_.Id }) -join ', '))" ($survivors.Count -eq 1)
Add-Result 'one side handed over and ended' 'one exits 0, the other keeps running' `
    "first exit=$(if ($null -eq $shareExit) { 'still running' } else { $shareExit }), second exit=$(if ($null -eq $otherExit) { 'still running' } else { $otherExit })" `
    (($null -eq $shareExit) -xor ($null -eq $otherExit))

$winner = $survivors | Select-Object -First 1
if ($winner) {
    $window = Wait-ForWindow $winner.Id
    Add-Result 'the winner has a window' 'at least one titled top-level window' `
        $(if ($window) { "pid=$($winner.Id) hwnd=$($window.Handle)" } else { '(none)' }) ($null -ne $window)
    $hasConsole = Test-HasConsole $winner.Id
    Add-Result 'the winner owns no console' 'False' "$hasConsole" ($hasConsole -eq $false)
}

Write-Host ''
Write-Host 'Cleanup: ending the Hub processes this script started.' -ForegroundColor DarkGray
Stop-Started
Add-Result 'no Hub process left behind' '0' "$((Get-HubProcesses).Count)" ((Get-HubProcesses).Count -eq 0)

# --- Summary -----------------------------------------------------

Write-Host ''
Write-Host '== Summary ==' -ForegroundColor Cyan
$failed = @($results | Where-Object { $_.Result -eq 'FAIL' })
$results | Format-Table -AutoSize Check, Expected, Actual, Result | Out-String -Width 220 | Write-Host
Write-Host ("{0} checks: {1} PASS / {2} FAIL" -f $results.Count, ($results.Count - $failed.Count), $failed.Count) `
    -ForegroundColor $(if ($failed.Count -eq 0) { 'Green' } else { 'Red' })
Write-Host ''
Write-Host 'What this proves: process counts, exit codes, window visibility, whether a terminal' -ForegroundColor DarkGray
Write-Host 'came with it, and the entry PE subsystem and console.' -ForegroundColor DarkGray
Write-Host 'What it cannot prove: the tray gesture itself, whether managed sessions stay' -ForegroundColor DarkGray
Write-Host 'interactive after a restore, and icon appearance.' -ForegroundColor DarkGray
Write-Host 'Those are the manual steps in docs/SINGLE_INSTANCE_ACCEPTANCE.md.' -ForegroundColor DarkGray

if ($failed.Count -gt 0) { exit 1 } else { exit 0 }
