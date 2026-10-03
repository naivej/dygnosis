param([Parameter(Mandatory = $true)][string]$LaunchFile)
$ErrorActionPreference = 'Stop'
$launch = Get-Content -Raw -LiteralPath $LaunchFile | ConvertFrom-Json
if (-not [System.IO.Path]::IsPathRooted($launch.executable) -or
    -not (Test-Path -LiteralPath $launch.executable -PathType Leaf)) {
    throw 'The package test needs an absolute VS Code executable path.'
}
# The caller supplies only its own flags and Windows file paths, which cannot
# contain quotes. Quote each argument so spaces survive Start-Process parsing.
$argumentList = @($launch.args | ForEach-Object {
    if ($_ -isnot [string] -or $_.Contains('"') -or $_.EndsWith('\')) {
        throw 'Invalid package-host argument.'
    }
    '"' + $_ + '"'
})
$result = @{ timed_out = $false; cleanup_verified = $false }
$ownedHost = $null
try {
    Add-Type -Path (Join-Path $PSScriptRoot 'windows-host.cs')
    $ownedHost = [DygnosisOwnedHost]::new($launch.executable, ($argumentList -join ' '), $launch.stdout, $launch.stderr)
    $result.pid = $ownedHost.pid
    $result | ConvertTo-Json | Set-Content -LiteralPath $launch.lifecycle -Encoding utf8
    $ownedHost.Run($launch.timeoutMs)
    if ($ownedHost.timed_out) { throw 'Installed VSIX host did not finish within its timeout.' }
    if ($ownedHost.exit_code -ne 0) { throw "VS Code package tests exited $($ownedHost.exit_code)." }
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
    }
    $result | ConvertTo-Json | Set-Content -LiteralPath $launch.lifecycle -Encoding utf8
}
