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
$process = Start-Process -FilePath $launch.executable -ArgumentList $argumentList -WindowStyle Hidden -PassThru
if (-not $process.WaitForExit(120000)) {
    & taskkill.exe /PID $process.Id /T /F | Out-Null
    throw 'Installed VSIX host did not finish within 120 seconds.'
}
$process.Refresh()
if ($process.ExitCode -ne 0) { throw "VS Code package tests exited $($process.ExitCode)." }
