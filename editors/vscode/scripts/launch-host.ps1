param(
    [Parameter(Mandatory = $true)][string]$LaunchFile,
    [Parameter(Mandatory = $true)][string]$Lifecycle
)
$ErrorActionPreference = 'Stop'
$result = @{
    stage = 'script_entered'; host_ready = $false
    timed_out = $false; cleanup_verified = $false
    launcher_pid = $PID; powershell_version = $PSVersionTable.PSVersion.ToString()
    powershell_home = $PSHOME
}
function Write-Lifecycle {
    $temporary = $Lifecycle + '.tmp'
    [System.IO.File]::WriteAllText($temporary, ($result | ConvertTo-Json), [System.Text.UTF8Encoding]::new($false))
    if ([System.IO.File]::Exists($Lifecycle)) {
        [System.IO.File]::Replace($temporary, $Lifecycle, [NullString]::Value)
    } else {
        [System.IO.File]::Move($temporary, $Lifecycle)
    }
}
$ownedHost = $null
try {
    Write-Lifecycle
    $result.stage = 'reading_launch'
    Write-Lifecycle
    $launch = Get-Content -Raw -LiteralPath $LaunchFile | ConvertFrom-Json
    $result.stage = 'validating'
    Write-Lifecycle
    if (-not [System.IO.Path]::IsPathRooted($launch.executable) -or
        -not (Test-Path -LiteralPath $launch.executable -PathType Leaf)) {
        throw 'The package test needs an absolute VS Code executable path.'
    }
    # The caller supplies its own flags and file paths. Keep spaces intact.
    $argumentList = @($launch.args | ForEach-Object {
        if ($_ -isnot [string] -or $_.Contains('"') -or $_.EndsWith('\')) {
            throw 'Invalid package-host argument.'
        }
        '"' + $_ + '"'
    })
    $result.stage = 'compiling'
    Write-Lifecycle
    Add-Type -Path (Join-Path $PSScriptRoot 'windows-host.cs')
    $result.stage = 'creating'
    Write-Lifecycle
    $ownedHost = [DygnosisOwnedHost]::new($launch.executable, ($argumentList -join ' '), $launch.stdout, $launch.stderr)
    $result.pid = $ownedHost.pid
    $result.stage = 'owned'
    $result.host_ready = $true
    Write-Lifecycle
    $ownedHost.Run($launch.timeoutMs)
    if ($ownedHost.timed_out) { throw 'Installed VSIX host did not finish within its timeout.' }
    if ($ownedHost.exit_code -ne 0) { throw "VS Code package tests exited $($ownedHost.exit_code)." }
    $result.stage = 'complete'
} catch {
    $result.error = $_.Exception.Message
    throw
} finally {
    if ($ownedHost) {
        $result.timed_out = $ownedHost.timed_out
        $result.exit_code = $ownedHost.exit_code
        $result.cleanup_verified = $ownedHost.cleanup_verified
        $result.cleanup_error = $ownedHost.cleanup_error
        # Disposal closes the job and native stream handles before evidence copy.
        $ownedHost.Dispose()
    } elseif ($result.stage -ne 'creating') {
        # No native constructor ran, so there is no host tree to terminate.
        $result.cleanup_verified = $true
    }
    Write-Lifecycle
}
