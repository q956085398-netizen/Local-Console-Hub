# Point one of the user's own shortcuts at the Hub's new-terminal request (#63).
#
# The user's daily PowerShell entry is a shortcut they already use. This script
# rewrites *that* shortcut -- the one named on the command line and nothing
# else -- so that clicking it asks the Hub for a terminal instead of opening a
# terminal of its own. System shell executables, the global console host and
# every other shortcut are untouched: the Hub never becomes the handler for
# PowerShell, it only replaces the entry the user chose (spec #59 decision 17).
#
# Usage:
#   powershell -NoProfile -File scripts\install-powershell-shortcut.ps1 -Shortcut "C:\Users\me\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Windows PowerShell.lnk"
#   powershell -NoProfile -File scripts\install-powershell-shortcut.ps1 -Shortcut "..." -Hub "E:\Local Console Hub\local-console-hub.exe"
#   powershell -NoProfile -File scripts\install-powershell-shortcut.ps1 -Shortcut "..." -Directory "D:\Work"
#   powershell -NoProfile -File scripts\install-powershell-shortcut.ps1 -Shortcut "..." -Restore
#
# The directory the new terminal opens in is the shortcut's own "Start in",
# expanded and carried as one argument (`--directory`), so an entry that meant
# a directory still means it. Pass -Directory to name a different one.
#
# The original shortcut is copied to "<name>.lch-original.lnk" beside itself
# before anything is written, and -Restore puts it back. Nothing here is
# destructive without a way back.

[CmdletBinding()]
param(
    # The shortcut to replace. This is the selection: the script modifies this
    # file and no other.
    [Parameter(Mandatory = $true)]
    [string] $Shortcut,

    # The Hub executable the shortcut should point at. Defaults to the
    # installed app, then to this working tree's release build.
    [string] $Hub,

    # The directory a terminal from this entry opens in. Defaults to the
    # shortcut's current "Start in".
    [string] $Directory,

    # Replace a shortcut whose current target is not a terminal/shell. Off by
    # default: the check is what stops a mistyped path from being repurposed.
    [switch] $Force,

    # Put the saved original back and leave.
    [switch] $Restore
)

$ErrorActionPreference = 'Stop'

$BACKUP_SUFFIX = '.lch-original.lnk'

# The target names that count as "this shortcut opens a terminal".
$TERMINAL_TARGETS = '^(powershell|pwsh|cmd|WindowsTerminal|wt)\.exe$'

function Resolve-Hub {
    param([string] $Requested)

    if ($Requested) {
        if (-not (Test-Path -LiteralPath $Requested -PathType Leaf)) {
            throw "The Hub executable does not exist: $Requested"
        }
        return (Get-Item -LiteralPath $Requested).FullName
    }

    # The installed app first: that is the copy the user's daily entry should
    # depend on, and depending on a build output is what this script exists to
    # stop (spec #59 decision 17: the daily entry must not lean on a development
    # build).
    $installed = Join-Path $env:LOCALAPPDATA 'Local Console Hub\local-console-hub.exe'
    if (Test-Path -LiteralPath $installed -PathType Leaf) {
        return (Get-Item -LiteralPath $installed).FullName
    }

    $repo = Split-Path -Parent $PSScriptRoot
    $release = Join-Path $repo 'src-tauri\target\release\local-console-hub.exe'
    if (Test-Path -LiteralPath $release -PathType Leaf) {
        Write-Host 'Note: the app does not appear to be installed, so the working tree release build is used.' -ForegroundColor Yellow
        return (Get-Item -LiteralPath $release).FullName
    }

    throw @"
No Hub executable was found. Install the app, or pass -Hub with the path to
local-console-hub.exe. Looked for:
  $installed
  $release
"@
}

# The PE subsystem of an executable: 2 = WINDOWS_GUI, 3 = WINDOWS_CUI.
#
# A development build is a console program, and an entry pointing at one puts a
# console window on the taskbar for as long as the Hub runs -- the very thing
# this entry is supposed to stop producing (docs/SINGLE_INSTANCE_ACCEPTANCE.md
# section 1).
function Get-Subsystem([string] $Path) {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
    $optionalHeader = $peOffset + 24
    $magic = [BitConverter]::ToUInt16($bytes, $optionalHeader)
    return [BitConverter]::ToUInt16($bytes, $optionalHeader + $(if ($magic -eq 0x20b) { 68 } else { 66 }))
}

