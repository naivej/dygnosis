$ErrorActionPreference = 'Stop'
$extensionRoot = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $extensionRoot '.test-data'
$runId = [Guid]::NewGuid().ToString()
$hostRoot = Join-Path $testRoot $runId
$workspaceRoot = Join-Path $hostRoot 'workspace'
New-Item -ItemType Directory -Force -Path $workspaceRoot | Out-Null
$vscodeExecutable = $env:DYGNOSIS_VSCODE_EXECUTABLE
if (-not $vscodeExecutable) {
    $vscodeExecutable = Join-Path $env:LOCALAPPDATA 'Programs/Microsoft VS Code/Code.exe'
}
if (-not (Test-Path -LiteralPath $vscodeExecutable)) { throw 'Set DYGNOSIS_VSCODE_EXECUTABLE to the minimum/current VS Code executable.' }
if (-not $env:DYGNOSIS_TEST_BINARY) { throw 'Set DYGNOSIS_TEST_BINARY to the matching built engine.' }
$hostUser = Join-Path $hostRoot 'profile/User'
New-Item -ItemType Directory -Force -Path $hostUser | Out-Null
@{ 'dynare.serverPath' = $env:DYGNOSIS_TEST_BINARY; 'extensions.autoUpdate' = $false; 'extensions.autoCheckUpdates' = $false } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $hostUser 'settings.json') -Encoding utf8
$env:DYGNOSIS_HOST_RESULT = Join-Path $hostRoot 'result.json'
$env:DYGNOSIS_HOST_RUN_ID = $runId
$arguments = @(
    ('"' + $workspaceRoot + '"'),
    ('--extensionDevelopmentPath="' + $extensionRoot + '"'),
    ('--extensionTestsPath="' + (Join-Path $PSScriptRoot 'host.cjs') + '"'),
    ('--user-data-dir="' + (Join-Path $hostRoot 'profile') + '"'),
    ('--extensions-dir="' + (Join-Path $hostRoot 'extensions') + '"'),
    '--skip-welcome', '--skip-release-notes', '--disable-workspace-trust', '--disable-gpu'
)
$process = Start-Process -FilePath $vscodeExecutable -ArgumentList $arguments -WindowStyle Hidden -PassThru -Wait
if (-not (Test-Path -LiteralPath $env:DYGNOSIS_HOST_RESULT)) { throw 'VS Code did not write a fresh extension test result.' }
$result = Get-Content -Raw -LiteralPath $env:DYGNOSIS_HOST_RESULT | ConvertFrom-Json
if ($result.runId -ne $runId -or $result.passed -ne $true) { throw 'VS Code extension test result is stale or failed.' }
if ($env:DYGNOSIS_EXPECTED_VSCODE -and $result.vscode -ne $env:DYGNOSIS_EXPECTED_VSCODE) { throw 'VS Code test host version differs from the expected version.' }
Get-Content -LiteralPath $env:DYGNOSIS_HOST_RESULT
if ($process.ExitCode -ne 0) { throw "VS Code extension tests exited $($process.ExitCode)." }
