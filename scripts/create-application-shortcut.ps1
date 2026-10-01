# Create one explicit entry for a saved Hub configuration (#89).
# Example: -Shortcut 'D:\Entries\ComfyUI.lnk' -Hub 'E:\Local Console Hub\local-console-hub.exe' -ApplicationId comfyui
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $Shortcut,
    [Parameter(Mandatory = $true)][string] $Hub,
    [Parameter(Mandatory = $true)][string] $ApplicationId
)
$ErrorActionPreference = 'Stop'

# Same stable config identity accepted by Hub; never a command to execute.
if ($ApplicationId -cnotmatch '^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$') {
    throw 'ApplicationId must be a saved config id: 1-64 ASCII letters, digits, underscores or hyphens, starting with a letter or digit.'
}
$exe = Get-Item -LiteralPath $Hub
if ($exe.PSIsContainer -or $exe.Extension -ne '.exe') { throw 'Hub must name an executable.' }
$destination = [IO.Path]::GetFullPath($Shortcut)
if ([IO.Path]::GetExtension($destination) -ne '.lnk') { throw 'Shortcut must end in .lnk.' }
if (Test-Path -LiteralPath $destination) { throw 'The shortcut already exists. Choose a new path to preserve the existing entry.' }
if (-not (Test-Path -LiteralPath ([IO.Path]::GetDirectoryName($destination)) -PathType Container)) {
    throw 'The shortcut directory must already exist.'
}
$shell = New-Object -ComObject WScript.Shell
$link = $shell.CreateShortcut($destination)
$link.TargetPath = $exe.FullName
$link.Arguments = '--open-app ' + $ApplicationId
$link.WorkingDirectory = $exe.DirectoryName
$link.IconLocation = $exe.FullName + ',0'
$link.Description = 'Open saved Hub application: ' + $ApplicationId
$link.Save()
Write-Output "Created: $destination"
