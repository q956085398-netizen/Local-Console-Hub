param(
    [string]$ConfigRoot = (Join-Path ([Environment]::GetFolderPath('ApplicationData')) 'LocalConsoleHub')
)

$ErrorActionPreference = 'Stop'
$taskWorkspace = Split-Path -Parent $PSScriptRoot
$taskConfigPath = Join-Path $ConfigRoot 'config.yaml'
$taskCreated = $false

if (-not [IO.File]::Exists($taskConfigPath)) {
    # Stage the acceptance fixture, then install it without replacing a file
    # another process may have created while the fixture was being prepared.
    $taskFixture = Join-Path $taskWorkspace 'fixtures\verification-config.yaml'
    $taskBytes = [IO.File]::ReadAllBytes($taskFixture)
    [IO.Directory]::CreateDirectory($ConfigRoot) | Out-Null
    $taskTemporary = Join-Path $ConfigRoot ('verification-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::WriteAllBytes($taskTemporary, $taskBytes)
        try {
            [IO.File]::Move($taskTemporary, $taskConfigPath)
            $taskCreated = $true
        } catch {
            if (-not [IO.File]::Exists($taskConfigPath)) { throw }
        }
    } finally {
        if ([IO.File]::Exists($taskTemporary)) { [IO.File]::Delete($taskTemporary) }
    }
}

Write-Output ('Config ready: ' + $taskConfigPath)
if ($taskCreated) {
    Write-Output 'Created the acceptance-test config because no config existed in this launch environment.'
} else {
    Write-Output 'Existing config preserved.'
}