# One argument, quoted the way Windows re-splits a command line.
#
# Windows turns a shortcut's arguments back into argv with the MSVCRT rules, so
# a path with a space has to be quoted, a quote inside it has to be escaped, and
# a trailing backslash has to be doubled or it would escape the closing quote.
# A path with no space or quote is left bare, which is also the case where a
# trailing backslash is harmless.
function ConvertTo-Argument([string] $Value) {
    if ($Value -eq '') { return '""' }
    if ($Value -notmatch '[\s"]') { return $Value }

    $builder = [System.Text.StringBuilder]::new()
    [void]$builder.Append('"')
    $backslashes = 0
    foreach ($character in $Value.ToCharArray()) {
        if ($character -eq '\') { $backslashes++; continue }
        if ($character -eq '"') {
            [void]$builder.Append('\' * (2 * $backslashes + 1))
            [void]$builder.Append('"')
            $backslashes = 0
            continue
        }
        if ($backslashes -gt 0) {
            [void]$builder.Append('\' * $backslashes)
            $backslashes = 0
        }
        [void]$builder.Append($character)
    }
    if ($backslashes -gt 0) { [void]$builder.Append('\' * (2 * $backslashes)) }
    [void]$builder.Append('"')
    return $builder.ToString()
}

if (-not (Test-Path -LiteralPath $Shortcut -PathType Leaf)) {
    Write-Host "The shortcut does not exist: $Shortcut" -ForegroundColor Red
    exit 2
}
$entry = Get-Item -LiteralPath $Shortcut
if ($entry.Extension -ne '.lnk') {
    Write-Host "Not a shortcut (.lnk): $($entry.FullName)" -ForegroundColor Red
    exit 2
}

$backup = Join-Path $entry.DirectoryName ($entry.BaseName + $BACKUP_SUFFIX)
$shell = New-Object -ComObject WScript.Shell

if ($Restore) {
    if (-not (Test-Path -LiteralPath $backup -PathType Leaf)) {
        Write-Host "No saved original beside that shortcut: $backup" -ForegroundColor Red
        exit 2
    }
    Copy-Item -LiteralPath $backup -Destination $entry.FullName -Force
    $restored = $shell.CreateShortcut($entry.FullName)
    Write-Host ''
    Write-Host 'Restored' -ForegroundColor Green
    Write-Host "  shortcut : $($entry.FullName)"
    Write-Host "  target   : $($restored.TargetPath)"
    Write-Host "  saved    : $backup (left in place)"
    Write-Host ''
    exit 0
}

$link = $shell.CreateShortcut($entry.FullName)
$previousTarget = $link.TargetPath
$previousArguments = $link.Arguments
$previousWorkingDirectory = $link.WorkingDirectory

if (-not (Test-Path -LiteralPath $previousTarget -PathType Leaf)) {
    Write-Host "The shortcut's current target does not exist: $previousTarget" -ForegroundColor Red
    Write-Host 'Fix or recreate the shortcut first; this script will not repurpose a broken one.' -ForegroundColor Red
    exit 2
}
$targetName = Split-Path -Leaf $previousTarget
if ($targetName -notmatch $TERMINAL_TARGETS -and -not $Force) {
    Write-Host "That shortcut does not open a terminal: it points at `"$targetName`"." -ForegroundColor Red
    Write-Host 'Pass -Force only if you really mean to repurpose this entry.' -ForegroundColor Red
    exit 2
}

$hubPath = Resolve-Hub -Requested $Hub
if ((Get-Subsystem $hubPath) -ne 2) {
    Write-Host "Warning: that Hub is a console program (a development build), so it will bring a" -ForegroundColor Yellow
    Write-Host 'console window with it. Install the app, or pass -Hub with the installed exe.' -ForegroundColor Yellow
}

# What the entry meant by "start here": its own "Start in", with environment
# variables expanded, because the stock PowerShell shortcut spells it
# `%HOMEDRIVE%%HOMEPATH%` rather than a path.
$wanted = $Directory
if (-not $wanted) {
    $wanted = [Environment]::ExpandEnvironmentVariables($previousWorkingDirectory)
}
if ($wanted) {
    if (-not (Test-Path -LiteralPath $wanted -PathType Container)) {
        Write-Host "The working directory does not exist: $wanted" -ForegroundColor Red
        Write-Host 'Pass -Directory with the directory this entry should open in.' -ForegroundColor Red
        Write-Host 'Nothing was written: an entry that names a directory it cannot open would' -ForegroundColor Red
        Write-Host 'fail on every click.' -ForegroundColor Red
        exit 2
    }
    $wanted = (Get-Item -LiteralPath $wanted).FullName
}

$arguments = '--new-terminal'
if ($wanted) {
    $arguments += ' --directory ' + (ConvertTo-Argument $wanted)
}

if (-not (Test-Path -LiteralPath $backup -PathType Leaf)) {
    Copy-Item -LiteralPath $entry.FullName -Destination $backup -Force
}

$link.TargetPath = $hubPath
$link.Arguments = $arguments
# Kept in step with the argument, so the shortcut's properties page still says
# where the terminal opens. The Hub itself never reads the process's own
# directory: the argument is what decides (spec #59 decision 5).
if ($wanted) { $link.WorkingDirectory = $wanted }
$link.Save()

Write-Host ''
Write-Host 'Installed' -ForegroundColor Green
Write-Host "  shortcut : $($entry.FullName)"
Write-Host "  was      : `"$previousTarget`" $previousArguments"
Write-Host "  now      : `"$hubPath`" $arguments"
Write-Host "  starts in: $(if ($wanted) { $wanted } else { '(unchanged; the terminal opens in your home directory)' })"
Write-Host "  saved    : $backup"
Write-Host ''
Write-Host 'This changed one shortcut. Shell executables, the console host and every other' -ForegroundColor DarkGray
Write-Host 'entry are untouched; System, IDEs and agents keep opening the PowerShell they always did.' -ForegroundColor DarkGray
Write-Host "Clicking the shortcut now opens a terminal inside the Hub, starting it if it is not running." -ForegroundColor DarkGray
Write-Host "Undo with: -Restore (or copy the saved file back over the shortcut)." -ForegroundColor DarkGray
Write-Host ''
exit 0
